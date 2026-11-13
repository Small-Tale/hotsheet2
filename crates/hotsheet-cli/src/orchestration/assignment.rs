//! HS2-T16SK0: a placed step's run waits, in dispatcher mode, for the target's
//! dispatcher or legacy integrator to assign its ticket. This finds the runs a
//! running scheduler session waits on, reads the target's coordination pool through
//! the hosted session listing (`roles`), reports the runs nobody will assign, and,
//! after the policy's bounded wait, claims the ticket for the session's own worker id.
//! The decision itself is the pure `decide_assignment`.
use std::collections::BTreeMap;

use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::{
    AssignmentInput, AssignmentState, DispatchMode, OrchestratedSession, PlacementAction,
    PlacementMemory, PlacementOutcome, PolicyBody, Run, RunState, RunUpdate, SessionOwner,
    SessionRole, SessionState, TargetAction, TargetOpError, decide_assignment,
};
use serde::Serialize;

use super::runs::field_ms;
use super::sessions::{HostedRoles, HostedSessionList};
use super::tickets::encode;
use super::*;

/// The run reason when a session claimed its run's ticket because no dispatcher
/// would assign it.
pub const CLAIMED_FOR_SESSION: &str = "claimed_for_session";

/// One placed step's run that a running scheduler session waits on, with who will
/// assign it and what the pass did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Assignment {
    pub run_id: String,
    pub session_id: String,
    pub target_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step_id: Option<String>,
    pub state: AssignmentState,
    /// The target's pool shows a live dispatcher; absent when it was not read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dispatcher_live: Option<bool>,
    /// The target's pool shows a live legacy integrator, which also assigns.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub integrator_live: Option<bool>,
    pub queued_at_ms: u64,
    /// While nobody will assign it: when the session claims the ticket for itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claim_at_ms: Option<u64>,
    /// The session registered in the target's pool in its role (HS2-2DPGDY). A
    /// session that has not is not given a claim; its joining is the event that
    /// lets the claim go ahead.
    pub joined: bool,
    /// What an applied pass did: `claimed`, or `refused` with the code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome_detail: Option<String>,
    /// The pass should claim now.
    #[serde(skip)]
    pub(super) claim: bool,
    /// When this changes by time alone.
    #[serde(skip)]
    pub(super) retry_at_ms: Option<u64>,
    /// The worker id the session's claims carry.
    #[serde(skip)]
    pub(super) worker_id: String,
}

/// The plan step a scheduler run (`sched-run-<step>` or `sched-run-<step>.<n>`)
/// was dispatched for.
pub(super) fn step_of(run: &Run) -> Option<String> {
    let rest = run.operation_id.strip_prefix("sched-run-")?;
    let step = match rest.rsplit_once('.') {
        Some((step, n)) if n.parse::<u32>().is_ok_and(|n| n >= 2) => step,
        _ => rest,
    };
    (!step.is_empty()).then(|| step.to_owned())
}

/// When a run was last queued for the target's dispatcher.
fn queued_at(run: &Run) -> u64 {
    run.history
        .iter()
        .rfind(|entry| entry.to == RunState::Queued)
        .map_or(run.created_at_ms, |entry| entry.at_ms)
}

impl OrchestrationRuntime {
    /// The target's coordination pool as the hosted listing of `session` reports it:
    /// whether a dispatcher and an integrator are live. `None` when it could not be
    /// asked. One read, through the gate as `sessions_manage` continuing.
    fn pool_roles(
        &self,
        session: &OrchestratedSession,
        actor: &MutationActor,
        tx: &mut Transaction<'_>,
    ) -> Option<HostedRoles> {
        let (target, authorized) = self
            .session_gate(session, Some(actor), true, None, tx)
            .ok()?;
        let outcome = (|| {
            let reply = self.hosted_call_at(
                Self::destination(&authorized)?,
                "/target-sessions",
                "GET",
                &format!("?operation_id={}", encode(&session.operation_id)),
                None,
                tx,
            )?;
            serde_json::from_value::<HostedSessionList>(reply)
                .map(|list| list.roles)
                .map_err(|_| OrchestrationError::Observation(TargetFailure::InvalidResponse))
        })();
        self.finish(&target, &authorized, &outcome, tx).ok()?;
        outcome.ok()
    }

    /// Every dispatcher-mode run still `queued` that a running worker session the
    /// owner's orchestrator owns was given, decided against the target's pool. Each
    /// target's pool is read once, as `actor`; without one nothing is read and every
    /// run reads `pool_unknown`.
    pub(super) fn scheduler_assignments(
        &self,
        owner_id: &str,
        policy: &PolicyBody,
        memory: &[PlacementMemory],
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Vec<Assignment> {
        let (Ok(sessions), Ok(runs)) = (
            self.session_list(owner_id, true),
            self.run_list(owner_id, true),
        ) else {
            return Vec::new();
        };
        let now_ms = tx.clock.now_ms();
        let wait_ms = u64::from(policy.placement.dispatcher_wait_minutes) * 60 * 1000;
        let mut roles: BTreeMap<String, Option<HostedRoles>> = BTreeMap::new();
        let mut found = Vec::new();
        for session in sessions.iter().filter(|session| {
            session.state == SessionState::Running
                && session.owner == SessionOwner::Orchestrator
                && session.role == SessionRole::Worker
        }) {
            let (Some(hosted), Some(run)) = (
                &session.hosted,
                session
                    .run_id
                    .as_ref()
                    .and_then(|run_id| runs.iter().find(|run| &run.run_id == run_id)),
            ) else {
                continue;
            };
            if run.mode != DispatchMode::Dispatcher
                || run.state != RunState::Queued
                || run.paused.is_some()
                || run.cancel_requested
            {
                continue;
            }
            let pool = match roles.get(&session.target_id) {
                Some(known) => *known,
                None => {
                    let read = actor.and_then(|actor| self.pool_roles(session, actor, tx));
                    roles.insert(session.target_id.clone(), read);
                    read
                }
            };
            let queued_at_ms = queued_at(run);
            let backoff_until_ms = memory
                .iter()
                .find(|entry| entry.target_id == session.target_id)
                .and_then(PlacementMemory::refused_until_ms);
            let input = AssignmentInput {
                dispatcher_live: pool.and_then(|pool| pool.dispatcher_live),
                integrator_live: pool.and_then(|pool| pool.integrator_live),
                queued_at_ms,
                wait_ms,
                backoff_until_ms,
                now_ms,
            };
            let decided = decide_assignment(&input);
            let joined = session.joined_at_ms.is_some();
            found.push(Assignment {
                run_id: run.run_id.clone(),
                session_id: session.session_id.clone(),
                target_id: session.target_id.clone(),
                step_id: step_of(run),
                state: decided.state,
                dispatcher_live: input.dispatcher_live,
                integrator_live: input.integrator_live,
                queued_at_ms,
                claim_at_ms: (decided.state == AssignmentState::NoLiveDispatcher).then(|| {
                    queued_at_ms
                        .saturating_add(wait_ms)
                        .max(backoff_until_ms.unwrap_or(0))
                }),
                joined,
                outcome: None,
                outcome_detail: None,
                claim: decided.claim && joined,
                retry_at_ms: decided.retry_at_ms,
                worker_id: hosted.worker_id.clone(),
            });
        }
        found
    }

    /// An applied pass claims each run that is due for its session, records what
    /// happened on the assignment and audits it. A claim someone else took is
    /// learned by the run's own sync; any other failure backs the target off like a
    /// refused start, so it is not tried again on every event.
    pub(super) fn assignment_apply(
        &self,
        owner_id: &str,
        assignments: &mut [Assignment],
        actor: Option<&MutationActor>,
        applied: &mut Vec<(String, PlacementOutcome)>,
        tx: &mut Transaction<'_>,
    ) {
        let Some(actor) = actor else {
            return;
        };
        for assignment in assignments.iter_mut().filter(|found| found.claim) {
            match self.assignment_claim(owner_id, assignment, actor, tx) {
                Ok(_) => {
                    assignment.outcome = Some("claimed".into());
                    // HS2-R7QH35: the session is told the ticket is now its own.
                    let _ =
                        self.session_tell_ticket(owner_id, &assignment.session_id, Some(actor), tx);
                }
                Err(error) => {
                    assignment.outcome = Some("refused".into());
                    assignment.outcome_detail = Some(error.code().into());
                    if matches!(
                        error,
                        OrchestrationError::TargetOperation(TargetOpError::Conflict)
                    ) {
                        // Someone took the ticket meanwhile: the run learns who now,
                        // so the next pass does not try again.
                        let _ = self.run_sync(owner_id, &assignment.run_id, Some(actor), tx);
                    } else {
                        applied.push((
                            assignment.target_id.clone(),
                            PlacementOutcome::Refused(error.code().into()),
                        ));
                    }
                }
            }
            self.scheduler_audit(
                owner_id,
                "scheduler_assignment",
                &DecisionOutcome {
                    action: PlacementAction::Keep,
                    target_id: assignment.target_id.clone(),
                    session_id: Some(assignment.session_id.clone()),
                    step_id: assignment.step_id.clone(),
                    outcome: assignment.outcome.clone().unwrap_or_default(),
                    detail: assignment.outcome_detail.clone(),
                    operation_id: assignment.run_id.clone(),
                    run_id: Some(assignment.run_id.clone()),
                },
                assignment.outcome.as_deref() == Some("claimed"),
                Some(actor),
                tx.clock.now_ms(),
            );
        }
    }

    /// Claim a queued run's ticket for the session's worker id, because nobody in the
    /// target's pool will assign it. Passes the gate as `work_start` continuing the
    /// run's own dispatch, so a person's approval for it is not asked again. A ticket
    /// someone else claimed meanwhile is a conflict; nothing is forced.
    pub(super) fn assignment_claim(
        &self,
        owner_id: &str,
        assignment: &Assignment,
        actor: &MutationActor,
        tx: &mut Transaction<'_>,
    ) -> Result<Run, OrchestrationError> {
        let run = self.run(owner_id, &assignment.run_id)?;
        if run.state != RunState::Queued || run.mode != DispatchMode::Dispatcher {
            return Err(OrchestrationError::Run(
                hotsheet_ticketing::orchestration::RunError::NotActive,
            ));
        }
        let target = Self::target_ref(owner_id, &run.target.target_id)?;
        let authorized = self.authorize(
            &target,
            Some(actor),
            ActionRequest::new(TargetAction::WorkStart)
                .operation(run.operation_id.clone())
                .continuing(),
            tx,
        )?;
        let outcome = (|| {
            let body = serde_json::to_value(HostedClaim {
                connection_id: run.target.connection_id.clone(),
                native_id: run.target.native_id.clone(),
                worker: assignment.worker_id.clone(),
                label: None,
                lease_minutes: None,
            })
            .map_err(|_| OrchestrationError::Corrupt)?;
            let hosted: HostedTicket = serde_json::from_value(self.hosted_call_at(
                Self::destination(&authorized)?,
                "/target-claims",
                "POST",
                "",
                Some(&body),
                tx,
            )?)
            .map_err(|_| OrchestrationError::Observation(TargetFailure::InvalidResponse))?;
            let claimed = self.run_apply(
                owner_id,
                &run.run_id,
                RunUpdate {
                    to: Some(RunState::Dispatched),
                    reason: Some(CLAIMED_FOR_SESSION.into()),
                    worker_id: Some(assignment.worker_id.clone()),
                    lease_expires_at_ms: Some(field_ms(&hosted.ticket, "claim_lease_expires_at")),
                    actor_id: actor.id.clone(),
                    ..RunUpdate::default()
                },
                tx,
            )?;
            self.run_note(owner_id, &claimed, "claimed for its session", tx);
            Ok(claimed)
        })();
        self.finish(&target, &authorized, &outcome, tx)?;
        outcome
    }
}
