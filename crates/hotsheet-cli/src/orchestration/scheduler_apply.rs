//! HS2-ECWG50: applying one placement. Inside the policy's limits the scheduler acts
//! and logs. HS2-XHQ1EH: a start no remembered rule allows files a permission request
//! and nothing is done; the person's answer wakes the scheduler. A wind-down only
//! reduces activity, so it never asks.
use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::{
    AccessRequest, AuditOutcome, AuditRecord, DenialReason, Placement, PlacementAction,
    PlacementOutcome, PlacementReason, SessionLoad, SessionRequest, TargetAction,
};

use super::scheduler::Gathered;
use super::*;

fn stop_operation(session_id: &str, tick_id: &str) -> String {
    let operation = format!("sched-stop-{session_id}");
    if operation.len() <= 160 {
        operation
    } else {
        format!("sched-stop-{tick_id}")
    }
}

impl OrchestrationRuntime {
    /// Whether the gate allows a session action now, without filing or using a
    /// permission request. When it does not, the action is passed through
    /// `authorize`, which files the request (shown through the permission bridge)
    /// and audits the denial. `Ok` means go ahead; the action itself passes the gate
    /// again and uses an Allow Once answer only once it happened. The error's detail
    /// names the policy limit or the permission request a person must answer. `Ok`
    /// carries the Allow Once request that allowed it, to use once the act happened.
    fn scheduler_gate(
        &self,
        target: &TargetRef,
        actor: Option<&MutationActor>,
        request: AccessRequest,
        tx: &mut Transaction<'_>,
    ) -> Result<Option<String>, (String, Option<String>)> {
        let decision = self
            .access(target, &request, access::is_person(actor), tx)
            .map_err(|error| (error.code().to_owned(), None))?;
        if decision.allowed {
            return Ok(decision
                .request_id
                .filter(|_| decision.basis.as_deref() == Some("allowed_once")));
        }
        let action = ActionRequest {
            session: request.session,
            operation_id: request.operation_id,
            load: request.load,
            provisional: request.provisional,
            role_session: request.role_session,
            ..ActionRequest::new(request.action)
        };
        match self.authorize(target, actor, action, tx) {
            // A person answered between the two reads: the action is authorized.
            Ok(authorized) => Ok(authorized
                .permission_id
                .filter(|_| authorized.basis == "allowed_once")),
            Err(error) => Err((
                error.code().to_owned(),
                error
                    .answer_reference()
                    .map(|(_, id)| id.to_owned())
                    .or(decision.detail),
            )),
        }
    }

    pub(super) fn scheduler_audit(
        &self,
        owner_id: &str,
        action: &str,
        outcome: &DecisionOutcome,
        succeeded: bool,
        actor: Option<&MutationActor>,
        now_ms: u64,
    ) {
        let _ = self.audit_record(
            owner_id,
            AuditRecord {
                at_ms: now_ms,
                actor_role: access::role_code(actor),
                actor_id: actor.and_then(|actor| actor.id.clone()),
                target_id: Some(outcome.target_id.clone()),
                action: action.into(),
                operation_id: Some(outcome.operation_id.clone()),
                outcome: if succeeded {
                    AuditOutcome::Succeeded
                } else {
                    AuditOutcome::Failed
                },
                reason: Some(outcome.outcome.clone()),
                permission_id: None,
                requested_by: None,
            },
        );
    }

    /// HS2-JSF985: a session was started for provisional work. The work is recorded
    /// with the gates it rests on, so their results reach it. Best effort: the
    /// session runs either way, and a dispatch of the same step records it if this
    /// could not.
    fn scheduler_started_provisional(
        &self,
        owner_id: &str,
        outcome: &DecisionOutcome,
        standing: &hotsheet_ticketing::orchestration::ProvisionalStanding,
        actor_id: &str,
        tx: &mut Transaction<'_>,
    ) {
        let Some(step) = outcome
            .step_id
            .as_deref()
            .and_then(|step_id| self.plan_step(owner_id, step_id).ok())
        else {
            return;
        };
        let _ = self.provisional_record(
            owner_id,
            &step,
            standing,
            &outcome.operation_id,
            actor_id,
            tx.clock.now_ms(),
        );
    }

    #[allow(clippy::too_many_arguments)] // HS2-ECWG50: one placement with the pass's context
    pub(super) fn scheduler_start(
        &self,
        owner_id: &str,
        gathered: &Gathered,
        placement: &Placement,
        actor: Option<&MutationActor>,
        actor_id: &str,
        applied: &mut Vec<(String, PlacementOutcome)>,
        tx: &mut Transaction<'_>,
    ) -> DecisionOutcome {
        let step_id = placement.steps.first().cloned();
        // HS2-TK9ADC: a role session's start is stable per target and role.
        let operation_id = match placement.role {
            Some(role) => format!("sched-role-{}-{}", role.code(), placement.target_id),
            None => format!("sched-start-{}", step_id.as_deref().unwrap_or("none")),
        };
        let mut outcome = DecisionOutcome {
            action: PlacementAction::Start,
            target_id: placement.target_id.clone(),
            session_id: None,
            step_id,
            outcome: String::new(),
            detail: None,
            operation_id: operation_id.clone(),
            run_id: None,
        };
        let inputs = &placement.inputs;
        let load = SessionLoad {
            target_sessions: inputs.target_sessions.saturating_sub(1),
            machine_sessions: inputs.machine_sessions.unwrap_or(1).saturating_sub(1),
            tool_sessions: inputs.tool_sessions.unwrap_or(0),
            account: placement.account.clone(),
            account_sessions: inputs.account_sessions.unwrap_or(0),
        };
        let target = TargetRef {
            owner_checkout_id: owner_id.into(),
            target_id: placement.target_id.clone(),
        };
        let gate = self.scheduler_gate(
            &target,
            actor,
            AccessRequest {
                actor_id: actor_id.into(),
                action: TargetAction::SessionsManage,
                session: Some(SessionRequest {
                    tool: placement.tool.clone(),
                    model: placement.model.clone(),
                    running: load.target_sessions,
                    starting: true,
                }),
                operation_id: Some(operation_id.clone()),
                load: Some(load.clone()),
                // HS2-JSF985: the policy's speculation switch applies to this start,
                // and building ahead asks under `provisional_dispatch` (HS2-XHQ1EH).
                provisional: placement.provisional,
                // HS2-TK9ADC: a role start asks `session_start` like any start.
                role_session: placement.role.is_some(),
            },
            tx,
        );
        let allowed_once = match gate {
            Ok(allowed_once) => allowed_once,
            Err((code, detail)) => {
                outcome.outcome = code;
                outcome.detail = detail;
                return outcome;
            }
        };
        if gathered.executor != "available" {
            outcome.outcome = format!("executor_{}", gathered.executor);
            return outcome;
        }
        // HS2-JSF985: the rule's answer the placement was decided on. Without one the
        // work is not started ahead of anything.
        let provisional = match (placement.provisional, outcome.step_id.as_deref()) {
            (false, _) => None,
            (true, step_id) => match step_id
                .and_then(|step_id| gathered.standings.get(step_id))
                .filter(|standing| standing.allowed)
            {
                Some(standing) => Some(standing.clone()),
                None => {
                    outcome.outcome = "speculation_refused".into();
                    return outcome;
                }
            },
        };
        let request = PlacementStart {
            owner_id: owner_id.into(),
            target_id: placement.target_id.clone(),
            tool: placement.tool.clone(),
            account: placement.account.clone(),
            model: placement.model.clone(),
            operation_id,
            actor_id: actor_id.into(),
            steps: placement.steps.clone(),
            load,
            limits: gathered.policy.machine_limits(&placement.target_id),
            probe: placement.probe,
            resources: gathered.resources_of(&placement.steps),
            role: placement.role,
        };
        let answer = self.session_executor.start(self, &request, tx);
        let succeeded = match answer {
            StartOutcome::Started { session_id, run_id } => {
                outcome.outcome = "started".into();
                outcome.session_id = Some(session_id);
                outcome.run_id = run_id;
                applied.push((placement.target_id.clone(), PlacementOutcome::Started));
                // HS2-XHQ1EH: the start happened, so an Allow Once answer is used.
                if let Some(request_id) = &allowed_once {
                    let _ = self.permission_ledger().used(request_id, tx);
                }
                if let Some(standing) = &provisional {
                    self.scheduler_started_provisional(owner_id, &outcome, standing, actor_id, tx);
                }
                true
            }
            // HS2-DG0YPA: nothing started, so the target's memory is unchanged.
            StartOutcome::Adopted { session_id, run_id } => {
                outcome.outcome = "adopted".into();
                outcome.session_id = Some(session_id);
                outcome.run_id = Some(run_id);
                true
            }
            StartOutcome::Refused { reason } => {
                outcome.outcome = "refused".into();
                applied.push((
                    placement.target_id.clone(),
                    PlacementOutcome::Refused(reason.clone()),
                ));
                outcome.detail = Some(reason);
                false
            }
            // HS2-XHQ1EH: the step's run (`sched-run-<step>`) or the start waits for a
            // person, recorded like a start the scheduler's own gate turned into a
            // request: the outcome code and the request id to answer. No backoff; the
            // answer wakes the scheduler.
            StartOutcome::PermissionRequired { request_id } => {
                outcome.outcome = DenialReason::PermissionRequired.code().into();
                outcome.detail = Some(request_id);
                return outcome;
            }
            StartOutcome::Unavailable => {
                outcome.outcome = "executor_unavailable".into();
                return outcome;
            }
        };
        self.scheduler_audit(
            owner_id,
            "scheduler_start",
            &outcome,
            succeeded,
            actor,
            tx.clock.now_ms(),
        );
        outcome
    }

    /// HS2-YC2P7K: a free session the pass gave ready work gets that work's run.
    /// `None` when the executor does not dispatch runs, so nothing is recorded. The
    /// run's own dispatch passes the gate as `work_start`.
    pub(super) fn scheduler_assign(
        &self,
        owner_id: &str,
        placement: &Placement,
        actor: Option<&MutationActor>,
        actor_id: &str,
        tx: &mut Transaction<'_>,
    ) -> Option<DecisionOutcome> {
        let step_id = placement.steps.first()?.clone();
        let session_id = placement.session_id.clone()?;
        let answer = self.session_executor.assign(
            self,
            &PlacementAssign {
                owner_id: owner_id.into(),
                target_id: placement.target_id.clone(),
                session_id: session_id.clone(),
                step_id: step_id.clone(),
                actor_id: actor_id.into(),
            },
            tx,
        );
        let mut outcome = DecisionOutcome {
            action: PlacementAction::Keep,
            target_id: placement.target_id.clone(),
            session_id: Some(session_id),
            step_id: Some(step_id.clone()),
            outcome: String::new(),
            detail: None,
            operation_id: super::session_executor::run_operation(&step_id),
            run_id: None,
        };
        let succeeded = match answer {
            AssignOutcome::Unsupported => return None,
            AssignOutcome::Assigned { run_id } => {
                outcome.outcome = "assigned".into();
                outcome.run_id = Some(run_id);
                true
            }
            AssignOutcome::Refused { reason } => {
                outcome.outcome = "refused".into();
                outcome.detail = Some(reason);
                false
            }
        };
        self.scheduler_audit(
            owner_id,
            "scheduler_assign",
            &outcome,
            succeeded,
            actor,
            tx.clock.now_ms(),
        );
        Some(outcome)
    }

    #[allow(clippy::too_many_arguments)] // HS2-ECWG50: one placement with the pass's context
    pub(super) fn scheduler_wind_down(
        &self,
        owner_id: &str,
        gathered: &Gathered,
        placement: &Placement,
        actor: Option<&MutationActor>,
        actor_id: &str,
        tick_id: &str,
        applied: &mut Vec<(String, PlacementOutcome)>,
        tx: &mut Transaction<'_>,
    ) -> DecisionOutcome {
        let session_id = placement.session_id.clone().unwrap_or_default();
        let operation_id = stop_operation(&session_id, tick_id);
        let mut outcome = DecisionOutcome {
            action: PlacementAction::WindDown,
            target_id: placement.target_id.clone(),
            session_id: Some(session_id.clone()),
            step_id: None,
            outcome: String::new(),
            detail: None,
            operation_id: operation_id.clone(),
            run_id: None,
        };
        let target = TargetRef {
            owner_checkout_id: owner_id.into(),
            target_id: placement.target_id.clone(),
        };
        // HS2-48H57C: winding down only reduces activity. It never asks, and a policy
        // that no longer permits the target or tool does not deny it; it passes the
        // same gate as any other wind-down for its identified actor and audit entry.
        let gate = self.scheduler_gate(
            &target,
            actor,
            AccessRequest {
                actor_id: actor_id.into(),
                action: TargetAction::SessionsManage,
                session: Some(SessionRequest {
                    tool: placement.tool.clone(),
                    model: placement.model.clone(),
                    running: placement.inputs.target_sessions,
                    starting: false,
                }),
                operation_id: Some(operation_id.clone()),
                load: None,
                provisional: false,
                role_session: false,
            },
            tx,
        );
        if let Err((code, detail)) = gate {
            outcome.outcome = code;
            outcome.detail = detail;
            return outcome;
        }
        if gathered.executor != "available" {
            outcome.outcome = format!("executor_{}", gathered.executor);
            return outcome;
        }
        let request = WindDownSession {
            owner_id: owner_id.into(),
            target_id: placement.target_id.clone(),
            session_id,
            tool: placement.tool.clone(),
            model: placement.model.clone(),
            operation_id,
            actor_id: actor_id.into(),
            reason: placement.reason.code().into(),
        };
        let answer = self.session_executor.wind_down(self, &request, tx);
        let succeeded = match answer {
            WindDownOutcome::Requested => {
                outcome.outcome = "wind_down_requested".into();
                // HS2-NPTKS8: a session that never joined is replaced, not a change of
                // direction, so it starts no cooldown on the target.
                if !matches!(
                    placement.reason,
                    PlacementReason::NeverJoined
                        | PlacementReason::RolePromptFailed
                        | PlacementReason::RunCancelled
                ) {
                    applied.push((placement.target_id.clone(), PlacementOutcome::WoundDown));
                }
                true
            }
            WindDownOutcome::NotAtSafePoint => {
                outcome.outcome = "not_at_safe_point".into();
                false
            }
            WindDownOutcome::NotFound => {
                outcome.outcome = "session_not_found".into();
                false
            }
            WindDownOutcome::Refused { reason } => {
                outcome.outcome = "refused".into();
                outcome.detail = Some(reason);
                false
            }
            WindDownOutcome::Unavailable => {
                outcome.outcome = "executor_unavailable".into();
                return outcome;
            }
        };
        self.scheduler_audit(
            owner_id,
            "scheduler_wind_down",
            &outcome,
            succeeded,
            actor,
            tx.clock.now_ms(),
        );
        outcome
    }
}
