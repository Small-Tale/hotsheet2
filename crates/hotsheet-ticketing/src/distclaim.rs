//! Git-native **distributed** claim/lease (`docs/08` §8.5, HS2-84) — the multi-machine
//! regime where independent workers coordinate over a shared git remote with **no central
//! coordinator**. (The single-shared-server regime uses the in-process `ops::claim_*`
//! primitive instead; this layers on top for the no-server case.)
//!
//! One **claim marker per ticket** lives under a reserved ref namespace,
//! `refs/hotsheet/claims/<ULID>`, pointing at a tiny orphan commit whose message carries
//! `{ worker, expires_at, claimed_at }`. The remote serializes pushes, so:
//!
//! - **Claim** = push to *create* the ref. A plain (non-force) push of an orphan commit is
//!   a fast-forward only when the ref doesn't exist yet, so the **first push wins and the
//!   second is rejected** — that rejection *is* the distributed compare-and-swap.
//! - **Renew / steal** = `push --force-with-lease=<ref>:<seen-oid>` — succeeds only if the
//!   marker is still what you last saw, so two workers can't both renew or steal a lease.
//! - **Enumerate** = `ls-remote` the namespace; **sweep** = delete markers whose lease has
//!   passed. Custom refs live outside `main`'s history, the working tree, and normal git
//!   surfaces, so they never clutter anything.
//!
//! Validated by the HS2-63 spike (custom-ref CAS, `--force-with-lease` renew, two-stealer
//! race → exactly one wins, and GitHub accepting the namespace). Shells out to `git` like
//! the rest of the store layer, so it works against any remote.

use std::path::Path;
use std::process::Command;

use serde::{Deserialize, Serialize};

use hotsheet_model::{Timestamp, Ulid};

/// A distributed-claim error (a real git/remote failure, distinct from a *lost* CAS race,
/// which is reported as [`ClaimResult::Lost`] rather than an error).
#[derive(Debug, thiserror::Error)]
pub enum DistError {
    #[error("no git remote is configured for this store")]
    NoRemote,
    #[error("git {op} failed: {msg}")]
    Git { op: String, msg: String },
}

/// The parsed payload of a claim marker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClaimMarker {
    /// The claimed ticket.
    #[serde(skip)]
    pub id: Option<Ulid>,
    /// The worker holding the claim.
    pub worker: String,
    /// When the lease expires (a stealer may reclaim after this).
    pub expires_at: Timestamp,
    /// When the claim was taken.
    pub claimed_at: Timestamp,
    /// The remote oid the marker currently points at (for `--force-with-lease`). Filled by
    /// [`list_claims`]; empty on a freshly built payload.
    #[serde(skip)]
    pub oid: String,
}

impl ClaimMarker {
    /// Whether the lease has expired at `now` (reclaimable).
    pub fn is_expired(&self, now: &Timestamp) -> bool {
        self.expires_at.chronological_cmp(now) != Some(std::cmp::Ordering::Greater)
    }
}

/// The outcome of a claim/renew attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimResult {
    /// We hold the marker now.
    Won(ClaimMarker),
    /// Another worker holds it (the push CAS was rejected).
    Lost,
}

/// The ref for a ticket's claim marker.
pub fn claim_ref(id: &Ulid) -> String {
    format!("refs/hotsheet/claims/{id}")
}

/// Attempt to claim `id` for `worker` with a lease until `expires_at`. Push-CAS-create:
/// the first worker wins ([`ClaimResult::Won`]); a rejected push means someone else holds
/// it ([`ClaimResult::Lost`]).
pub fn claim(
    store_path: &Path,
    id: &Ulid,
    worker: &str,
    expires_at: Timestamp,
    now: &Timestamp,
) -> Result<ClaimResult, DistError> {
    ensure_remote(store_path)?;
    let payload = ClaimMarker {
        id: Some(*id),
        worker: worker.to_string(),
        expires_at,
        claimed_at: now.clone(),
        oid: String::new(),
    };
    let commit = write_marker_commit(store_path, &payload)?;
    let r = claim_ref(id);
    // Plain (non-force) push: creates the ref if absent, rejected if it already exists.
    let (ok, _out, err) = git(store_path, &["push", "origin", &format!("{commit}:{r}")]);
    if ok {
        Ok(ClaimResult::Won(ClaimMarker {
            oid: commit,
            ..payload
        }))
    } else if looks_like_reject(&err) {
        Ok(ClaimResult::Lost)
    } else {
        Err(DistError::Git {
            op: "push (claim)".into(),
            msg: err,
        })
    }
}

/// Renew (or steal an expired) claim by force-with-lease against the `seen_oid` you last
/// observed (from [`list_claims`]). Succeeds only if the marker still points there — so two
/// simultaneous renewers/stealers, exactly one wins.
pub fn renew(
    store_path: &Path,
    id: &Ulid,
    worker: &str,
    seen_oid: &str,
    expires_at: Timestamp,
    now: &Timestamp,
) -> Result<ClaimResult, DistError> {
    ensure_remote(store_path)?;
    let payload = ClaimMarker {
        id: Some(*id),
        worker: worker.to_string(),
        expires_at,
        claimed_at: now.clone(),
        oid: String::new(),
    };
    let commit = write_marker_commit(store_path, &payload)?;
    let r = claim_ref(id);
    let lease = format!("--force-with-lease={r}:{seen_oid}");
    let (ok, _out, err) = git(
        store_path,
        &["push", &lease, "origin", &format!("{commit}:{r}")],
    );
    if ok {
        Ok(ClaimResult::Won(ClaimMarker {
            oid: commit,
            ..payload
        }))
    } else if looks_like_reject(&err) {
        Ok(ClaimResult::Lost)
    } else {
        Err(DistError::Git {
            op: "push (renew)".into(),
            msg: err,
        })
    }
}

/// Release a claim by deleting its marker ref. Returns whether a marker was removed.
pub fn release(store_path: &Path, id: &Ulid) -> Result<bool, DistError> {
    ensure_remote(store_path)?;
    let r = claim_ref(id);
    let (ok, _out, err) = git(store_path, &["push", "origin", &format!(":{r}")]);
    if ok {
        Ok(true)
    } else if err.contains("remote ref does not exist") {
        Ok(false)
    } else {
        Err(DistError::Git {
            op: "push (release)".into(),
            msg: err,
        })
    }
}

/// Enumerate the live claim markers on the remote (`ls-remote` the namespace, then read
/// each marker's payload). Sorted by ticket id.
pub fn list_claims(store_path: &Path) -> Result<Vec<ClaimMarker>, DistError> {
    ensure_remote(store_path)?;
    let (ok, out, err) = git(
        store_path,
        &["ls-remote", "origin", "refs/hotsheet/claims/*"],
    );
    if !ok {
        return Err(DistError::Git {
            op: "ls-remote".into(),
            msg: err,
        });
    }
    // Fetch the marker objects so their commit messages are readable locally.
    let _ = git(
        store_path,
        &[
            "fetch",
            "--quiet",
            "origin",
            "refs/hotsheet/claims/*:refs/hotsheet/claims/*",
        ],
    );

    let mut markers = Vec::new();
    for line in out.lines() {
        let mut parts = line.split_whitespace();
        let (Some(oid), Some(refname)) = (parts.next(), parts.next()) else {
            continue;
        };
        let Some(id) = refname
            .rsplit('/')
            .next()
            .and_then(|s| Ulid::from_string(s).ok())
        else {
            continue;
        };
        let (mok, msg, _e) = git(store_path, &["show", "-s", "--format=%B", oid]);
        if !mok {
            continue;
        }
        if let Ok(mut m) = serde_json::from_str::<ClaimMarker>(msg.trim()) {
            m.id = Some(id);
            m.oid = oid.to_string();
            markers.push(m);
        }
    }
    markers.sort_by_key(|m| m.id);
    Ok(markers)
}

/// Delete every marker whose lease has expired at `now` (the periodic cleanup sweep).
/// Returns how many were reaped.
pub fn sweep_expired(store_path: &Path, now: &Timestamp) -> Result<usize, DistError> {
    let mut reaped = 0;
    for m in list_claims(store_path)? {
        if m.is_expired(now) {
            if let Some(id) = m.id {
                if release(store_path, &id)? {
                    reaped += 1;
                }
            }
        }
    }
    Ok(reaped)
}

// ---- git plumbing ----------------------------------------------------------------

/// Create an orphan commit (empty tree) whose message is the marker payload; return its oid.
fn write_marker_commit(store_path: &Path, payload: &ClaimMarker) -> Result<String, DistError> {
    let json = serde_json::to_string(payload).map_err(|e| DistError::Git {
        op: "serialize".into(),
        msg: e.to_string(),
    })?;
    // Empty tree (hash-algorithm-agnostic: let git compute it from empty input).
    let (tok, tree, terr) = git_stdin(store_path, &["mktree"], "");
    if !tok {
        return Err(DistError::Git {
            op: "mktree".into(),
            msg: terr,
        });
    }
    // A bot identity so `commit-tree` works even in a repo with no configured user.
    let (cok, commit, cerr) = git(
        store_path,
        &[
            "-c",
            "user.name=Hot Sheet",
            "-c",
            "user.email=hotsheet@localhost",
            "commit-tree",
            tree.trim(),
            "-m",
            &json,
        ],
    );
    if !cok {
        return Err(DistError::Git {
            op: "commit-tree".into(),
            msg: cerr,
        });
    }
    Ok(commit.trim().to_string())
}

fn ensure_remote(store_path: &Path) -> Result<(), DistError> {
    let (_ok, out, _e) = git(store_path, &["remote"]);
    if out.trim().is_empty() {
        Err(DistError::NoRemote)
    } else {
        Ok(())
    }
}

/// A rejected push (the marker already exists / stale lease / non-fast-forward) — the CAS
/// lost the race, not a transport failure.
fn looks_like_reject(stderr: &str) -> bool {
    let s = stderr.to_lowercase();
    s.contains("rejected")
        || s.contains("fetch first")
        || s.contains("already exists")
        || s.contains("non-fast-forward")
        || s.contains("stale info")
        || s.contains("failed to push")
}

/// Run `git -C store_path <args>`, capturing (success, stdout, stderr).
fn git(store_path: &Path, args: &[&str]) -> (bool, String, String) {
    match Command::new("git")
        .arg("-C")
        .arg(store_path)
        .args(args)
        .output()
    {
        Ok(o) => (
            o.status.success(),
            String::from_utf8_lossy(&o.stdout).into_owned(),
            String::from_utf8_lossy(&o.stderr).into_owned(),
        ),
        Err(e) => (false, String::new(), e.to_string()),
    }
}

/// Like [`git`] but feeds `stdin` to the command.
fn git_stdin(store_path: &Path, args: &[&str], stdin: &str) -> (bool, String, String) {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = match Command::new("git")
        .arg("-C")
        .arg(store_path)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return (false, String::new(), e.to_string()),
    };
    if let Some(mut si) = child.stdin.take() {
        let _ = si.write_all(stdin.as_bytes());
    }
    match child.wait_with_output() {
        Ok(o) => (
            o.status.success(),
            String::from_utf8_lossy(&o.stdout).into_owned(),
            String::from_utf8_lossy(&o.stderr).into_owned(),
        ),
        Err(e) => (false, String::new(), e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(s: &str) -> Timestamp {
        Timestamp::new(format!("2026-08-22T00:00:{s}Z"))
    }
    fn id(hex: &str) -> Ulid {
        Ulid::from_string(hex).unwrap()
    }

    /// A bare remote + two clones ("workers"), each a real git repo with `origin` set.
    fn two_workers() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let bare = dir.path().join("remote.git");
        let a = dir.path().join("a");
        let b = dir.path().join("b");
        let run = |args: &[&str]| {
            let ok = std::process::Command::new("git")
                .args(args)
                .status()
                .unwrap()
                .success();
            assert!(ok, "git {args:?}");
        };
        run(&[
            "init",
            "--quiet",
            "--bare",
            "-b",
            "main",
            bare.to_str().unwrap(),
        ]);
        run(&[
            "clone",
            "--quiet",
            bare.to_str().unwrap(),
            a.to_str().unwrap(),
        ]);
        run(&[
            "clone",
            "--quiet",
            bare.to_str().unwrap(),
            b.to_str().unwrap(),
        ]);
        (dir, a, b)
    }

    #[test]
    fn first_claim_wins_second_loses_then_renew_release_sweep() {
        let (_d, a, b) = two_workers();
        let t = id("01ARZ3NDEKTSV4RRFFQ69G5FAV");
        let future = ts("30");
        let now = ts("00");

        // First worker wins the push-CAS; the second is rejected → Lost.
        let won = match claim(&a, &t, "worker-a", future.clone(), &now).unwrap() {
            ClaimResult::Won(m) => m,
            ClaimResult::Lost => panic!("first claim should win"),
        };
        assert_eq!(won.worker, "worker-a");
        assert_eq!(
            claim(&b, &t, "worker-b", future.clone(), &now).unwrap(),
            ClaimResult::Lost,
            "second claim loses the CAS"
        );

        // Enumerate sees exactly one live marker.
        let claims = list_claims(&b).unwrap();
        assert_eq!(claims.len(), 1);
        assert_eq!(claims[0].worker, "worker-a");
        assert_eq!(claims[0].id, Some(t));

        // The holder renews with the oid it last saw (force-with-lease matches) → Won.
        let renewed = match renew(&a, &t, "worker-a", &won.oid, ts("40"), &now).unwrap() {
            ClaimResult::Won(m) => m,
            ClaimResult::Lost => panic!("holder renew should win"),
        };
        // A different worker renewing against the STALE oid it once saw → Lost.
        assert_eq!(
            renew(&b, &t, "worker-b", &won.oid, ts("50"), &now).unwrap(),
            ClaimResult::Lost,
            "a stale force-with-lease is rejected"
        );
        assert_ne!(renewed.oid, won.oid, "renew advanced the marker");

        // Release deletes the marker; releasing again is idempotent (no error).
        assert!(release(&a, &t).unwrap());
        assert!(list_claims(&a).unwrap().is_empty());
        release(&a, &t).unwrap(); // already gone — must not error
    }

    #[test]
    fn sweep_reaps_only_expired_markers() {
        let (_d, a, _b) = two_workers();
        let live = id("01ARZ3NDEKTSV4RRFFQ69G5FB0");
        let dead = id("01ARZ3NDEKTSV4RRFFQ69G5FB1");
        let now = ts("30");

        // One lease in the future (live), one already past (dead).
        claim(&a, &live, "w", ts("59"), &now).unwrap();
        claim(&a, &dead, "w", ts("10"), &now).unwrap();

        let reaped = sweep_expired(&a, &now).unwrap();
        assert_eq!(reaped, 1, "only the expired marker is reaped");
        let left = list_claims(&a).unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].id, Some(live));
    }

    #[test]
    fn a_store_without_a_remote_reports_no_remote() {
        let dir = tempfile::tempdir().unwrap();
        std::process::Command::new("git")
            .args(["init", "--quiet", dir.path().to_str().unwrap()])
            .status()
            .unwrap();
        let t = id("01ARZ3NDEKTSV4RRFFQ69G5FAV");
        assert!(matches!(
            claim(dir.path(), &t, "w", ts("30"), &ts("00")),
            Err(DistError::NoRemote)
        ));
    }
}
