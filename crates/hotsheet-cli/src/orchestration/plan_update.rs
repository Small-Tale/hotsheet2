//! HS2-190RDX: Update Plan. A re-plan runs in the background while placement keeps
//! working the live plan: no pause. One press opens a machine-local draft seeded from
//! the live steps and asks the project's planner (a new one, or the open one with a
//! new proposal) to draft the plan there. Finishing the update swaps the checked-in
//! plan to the draft in one locked write: unchanged steps keep their running work,
//! and the scheduler runs of removed or changed steps are withdrawn, so their
//! sessions wind down at a safe point their host confirms (HS2-YFPZFP). A conflicting
//! or invalid draft, a cancel, or a planner that ends first leaves the live plan as
//! it was, with the reason recorded.
use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::{
    AuditOutcome, AuditRecord, PlanRegistry, PlanSwap, PlanUpdate, PlanUpdateState,
    PlanUpdateStore, PlanningState, ReadinessError, UpdateDecision, decide_update,
    plan_update_reason, planning_state,
};
use serde::{Deserialize, Serialize};

use super::planning::{PlanProposal, ProposeOutcome, ProposePlan};
use super::sessions::HostedSessionView;
use super::tickets::identifier;
use super::*;

/// HS2-190RDX: the reason of a scheduler run withdrawn because the plan update
/// changed its step. The step stays, so the scheduler dispatches it again from its
/// new definition once the session working the old one wound down.
pub const STEP_REPLANNED_RUN: &str = "step_replanned";

/// A press of Update Plan.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdatePlan {
    /// The AI tool a new planner runs in, by plugin id. An open planner keeps its own.
    pub tool: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub effort: Option<String>,
    /// A retained id: the same id answers with the same update and never opens two.
    #[serde(default)]
    pub operation_id: Option<String>,
}

/// Names the update to finish or cancel; the drafting one when omitted.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanUpdateRef {
    #[serde(default)]
    pub update_id: Option<String>,
}

/// HS2-190RDX: names the plan update whose draft a readiness read or change acts on.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PlanQuery {
    #[serde(default)]
    pub plan_update: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateOutcome {
    /// A new update opened for this press.
    Started,
    /// The drafting update, or the one this operation id names, answers instead.
    Existing,
}

/// Where one plan update stands, without its base.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanUpdateView {
    pub update_id: String,
    pub state: PlanUpdateState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<UpdateOutcome>,
    /// The planner's operation id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planner: Option<String>,
    /// The planner's hosted session, as Propose Plan shows it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<HostedSessionView>,
    /// Steps the draft holds now (while drafting).
    pub draft_steps: u32,
    pub created_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conflicts: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub swap: Option<PlanSwap>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unwithdrawn: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applied_revision: Option<String>,
}

impl OrchestrationRuntime {
    fn plan_updates(&self, owner_id: &str) -> Result<PlanUpdateStore, OrchestrationError> {
        PlanUpdateStore::new(&self.home, owner_id)
    }

    /// HS2-190RDX: whether `proposal` is the drafting update of the owner, so the
    /// planner's prompt names the draft. Best effort: unreadable reads as no.
    pub(super) fn plan_update_drafted_by(&self, owner_id: &str, proposal: &str) -> bool {
        self.plan_updates(owner_id)
            .and_then(|store| store.latest())
            .ok()
            .flatten()
            .is_some_and(|update| update.state.is_open() && update.update_id == proposal)
    }

    /// The plan a readiness request acts on: the live plan, or the named update's
    /// draft, which must be drafting (`plan_update_not_found`, `plan_update_not_open`).
    pub(super) fn plan_for(
        &self,
        owner_id: &str,
        plan_update: Option<&str>,
    ) -> Result<PlanRegistry, OrchestrationError> {
        let Some(update_id) = plan_update else {
            return Ok(self.plan());
        };
        identifier(update_id, "bounded plan update id required")?;
        match self.plan_updates(owner_id)?.latest()? {
            Some(update) if update.update_id == update_id && update.state.is_open() => {
                self.plan().for_update(&self.home, owner_id)
            }
            Some(update) if update.update_id == update_id => {
                Err(OrchestrationError::Readiness(ReadinessError::UpdateNotOpen))
            }
            _ => Err(OrchestrationError::Readiness(
                ReadinessError::UpdateNotFound,
            )),
        }
    }

    fn update_audit(
        &self,
        owner_id: &str,
        actor: Option<&MutationActor>,
        action: &str,
        update_id: Option<&str>,
        (outcome, reason): (AuditOutcome, Option<&str>),
        tx: &Transaction<'_>,
    ) -> Result<(), OrchestrationError> {
        self.audit_record(
            owner_id,
            AuditRecord {
                at_ms: tx.clock.now_ms(),
                actor_role: access::role_code(actor),
                actor_id: actor.and_then(|actor| actor.id.clone()),
                target_id: None,
                action: action.into(),
                operation_id: update_id.map(Into::into),
                outcome,
                reason: reason.map(Into::into),
                permission_id: None,
                requested_by: None,
            },
        )
    }

    /// End a drafting update without changing the live plan: record why and forget
    /// the draft. The caller holds the store's lock.
    fn end_update(
        store: &PlanUpdateStore,
        update: &mut PlanUpdate,
        state: PlanUpdateState,
        reason: &str,
        now_ms: u64,
    ) -> Result<(), OrchestrationError> {
        update.state = state;
        update.reason = Some(reason.into());
        update.ended_at_ms = Some(now_ms);
        store.write(update)?;
        store.discard_draft()
    }

    /// The latest update, with a drafting one whose planner ended first failed as
    /// `plan_update_planner_ended` on the way (as planners are recorded ended).
    fn plan_update_latest(
        &self,
        owner: &Checkout,
        store: &PlanUpdateStore,
        tx: &mut Transaction<'_>,
    ) -> Result<Option<(PlanUpdate, Option<HostedSessionView>)>, OrchestrationError> {
        let Some(mut update) = store.latest()? else {
            return Ok(None);
        };
        let planners = self.planners(owner, tx)?;
        let session = update.planner.as_ref().and_then(|planner| {
            planners
                .iter()
                .find(|view| &view.session.operation_id == planner)
                .cloned()
        });
        if update.state.is_open() && update.planner.is_some() {
            let facts =
                Self::planner_facts(&session.iter().cloned().collect::<Vec<HostedSessionView>>());
            if planning_state(&facts) != PlanningState::Planning {
                Self::end_update(
                    store,
                    &mut update,
                    PlanUpdateState::Failed,
                    plan_update_reason::PLANNER_ENDED,
                    tx.clock.now_ms(),
                )?;
                self.update_audit(
                    &owner.id,
                    None,
                    "plan_update_apply",
                    Some(&update.update_id),
                    (
                        AuditOutcome::Failed,
                        Some(plan_update_reason::PLANNER_ENDED),
                    ),
                    tx,
                )?;
            }
        }
        Ok(Some((update, session)))
    }

    fn update_view(
        &self,
        store: &PlanUpdateStore,
        update: PlanUpdate,
        session: Option<HostedSessionView>,
        outcome: Option<UpdateOutcome>,
    ) -> PlanUpdateView {
        let draft_steps = if update.state.is_open() {
            store.draft_steps().map_or(0, |steps| steps.len())
        } else {
            0
        };
        PlanUpdateView {
            update_id: update.update_id,
            state: update.state,
            outcome,
            planner: update.planner,
            session,
            draft_steps: u32::try_from(draft_steps).unwrap_or(u32::MAX),
            created_at_ms: update.created_at_ms,
            ended_at_ms: update.ended_at_ms,
            reason: update.reason,
            conflicts: update.conflicts,
            swap: update.swap,
            unwithdrawn: update.unwithdrawn,
            applied_revision: update.applied_revision,
        }
    }

    /// HS2-190RDX: the latest plan update for `plan show`, when there is one.
    pub(super) fn plan_update_status(
        &self,
        owner: &Checkout,
        tx: &mut Transaction<'_>,
    ) -> Result<Option<PlanUpdateView>, OrchestrationError> {
        let store = self.plan_updates(&owner.id)?;
        let _lock = store.lock()?;
        Ok(self
            .plan_update_latest(owner, &store, tx)?
            .map(|(update, session)| self.update_view(&store, update, session, None)))
    }

    /// Update Plan: open a draft of the plan and ask the planner to draft it, while
    /// placement keeps working the live plan. One update at a time: a second press
    /// answers with the drafting one. Refused before anything opens as Propose Plan
    /// is (`plan_no_targets`, `plan_no_goals`). A planner that cannot take the update
    /// fails it at once with its reason, and the live plan is unchanged. Needs an
    /// identified actor and is audited as `plan_update`.
    pub fn plan_update(
        &self,
        owner_id: &str,
        request: UpdatePlan,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<PlanUpdateView, OrchestrationError> {
        if actor.and_then(|actor| actor.id.as_ref()).is_none() {
            return Err(OrchestrationError::Invalid(
                "an identified actor is required to update the plan",
            ));
        }
        identifier(&request.tool, "bounded tool id required")?;
        if let Some(operation_id) = &request.operation_id {
            identifier(operation_id, "bounded operation id required")?;
        }
        let owner = self.owner(owner_id)?;
        if self.declarations(owner_id)?.targets.is_empty() {
            return Err(OrchestrationError::Readiness(ReadinessError::NoTargets));
        }
        if self.plan_goals(&owner, None)?.1 == 0 {
            return Err(OrchestrationError::Readiness(ReadinessError::NoGoals));
        }
        let store = self.plan_updates(owner_id)?;
        let lock = store.lock()?;
        let latest = self.plan_update_latest(&owner, &store, tx)?;
        let decision = decide_update(
            latest.as_ref().map(|(update, _)| update),
            request.operation_id.as_deref(),
            || hotsheet_ticketing::mint_ulid(tx.clock, &mut *tx.rng).to_string(),
        );
        let update_id = match decision {
            UpdateDecision::Existing(_) => {
                let (update, session) = latest.ok_or(OrchestrationError::Corrupt)?;
                self.update_audit(
                    owner_id,
                    actor,
                    "plan_update",
                    Some(&update.update_id),
                    (AuditOutcome::Succeeded, Some("plan_update_existing")),
                    tx,
                )?;
                return Ok(self.update_view(
                    &store,
                    update,
                    session,
                    Some(UpdateOutcome::Existing),
                ));
            }
            UpdateDecision::Open(update_id) => update_id,
        };
        let (base, base_revision) = self.plan().checked_in_steps(owner_id)?;
        store.seed_draft(base.clone(), tx)?;
        let mut update = PlanUpdate {
            update_id: update_id.clone(),
            state: PlanUpdateState::Drafting,
            planner: None,
            base_revision,
            base,
            created_at_ms: tx.clock.now_ms(),
            ended_at_ms: None,
            requested_by: actor.and_then(|actor| actor.id.clone()),
            reason: None,
            conflicts: Vec::new(),
            swap: None,
            unwithdrawn: Vec::new(),
            applied_revision: None,
        };
        // Written before the planner is asked, so its launch prompt names the draft.
        store.write(&update)?;
        drop(lock);
        let mut operation_id = Some(update_id.clone());
        let asked = self.plan_propose_inner(
            &owner,
            &ProposePlan {
                tool: request.tool,
                model: request.model,
                effort: request.effort,
                goal: None,
                operation_id: Some(update_id.clone()),
                plan_update: true,
            },
            &mut operation_id,
            actor,
            tx,
        );
        let _lock = store.lock()?;
        let failure = match &asked {
            // The planner took this update: a new one, or the open one replanned.
            Ok(PlanProposal {
                outcome: ProposeOutcome::Started | ProposeOutcome::Replanned,
                ..
            }) => None,
            Ok(proposal) if proposal.proposal_id == update_id => None,
            // It is still at work on another proposal and cannot take this one.
            Ok(_) => Some(ReadinessError::PlannerBusy.code()),
            Err(error) => Some(error.code()),
        };
        if let Some(reason) = failure {
            Self::end_update(
                &store,
                &mut update,
                PlanUpdateState::Failed,
                reason,
                tx.clock.now_ms(),
            )?;
            self.update_audit(
                owner_id,
                actor,
                "plan_update",
                Some(&update_id),
                (AuditOutcome::Failed, Some(reason)),
                tx,
            )?;
            return match asked {
                Err(error) => Err(error),
                Ok(_) => Err(OrchestrationError::Readiness(ReadinessError::PlannerBusy)),
            };
        }
        let proposal = asked?;
        update.planner = Some(proposal.session.session.operation_id.clone());
        store.write(&update)?;
        self.update_audit(
            owner_id,
            actor,
            "plan_update",
            Some(&update_id),
            (AuditOutcome::Succeeded, Some("plan_update_started")),
            tx,
        )?;
        Ok(self.update_view(
            &store,
            update,
            Some(proposal.session),
            Some(UpdateOutcome::Started),
        ))
    }

    /// The drafting update `reference` names (the drafting one when it names none).
    fn drafting(
        store: &PlanUpdateStore,
        reference: &PlanUpdateRef,
    ) -> Result<PlanUpdate, OrchestrationError> {
        let update = store
            .latest()?
            .filter(|update| {
                reference
                    .update_id
                    .as_ref()
                    .is_none_or(|id| *id == update.update_id)
            })
            .ok_or(OrchestrationError::Readiness(
                ReadinessError::UpdateNotFound,
            ))?;
        if !update.state.is_open() {
            return Err(OrchestrationError::Readiness(ReadinessError::UpdateNotOpen));
        }
        Ok(update)
    }

    /// Finish a plan update: swap the live plan to its draft in one locked write of
    /// the checked-in plan (the three-way merge of `merge_update`), withdraw the
    /// scheduler runs of removed (`step_removed`) and changed (`step_replanned`)
    /// steps so their sessions wind down at a safe point, and wake the scheduler.
    /// A refused merge fails the update with `plan_update_conflict` or
    /// `plan_update_invalid` and leaves the live plan as it was. Audited as
    /// `plan_update_apply`.
    pub fn plan_update_finish(
        &self,
        owner_id: &str,
        reference: &PlanUpdateRef,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<PlanUpdateView, OrchestrationError> {
        if actor.and_then(|actor| actor.id.as_ref()).is_none() {
            return Err(OrchestrationError::Invalid(
                "an identified actor is required to update the plan",
            ));
        }
        let owner = self.owner(owner_id)?;
        let store = self.plan_updates(owner_id)?;
        let lock = store.lock()?;
        let mut update = Self::drafting(&store, reference)?;
        let draft = store.draft_steps()?;
        let observed = self
            .plan()
            .for_update(&self.home, owner_id)?
            .tickets(owner_id)?;
        let now_ms = tx.clock.now_ms();
        let (swap, revision) =
            match self
                .plan()
                .apply_update(owner_id, &update.base, &draft, observed, tx)?
            {
                Ok(applied) => applied,
                Err(refusal) => {
                    update.conflicts = refusal.conflicts;
                    Self::end_update(
                        &store,
                        &mut update,
                        PlanUpdateState::Failed,
                        refusal.reason,
                        now_ms,
                    )?;
                    self.update_audit(
                        owner_id,
                        actor,
                        "plan_update_apply",
                        Some(&update.update_id),
                        (AuditOutcome::Failed, Some(refusal.reason)),
                        tx,
                    )?;
                    return Err(OrchestrationError::Readiness(
                        if refusal.reason == plan_update_reason::CONFLICT {
                            ReadinessError::UpdateConflict
                        } else {
                            ReadinessError::UpdateInvalid
                        },
                    ));
                }
            };
        // The swap is done; the work it withdraws follows. A cancel only reduces
        // activity, so it never asks; a run the target cannot be told about stays
        // `cancel_requested` and is listed, for reconciliation to finish.
        let mut unwithdrawn = Vec::new();
        for (steps, reason) in [
            (&swap.removed, super::readiness::STEP_REMOVED_RUN),
            (&swap.changed, STEP_REPLANNED_RUN),
        ] {
            for run in self.run_list(owner_id, false)?.into_iter().filter(|run| {
                !run.state.is_terminal()
                    && steps
                        .iter()
                        .any(|step| super::session_executor::scheduled_run_of(run, step))
            }) {
                match self.run_cancel_as(owner_id, &run.run_id, reason, actor, tx) {
                    Ok(view) if !view.run.cancel_requested => {}
                    Err(OrchestrationError::Run(
                        hotsheet_ticketing::orchestration::RunError::NotActive,
                    )) => {}
                    Ok(_) | Err(_) => unwithdrawn.push(run.run_id.clone()),
                }
            }
        }
        update.state = PlanUpdateState::Applied;
        update.ended_at_ms = Some(now_ms);
        update.applied_revision = revision;
        update.swap = Some(swap.clone());
        update.unwithdrawn = unwithdrawn;
        store.write(&update)?;
        store.discard_draft()?;
        drop(lock);
        self.update_audit(
            owner_id,
            actor,
            "plan_update_apply",
            Some(&update.update_id),
            (AuditOutcome::Changed, Some("plan_update_applied")),
            tx,
        )?;
        for step in &swap.removed {
            self.readiness_withdrawn(owner_id, step, now_ms);
        }
        self.readiness_reevaluate(owner_id, now_ms);
        self.scheduler_wake(owner_id);
        let session = self
            .plan_update_status(&owner, tx)?
            .and_then(|view| view.session);
        Ok(self.update_view(&store, update, session, None))
    }

    /// Cancel a drafting plan update: the draft is forgotten and the live plan is
    /// unchanged. The planner's terminal stays open; its later draft writes are
    /// refused with `plan_update_not_open`. Audited as `plan_update_cancel`.
    pub fn plan_update_cancel(
        &self,
        owner_id: &str,
        reference: &PlanUpdateRef,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<PlanUpdateView, OrchestrationError> {
        if actor.and_then(|actor| actor.id.as_ref()).is_none() {
            return Err(OrchestrationError::Invalid(
                "an identified actor is required to cancel a plan update",
            ));
        }
        self.owner(owner_id)?;
        let store = self.plan_updates(owner_id)?;
        let _lock = store.lock()?;
        let mut update = Self::drafting(&store, reference)?;
        Self::end_update(
            &store,
            &mut update,
            PlanUpdateState::Cancelled,
            plan_update_reason::CANCELLED,
            tx.clock.now_ms(),
        )?;
        self.update_audit(
            owner_id,
            actor,
            "plan_update_cancel",
            Some(&update.update_id),
            (AuditOutcome::Changed, None),
            tx,
        )?;
        Ok(self.update_view(&store, update, None, None))
    }
}
