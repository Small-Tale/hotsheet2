//! The Git-ref compare-and-swap that the built-in `git_ref` claim backend uses
//! (HS2-WJF0D8, `docs/36` §36.14). It generalizes [`crate::distclaim`]: one marker ref per
//! resource points at an orphan commit whose message is the claim, and the remote
//! serializes every change.
//!
//! - **Acquire** pushes a new marker. When no ref exists a plain push creates it, and
//!   only the first push wins. When a lapsed or released marker exists, a push with
//!   `--force-with-lease=<ref>:<seen oid>` replaces it, so two takers cannot both win.
//! - **Renew** rewrites the holder's own marker with the same lease check.
//! - **Release** deletes the marker with the same lease check, so it never removes a
//!   claim that replaced the one being released.
//!
//! Markers keep `distclaim`'s `worker`, `expires_at` and `claimed_at` fields, so the
//! distributed work loop and exact claims of the same ticket exclude each other. The
//! clock is the claimer's; machines that share a remote need roughly synchronized clocks.
//! The fencing token is `max(previous + 1, acquire time in ms)`, which keeps increasing
//! across a deleted marker as long as clocks move forward.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use hotsheet_model::Timestamp;

use crate::distclaim::{git, git_stdin, looks_like_reject};

/// The ref namespace of ticket claims, shared with [`crate::distclaim`].
pub const TICKET_REF_PREFIX: &str = "refs/hotsheet/claims/";
/// The ref namespace of non-ticket resource claims.
pub const RESOURCE_REF_PREFIX: &str = "refs/hotsheet/resource-claims/";

/// One claim marker. Unknown fields from a newer writer are ignored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefMarker {
    pub worker: String,
    pub expires_at: Timestamp,
    pub claimed_at: Timestamp,
    /// The claim's identity. A `distclaim` marker has none.
    #[serde(default)]
    pub claim_id: String,
    #[serde(default)]
    pub fencing: u64,
    #[serde(default)]
    pub acquired_at_ms: u64,
    #[serde(default)]
    pub expires_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// SHA-256 of a resource claim's renewal token (a 256-bit random secret, so an
    /// unkeyed digest is not guessable).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_digest: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
}

impl RefMarker {
    pub fn new(worker: &str, claim_id: &str, acquired_at_ms: u64, expires_at_ms: u64) -> Self {
        Self {
            worker: worker.to_owned(),
            expires_at: timestamp(expires_at_ms),
            claimed_at: timestamp(acquired_at_ms),
            claim_id: claim_id.to_owned(),
            fencing: 0,
            acquired_at_ms,
            expires_at_ms,
            host: None,
            token_digest: None,
            metadata: BTreeMap::new(),
        }
    }

    /// The lease end in ms; a `distclaim` marker carries only `expires_at`.
    pub fn expiry_ms(&self) -> u64 {
        if self.expires_at_ms > 0 {
            return self.expires_at_ms;
        }
        self.expires_at.instant().map_or(0, |at| {
            u64::try_from(at.unix_timestamp_nanos() / 1_000_000).unwrap_or(0)
        })
    }

    pub fn is_live(&self, now_ms: u64) -> bool {
        self.expiry_ms() > now_ms
    }

    pub fn with_expiry(mut self, expires_at_ms: u64) -> Self {
        self.expires_at_ms = expires_at_ms;
        self.expires_at = timestamp(expires_at_ms);
        self
    }
}

/// Milliseconds since the epoch as an RFC 3339 timestamp.
pub fn timestamp(ms: u64) -> Timestamp {
    let nanos = i128::from(ms) * 1_000_000;
    time::OffsetDateTime::from_unix_timestamp_nanos(nanos)
        .map(Timestamp::from_datetime)
        .unwrap_or_else(|_| Timestamp::new(ms.to_string()))
}

/// Milliseconds since the epoch of a timestamp, or 0.
pub fn millis(at: &Timestamp) -> u64 {
    at.instant().map_or(0, |at| {
        u64::try_from(at.unix_timestamp_nanos() / 1_000_000).unwrap_or(0)
    })
}

#[derive(Debug, thiserror::Error)]
pub enum CasError {
    /// The remote could not be reached; nothing changed.
    #[error("{0}")]
    Unreachable(String),
    /// A local Git step failed.
    #[error("git {0}")]
    Git(String),
}

/// What an acquire found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Acquired {
    /// This marker now holds the resource.
    Won(RefMarker),
    /// A live marker of another claim holds it.
    Held(RefMarker),
}

/// The marker refs of one repository's remote.
#[derive(Debug, Clone)]
pub struct GitRefCas {
    repository: PathBuf,
    remote: String,
}

const ATTEMPTS: usize = 4;

impl GitRefCas {
    pub fn new(repository: impl Into<PathBuf>, remote: impl Into<String>) -> Self {
        Self {
            repository: repository.into(),
            remote: remote.into(),
        }
    }

    pub fn repository(&self) -> &Path {
        &self.repository
    }

    pub fn ticket_ref(id: &hotsheet_model::Ulid) -> String {
        format!("{TICKET_REF_PREFIX}{id}")
    }

    fn git(&self, args: &[&str]) -> (bool, String, String) {
        git(&self.repository, args)
    }

    /// The current marker of `refname` and the oid it points at, read from the remote.
    pub fn read(&self, refname: &str) -> Result<Option<(String, RefMarker)>, CasError> {
        let (ok, out, err) = self.git(&["ls-remote", &self.remote, refname]);
        if !ok {
            return Err(CasError::Unreachable(err.trim().to_owned()));
        }
        let Some(oid) = out
            .lines()
            .find(|line| line.split_whitespace().nth(1) == Some(refname))
            .and_then(|line| line.split_whitespace().next())
            .map(str::to_owned)
        else {
            return Ok(None);
        };
        Ok(Some((oid.clone(), self.marker_at(refname, &oid)?)))
    }

    fn marker_at(&self, refname: &str, oid: &str) -> Result<RefMarker, CasError> {
        if !self
            .git(&["cat-file", "-e", &format!("{oid}^{{commit}}")])
            .0
        {
            let (ok, _, err) = self.git(&[
                "fetch",
                "--quiet",
                "--no-tags",
                &self.remote,
                &format!("+{refname}:{refname}"),
            ]);
            if !ok {
                return Err(CasError::Unreachable(err.trim().to_owned()));
            }
        }
        let (ok, message, err) = self.git(&["show", "-s", "--format=%B", oid]);
        if !ok {
            return Err(CasError::Git(format!("show: {}", err.trim())));
        }
        serde_json::from_str(message.trim())
            .map_err(|error| CasError::Git(format!("unreadable claim marker {refname}: {error}")))
    }

    /// Every marker under `prefix` (a ref namespace ending in `/`), by ref name.
    pub fn list(&self, prefix: &str) -> Result<Vec<(String, RefMarker)>, CasError> {
        let pattern = format!("{prefix}*");
        let (ok, out, err) = self.git(&["ls-remote", &self.remote, &pattern]);
        if !ok {
            return Err(CasError::Unreachable(err.trim().to_owned()));
        }
        let mut markers = Vec::new();
        for line in out.lines() {
            let mut parts = line.split_whitespace();
            let (Some(oid), Some(refname)) = (parts.next(), parts.next()) else {
                continue;
            };
            if let Ok(marker) = self.marker_at(refname, oid) {
                markers.push((refname.to_owned(), marker));
            }
        }
        markers.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(markers)
    }

    fn commit(&self, marker: &RefMarker) -> Result<String, CasError> {
        let json = serde_json::to_string(marker).map_err(|e| CasError::Git(e.to_string()))?;
        let (ok, tree, err) = git_stdin(&self.repository, &["mktree"], "");
        if !ok {
            return Err(CasError::Git(format!("mktree: {}", err.trim())));
        }
        let (ok, commit, err) = self.git(&[
            "-c",
            "user.name=Hot Sheet",
            "-c",
            "user.email=hotsheet@localhost",
            "commit-tree",
            tree.trim(),
            "-m",
            &json,
        ]);
        if !ok {
            return Err(CasError::Git(format!("commit-tree: {}", err.trim())));
        }
        Ok(commit.trim().to_owned())
    }

    /// Push `spec`, optionally only while `refname` still points at `seen`. `Ok(false)`
    /// is a lost race.
    fn push(&self, spec: &str, refname: &str, seen: Option<&str>) -> Result<bool, CasError> {
        let lease = seen.map(|oid| format!("--force-with-lease={refname}:{oid}"));
        let mut args = vec!["push", "--quiet"];
        if let Some(lease) = &lease {
            args.push(lease);
        }
        args.extend([self.remote.as_str(), spec]);
        let (ok, _, err) = self.git(&args);
        if ok {
            Ok(true)
        } else if looks_like_reject(&err) {
            Ok(false)
        } else {
            Err(CasError::Unreachable(err.trim().to_owned()))
        }
    }

    /// Take `refname` for `marker` unless a live marker of another claim holds it. A live
    /// marker of the same claim is kept (an idempotent retry) and its lease extended.
    pub fn acquire(
        &self,
        refname: &str,
        mut marker: RefMarker,
        now_ms: u64,
    ) -> Result<Acquired, CasError> {
        for _ in 0..ATTEMPTS {
            let current = self.read(refname)?;
            let seen = match &current {
                Some((_, held)) if held.is_live(now_ms) && held.claim_id != marker.claim_id => {
                    return Ok(Acquired::Held(held.clone()));
                }
                Some((_, held))
                    if held.claim_id == marker.claim_id && !marker.claim_id.is_empty() =>
                {
                    marker.fencing = held.fencing;
                    marker.acquired_at_ms = held.acquired_at_ms;
                    marker.claimed_at = held.claimed_at.clone();
                    current.as_ref().map(|(oid, _)| oid.as_str())
                }
                Some((oid, held)) => {
                    marker.fencing = (held.fencing + 1).max(now_ms);
                    Some(oid.as_str())
                }
                None => {
                    marker.fencing = now_ms.max(1);
                    None
                }
            };
            let commit = self.commit(&marker)?;
            if self.push(&format!("{commit}:{refname}"), refname, seen)? {
                return Ok(Acquired::Won(marker));
            }
        }
        match self.read(refname)? {
            Some((_, held)) if held.claim_id != marker.claim_id => Ok(Acquired::Held(held)),
            _ => Err(CasError::Git(format!("{refname} kept changing; try again"))),
        }
    }

    /// Replace the marker of `claim_id` with `next` (same claim). `Ok(None)` when no
    /// marker of that claim exists any more; `Ok(Some(other))` when another claim holds
    /// the ref.
    pub fn renew(
        &self,
        refname: &str,
        claim_id: &str,
        expires_at_ms: u64,
    ) -> Result<Result<RefMarker, Option<RefMarker>>, CasError> {
        for _ in 0..ATTEMPTS {
            let Some((oid, held)) = self.read(refname)? else {
                return Ok(Err(None));
            };
            if held.claim_id != claim_id {
                return Ok(Err(Some(held)));
            }
            let next = held.with_expiry(expires_at_ms);
            let commit = self.commit(&next)?;
            if self.push(&format!("{commit}:{refname}"), refname, Some(&oid))? {
                return Ok(Ok(next));
            }
        }
        Err(CasError::Git(format!("{refname} kept changing; try again")))
    }

    /// Delete the marker of `claim_id`. `Ok(false)` when it is not (any longer) there.
    pub fn release(&self, refname: &str, claim_id: &str) -> Result<bool, CasError> {
        for _ in 0..ATTEMPTS {
            let Some((oid, held)) = self.read(refname)? else {
                return Ok(false);
            };
            if held.claim_id != claim_id {
                return Ok(false);
            }
            if self.push(&format!(":{refname}"), refname, Some(&oid))? {
                let _ = self.git(&["update-ref", "-d", refname]);
                return Ok(true);
            }
        }
        Err(CasError::Git(format!("{refname} kept changing; try again")))
    }
}

#[cfg(test)]
#[path = "gitref_cas_tests.rs"]
mod tests;
