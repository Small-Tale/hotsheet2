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
//!
//! Every remote-backed cycle records its outcome in the store's durable sync health
//! ([`crate::sync_health`], HS2-ADHFKZ). A push the remote *refuses* is
//! [`SyncReport::Rejected`] (with the oversized unpublished objects named), not
//! [`SyncReport::Offline`]; an in-progress or abandoned rebase stops the cycle before it
//! touches the tree; and a recorded conflict on the same `(HEAD, upstream)` pair is not
//! replayed again.

use std::io::Write as _;
use std::path::Path;
use std::process::Stdio;

use crate::store::{lock_git_transaction, lock_history_sync};
use crate::sync_health::{
    CycleDetail, OVERSIZED_OBJECT_BYTES, OversizedObject, integration_in_progress,
    record_attempt_start, record_outcome, recorded_conflict,
};

/// The outcome of one [`sync_once`] cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncReport {
    /// No git remote configured (a local-only store), or a designated root whose enclosing
    /// repository owns its remotes (HS2-V6JEBR) — nothing to fetch or push.
    NoRemote,
    /// Local and remote already agree — nothing to do.
    UpToDate,
    /// Integrated remote changes and/or pushed local commits.
    Synced { pulled: bool, pushed: bool },
    /// The remote was unreachable, refused authentication, or moved during the push —
    /// kept working locally; the next cycle retries.
    Offline,
    /// The remote refused the push (HS2-ADHFKZ): a hook, branch protection, or a file over
    /// the host's size limit. Retrying the same history cannot succeed.
    Rejected,
    /// The merge driver couldn't auto-resolve; the rebase was aborted so the working tree
    /// is clean, and the conflict is left for the user (the rare same-paragraph body case).
    Conflict,
    /// A rebase or merge is in progress or was abandoned in the store (HS2-ADHFKZ); the
    /// cycle did nothing so it never stacks a second replay on top of it.
    RebaseInProgress,
    /// A history rewrite replaced the remote; the store must be recovered before it may
    /// sync again (HS2-RFJ1GN).
    HistoryRecoveryRequired,
}

/// Limits a cycle checks against; tests shrink them to stay fast.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SyncLimits {
    pub oversized_bytes: u64,
}

impl Default for SyncLimits {
    fn default() -> Self {
        Self {
            oversized_bytes: OVERSIZED_OBJECT_BYTES,
        }
    }
}

/// Run one sync cycle for the store at `store_path`, recording its outcome in the store's
/// durable sync health.
pub fn sync_once(store_path: &Path) -> SyncReport {
    sync_once_with(store_path, SyncLimits::default())
}

pub(crate) fn sync_once_with(store_path: &Path, limits: SyncLimits) -> SyncReport {
    // HS2-V6JEBR: a designated root's remotes and branch belong to the enclosing code
    // repository. Hot Sheet never commits its other paths, rebases it, or pushes it.
    if !publishes_to_remote(store_path) {
        return SyncReport::NoRemote;
    }
    record_attempt_start(store_path);
    let (report, detail) = cycle(store_path, limits);
    record_outcome(store_path, report, detail);
    report
}

/// Whether the store publishes to a Git remote of its own: it has a remote and is not a
/// designated root inside a code repository (HS2-V6JEBR).
#[must_use]
pub fn publishes_to_remote(store_path: &Path) -> bool {
    has_remote(store_path)
        && !matches!(
            crate::FsStore::git_owner_at(store_path),
            crate::store_git::StoreGitOwner::Designated { .. }
        )
}

fn failed(report: SyncReport, reason: impl Into<String>) -> (SyncReport, CycleDetail) {
    (
        report,
        CycleDetail {
            reason: Some(reason.into()),
            ..CycleDetail::default()
        },
    )
}

/// Why a cycle stopped at an in-progress or abandoned rebase or merge.
pub const STALLED_INTEGRATION_REASON: &str = "a rebase or merge is in progress or was \
     abandoned in the store; sync will not start another until it is finished or aborted";

fn stalled_integration_reason() -> String {
    STALLED_INTEGRATION_REASON.to_string()
}

fn cycle(store_path: &Path, limits: SyncLimits) -> (SyncReport, CycleDetail) {
    // HS2-RFJ1GN: never replay an old local branch after compaction replaced origin.
    if crate::repository_compaction::recovery_required(store_path) {
        return failed(
            SyncReport::HistoryRecoveryRequired,
            "a history rewrite replaced the remote; recover this checkout before syncing",
        );
    }
    // HS2-ADHFKZ: never commit into, or stack a replay on top of, an unfinished rebase.
    if integration_in_progress(store_path).is_some() {
        return failed(SyncReport::RebaseInProgress, stalled_integration_reason());
    }
    // A sync that already passed its entry check must finish before a rewrite
    // starts; one waiting behind a rewrite must see its recovery guard (HS2-RFJ1GN).
    let Ok(_history) = lock_history_sync(store_path) else {
        return failed(SyncReport::Offline, "could not lock the store for sync");
    };
    if crate::repository_compaction::recovery_required(store_path) {
        return failed(
            SyncReport::HistoryRecoveryRequired,
            "a history rewrite replaced the remote; recover this checkout before syncing",
        );
    }
    // Belt-and-suspenders: commit any stray working-tree changes so the tree is clean for a
    // rebase (Hot Sheet edits already auto-commit, but an external edit may not have).
    // Every local Git transaction holds the store's autocommit lock (HS2-V454FB) so a
    // concurrent path-scoped autocommit never collides on `index.lock`; the network
    // steps (fetch/push) run unlocked so a slow remote never blocks local writes.
    {
        let Ok(_lock) = lock_git_transaction(store_path) else {
            return failed(SyncReport::Offline, "could not lock the store for sync");
        };
        if integration_in_progress(store_path).is_some() {
            return failed(SyncReport::RebaseInProgress, stalled_integration_reason());
        }
        commit_pending(store_path);
    }

    // Fetch; an unreachable remote is not an error — stay offline-tolerant.
    let fetch = git_run(store_path, &["fetch", "--quiet"]);
    if !fetch.ok {
        return failed(
            SyncReport::Offline,
            transport_reason("fetch", &fetch.stderr),
        );
    }

    // Without an upstream tracking branch we can't compare/rebase; try to publish instead.
    if !git_ok(store_path, &["rev-parse", "--verify", "--quiet", "@{u}"]) {
        let push = git_run(store_path, &["push", "--quiet", "-u", "origin", "HEAD"]);
        return if push.ok {
            (
                SyncReport::Synced {
                    pulled: false,
                    pushed: true,
                },
                CycleDetail::default(),
            )
        } else {
            classify_push_failure(store_path, &push.stderr, limits)
        };
    }

    let (ahead, behind) = ahead_behind(store_path);

    // Integrate incoming commits by rebasing onto the upstream (uses the merge driver).
    let mut pulled = false;
    if behind > 0 {
        let Ok(_lock) = lock_git_transaction(store_path) else {
            return failed(SyncReport::Offline, "could not lock the store for sync");
        };
        // A Hot Sheet write that landed its file while fetch ran commits only once it
        // takes this lock; commit it here so the rebase never sees a dirty tree.
        commit_pending(store_path);
        let heads = current_heads(store_path);
        // HS2-ADHFKZ: the same divergence already failed to merge; replaying thousands
        // of local commits again cannot change the answer.
        if heads.is_some() && recorded_conflict(store_path) == heads {
            return conflict(heads);
        }
        if git_ok(store_path, &["rebase", "--quiet", "@{u}"]) {
            pulled = true;
        } else {
            // Unresolved conflict → abort so the tree stays clean, surface it to the user.
            git_ok(store_path, &["rebase", "--abort"]);
            return conflict(heads);
        }
    }

    // Push local commits (recompute ahead after a rebase may have replayed them).
    let mut pushed = false;
    if ahead > 0 || pulled {
        let (ahead_now, _) = ahead_behind(store_path);
        if ahead_now > 0 {
            let push = git_run(store_path, &["push", "--quiet"]);
            if push.ok {
                pushed = true;
            } else {
                // The next cycle re-fetches; a refusal is reported as such.
                return classify_push_failure(store_path, &push.stderr, limits);
            }
        }
    }

    let report = if pulled || pushed {
        SyncReport::Synced { pulled, pushed }
    } else {
        SyncReport::UpToDate
    };
    (report, CycleDetail::default())
}

fn conflict(heads: Option<(String, String)>) -> (SyncReport, CycleDetail) {
    (
        SyncReport::Conflict,
        CycleDetail {
            reason: Some(
                "a ticket body changed on both sides and could not be merged automatically"
                    .to_string(),
            ),
            oversized: Vec::new(),
            conflict_heads: heads,
        },
    )
}

/// `(HEAD, upstream)` commit ids.
fn current_heads(store_path: &Path) -> Option<(String, String)> {
    let head = git_stdout(store_path, &["rev-parse", "HEAD"])?;
    let upstream = git_stdout(store_path, &["rev-parse", "@{u}"])?;
    Some((head.trim().to_string(), upstream.trim().to_string()))
}

/// Whether Git's push output says the remote *refused* the update, as opposed to a
/// transport/auth failure or a race with another pusher (which a re-fetch fixes).
#[must_use]
pub fn push_was_refused(stderr: &str) -> bool {
    let lower = stderr.to_ascii_lowercase();
    let refused = lower.contains("[remote rejected]")
        || lower.contains("pre-receive hook declined")
        || lower.contains("exceeds github's file size limit")
        || lower.contains("large files detected")
        || lower.contains("[rejected]");
    refused && !push_raced(&lower)
}

/// Another pusher moved the remote branch first; a re-fetch and retry fixes it.
fn push_raced(lower: &str) -> bool {
    lower.contains("fetch first")
        || lower.contains("non-fast-forward")
        || lower.contains("stale info")
        || lower.contains("cannot lock ref")
}

fn classify_push_failure(
    store_path: &Path,
    stderr: &str,
    limits: SyncLimits,
) -> (SyncReport, CycleDetail) {
    if !push_was_refused(stderr) {
        return failed(SyncReport::Offline, transport_reason("push", stderr));
    }
    let oversized = oversized_unpublished_objects(store_path, limits.oversized_bytes);
    let reason = rejection_lines(stderr);
    (
        SyncReport::Rejected,
        CycleDetail {
            reason: Some(if reason.is_empty() {
                "the remote rejected the push".to_string()
            } else {
                reason
            }),
            oversized,
            conflict_heads: None,
        },
    )
}

/// The lines of Git's push output that explain a refusal (remote messages, errors, and
/// the rejected ref), without progress noise.
fn rejection_lines(stderr: &str) -> String {
    stderr
        .lines()
        .map(str::trim)
        .filter(|line| {
            line.starts_with("remote:") || line.starts_with("error:") || line.starts_with('!')
        })
        .filter(|line| *line != "remote:" && !line.starts_with("error: failed to push some refs"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn transport_reason(step: &str, stderr: &str) -> String {
    let lower = stderr.to_ascii_lowercase();
    if lower.contains("authentication failed")
        || lower.contains("permission denied")
        || lower.contains("could not read username")
        || lower.contains("403")
    {
        format!("the remote refused authentication during {step}")
    } else if push_raced(&lower) {
        "the remote moved during the push; the next sync integrates it and retries".to_string()
    } else {
        let detail = stderr
            .lines()
            .map(str::trim)
            .find(|line| line.starts_with("fatal:") || line.starts_with("error:"))
            .unwrap_or("");
        if detail.is_empty() {
            format!("the remote could not be reached during {step}")
        } else {
            format!("the remote could not be reached during {step}: {detail}")
        }
    }
}

/// Unpublished blobs (reachable from `HEAD` but from no remote-tracking ref) larger than
/// `limit` bytes, with the path that introduced each. Runs only after a refused push, so
/// the walk cost is paid once per rejection rather than per cycle.
#[must_use]
pub fn oversized_unpublished_objects(store_path: &Path, limit: u64) -> Vec<OversizedObject> {
    let Some(listing) = git_stdout(
        store_path,
        &["rev-list", "--objects", "HEAD", "--not", "--remotes"],
    ) else {
        return Vec::new();
    };
    let mut paths = std::collections::HashMap::new();
    let mut ids = String::new();
    for line in listing.lines() {
        if let Some((id, path)) = line.split_once(' ')
            && !path.is_empty()
        {
            paths
                .entry(id.to_string())
                .or_insert_with(|| path.to_string());
            ids.push_str(id);
            ids.push('\n');
        }
    }
    if ids.is_empty() {
        return Vec::new();
    }
    let Ok(mut child) = crate::git::command()
        .arg("-C")
        .arg(store_path)
        .args([
            "cat-file",
            "--batch-check=%(objecttype) %(objectname) %(objectsize)",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return Vec::new();
    };
    if let Some(mut stdin) = child.stdin.take() {
        // A writer thread keeps a large listing from deadlocking on a full stdout pipe.
        let writer = std::thread::spawn(move || stdin.write_all(ids.as_bytes()));
        let Ok(output) = child.wait_with_output() else {
            return Vec::new();
        };
        let _ = writer.join();
        let mut found: Vec<OversizedObject> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .filter_map(|line| {
                let mut parts = line.split(' ');
                let (kind, id, size) = (parts.next()?, parts.next()?, parts.next()?);
                let bytes: u64 = size.parse().ok()?;
                (kind == "blob" && bytes > limit).then(|| OversizedObject {
                    path: paths.get(id).cloned().unwrap_or_default(),
                    bytes,
                    limit_bytes: limit,
                })
            })
            .collect();
        found.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.path.cmp(&b.path)));
        found
    } else {
        Vec::new()
    }
}

/// `(upstream name, ahead, behind)` against the last *fetched* upstream — no network. A
/// branch without an upstream counts commits no remote-tracking ref contains.
#[must_use]
pub fn local_divergence(store_path: &Path) -> (Option<String>, u32, u32) {
    let upstream = git_stdout(
        store_path,
        &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
    )
    .map(|name| name.trim().to_string())
    .filter(|name| !name.is_empty());
    if upstream.is_some() {
        let (ahead, behind) = ahead_behind(store_path);
        return (upstream, ahead, behind);
    }
    let ahead = git_stdout(
        store_path,
        &["rev-list", "--count", "HEAD", "--not", "--remotes"],
    )
    .and_then(|count| count.trim().parse().ok())
    .unwrap_or(0);
    (None, ahead, 0)
}

/// What [`abort_stalled_integration`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StalledIntegrationAbort {
    /// The rebase or merge that was aborted.
    pub aborted: crate::sync_health::IntegrationInProgress,
    /// A ref keeping the commit `HEAD` pointed at before the abort, so nothing made during
    /// the stalled rebase is unreachable.
    pub backup_ref: String,
    /// The branch commit after the abort.
    pub head: String,
}

/// Errors from [`abort_stalled_integration`].
#[derive(Debug, thiserror::Error)]
pub enum StalledIntegrationError {
    #[error("no rebase or merge is in progress in this store")]
    NothingInProgress,
    #[error("could not lock the store: {0}")]
    Lock(#[from] crate::StoreError),
    #[error("git {step} failed: {detail}")]
    Git { step: &'static str, detail: String },
}

/// Abort a rebase or merge left in progress in the store (HS2-ADHFKZ), the explicit
/// recovery for [`SyncReport::RebaseInProgress`]. First records a backup ref at the
/// current `HEAD`, then restores the branch to its pre-rebase commit; local commits on
/// the branch are kept and nothing is pushed.
pub fn abort_stalled_integration(
    store_path: &Path,
) -> Result<StalledIntegrationAbort, StalledIntegrationError> {
    let _history = lock_history_sync(store_path)?;
    let _git = lock_git_transaction(store_path)?;
    let Some(kind) = integration_in_progress(store_path) else {
        return Err(StalledIntegrationError::NothingInProgress);
    };
    let head = git_stdout(store_path, &["rev-parse", "HEAD"])
        .map(|value| value.trim().to_string())
        .ok_or_else(|| StalledIntegrationError::Git {
            step: "rev-parse",
            detail: "HEAD is not a commit".to_string(),
        })?;
    let stamp = time::OffsetDateTime::now_utc().unix_timestamp_nanos();
    let backup_ref = format!("refs/hotsheet/sync-backup/{stamp}");
    let update = git_run(store_path, &["update-ref", &backup_ref, &head]);
    if !update.ok {
        return Err(StalledIntegrationError::Git {
            step: "update-ref",
            detail: update.stderr.trim().to_string(),
        });
    }
    let args: &[&str] = match kind {
        crate::sync_health::IntegrationInProgress::Rebase => &["rebase", "--abort"],
        crate::sync_health::IntegrationInProgress::Merge => &["merge", "--abort"],
    };
    let abort = git_run(store_path, args);
    if !abort.ok || integration_in_progress(store_path).is_some() {
        return Err(StalledIntegrationError::Git {
            step: "abort",
            detail: abort.stderr.trim().to_string(),
        });
    }
    let after = git_stdout(store_path, &["rev-parse", "HEAD"])
        .map(|value| value.trim().to_string())
        .unwrap_or_default();
    Ok(StalledIntegrationAbort {
        aborted: kind,
        backup_ref,
        head: after,
    })
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

struct GitRun {
    ok: bool,
    stderr: String,
}

fn git_run(root: &Path, args: &[&str]) -> GitRun {
    match crate::git::command()
        .arg("-C")
        .arg(root)
        .args(args)
        .stdin(Stdio::null())
        .output()
    {
        Ok(out) => GitRun {
            ok: out.status.success(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        },
        Err(error) => GitRun {
            ok: false,
            stderr: format!("error: {error}"),
        },
    }
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

    /// HS2-V6JEBR: a designated root's remote is the enclosing code repository's. Sync
    /// must not commit the user's other changes, rebase that repository, or push it.
    #[test]
    fn hs2_v6jebr_sync_leaves_a_designated_roots_enclosing_repository_alone() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();
        let ident = ["-c", "user.name=t", "-c", "user.email=t@localhost"];
        assert!(git_ok(repo, &["init", "--quiet", "-b", "main"]));
        let store = repo.join("tickets");
        crate::FsStore::init(&store, &crate::StoreMetadata::new("HS")).unwrap();
        assert!(git_ok(repo, &["add", "--", "tickets/hotsheet-store.json"]));
        let mut commit = ident.to_vec();
        commit.extend(["commit", "-q", "-m", "track"]);
        assert!(git_ok(repo, &commit));
        assert!(git_ok(
            repo,
            &["remote", "add", "origin", "/no/such/repo.git"]
        ));
        std::fs::write(repo.join("staged.txt"), "user work\n").unwrap();
        assert!(git_ok(repo, &["add", "--", "staged.txt"]));
        std::fs::write(repo.join("loose.txt"), "untracked\n").unwrap();
        let before = git_stdout(repo, &["rev-parse", "HEAD"]);

        assert_eq!(sync_once(&store), SyncReport::NoRemote);
        assert_eq!(git_stdout(repo, &["rev-parse", "HEAD"]), before);
        let status = git_stdout(repo, &["status", "--porcelain"]).unwrap();
        assert!(status.contains("A  staged.txt"), "{status}");
        assert!(status.contains("?? loose.txt"), "{status}");
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

    // ---- HS2-ADHFKZ: sync health transition matrix (synthetic bare remotes only) ----

    use crate::sync_health::{SyncState, health, read_record};

    fn small_limits() -> SyncLimits {
        SyncLimits {
            oversized_bytes: 1_024,
        }
    }

    fn commit_file(root: &Path, file: &str, body: &str) {
        std::fs::write(root.join(file), body).unwrap();
        ok(root, &["add", "-A"]);
        ok(root, &["commit", "--quiet", "-m", file]);
    }

    fn reflog_len(root: &Path) -> usize {
        git_stdout(root, &["reflog", "show", "--format=%H", "HEAD"])
            .unwrap_or_default()
            .lines()
            .count()
    }

    #[cfg(unix)]
    fn install_rejecting_hook(remote: &Path) {
        use std::os::unix::fs::PermissionsExt;
        let hook = remote.join("hooks").join("pre-receive");
        std::fs::write(
            &hook,
            "#!/bin/sh\necho 'error: GH001: Large files detected. You may want to try Git Large File Storage.' >&2\nexit 1\n",
        )
        .unwrap();
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn hs2_adhfkz_push_refusals_are_told_apart_from_transport_failures_and_races() {
        let github = "remote: error: GH001: Large files detected.\nremote: error: File \
                      attachments/a.bin is 113.46 MB; this exceeds GitHub's file size limit of \
                      100.00 MB\n ! [remote rejected] main -> main (pre-receive hook declined)\n\
                      error: failed to push some refs to 'https://github.com/o/r.git'";
        assert!(push_was_refused(github));
        assert!(push_was_refused(
            " ! [remote rejected] main -> main (protected branch hook declined)"
        ));
        assert!(!push_was_refused(
            " ! [rejected]        main -> main (fetch first)\nerror: failed to push some refs"
        ));
        assert!(!push_was_refused(
            " ! [rejected]        main -> main (non-fast-forward)"
        ));
        assert!(!push_was_refused(
            "fatal: Authentication failed for 'https://github.com/o/r.git/'"
        ));
        assert!(!push_was_refused(
            "fatal: unable to access 'https://github.com/o/r.git/': Could not resolve host"
        ));
        assert_eq!(
            transport_reason("push", "fatal: Authentication failed for 'x'"),
            "the remote refused authentication during push"
        );
        assert!(
            transport_reason("push", " ! [rejected] main -> main (fetch first)")
                .contains("moved during the push")
        );
        let lines = rejection_lines(github);
        assert!(lines.contains("GH001"), "{lines}");
        assert!(!lines.contains("To "), "{lines}");
    }

    #[test]
    fn hs2_adhfkz_an_unreachable_remote_records_a_persistent_offline_state() {
        let dir = tempfile::tempdir().unwrap();
        ok(dir.path(), &["init", "--quiet"]);
        ok(
            dir.path(),
            &["remote", "add", "origin", "/no/such/repo.git"],
        );
        assert_eq!(sync_once(dir.path()), SyncReport::Offline);
        assert_eq!(sync_once(dir.path()), SyncReport::Offline);
        let health = health(dir.path());
        assert_eq!(health.state, SyncState::Offline);
        assert_eq!(health.consecutive_failures, 2);
        assert!(health.last_success_at.is_none());
        assert!(!health.syncing);
        assert!(
            health
                .reason
                .as_deref()
                .is_some_and(|reason| reason.contains("could not be reached")),
            "{health:?}"
        );
        assert!(health.next_action.is_some());
    }

    /// Ahead → behind → diverged → up to date, each settling with nothing unpublished
    /// and a fresh last-success time; a no-op pass keeps the store up to date.
    #[test]
    fn hs2_adhfkz_ahead_behind_and_diverged_stores_settle_synced() {
        let (_dir, store, peer) = remote_with_two_clones();

        commit_file(&store, "ahead.md", "local\n");
        assert_eq!(health(&store).unpublished_commits, 1);
        assert_eq!(health(&store).state, SyncState::Pending);
        assert_eq!(
            sync_once(&store),
            SyncReport::Synced {
                pulled: false,
                pushed: true
            }
        );
        let after_ahead = health(&store);
        assert_eq!(after_ahead.state, SyncState::Synced);
        assert_eq!(after_ahead.unpublished_commits, 0);
        assert!(after_ahead.last_success_at.is_some());

        peer_pushes(&peer, "behind.md");
        assert_eq!(
            sync_once(&store),
            SyncReport::Synced {
                pulled: true,
                pushed: false
            }
        );

        commit_file(&store, "diverged-local.md", "local\n");
        peer_pushes(&peer, "diverged-peer.md");
        assert_eq!(
            sync_once(&store),
            SyncReport::Synced {
                pulled: true,
                pushed: true
            }
        );
        assert_eq!(sync_once(&store), SyncReport::UpToDate);
        let settled = health(&store);
        assert_eq!(settled.state, SyncState::UpToDate);
        assert_eq!(
            (settled.unpublished_commits, settled.incoming_commits),
            (0, 0)
        );
        assert_eq!(settled.consecutive_failures, 0);
        assert!(settled.next_action.is_none());
        assert!(clean(&store));
    }

    /// A refused push is `Rejected` (never `Offline`), names the oversized unpublished
    /// object, keeps every local commit, persists across repeated passes, and clears once
    /// the remote accepts the history again.
    #[cfg(unix)]
    #[test]
    fn hs2_adhfkz_a_rejected_push_keeps_local_work_and_names_the_oversized_object() {
        let (dir, store, _peer) = remote_with_two_clones();
        let remote = dir.path().join("remote.git");
        install_rejecting_hook(&remote);
        let payload = vec![7u8; 4_096];
        std::fs::create_dir_all(store.join("attachments")).unwrap();
        std::fs::write(store.join("attachments").join("big.bin"), &payload).unwrap();
        ok(&store, &["add", "-A"]);
        ok(&store, &["commit", "--quiet", "-m", "synthetic attachment"]);
        commit_file(&store, "small.md", "small\n");
        let local_head = head(&store);

        for pass in 1..=2 {
            assert_eq!(sync_once_with(&store, small_limits()), SyncReport::Rejected);
            assert_eq!(head(&store), local_head, "local commits must survive");
            let health = health(&store);
            assert_eq!(health.state, SyncState::Rejected);
            assert_eq!(health.consecutive_failures, pass);
            assert_eq!(health.unpublished_commits, 2);
            assert_eq!(health.oversized.len(), 1, "{health:?}");
            assert_eq!(health.oversized[0].path, "attachments/big.bin");
            assert_eq!(health.oversized[0].bytes, 4_096);
            let reason = health.reason.unwrap();
            assert!(reason.contains("GH001"), "{reason}");
            assert!(!reason.contains(remote.to_str().unwrap()), "{reason}");
            assert!(health.next_action.unwrap().contains("Git LFS"));
        }
        // The record survives a "restart": a fresh read sees the same durable state.
        let record = read_record(&store).unwrap();
        assert_eq!(record.outcome, Some(SyncState::Rejected));

        // Recovery: the remote accepts the history again.
        std::fs::remove_file(remote.join("hooks").join("pre-receive")).unwrap();
        assert_eq!(
            sync_once_with(&store, small_limits()),
            SyncReport::Synced {
                pulled: false,
                pushed: true
            }
        );
        let recovered = health(&store);
        assert_eq!(recovered.state, SyncState::Synced);
        assert_eq!(recovered.consecutive_failures, 0);
        assert!(recovered.oversized.is_empty());
        assert!(recovered.reason.is_none());
        assert_eq!(recovered.unpublished_commits, 0);
    }

    /// A rebase left stopped in the store is detected before any work: repeated passes
    /// neither commit into it nor start another replay. The explicit abort keeps a backup
    /// ref and local commits; the real conflict is then reported once per divergence.
    #[test]
    fn hs2_adhfkz_a_stalled_rebase_is_never_replayed_and_aborts_safely() {
        let (_dir, store, peer) = remote_with_two_clones();
        commit_file(&store, "shared.md", "local body\n");
        let local_head = head(&store);
        std::fs::write(peer.join("shared.md"), "peer body\n").unwrap();
        ok(&peer, &["add", "-A"]);
        ok(&peer, &["commit", "--quiet", "-m", "peer shared"]);
        ok(&peer, &["push", "--quiet"]);
        ok(&store, &["fetch", "--quiet"]);
        assert!(!git_ok(&store, &["rebase", "--quiet", "@{u}"]));
        assert!(crate::sync_health::integration_in_progress(&store).is_some());
        let stopped_at = head(&store);
        let reflog = reflog_len(&store);

        for _ in 0..3 {
            assert_eq!(sync_once(&store), SyncReport::RebaseInProgress);
        }
        assert_eq!(head(&store), stopped_at);
        assert_eq!(
            reflog_len(&store),
            reflog,
            "sync must not restart the replay"
        );
        let stalled = health(&store);
        assert_eq!(stalled.state, SyncState::RebaseInProgress);
        assert_eq!(stalled.consecutive_failures, 3);
        assert!(
            stalled
                .next_action
                .unwrap()
                .contains("--abort-stalled-rebase")
        );

        let aborted = abort_stalled_integration(&store).unwrap();
        assert_eq!(
            aborted.aborted,
            crate::sync_health::IntegrationInProgress::Rebase
        );
        assert_eq!(aborted.head, local_head.trim());
        assert_eq!(
            git_stdout(&store, &["rev-parse", &aborted.backup_ref])
                .unwrap()
                .trim(),
            stopped_at.trim()
        );
        assert!(crate::sync_health::integration_in_progress(&store).is_none());
        assert!(matches!(
            abort_stalled_integration(&store),
            Err(StalledIntegrationError::NothingInProgress)
        ));
        // Resolved live condition: health waits for the next cycle.
        assert_eq!(health(&store).state, SyncState::Pending);

        // The genuine conflict is tried once, then not replayed for the same heads.
        assert_eq!(sync_once(&store), SyncReport::Conflict);
        let after_first = reflog_len(&store);
        assert_eq!(sync_once(&store), SyncReport::Conflict);
        assert_eq!(sync_once(&store), SyncReport::Conflict);
        assert_eq!(
            reflog_len(&store),
            after_first,
            "the same conflict must not be replayed"
        );
        assert_eq!(head(&store), local_head);
        assert!(clean(&store));
        assert_eq!(health(&store).state, SyncState::Conflict);

        // The upstream moving is a new divergence, so it is tried again.
        peer_pushes(&peer, "more.md");
        assert_eq!(sync_once(&store), SyncReport::Conflict);
        assert!(reflog_len(&store) > after_first);

        // The user resolves by taking the remote body; sync settles.
        ok(&store, &["reset", "--quiet", "--hard", "@{u}"]);
        assert_eq!(sync_once(&store), SyncReport::UpToDate);
        let settled = health(&store);
        assert_eq!(settled.state, SyncState::UpToDate);
        assert_eq!(settled.consecutive_failures, 0);
    }

    #[test]
    fn hs2_adhfkz_an_abandoned_merge_is_detected_too() {
        let (_dir, store, peer) = remote_with_two_clones();
        commit_file(&store, "shared.md", "local body\n");
        std::fs::write(peer.join("shared.md"), "peer body\n").unwrap();
        ok(&peer, &["add", "-A"]);
        ok(&peer, &["commit", "--quiet", "-m", "peer shared"]);
        ok(&peer, &["push", "--quiet"]);
        ok(&store, &["fetch", "--quiet"]);
        assert!(!git_ok(&store, &["merge", "--quiet", "--no-edit", "@{u}"]));
        assert_eq!(sync_once(&store), SyncReport::RebaseInProgress);
        let aborted = abort_stalled_integration(&store).unwrap();
        assert_eq!(
            aborted.aborted,
            crate::sync_health::IntegrationInProgress::Merge
        );
        assert!(clean(&store));
    }

    #[test]
    fn hs2_adhfkz_oversized_scan_ignores_published_blobs() {
        let (_dir, store, peer) = remote_with_two_clones();
        // Published by the peer: never reported.
        std::fs::write(peer.join("published.bin"), vec![1u8; 4_096]).unwrap();
        ok(&peer, &["add", "-A"]);
        ok(&peer, &["commit", "--quiet", "-m", "published"]);
        ok(&peer, &["push", "--quiet"]);
        ok(&store, &["pull", "--quiet"]);
        assert!(oversized_unpublished_objects(&store, 1_024).is_empty());
        std::fs::write(store.join("a.bin"), vec![2u8; 2_048]).unwrap();
        std::fs::write(store.join("b.bin"), vec![3u8; 8_192]).unwrap();
        std::fs::write(store.join("tiny.bin"), vec![4u8; 16]).unwrap();
        ok(&store, &["add", "-A"]);
        ok(&store, &["commit", "--quiet", "-m", "local blobs"]);
        let found = oversized_unpublished_objects(&store, 1_024);
        let paths: Vec<_> = found.iter().map(|o| (o.path.as_str(), o.bytes)).collect();
        assert_eq!(paths, [("b.bin", 8_192), ("a.bin", 2_048)]);
    }
}
