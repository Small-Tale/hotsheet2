//! HS2-6ACT5Y: a Git lock file left in a ticket store is reported as `Blocked`, never as a
//! merge conflict or an outage; it never leaves a rebase half-way; and the first cycle
//! after the lock is removed integrates and publishes as usual. Synthetic bare remotes and
//! synthetic file bodies only.

use std::path::{Path, PathBuf};

use super::tests::{commit_file, head, ok, peer_pushes, remote_with_two_clones};
use super::*;
use crate::sync_health::{SyncState, health, read_record};
use crate::sync_replay::BEFORE_BRANCH_MOVE;

fn live() -> SyncLimits {
    SyncLimits {
        replay: ReplayMode::LiveRebase,
        ..SyncLimits::default()
    }
}

fn both_modes() -> [SyncLimits; 2] {
    [SyncLimits::default(), live()]
}

fn plant(lock: &Path) {
    std::fs::create_dir_all(lock.parent().unwrap()).unwrap();
    std::fs::write(lock, "stale\n").unwrap();
}

/// The store's branch, whatever the test Git's default branch name is.
fn branch(store: &Path) -> String {
    git_stdout(store, &["symbolic-ref", "--quiet", "--short", "HEAD"])
        .unwrap()
        .trim()
        .to_string()
}

fn tracking_lock(store: &Path) -> PathBuf {
    store.join(format!(".git/refs/remotes/origin/{}.lock", branch(store)))
}

/// The incident's locks, each on its own: the index and the remote-tracking ref, plus the
/// branch and `HEAD` a rebase moves. Each with the store-relative name the reason shows.
fn incident_locks(store: &Path) -> [(PathBuf, String); 4] {
    let branch = branch(store);
    [
        ".git/index.lock".to_string(),
        format!(".git/refs/remotes/origin/{branch}.lock"),
        format!(".git/refs/heads/{branch}.lock"),
        ".git/HEAD.lock".to_string(),
    ]
    .map(|shown| (store.join(&shown), shown))
}

#[test]
fn hs2_6act5y_a_stale_lock_blocks_sync_without_a_conflict_and_recovers_once_removed() {
    for limits in both_modes() {
        for at in 0..4 {
            let (_dir, store, peer) = remote_with_two_clones();
            commit_file(&store, "local.md", "local\n");
            peer_pushes(&peer, "incoming.md");
            let before = head(&store);
            let (lock, shown) = incident_locks(&store)[at].clone();
            plant(&lock);

            let report = sync_once_with(&store, limits);
            assert_eq!(report, SyncReport::Blocked, "{shown} {:?}", limits.replay);
            // Nothing was merged or started, and the branch is where it was.
            assert!(integration_in_progress(&store).is_none(), "{shown}");
            assert!(
                git_ok(&store, &["symbolic-ref", "--quiet", "HEAD"]),
                "{shown}"
            );
            assert_eq!(head(&store), before, "{shown}");
            assert!(!store.join("incoming.md").exists(), "{shown}");
            let blocked = health(&store);
            assert_eq!(blocked.state, SyncState::Blocked, "{shown}");
            let reason = blocked.reason.clone().unwrap_or_default();
            assert!(reason.contains(&shown), "{shown}: {reason}");
            assert!(reason.contains("created just now"), "{reason}");
            assert!(!reason.contains("merged automatically"), "{reason}");
            let action = blocked.next_action.unwrap_or_default();
            assert!(action.contains("no Git process is running"), "{action}");
            assert!(action.contains("hotsheet-cli sync"), "{action}");

            std::fs::remove_file(&lock).unwrap();
            assert_eq!(
                sync_once_with(&store, limits),
                SyncReport::Synced {
                    pulled: true,
                    pushed: true
                },
                "{shown}"
            );
            assert!(store.join("incoming.md").is_file());
            let healthy = health(&store);
            assert_eq!(healthy.state, SyncState::Synced);
            assert_eq!(healthy.consecutive_failures, 0);
            assert_eq!(healthy.reason, None);
        }
    }
}

#[test]
fn hs2_6act5y_repeated_blocked_passes_count_as_consecutive_failures() {
    let (_dir, store, peer) = remote_with_two_clones();
    peer_pushes(&peer, "incoming.md");
    plant(&store.join(".git/index.lock"));
    for pass in 1..=3 {
        assert_eq!(sync_once(&store), SyncReport::Blocked);
        let record = read_record(&store).unwrap();
        assert_eq!(record.outcome, Some(SyncState::Blocked));
        assert_eq!(record.consecutive_failures, pass);
    }
    assert!(integration_in_progress(&store).is_none());
    std::fs::remove_file(store.join(".git/index.lock")).unwrap();
    assert_eq!(
        sync_once(&store),
        SyncReport::Synced {
            pulled: true,
            pushed: false
        }
    );
    assert_eq!(read_record(&store).unwrap().consecutive_failures, 0);
}

/// A lock the pre-check does not cover (a new remote branch's tracking ref) is read from
/// the fetch's own output.
#[test]
fn hs2_6act5y_a_lock_named_by_a_failed_fetch_is_blocked_not_offline() {
    let (_dir, store, peer) = remote_with_two_clones();
    ok(&peer, &["checkout", "--quiet", "-b", "feature"]);
    commit_file(&peer, "feature.md", "feature\n");
    ok(&peer, &["push", "--quiet", "-u", "origin", "feature"]);
    let lock = store.join(".git/refs/remotes/origin/feature.lock");
    plant(&lock);
    assert_eq!(sync_once(&store), SyncReport::Blocked);
    let reason = health(&store).reason.unwrap_or_default();
    assert!(reason.contains("during fetch"), "{reason}");
    assert!(
        reason.contains("refs/remotes/origin/feature.lock"),
        "{reason}"
    );
    std::fs::remove_file(lock).unwrap();
    assert_eq!(sync_once(&store), SyncReport::UpToDate);
}

/// A lock another Git process takes after the pre-check, right before the replayed branch
/// moves, stops the cycle with nothing moved and no rebase in progress.
#[test]
fn hs2_6act5y_a_lock_taken_during_the_replay_leaves_the_branch_untouched() {
    if !crate::sync_replay::isolated_replay_supported() {
        return;
    }
    let (_dir, store, peer) = remote_with_two_clones();
    commit_file(&store, "local.md", "local\n");
    peer_pushes(&peer, "incoming.md");
    let before = head(&store);
    let lock = store.join(".git/index.lock");
    {
        let lock = lock.clone();
        BEFORE_BRANCH_MOVE.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || plant(&lock)));
        });
    }
    let report = sync_once(&store);
    BEFORE_BRANCH_MOVE.with(|hook| hook.borrow_mut().take());
    assert_eq!(report, SyncReport::Blocked);
    assert_eq!(head(&store), before);
    assert!(integration_in_progress(&store).is_none());
    let reason = health(&store).reason.unwrap_or_default();
    assert!(reason.contains("during the branch update"), "{reason}");
    std::fs::remove_file(lock).unwrap();
    assert_eq!(
        sync_once(&store),
        SyncReport::Synced {
            pulled: true,
            pushed: true
        }
    );
}

#[test]
fn hs2_6act5y_a_local_lock_in_push_output_is_blocked_but_a_remote_lock_is_not() {
    let (_dir, store, _peer) = remote_with_two_clones();
    let local = format!(
        "error: update_ref failed for ref 'refs/remotes/origin/main': cannot lock ref \
         'refs/remotes/origin/main': Unable to create '{}': File exists.\n",
        tracking_lock(&store).display()
    );
    // HS2-8BSDBA: only a lock still present after the settle window blocks the push.
    plant(&tracking_lock(&store));
    let (report, detail) = classify_push_failure(&store, &local, SyncLimits::default());
    assert_eq!(report, SyncReport::Blocked);
    assert!(
        detail.reason.unwrap_or_default().contains("during push"),
        "names the step"
    );
    let remote = "remote: error: cannot lock ref 'refs/heads/main': Unable to create \
                  '/srv/r.git/refs/heads/main.lock': File exists.\n \
                  ! [remote rejected] main -> main (reference already exists)\n";
    let (report, _) = classify_push_failure(&store, remote, SyncLimits::default());
    assert_ne!(report, SyncReport::Blocked);
}

/// The recorded lock is what the health reports even when the store also has an
/// unfinished rebase: that rebase cannot continue until the lock is gone.
#[test]
fn hs2_6act5y_a_recorded_lock_outranks_an_unfinished_rebase_in_health() {
    let (_dir, store, peer) = remote_with_two_clones();
    commit_file(&store, "local.md", "local\n");
    peer_pushes(&peer, "incoming.md");
    ok(&store, &["fetch", "--quiet"]);
    // A paused interactive rebase stands in for one a person left.
    ok(
        &store,
        &[
            "-c",
            "sequence.editor=sed -i.bak 1s/^pick/edit/",
            "rebase",
            "--quiet",
            "-i",
            "@{u}",
        ],
    );
    assert!(integration_in_progress(&store).is_some());
    plant(&store.join(".git/index.lock"));
    assert_eq!(sync_once(&store), SyncReport::Blocked);
    assert_eq!(health(&store).state, SyncState::Blocked);
    std::fs::remove_file(store.join(".git/index.lock")).unwrap();
    // Without the lock the paused rebase needs a decision again.
    assert_eq!(sync_once(&store), SyncReport::RebaseInProgress);
    assert_eq!(health(&store).state, SyncState::RebaseInProgress);
}
