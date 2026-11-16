//! HS2-T0V269: a sync cycle moves a provably stale Git lock aside and syncs in the same
//! pass; a lock a live process holds, or a young one, stays and blocks the pass until it
//! is released; and the headless recovery does the same without syncing. Synthetic bare
//! remotes and synthetic file bodies only.

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::tests::{commit_file, head, peer_pushes, remote_with_two_clones};
use super::*;
use crate::sync_health::{SyncState, health, read_record};
use crate::sync_stale_locks::tests::{age_file, git_running_in, hold_open, plant_old, stop};

fn live() -> SyncLimits {
    SyncLimits {
        replay: ReplayMode::LiveRebase,
        ..SyncLimits::default()
    }
}

fn branch(store: &Path) -> String {
    git_stdout(store, &["symbolic-ref", "--quiet", "--short", "HEAD"])
        .unwrap()
        .trim()
        .to_string()
}

/// The four locks the pre-check covers, with the store-relative name a reason shows.
fn needed_locks(store: &Path) -> [(PathBuf, String); 4] {
    let branch = branch(store);
    [
        ".git/index.lock".to_string(),
        format!(".git/refs/remotes/origin/{branch}.lock"),
        format!(".git/refs/heads/{branch}.lock"),
        ".git/HEAD.lock".to_string(),
    ]
    .map(|shown| (store.join(&shown), shown))
}

fn stash_dirs(store: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(
        store
            .join(".git")
            .join(crate::sync_stale_locks::STALE_LOCK_DIR),
    )
    .map(|entries| entries.flatten().map(|entry| entry.path()).collect())
    .unwrap_or_default();
    dirs.sort();
    dirs
}

#[test]
fn hs2_t0v269_an_abandoned_lock_is_moved_aside_and_the_same_pass_syncs() {
    for limits in [SyncLimits::default(), live()] {
        for at in 0..4 {
            let (_dir, store, peer) = remote_with_two_clones();
            commit_file(&store, "local.md", "local\n");
            peer_pushes(&peer, "incoming.md");
            let (lock, shown) = needed_locks(&store)[at].clone();
            plant_old(&lock);

            assert_eq!(
                sync_once_with(&store, limits),
                SyncReport::Synced {
                    pulled: true,
                    pushed: true
                },
                "{shown} {:?}",
                limits.replay
            );
            assert!(store.join("incoming.md").is_file(), "{shown}");
            assert!(!lock.exists(), "{shown}");
            // Moved, not deleted, with its relative path kept.
            let relative = shown.strip_prefix(".git/").unwrap();
            let dirs = stash_dirs(&store);
            assert_eq!(dirs.len(), 1, "{shown}");
            assert!(dirs[0].join(relative).is_file(), "{shown}");
            let synced = health(&store);
            assert_eq!(synced.state, SyncState::Synced);
            assert_eq!(synced.consecutive_failures, 0);
            let reason = synced.reason.unwrap_or_default();
            assert!(
                reason.starts_with(&format!("Moved a stale Git lock aside: {shown} to ")),
                "{reason}"
            );
            let recovery = read_record(&store).unwrap().stale_lock_recovery.unwrap();
            assert_eq!(recovery.moved.len(), 1);
            assert_eq!(recovery.moved[0].from, shown);

            // The next pass has nothing to recover; the durable record stays.
            assert_eq!(sync_once_with(&store, limits), SyncReport::UpToDate);
            assert_eq!(health(&store).reason, None);
            assert!(read_record(&store).unwrap().stale_lock_recovery.is_some());
            assert_eq!(stash_dirs(&store).len(), 1);
        }
    }
}

#[test]
fn hs2_t0v269_a_lock_a_live_process_holds_blocks_until_it_exits() {
    let (_dir, store, peer) = remote_with_two_clones();
    commit_file(&store, "local.md", "local\n");
    peer_pushes(&peer, "incoming.md");
    let before = head(&store);
    let lock = store.join(".git/index.lock");
    plant_old(&lock);

    let holder = hold_open(&lock);
    let pid = holder.id();
    // Repeated passes while it runs never move it.
    for pass in 1..=2 {
        assert_eq!(sync_once(&store), SyncReport::Blocked);
        assert!(lock.is_file());
        assert_eq!(head(&store), before);
        assert!(integration_in_progress(&store).is_none());
        let blocked = health(&store);
        assert_eq!(blocked.state, SyncState::Blocked);
        assert_eq!(blocked.consecutive_failures, pass);
        let reason = blocked.reason.unwrap_or_default();
        assert!(
            reason.contains(&format!("may still be in use: process {pid} (")),
            "{reason}"
        );
    }
    assert!(stash_dirs(&store).is_empty());
    assert!(read_record(&store).unwrap().stale_lock_recovery.is_none());

    stop(holder);
    assert_eq!(
        sync_once(&store),
        SyncReport::Synced {
            pulled: true,
            pushed: true
        }
    );
    assert!(!lock.exists());
    assert_eq!(stash_dirs(&store).len(), 1);
}

#[test]
fn hs2_t0v269_a_git_process_running_in_the_store_keeps_its_lock() {
    let (_dir, store, peer) = remote_with_two_clones();
    peer_pushes(&peer, "incoming.md");
    let lock = store.join(".git/index.lock");
    plant_old(&lock);

    let git = git_running_in(&store);
    let pid = git.id();
    let report = sync_once(&store);
    let reason = health(&store).reason.unwrap_or_default();
    stop(git);
    assert_eq!(report, SyncReport::Blocked);
    assert!(lock.is_file());
    assert!(reason.contains(&format!("process {pid} (git")), "{reason}");

    assert_eq!(
        sync_once(&store),
        SyncReport::Synced {
            pulled: true,
            pushed: false
        }
    );
    assert!(!lock.exists());
}

/// One lock is old and one is young: the old one moves, the pass stays blocked on the
/// young one and says both; once that one ages, the next pass moves it into its own
/// directory and syncs.
#[test]
fn hs2_t0v269_a_young_lock_still_blocks_after_an_old_one_moves() {
    let (_dir, store, peer) = remote_with_two_clones();
    peer_pushes(&peer, "incoming.md");
    let old = store.join(".git/index.lock");
    plant_old(&old);
    let young = store.join(".git/HEAD.lock");
    std::fs::write(&young, "").unwrap();

    assert_eq!(sync_once(&store), SyncReport::Blocked);
    assert!(!old.exists());
    assert!(young.is_file());
    let reason = health(&store).reason.unwrap_or_default();
    // The failure leads; the move is reported after it.
    assert!(
        reason
            .starts_with("a Git lock file blocked sync during the pre-sync check: .git/HEAD.lock"),
        "{reason}"
    );
    assert!(
        reason.contains("\nMoved a stale Git lock aside: .git/index.lock to "),
        "{reason}"
    );
    assert!(reason.contains("younger than 10 minutes"), "{reason}");
    let record = read_record(&store).unwrap();
    assert_eq!(record.consecutive_failures, 1);
    assert_eq!(record.stale_lock_recovery.unwrap().moved.len(), 1);

    age_file(&young, Duration::from_secs(3600));
    assert_eq!(
        sync_once(&store),
        SyncReport::Synced {
            pulled: true,
            pushed: false
        }
    );
    assert!(!young.exists());
    let dirs = stash_dirs(&store);
    assert_eq!(dirs.len(), 2, "each pass keeps its own directory");
    assert!(dirs.iter().any(|dir| dir.join("HEAD.lock").is_file()));
    assert!(dirs.iter().any(|dir| dir.join("index.lock").is_file()));
}

#[test]
fn hs2_t0v269_headless_recovery_moves_and_reports_without_syncing() {
    let (_dir, store, peer) = remote_with_two_clones();
    peer_pushes(&peer, "incoming.md");
    assert_eq!(
        recover_stale_locks(&store).unwrap(),
        StaleLockPass::default()
    );

    let lock = store.join(".git/index.lock");
    plant_old(&lock);
    let young = store.join(".git/HEAD.lock");
    std::fs::write(&young, "").unwrap();
    let pass = recover_stale_locks(&store).unwrap();
    assert_eq!(pass.held, [".git/index.lock", ".git/HEAD.lock"]);
    assert_eq!(pass.recovery.moved.len(), 1);
    assert_eq!(pass.recovery.moved[0].from, ".git/index.lock");
    assert_eq!(pass.remaining, [".git/HEAD.lock"]);
    assert!(!lock.exists());
    // Nothing was fetched or merged.
    assert!(!store.join("incoming.md").exists());
    let recorded = read_record(&store).unwrap().stale_lock_recovery.unwrap();
    assert_eq!(recorded.moved, pass.recovery.moved);
}
