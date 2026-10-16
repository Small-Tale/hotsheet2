//! Bounded settings transaction locks under `<home>/settings-locks` (HS2-MZ1TMH).
//!
//! Each settings file is locked through `<sha256(canonical path)>.lock` in the machine
//! home, so a committed project settings file never gains a lock beside it. Every project
//! (or temp test project) that ever wrote settings used to leave one lock file behind
//! forever. Locks now record the settings path they guard, and an occasional prune removes
//! unheld locks whose settings file no longer exists, plus unheld legacy (empty) locks.
//!
//! Deleting a lock file is made safe by an identity check: after taking the OS lock, a
//! writer confirms that the open file is still the one at the lock path. A writer whose
//! file was pruned while it waited drops that orphaned handle and retries on the fresh
//! file, so two writers can never hold "the" lock through different inodes.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::Path;
#[cfg(unix)]
use std::time::{Duration, SystemTime};

use crate::file_lock::FileLock;

/// Minimum time between prunes of one lock directory, so a busy writer does not rescan it.
#[cfg(unix)]
pub(crate) const PRUNE_INTERVAL: Duration = Duration::from_secs(60 * 60);
#[cfg(unix)]
const PRUNE_MARKER: &str = ".last-prune";

/// Lock `<lock_dir>/<file_name>` for the settings file at `target`, recording `target`
/// in the lock file, then prune stale locks if the last prune is older than
/// [`PRUNE_INTERVAL`].
pub(crate) fn acquire(lock_dir: &Path, file_name: &str, target: &Path) -> io::Result<FileLock> {
    std::fs::create_dir_all(lock_dir)?;
    let lock_path = lock_dir.join(file_name);
    let lock = acquire_recorded(&lock_path, target)?;
    #[cfg(unix)]
    {
        // Pruning is best-effort housekeeping and never fails the settings write.
        let _ = prune_if_due(lock_dir, &lock_path, PRUNE_INTERVAL, SystemTime::now());
    }
    Ok(lock)
}

fn acquire_recorded(lock_path: &Path, target: &Path) -> io::Result<FileLock> {
    loop {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)?;
        let lock = FileLock::acquire(file)?;
        if !still_at_path(lock.file(), lock_path)? {
            // Pruned while this writer waited; the orphaned inode guards nothing.
            continue;
        }
        if lock.file().metadata()?.len() == 0 {
            let mut file = lock.file();
            file.write_all(target.to_string_lossy().as_bytes())?;
            file.flush()?;
        }
        return Ok(lock);
    }
}

#[cfg(unix)]
fn still_at_path(file: &File, path: &Path) -> io::Result<bool> {
    use std::os::unix::fs::MetadataExt;

    let open = file.metadata()?;
    match std::fs::metadata(path) {
        Ok(current) => Ok(current.dev() == open.dev() && current.ino() == open.ino()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// Non-Unix hosts never prune, so a lock file is never replaced under a waiting writer.
#[cfg(not(unix))]
fn still_at_path(_file: &File, _path: &Path) -> io::Result<bool> {
    Ok(true)
}

/// Prune when the marker is missing or older than `interval`. Returns how many lock files
/// were removed (`None` when the prune was not due).
#[cfg(unix)]
pub(crate) fn prune_if_due(
    lock_dir: &Path,
    keep: &Path,
    interval: Duration,
    now: SystemTime,
) -> io::Result<Option<usize>> {
    let marker = lock_dir.join(PRUNE_MARKER);
    let last = std::fs::metadata(&marker).and_then(|metadata| metadata.modified());
    if let Ok(last) = last
        && now.duration_since(last).unwrap_or_default() < interval
    {
        return Ok(None);
    }
    // Touch first so concurrent writers that just missed the window skip this round.
    File::create(&marker)?.set_modified(now)?;
    prune(lock_dir, keep).map(Some)
}

/// Remove every unheld lock in `lock_dir` (other than `keep`) whose recorded settings file
/// no longer exists, or that records nothing (written before locks recorded their path).
#[cfg(unix)]
pub(crate) fn prune(lock_dir: &Path, keep: &Path) -> io::Result<usize> {
    let mut removed = 0;
    for entry in std::fs::read_dir(lock_dir)? {
        let path = entry?.path();
        if path == keep || path.extension().is_none_or(|extension| extension != "lock") {
            continue;
        }
        if prune_one(&path).unwrap_or(false) {
            removed += 1;
        }
    }
    Ok(removed)
}

#[cfg(unix)]
fn prune_one(path: &Path) -> io::Result<bool> {
    let file = match OpenOptions::new().read(true).write(true).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    // A held lock belongs to a live settings transaction; leave it for a later prune.
    let Some(lock) = FileLock::try_acquire(file)? else {
        return Ok(false);
    };
    if !still_at_path(lock.file(), path)? {
        return Ok(false);
    }
    let mut recorded = String::new();
    let mut file = lock.file();
    file.read_to_string(&mut recorded)?;
    if !recorded.is_empty() && Path::new(&recorded).exists() {
        return Ok(false);
    }
    // Unlink while holding the lock: a writer waiting on this inode re-checks its
    // identity after the lock is released and retries on a fresh file.
    std::fs::remove_file(path)?;
    drop(lock);
    Ok(true)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn lock_count(dir: &Path) -> usize {
        std::fs::read_dir(dir)
            .unwrap()
            .filter(|entry| {
                entry
                    .as_ref()
                    .unwrap()
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "lock")
            })
            .count()
    }

    #[test]
    fn locks_record_their_settings_path_and_prune_only_vanished_unheld_targets() {
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join("settings-locks");
        let live = tempfile::tempdir().unwrap();
        let live_target = live.path().join("settings.json");
        std::fs::write(&live_target, "{}").unwrap();
        drop(acquire(&dir, "live.lock", &live_target).unwrap());
        assert_eq!(
            std::fs::read_to_string(dir.join("live.lock")).unwrap(),
            live_target.to_string_lossy()
        );

        // Fifty temp projects come and go, as a test suite's would.
        for index in 0..50 {
            let project = tempfile::tempdir().unwrap();
            let target = project.path().join("settings.json");
            std::fs::write(&target, "{}").unwrap();
            drop(acquire(&dir, &format!("gone-{index}.lock"), &target).unwrap());
        }
        // A legacy zero-byte lock from before locks recorded their path.
        std::fs::write(dir.join("legacy.lock"), "").unwrap();
        // A vanished target whose lock is held right now must survive the prune.
        let held_target = home.path().join("vanished/settings.json");
        let held = acquire(&dir, "held.lock", &held_target).unwrap();
        assert_eq!(lock_count(&dir), 53);

        let removed = prune(&dir, &dir.join("live.lock")).unwrap();
        assert_eq!(
            removed, 51,
            "every vanished or legacy unheld lock is removed"
        );
        assert!(dir.join("live.lock").exists());
        assert!(dir.join("held.lock").exists());
        assert_eq!(lock_count(&dir), 2);
        drop(held);
        assert_eq!(prune(&dir, &dir.join("live.lock")).unwrap(), 1);
        assert_eq!(lock_count(&dir), 1);
    }

    #[test]
    fn prune_runs_at_most_once_per_interval() {
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join("settings-locks");
        let gone = home.path().join("gone/settings.json");
        let keep = dir.join("keep.lock");
        let now = SystemTime::now();
        drop(acquire_recorded_in(&dir, "a.lock", &gone));
        assert_eq!(
            prune_if_due(&dir, &keep, PRUNE_INTERVAL, now).unwrap(),
            Some(1)
        );
        drop(acquire_recorded_in(&dir, "b.lock", &gone));
        let soon = now + Duration::from_secs(60);
        assert_eq!(
            prune_if_due(&dir, &keep, PRUNE_INTERVAL, soon).unwrap(),
            None
        );
        assert!(dir.join("b.lock").exists());
        let later = now + PRUNE_INTERVAL + Duration::from_secs(1);
        assert_eq!(
            prune_if_due(&dir, &keep, PRUNE_INTERVAL, later).unwrap(),
            Some(1)
        );
        assert!(!dir.join("b.lock").exists());
    }

    fn acquire_recorded_in(dir: &Path, name: &str, target: &Path) -> FileLock {
        std::fs::create_dir_all(dir).unwrap();
        acquire_recorded(&dir.join(name), target).unwrap()
    }

    #[test]
    fn a_waiter_whose_lock_file_was_pruned_retries_on_a_fresh_file() {
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join("settings-locks");
        let target = home.path().join("gone/settings.json");
        let first = acquire_recorded_in(&dir, "x.lock", &target);
        let lock_path = dir.join("x.lock");
        let (tx, rx) = std::sync::mpsc::channel();
        let waiter = {
            let lock_path = lock_path.clone();
            let target = target.clone();
            std::thread::spawn(move || {
                let lock = acquire_recorded(&lock_path, &target).unwrap();
                tx.send(()).unwrap();
                // Holding the lock: the path names exactly the inode this waiter holds.
                assert!(still_at_path(lock.file(), &lock_path).unwrap());
                lock
            })
        };
        std::thread::sleep(Duration::from_millis(100));
        assert!(
            rx.try_recv().is_err(),
            "the waiter is blocked on the held lock"
        );
        // A pruner removes the file while the first holder still has it.
        std::fs::remove_file(&lock_path).unwrap();
        drop(first);
        let held = waiter.join().unwrap();
        rx.recv().unwrap();
        assert_eq!(
            std::fs::read_to_string(&lock_path).unwrap(),
            target.to_string_lossy()
        );
        // While the waiter holds the fresh file, a prune leaves it alone.
        assert_eq!(prune(&dir, &dir.join("other.lock")).unwrap(), 0);
        drop(held);
    }
}
