use std::path::Path;

use hotsheet_ticketing::{FsStore, StoreMetadata};

/// HS2-081PXN: real ticket-store fixtures own their Git baseline, even below an
/// ignored checkout directory. Source-free machine bootstrap anchors stay separate.
pub(super) fn init(path: &Path, metadata: &StoreMetadata) -> FsStore {
    std::fs::create_dir_all(path).unwrap();
    for args in [
        vec!["init", "-q"],
        vec!["config", "user.name", "Synthetic Fixture"],
        vec!["config", "user.email", "fixture@example.test"],
        vec!["config", "commit.gpgsign", "false"],
    ] {
        git(path, &args);
    }
    let store = FsStore::init(path, metadata).unwrap();
    git(path, &["add", "hotsheet-store.json", ".gitignore"]);
    git(
        path,
        &["commit", "-q", "-m", "Initialize synthetic ticket store"],
    );
    git(path, &["rev-parse", "--verify", "HEAD"]);
    let output = hotsheet_ticketing::git::command_in(path)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let root = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        std::fs::canonicalize(root.trim()).unwrap(),
        std::fs::canonicalize(path).unwrap()
    );
    store
}

fn git(path: &Path, args: &[&str]) {
    let output = hotsheet_ticketing::git::command_in(path)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "synthetic Git fixture {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
