//! HS2-JGH11Q: ticket writes made while a rebase or merge is unfinished in the store are
//! saved with their commits deferred, reported by sync health, kept across
//! `abort_stalled_integration` (whose `git rebase --abort` is a hard reset), and committed
//! on the branch. Synthetic bare remotes and synthetic file bodies only.

use std::path::{Path, PathBuf};

use super::tests::{clean, commit_file, head, ok, peer_pushes, reflog_len, remote_with_two_clones};
use super::*;
use crate::store::{FsStore, StoreMetadata};
use crate::sync_health::{IntegrationInProgress, SyncState, health};

fn attached(root: &Path) -> bool {
    git_ok(root, &["symbolic-ref", "--quiet", "HEAD"])
}

fn unfinished(root: &Path) -> bool {
    integration_in_progress(root).is_some()
}

fn read(root: &Path, file: &str) -> Option<String> {
    std::fs::read_to_string(root.join(file)).ok()
}

fn subject(root: &Path, rev: &str) -> String {
    git_stdout(root, &["log", "-1", "--format=%s", rev])
        .unwrap()
        .trim()
        .to_string()
}

/// A published Hot Sheet store with a collaborator clone in sync with it.
fn published_store() -> (tempfile::TempDir, PathBuf, PathBuf, FsStore) {
    let (dir, root, peer) = remote_with_two_clones();
    let store = FsStore::init(&root, &StoreMetadata::new("HS"))
        .unwrap()
        .with_deferred_push();
    assert!(store.autocommit("init store").unwrap());
    commit_file(&root, "gone.md", "deleted during the stall\n");
    assert!(matches!(sync_once(&root), SyncReport::Synced { .. }));
    ok(&peer, &["pull", "--quiet"]);
    (dir, root, peer, store)
}

/// A person's interactive rebase onto the fetched upstream, run with a todo editor that
/// rewrites the todo with `script` (a `sed`/shell body over `$1`).
#[cfg(unix)]
fn person_rebases(root: &Path, script: &str) {
    use std::os::unix::fs::PermissionsExt;
    let editor = root.join(".git/hotsheet-test-todo-editor.sh");
    std::fs::write(&editor, format!("#!/bin/sh\n{script}\n")).unwrap();
    std::fs::set_permissions(&editor, std::fs::Permissions::from_mode(0o755)).unwrap();
    let editor = format!("sequence.editor={}", editor.display());
    let _ = git_ok(root, &["-c", &editor, "rebase", "-i", "--quiet", "@{u}"]);
    assert!(unfinished(root), "the rebase did not stop");
}

/// Stop at the first pick (`edit`): a rebase a person paused, which sync never continues.
#[cfg(unix)]
fn person_stops_at_an_edit(root: &Path) {
    person_rebases(
        root,
        r#"sed '1s/^pick/edit/' "$1" > "$1.new" && mv "$1.new" "$1""#,
    );
}

/// Hot Sheet writes a tracked file, creates one, and deletes one while the store is
/// stalled, `rounds` times; each commit is deferred. Returns the paths it touched.
fn write_while_stalled(root: &Path, store: &FsStore, rounds: u32) -> Vec<PathBuf> {
    let paths = vec![root.join("seed.md"), root.join("written.md")];
    for round in 1..=rounds {
        std::fs::write(&paths[0], format!("edited during the stall {round}\n")).unwrap();
        std::fs::write(&paths[1], format!("created during the stall {round}\n")).unwrap();
        assert!(
            !store.autocommit_paths("stalled write", &paths).unwrap(),
            "a write committed into an unfinished rebase or merge"
        );
    }
    let gone = root.join("gone.md");
    if gone.exists() {
        std::fs::remove_file(&gone).unwrap();
        assert!(!store.autocommit_paths("stalled delete", &[gone]).unwrap());
    }
    assert!(!store.autocommit("everything").unwrap());
    paths
}

/// The ticket's sequence: write during a rebase (repeatedly), abort, write again, sync.
/// The abort keeps every deferred write and commits it on the branch; the rebase's own
/// pick is discarded; everything is then published.
#[cfg(unix)]
#[test]
fn hs2_jgh11q_writes_during_a_rebase_survive_the_abort_and_are_published() {
    let (_dir, root, peer, store) = published_store();
    commit_file(&root, "local.md", "unpublished local commit\n");
    let local_head = head(&root);
    peer_pushes(&peer, "incoming.md");
    ok(&root, &["fetch", "--quiet"]);
    person_stops_at_an_edit(&root);
    let stopped = head(&root);
    let reflog = reflog_len(&root);

    // A person's paused rebase is not continued by sync.
    assert_eq!(sync_once(&root), SyncReport::RebaseInProgress);
    let paths = write_while_stalled(&root, &store, 3);
    assert_eq!(
        head(&root),
        stopped,
        "nothing commits onto the detached HEAD"
    );
    assert_eq!(reflog_len(&root), reflog);
    assert_eq!(sync_once(&root), SyncReport::RebaseInProgress);
    assert_eq!(head(&root), stopped);
    let stalled = health(&root);
    assert_eq!(stalled.state, SyncState::RebaseInProgress);
    let reason = stalled.reason.unwrap();
    assert!(
        reason.contains("3 changes are saved in the store but not committed"),
        "{reason}"
    );
    assert!(reason.contains("--abort-stalled-rebase"), "{reason}");

    let aborted = abort_stalled_integration(&root).unwrap();
    assert_eq!(aborted.aborted, IntegrationInProgress::Rebase);
    assert_eq!(aborted.saved_writes, 3);
    assert!(aborted.saved_writes_committed);
    let saved_ref = aborted.saved_writes_ref.unwrap();
    assert!(attached(&root) && !unfinished(&root) && clean(&root));
    assert_eq!(aborted.head, head(&root).trim());
    // The writes are committed on the branch, directly on the local commit.
    assert_eq!(
        git_stdout(&root, &["rev-parse", "HEAD^"]).unwrap(),
        local_head
    );
    assert!(subject(&root, "HEAD").contains("saved while a rebase or merge was unfinished"));
    assert_eq!(
        read(&root, "seed.md").as_deref(),
        Some("edited during the stall 3\n")
    );
    assert_eq!(
        read(&root, "written.md").as_deref(),
        Some("created during the stall 3\n")
    );
    assert!(read(&root, "gone.md").is_none());
    // The backup holds them too, and the earlier backup holds the stopped HEAD.
    assert_eq!(
        git_stdout(&root, &["show", &format!("{saved_ref}:seed.md")]).unwrap(),
        "edited during the stall 3\n"
    );
    assert_eq!(
        git_stdout(&root, &["rev-parse", &aborted.backup_ref]).unwrap(),
        stopped
    );
    assert_eq!(health(&root).state, SyncState::Pending);

    // Writing again commits normally.
    std::fs::write(&paths[0], "edited after the abort\n").unwrap();
    assert!(store.autocommit_paths("after abort", &paths[..1]).unwrap());
    assert_eq!(
        sync_once(&root),
        SyncReport::Synced {
            pulled: true,
            pushed: true
        }
    );
    assert!(attached(&root) && clean(&root));
    assert_eq!(health(&root).state, SyncState::Synced);
    ok(&peer, &["pull", "--quiet"]);
    assert_eq!(
        read(&peer, "seed.md").as_deref(),
        Some("edited after the abort\n")
    );
    assert_eq!(
        read(&peer, "written.md").as_deref(),
        Some("created during the stall 3\n")
    );
    assert!(read(&peer, "gone.md").is_none());
    assert!(read(&peer, "local.md").is_some() && read(&peer, "incoming.md").is_some());
}

/// A rebase stopped at a real conflict: the abort discards the rebase's conflicted file
/// (the branch's version returns) but keeps the write Hot Sheet deferred elsewhere.
#[test]
fn hs2_jgh11q_an_abort_discards_the_conflict_but_keeps_the_deferred_write() {
    let (_dir, root, peer, store) = published_store();
    commit_file(&root, "shared.md", "local body\n");
    let local_head = head(&root);
    peer_pushes(&peer, "shared.md");
    ok(&root, &["fetch", "--quiet"]);
    assert!(!git_ok(&root, &["rebase", "--quiet", "@{u}"]));
    assert!(unfinished(&root));
    write_while_stalled(&root, &store, 1);
    let reason = health(&root).reason.unwrap();
    assert!(reason.contains("3 changes are saved"), "{reason}");

    let aborted = abort_stalled_integration(&root).unwrap();
    assert_eq!(aborted.saved_writes, 3);
    assert!(aborted.saved_writes_committed);
    assert!(attached(&root) && !unfinished(&root) && clean(&root));
    assert_eq!(
        git_stdout(&root, &["rev-parse", "HEAD^"]).unwrap(),
        local_head
    );
    assert_eq!(read(&root, "shared.md").as_deref(), Some("local body\n"));
    assert_eq!(
        read(&root, "seed.md").as_deref(),
        Some("edited during the stall 1\n")
    );
    assert!(read(&root, "gone.md").is_none());
    // The genuine conflict is still reported, and the committed writes stay.
    let before = head(&root);
    assert_eq!(sync_once(&root), SyncReport::Conflict);
    assert_eq!(head(&root), before);
}

/// A merge left in progress: writes are deferred and reported, and `merge --abort` (which
/// would refuse or keep them) ends with them committed on the branch.
#[test]
fn hs2_jgh11q_writes_during_a_merge_survive_the_abort() {
    let (_dir, root, peer, store) = published_store();
    commit_file(&root, "shared.md", "local body\n");
    let local_head = head(&root);
    peer_pushes(&peer, "shared.md");
    ok(&root, &["fetch", "--quiet"]);
    assert!(!git_ok(&root, &["merge", "--quiet", "@{u}"]));
    assert_eq!(
        integration_in_progress(&root),
        Some(IntegrationInProgress::Merge)
    );
    write_while_stalled(&root, &store, 2);
    assert_eq!(head(&root), local_head);
    assert_eq!(sync_once(&root), SyncReport::RebaseInProgress);
    assert!(
        health(&root)
            .reason
            .unwrap()
            .contains("3 changes are saved")
    );

    let aborted = abort_stalled_integration(&root).unwrap();
    assert_eq!(aborted.aborted, IntegrationInProgress::Merge);
    assert_eq!(aborted.saved_writes, 3);
    assert!(attached(&root) && !unfinished(&root) && clean(&root));
    assert_eq!(
        git_stdout(&root, &["rev-parse", "HEAD^"]).unwrap(),
        local_head
    );
    assert_eq!(
        read(&root, "written.md").as_deref(),
        Some("created during the stall 2\n")
    );
    assert_eq!(read(&root, "shared.md").as_deref(), Some("local body\n"));
}

/// A rebase sync continues by itself (paused before its first pick, no conflict): the
/// deferred writes are committed by that cycle and published; a later write then
/// commits and syncs normally.
#[cfg(unix)]
#[test]
fn hs2_jgh11q_writes_during_a_continued_rebase_are_published() {
    let (_dir, root, peer, store) = published_store();
    commit_file(&root, "local.md", "unpublished local commit\n");
    peer_pushes(&peer, "incoming.md");
    ok(&root, &["fetch", "--quiet"]);
    person_rebases(
        &root,
        r#"{ echo break; cat "$1"; } > "$1.new" && mv "$1.new" "$1""#,
    );
    let paths = write_while_stalled(&root, &store, 2);
    assert!(
        health(&root)
            .reason
            .unwrap()
            .contains("3 changes are saved")
    );

    assert_eq!(
        sync_once(&root),
        SyncReport::Synced {
            pulled: false,
            pushed: true
        }
    );
    assert!(attached(&root) && !unfinished(&root) && clean(&root));
    std::fs::write(&paths[1], "written after the continue\n").unwrap();
    assert!(store.autocommit_paths("after", &paths[1..]).unwrap());
    assert!(matches!(sync_once(&root), SyncReport::Synced { .. }));
    ok(&peer, &["pull", "--quiet"]);
    assert_eq!(
        read(&peer, "seed.md").as_deref(),
        Some("edited during the stall 2\n")
    );
    assert_eq!(
        read(&peer, "written.md").as_deref(),
        Some("written after the continue\n")
    );
    assert!(read(&peer, "gone.md").is_none() && read(&peer, "local.md").is_some());
}

/// Without deferred writes the abort reports none and makes no extra commit.
#[test]
fn hs2_jgh11q_an_abort_without_deferred_writes_adds_nothing() {
    let (_dir, root, peer, _store) = published_store();
    commit_file(&root, "shared.md", "local body\n");
    let local_head = head(&root);
    peer_pushes(&peer, "shared.md");
    ok(&root, &["fetch", "--quiet"]);
    assert!(!git_ok(&root, &["rebase", "--quiet", "@{u}"]));
    let reason = health(&root).reason.unwrap();
    assert!(!reason.contains("saved in the store"), "{reason}");

    let aborted = abort_stalled_integration(&root).unwrap();
    assert_eq!(aborted.saved_writes, 0);
    assert!(aborted.saved_writes_ref.is_none() && !aborted.saved_writes_committed);
    assert_eq!(head(&root), local_head);
    assert!(clean(&root));
}
