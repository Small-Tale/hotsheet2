//! HS2-ECWG50: applying one placement. Inside the policy the scheduler acts and logs;
//! outside it, the gate files the approval a policy asks for and nothing is done.
use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::{
    AccessRequest, AuditOutcome, AuditRecord, GrantRegistry, Placement, PlacementAction,
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
    /// Whether the gate allows a session action now, without consuming an approval.
    /// When it does not, the action is passed through `authorize`, which files the
    /// approval a policy asks for and audits the denial. `Ok` means go ahead.
    fn scheduler_gate(
        &self,
        target: &TargetRef,
        actor: Option<&MutationActor>,
        request: AccessRequest,
        tx: &mut Transaction<'_>,
    ) -> Result<(), (String, Option<String>)> {
        let decision = self
            .access(target, &request, tx)
            .map_err(|error| (error.code().to_owned(), None))?;
        if decision.allowed {
            return Ok(());
        }
        let action = ActionRequest {
            session: request.session,
            operation_id: request.operation_id,
            load: request.load,
            provisional: request.provisional,
            ..ActionRequest::new(request.action)
        };
        match self.authorize(target, actor, action, tx) {
            // A person answered between the two reads: the action is authorized.
            Ok(_) => Ok(()),
            Err(error) => Err((error.code().to_owned(), decision.detail)),
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
                grant_id: None,
            },
        );
    }

    /// HS2-JSF985: a session was started for provisional work. The approval a policy
    /// asked for is used by this one start, and the work is recorded with the gates
    /// it rests on, so their results reach it. Best effort: the session runs either
    /// way, and a dispatch of the same step records it if this could not.
    #[allow(clippy::too_many_arguments)] // HS2-JSF985: one start with the pass's context
    fn scheduler_started_provisional(
        &self,
        owner_id: &str,
        target: &TargetRef,
        outcome: &DecisionOutcome,
        standing: &hotsheet_ticketing::orchestration::ProvisionalStanding,
        actor_id: &str,
        gathered: &Gathered,
        tx: &mut Transaction<'_>,
    ) {
        use hotsheet_ticketing::orchestration::{ApprovalGate, ApprovalRegistry};
        if gathered
            .policy
            .approval
            .required_for
            .contains(&ApprovalGate::ProvisionalDispatch)
        {
            let _ = ApprovalRegistry::new(&self.home).consume_or_request(
                target,
                actor_id,
                ApprovalGate::ProvisionalDispatch,
                &outcome.operation_id,
                tx,
            );
        }
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
        let operation_id = format!("sched-start-{}", step_id.as_deref().unwrap_or("none"));
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
                // HS2-JSF985: the policy's speculation switch and, when it lists
                // one, its `provisional_dispatch` approval apply to this start.
                provisional: placement.provisional,
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
        };
        let answer = self.session_executor.start(self, &request, tx);
        let succeeded = match answer {
            StartOutcome::Started { session_id, run_id } => {
                outcome.outcome = "started".into();
                outcome.session_id = Some(session_id);
                outcome.run_id = run_id;
                applied.push((placement.target_id.clone(), PlacementOutcome::Started));
                if let Some(standing) = &provisional {
                    self.scheduler_started_provisional(
                        owner_id, &target, &outcome, standing, actor_id, gathered, tx,
                    );
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
        if placement.reason == PlacementReason::PolicyNotPermitted {
            // The policy's own gate denies everything on a target or tool it no
            // longer permits, and stopping that session is what the policy asks
            // for. The grant still has to allow managing it.
            let request = AccessRequest {
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
            };
            let granted = Self::binding_at(&self.home, &target).and_then(|binding| {
                GrantRegistry::new(&self.home).decide(
                    &target,
                    binding.destination.as_ref(),
                    &request,
                    tx.clock.now_ms(),
                )
            });
            match granted {
                Ok(decision) if decision.allowed => {}
                Ok(decision) => {
                    outcome.outcome = decision
                        .reason
                        .map_or("denied", |reason| reason.code())
                        .into();
                    return outcome;
                }
                Err(error) => {
                    outcome.outcome = error.code().into();
                    return outcome;
                }
            }
        } else {
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
                applied.push((placement.target_id.clone(), PlacementOutcome::WoundDown));
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
