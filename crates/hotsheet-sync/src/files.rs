//! Atomic file publication that works while readers hold the file open (HS2-J5DCJH).
//!
//! Hot Sheet publishes configuration, registries, ledgers and caches by writing a sibling
//! temporary file and renaming it over the target, so a reader sees either the complete
//! previous file or the complete new one. On Windows, `tempfile`'s
//! [`NamedTempFile::persist`](tempfile::NamedTempFile::persist) uses `MoveFileExW`, which
//! fails with `Access is denied` (os error 5) whenever another handle has the target open,
//! even a reader that allows deletion. [`std::fs::rename`] instead asks for POSIX rename
//! semantics (`FILE_RENAME_FLAG_POSIX_SEMANTICS`): the target is replaced while open
//! readers keep the old contents, exactly as on Unix. Readers Hot Sheet opens through
//! [`std::fs::File`] share deletion, so they never block a publication.
//!
//! A handle that does not share deletion (another program, a virus scanner inspecting the
//! fresh file) still blocks a Windows rename for a moment, so [`replace`] retries a
//! sharing or access failure a few times with a short, bounded backoff before it reports
//! the error. Off Windows it is exactly one [`std::fs::rename`].
//!
//! [`persist`] is the only way Hot Sheet publishes a [`tempfile::NamedTempFile`]; a
//! source-scan test forbids any other `NamedTempFile::persist` call under `crates/`.

use std::fs::File;
use std::io;
use std::path::Path;
use std::time::Duration;

/// How many times [`replace`] retries a transient Windows sharing failure.
const RETRIES: u32 = 8;
/// The first retry delay; each later one doubles it (1, 2, 4 ... 128 ms; 255 ms in all).
const FIRST_DELAY: Duration = Duration::from_millis(1);

/// Atomically rename `from` over `to`, replacing an existing `to` even while readers hold
/// it open. See the module documentation.
///
/// # Errors
///
/// The rename's error once it is not a transient sharing failure, or once the retries are
/// spent.
pub fn replace(from: &Path, to: &Path) -> io::Result<()> {
    replace_with(
        from,
        to,
        |from, to| std::fs::rename(from, to),
        std::thread::sleep,
    )
}

/// Publish a completed temporary file at `to` through [`replace`], returning its still-open
/// handle. On failure the temporary file is removed.
///
/// # Errors
///
/// The temporary file could not be kept, or [`replace`] failed.
pub fn persist(file: tempfile::NamedTempFile, to: impl AsRef<Path>) -> io::Result<File> {
    // `keep` clears Windows' temporary-file attribute and disarms deletion on drop.
    let (handle, staged) = file.keep().map_err(|error| error.error)?;
    if let Err(error) = replace(&staged, to.as_ref()) {
        let _ = std::fs::remove_file(&staged);
        return Err(error);
    }
    Ok(handle)
}

/// Whether `error` is a sharing failure that another handle may release shortly.
#[must_use]
pub fn is_transient_sharing_error(error: &io::Error) -> bool {
    // ERROR_ACCESS_DENIED (5) and ERROR_SHARING_VIOLATION (32). Only Windows reports
    // either for a rename blocked by another handle.
    cfg!(windows) && matches!(error.raw_os_error(), Some(5 | 32))
}

fn replace_with(
    from: &Path,
    to: &Path,
    mut rename: impl FnMut(&Path, &Path) -> io::Result<()>,
    mut sleep: impl FnMut(Duration),
) -> io::Result<()> {
    let mut delay = FIRST_DELAY;
    let mut attempt = 0;
    loop {
        match rename(from, to) {
            Err(error) if attempt < RETRIES && is_transient_sharing_error(&error) => {
                sleep(delay);
                delay = delay.saturating_mul(2);
                attempt += 1;
            }
            result => return result,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn hs2_j5dcjh_replace_keeps_open_readers_on_the_previous_file() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("config.json");
        std::fs::write(&target, b"previous").unwrap();
        let mut reader = File::open(&target).unwrap();
        let staged = dir.path().join("config.json.tmp");
        std::fs::write(&staged, b"next").unwrap();
        replace(&staged, &target).unwrap();
        let mut old = String::new();
        reader.read_to_string(&mut old).unwrap();
        assert_eq!(old, "previous");
        assert_eq!(std::fs::read(&target).unwrap(), b"next");
        assert!(!staged.exists());
    }

    #[test]
    fn hs2_j5dcjh_persist_publishes_over_an_open_target_and_returns_the_handle() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("registry.json");
        std::fs::write(&target, b"previous").unwrap();
        let mut reader = File::open(&target).unwrap();
        let mut staged = tempfile::NamedTempFile::new_in(dir.path()).unwrap();
        staged.write_all(b"next").unwrap();
        let mut handle = persist(staged, &target).unwrap();
        handle.write_all(b"!").unwrap();
        drop(handle);
        let mut old = String::new();
        reader.read_to_string(&mut old).unwrap();
        assert_eq!(old, "previous");
        assert_eq!(std::fs::read(&target).unwrap(), b"next!");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn hs2_j5dcjh_a_failed_persist_removes_the_temporary_file() {
        let dir = tempfile::tempdir().unwrap();
        let staged = tempfile::NamedTempFile::new_in(dir.path()).unwrap();
        let error = persist(staged, dir.path().join("missing/target")).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn hs2_j5dcjh_retries_only_transient_sharing_failures_with_a_bounded_backoff() {
        let path = Path::new("unused");
        let denied = || io::Error::from_raw_os_error(5);
        let mut calls = 0;
        let mut delays = Vec::new();
        let result = replace_with(
            path,
            path,
            |_, _| {
                calls += 1;
                if calls < 3 { Err(denied()) } else { Ok(()) }
            },
            |delay| delays.push(delay),
        );
        if cfg!(windows) {
            result.unwrap();
            assert_eq!(calls, 3);
            assert_eq!(delays, [Duration::from_millis(1), Duration::from_millis(2)]);
        } else {
            // Off Windows the error is not a transient sharing failure: no retry.
            assert_eq!(result.unwrap_err().raw_os_error(), Some(5));
            assert_eq!(calls, 1);
            assert!(delays.is_empty());
        }

        // A persistent failure stops after the bounded retries.
        let mut calls = 0;
        let mut slept = Duration::ZERO;
        let error = replace_with(
            path,
            path,
            |_, _| {
                calls += 1;
                Err(io::Error::from_raw_os_error(32))
            },
            |delay| slept += delay,
        )
        .unwrap_err();
        assert_eq!(error.raw_os_error(), Some(32));
        if cfg!(windows) {
            assert_eq!(calls, RETRIES + 1);
            assert_eq!(slept, Duration::from_millis(255));
        } else {
            assert_eq!(calls, 1);
        }

        // Any other error returns at once.
        let mut calls = 0;
        let error = replace_with(
            path,
            path,
            |_, _| {
                calls += 1;
                Err(io::Error::from(io::ErrorKind::NotFound))
            },
            |_| unreachable!("no retry for a missing file"),
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert_eq!(calls, 1);
    }
}
