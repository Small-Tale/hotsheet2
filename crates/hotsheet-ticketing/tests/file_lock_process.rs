//! Public advisory-lock transitions on native Unix and Windows (HS2-9TD8Q5).
#![cfg(any(unix, windows))]

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use hotsheet_ticketing::FileLock;

fn open(path: &Path) -> File {
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .unwrap()
}

fn try_lock(path: &Path) -> Option<FileLock> {
    let path = path.to_path_buf();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(FileLock::try_acquire(open(&path)));
    });
    rx.recv_timeout(Duration::from_secs(10))
        .expect("nonblocking lock acquisition blocked")
        .unwrap()
}

struct Holder(Child);

impl Holder {
    fn start(path: &Path) -> Self {
        let mut holder = Self(
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "hs2_9td8q5_lock_holder_child", "--nocapture"])
                .env("HS2_9TD8Q5_LOCK_PATH", path)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let stdout = holder.0.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                if line.unwrap().ends_with("LOCK_ACQUIRED") {
                    let _ = tx.send(());
                    return;
                }
            }
        });
        rx.recv_timeout(Duration::from_secs(10))
            .expect("lock-holder child did not become ready");
        holder
    }
}

impl Drop for Holder {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn hs2_9td8q5_lock_holder_child() {
    let Some(path) = std::env::var_os("HS2_9TD8Q5_LOCK_PATH") else {
        return;
    };
    let _lock = FileLock::acquire(open(Path::new(&path))).unwrap();
    println!("LOCK_ACQUIRED");
    std::io::stdout().flush().unwrap();
    let mut release = [0];
    let _ = std::io::stdin().read_exact(&mut release);
}

#[test]
fn hs2_9td8q5_nonblocking_lock_repeats_busy_drop_and_reacquire() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("state.lock");
    for _ in 0..3 {
        let lock = try_lock(&path).expect("unlocked file must be acquired");
        assert!(try_lock(&path).is_none(), "held lock must return busy");
        drop(lock);
        assert!(path.is_file(), "lock release must preserve the lock file");
        assert!(try_lock(&path).is_some(), "dropped lock must be reusable");
    }
}

#[test]
fn hs2_9td8q5_process_exit_releases_a_busy_lock_without_destructors() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("wait.lock");
    for _ in 0..3 {
        let mut holder = Holder::start(&path);
        assert!(try_lock(&path).is_none(), "child must own the live lock");
        // Child::kill uses the native process API on Windows; no Unix signal contract.
        holder.0.kill().unwrap();
        holder.0.wait().unwrap();
        assert!(
            try_lock(&path).is_some(),
            "OS must release the dead owner's lock"
        );
    }
}
