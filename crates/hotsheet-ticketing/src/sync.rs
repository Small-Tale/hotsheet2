//! Automatic repo sync (`docs/02` §2.12, HS2-19) — one aggressive **fetch → integrate →
//! push** cycle per git-remote store, so a user effectively never runs git by hand.
//!
//! Incoming changes are integrated by **rebase through the semantic merge driver** (§2.7,
//! HS2-18): git consults the registered `merge=hotsheet-ticket` driver for ticket files, so
//! pulls land automatically and a conflict surfaces only in the rare same-paragraph body
//! case. The cycle is **offline-tolerant** (an unreachable remote never blocks local work)
//! and leaves a **clean working tree** on an unresolved conflict (the rebase is aborted).
//!
//! [`sync_once`] is a single cycle — the caller drives cadence/backoff (a periodic +
//! event-driven loop lands with the server, HS2-19 follow-up). Local edits already
//! auto-commit ([`crate::store`]), so this focuses on the remote half; a `local-only` store
//! reports [`SyncReport::NoRemote`].

use std::path::Path;

use crate::store::{lock_git_transaction, lock_history_sync};

/// The outcome of one [`sync_once`] cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncReport {
    /// No git remote configured (a local-only store) — nothing to fetch or push.
    NoRemote,
    /// Local and remote already agree — nothing to do.
    UpToDate,
    /// Integrated remote changes and/or pushed local commits.
    Synced { pulled: bool, pushed: bool },
    /// The remote was unreachable (or a push couldn't complete) — kept working locally;
    /// the next cycle retries.
    Offline,
    /// The merge driver couldn't auto-resolve; the rebase was aborted so the working tree
    /// is clean, and the conflict is left for the user (the rare same-paragraph body case).
    Conflict,
}

/// Run one sync cycle for the store at `store_path`.
pub fn sync_once(store_path: &Path) -> SyncReport {
    // HS2-RFJ1GN: never replay an old local branch after compaction replaced origin.
    if crate::repository_compaction::recovery_required(store_path) {
        return SyncReport::Offline;
    }
    if !has_remote(store_path) {
        return SyncReport::NoRemote;
    }
    // A sync that already passed its entry check must finish before a rewrite
    // starts; one waiting behind a rewrite must see its recovery guard (HS2-RFJ1GN).
    let Ok(_history) = lock_history_sync(store_path) else {
        return SyncReport::Offline;
    };
    if crate::repository_compaction::recovery_required(store_path) {
        return SyncReport::Offline;
    }
    // Belt-and-suspenders: commit any stray working-tree changes so the tree is clean for a
    // rebase (Hot Sheet edits already auto-commit, but an external edit may not have).
    // Every local Git transaction holds the store's autocommit lock (HS2-V454FB) so a
    // concurrent path-scoped autocommit never collides on `index.lock`; the network
    // steps (fetch/push) run unlocked so a slow remote never blocks local writes.
    {
        let Ok(_lock) = lock_git_transaction(store_path) else {
            return SyncReport::Offline;
        };
        commit_pending(store_path);
    }

    // Fetch; an unreachable remote is not an error — stay offline-tolerant.
    if !git_ok(store_path, &["fetch", "--quiet"]) {
        return SyncReport::Offline;
    }

    // Without an upstream tracking branch we can't compare/rebase; try to publish instead.
    if !git_ok(store_path, &["rev-parse", "--verify", "--quiet", "@{u}"]) {
        return if git_ok(store_path, &["push", "--quiet", "-u", "origin", "HEAD"]) {
            SyncReport::Synced {
                pulled: false,
                pushed: true,
            }
        } else {
            SyncReport::Offline
        };
    }

    let (ahead, behind) = ahead_behind(store_path);

    // Integrate incoming commits by rebasing onto the upstream (uses the merge driver).
    let mut pulled = false;
    if behind > 0 {
        let Ok(_lock) = lock_git_transaction(store_path) else {
            return SyncReport::Offline;
        };
        // A Hot Sheet write that landed its file while fetch ran commits only once it
        // takes this lock; commit it here so the rebase never sees a dirty tree.
        commit_pending(store_path);
        if git_ok(store_path, &["rebase", "--quiet", "@{u}"]) {
            pulled = true;
        } else {
            // Unresolved conflict → abort so the tree stays clean, surface it to the user.
            git_ok(store_path, &["rebase", "--abort"]);
            return SyncReport::Conflict;
        }
    }

    // Push local commits (recompute ahead after a rebase may have replayed them).
    let mut pushed = false;
    if ahead > 0 || pulled {
        let (ahead_now, _) = ahead_behind(store_path);
        if ahead_now > 0 {
            if git_ok(store_path, &["push", "--quiet"]) {
                pushed = true;
            } else {
                // A rejected/interrupted push — report offline; the next cycle re-fetches.
                return SyncReport::Offline;
            }
        }
    }

    if pulled || pushed {
        SyncReport::Synced { pulled, pushed }
    } else {
        SyncReport::UpToDate
    }
}

/// Whether the store's git repo has any remote configured.
fn has_remote(store_path: &Path) -> bool {
    git_stdout(store_path, &["remote"])
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
}

/// Auto-commit any pending working-tree changes (no-op when the tree is clean).
fn commit_pending(store_path: &Path) {
    let _ = git_ok(store_path, &["add", "-A"]);
    if !git_ok(store_path, &["diff", "--cached", "--quiet"]) {
        let _ = git_ok(
            store_path,
            &["commit", "--quiet", "-m", "Hot Sheet: sync local changes"],
        );
    }
}

/// `(ahead, behind)` relative to the upstream — how many commits local has that the remote
/// doesn't, and vice-versa. `(0, 0)` if it can't be determined.
fn ahead_behind(store_path: &Path) -> (u32, u32) {
    let out = git_stdout(
        store_path,
        &["rev-list", "--left-right", "--count", "HEAD...@{u}"],
    );
    out.and_then(|s| {
        let mut parts = s.split_whitespace();
        let ahead = parts.next()?.parse().ok()?;
        let behind = parts.next()?.parse().ok()?;
        Some((ahead, behind))
    })
    .unwrap_or((0, 0))
}

fn git_ok(root: &Path, args: &[&str]) -> bool {
    crate::git::command()
        .arg("-C")
        .arg(root)
        .args(args)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn git_stdout(root: &Path, args: &[&str]) -> Option<String> {
    let out = crate::git::command()
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_store_without_a_remote_reports_no_remote() {
        let dir = tempfile::tempdir().unwrap();
        assert!(git_ok(dir.path(), &["init", "--quiet"]));
        assert_eq!(sync_once(dir.path()), SyncReport::NoRemote);
    }

    #[test]
    fn an_unreachable_remote_is_offline_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(git_ok(dir.path(), &["init", "--quiet"]));
        assert!(git_ok(
            dir.path(),
            &["remote", "add", "origin", "/no/such/repo.git"],
        ));
        assert_eq!(sync_once(dir.path()), SyncReport::Offline);
    }

    fn ok(root: &Path, args: &[&str]) {
        assert!(
            git_ok(root, args),
            "git {args:?} failed in {}",
            root.display()
        );
    }

    fn identity(root: &Path) {
        ok(root, &["config", "user.name", "Sync Test"]);
        ok(root, &["config", "user.email", "sync@test.invalid"]);
    }

    /// A bare remote with one commit, plus two clones of it: `store` (the synced store)
    /// and `peer` (another collaborator).
    fn remote_with_two_clones() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let remote = dir.path().join("remote.git");
        let store = dir.path().join("store");
        let peer = dir.path().join("peer");
        let remote_arg = remote.to_str().unwrap();
        ok(dir.path(), &["init", "--quiet", "--bare", remote_arg]);
        ok(
            dir.path(),
            &["clone", "--quiet", remote_arg, peer.to_str().unwrap()],
        );
        identity(&peer);
        std::fs::write(peer.join("seed.md"), "seed\n").unwrap();
        ok(&peer, &["add", "-A"]);
        ok(&peer, &["commit", "--quiet", "-m", "seed"]);
        ok(&peer, &["push", "--quiet", "-u", "origin", "HEAD"]);
        ok(
            dir.path(),
            &["clone", "--quiet", remote_arg, store.to_str().unwrap()],
        );
        identity(&store);
        (dir, store, peer)
    }

    fn head(root: &Path) -> String {
        git_stdout(root, &["rev-parse", "HEAD"]).unwrap()
    }

    fn clean(root: &Path) -> bool {
        git_stdout(root, &["status", "--porcelain"])
            .unwrap()
            .is_empty()
    }

    fn peer_pushes(peer: &Path, file: &str) {
        std::fs::write(peer.join(file), "from peer\n").unwrap();
        ok(peer, &["add", "-A"]);
        ok(peer, &["commit", "--quiet", "-m", file]);
        // The store may push concurrently (sync or a best-effort autocommit push), so a
        // collaborator integrates and retries like a real one would.
        for _ in 0..20 {
            ok(peer, &["pull", "--quiet", "--rebase"]);
            if git_ok(peer, &["push", "--quiet"]) {
                return;
            }
        }
        panic!("peer could not push {file}");
    }

    /// Run `sync_once` on a thread while the test holds the store's Git transaction
    /// lock; `held_check` runs while sync is blocked, before the lock is released.
    fn sync_while_locked(store: &Path, held_check: impl FnOnce()) -> SyncReport {
        use std::sync::mpsc::{RecvTimeoutError, sync_channel};
        use std::time::Duration;

        let held = lock_git_transaction(store).unwrap();
        let (sender, receiver) = sync_channel(1);
        let root = store.to_path_buf();
        let worker = std::thread::spawn(move || sender.send(sync_once(&root)).unwrap());
        assert!(matches!(
            receiver.recv_timeout(Duration::from_millis(300)),
            Err(RecvTimeoutError::Timeout)
        ));
        held_check();
        drop(held);
        let report = receiver.recv_timeout(Duration::from_secs(30)).unwrap();
        worker.join().unwrap();
        report
    }

    #[test]
    fn the_pending_commit_waits_for_the_autocommit_lock() {
        let (_dir, store, _peer) = remote_with_two_clones();
        let before = head(&store);
        std::fs::write(store.join("external.md"), "edited outside Hot Sheet\n").unwrap();
        let report = sync_while_locked(&store, || {
            // Still blocked on the lock: nothing staged, nothing committed.
            assert_eq!(head(&store), before);
            assert!(git_ok(&store, &["diff", "--cached", "--quiet"]));
            assert!(!store.join(".git/index.lock").exists());
        });
        assert_eq!(
            report,
            SyncReport::Synced {
                pulled: false,
                pushed: true
            }
        );
        assert_ne!(head(&store), before);
        assert!(clean(&store));
    }

    #[test]
    fn the_rebase_waits_for_the_autocommit_lock() {
        let (_dir, store, peer) = remote_with_two_clones();
        // A local commit plus an incoming one forces a real rebase.
        std::fs::write(store.join("local.md"), "local\n").unwrap();
        ok(&store, &["add", "-A"]);
        ok(&store, &["commit", "--quiet", "-m", "local"]);
        peer_pushes(&peer, "incoming.md");
        let before = head(&store);
        let report = sync_while_locked(&store, || {
            assert_eq!(
                head(&store),
                before,
                "sync moved HEAD while the lock was held"
            );
            assert!(!store.join(".git/rebase-merge").exists());
            assert!(!store.join(".git/rebase-apply").exists());
            // A Hot Sheet write lands its file while sync waits; the rebase must not
            // fail on the now-dirty tree.
            std::fs::write(store.join("late.md"), "written while sync waited\n").unwrap();
        });
        assert_eq!(
            report,
            SyncReport::Synced {
                pulled: true,
                pushed: true
            }
        );
        assert!(store.join("incoming.md").is_file());
        assert!(clean(&store));
        ok(&peer, &["pull", "--quiet"]);
        assert!(peer.join("local.md").is_file());
        assert!(peer.join("late.md").is_file());
    }

    /// Adversarial interleaving: path-scoped autocommits race repeated sync cycles
    /// (pending commit, fetch, rebase, push) against a remote that keeps moving. Every
    /// autocommit must commit, the tree must end clean, and the remote must receive
    /// every write.
    #[test]
    fn concurrent_autocommits_and_sync_cycles_never_lose_or_fail_a_write() {
        let (_dir, store_root, peer) = remote_with_two_clones();
        let store =
            crate::store::FsStore::init(&store_root, &crate::store::StoreMetadata::new("HS"))
                .unwrap();
        assert!(store.autocommit("init store").unwrap());
        let writes = 12;
        let writer = {
            let root = store_root.clone();
            std::thread::spawn(move || {
                for n in 0..writes {
                    let path = root.join(format!("write-{n}.md"));
                    std::fs::write(&path, format!("write {n}\n")).unwrap();
                    // A sync cycle's pending commit may already have taken the file; the
                    // write must still succeed either way.
                    store
                        .autocommit_paths(&format!("write {n}"), &[path])
                        .unwrap();
                }
            })
        };
        let mut cycles = 0;
        let mut unexpected = None;
        while !writer.is_finished() || cycles < 3 {
            if cycles % 2 == 0 {
                peer_pushes(&peer, &format!("peer-{cycles}.md"));
            }
            let report = sync_once(&store_root);
            // HS2-KXH2ZV: a background autocommit push can update origin's
            // tracking ref during fetch. Offline is a retryable sync result.
            // Keep the repository alive until the writer has joined, even if
            // an unexpected result needs to fail the test.
            if !matches!(
                report,
                SyncReport::Synced { .. } | SyncReport::UpToDate | SyncReport::Offline
            ) {
                unexpected = Some((cycles, report));
                break;
            }
            cycles += 1;
        }
        writer.join().unwrap();
        assert!(
            unexpected.is_none(),
            "unexpected sync result: {unexpected:?}"
        );
        let mut settled = SyncReport::Offline;
        for _ in 0..20 {
            settled = sync_once(&store_root);
            if !matches!(settled, SyncReport::Offline) {
                break;
            }
        }
        assert!(
            matches!(settled, SyncReport::Synced { .. } | SyncReport::UpToDate),
            "sync did not settle after the writer finished: {settled:?}"
        );
        assert!(clean(&store_root));
        ok(&peer, &["pull", "--quiet"]);
        for n in 0..writes {
            assert!(
                peer.join(format!("write-{n}.md")).is_file(),
                "write {n} never reached the remote"
            );
        }
    }
}
