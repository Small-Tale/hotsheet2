//! HS2-95SWQ4: the scheduler's [`SessionExecutor`] over orchestrator-owned sessions
//! (HS2-GDJXGZ). The scheduler decides; this maps each decision onto the session
//! runtime and each of its answers onto a typed outcome.
//!
//! - **List.** This machine's unfinished session records, each brought in line with
//!   its host first, so a crashed session is `lost` and a session a person took over
//!   at the host is theirs before the scheduler looks. Hosts are asked as the policy's
//!   `placement.actor`; without one no host is asked and nothing reads as idle or safe.
//! - **Start.** The scheduler's operation id is stable for a piece of work, the session
//!   runtime's is one session's for good. While the session the id started is
//!   unfinished the same id is the same start; once it is finished (refused, wound
//!   down, lost) the next start for that work is a new session that replaces it.
//! - **Wind down.** A stop without force. The host checks the safe point itself, so
//!   nothing here can interrupt work, and a person's session is never stopped.
use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::{
    OrchestratedSession, SafePoint, ScheduledSession, SessionActivity, SessionError,
    SessionIdleStore, SessionOwner, SessionState,
};

use super::sessions::PlacedGate;
use super::*;

/// How long a start may stay unconfirmed before it is taken to have failed. Longer
/// than any one request to a host, so a start still on its way is never ended.
pub const UNCONFIRMED_START_MS: u64 = 5 * 60 * 1000;

/// Orchestrator-owned sessions as the scheduler's executor.
pub struct OwnedSessions;

impl SessionExecutor for OwnedSessions {
    fn list(
        &self,
        runtime: &OrchestrationRuntime,
        owner_id: &str,
        tx: &mut Transaction<'_>,
    ) -> Result<Vec<ScheduledSession>, SessionExecutorError> {
        runtime
            .scheduled_sessions(owner_id, tx)
            .map_err(|error| SessionExecutorError::Failed(error.code().into()))
    }

    fn start(
        &self,
        runtime: &OrchestrationRuntime,
        request: &PlacementStart,
        tx: &mut Transaction<'_>,
    ) -> StartOutcome {
        runtime.placed_start(request, tx)
    }

    fn wind_down(
        &self,
        runtime: &OrchestrationRuntime,
        request: &WindDownSession,
        tx: &mut Transaction<'_>,
    ) -> WindDownOutcome {
        runtime.placed_wind_down(request, tx)
    }
}

/// The scheduler acts for whoever asked, as the orchestrator: never as a person, so
/// a stop it makes passes the gate and can never be forced.
fn orchestrator(actor_id: &str) -> Option<MutationActor> {
    MutationActor::parse("system", Some(actor_id.to_owned()))
        .ok()
        .filter(|actor| actor.id.is_some())
}

/// Which start of the work `base` names the session operation `operation` was: 1 for
/// the first, `n` for `<base>.<n>`.
fn generation(base: &str, operation: &str) -> Option<u32> {
    if operation == base {
        return Some(1);
    }
    operation
        .strip_prefix(base)?
        .strip_prefix('.')?
        .parse::<u32>()
        .ok()
        .filter(|n| *n >= 2)
}

/// Whether the host found the session idle: between turns, holding no claim and with
/// no handoff pending. `None` when it could not say.
fn idle(safe_point: Option<&SafePoint>) -> Option<bool> {
    let facts = safe_point?.facts;
    match (facts.busy, facts.held_claims, facts.pending_handoffs) {
        (Some(true), _, _) => Some(false),
        (_, Some(held), _) if held > 0 => Some(false),
        (_, _, Some(pending)) if pending > 0 => Some(false),
        (Some(false), Some(0), Some(0)) => Some(true),
        _ => None,
    }
}

/// The last reason code in a session's history, or the state it ended in.
fn ended_as(session: &OrchestratedSession) -> String {
    session
        .history
        .last()
        .and_then(|entry| entry.reason.clone())
        .unwrap_or_else(|| format!("session_{}", session.state.code()))
}

impl OrchestrationRuntime {
    /// Who the scheduler's reads of a host act as: the actor the policy names for
    /// placement. A policy without one reads no host.
    fn placement_reader(&self, owner_id: &str) -> Option<MutationActor> {
        self.policy(owner_id)
            .ok()?
            .policy?
            .placement
            .actor
            .as_deref()
            .and_then(orchestrator)
    }

    /// The owner's running sessions as the scheduler sees them. A start its host never
    /// confirmed is not a session: it is left out, repeated by the next start for its
    /// work, and ended as `never_started` once it is too old to still be on its way.
    pub(super) fn scheduled_sessions(
        &self,
        owner_id: &str,
        tx: &mut Transaction<'_>,
    ) -> Result<Vec<ScheduledSession>, OrchestrationError> {
        let reader = self.placement_reader(owner_id);
        let now_ms = tx.clock.now_ms();
        let mut listed = Vec::new();
        let mut seen = Vec::new();
        for record in self.session_list(owner_id, true)? {
            // One request to the session's host. A host that cannot be asked, or a
            // grant that no longer covers the read, leaves the record as it is: the
            // session still counts, and nothing about it reads as idle or safe.
            let view = reader.as_ref().and_then(|actor| {
                self.session_safe_point(owner_id, &record.session_id, Some(actor), tx)
                    .ok()
            });
            let safe_point = view.as_ref().and_then(|view| view.safe_point.clone());
            let session = view.map_or(record, |view| view.session);
            match session.state {
                SessionState::Running => {}
                SessionState::Requested => {
                    if let (Some(actor), Some(_)) = (&reader, &safe_point) {
                        if now_ms.saturating_sub(session.created_at_ms) >= UNCONFIRMED_START_MS {
                            // Its host answered and has not started it. Nothing runs,
                            // so the stop interrupts nothing; the host decides again.
                            let _ = self.session_stop(
                                owner_id,
                                &session.session_id,
                                StopSession { force: false },
                                Some(actor),
                                tx,
                            );
                        }
                    }
                    continue;
                }
                // The sync just learned it is gone.
                _ => continue,
            }
            let idle = idle(safe_point.as_ref());
            seen.push((session.session_id.clone(), idle));
            listed.push(ScheduledSession {
                started_at_ms: session
                    .history
                    .iter()
                    .find(|entry| entry.to == SessionState::Running)
                    .map_or(session.created_at_ms, |entry| entry.at_ms),
                session_id: session.session_id,
                target_id: session.target_id,
                tool: session.tool,
                account: session.account,
                model: session.model,
                run_id: session.run_id,
                owner_id: Some(owner_id.to_owned()),
                // Unknown is busy: a session is never wound down on a guess.
                activity: if idle == Some(true) {
                    SessionActivity::Idle
                } else {
                    SessionActivity::Busy
                },
                idle_since_ms: None,
                safe_point: safe_point.is_some_and(|safe_point| safe_point.safe),
                human_takeover: session.owner == SessionOwner::Human,
                // A stop is immediate at a safe point and refused away from one, so no
                // session is ever between the two.
                winding_down: false,
            });
        }
        let idle_since = SessionIdleStore::new(&self.home).observe(owner_id, &seen, now_ms)?;
        for session in &mut listed {
            if session.activity == SessionActivity::Idle {
                session.idle_since_ms = idle_since.get(&session.session_id).copied();
            }
        }
        Ok(listed)
    }

    pub(super) fn placed_start(
        &self,
        request: &PlacementStart,
        tx: &mut Transaction<'_>,
    ) -> StartOutcome {
        let Some(actor) = orchestrator(&request.actor_id) else {
            return StartOutcome::Unavailable;
        };
        let owner_id = request.owner_id.as_str();
        let refused = |error: &OrchestrationError| StartOutcome::Refused {
            reason: error.code().into(),
        };
        let records = match self.session_list(owner_id, false) {
            Ok(records) => records,
            Err(error) => return refused(&error),
        };
        let latest = records
            .iter()
            .filter(|session| session.target_id == request.target_id)
            .filter_map(|session| {
                generation(&request.operation_id, &session.operation_id).map(|n| (n, session))
            })
            .max_by_key(|(n, _)| *n);
        let (operation_id, replaces) = match latest {
            None => (request.operation_id.clone(), None),
            // Still on its way or running: the same start, asked again.
            Some((_, session)) if !session.state.is_terminal() => {
                (session.operation_id.clone(), session.replaces.clone())
            }
            // Finished: the work gets a new session that replaces it.
            Some((n, session)) => (
                format!("{}.{}", request.operation_id, n + 1),
                Some(session.session_id.clone()),
            ),
        };
        let outcome = self.session_start_gated(
            owner_id,
            StartSession {
                target_id: request.target_id.clone(),
                operation_id: operation_id.clone(),
                tool: request.tool.clone(),
                model: request.model.clone(),
                effort: None,
                account: request.account.clone(),
                role: None,
                run_id: None,
                replaces,
            },
            Some(&PlacedGate {
                operation_id: &request.operation_id,
                load: Some(&request.load),
            }),
            Some(&actor),
            tx,
        );
        match outcome {
            Ok(view) if view.session.state == SessionState::Running => StartOutcome::Started {
                session_id: view.session.session_id,
            },
            // The host launched it for an earlier request and it has ended since.
            Ok(view) => StartOutcome::Refused {
                reason: ended_as(&view.session),
            },
            Err(error) => {
                // A refused start is recorded as failed with the host's own reason
                // (`below_ram_floor`, `session_cap_reached`), which says more than
                // the error it was reported as.
                let recorded = self
                    .session_list(owner_id, false)
                    .ok()
                    .and_then(|records| {
                        records
                            .into_iter()
                            .find(|session| session.operation_id == operation_id)
                    })
                    .filter(|session| session.state == SessionState::Failed)
                    .map(|session| ended_as(&session));
                match recorded {
                    Some(reason) => StartOutcome::Refused { reason },
                    None => refused(&error),
                }
            }
        }
    }

    pub(super) fn placed_wind_down(
        &self,
        request: &WindDownSession,
        tx: &mut Transaction<'_>,
    ) -> WindDownOutcome {
        let Some(actor) = orchestrator(&request.actor_id) else {
            return WindDownOutcome::Unavailable;
        };
        let refused = |reason: &str| WindDownOutcome::Refused {
            reason: reason.into(),
        };
        let session = match self.session(&request.owner_id, &request.session_id) {
            Ok(session) => session,
            Err(OrchestrationError::Session(SessionError::NotFound)) => {
                return WindDownOutcome::NotFound;
            }
            Err(error) => return refused(error.code()),
        };
        if session.target_id != request.target_id || session.state.is_terminal() {
            return WindDownOutcome::NotFound;
        }
        if session.owner == SessionOwner::Human {
            return refused(SessionError::HumanOwned.code());
        }
        match self.session_stop_gated(
            &request.owner_id,
            &request.session_id,
            StopSession { force: false },
            Some(&PlacedGate {
                operation_id: &request.operation_id,
                load: None,
            }),
            Some(&actor),
            tx,
        ) {
            Ok(view) if view.session.state == SessionState::Stopped => WindDownOutcome::Requested,
            // It was already gone when the stop arrived.
            Ok(_) => WindDownOutcome::NotFound,
            Err(OrchestrationError::Session(SessionError::NotSafe)) => {
                WindDownOutcome::NotAtSafePoint
            }
            Err(OrchestrationError::Session(SessionError::NotFound | SessionError::NotActive)) => {
                WindDownOutcome::NotFound
            }
            Err(error) => refused(error.code()),
        }
    }
}
