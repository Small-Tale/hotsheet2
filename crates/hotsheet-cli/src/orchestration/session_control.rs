//! HS2-ECWG50: the seam between the scheduler, which decides where sessions run, and
//! whoever starts and stops them (HS2-GDJXGZ). The scheduler knows a session only
//! through its own [`ScheduledSession`]; an executor maps its records onto it.
//! HS2-95SWQ4: every call is handed the runtime it serves and the pass's transaction,
//! so an executor keeps no state of its own and acts on the pass's clock.
use std::sync::Mutex;

use hotsheet_ticketing::orchestration::{
    MachineLimits, ResourceNeed, ScheduledSession, SessionActivity, SessionLoad, Transaction,
};
use serde::Serialize;

use super::OrchestrationRuntime;

/// Why the sessions could not be listed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionExecutorError {
    /// No executor is wired into this process.
    Unavailable,
    /// The executor failed, with a stable code.
    Failed(String),
}

/// One session the scheduler wants started. The executor passes the gate for the
/// start itself (§26.10) with this `load` and `operation_id`, asks the target's
/// machine for admission, and may answer a refusal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlacementStart {
    pub owner_id: String,
    pub target_id: String,
    pub tool: String,
    pub account: Option<String>,
    pub model: Option<String>,
    /// Stable for one piece of work, so a repeated request is the same start.
    pub operation_id: String,
    pub actor_id: String,
    /// The ready plan steps the session is for. The target's dispatcher still assigns
    /// each exact ticket.
    pub steps: Vec<String>,
    /// Sessions running when the decision was taken, for the policy's caps.
    pub load: SessionLoad,
    /// The policy's machine limits for the target, which travel with an admission.
    pub limits: MachineLimits,
    /// The tool account is not verified available.
    pub probe: bool,
    /// HS2-2FG91X: resources the steps' work requires, which the session claims
    /// itself with the claim service's primitives (`docs/36-resource-claims.md`).
    /// Neither the scheduler nor the executor claims, renews or releases for it.
    pub resources: Vec<ResourceNeed>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartOutcome {
    Started {
        session_id: String,
        /// HS2-YC2P7K: the run dispatched for the session's step, when the executor
        /// dispatches one.
        run_id: Option<String>,
    },
    /// HS2-DG0YPA: a live session already works the step's run; it is adopted and
    /// nothing is launched.
    Adopted {
        session_id: String,
        run_id: String,
    },
    /// The gate, the target or its machine refused, with a stable code.
    Refused {
        reason: String,
    },
    /// HS2-XHQ1EH: the start or its step's run waits for a person's answer to this
    /// orchestration permission request. Nothing started and the target is not
    /// backed off: the answer wakes the scheduler.
    PermissionRequired {
        request_id: String,
    },
    Unavailable,
}

/// One session the scheduler wants wound down at its next safe point.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WindDownSession {
    pub owner_id: String,
    pub target_id: String,
    pub session_id: String,
    pub tool: String,
    pub model: Option<String>,
    pub operation_id: String,
    pub actor_id: String,
    /// Why, as the placement reason code (`idle`, `over_cap`, `policy_not_permitted`).
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WindDownOutcome {
    /// The session will stop at its safe point; it may still be listed until then,
    /// with `winding_down` set.
    Requested,
    /// It is no longer at a safe point; nothing was asked of it.
    NotAtSafePoint,
    NotFound,
    Refused {
        reason: String,
    },
    Unavailable,
}

/// HS2-YC2P7K: a running session the scheduler hands ready work (`refill`). The
/// executor dispatches the step's run and links it to the session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlacementAssign {
    pub owner_id: String,
    pub target_id: String,
    pub session_id: String,
    pub step_id: String,
    pub actor_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssignOutcome {
    Assigned {
        run_id: String,
    },
    /// The gate, the readiness guard, the target or the session refused.
    Refused {
        reason: String,
    },
    /// This executor does not dispatch runs; the session keeps the work as before.
    Unsupported,
}

/// Starts, lists and winds down orchestrator-owned sessions. The scheduler decides;
/// this acts. Implementations must not block for longer than one target request.
pub trait SessionExecutor: Send + Sync {
    /// Every AI session on the owner's targets that the executor knows, including
    /// ones the orchestrator does not own (`owner_id` says whose each is).
    fn list(
        &self,
        runtime: &OrchestrationRuntime,
        owner_id: &str,
        tx: &mut Transaction<'_>,
    ) -> Result<Vec<ScheduledSession>, SessionExecutorError>;
    fn start(
        &self,
        runtime: &OrchestrationRuntime,
        request: &PlacementStart,
        tx: &mut Transaction<'_>,
    ) -> StartOutcome;
    /// Ask a session to stop at its next safe point. Never interrupts work.
    fn wind_down(
        &self,
        runtime: &OrchestrationRuntime,
        request: &WindDownSession,
        tx: &mut Transaction<'_>,
    ) -> WindDownOutcome;
    /// HS2-YC2P7K: give a running session the run of the step it was refilled with.
    fn assign(
        &self,
        _runtime: &OrchestrationRuntime,
        _request: &PlacementAssign,
        _tx: &mut Transaction<'_>,
    ) -> AssignOutcome {
        AssignOutcome::Unsupported
    }
}

/// The default: no executor is wired in, so nothing is listed, started or stopped.
pub struct ExecutorUnavailable;
impl SessionExecutor for ExecutorUnavailable {
    fn list(
        &self,
        _: &OrchestrationRuntime,
        _: &str,
        _: &mut Transaction<'_>,
    ) -> Result<Vec<ScheduledSession>, SessionExecutorError> {
        Err(SessionExecutorError::Unavailable)
    }
    fn start(
        &self,
        _: &OrchestrationRuntime,
        _: &PlacementStart,
        _: &mut Transaction<'_>,
    ) -> StartOutcome {
        StartOutcome::Unavailable
    }
    fn wind_down(
        &self,
        _: &OrchestrationRuntime,
        _: &WindDownSession,
        _: &mut Transaction<'_>,
    ) -> WindDownOutcome {
        WindDownOutcome::Unavailable
    }
}

/// What a [`RecordingSessions`] was asked to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionCall {
    Start(PlacementStart),
    WindDown(WindDownSession),
}

/// A scripted executor for tests: it records every request and answers what it was
/// told to. A start it accepts appears in its list as a busy, owned session.
#[derive(Default)]
pub struct RecordingSessions {
    pub sessions: Mutex<Vec<ScheduledSession>>,
    pub calls: Mutex<Vec<SessionCall>>,
    /// Refuse every start with this code.
    pub refuse_starts: Mutex<Option<String>>,
    /// Fail the listing with this code.
    pub fail_list: Mutex<Option<String>>,
    /// What a wind-down answers; `Requested` when unset.
    pub wind_down_answer: Mutex<Option<WindDownOutcome>>,
    /// HS2-DG0YPA: answer every start by adopting this `(session, run)`, as when a
    /// live session already works the step's run.
    pub adopt_starts: Mutex<Option<(String, String)>>,
}

fn locked<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl RecordingSessions {
    pub fn calls(&self) -> Vec<SessionCall> {
        locked(&self.calls).clone()
    }
}

impl SessionExecutor for RecordingSessions {
    fn list(
        &self,
        _: &OrchestrationRuntime,
        _: &str,
        _: &mut Transaction<'_>,
    ) -> Result<Vec<ScheduledSession>, SessionExecutorError> {
        match locked(&self.fail_list).clone() {
            Some(code) => Err(SessionExecutorError::Failed(code)),
            None => Ok(locked(&self.sessions).clone()),
        }
    }
    fn start(
        &self,
        _: &OrchestrationRuntime,
        request: &PlacementStart,
        _: &mut Transaction<'_>,
    ) -> StartOutcome {
        locked(&self.calls).push(SessionCall::Start(request.clone()));
        if let Some(reason) = locked(&self.refuse_starts).clone() {
            return StartOutcome::Refused { reason };
        }
        if let Some((session_id, run_id)) = locked(&self.adopt_starts).clone() {
            return StartOutcome::Adopted { session_id, run_id };
        }
        let mut sessions = locked(&self.sessions);
        let session_id = format!("session-{}", sessions.len() + 1);
        sessions.push(ScheduledSession {
            session_id: session_id.clone(),
            target_id: request.target_id.clone(),
            tool: request.tool.clone(),
            account: request.account.clone(),
            model: request.model.clone(),
            run_id: None,
            owner_id: Some(request.owner_id.clone()),
            activity: SessionActivity::Busy,
            started_at_ms: 0,
            idle_since_ms: None,
            safe_point: false,
            human_takeover: false,
            winding_down: false,
        });
        StartOutcome::Started {
            session_id,
            run_id: None,
        }
    }
    fn wind_down(
        &self,
        _: &OrchestrationRuntime,
        request: &WindDownSession,
        _: &mut Transaction<'_>,
    ) -> WindDownOutcome {
        locked(&self.calls).push(SessionCall::WindDown(request.clone()));
        let answer = locked(&self.wind_down_answer)
            .clone()
            .unwrap_or(WindDownOutcome::Requested);
        if answer == WindDownOutcome::Requested {
            for session in locked(&self.sessions)
                .iter_mut()
                .filter(|session| session.session_id == request.session_id)
            {
                session.winding_down = true;
            }
        }
        answer
    }
}
