//! Server **lifecycle** primitives (`docs/04` §4.3.1, HS2-59): the machine-local instance
//! registry, a per-store index-writer lock, discovery, and stop — the pieces that let a
//! client find (and not collide with) a running local server, and let the server outlive
//! whatever launched it.
//!
//! - **Instance file** — `${HOTSHEET_HOME}/instances/<project-id>.json` written on start and
//!   removed on graceful shutdown; [`find_instance`] returns it only if the recorded pid is
//!   still alive (a crashed server leaves a stale file that reads as "none"). Under the
//!   multi-store topology (HS2-87, topology A) the **one** machine server writes one such
//!   file per hosted project, all pointing at itself — so "who serves project X?" resolves
//!   to that single server for every project it hosts.
//! - **Index-writer lock** — a per-store lock so a **second** server on the same store
//!   refuses instead of double-writing the disposable index (join-don't-collide). A stale
//!   lock from a dead server is reclaimed.
//! - **Stop** — [`stop_instance`] signals a running server to shut down (explicit shutdown
//!   only, never implicit on a client closing).
//!
//! Deferred to the client work (HS2-4072GM): discovery-driven **auto-start** of a detached
//! server, and **supervise** (restart-on-crash / health) — the client owns those, and no
//! client exists yet.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A running server's coordinates — written on start, removed on graceful shutdown.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstanceInfo {
    pub pid: u32,
    /// The base URL a client attaches to, e.g. `http://127.0.0.1:8787`.
    pub url: String,
    pub secret: String,
    pub store_path: String,
    pub index_path: String,
    pub started_at: String,
}

/// A stable per-store id (hash of the canonical store path) — the same key the index uses.
fn project_id(store_path: &Path) -> String {
    let root = store_path
        .canonicalize()
        .unwrap_or_else(|_| store_path.to_path_buf());
    hotsheet_index::hash_bytes(root.to_string_lossy().as_bytes())[..16].to_string()
}

/// `${HOTSHEET_HOME:-~/.hotsheet2}/instances` — machine-local, disposable (NOT `~/.hotsheet`,
/// which a separately-installed Hot Sheet 1 owns; HS2-104).
fn machine_instances_dir() -> PathBuf {
    hotsheet_plugins::hotsheet_home().join("instances")
}

/// The directory that holds instance files and index-writer locks.
///
/// The registry resolves its directory **once**, when constructed, so a caller that must
/// not depend on process-global state (a test, or a client given an explicit home) can
/// inject it with [`InstanceRegistry::at`]. Production code uses
/// [`InstanceRegistry::machine`] (or the free-function wrappers below), which reads
/// `${HOTSHEET_HOME}` at construction time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceRegistry {
    instances_dir: PathBuf,
}

impl InstanceRegistry {
    /// The machine registry: `${HOTSHEET_HOME:-~/.hotsheet2}/instances`, resolved now.
    pub fn machine() -> Self {
        Self::at(machine_instances_dir())
    }

    /// A registry rooted at an explicit directory (created lazily on first write).
    pub fn at(instances_dir: impl Into<PathBuf>) -> Self {
        Self {
            instances_dir: instances_dir.into(),
        }
    }

    /// The directory this registry reads and writes.
    pub fn dir(&self) -> &Path {
        &self.instances_dir
    }

    /// The instance file path for a store.
    pub fn instance_path(&self, store_path: &Path) -> PathBuf {
        self.instances_dir
            .join(format!("{}.json", project_id(store_path)))
    }

    fn lock_path(&self, store_path: &Path) -> PathBuf {
        self.instances_dir
            .join(format!("{}.lock", project_id(store_path)))
    }

    /// Find a **live** server serving `store_path`, if any. A stale file left by a crashed
    /// server (its pid no longer alive) returns `None`.
    pub fn find_instance(&self, store_path: &Path) -> Option<InstanceInfo> {
        let text = std::fs::read_to_string(self.instance_path(store_path)).ok()?;
        let info: InstanceInfo = serde_json::from_str(&text).ok()?;
        pid_alive(info.pid).then_some(info)
    }

    /// Write the instance file for a starting server; the returned guard removes it on
    /// drop (so a graceful shutdown leaves no stale file).
    pub fn register_instance(
        &self,
        info: &InstanceInfo,
        store_path: &Path,
    ) -> std::io::Result<InstanceGuard> {
        std::fs::create_dir_all(&self.instances_dir)?;
        let path = self.instance_path(store_path);
        std::fs::write(&path, serde_json::to_string_pretty(info)? + "\n")?;
        // Unlike checkout/store ids, this file contains an actual bearer credential.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(InstanceGuard {
            path,
            registration: info.clone(),
        })
    }

    /// Stop the server serving `store_path` (SIGTERM → graceful shutdown). Returns whether
    /// a live server was found and signalled.
    pub fn stop_instance(&self, store_path: &Path) -> bool {
        match self.find_instance(store_path) {
            Some(info) => signal(info.pid, "TERM"),
            None => false,
        }
    }

    /// Whether a **live** server currently holds this store's index-writer lock. A stale
    /// lock (its holder pid no longer alive) reads as not-locked, matching what
    /// [`InstanceRegistry::acquire_writer_lock`] would reclaim.
    pub fn is_writer_locked(&self, store_path: &Path) -> bool {
        std::fs::read_to_string(self.lock_path(store_path))
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok())
            .is_some_and(pid_alive)
    }

    /// Take the exclusive index-writer lock for a store. Fails with [`LockError::Held`]
    /// when a **live** server already holds it (the second server should attach, not
    /// duplicate); a stale lock left by a dead server is reclaimed.
    pub fn acquire_writer_lock(&self, store_path: &Path) -> Result<WriterLock, LockError> {
        std::fs::create_dir_all(&self.instances_dir)?;
        let path = self.lock_path(store_path);
        let me = std::process::id();

        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(_) => {
                std::fs::write(&path, me.to_string())?;
                Ok(WriterLock { path })
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let holder = std::fs::read_to_string(&path)
                    .ok()
                    .and_then(|s| s.trim().parse::<u32>().ok());
                match holder {
                    Some(pid) if pid != me && pid_alive(pid) => Err(LockError::Held(pid)),
                    // Stale (dead holder) or our own — reclaim by overwriting.
                    _ => {
                        std::fs::write(&path, me.to_string())?;
                        Ok(WriterLock { path })
                    }
                }
            }
            Err(e) => Err(LockError::Io(e)),
        }
    }
}

// ---- machine-registry convenience wrappers -----------------------------------------
//
// Each wrapper resolves `${HOTSHEET_HOME}` at call time via `InstanceRegistry::machine()`.
// Code that needs a stable directory across several calls (or a test) should hold one
// `InstanceRegistry` instead.

/// The instance file path for a store under the machine registry.
pub fn instance_path(store_path: &Path) -> PathBuf {
    InstanceRegistry::machine().instance_path(store_path)
}

/// [`InstanceRegistry::find_instance`] on the machine registry.
pub fn find_instance(store_path: &Path) -> Option<InstanceInfo> {
    InstanceRegistry::machine().find_instance(store_path)
}

/// [`InstanceRegistry::register_instance`] on the machine registry.
pub fn register_instance(info: &InstanceInfo, store_path: &Path) -> std::io::Result<InstanceGuard> {
    InstanceRegistry::machine().register_instance(info, store_path)
}

/// [`InstanceRegistry::stop_instance`] on the machine registry.
pub fn stop_instance(store_path: &Path) -> bool {
    InstanceRegistry::machine().stop_instance(store_path)
}

/// Removes the instance file when the server stops.
pub struct InstanceGuard {
    path: PathBuf,
    registration: InstanceInfo,
}

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        // An upgrading replacement may publish this same store before the old process has
        // finished dropping its state. Never let the old guard erase the new registration.
        let still_ours = std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|text| serde_json::from_str::<InstanceInfo>(&text).ok())
            .is_some_and(|current| current == self.registration);
        if still_ours {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Whether a process is alive (`kill -0`).
pub fn pid_alive(pid: u32) -> bool {
    signal(pid, "0")
}

#[cfg(unix)]
fn signal(pid: u32, sig: &str) -> bool {
    // Guard against `kill` targeting a process GROUP / everything: a stringified pid that
    // wraps past `pid_t` (i32) — e.g. a garbage lock file holding `u32::MAX` — parses to a
    // negative number, and `kill -0 -1` signals every reachable process and *succeeds*,
    // wrongly reading as "alive" (so a corrupt lock would block forever). Only a real,
    // positive, single-process pid is a valid target.
    if pid == 0 || pid > i32::MAX as u32 {
        return false;
    }
    std::process::Command::new("kill")
        .arg(format!("-{sig}"))
        .arg(pid.to_string())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn signal(_pid: u32, _sig: &str) -> bool {
    false
}

// ---- index-writer lock (join-don't-collide) --------------------------------------

/// Why a writer lock couldn't be taken.
#[derive(Debug, thiserror::Error)]
pub enum LockError {
    #[error("another server (pid {0}) is already serving this store")]
    Held(u32),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Holds the per-store index-writer lock; releases it on drop.
#[derive(Debug)]
pub struct WriterLock {
    path: PathBuf,
}

impl Drop for WriterLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// [`InstanceRegistry::is_writer_locked`] on the machine registry.
pub fn is_writer_locked(store_path: &Path) -> bool {
    InstanceRegistry::machine().is_writer_locked(store_path)
}

/// [`InstanceRegistry::acquire_writer_lock`] on the machine registry.
pub fn acquire_writer_lock(store_path: &Path) -> Result<WriterLock, LockError> {
    InstanceRegistry::machine().acquire_writer_lock(store_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A registry under a private temp dir. No process-global state is touched: under
    /// `cargo test` (libtest) every unit test in this crate shares one process, so a
    /// fixture that set `HOTSHEET_HOME` re-pointed *every* concurrently running test at a
    /// different — possibly already-deleted — directory (HS2-NPFAS1).
    fn isolated_registry() -> (tempfile::TempDir, InstanceRegistry) {
        let home = tempfile::tempdir().unwrap();
        let registry = InstanceRegistry::at(home.path().join("instances"));
        (home, registry)
    }

    fn info(pid: u32, store: &Path) -> InstanceInfo {
        InstanceInfo {
            pid,
            url: "http://127.0.0.1:8787".into(),
            secret: "s".into(),
            store_path: store.display().to_string(),
            index_path: "/idx".into(),
            started_at: "2026-01-01T00:00:00Z".into(),
        }
    }

    #[test]
    fn find_instance_ignores_a_stale_file() {
        let (_home, registry) = isolated_registry();
        let store = tempfile::tempdir().unwrap();
        // u32::MAX is never a real pid, so this registration reads as a dead server.
        registry
            .register_instance(&info(u32::MAX, store.path()), store.path())
            .unwrap();
        assert!(
            registry.find_instance(store.path()).is_none(),
            "a dead-pid instance file reads as no running server"
        );
    }

    #[test]
    fn find_instance_returns_a_live_server() {
        let (_home, registry) = isolated_registry();
        let store = tempfile::tempdir().unwrap();
        let me = std::process::id(); // this test process is certainly alive
        let _g = registry
            .register_instance(&info(me, store.path()), store.path())
            .unwrap();
        let found = registry
            .find_instance(store.path())
            .expect("live instance found");
        assert_eq!(found.pid, me);
    }

    #[test]
    fn instance_guard_removes_the_file_on_drop() {
        let (_home, registry) = isolated_registry();
        let store = tempfile::tempdir().unwrap();
        {
            let _g = registry
                .register_instance(&info(std::process::id(), store.path()), store.path())
                .unwrap();
            assert!(registry.instance_path(store.path()).exists());
        }
        assert!(
            !registry.instance_path(store.path()).exists(),
            "guard cleaned up on drop"
        );
    }

    #[test]
    fn old_instance_guard_preserves_a_replacement_registration() {
        let (_home, registry) = isolated_registry();
        let store = tempfile::tempdir().unwrap();
        let old = info(std::process::id(), store.path());
        let old_guard = registry.register_instance(&old, store.path()).unwrap();
        let mut replacement = old.clone();
        replacement.started_at = "2026-01-02T00:00:00Z".into();
        let replacement_guard = registry
            .register_instance(&replacement, store.path())
            .unwrap();

        drop(old_guard);
        assert_eq!(registry.find_instance(store.path()), Some(replacement));
        drop(replacement_guard);
        assert!(!registry.instance_path(store.path()).exists());
    }

    #[test]
    fn stop_instance_signals_only_a_live_registration() {
        let (_home, registry) = isolated_registry();
        let store = tempfile::tempdir().unwrap();
        assert!(!registry.stop_instance(store.path()), "nothing registered");

        // A live foreign holder we can safely TERM.
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let _g = registry
            .register_instance(&info(child.id(), store.path()), store.path())
            .unwrap();
        assert!(registry.stop_instance(store.path()), "live child signalled");
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            assert!(
                child.wait().unwrap().signal().is_some(),
                "child died by signal"
            );
        }
        #[cfg(not(unix))]
        child.wait().unwrap();
        // Dead now: the same file reads as no live server.
        assert!(!registry.stop_instance(store.path()));
    }

    #[test]
    fn writer_lock_blocks_a_second_live_holder_but_reclaims_a_stale_one() {
        let (_home, registry) = isolated_registry();
        let store = tempfile::tempdir().unwrap();
        let lock_path = registry.lock_path(store.path());

        // A live foreign holder (a child process we can signal) blocks a second acquire.
        std::fs::create_dir_all(registry.dir()).unwrap();
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let live_pid = child.id();
        std::fs::write(&lock_path, live_pid.to_string()).unwrap();
        assert!(registry.is_writer_locked(store.path()));
        match registry.acquire_writer_lock(store.path()) {
            Err(LockError::Held(p)) if p == live_pid => {}
            other => panic!("expected Held({live_pid}), got {other:?}"),
        }
        child.kill().ok();
        child.wait().ok();

        // A stale holder (dead pid) is reclaimed.
        std::fs::write(&lock_path, u32::MAX.to_string()).unwrap();
        assert!(!registry.is_writer_locked(store.path()));
        let lock = registry
            .acquire_writer_lock(store.path())
            .expect("stale lock reclaimed");
        assert!(
            registry.is_writer_locked(store.path()),
            "our own live pid holds it"
        );
        drop(lock);
        assert!(!lock_path.exists(), "lock released on drop");
    }

    #[test]
    fn out_of_range_pids_never_read_as_alive() {
        // Regression: `u32::MAX` stringified and handed to `kill` wraps to -1 on Linux,
        // signalling every process and reporting "alive" — so a corrupt lock file would
        // block forever. Pid 0 (the caller's own group) is the same hazard. Both must be
        // treated as dead so the stale lock is reclaimable.
        assert!(!pid_alive(u32::MAX), "u32::MAX pid is not a live process");
        assert!(!pid_alive(0), "pid 0 is not a single live process");
        // Sanity: this test's own process IS alive.
        assert!(pid_alive(std::process::id()));
    }

    /// Regression for HS2-NPFAS1: pins the exact interleaving that made
    /// `find_instance_returns_a_live_server` fail with `NotFound` under `cargo test`.
    ///
    /// Thread A registers an instance; between its write and its read, thread B sets up
    /// **and tears down** its own isolated home (what a concurrently finishing sibling test
    /// does). With the old `HOTSHEET_HOME` fixture, B's `set_var` re-pointed A's directory
    /// lookups at B's home, whose TempDir was then deleted — so A's `register`/`find`/guard
    /// drop hit a missing directory. With an injected registry each thread keeps its own
    /// directory, so the sequence is deterministic and must pass.
    #[test]
    fn concurrent_isolated_homes_do_not_interfere() {
        use std::sync::{Arc, Barrier};

        let store = tempfile::tempdir().unwrap();
        let store_path = store.path().to_path_buf();
        // Two rendezvous points: (1) B starts after A registered, (2) A resumes after B's
        // home is gone.
        let after_register = Arc::new(Barrier::new(2));
        let after_teardown = Arc::new(Barrier::new(2));

        let a = {
            let store_path = store_path.clone();
            let after_register = after_register.clone();
            let after_teardown = after_teardown.clone();
            std::thread::spawn(move || {
                let (home, registry) = isolated_registry();
                let me = std::process::id();
                let guard = registry
                    .register_instance(&info(me, &store_path), &store_path)
                    .unwrap();
                after_register.wait();
                after_teardown.wait();
                let found = registry
                    .find_instance(&store_path)
                    .expect("A's live instance is still found after B tore down its home");
                assert_eq!(found.pid, me);
                drop(guard);
                assert!(
                    !registry.instance_path(&store_path).exists(),
                    "A's guard removed A's file"
                );
                drop(home);
            })
        };
        let b = {
            let after_register = after_register.clone();
            let after_teardown = after_teardown.clone();
            std::thread::spawn(move || {
                after_register.wait();
                let (home, registry) = isolated_registry();
                let g = registry
                    .register_instance(&info(std::process::id(), &store_path), &store_path)
                    .unwrap();
                drop(g);
                drop(home); // B's TempDir is deleted while A is mid-test.
                after_teardown.wait();
            })
        };
        a.join().expect("thread A");
        b.join().expect("thread B");
    }

    /// Adversarial: many threads run the full register → find → drop sequence at once on
    /// the same store path, each with its own registry. None may observe another's files.
    #[test]
    fn many_parallel_registries_stay_independent() {
        let store = tempfile::tempdir().unwrap();
        let store_path = store.path().to_path_buf();
        let threads: Vec<_> = (0..16)
            .map(|i| {
                let store_path = store_path.clone();
                std::thread::spawn(move || {
                    for _ in 0..8 {
                        let (_home, registry) = isolated_registry();
                        let mut mine = info(std::process::id(), &store_path);
                        mine.url = format!("http://127.0.0.1:{}", 9000 + i);
                        let guard = registry.register_instance(&mine, &store_path).unwrap();
                        let lock = registry.acquire_writer_lock(&store_path).unwrap();
                        assert_eq!(registry.find_instance(&store_path), Some(mine));
                        assert!(registry.is_writer_locked(&store_path));
                        drop(lock);
                        drop(guard);
                        assert!(registry.find_instance(&store_path).is_none());
                        assert!(!registry.is_writer_locked(&store_path));
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().expect("worker thread");
        }
    }
}
