//! HS2-7QHWHJ: following target ticket changes without polling.
//!
//! The hosted half answers "which tickets of this checkout changed after cursor N",
//! waiting a bounded time when none did. The orchestrating half holds one such
//! request open per target that has unfinished runs and syncs exactly the runs whose
//! tickets changed. No target is contacted while it has no active run.
//!
//! HS2-257Q2Z: the same feed carries the hosting server's changes to the sessions an
//! orchestrator started there (an exit, a finished turn), and a target is followed
//! while the owner has an unfinished session on it as well. A change the owner's
//! scheduler cannot learn from a run event wakes its loop from here.
//!
//! HS2-TMA4YA: a target is also followed while an unfinished plan step names one of
//! its tickets (a prerequisite, or the step's own ticket), whether or not a run
//! stands on it. A change of such a ticket reads it again through the gate and
//! publishes the readiness level it changes.
use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::checkouts::Checkout;
use hotsheet_ticketing::orchestration::{
    ExternalSource, OrchestratedSession, Run, TargetAction, TargetOpError,
};
use serde::{Deserialize, Serialize};

use super::*;

/// The longest one hosted events request waits. Kept under the remote transport's
/// read timeout so a quiet target is a normal empty answer, not a failure.
pub const MAX_TARGET_EVENT_WAIT_MS: u64 = 8_000;
/// A watch re-confirms its authority this often even when nothing fails.
const REAUTHORIZE_MS: u64 = 10 * 60 * 1000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
// HS2-T2KS8G: an answer read from a peer accepts fields a newer peer adds.
pub struct TargetTicketEvent {
    pub cursor: u64,
    pub connection_id: String,
    pub native_id: String,
}

/// HS2-257Q2Z: a session an orchestrator started on the host ended or finished a
/// turn. `session_id` is the host's id for it (its terminal in that server's drawer).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
// HS2-T2KS8G: an answer read from a peer accepts fields a newer peer adds.
pub struct TargetSessionEvent {
    pub cursor: u64,
    pub session_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
// HS2-T2KS8G: an answer read from a peer accepts fields a newer peer adds.
pub struct TargetEventPage {
    /// Pass as `since` to continue.
    pub cursor: u64,
    pub events: Vec<TargetTicketEvent>,
    /// HS2-257Q2Z: the host's sessions that ended or finished a turn after `since`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sessions: Vec<TargetSessionEvent>,
    /// The host no longer retains everything after `since`; treat every ticket of the
    /// checkout as possibly changed.
    pub overflow: bool,
    /// HS2-FJ6MDW: the hosted project's own speculation setting as of this answer, so a
    /// follower learns a change of it without a capacity read. Absent from an older
    /// host, which says nothing about it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speculation: Option<HostedSpeculation>,
}

/// HS2-FJ6MDW: a hosted project's own speculation setting, alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
// HS2-T2KS8G: an answer read from a peer accepts fields a newer peer adds.
pub struct HostedSpeculation {
    /// `None`: the project does not opt in.
    pub speculation: Option<hotsheet_ticketing::orchestration::TargetSpeculation>,
}

impl HostedSpeculation {
    /// What the rule reads: a project that does not opt in is not allowed.
    pub fn setting(self) -> hotsheet_ticketing::orchestration::TargetSpeculation {
        self.speculation
            .unwrap_or(hotsheet_ticketing::orchestration::TargetSpeculation::NOT_OPTED_IN)
    }
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
    /// HS2-WBREDM: the last step failed, so the next one that succeeds is a reconnect.
    reconnecting: bool,
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
    /// HS2-257Q2Z: the owner's scheduler was woken, because a session of the owner on
    /// this target changed in a way no run event reports.
    pub woke_scheduler: bool,
    /// HS2-TMA4YA: plan tickets of this target read again because they changed (or the
    /// follower could not tell what it missed), so their readiness was published.
    pub refreshed: usize,
    /// HS2-FJ6MDW: the target's own speculation setting changed and was stored.
    pub speculation_changed: bool,
}

/// The actor a run's supervision acts as: whoever dispatched it.
pub(super) fn supervisor(run: &Run) -> Option<MutationActor> {
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
        let mut page = events.poll(&checkout, since, wait_ms.min(MAX_TARGET_EVENT_WAIT_MS))?;
        // HS2-FJ6MDW: read after the wait, so the answer carries the setting as it is
        // when the follower receives it.
        page.speculation = Some(HostedSpeculation {
            speculation: super::capacity::checkout_speculation(&checkout.root),
        });
        Ok(page)
    }

    /// HS2-FJ6MDW hosted half: the hosted project's own speculation setting, alone. It
    /// samples nothing; ordinary authenticated project data like the capacity read.
    pub fn hosted_speculation(
        &self,
        checkout_id: &str,
    ) -> Result<HostedSpeculation, OrchestrationError> {
        let checkout = self.owner(checkout_id)?;
        Ok(HostedSpeculation {
            speculation: super::capacity::checkout_speculation(&checkout.root),
        })
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
        // HS2-257Q2Z: the owner's unfinished sessions on this target, by the host's id.
        let sessions: Vec<OrchestratedSession> = self
            .session_list(owner_id, true)
            .unwrap_or_default()
            .into_iter()
            .filter(|session| session.target_id == target_id)
            .collect();
        // HS2-TMA4YA: the tickets of this target an unfinished plan step names.
        let (tickets, planner) = self.plan_tickets(owner_id, target_id);
        let planner = planner
            .filter(|_| !tickets.is_empty())
            .and_then(|id| MutationActor::parse("system", Some(id)).ok());
        // Idle: no run, no session and no planned ticket, no request.
        let Some(lead) = runs
            .iter()
            .find_map(supervisor)
            .or_else(|| sessions.iter().find_map(session_supervisor))
            .or(planner)
        else {
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
        // HS2-WBREDM: whenever the follower (re)connects, or cannot tell what it missed,
        // every run is reconciled rather than only read: nothing is resumed on a
        // record that went stale while the target was out of sight.
        let reconcile = |runs: &[&Run], step: &mut WatchStep, tx: &mut Transaction<'_>| {
            for run in runs {
                let confirmed = self
                    .run_reconcile(owner_id, &run.run_id, supervisor(run).as_ref(), tx)
                    .is_ok_and(|view| {
                        !view
                            .unverified
                            .contains(&ExternalSource::TargetReachability.code())
                    });
                if confirmed {
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
                    state.reconnecting = true;
                    reconcile(&all, &mut step, tx);
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
                state.reconnecting = true;
                reconcile(&all, &mut step, tx);
                step.failed = true;
                return step;
            }
        };
        let first = state.cursor.is_none();
        // A cursor that goes backwards means the host restarted its stream.
        let reset = state.cursor.is_some_and(|cursor| page.cursor < cursor);
        state.cursor = Some(page.cursor);
        let reconnected = std::mem::take(&mut state.reconnecting);
        let caught_up = first || reset || page.overflow || reconnected;
        // HS2-FJ6MDW: a changed setting of the target's own project changes what the
        // provisional rule answers. It is stored and wakes the owner's scheduler, and
        // nothing is read or stored while the owner's policy does not opt in.
        if let Some(reported) = page.speculation {
            if self.speculating(owner_id).is_some()
                && self
                    .provisional_targets_seen(
                        owner_id,
                        &[(target_id.to_owned(), Some(reported.setting()))],
                        now_ms,
                    )
                    .unwrap_or(false)
            {
                self.readiness_reevaluate(owner_id, now_ms);
                self.scheduler_wake(owner_id);
                step.woke_scheduler = true;
                step.speculation_changed = true;
            }
        }
        if wakes_scheduler(&page, &sessions, &runs, caught_up) {
            self.scheduler_wake(owner_id);
            step.woke_scheduler = true;
        }
        // HS2-TMA4YA: planned tickets that changed, or all of them when the follower
        // cannot tell what it missed, are read again and their levels published.
        let changed: Vec<_> = tickets
            .iter()
            .filter(|ticket| {
                caught_up
                    || page.events.iter().any(|event| {
                        event.connection_id == ticket.connection_id
                            && event.native_id == ticket.native_id
                    })
            })
            .cloned()
            .collect();
        if !changed.is_empty() {
            let count = changed.len();
            if self
                .refresh_tickets(owner_id, changed, Some(&lead), false, tx)
                .is_ok()
            {
                self.readiness_reevaluate(owner_id, tx.clock.now_ms());
                step.refreshed = count;
            }
        }
        if caught_up {
            // Nothing is known about what happened before: catch every run up.
            reconcile(&all, &mut step, tx);
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

/// HS2-257Q2Z: the actor who started a session follows it, as a run's dispatcher does.
fn session_supervisor(session: &OrchestratedSession) -> Option<MutationActor> {
    let id = session.history.first()?.actor_id.clone()?;
    MutationActor::parse("system", Some(id)).ok()
}

/// HS2-257Q2Z: whether a page tells the owner's scheduler something no run event
/// does. A session of the owner ended or finished a turn on its host; or a ticket of
/// the target changed (a claim taken or let go is what makes a session busy or idle)
/// while a session of the owner works no unfinished run, so no run sync reports it;
/// or the follower cannot tell what it missed while it has any session there.
fn wakes_scheduler(
    page: &TargetEventPage,
    sessions: &[OrchestratedSession],
    runs: &[Run],
    caught_up: bool,
) -> bool {
    if sessions.is_empty() {
        return false;
    }
    if caught_up {
        return true;
    }
    let mine = |hosted_id: &str| {
        sessions.iter().any(|session| {
            session
                .hosted
                .as_ref()
                .is_some_and(|hosted| hosted.session_id == hosted_id)
        })
    };
    if page.sessions.iter().any(|event| mine(&event.session_id)) {
        return true;
    }
    let free = sessions.iter().any(|session| {
        session
            .run_id
            .as_deref()
            .is_none_or(|run_id| runs.iter().all(|run| run.run_id != run_id))
    });
    free && !page.events.is_empty()
}
