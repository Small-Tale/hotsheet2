//! Durable ticket-store **sync health** (HS2-ADHFKZ, `docs/02` §2.12.2).
//!
//! [`crate::sync::sync_once`] records every remote-backed cycle in a small JSON record in
//! the store's private Git directory (`<git-dir>/hotsheet-sync-health.json`): never
//! tracked, shared by the CLI and every server on the machine, and kept across restarts.
//! [`health`] combines that record with cheap live checks (no network) so the graphical
//! client and `hotsheet-cli sync --status` read one contract:
//!
//! - live conditions win: a missing remote, the HS2-RFJ1GN history-recovery guard, and an
//!   in-progress or abandoned rebase/merge are reported even when no cycle recorded them;
//! - otherwise the last recorded outcome stands, with its sanitized reason, the oversized
//!   unpublished objects behind a rejected push, and the consecutive failure count;
//! - the unpublished-commit count compares `HEAD` with the last fetched upstream, so a
//!   locally saved ticket is never implied to be remotely available.
//!
//! The record never holds credentials (remote URLs are removed from reasons) or ticket or
//! attachment content: only object paths, sizes, commit ids, counts, and Git's own
//! rejection lines.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::sync::SyncReport;

/// GitHub rejects any pushed blob over 100 MiB; the sync scan reports unpublished blobs
/// above this size when a push is rejected.
pub const OVERSIZED_OBJECT_BYTES: u64 = 100 * 1024 * 1024;

/// Offline cycles in a row before the client treats being offline as a persistent
/// failure worth a banner rather than a momentary blip.
pub const PERSISTENT_OFFLINE_FAILURES: u32 = 3;

/// A cycle that started this long ago without finishing is no longer reported as active
/// (the process that ran it most likely exited mid-cycle).
const ACTIVE_ATTEMPT_WINDOW_SECONDS: i64 = 15 * 60;

const RECORD_FILE: &str = "hotsheet-sync-health.json";
const RECORD_SCHEMA: u32 = 1;
const MAX_REASON_CHARS: usize = 600;

/// The published state of one ticket store's sync.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SyncState {
    /// The store path is not a Git repository.
    NotRepository,
    /// No Git remote (a local-only store) or a designated root whose code repository
    /// owns publication.
    NoRemote,
    /// A remote exists but no cycle has recorded an outcome that still applies.
    Pending,
    /// The last cycle found local and remote in agreement.
    UpToDate,
    /// The last cycle integrated and/or published commits.
    Synced,
    /// The remote was unreachable, refused authentication, or moved during the push.
    /// Transient: the loop retries with backoff.
    Offline,
    /// The remote refused the push (for example a file over GitHub's size limit).
    /// Retrying the same push cannot succeed; local history needs a repair.
    Rejected,
    /// A ticket body changed on both sides and the merge driver could not resolve it.
    Conflict,
    /// A rebase or merge is in progress or was abandoned in the store; sync will not
    /// start another one until it is finished or aborted.
    RebaseInProgress,
    /// A history rewrite replaced the remote; the local checkout must be recovered
    /// before it may sync (HS2-RFJ1GN).
    HistoryRecoveryRequired,
}

impl SyncState {
    /// Whether this state means local commits are not being published.
    #[must_use]
    pub fn is_failure(self) -> bool {
        matches!(
            self,
            Self::Offline
                | Self::Rejected
                | Self::Conflict
                | Self::RebaseInProgress
                | Self::HistoryRecoveryRequired
        )
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotRepository => "not_repository",
            Self::NoRemote => "no_remote",
            Self::Pending => "pending",
            Self::UpToDate => "up_to_date",
            Self::Synced => "synced",
            Self::Offline => "offline",
            Self::Rejected => "rejected",
            Self::Conflict => "conflict",
            Self::RebaseInProgress => "rebase_in_progress",
            Self::HistoryRecoveryRequired => "history_recovery_required",
        }
    }
}

impl From<SyncReport> for SyncState {
    fn from(report: SyncReport) -> Self {
        match report {
            SyncReport::NoRemote => Self::NoRemote,
            SyncReport::UpToDate => Self::UpToDate,
            SyncReport::Synced { .. } => Self::Synced,
            SyncReport::Offline => Self::Offline,
            SyncReport::Rejected => Self::Rejected,
            SyncReport::Conflict => Self::Conflict,
            SyncReport::RebaseInProgress => Self::RebaseInProgress,
            SyncReport::HistoryRecoveryRequired => Self::HistoryRecoveryRequired,
        }
    }
}

/// An unpublished Git blob larger than the remote accepts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OversizedObject {
    /// Store-relative path of the file in the unpublished commit that introduced it.
    pub path: String,
    pub bytes: u64,
    pub limit_bytes: u64,
}

/// The durable per-store record [`crate::sync::sync_once`] maintains.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncRecord {
    #[serde(default)]
    pub schema: u32,
    /// Start of the most recent cycle; later than `last_attempt_at` while one runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt_started_at: Option<String>,
    /// End of the most recent finished cycle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_attempt_at: Option<String>,
    /// End of the most recent cycle that reached the remote and left nothing unpublished.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_success_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<SyncState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default)]
    pub consecutive_failures: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub oversized: Vec<OversizedObject>,
    /// `(HEAD, upstream)` of a recorded conflict, so the same divergence is not replayed
    /// again on every pass.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conflict_heads: Option<(String, String)>,
    /// `(orig-head, attempts)` of the unfinished rebase sync last tried to continue
    /// (HS2-QR1K8R), so one replay is continued a bounded number of times.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continue_attempts: Option<(String, u32)>,
    /// When a cycle last registered the missing semantic merge driver (HS2-9TV6A2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge_driver_repaired_at: Option<String>,
}

/// What a finished cycle reports into the record beyond its [`SyncReport`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CycleDetail {
    pub reason: Option<String>,
    pub oversized: Vec<OversizedObject>,
    pub conflict_heads: Option<(String, String)>,
}

/// The health contract the server and `hotsheet-cli sync --status` expose.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncHealth {
    pub state: SyncState,
    /// A cycle is running now.
    pub syncing: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream: Option<String>,
    /// Local commits the last fetched upstream does not contain (no network).
    pub unpublished_commits: u32,
    /// Fetched upstream commits not yet integrated locally.
    pub incoming_commits: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_attempt_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_success_at: Option<String>,
    pub consecutive_failures: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub oversized: Vec<OversizedObject>,
    /// One plain-language next step, naming the headless command when one applies.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_action: Option<String>,
    /// Whether ticket merges in this checkout go through the semantic merge driver
    /// (HS2-9TV6A2): `registered` or `missing`, read live.
    pub merge_driver: crate::merge_driver::MergeDriverStatus,
    /// When a sync cycle last registered the missing driver itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merge_driver_repaired_at: Option<String>,
}

/// One linked Git ticket source's sync health.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TicketStoreSyncEntry {
    pub connection_id: String,
    pub label: String,
    pub locator: String,
    pub health: SyncHealth,
}

/// `GET /checkouts/{id}/repository/sync`: every linked Git ticket store's sync health.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TicketStoreSyncReport {
    pub checkout_id: String,
    pub stores: Vec<TicketStoreSyncEntry>,
}

/// The sync health of each Git ticket source the checkout links. Provider-hosted sources
/// (GitHub Issues, Jira, ...) publish through their provider and are not listed.
#[must_use]
pub fn checkout_report(checkout: &crate::checkouts::Checkout) -> TicketStoreSyncReport {
    TicketStoreSyncReport {
        checkout_id: checkout.id.clone(),
        stores: checkout
            .sources
            .iter()
            .filter(|source| source.provider == "git")
            .map(|source| {
                let path = Path::new(&source.locator);
                TicketStoreSyncEntry {
                    connection_id: source.connection_id.clone(),
                    label: path.file_name().map_or_else(
                        || source.locator.clone(),
                        |name| name.to_string_lossy().into_owned(),
                    ),
                    locator: source.locator.clone(),
                    health: health(path),
                }
            })
            .collect(),
    }
}

/// Read the store's sync health. Blocking (a few local `git` calls, no network).
#[must_use]
pub fn health(store_path: &Path) -> SyncHealth {
    health_at(store_path, OffsetDateTime::now_utc())
}

fn health_at(store_path: &Path, now: OffsetDateTime) -> SyncHealth {
    let record = read_record(store_path).unwrap_or_default();
    let mut health = SyncHealth {
        state: SyncState::Pending,
        syncing: is_active(&record, now),
        upstream: None,
        unpublished_commits: 0,
        incoming_commits: 0,
        last_attempt_at: record.last_attempt_at.clone(),
        last_success_at: record.last_success_at.clone(),
        consecutive_failures: record.consecutive_failures,
        reason: None,
        oversized: Vec::new(),
        next_action: None,
        merge_driver: crate::merge_driver::status(store_path),
        merge_driver_repaired_at: record.merge_driver_repaired_at.clone(),
    };
    if git_dir(store_path).is_none() {
        health.state = SyncState::NotRepository;
        health.syncing = false;
        return health;
    }
    if !crate::sync::publishes_to_remote(store_path) {
        // A designated root's Git state belongs to its code repository (HS2-V6JEBR).
        health.state = SyncState::NoRemote;
        health.syncing = false;
        return health;
    }
    if crate::repository_compaction::recovery_required(store_path) {
        health.state = SyncState::HistoryRecoveryRequired;
        health.reason = Some(
            "a history rewrite replaced the remote; recover this checkout before syncing"
                .to_string(),
        );
    } else if integration_in_progress(store_path).is_some() {
        health.state = SyncState::RebaseInProgress;
        // The cycle that found it says why it was not continued (HS2-QR1K8R).
        health.reason = record
            .reason
            .clone()
            .filter(|_| record.outcome == Some(SyncState::RebaseInProgress))
            .or_else(|| Some(crate::sync::STALLED_INTEGRATION_REASON.to_string()));
    } else if let Some(outcome) = record.outcome {
        // A live-only condition the store no longer shows has been resolved; the next
        // cycle decides the new state.
        if !matches!(
            outcome,
            SyncState::RebaseInProgress | SyncState::HistoryRecoveryRequired | SyncState::NoRemote
        ) {
            health.state = outcome;
            health.reason.clone_from(&record.reason);
            health.oversized.clone_from(&record.oversized);
        }
    }
    let (upstream, ahead, behind) = crate::sync::local_divergence(store_path);
    health.upstream = upstream;
    health.unpublished_commits = ahead;
    health.incoming_commits = behind;
    health.next_action = next_action(&health);
    health
}

fn is_active(record: &SyncRecord, now: OffsetDateTime) -> bool {
    let Some(started) = record.attempt_started_at.as_deref().and_then(parse_time) else {
        return false;
    };
    let finished = record.last_attempt_at.as_deref().and_then(parse_time);
    finished.is_none_or(|finished| started > finished)
        && (now - started).whole_seconds() < ACTIVE_ATTEMPT_WINDOW_SECONDS
}

fn next_action(health: &SyncHealth) -> Option<String> {
    Some(match health.state {
        SyncState::Rejected if !health.oversized.is_empty() => {
            "Local tickets are saved but cannot be published until the oversized file leaves \
             unpublished history (move it to Git LFS or rewrite those commits), then run \
             `hotsheet-cli sync`."
                .to_string()
        }
        SyncState::Rejected => {
            "Read the remote's reason, fix the cause (permissions, branch protection, or \
             hooks), then run `hotsheet-cli sync`."
                .to_string()
        }
        SyncState::Offline => {
            "Check the network and Git credentials; sync retries automatically, or run \
             `hotsheet-cli sync`."
                .to_string()
        }
        SyncState::Conflict if !health.merge_driver.applies() => {
            "The semantic ticket merge driver is not registered in this checkout, so ticket \
             files were merged as plain text. Run `hotsheet-cli doctor` to register it, then \
             run `hotsheet-cli sync`."
                .to_string()
        }
        SyncState::Conflict => {
            "Resolve the conflicting ticket body in the store with Git, commit, then run \
             `hotsheet-cli sync`."
                .to_string()
        }
        SyncState::RebaseInProgress => "Sync continues an unfinished rebase by itself when \
             nothing conflicts; this one needs a decision. Finish the rebase or merge in the \
             store, or run `hotsheet-cli sync --abort-stalled-rebase` to restore the local \
             branch (a backup ref is kept)."
            .to_string(),
        SyncState::HistoryRecoveryRequired => "Recover the store after the history rewrite: run \
             `hotsheet-cli checkout compact <source> --recover`."
            .to_string(),
        SyncState::Pending if health.unpublished_commits > 0 => {
            "Local commits are waiting for the next sync; run `hotsheet-cli sync` to publish now."
                .to_string()
        }
        _ => return None,
    })
}

/// Record the start of a cycle.
pub(crate) fn record_attempt_start(store_path: &Path) {
    let mut record = read_record(store_path).unwrap_or_default();
    record.attempt_started_at = Some(now_string());
    let _ = write_record(store_path, &record);
}

/// Record a finished cycle's outcome.
pub(crate) fn record_outcome(store_path: &Path, report: SyncReport, detail: CycleDetail) {
    let mut record = read_record(store_path).unwrap_or_default();
    apply_outcome(&mut record, report, detail, &now_string());
    let _ = write_record(store_path, &record);
}

fn apply_outcome(record: &mut SyncRecord, report: SyncReport, detail: CycleDetail, now: &str) {
    let state = SyncState::from(report);
    record.schema = RECORD_SCHEMA;
    record.last_attempt_at = Some(now.to_string());
    if record.attempt_started_at.is_none() {
        record.attempt_started_at = Some(now.to_string());
    }
    if state.is_failure() {
        record.consecutive_failures = record.consecutive_failures.saturating_add(1);
    } else {
        record.consecutive_failures = 0;
        if matches!(state, SyncState::UpToDate | SyncState::Synced) {
            record.last_success_at = Some(now.to_string());
        }
    }
    record.outcome = Some(state);
    record.reason = detail.reason.map(|reason| sanitize_reason(&reason));
    record.oversized = detail.oversized;
    record.conflict_heads = detail.conflict_heads;
}

/// Remember that a cycle registered the missing merge driver (HS2-9TV6A2).
pub(crate) fn record_merge_driver(
    store_path: &Path,
    status: crate::merge_driver::MergeDriverStatus,
) {
    if status != crate::merge_driver::MergeDriverStatus::Repaired {
        return;
    }
    let mut record = read_record(store_path).unwrap_or_default();
    record.merge_driver_repaired_at = Some(now_string());
    let _ = write_record(store_path, &record);
}

/// Count one more automatic continue of the unfinished rebase that started at
/// `orig_head` and return the new count (HS2-QR1K8R). The count stops one past
/// [`crate::sync_replay::MAX_CONTINUE_ATTEMPTS`], so a stall the cycle no longer continues
/// does not rewrite the record on every pass.
pub(crate) fn note_continue_attempt(store_path: &Path, orig_head: &str) -> u32 {
    let mut record = read_record(store_path).unwrap_or_default();
    let before = match &record.continue_attempts {
        Some((head, count)) if head == orig_head => *count,
        _ => 0,
    };
    let count = (before + 1).min(crate::sync_replay::MAX_CONTINUE_ATTEMPTS + 1);
    if count != before {
        record.continue_attempts = Some((orig_head.to_string(), count));
        let _ = write_record(store_path, &record);
    }
    count
}

/// Forget the continue count once no rebase or merge is unfinished.
pub(crate) fn clear_continue_attempts(store_path: &Path) {
    if let Some(mut record) = read_record(store_path)
        && record.continue_attempts.take().is_some()
    {
        let _ = write_record(store_path, &record);
    }
}

/// The recorded conflict's `(HEAD, upstream)`, when the last outcome was a conflict.
pub(crate) fn recorded_conflict(store_path: &Path) -> Option<(String, String)> {
    let record = read_record(store_path)?;
    (record.outcome == Some(SyncState::Conflict))
        .then_some(record.conflict_heads)
        .flatten()
}

/// The durable record, if one was written.
#[must_use]
pub fn read_record(store_path: &Path) -> Option<SyncRecord> {
    let path = git_dir(store_path)?.join(RECORD_FILE);
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

fn write_record(store_path: &Path, record: &SyncRecord) -> std::io::Result<()> {
    let dir = git_dir(store_path)
        .ok_or_else(|| std::io::Error::other("store is not a Git repository"))?;
    let mut temp = tempfile::NamedTempFile::new_in(&dir)?;
    serde_json::to_writer_pretty(&mut temp, record)?;
    temp.persist(dir.join(RECORD_FILE))
        .map(|_| ())
        .map_err(|error| error.error)
}

fn git_dir(store_path: &Path) -> Option<PathBuf> {
    let out = crate::git::command()
        .arg("-C")
        .arg(store_path)
        .args(["rev-parse", "--absolute-git-dir"])
        .output()
        .ok()?;
    out.status.success().then(|| {
        PathBuf::from(
            String::from_utf8_lossy(&out.stdout)
                .trim_end_matches(['\n', '\r'])
                .to_string(),
        )
    })
}

/// Which history integration is in progress (or was abandoned) in the store, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntegrationInProgress {
    Rebase,
    Merge,
}

/// Detect an in-progress or abandoned rebase or merge, before sync starts another.
#[must_use]
pub fn integration_in_progress(store_path: &Path) -> Option<IntegrationInProgress> {
    let dir = git_dir(store_path)?;
    if dir.join("rebase-merge").exists() || dir.join("rebase-apply").exists() {
        Some(IntegrationInProgress::Rebase)
    } else if dir.join("MERGE_HEAD").exists() {
        Some(IntegrationInProgress::Merge)
    } else {
        None
    }
}

/// Remove remote URLs (which may embed credentials) and cap the length of a reason
/// taken from Git's output.
#[must_use]
pub fn sanitize_reason(raw: &str) -> String {
    let cleaned = raw
        .lines()
        .map(|line| {
            line.split_whitespace()
                .map(|word| {
                    if word.contains("://") || looks_like_scp_remote(word) {
                        "<remote>"
                    } else {
                        word
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if cleaned.chars().count() <= MAX_REASON_CHARS {
        cleaned
    } else {
        let mut capped: String = cleaned.chars().take(MAX_REASON_CHARS).collect();
        capped.push('…');
        capped
    }
}

/// `user@host:path` remotes (scp syntax).
fn looks_like_scp_remote(word: &str) -> bool {
    word.split_once('@')
        .is_some_and(|(user, rest)| !user.is_empty() && rest.contains(':'))
}

fn now_string() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default()
}

fn parse_time(value: &str) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(value, &Rfc3339).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detail(reason: &str) -> CycleDetail {
        CycleDetail {
            reason: Some(reason.to_string()),
            ..CycleDetail::default()
        }
    }

    /// HS2-ADHFKZ: the failure counter and last-success time across a
    /// success → failure → failure → success → failure sequence.
    #[test]
    fn hs2_adhfkz_outcomes_count_consecutive_failures_and_keep_last_success() {
        let mut record = SyncRecord::default();
        apply_outcome(
            &mut record,
            SyncReport::UpToDate,
            CycleDetail::default(),
            "t1",
        );
        assert_eq!(record.last_success_at.as_deref(), Some("t1"));
        assert_eq!(record.consecutive_failures, 0);

        apply_outcome(&mut record, SyncReport::Rejected, detail("no"), "t2");
        apply_outcome(&mut record, SyncReport::Offline, detail("down"), "t3");
        assert_eq!(record.consecutive_failures, 2);
        assert_eq!(record.last_success_at.as_deref(), Some("t1"));
        assert_eq!(record.last_attempt_at.as_deref(), Some("t3"));
        assert_eq!(record.outcome, Some(SyncState::Offline));
        assert_eq!(record.reason.as_deref(), Some("down"));

        apply_outcome(
            &mut record,
            SyncReport::Synced {
                pulled: false,
                pushed: true,
            },
            CycleDetail::default(),
            "t4",
        );
        assert_eq!(record.consecutive_failures, 0);
        assert_eq!(record.last_success_at.as_deref(), Some("t4"));
        assert_eq!(record.reason, None);

        apply_outcome(
            &mut record,
            SyncReport::Conflict,
            CycleDetail::default(),
            "t5",
        );
        assert_eq!(record.consecutive_failures, 1);
        // NoRemote is neither a failure nor a publication.
        apply_outcome(
            &mut record,
            SyncReport::NoRemote,
            CycleDetail::default(),
            "t6",
        );
        assert_eq!(record.consecutive_failures, 0);
        assert_eq!(record.last_success_at.as_deref(), Some("t4"));
    }

    #[test]
    fn hs2_adhfkz_reasons_never_keep_remote_urls_or_unbounded_text() {
        let reason = sanitize_reason(
            "To https://user:token@github.com/o/r.git\n ! [remote rejected] main -> main\n\
             fatal: git@github.com:o/r.git refused",
        );
        assert!(!reason.contains("token"), "{reason}");
        assert!(!reason.contains("github.com"), "{reason}");
        assert!(reason.contains("[remote rejected]"), "{reason}");
        let long = sanitize_reason(&"x ".repeat(2_000));
        assert_eq!(long.chars().count(), MAX_REASON_CHARS + 1);
    }

    #[test]
    fn hs2_adhfkz_an_unfinished_attempt_is_active_only_inside_its_window() {
        let now = parse_time("2026-11-10T12:00:00Z").unwrap();
        let mut record = SyncRecord {
            attempt_started_at: Some("2026-11-10T11:59:00Z".into()),
            last_attempt_at: Some("2026-11-10T11:58:00Z".into()),
            ..SyncRecord::default()
        };
        assert!(is_active(&record, now));
        record.last_attempt_at = Some("2026-11-10T11:59:30Z".into());
        assert!(!is_active(&record, now));
        record.attempt_started_at = Some("2026-11-10T11:00:00Z".into());
        record.last_attempt_at = None;
        assert!(
            !is_active(&record, now),
            "a crashed cycle is not active forever"
        );
    }

    #[test]
    fn hs2_adhfkz_a_plain_directory_is_not_a_repository() {
        let dir = tempfile::tempdir().unwrap();
        let health = health(dir.path());
        assert_eq!(health.state, SyncState::NotRepository);
        assert!(!health.syncing);
        assert_eq!(read_record(dir.path()), None);
    }
}
