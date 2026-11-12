//! The Job record (`docs/33-jobs.md` §33.1). Every text field is bounded and validated on
//! the way in, so `job.json` stays a small fixed-shape record.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::error::JobError;
use crate::steps::{Progress, Step, rollup};

/// The on-disk record version. A reader rejects any other value without rewriting it.
pub const RECORD_VERSION: u32 = 1;

pub(crate) const MAX_TOKEN: usize = 128;
pub(crate) const MAX_TITLE: usize = 200;
pub(crate) const MAX_KEY: usize = 200;
pub(crate) const MAX_SUMMARY: usize = 1024;
pub(crate) const MAX_URL: usize = 2048;
pub(crate) const MAX_LATEST: usize = 8;
pub(crate) const MAX_REFS: usize = 16;
pub(crate) const MAX_VALUE: usize = 512;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Queued,
    Running,
    /// An active producer is waiting on an external condition.
    Waiting,
    Succeeded,
    Failed,
    Cancelled,
    /// The producer stopped reporting without an outcome.
    Lost,
}

impl Status {
    pub const ALL: [Status; 7] = [
        Status::Queued,
        Status::Running,
        Status::Waiting,
        Status::Succeeded,
        Status::Failed,
        Status::Cancelled,
        Status::Lost,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Status::Queued => "queued",
            Status::Running => "running",
            Status::Waiting => "waiting",
            Status::Succeeded => "succeeded",
            Status::Failed => "failed",
            Status::Cancelled => "cancelled",
            Status::Lost => "lost",
        }
    }

    /// A producer holds (or held) a lease and may still report.
    pub fn is_active(self) -> bool {
        matches!(self, Status::Running | Status::Waiting)
    }

    /// No producer is expected to report again without a reattach. `lost` counts: it is
    /// terminal for owned and reported producers and retained like a failure.
    pub fn is_finished(self) -> bool {
        matches!(
            self,
            Status::Succeeded | Status::Failed | Status::Cancelled | Status::Lost
        )
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The two states a live producer can report between outcomes.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActiveStatus {
    Running,
    Waiting,
}

impl From<ActiveStatus> for Status {
    fn from(value: ActiveStatus) -> Self {
        match value {
            ActiveStatus::Running => Status::Running,
            ActiveStatus::Waiting => Status::Waiting,
        }
    }
}

/// The two outcomes a producer reports. Cancellation and loss have their own paths.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FinalStatus {
    Succeeded,
    Failed,
}

impl From<FinalStatus> for Status {
    fn from(value: FinalStatus) -> Self {
        match value {
            FinalStatus::Succeeded => Status::Succeeded,
            FinalStatus::Failed => Status::Failed,
        }
    }
}

/// Where a Job's truth lives (`docs/33-jobs.md` §33.4).
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProducerFamily {
    /// A child process Hot Sheet launched, identified by its process-journal launch id.
    Owned,
    /// An external operation that a monitor observes and can reattach to.
    Watch,
    /// An outside producer reporting through the CLI or MCP.
    Reported,
}

impl ProducerFamily {
    pub const ALL: [ProducerFamily; 3] = [
        ProducerFamily::Owned,
        ProducerFamily::Watch,
        ProducerFamily::Reported,
    ];
}

impl fmt::Display for ProducerFamily {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ProducerFamily::Owned => "owned",
            ProducerFamily::Watch => "watch",
            ProducerFamily::Reported => "reported",
        })
    }
}

/// A producer kind: its family plus a stable name such as `github-actions`.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct JobKind {
    pub family: ProducerFamily,
    pub name: String,
}

/// Actions the kind declares. A client offers only declared actions.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    #[serde(default)]
    pub cancel: bool,
    #[serde(default)]
    pub retry: bool,
    #[serde(default)]
    pub open_url: bool,
}

/// The qualified ticket that spawned a Job. Routing never uses a bare native id.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TicketLink {
    pub connection_id: String,
    pub native_id: String,
}

/// The external operation a watch Job observes, for example one CI run. A reattach must
/// present the same reference.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalRef {
    pub system: String,
    pub id: String,
}

/// The producer's claim on a Job. `generation` grows by one each time a lease is taken.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Lease {
    pub owner: String,
    pub generation: u64,
    pub expires_ms: u64,
    /// Process-journal launch id of an owned process. Never a bare PID.
    pub launch_id: Option<String>,
    /// Highest producer report sequence accepted under this lease.
    pub last_report_seq: Option<u64>,
}

/// What a producer presents with every write.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LeaseToken {
    pub owner: String,
    pub generation: u64,
}

/// The immutable result. `refs` carries identifiers (a run id, a commit) for the ticket
/// result summary and notifications; it never carries feed lines.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Outcome {
    pub summary: Option<String>,
    #[serde(default)]
    pub refs: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EtaSource {
    Producer,
    /// The median duration of completed Jobs with the same job key.
    History,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct Eta {
    pub finish_at_ms: u64,
    pub source: EtaSource,
}

/// Derived producer liveness. `Stalled` is not a stored status: the lease expired and
/// reconciliation has not yet declared the Job lost.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Liveness {
    /// Queued: no producer yet.
    Idle,
    Live,
    Stalled,
    /// Succeeded, failed, cancelled or lost.
    Finished,
}

/// Feed cursor bounds kept in the snapshot so a reader can detect a gap.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FeedState {
    /// The oldest sequence still retained. Equal to `next_seq` when nothing is retained.
    pub first_seq: u64,
    /// The sequence the next entry receives. Starts at 1 and never goes back.
    pub next_seq: u64,
}

impl Default for FeedState {
    fn default() -> Self {
        Self {
            first_seq: 1,
            next_seq: 1,
        }
    }
}

/// The `job.json` snapshot.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Job {
    pub version: u32,
    /// ULID.
    pub id: String,
    pub checkout_id: String,
    pub kind: JobKind,
    /// Groups comparable runs for historical ETA. Not the Job id.
    pub job_key: String,
    pub title: String,
    pub capabilities: Capabilities,
    pub ticket: Option<TicketLink>,
    pub external: Option<ExternalRef>,
    /// The Job this one retries.
    pub retry_of: Option<String>,
    /// Present only when the kind declares `open_url`.
    pub url: Option<String>,
    pub status: Status,
    pub created_ms: u64,
    pub updated_ms: u64,
    pub started_ms: Option<u64>,
    pub finished_ms: Option<u64>,
    /// Grows by one on every write of this record.
    pub revision: u64,
    pub lease: Option<Lease>,
    /// A cancel request is not a transition; the producer confirms or reports failure.
    pub cancel_requested_ms: Option<u64>,
    pub cancel_failure: Option<String>,
    /// A watch Job was asked to stop observing. Distinct from cancel: the external
    /// operation is left alone.
    pub stop_watching_requested_ms: Option<u64>,
    pub producer_eta_ms: Option<u64>,
    /// Median duration of earlier successful Jobs with this job key, when enough exist.
    pub history_estimate_ms: Option<u64>,
    pub outcome: Option<Outcome>,
    pub steps: Vec<Step>,
    /// A few latest-value fields such as the current test or CI step.
    pub latest: BTreeMap<String, String>,
    pub feed: FeedState,
}

impl Job {
    pub fn liveness(&self, now_ms: u64) -> Liveness {
        match self.status {
            Status::Queued => Liveness::Idle,
            Status::Running | Status::Waiting => match &self.lease {
                Some(lease) if now_ms < lease.expires_ms => Liveness::Live,
                _ => Liveness::Stalled,
            },
            _ => Liveness::Finished,
        }
    }

    /// The producer's ETA when it gave one, otherwise the history estimate from the start
    /// time. A finished or unstarted Job, or one with too little history, has none.
    pub fn eta(&self) -> Option<Eta> {
        if !self.status.is_active() {
            return None;
        }
        if let Some(finish_at_ms) = self.producer_eta_ms {
            return Some(Eta {
                finish_at_ms,
                source: EtaSource::Producer,
            });
        }
        let started = self.started_ms?;
        let estimate = self.history_estimate_ms?;
        Some(Eta {
            finish_at_ms: started.saturating_add(estimate),
            source: EtaSource::History,
        })
    }

    /// Weighted rollup of the step tree; `None` without steps.
    pub fn progress(&self) -> Option<Progress> {
        rollup(&self.steps)
    }

    pub fn token(&self) -> Option<LeaseToken> {
        self.lease.as_ref().map(|lease| LeaseToken {
            owner: lease.owner.clone(),
            generation: lease.generation,
        })
    }
}

/// An identifier that is safe as a path component and in a record.
pub(crate) fn token(field: &'static str, value: &str) -> Result<(), JobError> {
    let ok = !value.is_empty()
        && value.len() <= MAX_TOKEN
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b));
    if ok {
        Ok(())
    } else {
        Err(JobError::Invalid(field))
    }
}

/// A checkout id is also a directory name, so `:` is refused as well.
pub(crate) fn checkout_token(value: &str) -> Result<(), JobError> {
    token("checkout_id", value)?;
    if value.contains(':') || value.starts_with('.') {
        return Err(JobError::Invalid("checkout_id"));
    }
    Ok(())
}

pub(crate) fn job_id(value: &str) -> Result<(), JobError> {
    match ulid::Ulid::from_string(value) {
        Ok(parsed) if parsed.to_string() == value => Ok(()),
        _ => Err(JobError::Invalid("job_id")),
    }
}

/// Bounded single-line text without control characters.
pub(crate) fn text(field: &'static str, value: &str, max: usize) -> Result<(), JobError> {
    if value.trim().is_empty() || value.len() > max || value.chars().any(char::is_control) {
        Err(JobError::Invalid(field))
    } else {
        Ok(())
    }
}

pub(crate) fn optional_text(
    field: &'static str,
    value: Option<&str>,
    max: usize,
) -> Result<(), JobError> {
    value.map_or(Ok(()), |value| text(field, value, max))
}

pub(crate) fn validate_ticket(link: Option<&TicketLink>) -> Result<(), JobError> {
    let Some(link) = link else { return Ok(()) };
    text("ticket.connection_id", &link.connection_id, MAX_TOKEN)?;
    text("ticket.native_id", &link.native_id, MAX_KEY)
}

pub(crate) fn validate_external(external: Option<&ExternalRef>) -> Result<(), JobError> {
    let Some(external) = external else {
        return Ok(());
    };
    token("external.system", &external.system)?;
    text("external.id", &external.id, MAX_KEY)
}

pub(crate) fn validate_outcome(outcome: &Outcome) -> Result<(), JobError> {
    optional_text("outcome.summary", outcome.summary.as_deref(), MAX_SUMMARY)?;
    if outcome.refs.len() > MAX_REFS {
        return Err(JobError::Invalid("outcome.refs"));
    }
    for (key, value) in &outcome.refs {
        token("outcome.refs", key)?;
        text("outcome.refs", value, MAX_VALUE)?;
    }
    Ok(())
}
