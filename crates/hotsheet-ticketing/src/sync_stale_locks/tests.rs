//! HS2-T0V269: the staleness proof and the move aside, on synthetic repositories. Scans
//! are injected for the proof's branches; the platform scan runs for real (`/proc` on
//! Linux, `lsof` on macOS) against a live process that holds a lock.

#[cfg(unix)]
use std::io::{BufRead as _, BufReader, Write as _};
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::process::{Child, Stdio};
use std::time::{Duration, SystemTime};

use super::*;

/// Make `path` look `age` old.
pub(crate) fn age_file(path: &Path, age: Duration) {
    std::fs::File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(SystemTime::now() - age)
        .unwrap();
}

/// Plant a lock an hour old, older than [`STALE_LOCK_MIN_AGE`].
pub(crate) fn plant_old(lock: &Path) {
    std::fs::create_dir_all(lock.parent().unwrap()).unwrap();
    std::fs::write(lock, "stale\n").unwrap();
    age_file(lock, Duration::from_secs(3600));
}

/// A live process that holds `lock` open on fd 3, as a running Git command holds its lock.
/// It has told us it holds the file before this returns.
#[cfg(unix)]
pub(crate) fn hold_open(lock: &Path) -> Child {
    let mut child = crate::git::launch("sh")
        .args(["-c", "exec 3<\"$1\"; echo ready; exec sleep 600", "sh"])
        .arg(lock)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.as_mut().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert_eq!(line.trim(), "ready");
    child
}

/// A live `git` process whose working directory is `store`, answering on stdin. It has
/// answered one request before this returns, so it has started and changed directory.
#[cfg(unix)]
pub(crate) fn git_running_in(store: &Path) -> Child {
    let mut child = crate::git::command()
        .args(["cat-file", "--batch-check"])
        .current_dir(store)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.as_mut().unwrap().write_all(b"HEAD\n").unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.as_mut().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert!(!line.is_empty());
    child
}

#[cfg(unix)]
pub(crate) fn stop(mut child: Child) {
    drop(child.stdin.take());
    let _ = child.kill();
    let _ = child.wait();
}

fn repo() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("store");
    std::fs::create_dir_all(&store).unwrap();
    assert!(crate::sync::git_ok(&store, &["init", "--quiet"]));
    (dir, store)
}

fn targets(store: &Path) -> Targets {
    Targets::for_store(store).unwrap()
}

/// The scan result of a process that holds only this store's coordination lock: this
/// process, as the cycle runs it.
fn me(store: &Path) -> ProcessFiles {
    ProcessFiles {
        pid: std::process::id(),
        command: "hotsheet".into(),
        cwd: Some(PathBuf::from("/")),
        open: vec![targets(store).common_dir.join("hotsheet-autocommit.lock")],
    }
}

fn stashed(store: &Path) -> Vec<PathBuf> {
    let root = store.join(".git").join(STALE_LOCK_DIR);
    let mut out = Vec::new();
    let Ok(dirs) = std::fs::read_dir(root) else {
        return out;
    };
    for dir in dirs.flatten() {
        out.push(dir.path());
    }
    out.sort();
    out
}

#[test]
fn hs2_t0v269_an_old_unheld_lock_is_moved_aside_never_deleted() {
    let (_dir, store) = repo();
    let lock = store.join(".git/index.lock");
    plant_old(&lock);
    let ref_lock = store.join(".git/refs/heads/main.lock");
    plant_old(&ref_lock);
    let mine = me(&store);
    let scan = move || Ok(vec![mine.clone()]);
    let recovery = recover_with(
        &store,
        &[lock.clone(), ref_lock.clone()],
        SystemTime::now(),
        &scan,
    );
    assert!(recovery.kept.is_empty(), "{:?}", recovery.kept);
    assert_eq!(recovery.moved.len(), 2);
    assert!(!lock.exists() && !ref_lock.exists());
    let dirs = stashed(&store);
    assert_eq!(dirs.len(), 1, "one stash directory per pass");
    // The lock keeps its path relative to the Git directory, and its content.
    assert_eq!(
        std::fs::read_to_string(dirs[0].join("index.lock")).unwrap(),
        "stale\n"
    );
    assert!(dirs[0].join("refs/heads/main.lock").is_file());
    let record: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dirs[0].join("recovery.json")).unwrap()).unwrap();
    assert_eq!(record["moved"].as_array().unwrap().len(), 2);
    assert_eq!(record["ticket"], "HS2-T0V269");
    let first = &recovery.moved[0];
    assert_eq!(first.from, ".git/index.lock");
    assert!(
        first.to.starts_with(".git/hotsheet-stale-locks/") && first.to.ends_with("/index.lock"),
        "{}",
        first.to
    );
    assert!(first.age_seconds >= 3600);
    let line = moved_line(&recovery.moved);
    assert!(
        line.starts_with("Moved stale Git locks aside: .git/index.lock to "),
        "{line}"
    );
}

#[test]
fn hs2_t0v269_a_young_lock_is_kept_without_scanning_processes() {
    let (_dir, store) = repo();
    let lock = store.join(".git/index.lock");
    std::fs::write(&lock, "").unwrap();
    age_file(&lock, STALE_LOCK_MIN_AGE - Duration::from_secs(30));
    let scan = || -> Result<Vec<ProcessFiles>, String> { panic!("a young lock needs no scan") };
    let recovery = recover_with(
        &store,
        std::slice::from_ref(&lock),
        SystemTime::now(),
        &scan,
    );
    assert!(recovery.moved.is_empty());
    assert!(matches!(recovery.kept[..], [(_, Kept::TooYoung { .. })]));
    assert!(lock.is_file());
    assert!(stashed(&store).is_empty());
    let lines = recovery.kept_lines();
    assert_eq!(
        lines,
        [
            "Not moved automatically: .git/index.lock is younger than 10 minutes, so its Git \
          process may still be running."
        ]
    );
}

#[test]
fn hs2_t0v269_an_unverifiable_or_failed_scan_moves_nothing() {
    let (_dir, store) = repo();
    let lock = store.join(".git/index.lock");
    plant_old(&lock);
    // The scan failed outright (no lsof, no /proc, or Windows).
    let failed = || Err("no scan here".to_string());
    let recovery = recover_with(
        &store,
        std::slice::from_ref(&lock),
        SystemTime::now(),
        &failed,
    );
    assert_eq!(
        recovery.kept,
        [(
            ".git/index.lock".to_string(),
            Kept::Unverifiable("no scan here".into())
        )]
    );
    // The scan ran but could not see this process's own open lock (a sandbox): untrusted.
    let blind = || Ok(Vec::new());
    let recovery = recover_with(
        &store,
        std::slice::from_ref(&lock),
        SystemTime::now(),
        &blind,
    );
    assert!(matches!(recovery.kept[..], [(_, Kept::Unverifiable(_))]));
    assert!(lock.is_file());
    assert!(stashed(&store).is_empty());
}

#[test]
fn hs2_t0v269_any_process_in_the_git_directory_or_git_in_the_store_keeps_the_lock() {
    let (_dir, store) = repo();
    let lock = store.join(".git/index.lock");
    plant_old(&lock);
    let t = targets(&store);
    let mine = me(&store);
    let other = |process: ProcessFiles| {
        let mine = mine.clone();
        move || Ok(vec![mine.clone(), process.clone()])
    };
    let cases = [
        // Has the lock itself open.
        (
            ProcessFiles {
                pid: 7,
                command: "git".into(),
                cwd: Some(PathBuf::from("/")),
                open: vec![t.common_dir.join("index.lock")],
            },
            false,
        ),
        // Has a pack open (an editor's libgit2 integration).
        (
            ProcessFiles {
                pid: 8,
                command: "Editor".into(),
                cwd: Some(PathBuf::from("/")),
                open: vec![t.common_dir.join("objects/pack/p.pack")],
            },
            false,
        ),
        // A Git process working in the store with nothing open yet.
        (
            ProcessFiles {
                pid: 9,
                command: "git".into(),
                cwd: Some(t.work_dirs[0].join("sub")),
                open: Vec::new(),
            },
            true,
        ),
        // Anything working inside the Git directory.
        (
            ProcessFiles {
                pid: 10,
                command: "sh".into(),
                cwd: Some(t.common_dir.join("refs")),
                open: Vec::new(),
            },
            true,
        ),
    ];
    for (process, working_in_store) in cases {
        let pid = process.pid;
        let scan = other(process.clone());
        let recovery = recover_with(
            &store,
            std::slice::from_ref(&lock),
            SystemTime::now(),
            &scan,
        );
        let [(_, Kept::Held(holders))] = &recovery.kept[..] else {
            panic!("{process:?}: {:?}", recovery.kept);
        };
        assert_eq!(holders[0].pid, pid);
        assert_eq!(holders[0].working_in_store, working_in_store, "{process:?}");
        assert!(lock.is_file());
        let line = &recovery.kept_lines()[0];
        assert!(line.contains(&format!("process {pid} (")), "{line}");
    }
    assert!(stashed(&store).is_empty());
}

#[test]
fn hs2_t0v269_harmless_processes_do_not_keep_a_lock() {
    let (_dir, store) = repo();
    let t = targets(&store);
    let mine = me(&store);
    let harmless = [
        // Another Hot Sheet waiting on the coordination lock this process holds.
        ProcessFiles {
            pid: 11,
            command: "hotsheet-server".into(),
            cwd: Some(PathBuf::from("/")),
            open: vec![t.common_dir.join("hotsheet-history-sync.lock")],
        },
        // A shell sitting in the store is not Git.
        ProcessFiles {
            pid: 12,
            command: "zsh".into(),
            cwd: Some(t.work_dirs[0].clone()),
            open: Vec::new(),
        },
        // Git elsewhere.
        ProcessFiles {
            pid: 13,
            command: "git".into(),
            cwd: Some(PathBuf::from("/")),
            open: vec![PathBuf::from("/elsewhere/.git/index.lock")],
        },
    ];
    let mut all = vec![mine];
    all.extend(harmless);
    assert_eq!(holders(&all, &t, std::process::id()), Ok(Vec::new()));
}

#[test]
fn hs2_t0v269_a_lock_that_changes_or_vanishes_mid_move_is_not_moved() {
    let (_dir, store) = repo();
    let t = targets(&store);
    let lock = store.join(".git/index.lock");
    plant_old(&lock);
    let checked = std::fs::metadata(&lock).unwrap().modified().unwrap();
    let stash = stash_dir(&t.common_dir, SystemTime::now()).unwrap();
    // Another Git process replaced it after the check.
    age_file(&lock, Duration::from_secs(10));
    assert!(move_aside(&lock, checked, &t.common_dir, &stash).is_err());
    assert!(lock.is_file());
    // Its owner released it.
    std::fs::remove_file(&lock).unwrap();
    assert_eq!(move_aside(&lock, checked, &t.common_dir, &stash), Ok(None));
    // A second pass in the same second gets its own directory.
    let again = stash_dir(&t.common_dir, SystemTime::now()).unwrap();
    assert_ne!(again, stash);
}

#[test]
fn hs2_t0v269_reads_lsof_field_output() {
    let output = "p100\ncgit\nfcwd\nn/repo/store\nf3\nn/repo/store/.git/index.lock\nftxt\n\
                  n/usr/bin/git\np200\nczsh\nfcwd\nn/home\nf0\nn/dev/ttys001\n";
    let processes = parse_lsof(output);
    assert_eq!(processes.len(), 2);
    assert_eq!(processes[0].pid, 100);
    assert_eq!(processes[0].command, "git");
    assert_eq!(processes[0].cwd.as_deref(), Some(Path::new("/repo/store")));
    assert!(
        processes[0]
            .open
            .contains(&PathBuf::from("/repo/store/.git/index.lock"))
    );
    assert_eq!(processes[1].command, "zsh");
    assert_eq!(processes[1].cwd.as_deref(), Some(Path::new("/home")));
}

#[test]
fn hs2_t0v269_kept_lines_group_locks_kept_for_the_same_reason() {
    let recovery = Recovery {
        moved: Vec::new(),
        kept: vec![
            (".git/index.lock".into(), Kept::TooYoung { age_seconds: 5 }),
            (".git/HEAD.lock".into(), Kept::TooYoung { age_seconds: 9 }),
            (
                ".git/refs/heads/main.lock".into(),
                Kept::NotMoved("denied".into()),
            ),
        ],
    };
    let lines = recovery.kept_lines();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].starts_with("Not moved automatically: .git/index.lock, .git/HEAD.lock is"));
    assert!(lines[1].ends_with("could not be moved: denied."));
}

/// Without a process scan (Windows), recovery refuses rather than guessing: an abandoned
/// lock stays and is reported as not proven stale.
#[cfg(not(unix))]
#[test]
fn hs2_t0v269_recovery_refuses_where_no_process_scan_exists() {
    let (_dir, store) = repo();
    let _transaction = crate::store::lock_git_transaction(&store).unwrap();
    let lock = store.join(".git/index.lock");
    plant_old(&lock);
    let recovery = recover(&store, std::slice::from_ref(&lock));
    assert!(recovery.moved.is_empty());
    assert!(matches!(recovery.kept[..], [(_, Kept::Unverifiable(_))]));
    assert!(lock.is_file());
}

/// The platform scan for real, while this process holds the store's coordination lock as
/// the cycle does: an unheld old lock moves; a live process holding it keeps it.
#[cfg(unix)]
#[test]
fn hs2_t0v269_the_platform_scan_sees_a_live_holder_and_proves_an_abandoned_lock() {
    let (_dir, store) = repo();
    let _transaction = crate::store::lock_git_transaction(&store).unwrap();
    let lock = store.join(".git/index.lock");
    plant_old(&lock);

    let holder = hold_open(&lock);
    let pid = holder.id();
    let recovery = recover(&store, std::slice::from_ref(&lock));
    stop(holder);
    let [(_, Kept::Held(holders))] = &recovery.kept[..] else {
        panic!("{:?}", recovery.kept);
    };
    assert!(
        holders.iter().any(|holder| holder.pid == pid),
        "{holders:?}"
    );
    assert!(lock.is_file());

    let git = git_running_in(&store);
    let pid = git.id();
    let recovery = recover(&store, std::slice::from_ref(&lock));
    stop(git);
    let [(_, Kept::Held(holders))] = &recovery.kept[..] else {
        panic!("{:?}", recovery.kept);
    };
    assert!(
        holders.iter().any(|holder| holder.pid == pid),
        "{holders:?}"
    );
    assert!(lock.is_file());

    // Both holders are gone: the lock is proven stale and moved.
    let recovery = recover(&store, std::slice::from_ref(&lock));
    assert!(recovery.kept.is_empty(), "{:?}", recovery.kept);
    assert_eq!(recovery.moved.len(), 1);
    assert!(!lock.exists());
}
