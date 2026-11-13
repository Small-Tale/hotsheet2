//! HS2-EBHDSB: durable records of work dispatched to exact target tickets.
//!
//! A run is this machine's account of one dispatch attempt. It never holds a lock of
//! its own: the target ticket's claim is the only lock, and a run records what the
//! target answered. History keeps every transition, including repeated ones.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::contract::{valid_identifier, valid_token};
use super::reconcile::RunReconciliation;
use super::storage::{Storage, decode};
use super::*;

pub const MAX_RUNS: usize = 1024;
pub const MAX_RUN_HISTORY: usize = 64;
const MAX_RUN_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    /// Recorded before the target was asked; nothing is known to be held there.
    Requested,
    /// Handed to the target's own dispatcher (marked Up Next); no claim yet.
    Queued,
    /// The target ticket is claimed for this run's worker.
    Dispatched,
    Completed,
    Failed,
    Cancelled,
    /// A person or another worker took the target ticket over.
    Intervened,
}

impl RunState {
    pub const ALL: [Self; 7] = [
        Self::Requested,
        Self::Queued,
        Self::Dispatched,
        Self::Completed,
        Self::Failed,
        Self::Cancelled,
        Self::Intervened,
    ];

    /// A run that still stands for work in progress on its target ticket.
    pub fn is_active(self) -> bool {
        matches!(self, Self::Requested | Self::Queued | Self::Dispatched)
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }

    /// The only edges a run may take. `Dispatched -> Dispatched` (a renewed lease) is
    /// an update, not a transition.
    pub fn may_become(self, next: Self) -> bool {
        use RunState::{Cancelled, Completed, Dispatched, Failed, Intervened, Queued, Requested};
        matches!(
            (self, next),
            // HS2-WBREDM: a ticket someone finished before the dispatch was recorded
            // completes the run; there is nothing left to dispatch.
            (Requested, Queued | Dispatched | Completed | Failed | Cancelled)
                | (
                    Queued,
                    Dispatched | Completed | Failed | Cancelled | Intervened
                )
                | (Dispatched, Completed | Failed | Cancelled | Intervened)
                // A run a person took over can be handed back, finish, or be dropped;
                // its ticket can also disappear from the target (HS2-WBREDM).
                | (Intervened, Dispatched | Completed | Failed | Cancelled)
        )
    }

    /// HS2-WBREDM: a person's correction may move an unfinished run to any other
    /// state, including backwards. A finished run stays finished: reopening it could
    /// put two active runs on one target ticket.
    pub fn may_be_corrected_to(self, next: Self) -> bool {
        !self.is_terminal() && self != next
    }
}

/// HS2-WBREDM: a deliberate control action recorded in a run's history. Each one is
/// its own entry, however often it is repeated or reversed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunControl {
    Pause,
    Resume,
    Handoff,
    Correct,
}

impl RunControl {
    pub const ALL: [Self; 4] = [Self::Pause, Self::Resume, Self::Handoff, Self::Correct];

    pub fn code(self) -> &'static str {
        match self {
            Self::Pause => "pause",
            Self::Resume => "resume",
            Self::Handoff => "handoff",
            Self::Correct => "correct",
        }
    }
}

/// HS2-WBREDM: the orchestrator does not act on the target for a paused run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunPause {
    pub since_ms: u64,
    /// A stable code, never free text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DispatchMode {
    /// An exact claim of the target ticket for a named worker.
    Direct,
    /// The target's own dispatcher assigns the ticket; the orchestrator only queues it.
    Dispatcher,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunTicket {
    pub connection_id: String,
    pub native_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunTarget {
    pub target_id: String,
    pub connection_id: String,
    pub native_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunTransition {
    pub at_ms: u64,
    /// Absent for the first entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<RunState>,
    pub to: RunState,
    /// A stable code, never free text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
    /// HS2-WBREDM: set when a control action made this entry. `from` then equals `to`
    /// unless the action also changed the state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control: Option<RunControl>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Run {
    pub run_id: String,
    pub owner_checkout_id: String,
    /// The caller's idempotency key for this dispatch.
    pub operation_id: String,
    /// The orchestration ticket the work is for.
    pub ticket: RunTicket,
    pub target: RunTarget,
    pub mode: DispatchMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_id: Option<String>,
    pub state: RunState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease_expires_at_ms: Option<u64>,
    /// A cancel was asked for but the target could not be told yet.
    #[serde(default)]
    pub cancel_requested: bool,
    /// HS2-7QHWHJ: the target ticket as last confirmed by a successful read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed: Option<RunObservation>,
    /// Set while the target cannot be confirmed; `observed` is then stale.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unreachable: Option<RunUnreachable>,
    /// HS2-WBREDM: set while the run is paused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paused: Option<RunPause>,
    /// HS2-WBREDM: the outcome of the last reconciliation that found something new.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reconciliation: Option<RunReconciliation>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub history: Vec<RunTransition>,
}

/// The target ticket's state as of one successful read. Codes and identifiers only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunObservation {
    pub at_ms: u64,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_phase: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease_expires_at_ms: Option<u64>,
}

impl RunObservation {
    /// The same facts, whenever they were read.
    pub fn same_facts(&self, other: &Self) -> bool {
        self.status == other.status
            && self.started_phase == other.started_phase
            && self.claimed_by == other.claimed_by
            && self.lease_expires_at_ms == other.lease_expires_at_ms
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunUnreachable {
    pub since_ms: u64,
    /// The stable code of why the target could not be confirmed.
    pub reason: String,
}

/// What a caller supplies to start a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRun {
    pub operation_id: String,
    pub ticket: RunTicket,
    pub target: RunTarget,
    pub mode: DispatchMode,
    pub worker_id: Option<String>,
}

/// A change to an existing run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunUpdate {
    pub to: Option<RunState>,
    pub reason: Option<String>,
    pub actor_id: Option<String>,
    /// `Some(None)` clears a lapsed or released lease.
    pub lease_expires_at_ms: Option<Option<u64>>,
    pub worker_id: Option<String>,
    pub cancel_requested: Option<bool>,
    /// HS2-7QHWHJ: a newer confirmed observation. One older than the stored
    /// observation is ignored, so late or replayed reads never move it backwards.
    pub observed: Option<RunObservation>,
    /// `Some(None)` records that the target answered again.
    pub unreachable: Option<Option<RunUnreachable>>,
    /// HS2-WBREDM: the control action this update is. With [`RunControl::Correct`] a
    /// state change follows [`RunState::may_be_corrected_to`] instead of the edges.
    pub control: Option<RunControl>,
    /// `Some(None)` resumes. Pausing a paused run keeps when it was first paused.
    pub paused: Option<Option<RunPause>>,
    /// A reconciliation outcome. One with the same facts as the stored outcome is
    /// ignored, so re-checking an unchanged world writes nothing.
    pub reconciliation: Option<RunReconciliation>,
    /// The change was learned from the target rather than done by the orchestrator.
    /// The registry ignores it; the event stream records it as the source.
    pub from_target: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunError {
    /// Another active run already stands for this target ticket.
    TargetActive,
    /// The worker already holds an active direct run on this target.
    WorkerBusy,
    /// The run is finished; it cannot be renewed, cancelled or changed.
    NotActive,
    InvalidTransition,
    NotFound,
    /// HS2-WBREDM: the run is paused; resume it before acting on its target.
    Paused,
}

impl RunError {
    pub fn code(self) -> &'static str {
        match self {
            Self::TargetActive => "run_target_active",
            Self::WorkerBusy => "run_worker_busy",
            Self::NotActive => "run_not_active",
            Self::InvalidTransition => "run_invalid_transition",
            Self::NotFound => "run_not_found",
            Self::Paused => "run_paused",
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RunFile {
    schema_version: u32,
    runs: Vec<Run>,
}

#[derive(Clone)]
pub struct RunRegistry {
    storage: Storage,
}

impl RunRegistry {
    pub fn new(home: &Path) -> Self {
        Self {
            storage: Storage::machine_named(home, "orchestration-runs.json"),
        }
    }

    fn read_file(&self) -> Result<RunFile, OrchestrationError> {
        let Some(bytes) = self.storage.read(MAX_RUN_BYTES)? else {
            return Ok(RunFile {
                schema_version: ORCHESTRATION_VERSION,
                runs: Vec::new(),
            });
        };
        let file: RunFile = decode(&bytes)?;
        for run in &file.runs {
            validate(run).map_err(|_| OrchestrationError::Corrupt)?;
        }
        Ok(file)
    }

    fn write_file(&self, file: &RunFile) -> Result<(), OrchestrationError> {
        let bytes = serde_json::to_vec(file).map_err(|_| OrchestrationError::Corrupt)?;
        self.storage.write(&bytes, MAX_RUN_BYTES)
    }

    pub fn get(&self, owner_checkout_id: &str, run_id: &str) -> Result<Run, OrchestrationError> {
        valid_identifier(owner_checkout_id)?;
        valid_token(run_id)?;
        self.read_file()?
            .runs
            .into_iter()
            .find(|run| run.run_id == run_id && run.owner_checkout_id == owner_checkout_id)
            .ok_or(OrchestrationError::Run(RunError::NotFound))
    }

    /// Runs of one owning checkout, oldest first.
    pub fn list(
        &self,
        owner_checkout_id: &str,
        active_only: bool,
    ) -> Result<Vec<Run>, OrchestrationError> {
        valid_identifier(owner_checkout_id)?;
        Ok(self
            .read_file()?
            .runs
            .into_iter()
            .filter(|run| run.owner_checkout_id == owner_checkout_id)
            .filter(|run| !active_only || run.state.is_active())
            .collect())
    }

    /// HS2-WBREDM: every owning checkout that has an unfinished run, so a restarted
    /// host knows which runs to reconcile without being asked about each owner.
    pub fn unfinished_owners(&self) -> Result<Vec<String>, OrchestrationError> {
        let owners: std::collections::BTreeSet<String> = self
            .read_file()?
            .runs
            .into_iter()
            .filter(|run| !run.state.is_terminal())
            .map(|run| run.owner_checkout_id)
            .collect();
        Ok(owners.into_iter().collect())
    }

    /// Start a run in `requested`, or return the run an earlier request with the same
    /// operation id started (`false`). The same operation id for a different target,
    /// ticket, mode or worker conflicts. A target ticket has at most one active run.
    pub fn begin(
        &self,
        owner_checkout_id: &str,
        new: NewRun,
        actor_id: Option<String>,
        tx: &mut Transaction<'_>,
    ) -> Result<(Run, bool), OrchestrationError> {
        valid_identifier(owner_checkout_id)?;
        let now_ms = tx.clock.now_ms();
        let mut run = Run {
            run_id: String::new(),
            owner_checkout_id: owner_checkout_id.into(),
            operation_id: new.operation_id,
            ticket: new.ticket,
            target: new.target,
            mode: new.mode,
            worker_id: new.worker_id,
            state: RunState::Requested,
            lease_expires_at_ms: None,
            cancel_requested: false,
            observed: None,
            unreachable: None,
            paused: None,
            reconciliation: None,
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
            history: vec![RunTransition {
                at_ms: now_ms,
                from: None,
                to: RunState::Requested,
                reason: None,
                actor_id,
                control: None,
            }],
        };
        match (run.mode, &run.worker_id) {
            (DispatchMode::Direct, None) => {
                return Err(OrchestrationError::Invalid(
                    "a direct dispatch names its worker",
                ));
            }
            (DispatchMode::Dispatcher, Some(_)) => {
                return Err(OrchestrationError::Invalid(
                    "the target's dispatcher chooses the worker",
                ));
            }
            _ => {}
        }
        validate_fields(&run)?;
        let _lock = self.storage.lock()?;
        let mut file = self.read_file()?;
        let owned = |other: &&Run| other.owner_checkout_id == owner_checkout_id;
        if let Some(existing) = file
            .runs
            .iter()
            .filter(owned)
            .find(|other| other.operation_id == run.operation_id)
        {
            let same = existing.ticket == run.ticket
                && existing.target == run.target
                && existing.mode == run.mode
                // A dispatcher run learns its worker later; that is not a difference.
                && (run.mode == DispatchMode::Dispatcher || existing.worker_id == run.worker_id);
            return if same {
                Ok((existing.clone(), false))
            } else {
                Err(OrchestrationError::Conflict)
            };
        }
        let active = || {
            file.runs
                .iter()
                .filter(owned)
                .filter(|r| r.state.is_active())
        };
        if active().any(|other| other.target == run.target) {
            return Err(OrchestrationError::Run(RunError::TargetActive));
        }
        if run.mode == DispatchMode::Direct
            && active().any(|other| {
                other.mode == DispatchMode::Direct
                    && other.worker_id == run.worker_id
                    && other.target.target_id == run.target.target_id
            })
        {
            return Err(OrchestrationError::Run(RunError::WorkerBusy));
        }
        run.run_id = tx.token();
        validate(&run)?;
        prune(&mut file);
        if file.runs.len() >= MAX_RUNS {
            return Err(OrchestrationError::Capacity);
        }
        file.runs.push(run.clone());
        self.write_file(&file)?;
        Ok((run, true))
    }

    /// Apply one change. A state change must follow [`RunState::may_become`]; anything
    /// else on a finished run is refused. An update that changes nothing writes nothing.
    pub fn update(
        &self,
        owner_checkout_id: &str,
        run_id: &str,
        update: RunUpdate,
        tx: &mut Transaction<'_>,
    ) -> Result<Run, OrchestrationError> {
        valid_identifier(owner_checkout_id)?;
        valid_token(run_id)?;
        let now_ms = tx.clock.now_ms();
        let _lock = self.storage.lock()?;
        let mut file = self.read_file()?;
        let run = file
            .runs
            .iter_mut()
            .find(|run| run.run_id == run_id && run.owner_checkout_id == owner_checkout_id)
            .ok_or(OrchestrationError::Run(RunError::NotFound))?;
        let before = run.clone();
        if run.state.is_terminal() {
            return Err(OrchestrationError::Run(RunError::NotActive));
        }
        let mut entry = None;
        if let Some(to) = update.to.filter(|to| *to != run.state) {
            let allowed = if update.control == Some(RunControl::Correct) {
                run.state.may_be_corrected_to(to)
            } else {
                run.state.may_become(to)
            };
            if !allowed {
                return Err(OrchestrationError::Run(RunError::InvalidTransition));
            }
            entry = Some(RunTransition {
                at_ms: now_ms,
                from: Some(run.state),
                to,
                reason: update.reason.clone(),
                actor_id: update.actor_id.clone(),
                control: update.control,
            });
            run.state = to;
            if !matches!(to, RunState::Dispatched) {
                run.lease_expires_at_ms = None;
            }
            // HS2-68DWAM: the stored outcome was assessed on the earlier state, so a
            // transition supersedes it; a finished run depends on nothing any more.
            // An outcome this update carries is stored below, for the new state.
            run.reconciliation = None;
            if to.is_terminal() {
                run.cancel_requested = false;
                run.paused = None;
            }
        }
        if let Some(lease) = update.lease_expires_at_ms {
            run.lease_expires_at_ms = lease;
        }
        if let Some(worker) = update.worker_id {
            run.worker_id = Some(worker);
        }
        if let Some(cancel) = update.cancel_requested {
            run.cancel_requested = cancel && !run.state.is_terminal();
        }
        if let Some(observed) = update.observed {
            if run
                .observed
                .as_ref()
                .is_none_or(|current| observed.at_ms >= current.at_ms)
            {
                run.observed = Some(observed);
            }
        }
        if let Some(unreachable) = update.unreachable {
            // Keep when the outage began; a repeated failure does not restart it.
            run.unreachable = match (unreachable, run.unreachable.take()) {
                (Some(_), Some(since)) => Some(since),
                (next, _) => next,
            };
        }
        if let Some(paused) = update.paused.filter(|_| !run.state.is_terminal()) {
            // Keep when the pause began; pausing again does not restart it.
            run.paused = match (paused, run.paused.take()) {
                (Some(_), Some(since)) => Some(since),
                (next, _) => next,
            };
        }
        if let Some(found) = update.reconciliation {
            if run
                .reconciliation
                .as_ref()
                .is_none_or(|current| !current.same_facts(&found))
            {
                run.reconciliation = Some(found);
            }
        }
        // HS2-WBREDM: a control action that changed the worker or the pause without
        // changing the state is still its own history entry.
        if entry.is_none()
            && update.control.is_some()
            && (run.worker_id != before.worker_id
                || run.paused.is_some() != before.paused.is_some())
        {
            entry = Some(RunTransition {
                at_ms: now_ms,
                from: Some(run.state),
                to: run.state,
                reason: update.reason,
                actor_id: update.actor_id,
                control: update.control,
            });
        }
        if let Some(entry) = entry {
            run.history.push(entry);
            // Keep the first entry (how the run began) and the most recent ones.
            if run.history.len() > MAX_RUN_HISTORY {
                run.history.remove(1);
            }
        }
        if *run == before {
            return Ok(before);
        }
        run.updated_at_ms = now_ms;
        validate(run)?;
        let updated = run.clone();
        self.write_file(&file)?;
        Ok(updated)
    }
}

/// At capacity, forget the oldest finished run; an active or taken-over run stays.
fn prune(file: &mut RunFile) {
    while file.runs.len() >= MAX_RUNS {
        let Some(index) = file.runs.iter().position(|run| run.state.is_terminal()) else {
            return;
        };
        file.runs.remove(index);
    }
}

fn native(value: &str) -> Result<(), OrchestrationError> {
    if value.is_empty()
        || value.len() > 200
        || value
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || matches!(c, '/' | '\\'))
    {
        return Err(OrchestrationError::Invalid(
            "bounded native ticket id required",
        ));
    }
    Ok(())
}

fn validate(run: &Run) -> Result<(), OrchestrationError> {
    valid_token(&run.run_id)?;
    validate_fields(run)
}

/// Everything but the run id, so a request is checked before an id is minted.
fn validate_fields(run: &Run) -> Result<(), OrchestrationError> {
    valid_identifier(&run.owner_checkout_id)?;
    valid_identifier(&run.operation_id)?;
    valid_identifier(&run.ticket.connection_id)?;
    native(&run.ticket.native_id)?;
    valid_token(&run.target.target_id)?;
    valid_identifier(&run.target.connection_id)?;
    native(&run.target.native_id)?;
    if let Some(worker) = &run.worker_id {
        valid_identifier(worker)?;
    }
    if run.history.is_empty() || run.history.len() > MAX_RUN_HISTORY {
        return Err(OrchestrationError::Invalid("bounded run history required"));
    }
    for entry in &run.history {
        for value in [&entry.reason, &entry.actor_id].into_iter().flatten() {
            valid_identifier(value)?;
        }
    }
    if let Some(observed) = &run.observed {
        for value in [
            Some(&observed.status),
            observed.started_phase.as_ref(),
            observed.claimed_by.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            valid_identifier(value)?;
        }
    }
    if let Some(unreachable) = &run.unreachable {
        valid_identifier(&unreachable.reason)?;
    }
    if let Some(paused) = &run.paused {
        for value in [&paused.reason, &paused.actor_id].into_iter().flatten() {
            valid_identifier(value)?;
        }
    }
    if let Some(found) = &run.reconciliation {
        found.validate()?;
    }
    Ok(())
}
