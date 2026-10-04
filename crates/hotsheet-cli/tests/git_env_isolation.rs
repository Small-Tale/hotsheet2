//! HS2-RRD417: the real `hotsheet-cli` binary run under an inherited git repository
//! environment (as from a git hook, `git rebase --exec`, or `git bisect run`) must still
//! operate only on its own store. `GIT_DIR`, `GIT_WORK_TREE`, `GIT_INDEX_FILE`, and
//! friends point at a throwaway *sentinel* repository; afterwards the sentinel must be
//! byte-for-byte unchanged (no commits, no config change, notably no `core.bare`) and
//! the store must hold the expected history.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use assert_cmd::Command;

const IDENTITY: [(&str, &str); 4] = [
    ("GIT_AUTHOR_NAME", "Hot Sheet test"),
    ("GIT_AUTHOR_EMAIL", "hotsheet@example.invalid"),
    ("GIT_COMMITTER_NAME", "Hot Sheet test"),
    ("GIT_COMMITTER_EMAIL", "hotsheet@example.invalid"),
];

fn git(dir: &Path, args: &[&str]) -> String {
    let out = hotsheet_ticketing::git::command_in(dir)
        .args(args)
        .envs(IDENTITY)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

/// A committed sentinel repository the hostile environment points at.
fn sentinel(root: &Path) -> PathBuf {
    let dir = root.join("sentinel");
    std::fs::create_dir(&dir).unwrap();
    git(&dir, &["init", "-q", "-b", "main"]);
    std::fs::write(dir.join("README"), "sentinel\n").unwrap();
    git(&dir, &["add", "README"]);
    git(&dir, &["commit", "-q", "-m", "sentinel"]);
    dir
}

/// Every file under `dir` (worktree and `.git`) with its bytes.
fn snapshot(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(base, &path, out);
            } else {
                let bytes = std::fs::read(&path).unwrap();
                out.insert(path.strip_prefix(base).unwrap().to_owned(), bytes);
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

/// `hotsheet-cli` with the sentinel exported the way a git hook would.
fn hostile_cli(sentinel: &Path, home: &Path) -> Command {
    let git_dir = sentinel.join(".git");
    let mut cmd = Command::cargo_bin("hotsheet-cli").unwrap();
    cmd.env("HOTSHEET_HOME", home)
        .envs(IDENTITY)
        .env("GIT_DIR", &git_dir)
        .env("GIT_WORK_TREE", sentinel)
        .env("GIT_INDEX_FILE", git_dir.join("index"))
        .env("GIT_OBJECT_DIRECTORY", git_dir.join("objects"))
        .env("GIT_COMMON_DIR", &git_dir)
        .env("GIT_PREFIX", "");
    cmd
}

#[test]
fn cli_store_operations_ignore_an_inherited_git_repository_environment() {
    let root = tempfile::tempdir().unwrap();
    let sentinel = sentinel(root.path());
    let before = snapshot(&sentinel);
    let home = root.path().join("home");
    let project = root.path().join("project");
    std::fs::create_dir(&project).unwrap();
    let remote = root.path().join("remote.git");
    git(
        root.path(),
        &["init", "-q", "--bare", remote.to_str().unwrap()],
    );
    let store = root.path().join("store");

    // Standalone init: `git init`, merge-driver config, `remote add origin`.
    hostile_cli(&sentinel, &home)
        .current_dir(&project)
        .args(["init", "--standalone", "--prefix", "ISO", "--at"])
        .arg(&store)
        .arg("--remote")
        .arg(&remote)
        .assert()
        .success();
    // Committing writes.
    hostile_cli(&sentinel, &home)
        .arg("-C")
        .arg(&store)
        .args(["new", "--title", "isolated ticket", "--category", "bug"])
        .assert()
        .success();
    let slug = hotsheet_ticketing::FsStore::open(&store)
        .unwrap()
        .list_tickets()
        .unwrap()
        .into_iter()
        .next()
        .expect("ticket created")
        .slug
        .to_string();
    hostile_cli(&sentinel, &home)
        .arg("-C")
        .arg(&store)
        .args(["edit", &slug, "--status", "started"])
        .assert()
        .success();
    // Fetch / integrate / push.
    hostile_cli(&sentinel, &home)
        .arg("-C")
        .arg(&store)
        .arg("sync")
        .assert()
        .success();
    // A plain in-place `init` of a fresh directory.
    let local = root.path().join("local-store");
    std::fs::create_dir(&local).unwrap();
    hostile_cli(&sentinel, &home)
        .arg("init")
        .arg("-C")
        .arg(&local)
        .assert()
        .success();

    assert_eq!(
        before,
        snapshot(&sentinel),
        "the inherited GIT_DIR repository must be byte-for-byte untouched"
    );
    assert!(
        git(&sentinel, &["config", "--get", "core.bare"]) == "false",
        "the sentinel must never be made bare"
    );
    assert_eq!(git(&sentinel, &["rev-list", "--count", "HEAD"]), "1");

    // The store itself is a correct, non-bare repository holding the ticket history.
    assert!(store.join(".git").is_dir());
    assert_eq!(git(&store, &["rev-parse", "--is-bare-repository"]), "false");
    let log = git(&store, &["log", "--format=%s"]);
    assert!(log.lines().count() >= 2, "store history: {log}");
    assert_eq!(git(&store, &["status", "--porcelain"]), "");
    assert_eq!(
        git(&store, &["rev-parse", "HEAD"]),
        git(&remote, &["rev-parse", "HEAD"]),
        "sync pushed the store to its own remote"
    );
    assert!(local.join(".git").is_dir());
    assert_eq!(git(&local, &["rev-parse", "--is-bare-repository"]), "false");
}
