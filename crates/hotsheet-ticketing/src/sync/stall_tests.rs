//! HS2-QR1K8R: a ticket write during a sync replay must not stall the store, nothing
//! commits onto a detached `HEAD`, and an unfinished rebase that needs no judgment is
//! continued. Synthetic bare remotes and synthetic file bodies only.

use std::path::Path;

use super::tests::{clean, commit_file, head, ok, peer_pushes, reflog_len, remote_with_two_clones};
use super::*;
use crate::store::{FsStore, StoreMetadata};
use crate::sync_health::{IntegrationInProgress, SyncState, health, read_record};
use crate::sync_replay::{BEFORE_BRANCH_MOVE, MAX_CONTINUE_ATTEMPTS};

fn live() -> SyncLimits {
    SyncLimits {
        replay: ReplayMode::LiveRebase,
        ..SyncLimits::default()
    }
}

fn both_modes() -> [SyncLimits; 2] {
    [SyncLimits::default(), live()]
}

fn attached(root: &Path) -> bool {
    git_ok(root, &["symbolic-ref", "--quiet", "HEAD"])
}

fn unfinished(root: &Path) -> bool {
    integration_in_progress(root).is_some()
}

fn read(root: &Path, file: &str) -> String {
    std::fs::read_to_string(root.join(file)).unwrap_or_default()
}

fn backup_refs(root: &Path) -> Vec<String> {
    git_stdout(
        root,
        &[
            "for-each-ref",
            "--format=%(objectname)",
            "refs/hotsheet/sync-backup",
        ],
    )
    .unwrap_or_default()
    .lines()
    .map(str::to_string)
    .collect()
}

/// A store whose Markdown files merge through Git's built-in `union` driver, standing in
/// for the semantic ticket driver: two sides that changed one file merge without a
/// conflict. The store is published and in sync.
fn merging_store() -> (
    tempfile::TempDir,
    std::path::PathBuf,
    std::path::PathBuf,
    FsStore,
) {
    let (dir, root, peer) = remote_with_two_clones();
    let store = FsStore::init(&root, &StoreMetadata::new("HS"))
        .unwrap()
        .with_deferred_push();
    std::fs::write(root.join(".gitattributes"), "*.md merge=union\n").unwrap();
    assert!(store.autocommit("init store").unwrap());
    assert!(matches!(
        sync_once(&root),
        SyncReport::Synced { .. } | SyncReport::UpToDate
    ));
    ok(&peer, &["pull", "--quiet"]);
    (dir, root, peer, store)
}

/// Two unpublished local commits (the second creates `new-ticket.md`) and one incoming
/// commit: the divergence of the incident.
fn diverge(root: &Path, peer: &Path, round: u32) {
    commit_file(root, "seed.md", &format!("local edit {round}\n"));
    commit_file(root, &new_ticket(round), &format!("created {round}\n"));
    peer_pushes(peer, &format!("incoming-{round}.md"));
}

/// The file the second local commit of a round creates.
fn new_ticket(round: u32) -> String {
    if round == 1 {
        "new-ticket.md".to_string()
    } else {
        format!("new-ticket-{round}.md")
    }
}

/// Another process writes `file` while the replay has the file out of the tree:
/// a `post-commit` hook, which Git runs as a separate process after the first pick.
#[cfg(unix)]
fn write_after_first_pick(root: &Path, file: &str, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    let hook = root.join(".git/hooks/post-commit");
    let _ = std::fs::remove_file(root.join(".git/hotsheet-test-fired"));
    std::fs::write(
        &hook,
        format!(
            "#!/bin/sh\nif [ ! -e .git/hotsheet-test-fired ]; then\n  touch \
             .git/hotsheet-test-fired\n  printf '%s\\n' '{body}' > {file}\nfi\n"
        ),
    )
    .unwrap();
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// Leave the store exactly as the incident did, with Git alone: the pick rescheduled, the
/// abort refused, and (as an older Hot Sheet did) the write committed onto the detached
/// `HEAD`. Returns that detached commit.
#[cfg(unix)]
fn incident_state(root: &Path, peer: &Path, round: u32) -> String {
    diverge(root, peer, round);
    write_after_first_pick(
        root,
        &new_ticket(round),
        &format!("created and edited {round}"),
    );
    ok(root, &["fetch", "--quiet"]);
    assert!(!git_ok(root, &["rebase", "--quiet", "@{u}"]));
    assert!(!git_ok(root, &["rebase", "--abort"]));
    ok(root, &["add", "-A"]);
    ok(root, &["commit", "--quiet", "-m", "write on the replay"]);
    assert_incident_shape(root);
    assert!(clean(root));
    head(root)
}

/// The signature recorded in the ticket: `rebase-merge` exists, the same pick is the last
/// line of `done` and the only line of the todo, no `stopped-sha`, no unmerged path, and
/// `HEAD` is detached.
fn assert_incident_shape(root: &Path) {
    let state = root.join(".git/rebase-merge");
    assert!(state.is_dir(), "no unfinished rebase");
    let done = std::fs::read_to_string(state.join("done")).unwrap();
    let todo = std::fs::read_to_string(state.join("git-rebase-todo")).unwrap();
    let todo: Vec<&str> = todo.lines().filter(|line| !line.is_empty()).collect();
    assert_eq!(todo.len(), 1, "{todo:?}");
    assert_eq!(
        done.lines().last(),
        Some(todo[0]),
        "the pick is rescheduled"
    );
    assert!(!state.join("stopped-sha").exists());
    assert!(unmerged_paths(root).is_empty());
    assert!(!attached(root));
}

// ---- the replay no longer happens in the working tree -------------------------------

/// The incident's interleaving against the default cycle: a write to a file that only an
/// unpublished local commit contains lands between the replay and the branch move. The
/// store never shows a rebase, `HEAD` stays on the branch, and the write is kept.
#[test]
fn hs2_qr1k8r_a_write_during_the_replay_cannot_stall_the_store() {
    let (_dir, root, peer, store) = merging_store();
    diverge(&root, &peer, 1);
    let before = head(&root);
    let observed = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    {
        let (root, observed) = (root.clone(), observed.clone());
        BEFORE_BRANCH_MOVE.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                // Mid-replay the live tree still has every local file and the branch.
                observed.lock().unwrap().push((
                    root.join("new-ticket.md").is_file(),
                    attached(&root),
                    unfinished(&root),
                    head(&root),
                ));
                std::fs::write(root.join("new-ticket.md"), "created and edited\n").unwrap();
            }));
        });
    }
    let report = sync_once(&root);
    BEFORE_BRANCH_MOVE.with(|hook| hook.borrow_mut().take());
    assert_eq!(
        report,
        SyncReport::Synced {
            pulled: true,
            pushed: true
        }
    );
    assert_eq!(
        *observed.lock().unwrap(),
        [(true, true, false, before)],
        "one replay, with the live tree untouched while it ran"
    );
    assert!(attached(&root));
    assert!(!unfinished(&root));
    assert!(root.join("incoming-1.md").is_file());
    assert_eq!(read(&root, "new-ticket.md"), "created and edited\n");
    // The waiting autocommit lands on the branch and the next cycle publishes it.
    assert!(
        store
            .autocommit_paths("edit", &[root.join("new-ticket.md")])
            .unwrap()
    );
    assert!(attached(&root));
    assert_eq!(
        sync_once(&root),
        SyncReport::Synced {
            pulled: false,
            pushed: true
        }
    );
    ok(&peer, &["pull", "--quiet"]);
    assert_eq!(read(&peer, "new-ticket.md"), "created and edited\n");
    assert_eq!(read(&peer, "seed.md"), "local edit 1\n");
    assert!(backup_refs(&root).is_empty(), "nothing needed recovery");
}

/// A write that touches a path the incoming commits change makes the branch move refuse
/// with nothing changed; the cycle commits the write and replays again.
#[test]
fn hs2_qr1k8r_a_write_to_an_incoming_path_is_committed_and_replayed_again() {
    let (_dir, root, peer, _store) = merging_store();
    diverge(&root, &peer, 1);
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    {
        let (root, calls) = (root.clone(), calls.clone());
        BEFORE_BRANCH_MOVE.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                if calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                    std::fs::write(root.join("incoming-1.md"), "written locally\n").unwrap();
                }
            }));
        });
    }
    let report = sync_once(&root);
    BEFORE_BRANCH_MOVE.with(|hook| hook.borrow_mut().take());
    assert_eq!(
        report,
        SyncReport::Synced {
            pulled: true,
            pushed: true
        }
    );
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
    assert!(attached(&root) && !unfinished(&root) && clean(&root));
    let merged = read(&root, "incoming-1.md");
    assert!(
        merged.contains("written locally") && merged.contains("from peer"),
        "{merged}"
    );
    ok(&peer, &["pull", "--quiet"]);
    assert_eq!(read(&peer, "incoming-1.md"), merged);
}

/// Writes that keep landing on incoming paths end the cycle as a retryable outcome after
/// a bounded number of replays. Nothing is lost and no rebase is left behind.
#[test]
fn hs2_qr1k8r_replays_are_bounded_when_writes_keep_landing_on_incoming_paths() {
    let (_dir, root, peer, _store) = merging_store();
    diverge(&root, &peer, 1);
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    {
        let (root, calls) = (root.clone(), calls.clone());
        BEFORE_BRANCH_MOVE.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                let n = calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                std::fs::write(root.join("incoming-1.md"), format!("local write {n}\n")).unwrap();
            }));
        });
    }
    let report = sync_once(&root);
    BEFORE_BRANCH_MOVE.with(|hook| hook.borrow_mut().take());
    assert_eq!(report, SyncReport::Offline);
    assert_eq!(
        calls.load(std::sync::atomic::Ordering::SeqCst),
        MAX_REPLAY_ATTEMPTS
    );
    assert!(attached(&root) && !unfinished(&root));
    let stopped = health(&root);
    assert_eq!(stopped.state, SyncState::Offline);
    assert!(
        stopped
            .reason
            .unwrap()
            .contains("local writes kept changing"),
        "the reason names the cause"
    );
    assert_eq!(read(&root, "incoming-1.md"), "local write 2\n");
    // Quiet again: the next cycle settles and publishes every write.
    assert_eq!(
        sync_once(&root),
        SyncReport::Synced {
            pulled: true,
            pushed: true
        }
    );
    assert!(clean(&root));
    ok(&peer, &["pull", "--quiet"]);
    let merged = read(&peer, "incoming-1.md");
    for n in 0..MAX_REPLAY_ATTEMPTS {
        assert!(merged.contains(&format!("local write {n}")), "{merged}");
    }
}

/// A conflict found by the isolated replay leaves the store exactly as it was: no rebase
/// to abort, the branch and reflog untouched, and the same divergence is not replayed.
#[test]
fn hs2_qr1k8r_a_conflict_leaves_no_rebase_and_the_working_tree_untouched() {
    let (_dir, root, peer) = remote_with_two_clones();
    commit_file(&root, "shared.md", "local body\n");
    commit_file(&root, "other.md", "second local commit\n");
    std::fs::write(peer.join("shared.md"), "peer body\n").unwrap();
    ok(&peer, &["add", "-A"]);
    ok(&peer, &["commit", "--quiet", "-m", "peer shared"]);
    ok(&peer, &["push", "--quiet"]);
    let (before, reflog) = (head(&root), reflog_len(&root));
    for pass in 1..=3 {
        assert_eq!(sync_once(&root), SyncReport::Conflict);
        assert_eq!(head(&root), before);
        assert_eq!(reflog_len(&root), reflog);
        assert!(attached(&root) && !unfinished(&root) && clean(&root));
        assert_eq!(health(&root).consecutive_failures, pass);
    }
    assert_eq!(read(&root, "shared.md"), "local body\n");
}

/// A history the isolated replay does not handle (a local root commit) falls back to
/// `git rebase` and still settles.
#[test]
fn hs2_qr1k8r_an_unrelated_local_root_falls_back_to_git_rebase() {
    let (dir, _store, _peer) = remote_with_two_clones();
    let root = dir.path().join("fresh");
    std::fs::create_dir(&root).unwrap();
    let remote = dir.path().join("remote.git");
    let branch = git_stdout(&remote, &["symbolic-ref", "--short", "HEAD"]).unwrap();
    ok(&root, &["init", "--quiet", "-b", branch.trim()]);
    super::tests::identity(&root);
    commit_file(&root, "local-root.md", "a root commit\n");
    ok(
        &root,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    ok(&root, &["fetch", "--quiet"]);
    let upstream = format!("origin/{}", branch.trim());
    ok(
        &root,
        &["branch", "--quiet", "--set-upstream-to", &upstream],
    );
    assert_eq!(
        crate::sync_replay::replay_onto_upstream(&root),
        Err(crate::sync_replay::ReplayError::Unsupported(
            "a local root commit".to_string()
        ))
    );
    assert_eq!(
        sync_once(&root),
        SyncReport::Synced {
            pulled: true,
            pushed: true
        }
    );
    assert!(root.join("seed.md").is_file() && root.join("local-root.md").is_file());
    assert!(attached(&root) && !unfinished(&root));
}

/// The replayed commits keep their author, author date, and exact message; commits the
/// upstream already has are dropped, as `git rebase` does.
#[test]
fn hs2_qr1k8r_replayed_commits_keep_author_date_and_message() {
    let (_dir, root, peer) = remote_with_two_clones();
    std::fs::write(root.join("a.md"), "a\n").unwrap();
    ok(&root, &["add", "-A"]);
    assert!(
        crate::git::command()
            .arg("-C")
            .arg(&root)
            .args(["commit", "--quiet", "-m", "Subject line\n\nBody line."])
            .env("GIT_AUTHOR_NAME", "Other Author")
            .env("GIT_AUTHOR_EMAIL", "other@test.invalid")
            .env("GIT_AUTHOR_DATE", "@1700000000 -0800")
            .status()
            .unwrap()
            .success()
    );
    // The peer publishes the same change the store's second commit makes.
    commit_file(&root, "same.md", "from peer\n");
    peer_pushes(&peer, "same.md");
    assert_eq!(
        sync_once(&root),
        SyncReport::Synced {
            pulled: true,
            pushed: true
        }
    );
    let log = git_stdout(
        &root,
        &["log", "--format=%an|%ae|%ad|%B", "--date=raw", "-2"],
    )
    .unwrap();
    assert!(
        log.starts_with(
            "Other Author|other@test.invalid|1700000000 -0800|Subject line\n\nBody line.\n"
        ),
        "{log}"
    );
    // Seed, the peer's commit, and the one replayed commit: the duplicate was dropped.
    assert_eq!(
        git_stdout(&root, &["rev-list", "--count", "HEAD"])
            .unwrap()
            .trim(),
        "3"
    );
    assert!(clean(&root));
}

// ---- the incident, on the path an older Git still uses ------------------------------

/// The regression test of the incident: two local commits, an incoming commit, and
/// another process writing a file the second pick creates while `git rebase` has it out
/// of the tree. Before the fix this ended in an unfinished `rebase-merge` with the pick
/// rescheduled, an autocommit on the detached `HEAD`, and `rebase_in_progress` forever.
#[cfg(unix)]
#[test]
fn hs2_qr1k8r_a_write_during_a_live_rebase_is_continued_not_stalled() {
    let (_dir, root, peer, store) = merging_store();
    diverge(&root, &peer, 1);
    write_after_first_pick(&root, "new-ticket.md", "created and edited");
    let report = sync_once_with(&root, live());
    assert!(
        root.join(".git/hotsheet-test-fired").exists(),
        "the write landed mid-replay"
    );
    assert_eq!(
        report,
        SyncReport::Synced {
            pulled: true,
            pushed: true
        }
    );
    assert!(attached(&root) && !unfinished(&root) && clean(&root));
    let merged = read(&root, "new-ticket.md");
    assert!(merged.contains("created and edited"), "{merged}");
    assert_eq!(
        backup_refs(&root).len(),
        1,
        "the continue kept a backup ref"
    );
    assert_eq!(health(&root).state, SyncState::Synced);
    // A later write commits on the branch, never on a detached HEAD.
    std::fs::write(root.join("later.md"), "later\n").unwrap();
    assert!(
        store
            .autocommit_paths("later", &[root.join("later.md")])
            .unwrap()
    );
    assert!(attached(&root));
    assert_eq!(
        sync_once_with(&root, live()),
        SyncReport::Synced {
            pulled: false,
            pushed: true
        }
    );
    ok(&peer, &["pull", "--quiet"]);
    assert_eq!(read(&peer, "new-ticket.md"), merged);
    assert!(peer.join("later.md").is_file());
}

/// The same race on a file the upstream already tracks. Git reschedules the pick here
/// too, and the old abort then succeeded and silently discarded the write.
#[cfg(unix)]
#[test]
fn hs2_qr1k8r_a_write_to_a_tracked_file_during_a_live_rebase_is_kept() {
    let (_dir, root, peer, _store) = merging_store();
    commit_file(&root, "first.md", "first local commit\n");
    commit_file(&root, "seed.md", "local edit\n");
    peer_pushes(&peer, "incoming.md");
    write_after_first_pick(&root, "seed.md", "edited by another process");
    assert_eq!(
        sync_once_with(&root, live()),
        SyncReport::Synced {
            pulled: true,
            pushed: true
        }
    );
    assert!(attached(&root) && !unfinished(&root) && clean(&root));
    let merged = read(&root, "seed.md");
    assert!(merged.contains("edited by another process"), "{merged}");
    ok(&peer, &["pull", "--quiet"]);
    assert_eq!(read(&peer, "seed.md"), merged);
}

/// When the continued pick then really conflicts, the cycle reports the stall with its
/// reason, keeps a backup ref, discards nothing, and does not try again.
#[cfg(unix)]
#[test]
fn hs2_qr1k8r_a_continue_that_conflicts_is_reported_and_never_stacked() {
    let (_dir, root, peer) = remote_with_two_clones();
    let store = FsStore::init(&root, &StoreMetadata::new("HS"))
        .unwrap()
        .with_deferred_push();
    assert!(store.autocommit("init store").unwrap());
    let _ = sync_once(&root);
    ok(&peer, &["pull", "--quiet"]);
    diverge(&root, &peer, 1);
    let local_head = head(&root);
    write_after_first_pick(&root, "new-ticket.md", "a different body");
    assert_eq!(sync_once_with(&root, live()), SyncReport::RebaseInProgress);
    let stalled = health(&root);
    assert_eq!(stalled.state, SyncState::RebaseInProgress);
    let reason = stalled.reason.unwrap();
    assert!(
        reason.contains("could not be continued automatically"),
        "{reason}"
    );
    assert!(reason.contains("refs/hotsheet/sync-backup/"), "{reason}");
    assert!(stalled.next_action.unwrap().contains("needs a decision"));
    assert_eq!(unmerged_paths(&root), ["new-ticket.md"]);
    let (stopped_at, reflog, backups) = (head(&root), reflog_len(&root), backup_refs(&root));
    assert_eq!(backups.len(), 1);
    for mode in both_modes() {
        assert_eq!(sync_once_with(&root, mode), SyncReport::RebaseInProgress);
    }
    assert_eq!(
        (head(&root), reflog_len(&root), backup_refs(&root)),
        (stopped_at, reflog, backups),
        "a real conflict is never continued or replayed"
    );
    assert!(
        health(&root)
            .reason
            .unwrap()
            .contains("stopped at a conflict in 1 file")
    );
    // A write while it is stalled is saved but not committed onto the detached HEAD.
    std::fs::write(root.join("during.md"), "during the stall\n").unwrap();
    assert!(
        !store
            .autocommit_paths("during", &[root.join("during.md")])
            .unwrap()
    );
    assert_eq!(read(&root, "during.md"), "during the stall\n");
    // The explicit recovery still works and keeps every local commit.
    let aborted = abort_stalled_integration(&root).unwrap();
    assert_eq!(aborted.aborted, IntegrationInProgress::Rebase);
    assert_eq!(aborted.head, local_head.trim());
    assert!(attached(&root));
    assert_eq!(read(&root, "during.md"), "during the stall\n");
}

// ---- self-heal of a rebase that was left unfinished ---------------------------------

/// The exact state of the incident is continued by the next cycle in either replay mode:
/// the branch is reattached, the commits made on the detached `HEAD` are kept, a write
/// deferred during the stall is committed, and everything is published.
#[cfg(unix)]
#[test]
fn hs2_qr1k8r_the_incident_state_is_continued_by_the_next_cycle() {
    for mode in both_modes() {
        let (_dir, root, peer, store) = merging_store();
        let detached = incident_state(&root, &peer, 1);
        assert_eq!(health(&root).state, SyncState::RebaseInProgress);
        // Nothing commits onto the detached HEAD while the replay is unfinished.
        std::fs::write(root.join("deferred.md"), "written during the stall\n").unwrap();
        for _ in 0..2 {
            assert!(
                !store
                    .autocommit_paths("deferred", &[root.join("deferred.md")])
                    .unwrap()
            );
            assert!(!store.autocommit("everything").unwrap());
        }
        assert_eq!(head(&root), detached, "{mode:?}");

        assert_eq!(
            sync_once_with(&root, mode),
            SyncReport::Synced {
                pulled: false,
                pushed: true
            },
            "{mode:?}"
        );
        assert!(attached(&root) && !unfinished(&root) && clean(&root));
        assert_eq!(backup_refs(&root), [detached.trim().to_string()]);
        assert!(git_ok(
            &root,
            &["merge-base", "--is-ancestor", detached.trim(), "HEAD"]
        ));
        let settled = health(&root);
        assert_eq!(settled.state, SyncState::Synced);
        assert_eq!(settled.consecutive_failures, 0);
        assert_eq!(settled.unpublished_commits, 0);
        assert_eq!(sync_once_with(&root, mode), SyncReport::UpToDate);
        assert!(read_record(&root).unwrap().continue_attempts.is_none());
        ok(&peer, &["pull", "--quiet"]);
        assert!(read(&peer, "new-ticket.md").contains("created and edited 1"));
        assert_eq!(read(&peer, "deferred.md"), "written during the stall\n");
        assert_eq!(read(&peer, "seed.md"), "local edit 1\n");
    }
}

/// One replay is continued at most [`MAX_CONTINUE_ATTEMPTS`] times; past that the cycle
/// reports it and leaves it alone until it is finished or aborted.
#[cfg(unix)]
#[test]
fn hs2_qr1k8r_continues_are_bounded_per_unfinished_replay() {
    let (_dir, root, peer, _store) = merging_store();
    let detached = incident_state(&root, &peer, 1);
    let orig_head = read(&root, ".git/rebase-merge/orig-head");
    for expected in 1..=MAX_CONTINUE_ATTEMPTS + 1 {
        assert_eq!(
            crate::sync_health::note_continue_attempt(&root, orig_head.trim()),
            expected
        );
    }
    // The count stops one past the bound instead of growing on every pass.
    assert_eq!(
        crate::sync_health::note_continue_attempt(&root, orig_head.trim()),
        MAX_CONTINUE_ATTEMPTS + 1
    );
    let reflog = reflog_len(&root);
    for _ in 0..2 {
        assert_eq!(sync_once(&root), SyncReport::RebaseInProgress);
    }
    assert_eq!((head(&root), reflog_len(&root)), (detached, reflog));
    assert!(backup_refs(&root).is_empty(), "nothing was attempted");
    let reason = health(&root).reason.unwrap();
    assert!(reason.contains("after 3 attempts"), "{reason}");
    // Another replay (a different orig-head) starts its own count.
    assert_eq!(
        crate::sync_health::note_continue_attempt(&root, "another-replay"),
        1
    );
    // Aborting clears the count with the stall, so a later stall is continued again.
    abort_stalled_integration(&root).unwrap();
    assert!(matches!(sync_once(&root), SyncReport::Synced { .. }));
    assert!(read_record(&root).unwrap().continue_attempts.is_none());
}

/// The states a store can be found in, walked in repeated, interleaved, and out-of-order
/// sequences, in both replay modes. Rows: what the cycle does in each state.
///
/// | Found                                   | Cycle                                  |
/// | --------------------------------------- | -------------------------------------- |
/// | nothing unfinished                      | replay, publish                        |
/// | rebase, rescheduled pick, no conflict   | back up, continue, publish             |
/// | rebase stopped at a conflict            | report, change nothing                 |
/// | rebase resolved by a person, not continued | report, change nothing              |
/// | merge unfinished (even without conflict) | report, change nothing                |
#[cfg(unix)]
#[test]
fn hs2_qr1k8r_sync_state_transition_matrix() {
    for mode in both_modes() {
        let (_dir, root, peer, store) = merging_store();
        let sync = |root: &Path| sync_once_with(root, mode);
        let unchanged = |root: &Path, passes: u32| {
            let (at, reflog) = (head(root), reflog_len(root));
            for _ in 0..passes {
                assert_eq!(sync(root), SyncReport::RebaseInProgress, "{mode:?}");
            }
            assert_eq!((head(root), reflog_len(root)), (at, reflog), "{mode:?}");
        };

        // Out of order: nothing to abort in a healthy store, twice.
        for _ in 0..2 {
            assert!(matches!(
                abort_stalled_integration(&root),
                Err(StalledIntegrationError::NothingInProgress)
            ));
        }
        assert_eq!(sync(&root), SyncReport::UpToDate);

        // Rescheduled pick → continued. Repeated: a second stall starts a new count.
        for round in 1..=2 {
            incident_state(&root, &peer, round);
            assert!(matches!(sync(&root), SyncReport::Synced { .. }), "{mode:?}");
            assert!(attached(&root) && !unfinished(&root) && clean(&root));
            assert_eq!(backup_refs(&root).len() as u32, round);
            // Out of order: the abort after a finished continue has nothing to do.
            assert!(abort_stalled_integration(&root).is_err());
            assert_eq!(sync(&root), SyncReport::UpToDate);
        }

        // A person's conflicting rebase: reported, never touched, however often.
        ok(&peer, &["pull", "--quiet"]);
        std::fs::write(root.join("plain.txt"), "local body\n").unwrap();
        ok(&root, &["add", "-A"]);
        ok(&root, &["commit", "--quiet", "-m", "local plain"]);
        std::fs::write(peer.join("plain.txt"), "peer body\n").unwrap();
        ok(&peer, &["add", "-A"]);
        ok(&peer, &["commit", "--quiet", "-m", "peer plain"]);
        ok(&peer, &["push", "--quiet"]);
        ok(&root, &["fetch", "--quiet"]);
        assert!(!git_ok(&root, &["rebase", "--quiet", "@{u}"]));
        assert_eq!(unmerged_paths(&root), ["plain.txt"]);
        unchanged(&root, 3);
        // Interleaved: a write during the stall is saved, not committed.
        std::fs::write(root.join("mid.md"), "mid\n").unwrap();
        assert!(
            !store
                .autocommit_paths("mid", &[root.join("mid.md")])
                .unwrap()
        );
        // The person resolves and stages but does not continue: still theirs to finish.
        std::fs::write(root.join("plain.txt"), "resolved body\n").unwrap();
        ok(&root, &["add", "--", "plain.txt"]);
        assert!(unmerged_paths(&root).is_empty());
        unchanged(&root, 2);
        assert!(backup_refs(&root).len() == 2, "no continue was attempted");
        // They finish it; the next cycle commits the deferred write and publishes.
        ok(&root, &["-c", "core.editor=true", "rebase", "--continue"]);
        assert_eq!(
            sync(&root),
            SyncReport::Synced {
                pulled: false,
                pushed: true
            },
            "{mode:?}"
        );
        assert!(clean(&root) && attached(&root));

        // An unfinished merge is never concluded for the user, conflict or not.
        ok(&peer, &["pull", "--quiet"]);
        assert_eq!(read(&peer, "mid.md"), "mid\n");
        commit_file(&root, "merge-local.md", "local\n");
        peer_pushes(&peer, "merge-peer.md");
        ok(&root, &["fetch", "--quiet"]);
        ok(
            &root,
            &["merge", "--quiet", "--no-commit", "--no-ff", "@{u}"],
        );
        assert_eq!(
            integration_in_progress(&root),
            Some(IntegrationInProgress::Merge)
        );
        unchanged(&root, 2);
        assert!(!store.autocommit("into the merge").unwrap());
        let aborted = abort_stalled_integration(&root).unwrap();
        assert_eq!(aborted.aborted, IntegrationInProgress::Merge);
        assert_eq!(
            sync(&root),
            SyncReport::Synced {
                pulled: true,
                pushed: true
            },
            "{mode:?}"
        );
        assert_eq!(sync(&root), SyncReport::UpToDate);
        let settled = health(&root);
        assert_eq!(settled.state, SyncState::UpToDate);
        assert_eq!(settled.consecutive_failures, 0);
        assert!(settled.reason.is_none());
    }
}

// ---- stress -------------------------------------------------------------------------

/// The incident's race, repeatedly: a writer creates files, commits them, and keeps
/// rewriting them (so unpublished commits always contain files the upstream lacks) while
/// sync cycles replay against a remote that keeps moving. No cycle may leave a rebase or
/// a detached `HEAD`, and the last body of every file must reach the remote.
#[test]
fn hs2_qr1k8r_stress_writes_racing_replays_never_stall_or_lose_a_write() {
    const FILES: usize = 8;
    const REWRITES: usize = 4;
    let (_dir, root, peer, store) = merging_store();
    let writer = {
        let root = root.clone();
        std::thread::spawn(move || {
            for n in 0..FILES {
                let path = root.join(format!("ticket-{n}.md"));
                for edit in 0..REWRITES {
                    crate::store::write_file_atomically(
                        &path,
                        format!("ticket {n} edit {edit}\n").as_bytes(),
                    )
                    .unwrap();
                    store
                        .autocommit_paths(
                            &format!("ticket {n} edit {edit}"),
                            std::slice::from_ref(&path),
                        )
                        .unwrap();
                }
            }
        })
    };
    let mut cycles = 0;
    let mut failure = None;
    while !writer.is_finished() || cycles < 4 {
        peer_pushes(&peer, &format!("peer-{cycles}.md"));
        let report = sync_once(&root);
        if !matches!(
            report,
            SyncReport::Synced { .. } | SyncReport::UpToDate | SyncReport::Offline
        ) || unfinished(&root)
            || !attached(&root)
        {
            failure = Some((cycles, report, unfinished(&root), attached(&root)));
            break;
        }
        cycles += 1;
    }
    writer.join().unwrap();
    assert!(
        failure.is_none(),
        "cycle left the store stalled: {failure:?}"
    );
    let mut settled = SyncReport::Offline;
    for _ in 0..20 {
        settled = sync_once(&root);
        if settled == SyncReport::UpToDate {
            break;
        }
    }
    assert_eq!(settled, SyncReport::UpToDate);
    assert!(clean(&root) && attached(&root) && !unfinished(&root));
    assert!(backup_refs(&root).is_empty(), "no recovery was ever needed");
    ok(&peer, &["pull", "--quiet"]);
    for n in 0..FILES {
        assert_eq!(
            read(&peer, &format!("ticket-{n}.md")),
            format!("ticket {n} edit {}\n", REWRITES - 1)
        );
    }
}
