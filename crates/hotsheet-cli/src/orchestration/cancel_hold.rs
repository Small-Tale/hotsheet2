//! HS2-HV9ED3: a person's cancel of a scheduler run is respected. The step is held,
//! not dispatched again, until a person dispatches its ticket again or redefines the
//! step; the session working the cancelled run is told and wound down at its next
//! safe point. An AI actor's cancel is not a hold: the step is dispatched again.
use std::collections::BTreeSet;

use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::{
    PlacementAction, PlacementReason, PlanStep, RequestKind, Run, RunState, Schedule, SessionOwner,
    SessionState,
};

use super::*;

/// The run reason when a person cancelled it.
pub const CANCELLED_BY_PERSON: &str = "cancelled_by_person";

/// Whether a person cancelled the run.
pub(super) fn person_cancelled(run: &Run) -> bool {
    run.state == RunState::Cancelled
        && run.history.last().and_then(|entry| entry.reason.as_deref()) == Some(CANCELLED_BY_PERSON)
}

/// The run a person cancelled that holds `step`: its latest scheduler run, when a
/// person cancelled it, no run of the same target ticket was dispatched after that,
/// and the step was not redefined since. `None` when the step is not held.
pub(super) fn held_by<'a>(step: &PlanStep, runs: &'a [Run]) -> Option<&'a Run> {
    let latest = runs
        .iter()
        .filter(|run| super::session_executor::scheduled_run_of(run, &step.step_id))
        .max_by_key(|run| {
            (
                run.created_at_ms,
                run.operation_id.len(),
                run.operation_id.clone(),
            )
        })?;
    if !person_cancelled(latest) {
        return None;
    }
    let cancelled_at = latest
        .history
        .last()
        .map_or(latest.updated_at_ms, |entry| entry.at_ms);
    let dispatched_since = runs.iter().any(|run| {
        run.run_id != latest.run_id
            && run.target == latest.target
            && run.created_at_ms > cancelled_at
    });
    (step.updated_at_ms <= cancelled_at && !dispatched_since).then_some(latest)
}

/// HS2-YFPZFP: whether the run's work was withdrawn: a person cancelled it, or its
/// plan step was removed (or, HS2-190RDX, changed by a plan update). A session working such a run is told and wound down.
pub(super) fn withdrawn(run: &Run) -> bool {
    person_cancelled(run)
        || (run.state == RunState::Cancelled
            && matches!(
                run.history.last().and_then(|entry| entry.reason.as_deref()),
                Some(super::readiness::STEP_REMOVED_RUN | super::plan_update::STEP_REPLANNED_RUN)
            ))
}

impl OrchestrationRuntime {
    /// Runs of the owner whose work was withdrawn ([`withdrawn`]), by id.
    pub(super) fn withdrawn_runs(&self, owner_id: &str) -> BTreeSet<String> {
        self.run_list(owner_id, false)
            .unwrap_or_default()
            .into_iter()
            .filter(withdrawn)
            .map(|run| run.run_id)
            .collect()
    }

    /// Rewrite a computed schedule so each running session the orchestrator owns
    /// whose run's work was withdrawn (a person's cancel, or its step removed,
    /// HS2-YFPZFP) is wound down (`run_cancelled`) at its next safe point. A person's
    /// session is left alone.
    pub(super) fn scheduler_cancelled(&self, owner_id: &str, schedule: &mut Schedule) {
        let cancelled = self.withdrawn_runs(owner_id);
        if cancelled.is_empty() {
            return;
        }
        let Ok(records) = self.session_list(owner_id, true) else {
            return;
        };
        for session in records.iter().filter(|session| {
            session.state == SessionState::Running
                && session.owner == SessionOwner::Orchestrator
                && session
                    .run_id
                    .as_ref()
                    .is_some_and(|run| cancelled.contains(run))
        }) {
            if let Some(placement) = schedule.placements.iter_mut().find(|placement| {
                placement.session_id.as_deref() == Some(session.session_id.as_str())
                    && placement.action == PlacementAction::Keep
            }) {
                placement.action = PlacementAction::WindDown;
                placement.reason = PlacementReason::RunCancelled;
                placement.detail = None;
                placement.steps.clear();
                placement.score = None;
                placement.retry_at_ms = None;
            }
        }
    }

    /// Tell each session about to be wound down for a cancelled run why, through its
    /// host's session push. Best effort; the same message is pushed once.
    pub(super) fn tell_cancelled(
        &self,
        owner_id: &str,
        schedule: &Schedule,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) {
        for session_id in schedule
            .placements
            .iter()
            .filter(|placement| placement.reason == PlacementReason::RunCancelled)
            .filter_map(|placement| placement.session_id.clone())
        {
            let _ = self.session_tell(
                owner_id,
                &session_id,
                Some((RequestKind::Cancelled, None)),
                actor,
                tx,
            );
        }
    }
}
