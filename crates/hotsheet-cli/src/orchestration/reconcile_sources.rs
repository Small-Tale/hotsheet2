//! The external sources a reconciliation reads besides the target ticket
//! (HS2-7QNK7Y and its siblings), each as a typed [`Observed<ExternalWork>`] built
//! from this machine's own records and its adapters' records. The classification is
//! pure and lives in the core (`reconcile_sources.rs`); this module only reads.
use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::{
    AttemptAnswer, AttemptRecord, ExternalWork, GateError, GateFilter, GateKind, GateObservation,
    Observations, Observed, ResultUpdate, Run, TargetAction, TargetFacts, ValidationResult,
    ValidationState, ValidationStep, ci_work, deployment_work, evaluate, validation_attempt,
    validation_work,
};

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
        Observations {
            ci: self.run_ci_work(owner_id, &run.run_id),
            deployment: self.run_deployment_work(owner_id, &run.run_id),
            validation: self.run_validation_work(owner_id, &run.run_id, actor, tx),
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
}
