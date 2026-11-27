//! HS2-7QHWHJ: the bounded event stream of orchestration runs.
//!
//! One append-only journal per owning checkout with a sequence cursor, so a reader
//! replays from where it stopped and learns when retention removed entries it had not
//! seen. Events carry identifiers, states and codes only.

use serde::{Deserialize, Serialize};

use super::audit::{Journal, JournalEntry, JournalPage, JournalRecord};
use super::contract::{valid_identifier, valid_token};
use super::{GateState, OrchestrationError, RunState};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunEventKind {
    /// The run changed state (`state` is the new one, `reason` why).
    State,
    /// The run's lease was extended.
    Lease,
    /// The target ticket was read and differs from the last confirmed observation.
    Observed,
    /// The target could not be confirmed; the last confirmed state is now stale.
    TargetUnreachable,
    /// The target answered again after being unreachable.
    TargetReachable,
    /// A cancel was asked for but the target could not be told yet.
    CancelRequested,
    /// A CI or deployment gate of the run changed (HS2-5JBYPV).
    Gate,
    /// A validation step of the run changed (HS2-WN0MXZ).
    Validation,
    /// An orchestrator-owned session of the run changed (HS2-GDJXGZ).
    Session,
    /// HS2-WBREDM: a reconciliation reached a new outcome (`reason` is its disposition).
    Reconciled,
    /// HS2-WBREDM: the run was paused, resumed, handed off or corrected by hand.
    Paused,
    Resumed,
    Handoff,
    Corrected,
    /// HS2-42TARA: a plan step's readiness level changed. `step_id` names the step
    /// (a step may have no run yet; HS2-J7RG42) and `reason` the new level.
    Readiness,
    /// HS2-JSF985: provisional work changed state. `step_id` names the step,
    /// `reason` the new state (`provisional`, `confirmed`, `at_risk`, `rebased`,
    /// `reopened`, `reevaluate`) and `gate_id` the gate that caused it, when one did.
    Provisional,
}

/// Who learned the fact: the orchestrator acting, or a change observed on the target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunEventSource {
    Orchestrator,
    Target,
}

impl RunEventKind {
    /// HS2-J7RG42: an event about a plan step, which may have no run, rather than a run.
    pub fn is_step_event(self) -> bool {
        matches!(self, Self::Readiness | Self::Provisional)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, from = "StoredRunEvent")]
pub struct RunEvent {
    /// When the fact was true at its source, which may be earlier than its arrival.
    pub at_ms: u64,
    /// The run the event is about. HS2-J7RG42: absent from a plan step's event.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    /// HS2-J7RG42: the plan step a `readiness` or `provisional` event is about.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_id: Option<String>,
    pub kind: RunEventKind,
    pub source: RunEventSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<RunState>,
    /// A stable code, never free text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
    /// Identifies the fact itself. Two reports with the same key for one run are the
    /// same event and are recorded once, however often or in whatever order they arrive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dedupe: Option<String>,
    /// HS2-5JBYPV: the gate a `gate` event is about, and the state it entered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate_state: Option<GateState>,
}

/// HS2-J7RG42: an event as stored. Events written before the step field carried a
/// step's id in `run_id`; they read as step events with no run.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredRunEvent {
    at_ms: u64,
    #[serde(default)]
    run_id: Option<String>,
    #[serde(default)]
    step_id: Option<String>,
    kind: RunEventKind,
    source: RunEventSource,
    #[serde(default)]
    state: Option<RunState>,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    actor_id: Option<String>,
    #[serde(default)]
    dedupe: Option<String>,
    #[serde(default)]
    gate_id: Option<String>,
    #[serde(default)]
    gate_state: Option<GateState>,
}

impl From<StoredRunEvent> for RunEvent {
    fn from(stored: StoredRunEvent) -> Self {
        let (run_id, step_id) = match (stored.kind.is_step_event(), stored.run_id, stored.step_id) {
            (true, Some(old), None) => (None, Some(old)),
            (_, run_id, step_id) => (run_id, step_id),
        };
        Self {
            at_ms: stored.at_ms,
            run_id,
            step_id,
            kind: stored.kind,
            source: stored.source,
            state: stored.state,
            reason: stored.reason,
            actor_id: stored.actor_id,
            dedupe: stored.dedupe,
            gate_id: stored.gate_id,
            gate_state: stored.gate_state,
        }
    }
}

impl RunEvent {
    /// The run or plan step the event is about.
    pub fn subject(&self) -> Option<&str> {
        self.run_id.as_deref().or(self.step_id.as_deref())
    }

    /// Whether the event is about this run or this plan step.
    pub fn concerns(&self, id: &str) -> bool {
        self.run_id.as_deref() == Some(id) || self.step_id.as_deref() == Some(id)
    }
}

impl JournalRecord for RunEvent {
    const DIRECTORY: &'static str = "orchestration-run-events";
    fn validate(&self) -> Result<(), OrchestrationError> {
        // A step's event names its step; any other event names its run.
        match (self.kind.is_step_event(), &self.run_id, &self.step_id) {
            (true, run_id, Some(step_id)) => {
                valid_token(step_id)?;
                if let Some(run_id) = run_id {
                    valid_token(run_id)?;
                }
            }
            (false, Some(run_id), None) => valid_token(run_id)?,
            _ => {
                return Err(OrchestrationError::Invalid(
                    "a run event names its run, a step event its step",
                ));
            }
        }
        for value in [&self.reason, &self.actor_id, &self.dedupe]
            .into_iter()
            .flatten()
        {
            valid_identifier(value)?;
        }
        if let Some(gate_id) = &self.gate_id {
            valid_token(gate_id)?;
        }
        Ok(())
    }
}

pub type RunEventEntry = JournalEntry<RunEvent>;
pub type RunEventPage = JournalPage<RunEvent>;

/// The run event journal of one owning checkout.
#[derive(Clone)]
pub struct RunEventJournal(Journal<RunEvent>);

impl RunEventJournal {
    pub fn new(
        home: &std::path::Path,
        owner_checkout_id: &str,
    ) -> Result<Self, OrchestrationError> {
        Journal::new(home, owner_checkout_id).map(Self)
    }

    /// The directory a waiter watches to learn of a new event without polling.
    pub fn directory(&self) -> &std::path::Path {
        self.0.directory()
    }

    /// Record an event once. An event with a `dedupe` key that a retained event of the
    /// same run already carries is not appended again; the first report is returned.
    pub fn record(&self, event: RunEvent) -> Result<(RunEventEntry, bool), OrchestrationError> {
        let Some(key) = event.dedupe.clone() else {
            return self.0.append(event).map(|entry| (entry, true));
        };
        let (run_id, step_id) = (event.run_id.clone(), event.step_id.clone());
        self.0.append_unless(event, |existing| {
            existing.run_id == run_id
                && existing.step_id == step_id
                && existing.dedupe.as_deref() == Some(key.as_str())
        })
    }

    /// HS2-KXKA7X: the journal's end, the cursor a new reader starts from so it reads
    /// only what is appended after it.
    pub fn head(&self) -> Result<u64, OrchestrationError> {
        self.0
            .read_matching(0, 1, |_| false)
            .map(|page| page.cursor)
    }

    /// Events after the cursor, for one run or plan step or all, oldest first.
    pub fn read(
        &self,
        after: u64,
        limit: usize,
        subject: Option<&str>,
    ) -> Result<RunEventPage, OrchestrationError> {
        if let Some(subject) = subject {
            valid_token(subject)?;
        }
        self.0.read_matching(after, limit, |event| {
            subject.is_none_or(|id| event.concerns(id))
        })
    }

    #[cfg(test)]
    pub(crate) fn with_segment_limit(self, limit: u64) -> Self {
        Self(self.0.with_segment_limit(limit))
    }
}
