//! HS2-T16SK0: a placed step's run waits, in dispatcher mode, for the target's
//! dispatcher (or legacy integrator) to assign its exact ticket. A target whose
//! coordination pool has neither live never assigns it, so the session it was started
//! for would idle with nothing said. This is the pure decision over what the target's
//! pool shows: wait for the pool, report that nobody will assign, and after a bounded
//! wait claim the ticket for the session's own worker id.
use serde::{Deserialize, Serialize};

/// How long a queued run waits for a live dispatcher before its session claims the
/// ticket itself, unless the policy says otherwise (`placement.dispatcher_wait_minutes`).
pub const DEFAULT_DISPATCHER_WAIT_MINUTES: u32 = 2;

/// Who will assign a queued run's ticket, as the target's pool shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssignmentState {
    /// A dispatcher or a legacy integrator is live in the pool: it assigns the ticket.
    AwaitingDispatcher,
    /// Neither is live, so nobody will assign it.
    NoLiveDispatcher,
    /// The pool could not be read: nothing is concluded.
    PoolUnknown,
}

impl AssignmentState {
    pub fn code(self) -> &'static str {
        match self {
            Self::AwaitingDispatcher => "awaiting_dispatcher",
            Self::NoLiveDispatcher => "no_live_dispatcher",
            Self::PoolUnknown => "pool_unknown",
        }
    }
}

/// What one pass knows about a queued run that a session waits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssignmentInput {
    /// The target's pool shows a live dispatcher; `None` when it could not be read.
    pub dispatcher_live: Option<bool>,
    /// The target's pool shows a live legacy integrator, which also assigns.
    pub integrator_live: Option<bool>,
    /// When the run was queued for the target's dispatcher.
    pub queued_at_ms: u64,
    /// How long to wait for a dispatcher before the session claims for itself.
    pub wait_ms: u64,
    /// A claim that failed is not tried again before this time.
    pub backoff_until_ms: Option<u64>,
    pub now_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssignmentDecision {
    pub state: AssignmentState,
    /// Claim the run's ticket for the session's worker id now.
    pub claim: bool,
    /// When this decision changes by time alone: the end of the wait or of a backoff.
    pub retry_at_ms: Option<u64>,
}

/// Decide what a queued run's session does about its assignment. Pure.
///
/// - A live dispatcher or integrator assigns the ticket; nothing is done.
/// - A pool that could not be read concludes nothing and names no time: the next
///   event looks again.
/// - With neither live, the session claims the ticket for itself once the run has
///   waited `wait_ms` and no earlier claim's backoff is running; until then the
///   decision names the time it changes.
pub fn decide_assignment(input: &AssignmentInput) -> AssignmentDecision {
    let live = |value: Option<bool>| value == Some(true);
    // A dispatcher or integrator that is live assigns, whatever the other flag says.
    if live(input.dispatcher_live) || live(input.integrator_live) {
        return AssignmentDecision {
            state: AssignmentState::AwaitingDispatcher,
            claim: false,
            retry_at_ms: None,
        };
    }
    if input.dispatcher_live.is_none() || input.integrator_live.is_none() {
        return AssignmentDecision {
            state: AssignmentState::PoolUnknown,
            claim: false,
            retry_at_ms: None,
        };
    }
    let due = input
        .queued_at_ms
        .saturating_add(input.wait_ms)
        .max(input.backoff_until_ms.unwrap_or(0));
    AssignmentDecision {
        state: AssignmentState::NoLiveDispatcher,
        claim: input.now_ms >= due,
        retry_at_ms: (input.now_ms < due).then_some(due),
    }
}
