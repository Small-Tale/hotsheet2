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
fn instances_dir() -> PathBuf {
    hotsheet_plugins::hotsheet_home().join("instances")
}

/// The instance file path for a store.
pub fn instance_path(store_path: &Path) -> PathBuf {
    instances_dir().join(format!("{}.json", project_id(store_path)))
}

/// Find a **live** server serving `store_path`, if any. A stale file left by a crashed
/// server (its pid no longer alive) returns `None`.
pub fn find_instance(store_path: &Path) -> Option<InstanceInfo> {
    let text = std::fs::read_to_string(instance_path(store_path)).ok()?;
    let info: InstanceInfo = serde_json::from_str(&text).ok()?;
    pid_alive(info.pid).then_some(info)
}

/// Write the instance file for a starting server; the returned guard removes it on drop
/// (so a graceful shutdown leaves no stale file).
pub fn register_instance(info: &InstanceInfo, store_path: &Path) -> std::io::Result<InstanceGuard> {
    std::fs::create_dir_all(instances_dir())?;
    let path = instance_path(store_path);
    std::fs::write(&path, serde_json::to_string_pretty(info)? + "\n")?;
    // Unlike checkout/store ids, this file contains an actual bearer credential.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(InstanceGuard { path })
}

/// Removes the instance file when the server stops.
pub struct InstanceGuard {
    path: PathBuf,
}

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Stop the server serving `store_path` (SIGTERM → graceful shutdown). Returns whether a
/// live server was found and signalled.
pub fn stop_instance(store_path: &Path) -> bool {
    match find_instance(store_path) {
        Some(info) => signal(info.pid, "TERM"),
        None => false,
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

/// Whether a **live** server currently holds this store's index-writer lock. A stale lock
/// (its holder pid no longer alive) reads as not-locked, matching what
/// [`acquire_writer_lock`] would reclaim.
pub fn is_writer_locked(store_path: &Path) -> bool {
    let path = instances_dir().join(format!("{}.lock", project_id(store_path)));
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .is_some_and(pid_alive)
}

/// Take the exclusive index-writer lock for a store. Fails with [`LockError::Held`] when a
/// **live** server already holds it (the second server should attach, not duplicate); a
/// stale lock left by a dead server is reclaimed.
pub fn acquire_writer_lock(store_path: &Path) -> Result<WriterLock, LockError> {
    std::fs::create_dir_all(instances_dir())?;
    let path = instances_dir().join(format!("{}.lock", project_id(store_path)));
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Point HOTSHEET_HOME at a temp dir so tests never touch the real machine home.
    fn isolated_home() -> tempfile::TempDir {
        let home = tempfile::tempdir().unwrap();
        // SAFETY: tests here run single-threaded per-process under nextest.
        unsafe { std::env::set_var("HOTSHEET_HOME", home.path()) };
        home
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
        let _home = isolated_home();
        let store = tempfile::tempdir().unwrap();
        // A dead pid (our own pid + a big offset is very unlikely to be alive; use pid 1? it
        // IS alive. Use u32::MAX which is never a real pid).
        register_instance(&info(u32::MAX, store.path()), store.path()).unwrap();
        assert!(
            find_instance(store.path()).is_none(),
            "a dead-pid instance file reads as no running server"
        );
    }

    #[test]
    fn find_instance_returns_a_live_server() {
        let _home = isolated_home();
        let store = tempfile::tempdir().unwrap();
        let me = std::process::id(); // this test process is certainly alive
        let _g = register_instance(&info(me, store.path()), store.path()).unwrap();
        let found = find_instance(store.path()).expect("live instance found");
        assert_eq!(found.pid, me);
    }

    #[test]
    fn instance_guard_removes_the_file_on_drop() {
        let _home = isolated_home();
        let store = tempfile::tempdir().unwrap();
        {
            let _g =
                register_instance(&info(std::process::id(), store.path()), store.path()).unwrap();
            assert!(instance_path(store.path()).exists());
        }
        assert!(
            !instance_path(store.path()).exists(),
            "guard cleaned up on drop"
        );
    }

    #[test]
    fn writer_lock_blocks_a_second_live_holder_but_reclaims_a_stale_one() {
        let _home = isolated_home();
        let store = tempfile::tempdir().unwrap();
        let lock_path = instances_dir().join(format!("{}.lock", project_id(store.path())));

        // A live foreign holder (a child process we can signal) blocks a second acquire.
        std::fs::create_dir_all(instances_dir()).unwrap();
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let live_pid = child.id();
        std::fs::write(&lock_path, live_pid.to_string()).unwrap();
        match acquire_writer_lock(store.path()) {
            Err(LockError::Held(p)) if p == live_pid => {}
            other => panic!("expected Held({live_pid}), got {other:?}"),
        }
        child.kill().ok();
        child.wait().ok();

        // A stale holder (dead pid) is reclaimed.
        std::fs::write(&lock_path, u32::MAX.to_string()).unwrap();
        let lock = acquire_writer_lock(store.path()).expect("stale lock reclaimed");
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
}
