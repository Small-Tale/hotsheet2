//! Spawn workspace binaries from a stable per-build snapshot (HS2-4TCTEW).
//!
//! Every Cargo command, even one with nothing to rebuild, replaces each uplifted binary in the
//! target profile directory (`target/debug/hotsheet-cli` and its siblings): it removes the file
//! and clones it again from `deps/`. Commands that select other packages also resolve other
//! features, so they rebuild and replace the binary with a different build. A test that spawns
//! `target/debug/<name>` while another Cargo command runs against the same target directory can
//! therefore find the path missing (`No such file or directory`). Five no-op
//! `cargo nextest run --workspace --no-run` commands left `target/debug/hotsheet-cli` missing on
//! 77 of about one million `stat` calls.
//!
//! [`path`] copies every executable in the profile directory into
//! `<profile>/test-bin-snapshots/<build identity>/` and returns the snapshot of `name`. On APFS
//! the copy is a clone: it shares blocks with the original and costs no disk space. The
//! binaries stay siblings, so a CLI still finds `hotsheet-server` beside itself. Exactly one
//! process builds each snapshot, under a lock file beside the snapshots; the others wait on that
//! lock and reuse the published directory without copying anything. The set is every profile
//! executable rather than a fixed list, so a binary that another binary starts as its sibling
//! is never left out when the workspace adds one.
//!
//! The snapshot holds private copies, never hard links. On macOS, unlinking another name of an
//! inode while a process makes its first exec of that inode can SIGKILL the exec. Linked
//! snapshots lost 6 of 180 parallel first execs when racing processes discarded their links,
//! and Cargo's own uplift unlinks the profile name in the same way. Clones lost none of 240.
//! Integration tests include this file with
//! `#[path = "../../test-support/workspace_bin.rs"] mod workspace_bin;` from their crate root and
//! never spawn `env!("CARGO_BIN_EXE_<name>")` or `Command::cargo_bin` directly.

use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime};

/// Snapshots unused for longer than this are pruned when a new one is made. A nextest test is
/// terminated after ten minutes, so no live test process still spawns from one this old.
const SNAPSHOT_RETENTION: Duration = Duration::from_secs(2 * 60 * 60);

/// Bound on waiting for Cargo to recreate a binary it is replacing. The gap normally lasts
/// well under a millisecond; this only turns a binary that never returns into a clear failure.
const REPLACEMENT_WAIT_LIMIT: Duration = Duration::from_secs(120);

/// The stable snapshot path of workspace binary `name` (without the platform suffix).
pub fn path(name: &str) -> PathBuf {
    static SNAPSHOT: OnceLock<PathBuf> = OnceLock::new();
    let snapshot = SNAPSHOT.get_or_init(snapshot);
    let binary = snapshot.join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    assert!(
        binary.is_file(),
        "workspace binary {name} is not built in {}; build the workspace binaries before this suite \
         (cargo nextest run --workspace builds them)",
        profile_dir().display()
    );
    binary
}

/// The Cargo profile directory that holds the uplifted binaries (`target/debug`).
fn profile_dir() -> PathBuf {
    let mut dir = std::env::current_exe().expect("locate the test executable");
    dir.pop();
    if dir.ends_with("deps") {
        dir.pop();
    }
    dir
}

fn snapshot() -> PathBuf {
    let profile = profile_dir();
    let root = profile.join("test-bin-snapshots");
    std::fs::create_dir_all(&root).expect("create the test binary snapshot directory");

    // Identify every executable the profile lists, then list again: a name that Cargo was
    // replacing during one listing is present in the next. Stop once a pass finds no new name.
    let mut identities = BTreeMap::new();
    loop {
        let new_names = executable_names(&profile)
            .into_iter()
            .filter(|name| !identities.contains_key(name))
            .collect::<Vec<_>>();
        if new_names.is_empty() {
            break;
        }
        for name in new_names {
            let identity = when_present(&profile.join(&name), |source| {
                std::fs::metadata(source).map(|metadata| identity(&metadata))
            });
            identities.insert(name, identity);
        }
    }

    let snapshot = root.join(format!("{:016x}", fingerprint(&identities)));
    // A published snapshot is complete (it appears by one rename), so reuse needs no lock.
    // Otherwise exactly one process builds it: the others wait on the lock, then find it
    // published. Without reflinks (Linux CI) each build is a real copy of several hundred
    // megabytes, which every process starting at once must not repeat.
    let _lock = (!snapshot.is_dir()).then(|| {
        let lock = exclusive_lock(&root.join(LOCK));
        if !snapshot.is_dir() {
            build(&profile, &root, &snapshot, identities.keys());
        }
        lock
    });
    // Every process that uses a snapshot renews it, so pruning measures the last use.
    std::fs::write(snapshot.join(LAST_USED), std::process::id().to_string())
        .expect("mark the test binary snapshot as used");
    snapshot
}

/// Copy `names` from the profile into a staging directory and publish it as `snapshot`.
/// The caller holds the snapshot lock.
fn build<'a>(
    profile: &Path,
    root: &Path,
    snapshot: &Path,
    names: impl Iterator<Item = &'a String>,
) {
    let staging = root.join(format!(".staging-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir(&staging).expect("create a test binary snapshot staging directory");
    // If Cargo replaces a binary between its identification and this copy, the snapshot holds
    // the newer complete build under the older identity. It is still one whole executable,
    // and the next process identifies the newer build itself.
    for name in names {
        when_present(&profile.join(name), |source| {
            std::fs::copy(source, staging.join(name))
        });
    }
    std::fs::rename(&staging, snapshot).expect("publish the test binary snapshot");
    prune(root, snapshot);
}

/// Lock file beside the snapshots that lets exactly one process build a snapshot.
const LOCK: &str = ".lock";

/// Bound on waiting for another process to finish building a snapshot. Building copies a few
/// hundred megabytes at most; this only turns a wedged holder into a clear failure.
const LOCK_WAIT_LIMIT: Duration = Duration::from_secs(10 * 60);

/// Take an exclusive cross-process lock on `path`, held until the returned file drops. The
/// blocking `lock` runs on a helper thread, so the wait ends when the holder releases it and
/// is bounded only by [`LOCK_WAIT_LIMIT`].
// HS2-4TCTEW: std's File::lock is stable since 1.89 and the pinned toolchain is 1.97; the
// workspace MSRV (1.85) covers the published crates, not this test-only helper.
#[allow(
    clippy::incompatible_msrv,
    reason = "HS2-4TCTEW: test helper built only with the pinned toolchain"
)]
fn exclusive_lock(path: &Path) -> std::fs::File {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
        .expect("open the test binary snapshot lock");
    let (locked, wait) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = file.lock().map(|()| file);
        let _ = locked.send(result);
    });
    match wait.recv_timeout(LOCK_WAIT_LIMIT) {
        Ok(Ok(file)) => file,
        Ok(Err(error)) => panic!("lock {}: {error}", path.display()),
        Err(_) => panic!(
            "another test process held {} for {LOCK_WAIT_LIMIT:?} while building a snapshot",
            path.display()
        ),
    }
}

/// Marker whose modification time records a snapshot's most recent use.
const LAST_USED: &str = ".last-used";

/// Names of the executables directly inside the profile directory.
fn executable_names(profile: &Path) -> Vec<String> {
    std::fs::read_dir(profile)
        .expect("list the Cargo profile directory")
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter(|entry| is_executable(&entry.path()))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect()
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.extension().is_none()
        && std::fs::metadata(path).is_ok_and(|metadata| metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
}

/// Run `step` on `source`, waiting while Cargo has removed `source` and not yet recreated it.
fn when_present<T>(source: &Path, step: impl Fn(&Path) -> std::io::Result<T>) -> T {
    let started = Instant::now();
    loop {
        match step(source) {
            Ok(value) => return value,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                assert!(
                    started.elapsed() < REPLACEMENT_WAIT_LIMIT,
                    "{} stayed missing while Cargo replaced it",
                    source.display()
                );
                std::thread::yield_now();
            }
            Err(error) => panic!("snapshot {}: {error}", source.display()),
        }
    }
}

#[cfg(unix)]
fn identity(metadata: &std::fs::Metadata) -> String {
    use std::os::unix::fs::MetadataExt;
    format!("{}:{}", metadata.dev(), metadata.ino())
}

#[cfg(not(unix))]
fn identity(metadata: &std::fs::Metadata) -> String {
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok())
        .unwrap_or_default();
    format!("{}:{}", metadata.len(), modified.as_nanos())
}

/// FNV-1a over the sorted name and identity pairs.
fn fingerprint(identities: &BTreeMap<String, String>) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for (name, identity) in identities {
        for byte in name.bytes().chain([0]).chain(identity.bytes()).chain([0]) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
    }
    hash
}

/// Remove snapshots and abandoned staging directories unused for longer than
/// [`SNAPSHOT_RETENTION`], so replaced builds do not accumulate under the target directory.
fn prune(root: &Path, keep: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let expired = std::fs::metadata(path.join(LAST_USED))
            .or_else(|_| entry.metadata())
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| SystemTime::now().duration_since(modified).ok())
            .is_some_and(|age| age > SNAPSHOT_RETENTION);
        if path != keep && path.is_dir() && expired {
            let _ = std::fs::remove_dir_all(path);
        }
    }
}
