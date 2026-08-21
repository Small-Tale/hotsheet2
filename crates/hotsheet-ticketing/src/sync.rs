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
use std::process::Command;

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
    if !has_remote(store_path) {
        return SyncReport::NoRemote;
    }
    // Belt-and-suspenders: commit any stray working-tree changes so the tree is clean for a
    // rebase (Hot Sheet edits already auto-commit, but an external edit may not have).
    commit_pending(store_path);

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
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn git_stdout(root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
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
}
