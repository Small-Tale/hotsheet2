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
    /// HS2-42TARA: a plan step's readiness level changed. `run_id` holds the step id
    /// (a step may have no run yet) and `reason` the new level.
    Readiness,
    /// HS2-JSF985: provisional work changed state. `run_id` holds the step id,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunEvent {
    /// When the fact was true at its source, which may be earlier than its arrival.
    pub at_ms: u64,
    pub run_id: String,
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

impl JournalRecord for RunEvent {
    const DIRECTORY: &'static str = "orchestration-run-events";
    fn validate(&self) -> Result<(), OrchestrationError> {
        valid_token(&self.run_id)?;
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
        let run_id = event.run_id.clone();
        self.0.append_unless(event, |existing| {
            existing.run_id == run_id && existing.dedupe.as_deref() == Some(key.as_str())
        })
    }

    /// Events after the cursor, for one run or all, oldest first.
    pub fn read(
        &self,
        after: u64,
        limit: usize,
        run_id: Option<&str>,
    ) -> Result<RunEventPage, OrchestrationError> {
        if let Some(run_id) = run_id {
            valid_token(run_id)?;
        }
        self.0.read_matching(after, limit, |event| {
            run_id.is_none_or(|id| event.run_id == id)
        })
    }

    #[cfg(test)]
    pub(crate) fn with_segment_limit(self, limit: u64) -> Self {
        Self(self.0.with_segment_limit(limit))
    }
}
