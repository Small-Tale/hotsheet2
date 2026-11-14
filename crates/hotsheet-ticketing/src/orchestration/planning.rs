//! HS2-HCW7MD: Propose Plan. A planning AI session in the orchestration project reads
//! the open goal tickets and proposes the dry-run plan with the existing ticket and
//! readiness actions. This module decides, from this machine's hosted records alone,
//! whether a press starts a planner or answers with the one already there, and what
//! state planning is in. It is pure: the facts come from the hosted session registry
//! and the terminal host's listing.

use serde::{Deserialize, Serialize};

/// What this machine knows about one planner session of an orchestration project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannerFacts {
    pub operation_id: String,
    /// The terminal was launched. A reservation that never launched may be retried.
    pub started: bool,
    /// The session ended (stopped, lost or forgotten by its host).
    pub ended: bool,
    /// What the terminal host says now: `None` when it could not be asked.
    pub alive: Option<bool>,
    pub created_at_ms: u64,
}

impl PlannerFacts {
    /// The session may still be working: launched, not ended, and not known gone. A
    /// host that cannot be asked never reads as "gone", so a planner is not doubled
    /// while its host is unreachable.
    pub fn live(&self) -> bool {
        self.started && !self.ended && self.alive != Some(false)
    }

    /// Reserved and never launched: the same operation can be repeated.
    pub fn pending(&self) -> bool {
        !self.started && !self.ended
    }
}

/// Where planning stands for an orchestration project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanningState {
    /// No planner was ever started here.
    Idle,
    /// A planner is being started or runs now.
    Planning,
    /// The latest planner ended; its plan stands until the next proposal.
    Ended,
}

impl PlanningState {
    pub const ALL: [Self; 3] = [Self::Idle, Self::Planning, Self::Ended];

    pub fn code(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Planning => "planning",
            Self::Ended => "ended",
        }
    }
}

/// What a press of Propose Plan does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProposeDecision {
    /// Start a new planner under this operation id.
    Start(String),
    /// Answer with, or repeat the start of, the planner this operation id names. No
    /// second planner is launched.
    Reuse(String),
}

/// The newest planner first; equal times by operation id, so the order is total.
fn newest(planners: &[PlannerFacts]) -> Vec<&PlannerFacts> {
    let mut sorted: Vec<&PlannerFacts> = planners.iter().collect();
    sorted.sort_by(|a, b| {
        b.created_at_ms
            .cmp(&a.created_at_ms)
            .then_with(|| b.operation_id.cmp(&a.operation_id))
    });
    sorted
}

/// Where planning stands, from every planner this machine hosts for the project.
pub fn planning_state(planners: &[PlannerFacts]) -> PlanningState {
    if planners
        .iter()
        .any(|planner| planner.live() || planner.pending())
    {
        return PlanningState::Planning;
    }
    if planners.is_empty() {
        PlanningState::Idle
    } else {
        PlanningState::Ended
    }
}

/// The planner a status read shows: the live or pending one, else the newest.
pub fn current_planner(planners: &[PlannerFacts]) -> Option<&PlannerFacts> {
    let sorted = newest(planners);
    sorted
        .iter()
        .find(|planner| planner.live() || planner.pending())
        .or_else(|| sorted.first())
        .copied()
}

/// Decide a press of Propose Plan. One planner per orchestration project at a time:
///
/// 1. An operation id the caller retained and this machine knows is that planner,
///    whatever became of it (a repeated request never launches twice).
/// 2. Otherwise a live planner, or one reserved and never launched, answers the press.
/// 3. Otherwise a new planner starts under the caller's id, or under `fresh`.
///
/// Re-proposing after a planner ended starts a new one; the plan itself is idempotent
/// by goal ticket and target ticket, so the new planner updates what is there.
pub fn decide_propose(
    planners: &[PlannerFacts],
    requested: Option<&str>,
    fresh: impl FnOnce() -> String,
) -> ProposeDecision {
    if let Some(requested) = requested
        && planners
            .iter()
            .any(|planner| planner.operation_id == requested)
    {
        return ProposeDecision::Reuse(requested.to_owned());
    }
    if let Some(current) = newest(planners)
        .into_iter()
        .find(|planner| planner.live() || planner.pending())
    {
        return ProposeDecision::Reuse(current.operation_id.clone());
    }
    ProposeDecision::Start(requested.map_or_else(fresh, str::to_owned))
}
