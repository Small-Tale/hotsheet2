//! The external sources a reconciliation reads besides the target ticket
//! (HS2-7QNK7Y and its siblings), each as a typed [`Observed<ExternalWork>`] built
//! from this machine's own records and its adapters' records. The classification is
//! pure and lives in the core (`reconcile_sources.rs`); this module only reads.
use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::{
    AttemptAnswer, AttemptRecord, ExternalWork, GateError, GateFilter, GateKind, GateObservation,
    GateState, NOTE_WAITING_ON_GATE, NOTE_WAITING_ON_VALIDATION, Observations, Observed,
    OrchestratedSession, ResultUpdate, Run, RunState, SafePointBlocker, SessionNeed, SessionOwner,
    TargetAction, TargetFacts, ValidationResult, ValidationState, ValidationStep, ci_work,
    deployment_work, evaluate, session_work, validation_attempt, validation_work,
};

use super::sessions::StopSession;

use super::tickets::encode;
use super::validation::{HostedExecutionState, HostedValidation};

use super::*;

/// The code of a source whose records on this machine could not be read.
const GATE_RECORDS_UNREADABLE: &str = "gate_records_unreadable";

impl OrchestrationRuntime {
    /// Every observation reconciliation compares a run with, given what the target
    /// read showed.
    pub(super) fn run_observations(
        &self,
        owner_id: &str,
        run: &Run,
        target: Observed<TargetFacts>,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Observations {
        let ci = self.run_ci_work(owner_id, &run.run_id);
        let deployment = self.run_deployment_work(owner_id, &run.run_id);
        // Validation first: it may record that an attempt ended, which changes whether
        // the run still waits on one.
        let validation = self.run_validation_work(owner_id, &run.run_id, actor, tx);
        let session = self.run_session_work(owner_id, run, actor, tx);
        Observations {
            ci,
            deployment,
            validation,
            session,
            ..Observations::target_only(target)
        }
    }

    /// The run's standing gates of one kind, each with its adapter's own answer.
    /// Reads this machine's records and the adapters' records; contacts no provider.
    fn gate_readings(
        &self,
        owner_id: &str,
        run_id: &str,
        kind: GateKind,
    ) -> Result<Vec<GateObservation>, OrchestrationError> {
        let gates = self.gate_registry().list(
            owner_id,
            &GateFilter {
                run_id: Some(run_id.into()),
                current_only: true,
                ..GateFilter::default()
            },
        )?;
        Ok(gates
            .into_iter()
            .filter(|gate| gate.kind == kind)
            .map(|gate| {
                let reading = match self.gate_provider(&gate.provider, gate.kind) {
                    Some(provider) => provider.read(&gate).map_err(|error| error.code().into()),
                    None => Err(GateError::ProviderUnsupported.code().into()),
                };
                (gate, reading)
            })
            .collect())
    }

    /// HS2-7QNK7Y: the CI runs gating a run, for reconciliation. Verified when the
    /// run's waiting CI gates are the watches the CI adapter shows live (or there are
    /// none in flight); mismatched with `recorded_work_missing` when a watch the run
    /// waits on is gone; unverifiable with `watch_lost` or the adapter's code when it
    /// cannot say.
    pub fn run_ci_work(&self, owner_id: &str, run_id: &str) -> Observed<ExternalWork> {
        match self.gate_readings(owner_id, run_id, GateKind::Ci) {
            Ok(readings) => ci_work(&readings),
            Err(_) => Observed::Unverifiable(GATE_RECORDS_UNREADABLE.into()),
        }
    }

    /// HS2-BYEB60: the deployments started for a run, for reconciliation, by their
    /// retained operation ids. No deployment executor ships (HS2-G3VAK4), so for an
    /// approved request this verifies, from this machine's records, that nothing was
    /// executed (`deployment_not_executed`). An execution a registered executor
    /// recorded is read from that executor's own records.
    pub fn run_deployment_work(&self, owner_id: &str, run_id: &str) -> Observed<ExternalWork> {
        match self.gate_readings(owner_id, run_id, GateKind::Deployment) {
            Ok(readings) => deployment_work(&readings),
            Err(_) => Observed::Unverifiable(GATE_RECORDS_UNREADABLE.into()),
        }
    }

    /// HS2-G5H9FQ: the validation attempts that name a run, for reconciliation. Each
    /// attempt still `running` is asked of the machine that hosts its step's target,
    /// without waiting and without starting anything, under a continuing
    /// `validation_run` gate as `actor`. What the host observed is recorded as a
    /// continuing request would record it: an outcome, or `unverified` with
    /// `execution_lost` for an execution the host confirmed and no longer knows. A host
    /// that cannot be asked leaves the attempt `running` and the source unverifiable.
    pub fn run_validation_work(
        &self,
        owner_id: &str,
        run_id: &str,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Observed<ExternalWork> {
        let steps = match self.validations().steps(owner_id, None, tx.clock.now_ms()) {
            Ok(steps) => steps,
            Err(_) => return Observed::Unverifiable("validation_records_unreadable".into()),
        };
        let mut works = Vec::new();
        for evidence in steps {
            for result in evidence
                .history
                .iter()
                .filter(|result| result.run_id.as_deref() == Some(run_id))
            {
                let answer = (result.state == ValidationState::Running)
                    .then(|| self.attempt_answer(owner_id, &evidence.step, result, actor, tx));
                let (answer, outcome, gate) = match answer {
                    Some((answer, outcome, gate)) => (Some(answer), outcome, gate),
                    None => (None, None, None),
                };
                let (work, record) = validation_attempt(result, answer.as_ref(), tx.clock.now_ms());
                let update = match (record, outcome) {
                    (AttemptRecord::Outcome, Some(outcome)) => {
                        let (state, reason) =
                            evaluate(&evidence.step.definition.evidence, &outcome);
                        Some(ResultUpdate {
                            to: Some(state),
                            reason: reason.map(Into::into),
                            outcome: Some(outcome),
                            host_started: None,
                        })
                    }
                    (AttemptRecord::Unverified(reason), _) => Some(ResultUpdate {
                        to: Some(ValidationState::Unverified),
                        reason: Some(reason.into()),
                        outcome: None,
                        host_started: None,
                    }),
                    _ => None,
                };
                if let Some(update) = update {
                    let step = &evidence.step;
                    match self.validations().finish(
                        owner_id,
                        &step.ticket,
                        &step.definition.step_id,
                        &result.result_id,
                        update,
                        tx,
                    ) {
                        Ok(finished) => {
                            if let Some((target, authorized)) = &gate {
                                let passed = finished.state == ValidationState::Passed;
                                let _ = self.record_outcome(
                                    target,
                                    authorized,
                                    passed,
                                    (!passed).then(|| finished.state.code()),
                                    tx,
                                );
                            }
                            self.validation_report(
                                owner_id,
                                &step.ticket,
                                &step.definition.step_id,
                                &step.definition.target_id,
                                &finished,
                                tx,
                            );
                        }
                        // The record could not be written: what was observed is not
                        // on record, so nothing is concluded.
                        Err(error) => {
                            works.push(
                                hotsheet_ticketing::orchestration::SourceWork::Unverifiable(
                                    error.code().into(),
                                ),
                            );
                            continue;
                        }
                    }
                }
                works.push(work);
            }
        }
        validation_work(works)
    }

    /// Ask the machine hosting a step about one attempt, with no wait. Returns the
    /// answer, the outcome when the host has one, and the gate decision to record a
    /// finished attempt under.
    fn attempt_answer(
        &self,
        owner_id: &str,
        step: &ValidationStep,
        result: &ValidationResult,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> (
        AttemptAnswer,
        Option<hotsheet_ticketing::orchestration::ExecutionOutcome>,
        Option<(TargetRef, Authorized)>,
    ) {
        let unreachable = |error: OrchestrationError| {
            (AttemptAnswer::Unreachable(error.code().into()), None, None)
        };
        let target = match Self::target_ref(owner_id, &step.definition.target_id) {
            Ok(target) => target,
            Err(error) => return unreachable(error),
        };
        // Continuing the request the attempt belongs to: nobody is asked again.
        let authorized = match self.authorize(
            &target,
            actor,
            ActionRequest::new(TargetAction::ValidationRun)
                .operation(result.operation_id.clone())
                .continuing(),
            tx,
        ) {
            Ok(authorized) => authorized,
            Err(error) => return unreachable(error),
        };
        let reply = Self::destination(&authorized).and_then(|destination| {
            self.hosted_call_at(
                destination,
                "/target-validations",
                "GET",
                &format!("?execution_id={}&wait_ms=0", encode(&result.result_id)),
                None,
                tx,
            )
        });
        let hosted = reply.and_then(|reply| {
            serde_json::from_value::<HostedValidation>(reply)
                .map_err(|_| OrchestrationError::Observation(TargetFailure::InvalidResponse))
        });
        match hosted {
            Ok(HostedValidation {
                outcome: Some(outcome),
                ..
            }) => (
                AttemptAnswer::Finished,
                Some(outcome),
                Some((target, authorized)),
            ),
            Ok(HostedValidation {
                state: HostedExecutionState::Unknown,
                ..
            }) => (AttemptAnswer::Unknown, None, Some((target, authorized))),
            Ok(_) => (AttemptAnswer::Running, None, None),
            Err(error) => unreachable(error),
        }
    }

    /// HS2-75Q254: the orchestrator-owned sessions started for a run, for
    /// reconciliation. Each active one is first read from its host as `actor` (the
    /// same safe-point read the scheduler makes), so a crashed session is `lost` and a
    /// person's takeover at the host is learned; a host that cannot be asked leaves the
    /// source unverifiable (`session_unknown`, or the refusal's code). The run's
    /// latest session is then judged against whether the run needs one now
    /// ([`SessionNeed`]): a session wound down on purpose is never missing work, and a
    /// lost one is missing work only while the run is being worked.
    pub fn run_session_work(
        &self,
        owner_id: &str,
        run: &Run,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Observed<ExternalWork> {
        let sessions = match self.session_list(owner_id, false) {
            Ok(sessions) => sessions,
            Err(_) => return Observed::Unverifiable("session_records_unreadable".into()),
        };
        let mut seen = Vec::new();
        for session in sessions
            .into_iter()
            .filter(|session| session.run_id.as_deref() == Some(run.run_id.as_str()))
        {
            if session.state.is_terminal() {
                seen.push((session, None));
                continue;
            }
            // The safe-point read syncs the record and says whether the terminal host
            // could be asked at all, which a plain sync leaves unsaid.
            match self.session_safe_point(owner_id, &session.session_id, actor, tx) {
                Ok(view) => {
                    let unknown = view.safe_point.as_ref().is_some_and(|point| {
                        point.blockers.contains(&SafePointBlocker::SessionUnknown)
                    });
                    let code = (unknown && view.session.state.is_active())
                        .then(|| SafePointBlocker::SessionUnknown.code().to_owned());
                    seen.push((view.session, code));
                }
                Err(error) => seen.push((session, Some(error.code().to_owned()))),
            }
        }
        session_work(&seen, self.session_need(owner_id, run, tx.clock.now_ms()))
    }

    /// HS2-75Q254: whether a run needs a live session now. A dispatched run waits,
    /// and needs none, while a CI or deployment gate of it waits on a provider or a
    /// person, while its target ticket is in `final_testing`, or while a validation
    /// attempt of it runs. Reads this machine's records only.
    pub fn session_need(&self, owner_id: &str, run: &Run, now_ms: u64) -> SessionNeed {
        if run.state != RunState::Dispatched {
            return SessionNeed::Idle;
        }
        let gates = self
            .gate_registry()
            .list(
                owner_id,
                &GateFilter {
                    run_id: Some(run.run_id.clone()),
                    current_only: true,
                    ..GateFilter::default()
                },
            )
            .unwrap_or_default();
        let gated = gates
            .iter()
            .any(|gate| gate.awaits_provider() || gate.state == GateState::AwaitingApproval)
            || run
                .observed
                .as_ref()
                .and_then(|observed| observed.started_phase.as_deref())
                == Some("final_testing");
        if gated {
            return SessionNeed::Waiting(NOTE_WAITING_ON_GATE);
        }
        let validating = self
            .validations()
            .steps(owner_id, None, now_ms)
            .unwrap_or_default()
            .iter()
            .flat_map(|evidence| &evidence.history)
            .any(|result| {
                result.run_id.as_deref() == Some(run.run_id.as_str())
                    && result.state == ValidationState::Running
            });
        if validating {
            return SessionNeed::Waiting(NOTE_WAITING_ON_VALIDATION);
        }
        SessionNeed::Working
    }

    /// HS2-75Q254: the session that still holds a run's work, from this machine's
    /// records: its latest session while that is `requested` or `running`. A start for
    /// the run's work while this answers `Some` would be a duplicate; the start path
    /// should adopt it instead (see the handoff of HS2-75Q254).
    pub fn run_session_holder(
        &self,
        owner_id: &str,
        run_id: &str,
    ) -> Result<Option<OrchestratedSession>, OrchestrationError> {
        Ok(self
            .session_list(owner_id, false)?
            .into_iter()
            .rfind(|session| session.run_id.as_deref() == Some(run_id))
            .filter(|session| session.state.is_active()))
    }

    /// HS2-75Q254: a run reconciliation found finished no longer needs its sessions.
    /// When the policy enables automatic placement the scheduler's idle wind-down
    /// keeps or stops them (it applies `idle_minutes` and `keep_warm`); otherwise
    /// nobody else would, so each orchestrator-owned session of the run is asked to stop
    /// at a safe point its host confirms. A session that is not at one is left
    /// running; a person's session is never touched. Returns the sessions stopped.
    pub(super) fn wind_down_finished_run(
        &self,
        owner_id: &str,
        run: &Run,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Vec<String> {
        let automatic = self
            .policy(owner_id)
            .ok()
            .and_then(|view| view.policy)
            .is_some_and(|policy| policy.placement.automatic);
        if automatic || actor.is_none() {
            return Vec::new();
        }
        let sessions = self.session_list(owner_id, true).unwrap_or_default();
        sessions
            .into_iter()
            .filter(|session| {
                session.run_id.as_deref() == Some(run.run_id.as_str())
                    && session.owner != SessionOwner::Human
            })
            .filter_map(|session| {
                self.session_stop(
                    owner_id,
                    &session.session_id,
                    StopSession { force: false },
                    actor,
                    tx,
                )
                .ok()
                .map(|stopped| stopped.session.session_id)
            })
            .collect()
    }
}
