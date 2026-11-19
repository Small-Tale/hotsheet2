//! HS2-95SWQ4: the scheduler's [`SessionExecutor`] over orchestrator-owned sessions
//! (HS2-GDJXGZ). The scheduler decides; this maps each decision onto the session
//! runtime and each of its answers onto a typed outcome.
//!
//! - **List.** This machine's unfinished session records, each brought in line with
//!   its host first, so a crashed session is `lost` and a session a person took over
//!   at the host is theirs before the scheduler looks. Hosts are asked as the policy's
//!   `placement.actor`, else as the orchestration project's orchestrator (HS2-XYAHB4);
//!   without a policy no host is asked and nothing reads as idle or safe.
//! - **Start.** The scheduler's operation id is stable for a piece of work, the session
//!   runtime's is one session's for good. While the session the id started is
//!   unfinished the same id is the same start; once it is finished (refused, wound
//!   down, lost) the next start for that work is a new session that replaces it. A
//!   start never confirmed whose tool, model or account the scheduler has since
//!   changed is ended through its host first (HS2-7VV79Q).
//! - **Dispatch (HS2-YC2P7K).** A start for a plan step first dispatches the step's run
//!   in dispatcher mode under `sched-run-<step id>`, through `run_dispatch` with its
//!   readiness guard, and starts the session linked to that run. The step is then in
//!   progress. A refill gives a free session the run of its new step the same way.
//! - **One session per run (HS2-DG0YPA).** A start for a run another live session
//!   already works adopts that session instead of launching a second one; a person's
//!   or an unconfirmed holder refuses the start as `session_run_held`.
//! - **Wind down.** A stop without force. The host checks the safe point itself, so
//!   nothing here can interrupt work, and a person's session is never stopped.
use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::{
    DispatchMode, OrchestratedSession, Run, RunState, SafePoint, ScheduledSession, SessionActivity,
    SessionError, SessionIdleStore, SessionOwner, SessionRole, SessionState,
};

use super::sessions::PlacedGate;
use super::*;

/// How long a start may stay unconfirmed before it is taken to have failed. Longer
/// than any one request to a host, so a start still on its way is never ended.
pub const UNCONFIRMED_START_MS: u64 = 5 * 60 * 1000;

/// HS2-3REXQ7: an assignment refused because the session already holds that run.
pub const SESSION_HOLDS_RUN: &str = "session_holds_run";

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

    fn assign(
        &self,
        runtime: &OrchestrationRuntime,
        request: &PlacementAssign,
        tx: &mut Transaction<'_>,
    ) -> AssignOutcome {
        runtime.placed_assign(request, tx)
    }
}

/// HS2-YC2P7K: the scheduler's operation id for the run of a plan step. Stable for
/// the step, so a repeated dispatch is the same run.
pub fn run_operation(step_id: &str) -> String {
    format!("sched-run-{step_id}")
}

/// HS2-YC2P7K: whether `run` is one the scheduler dispatched for a step, under the
/// step's operation id or a later generation of it.
pub(super) fn scheduled_run_of(run: &Run, step_id: &str) -> bool {
    generation(&run_operation(step_id), &run.operation_id).is_some()
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
    /// placement, else the orchestration project's own orchestrator (HS2-XYAHB4).
    /// Without a policy no host is read.
    fn placement_reader(&self, owner_id: &str) -> Option<MutationActor> {
        orchestrator(
            &self
                .policy(owner_id)
                .ok()?
                .policy?
                .placement
                .acting_actor(owner_id),
        )
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
        let finished_runs: std::collections::BTreeSet<String> = self
            .run_list(owner_id, false)?
            .into_iter()
            .filter(|run| run.state.is_terminal())
            .map(|run| run.run_id)
            .collect();
        let cancelled = self.withdrawn_runs(owner_id);
        let mut listed = Vec::new();
        let mut seen = Vec::new();
        let mut reported_idle: std::collections::BTreeMap<String, u64> =
            std::collections::BTreeMap::new();
        for record in self.session_list(owner_id, true)? {
            // One request to the session's host. A host that cannot be asked, or a
            // gate that no longer allows the read, leaves the record as it is: the
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
            if let Some(since) = safe_point
                .as_ref()
                .and_then(|safe_point| safe_point.facts.idle_since_ms)
            {
                reported_idle.insert(session.session_id.clone(), since);
            }
            // HS2-YC2P7K: a session works its run while the run is unfinished, idle
            // between turns or not; one whose run finished is free again.
            // HS2-HV9ED3: a session whose run a person cancelled stays busy with it
            // until it is wound down, so it is never refilled meanwhile. So does one
            // whose run's step was removed (HS2-YFPZFP).
            let run_id = session
                .run_id
                .clone()
                .filter(|run_id| !finished_runs.contains(run_id) || cancelled.contains(run_id));
            let working = run_id.is_some() || idle != Some(true);
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
                run_id,
                owner_id: Some(owner_id.to_owned()),
                // Unknown is busy: a session is never wound down on a guess.
                activity: if working {
                    SessionActivity::Busy
                } else {
                    SessionActivity::Idle
                },
                idle_since_ms: None,
                safe_point: safe_point.is_some_and(|safe_point| safe_point.safe),
                human_takeover: session.owner == SessionOwner::Human,
                // A stop is immediate at a safe point and refused away from one, so no
                // session is ever between the two.
                winding_down: false,
                // HS2-TK9ADC: a dispatcher, integrator or planner is never given work.
                role: (session.role != SessionRole::Worker).then_some(session.role),
            });
        }
        // HS2-RG91SK: the host says since when a session has been idle (its last
        // finished turn), so work that began and ended between two passes is not missed.
        // A host that does not say (an older one) falls back to the first listing here
        // that saw the session idle.
        let idle_since = SessionIdleStore::new(&self.home).observe(owner_id, &seen, now_ms)?;
        for session in &mut listed {
            if session.activity == SessionActivity::Idle {
                session.idle_since_ms = reported_idle
                    .get(&session.session_id)
                    .copied()
                    .or_else(|| idle_since.get(&session.session_id).copied());
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
        // HS2-XHQ1EH: a permission request is a wait for a person, not a refusal.
        let refused = |error: &OrchestrationError| match error {
            OrchestrationError::PermissionRequired { request_id } => {
                StartOutcome::PermissionRequired {
                    request_id: request_id.clone(),
                }
            }
            _ => StartOutcome::Refused {
                reason: error.code().into(),
            },
        };
        // HS2-9FKTB3: an operation id names one start whichever site of the project it
        // went to, so the latest one is looked for on every target.
        let latest_of = |records: Vec<OrchestratedSession>| {
            records
                .into_iter()
                .filter_map(|session| {
                    generation(&request.operation_id, &session.operation_id).map(|n| (n, session))
                })
                .max_by_key(|(n, _)| *n)
        };
        let mut latest = match self.session_list(owner_id, false) {
            Ok(records) => latest_of(records),
            Err(error) => return refused(&error),
        };
        // HS2-7VV79Q: a start its host never confirmed, for a choice the scheduler has
        // since changed (another tool, model or account), cannot be asked again under
        // its operation id. It is ended at once through its host rather than refused
        // until it is old: the host releases a reservation it never launched, or has
        // no record of it, and either way it ends as `never_started`. A host that
        // cannot be asked concludes nothing, and the start is refused as before.
        if let Some((_, session)) = &latest
            && session.state == SessionState::Requested
            && (session.tool != request.tool
                || session.target_id != request.target_id
                || session.model != request.model
                || session.account != request.account)
        {
            match self.session_stop(
                owner_id,
                &session.session_id,
                StopSession { force: false },
                Some(&actor),
                tx,
            ) {
                Ok(view) if view.session.state.is_terminal() => {}
                Ok(_) => return refused(&OrchestrationError::Conflict),
                Err(error) => return refused(&error),
            }
            latest = match self.session_list(owner_id, false) {
                Ok(records) => latest_of(records),
                Err(error) => return refused(&error),
            };
        }
        let latest = latest.as_ref().map(|(n, session)| (*n, session));
        let (operation_id, replaces, earlier_run) = match latest {
            None => (request.operation_id.clone(), None, None),
            // Still on its way or running: the same start, asked again.
            Some((_, session)) if !session.state.is_terminal() => (
                session.operation_id.clone(),
                session.replaces.clone(),
                session.run_id.clone(),
            ),
            // Finished: the work gets a new session that replaces it.
            Some((n, session)) => (
                format!("{}.{}", request.operation_id, n + 1),
                Some(session.session_id.clone()),
                None,
            ),
        };
        // HS2-YC2P7K: the step's run first, so the session starts linked to it and the
        // step is in progress. A start being repeated keeps what it named, and is
        // linked to the run once it runs.
        let repeated = latest.is_some_and(|(_, session)| !session.state.is_terminal());
        let run_id = match (earlier_run, request.steps.first()) {
            (Some(run_id), _) => Some(run_id),
            (None, Some(step_id)) => {
                match self.scheduler_run(owner_id, step_id, &request.target_id, &actor, tx) {
                    Ok(run) => Some(run.run_id),
                    Err(error) => return refused(&error),
                }
            }
            (None, None) => None,
        };
        // HS2-DG0YPA: a live session that already works the run is adopted, never
        // doubled. A crashed one was just learned as lost and holds nothing.
        if !repeated && let Some(run_id) = &run_id {
            self.sync_run_holders(owner_id, run_id, &operation_id, Some(&actor), tx);
            match self.run_session_holder(owner_id, run_id) {
                Ok(None) => {}
                Ok(Some(holder))
                    if holder.state == SessionState::Running
                        && holder.owner == SessionOwner::Orchestrator
                        && holder.target_id == request.target_id =>
                {
                    return StartOutcome::Adopted {
                        session_id: holder.session_id,
                        run_id: run_id.clone(),
                    };
                }
                // A person's session, or a start its host has not confirmed yet.
                Ok(Some(_)) => {
                    return StartOutcome::Refused {
                        reason: SessionError::RunHeld.code().into(),
                    };
                }
                Err(error) => return refused(&error),
            }
        }
        let (start_run, link_run) = if repeated {
            (
                latest.and_then(|(_, session)| session.run_id.clone()),
                run_id.clone(),
            )
        } else {
            (run_id.clone(), None)
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
                role: request.role,
                run_id: start_run,
                replaces,
                // HS2-7YSKXE: the session acquires what its work needs itself.
                resources: request.resources.clone(),
            },
            Some(&PlacedGate {
                operation_id: &request.operation_id,
                load: Some(&request.load),
            }),
            Some(&actor),
            tx,
        );
        match outcome {
            Ok(view) if view.session.state == SessionState::Running => {
                if let Some(run_id) =
                    link_run.filter(|run| view.session.run_id.as_ref() != Some(run))
                    && let Err(error) =
                        self.session_link(owner_id, &view.session.session_id, &run_id, &actor, tx)
                {
                    return refused(&error);
                }
                StartOutcome::Started {
                    session_id: view.session.session_id,
                    run_id,
                }
            }
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

    /// HS2-YC2P7K: the run the scheduler dispatched for a step, or a new one. A
    /// repeated request is the same run; once that run failed or was cancelled the
    /// next is `<id>.<n>`. Dispatched in dispatcher mode as `actor`, so the target's
    /// dispatcher assigns the exact ticket, through the gate (`work_start`) and the
    /// readiness guard. HS2-9FKTB3: on `site`, the site of the step's project the
    /// scheduler chose; a live run of the step on another site is refused.
    pub(super) fn scheduler_run(
        &self,
        owner_id: &str,
        step_id: &str,
        site: &str,
        actor: &MutationActor,
        tx: &mut Transaction<'_>,
    ) -> Result<Run, OrchestrationError> {
        let step = self.readiness(owner_id, step_id, tx)?.readiness.step;
        let base = run_operation(step_id);
        let latest = self
            .run_list(owner_id, false)?
            .into_iter()
            .filter_map(|run| generation(&base, &run.operation_id).map(|n| (n, run)))
            .max_by_key(|(n, _)| *n);
        let operation_id = match latest {
            None => base,
            Some((_, run)) if !run.state.is_terminal() => {
                return if run.target.target_id == site {
                    Ok(run)
                } else {
                    Err(OrchestrationError::Site(
                        hotsheet_ticketing::orchestration::SiteError::RunConflict,
                    ))
                };
            }
            Some((_, run)) if run.state == RunState::Completed => {
                return Err(OrchestrationError::Run(
                    hotsheet_ticketing::orchestration::RunError::NotActive,
                ));
            }
            Some((n, _)) => format!("{base}.{}", n + 1),
        };
        self.run_dispatch(
            owner_id,
            DispatchRun {
                ticket: OrchestrationTicket {
                    connection_id: step.ticket.connection_id.clone(),
                    native_id: step.ticket.native_id.clone(),
                },
                target: TargetTicket {
                    target_id: site.to_owned(),
                    connection_id: step.target.connection_id.clone(),
                    native_id: step.target.native_id.clone(),
                },
                operation_id,
                mode: Some(DispatchMode::Dispatcher),
                worker_id: None,
                worker_label: None,
                lease_minutes: None,
                override_readiness: false,
            },
            Some(actor),
            tx,
        )
        .map(|view| view.run)
    }

    /// HS2-YC2P7K: a running session the scheduler refilled gets its step's run.
    pub(super) fn placed_assign(
        &self,
        request: &PlacementAssign,
        tx: &mut Transaction<'_>,
    ) -> AssignOutcome {
        let refused = |error: &OrchestrationError| AssignOutcome::Refused {
            reason: error.code().into(),
        };
        let Some(actor) = orchestrator(&request.actor_id) else {
            return AssignOutcome::Refused {
                reason: "actor_required".into(),
            };
        };
        let session = match self.session(&request.owner_id, &request.session_id) {
            Ok(session) => session,
            Err(error) => return refused(&error),
        };
        if session.target_id != request.target_id
            || session.state != SessionState::Running
            || session.owner == SessionOwner::Human
        {
            return AssignOutcome::Refused {
                reason: SessionError::NotActive.code().into(),
            };
        }
        let run = match self.scheduler_run(
            &request.owner_id,
            &request.step_id,
            &request.target_id,
            &actor,
            tx,
        ) {
            Ok(run) => run,
            Err(error) => return refused(&error),
        };
        // HS2-3REXQ7: a session is never refilled with the run it already holds.
        if session.run_id.as_deref() == Some(run.run_id.as_str()) {
            return AssignOutcome::Refused {
                reason: SESSION_HOLDS_RUN.into(),
            };
        }
        match self.session_link(
            &request.owner_id,
            &session.session_id,
            &run.run_id,
            &actor,
            tx,
        ) {
            Ok(_) => {
                // HS2-R7QH35: the running session is told its new ticket. Best effort:
                // the assignment stands whatever the push answered.
                let _ = self.session_tell_ticket(
                    &request.owner_id,
                    &session.session_id,
                    Some(&actor),
                    tx,
                );
                AssignOutcome::Assigned { run_id: run.run_id }
            }
            Err(error) => refused(&error),
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
