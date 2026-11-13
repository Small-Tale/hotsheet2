//! HS2-7QHWHJ: following target ticket changes without polling.
//!
//! The hosted half answers "which tickets of this checkout changed after cursor N",
//! waiting a bounded time when none did. The orchestrating half holds one such
//! request open per target that has unfinished runs and syncs exactly the runs whose
//! tickets changed. No target is contacted while it has no active run.
use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::checkouts::Checkout;
use hotsheet_ticketing::orchestration::{Run, TargetAction, TargetOpError};
use serde::{Deserialize, Serialize};

use super::*;

/// The longest one hosted events request waits. Kept under the remote transport's
/// read timeout so a quiet target is a normal empty answer, not a failure.
pub const MAX_TARGET_EVENT_WAIT_MS: u64 = 8_000;
/// A watch re-confirms its authority this often even when nothing fails.
const REAUTHORIZE_MS: u64 = 10 * 60 * 1000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetTicketEvent {
    pub cursor: u64,
    pub connection_id: String,
    pub native_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetEventPage {
    /// Pass as `since` to continue.
    pub cursor: u64,
    pub events: Vec<TargetTicketEvent>,
    /// The host no longer retains everything after `since`; treat every ticket of the
    /// checkout as possibly changed.
    pub overflow: bool,
}

/// The host's live record of ticket changes. A server supplies it from its change
/// stream; a process without one (the bare CLI) has none and says so.
pub trait TargetEvents: Send + Sync {
    /// Ticket changes of one hosted checkout after `since`. `None` is a handshake:
    /// the current cursor and no backlog. With nothing to report, waits up to
    /// `wait_ms` for the next change.
    fn poll(
        &self,
        checkout: &Checkout,
        since: Option<u64>,
        wait_ms: u64,
    ) -> Result<TargetEventPage, OrchestrationError>;
}

/// What a watch remembers between steps.
#[derive(Default)]
pub struct WatchState {
    cursor: Option<u64>,
    authorized: Option<(Authorized, u64)>,
}

impl WatchState {
    pub fn cursor(&self) -> Option<u64> {
        self.cursor
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct WatchStep {
    /// False when the target has no unfinished run left: the watch should end, and it
    /// contacted nothing.
    pub active: bool,
    /// Runs brought in line with their target ticket by this step.
    pub synced: Vec<String>,
    /// The target could not be followed this step; the caller backs off, and the
    /// cursor is kept so nothing is missed on reconnect.
    pub failed: bool,
}

/// The actor a run's supervision acts as: whoever dispatched it.
fn supervisor(run: &Run) -> Option<MutationActor> {
    let id = run.history.first()?.actor_id.clone()?;
    MutationActor::parse("system", Some(id)).ok()
}

impl OrchestrationRuntime {
    /// HS2-7QHWHJ hosted half.
    pub fn hosted_events(
        &self,
        checkout_id: &str,
        since: Option<u64>,
        wait_ms: u64,
    ) -> Result<TargetEventPage, OrchestrationError> {
        let checkout = self.owner(checkout_id)?;
        let Some(events) = &self.target_events else {
            return Err(OrchestrationError::TargetOperation(
                TargetOpError::PeerUnsupported,
            ));
        };
        events.poll(&checkout, since, wait_ms.min(MAX_TARGET_EVENT_WAIT_MS))
    }

    /// One step of following a target: hold one bounded request open for its ticket
    /// changes and sync the runs they concern. Call again while `active`.
    pub fn watch_step(
        &self,
        owner_id: &str,
        target_id: &str,
        state: &mut WatchState,
        wait_ms: u64,
        tx: &mut Transaction<'_>,
    ) -> WatchStep {
        let runs: Vec<Run> = self
            .run_list(owner_id, false)
            .unwrap_or_default()
            .into_iter()
            // A run a person took over is still followed: it may be handed back or finish.
            .filter(|run| run.target.target_id == target_id && !run.state.is_terminal())
            .collect();
        // Idle: no run, no request.
        let Some(lead) = runs.iter().find_map(supervisor) else {
            *state = WatchState::default();
            return WatchStep::default();
        };
        let mut step = WatchStep {
            active: true,
            ..WatchStep::default()
        };
        let sync = |runs: &[&Run], step: &mut WatchStep, tx: &mut Transaction<'_>| {
            for run in runs {
                // A failed sync records the outage on the run itself.
                if self
                    .run_sync(owner_id, &run.run_id, supervisor(run).as_ref(), tx)
                    .is_ok()
                {
                    step.synced.push(run.run_id.clone());
                }
            }
        };
        let all: Vec<&Run> = runs.iter().collect();
        let now_ms = tx.clock.now_ms();
        let stale = state
            .authorized
            .as_ref()
            .is_none_or(|(_, at)| now_ms.saturating_sub(*at) >= REAUTHORIZE_MS);
        if stale {
            let Ok(target) = Self::target_ref(owner_id, target_id) else {
                return WatchStep::default();
            };
            match self.authorize(
                &target,
                Some(&lead),
                ActionRequest::new(TargetAction::TicketsRead),
                tx,
            ) {
                Ok(authorized) => state.authorized = Some((authorized, now_ms)),
                Err(_) => {
                    state.authorized = None;
                    sync(&all, &mut step, tx);
                    step.failed = true;
                    return step;
                }
            }
        }
        let Some((authorized, _)) = &state.authorized else {
            step.failed = true;
            return step;
        };
        let query = match state.cursor {
            Some(cursor) => format!(
                "?since={cursor}&wait_ms={}",
                wait_ms.min(MAX_TARGET_EVENT_WAIT_MS)
            ),
            None => String::new(),
        };
        let page = Self::destination(authorized)
            .and_then(|destination| {
                self.hosted_call_at(destination, "/target-events", "GET", &query, None, tx)
            })
            .and_then(|reply| {
                serde_json::from_value::<TargetEventPage>(reply)
                    .map_err(|_| OrchestrationError::Observation(TargetFailure::InvalidResponse))
            });
        let page = match page {
            Ok(page) => page,
            Err(_) => {
                // Keep the cursor: the next successful request replays what was missed.
                state.authorized = None;
                sync(&all, &mut step, tx);
                step.failed = true;
                return step;
            }
        };
        let first = state.cursor.is_none();
        // A cursor that goes backwards means the host restarted its stream.
        let reset = state.cursor.is_some_and(|cursor| page.cursor < cursor);
        state.cursor = Some(page.cursor);
        if first || reset || page.overflow {
            // Nothing is known about what happened before: catch every run up.
            sync(&all, &mut step, tx);
        } else {
            let changed: Vec<&Run> = runs
                .iter()
                .filter(|run| {
                    page.events.iter().any(|event| {
                        event.connection_id == run.target.connection_id
                            && event.native_id == run.target.native_id
                    })
                })
                .collect();
            sync(&changed, &mut step, tx);
        }
        step
    }
}
