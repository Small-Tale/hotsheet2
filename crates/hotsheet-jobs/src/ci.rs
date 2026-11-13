//! The CI gate of a commit as other crates use it (HS2-3R9V0R): start or find the watch
//! Job, read its state as one enum, and wait for it to change. A caller never polls: the
//! monitor observes the provider, and [`wait_for_gate_change`] blocks on the Job store's
//! file changes.
//!
//! The watch only makes progress while the machine's monitor runs. A caller that creates
//! a watch makes sure of that with [`crate::ensure_monitor_running`].

use std::time::{Duration, Instant};

use crate::error::JobError;
use crate::github::{CiTarget, GITHUB_ACTIONS_KIND, RunFloor};
use crate::model::{Capabilities, Job, JobKind, ProducerFamily, Status, TicketLink};
use crate::store::{JobStore, NewJob};

/// The run a verdict rests on.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CiRun {
    pub id: u64,
    pub attempt: u32,
    /// The commit the run built. It is the watched commit or a descendant of it.
    pub head_sha: String,
    pub url: Option<String>,
}

/// The CI gate of one commit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GateState {
    /// Watched, no verdict yet: no covering run, or the newest one is queued or running.
    Pending,
    /// A successful push run on the shipping branch covers the commit.
    Green { run: CiRun },
    /// The newest covering run finished unsuccessfully.
    Red { run: CiRun },
    /// The branch moved to a line that does not contain the commit; no run can cover it.
    Superseded,
    /// The newest covering run was cancelled, or watching was stopped.
    Cancelled,
    /// Not watched, the watch record is gone, or its observer was lost.
    Unknown,
}

impl GateState {
    /// A verdict that will not change for this watch Job.
    pub fn is_final(&self) -> bool {
        !matches!(self, GateState::Pending | GateState::Unknown)
    }
}

/// A gate state with the Job it was read from and the store revision it was read at.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GateReading {
    pub state: GateState,
    pub job_id: Option<String>,
    /// Pass to [`JobStore::wait_for_change`] to wait for the next change of any kind.
    pub revision: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CiWatchOptions {
    /// The qualified ticket waiting on this gate, usually one in `final_testing`.
    pub ticket: Option<TicketLink>,
    /// Declare the `cancel` capability. Only the commit's own run is ever cancelled.
    pub can_cancel: bool,
}

/// The outcome record of a finished CI watch, for the ticket result summary
/// (HS2-BDH12X), the notifier (HS2-FCBYC8) and orchestration. Identifiers only.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CiOutcome {
    pub job_id: String,
    pub target: CiTarget,
    pub gate: GateState,
    pub ticket: Option<TicketLink>,
    pub summary: Option<String>,
    pub finished_ms: u64,
}

fn watches(job: &Job, target: &CiTarget) -> bool {
    job.kind.name == GITHUB_ACTIONS_KIND
        && job
            .external
            .as_ref()
            .and_then(CiTarget::parse)
            .is_some_and(|(found, _)| &found == target)
}

fn new_job(
    checkout_id: &str,
    target: &CiTarget,
    options: &CiWatchOptions,
    floor: Option<RunFloor>,
    retry_of: Option<String>,
) -> NewJob {
    NewJob {
        checkout_id: checkout_id.to_owned(),
        kind: JobKind {
            family: ProducerFamily::Watch,
            name: GITHUB_ACTIONS_KIND.to_owned(),
        },
        job_key: target.job_key(),
        title: format!(
            "{} on {} for {}",
            target.workflow,
            target.branch,
            target.short_sha()
        ),
        capabilities: Capabilities {
            cancel: options.can_cancel,
            retry: true,
            open_url: true,
        },
        ticket: options.ticket.clone(),
        external: Some(target.external(floor)),
        retry_of,
        url: None,
    }
}

/// The newest watch Job for the target, if one is still retained.
pub fn find_ci_watch(
    store: &JobStore,
    checkout_id: &str,
    target: &CiTarget,
) -> Result<Option<Job>, JobError> {
    Ok(store
        .list(checkout_id)?
        .jobs
        .into_iter()
        .rfind(|job| watches(job, target)))
}

/// Start or find the watch Job for the target. Idempotent, also across processes: any
/// retained Job for the target is returned, whatever its state, so a caller that repeats
/// the call never piles up Jobs. Use [`rewatch_ci`] to watch again after a verdict.
pub fn ensure_ci_watch(
    store: &JobStore,
    checkout_id: &str,
    target: &CiTarget,
    options: &CiWatchOptions,
) -> Result<Job, JobError> {
    let new = new_job(checkout_id, target, options, None, None);
    store
        .find_or_create(new, |job| watches(job, target))
        .map(|(job, _)| job)
}

/// Watch again after a red or cancelled verdict, for example once a fix was pushed
/// or the run was re-run. The new Job is linked to the old one and ignores the run that
/// was already judged. A target that is still being watched, or is green, is returned
/// unchanged, so this is idempotent too.
pub fn rewatch_ci(
    store: &JobStore,
    checkout_id: &str,
    target: &CiTarget,
    options: &CiWatchOptions,
) -> Result<Job, JobError> {
    let previous = find_ci_watch(store, checkout_id, target)?;
    // A lost watch is reattached by the monitor, so it counts as still being watched.
    let keep = |job: &Job| !matches!(job.status, Status::Failed | Status::Cancelled);
    let floor = previous.as_ref().and_then(|job| {
        let earlier = job
            .external
            .as_ref()
            .and_then(CiTarget::parse)
            .and_then(|(_, floor)| floor);
        earlier.max(run_of(job))
    });
    let retry_of = previous
        .as_ref()
        .filter(|job| !keep(job))
        .map(|job| job.id.clone());
    let new = new_job(checkout_id, target, options, floor, retry_of);
    store
        .find_or_create(new, |job| watches(job, target) && keep(job))
        .map(|(job, _)| job)
}

fn run_of(job: &Job) -> Option<RunFloor> {
    let refs = &job.outcome.as_ref()?.refs;
    Some(RunFloor {
        run_id: refs.get("run_id")?.parse().ok()?,
        attempt: refs.get("run_attempt")?.parse().ok()?,
    })
}

/// The gate state one watch Job stands for.
pub fn gate_of(job: &Job) -> GateState {
    let refs = job.outcome.as_ref().map(|outcome| &outcome.refs);
    let run = || {
        let refs = refs?;
        Some(CiRun {
            id: refs.get("run_id")?.parse().ok()?,
            attempt: refs.get("run_attempt")?.parse().ok()?,
            head_sha: refs.get("run_head_sha")?.clone(),
            url: refs.get("run_url").cloned(),
        })
    };
    let gate = refs.and_then(|refs| refs.get("gate")).map(String::as_str);
    match (job.status, gate) {
        (Status::Queued | Status::Running | Status::Waiting, _) => GateState::Pending,
        (Status::Succeeded, Some("green")) => {
            run().map_or(GateState::Unknown, |run| GateState::Green { run })
        }
        (Status::Failed, Some("red")) => {
            run().map_or(GateState::Unknown, |run| GateState::Red { run })
        }
        (Status::Failed, Some("superseded")) => GateState::Superseded,
        (Status::Cancelled, _) => GateState::Cancelled,
        // Lost, or failed for a reason that says nothing about the commit.
        _ => GateState::Unknown,
    }
}

/// Read the gate of the target from the newest watch Job. Never contacts the provider.
pub fn gate_state(
    store: &JobStore,
    checkout_id: &str,
    target: &CiTarget,
) -> Result<GateReading, JobError> {
    // Read the revision first: a change after it is then seen by the next wait.
    let revision = store.revision(checkout_id)?;
    let job = find_ci_watch(store, checkout_id, target)?;
    Ok(GateReading {
        state: job.as_ref().map_or(GateState::Unknown, gate_of),
        job_id: job.map(|job| job.id),
        revision,
    })
}

/// Block until the gate differs from `last` or `timeout` passes, and return the reading
/// at that moment (equal to `last` on a timeout). Wakes on the store's file changes.
pub fn wait_for_gate_change(
    store: &JobStore,
    checkout_id: &str,
    target: &CiTarget,
    last: &GateState,
    timeout: Duration,
) -> Result<GateReading, JobError> {
    let deadline = Instant::now() + timeout;
    loop {
        let reading = gate_state(store, checkout_id, target)?;
        let remaining = deadline.saturating_duration_since(Instant::now());
        if &reading.state != last || remaining.is_zero() {
            return Ok(reading);
        }
        store.wait_for_change(checkout_id, reading.revision, remaining)?;
    }
}

/// The outcome record of a finished CI watch Job; `None` for any other Job or one that
/// has not finished.
pub fn ci_outcome(job: &Job) -> Option<CiOutcome> {
    if job.kind.name != GITHUB_ACTIONS_KIND || !job.status.is_finished() {
        return None;
    }
    let (target, _) = job.external.as_ref().and_then(CiTarget::parse)?;
    Some(CiOutcome {
        job_id: job.id.clone(),
        target,
        gate: gate_of(job),
        ticket: job.ticket.clone(),
        summary: job
            .outcome
            .as_ref()
            .and_then(|outcome| outcome.summary.clone()),
        finished_ms: job.finished_ms?,
    })
}
