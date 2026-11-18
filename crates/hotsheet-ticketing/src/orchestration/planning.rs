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
    /// HS2-FDS95D: the later proposals pushed into this planner after it finished a
    /// turn, oldest first. Each is an operation id a press retained.
    pub proposals: Vec<String>,
    /// HS2-FDS95D: the planner finished its turn: it runs, its prompt reached it, its
    /// tool is past any first-run question and the host says it is not busy. Only then
    /// does a press give it a new proposal.
    pub finished_turn: bool,
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

    /// HS2-FDS95D: this planner answers for `operation_id`: its own start, or a later
    /// proposal pushed into it.
    pub fn answers_for(&self, operation_id: &str) -> bool {
        self.operation_id == operation_id
            || self
                .proposals
                .iter()
                .any(|proposal| proposal == operation_id)
    }

    /// HS2-FDS95D: the proposal this planner works on now: its latest pushed proposal,
    /// else its own start.
    pub fn latest_proposal(&self) -> &str {
        self.proposals
            .last()
            .map_or(self.operation_id.as_str(), String::as_str)
    }

    /// HS2-FDS95D: a press may give it a new proposal.
    pub fn replannable(&self) -> bool {
        self.live() && self.finished_turn
    }
}

/// HS2-FDS95D: how many later proposals one planner keeps. The oldest is forgotten
/// first; a press repeating a forgotten id is answered as a new press.
pub const MAX_PLANNER_PROPOSALS: usize = 64;

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
    /// HS2-FDS95D: push a new proposal, under `proposal`, into the planner `planner`
    /// names, which finished its turn. No second planner is launched.
    Replan { planner: String, proposal: String },
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
///    whatever became of it: its start or a proposal pushed into it (a repeated request
///    never launches or pushes twice).
/// 2. Otherwise a live planner that finished its turn is given a new proposal under the
///    caller's id, or under `fresh` (HS2-FDS95D).
/// 3. Otherwise a live planner still working, or one reserved and never launched,
///    answers the press.
/// 4. Otherwise a new planner starts under the caller's id, or under `fresh`.
///
/// Re-proposing after a planner ended starts a new one; the plan itself is idempotent
/// by goal ticket and target ticket, so the new planner updates what is there.
pub fn decide_propose(
    planners: &[PlannerFacts],
    requested: Option<&str>,
    fresh: impl FnOnce() -> String,
) -> ProposeDecision {
    if let Some(requested) = requested
        && let Some(known) = planners
            .iter()
            .find(|planner| planner.answers_for(requested))
    {
        return ProposeDecision::Reuse(known.operation_id.clone());
    }
    if let Some(current) = newest(planners)
        .into_iter()
        .find(|planner| planner.live() || planner.pending())
    {
        if current.replannable() {
            return ProposeDecision::Replan {
                planner: current.operation_id.clone(),
                proposal: requested.map_or_else(fresh, str::to_owned),
            };
        }
        return ProposeDecision::Reuse(current.operation_id.clone());
    }
    ProposeDecision::Start(requested.map_or_else(fresh, str::to_owned))
}
