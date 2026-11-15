//! Launch a freshly built executable once before a test times it (HS2-04D6DW, HS2-W00050).
//!
//! macOS assesses a newly linked or newly copied executable on its first launch, through one
//! queue for the whole host. 40 fresh copies launched together waited 0.2 to 0.8 s each on an
//! idle machine, and while other worktrees build and run their own fresh binaries the wait
//! grows past several seconds. A test whose readiness, output, or exit window starts before
//! the child's first launch then fails although the code under test is fine.
//!
//! [`launched`] runs the executable once to completion, without any deadline, and returns its
//! path, so a busy host only slows the test and its own windows measure the code under test.
//! Integration tests include this file with
//! `#[path = "../../test-support/first_launch.rs"] mod first_launch;` from their crate root.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, PoisonError};

/// Launch `exe` with `args` once per test process and executable file (path and inode, so a
/// replaced copy counts as new), wait for it to exit, and return `exe`. Pick `args` that exit
/// at once without side effects, such as `["--version"]`. The exit status is not checked: only
/// the completed launch matters.
pub fn launched<P: AsRef<Path>>(exe: P, args: &[&str]) -> P {
    static LAUNCHED: Mutex<Vec<(PathBuf, Option<u64>)>> = Mutex::new(Vec::new());
    let path = exe.as_ref().to_path_buf();
    let key = (path.clone(), inode(&path));
    let mut launched = LAUNCHED.lock().unwrap_or_else(PoisonError::into_inner);
    if !launched.contains(&key) {
        Command::new(&path)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap_or_else(|error| panic!("launch {} once: {error}", path.display()));
        launched.push(key);
    }
    drop(launched);
    exe
}

#[cfg(unix)]
fn inode(path: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).ok().map(|metadata| metadata.ino())
}

#[cfg(not(unix))]
fn inode(_path: &Path) -> Option<u64> {
    None
}
