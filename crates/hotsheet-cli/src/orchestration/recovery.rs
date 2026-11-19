//! HS2-WBREDM: reconciling runs after an interruption, and the control actions a
//! person or supervisor takes on one: pause, resume, handoff and correction.
//!
//! Reconciliation reads; it never starts work. It brings the run in line with its
//! target ticket, repeats a cancel the target was never told about, and records per
//! external source what agrees, what does not and what could not be verified.
use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::{
    AuditOutcome, AuditRecord, DispatchMode, Observed, Run, RunControl, RunError, RunPause,
    RunReconciliation, RunState, RunUpdate, TargetAction, TargetFacts, TargetOpError, assess,
};
use serde::{Deserialize, Serialize};

use super::runs::{field_ms, text};
use super::tickets::identifier;
use super::watch::supervisor;
use super::*;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PauseRun {
    /// A stable code for why, such as `maintenance`. Never free text.
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandoffRun {
    /// The worker that takes the run over. Absent hands it to a person: the run lets
    /// go of its claim and keeps following the ticket.
    #[serde(default)]
    pub to_worker: Option<String>,
    #[serde(default)]
    pub worker_label: Option<String>,
    #[serde(default)]
    pub lease_minutes: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorrectRun {
    /// The state the run is really in.
    pub state: RunState,
    /// A stable code for why the record was wrong. Never free text.
    pub reason: String,
    /// The worker that really holds the work, when that is what was wrong.
    #[serde(default)]
    pub worker_id: Option<String>,
}

/// One run a bulk reconciliation could not examine at all.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReconcileFailure {
    pub run_id: String,
    pub code: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReconcileSummary {
    /// Every unfinished run after reconciliation, each with its `reconciliation` and
    /// the names of what could not be verified.
    pub runs: Vec<RunView>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub failed: Vec<ReconcileFailure>,
}

fn actor_id(actor: Option<&MutationActor>) -> Result<String, OrchestrationError> {
    actor
        .and_then(|actor| actor.id.clone())
        .ok_or(OrchestrationError::Invalid(
            "an identified actor is required to control a run",
        ))
}

fn view(run: Run) -> RunView {
    RunView {
        unverified: run
            .reconciliation
            .as_ref()
            .map(RunReconciliation::unverified)
            .unwrap_or_default(),
        run,
    }
}

impl OrchestrationRuntime {
    /// Owning checkouts that have an unfinished run: what a restarted host reconciles.
    pub fn run_owners(&self) -> Result<Vec<String>, OrchestrationError> {
        self.runs().unfinished_owners()
    }

    /// A control action changes this machine's record only, so it must work while the
    /// target cannot be reached. It is audited here rather than through the target
    /// gate, which needs a fresh resolution of the target.
    fn control_audit(
        &self,
        owner_id: &str,
        run: &Run,
        actor: Option<&MutationActor>,
        action: &'static str,
        reason: Option<String>,
        tx: &mut Transaction<'_>,
    ) -> Result<(), OrchestrationError> {
        self.audit_record(
            owner_id,
            AuditRecord {
                at_ms: tx.clock.now_ms(),
                actor_role: super::access::role_code(actor),
                actor_id: actor.and_then(|actor| actor.id.clone()),
                target_id: Some(run.target.target_id.clone()),
                action: action.into(),
                operation_id: Some(run.operation_id.clone()),
                outcome: AuditOutcome::Succeeded,
                reason,
                permission_id: None,
                requested_by: None,
            },
        )
    }

    /// Reconcile one run with its target: read the target ticket, bring the run in
    /// line with it, repeat a cancel the target was never told about, and record what
    /// each external source showed. A target that cannot be confirmed is not an error
    /// here: the answer names it, with the reason, as unverified. Without an actor the
    /// run is reconciled as whoever dispatched it.
    pub fn run_reconcile(
        &self,
        owner_id: &str,
        run_id: &str,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<RunView, OrchestrationError> {
        let before = self.run(owner_id, run_id)?;
        // HS2-JSF985: a gate that resolved while the orchestrator was down decides
        // what becomes of the work started ahead of it, whatever state this run is in.
        self.provisional_follow(owner_id, tx);
        if before.state.is_terminal() {
            return Ok(view(before));
        }
        let lead = actor.cloned().or_else(|| supervisor(&before));
        let now_ms = tx.clock.now_ms();
        let target = match self.run_sync(owner_id, run_id, lead.as_ref(), tx) {
            Ok(synced) => {
                let gone = synced.run.state == RunState::Failed
                    && synced.run.history.last().and_then(|e| e.reason.as_deref())
                        == Some(TargetOpError::NotFound.code());
                match &synced.run.observed {
                    _ if gone => Observed::Verified(TargetFacts::missing()),
                    Some(observed) => Observed::Verified(TargetFacts {
                        exists: true,
                        status: observed.status.clone(),
                        claimed_by: observed.claimed_by.clone(),
                        lease_expires_at_ms: observed.lease_expires_at_ms,
                        claim_count: observed.claim_count,
                    }),
                    None => Observed::Unverifiable("target_not_observed".into()),
                }
            }
            // The request itself or this machine's own storage is at fault: nothing
            // was learned about the target either way.
            Err(
                error @ (OrchestrationError::Invalid(_)
                | OrchestrationError::Storage(_)
                | OrchestrationError::Corrupt
                | OrchestrationError::UnsupportedVersion { .. }
                | OrchestrationError::Run(_)),
            ) => return Err(error),
            Err(error) => Observed::Unverifiable(error.code().into()),
        };
        let mut run = self.run(owner_id, run_id)?;
        // A cancel that could not reach the target is repeated now that it answers.
        // Releasing a claim and withdrawing an Up Next mark are both idempotent.
        if matches!(target, Observed::Verified(_))
            && run.cancel_requested
            && run.paused.is_none()
            && !run.state.is_terminal()
        {
            if let Ok(cancelled) = self.run_cancel(owner_id, run_id, lead.as_ref(), tx) {
                run = cancelled.run;
            }
        }
        if run.state.is_terminal() {
            // HS2-75Q254: the run's sessions have nothing left to do for it.
            self.wind_down_finished_run(owner_id, &run, lead.as_ref(), tx);
            return Ok(view(run));
        }
        let observations = self.run_observations(owner_id, &run, target, lead.as_ref(), tx);
        let found = RunReconciliation::new(now_ms, assess(&run, &observations, now_ms));
        self.run_apply(
            owner_id,
            run_id,
            RunUpdate {
                reconciliation: Some(found),
                actor_id: lead.and_then(|actor| actor.id),
                ..RunUpdate::default()
            },
            tx,
        )
        .map(view)
    }

    /// Reconcile every unfinished run of one owning checkout.
    pub fn runs_reconcile(
        &self,
        owner_id: &str,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<ReconcileSummary, OrchestrationError> {
        let mut summary = ReconcileSummary {
            runs: Vec::new(),
            failed: Vec::new(),
        };
        // HS2-JSF985: also when no run is unfinished.
        self.provisional_follow(owner_id, tx);
        for run in self.run_list(owner_id, false)? {
            if run.state.is_terminal() {
                continue;
            }
            match self.run_reconcile(owner_id, &run.run_id, actor, tx) {
                Ok(view) => summary.runs.push(view),
                Err(error) => summary.failed.push(ReconcileFailure {
                    run_id: run.run_id,
                    code: error.code(),
                }),
            }
        }
        Ok(summary)
    }

    /// Pause a run: until it is resumed the orchestrator does not renew its lease or
    /// repeat its dispatch. The target is not told and its claim is left alone.
    /// Pausing a paused run changes nothing.
    pub fn run_pause(
        &self,
        owner_id: &str,
        run_id: &str,
        request: PauseRun,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<RunView, OrchestrationError> {
        let actor_id = actor_id(actor)?;
        if let Some(reason) = &request.reason {
            identifier(reason, "bounded reason code required")?;
        }
        let _operation = self.run_lock(owner_id, run_id)?;
        let run = self.run(owner_id, run_id)?;
        if run.state.is_terminal() {
            return Err(OrchestrationError::Run(RunError::NotActive));
        }
        if run.paused.is_some() {
            return Ok(view(run));
        }
        let paused = self.run_apply(
            owner_id,
            run_id,
            RunUpdate {
                control: Some(RunControl::Pause),
                paused: Some(Some(RunPause {
                    since_ms: tx.clock.now_ms(),
                    reason: request.reason.clone(),
                    actor_id: Some(actor_id.clone()),
                })),
                reason: Some(request.reason.clone().unwrap_or_else(|| "paused".into())),
                actor_id: Some(actor_id),
                ..RunUpdate::default()
            },
            tx,
        )?;
        self.control_audit(owner_id, &paused, actor, "run_pause", request.reason, tx)?;
        Ok(view(paused))
    }

    /// Resume a paused run and reconcile it at once, so nothing is resumed on a
    /// record that went stale while it was paused. Resuming a run that is not paused
    /// only reconciles it.
    pub fn run_resume(
        &self,
        owner_id: &str,
        run_id: &str,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<RunView, OrchestrationError> {
        let actor_id = actor_id(actor)?;
        let operation = self.run_lock(owner_id, run_id)?;
        let run = self.run(owner_id, run_id)?;
        if run.state.is_terminal() {
            return Err(OrchestrationError::Run(RunError::NotActive));
        }
        if run.paused.is_some() {
            let resumed = self.run_apply(
                owner_id,
                run_id,
                RunUpdate {
                    control: Some(RunControl::Resume),
                    paused: Some(None),
                    reason: Some("resumed".into()),
                    actor_id: Some(actor_id),
                    ..RunUpdate::default()
                },
                tx,
            )?;
            self.control_audit(owner_id, &resumed, actor, "run_resume", None, tx)?;
        }
        drop(operation);
        self.run_reconcile(owner_id, run_id, actor, tx)
            .or_else(|_| self.run(owner_id, run_id).map(view))
    }

    /// Hand a run to another worker, or to a person. Naming a worker that already
    /// holds the target ticket adopts that claim. Otherwise a direct run releases its
    /// own claim and claims the ticket for the new worker; a claim someone else holds
    /// is never forced away. Without a worker the run lets go of its claim and keeps
    /// following the ticket as `intervened`.
    pub fn run_handoff(
        &self,
        owner_id: &str,
        run_id: &str,
        request: HandoffRun,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<RunView, OrchestrationError> {
        if let Some(worker) = &request.to_worker {
            identifier(worker, "bounded worker id required")?;
        }
        let minutes = request
            .lease_minutes
            .unwrap_or(super::runs::DEFAULT_LEASE_MINUTES);
        if !(5..=super::runs::MAX_LEASE_MINUTES).contains(&minutes) {
            return Err(OrchestrationError::Invalid(
                "lease minutes must be 5 to 240",
            ));
        }
        // HS2-EZV83C: the old claim's release, the new claim and the record of the
        // handoff are one step to a concurrent sync. Without this, the server's own
        // follower read the target in between and recorded the handoff as a person's
        // intervention (`dispatched -> intervened -> dispatched`).
        let _operation = self.run_lock(owner_id, run_id)?;
        let run = self.run(owner_id, run_id)?;
        if run.state.is_terminal() {
            return Err(OrchestrationError::Run(RunError::NotActive));
        }
        if run.paused.is_some() {
            return Err(OrchestrationError::Run(RunError::Paused));
        }
        // A worker that already holds a direct run on this target takes no second one.
        if let Some(next) = &request.to_worker {
            let busy = self.run_list(owner_id, true)?.into_iter().any(|other| {
                other.run_id != run.run_id
                    && other.mode == DispatchMode::Direct
                    && other.worker_id.as_ref() == Some(next)
                    && other.target.target_id == run.target.target_id
            });
            if busy {
                return Err(OrchestrationError::Run(RunError::WorkerBusy));
            }
        }
        let target = Self::target_ref(owner_id, &run.target.target_id)?;
        let authorized = self.authorize(
            &target,
            actor,
            ActionRequest::new(TargetAction::WorkStart)
                .operation(run.operation_id.clone())
                .continuing(),
            tx,
        )?;
        let outcome = (|| {
            let destination = Self::destination(&authorized)?;
            let ticket = self.read_authorized(&authorized, &Self::run_target(&run), tx)?;
            let now_ms = tx.clock.now_ms();
            let lease = field_ms(&ticket, "claim_lease_expires_at");
            let live = text(&ticket, "claimed_by").filter(|_| lease.is_some_and(|at| at > now_ms));
            let own = run.worker_id.as_deref();
            let claim = |worker: &str, label: Option<String>, minutes: Option<u32>| {
                serde_json::to_value(HostedClaim {
                    connection_id: run.target.connection_id.clone(),
                    native_id: run.target.native_id.clone(),
                    worker: worker.into(),
                    label,
                    lease_minutes: minutes,
                })
                .map_err(|_| OrchestrationError::Corrupt)
            };
            let release_own = |tx: &mut Transaction<'_>| -> Result<(), OrchestrationError> {
                if let Some(worker) = own.filter(|worker| live == Some(*worker)) {
                    let body = claim(worker, None, None)?;
                    self.hosted_call_at(
                        destination,
                        "/target-claims",
                        "DELETE",
                        "",
                        Some(&body),
                        tx,
                    )?;
                }
                Ok(())
            };
            let step = |to, reason: &str| RunUpdate {
                control: Some(RunControl::Handoff),
                to: Some(to),
                reason: Some(reason.into()),
                actor_id: actor.and_then(|actor| actor.id.clone()),
                ..RunUpdate::default()
            };
            let update = match request.to_worker.as_deref() {
                // The new worker already holds the ticket: adopt, claim nothing.
                Some(next) if live == Some(next) => RunUpdate {
                    worker_id: Some(next.into()),
                    lease_expires_at_ms: Some(lease),
                    ..step(RunState::Dispatched, "handoff_adopted")
                },
                Some(_) if run.mode != DispatchMode::Direct => {
                    return Err(OrchestrationError::Invalid(
                        "the target's dispatcher chooses the worker; a handoff can only adopt the worker holding the ticket",
                    ));
                }
                // Someone else holds it: their claim is theirs.
                Some(_) if live.is_some() && live != own => {
                    return Err(OrchestrationError::TargetOperation(TargetOpError::Conflict));
                }
                Some(next) => {
                    release_own(tx)?;
                    let body = claim(next, request.worker_label.clone(), Some(minutes))?;
                    let hosted: HostedTicket = serde_json::from_value(self.hosted_call_at(
                        destination,
                        "/target-claims",
                        "POST",
                        "",
                        Some(&body),
                        tx,
                    )?)
                    .map_err(|_| OrchestrationError::Observation(TargetFailure::InvalidResponse))?;
                    RunUpdate {
                        worker_id: Some(next.into()),
                        lease_expires_at_ms: Some(field_ms(
                            &hosted.ticket,
                            "claim_lease_expires_at",
                        )),
                        ..step(RunState::Dispatched, "handed_off")
                    }
                }
                None => {
                    if run.mode == DispatchMode::Direct {
                        release_own(tx)?;
                    }
                    step(RunState::Intervened, "handed_off")
                }
            };
            let handed = self.run_apply(owner_id, run_id, update, tx)?;
            if handed.history.len() != run.history.len() || handed.worker_id != run.worker_id {
                self.run_note(owner_id, &handed, "handed off", tx);
            }
            Ok(view(handed))
        })();
        self.finish(&target, &authorized, &outcome, tx)?;
        outcome
    }

    /// Correct a run's record by hand when it is wrong and the target cannot settle
    /// it. Any unfinished run may be moved to any other state, backwards included; a
    /// finished run stays finished. Only this machine's record changes: the target is
    /// not told, and the next successful read of the target still wins. An AI actor
    /// cannot correct a run it supervises.
    pub fn run_correct(
        &self,
        owner_id: &str,
        run_id: &str,
        request: CorrectRun,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<RunView, OrchestrationError> {
        if actor.is_some_and(MutationActor::is_ai) {
            return Err(OrchestrationError::Forbidden(
                "a run is corrected by a person, never by an AI actor",
            ));
        }
        let actor_id = actor_id(actor)?;
        identifier(&request.reason, "bounded reason code required")?;
        if let Some(worker) = &request.worker_id {
            identifier(worker, "bounded worker id required")?;
        }
        let _operation = self.run_lock(owner_id, run_id)?;
        let run = self.run(owner_id, run_id)?;
        if run.state.is_terminal() {
            return Err(OrchestrationError::Run(RunError::NotActive));
        }
        let corrected = self.run_apply(
            owner_id,
            run_id,
            RunUpdate {
                control: Some(RunControl::Correct),
                to: Some(request.state),
                reason: Some(request.reason.clone()),
                actor_id: Some(actor_id),
                worker_id: request.worker_id,
                ..RunUpdate::default()
            },
            tx,
        )?;
        if corrected != run {
            self.control_audit(
                owner_id,
                &corrected,
                actor,
                "run_correct",
                Some(request.reason),
                tx,
            )?;
            if corrected.state.is_terminal() {
                self.run_note(owner_id, &corrected, "finished by a correction", tx);
            }
        }
        Ok(view(corrected))
    }
}
