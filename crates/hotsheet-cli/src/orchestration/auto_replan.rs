//! HS2-BQN0M5: re-plan the default goal automatically when a target gets a new ticket.
//!
//! While the standing default goal (HS2-YSRT6Y) is the only open goal and the owner's
//! orchestration runs (its placement is on), the target follower (HS2-7QHWHJ,
//! HS2-TMA4YA) follows every project target. A ticket event that names an open ticket
//! no unfinished step names, and no run works, becomes *pending*: it starts one
//! background Update Plan (HS2-190RDX), audited as `plan_update_auto`. Everything is
//! driven by the follower's own steps (each one a bounded long-poll answer), never by a
//! timer:
//!
//! - **Coalesced.** Every pending ticket of a page, and of later pages while an update
//!   drafts, goes into one update. A pending ticket is settled once a step names it, or
//!   once an update newer than the latest one when it was seen started, because that
//!   planner saw it.
//! - **Respectful.** A drafting update, or a planner still at work on another proposal,
//!   defers the update to a later step; it starts once the planner is free.
//! - **Once per ticket.** A ticket handed to a re-plan does not start another one while
//!   it stays open, even when the planner chose to leave it out. Closing it forgets it,
//!   so a reopened ticket counts again.
//! - **Unknown history starts nothing.** A handshake, an overflow or a reconnect cannot
//!   say which tickets are new, so it hands none to a re-plan.
use std::collections::{BTreeMap, BTreeSet};

use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::{
    DeclarationState, PlanningState, RunTarget, TargetOpError, current_planner, planning_state,
};
use serde_json::Value;

use super::plan_update::UpdatePlan;
use super::*;

/// The audit action of an Update Plan this module starts.
pub const AUTO_REPLAN_ACTION: &str = "plan_update_auto";
/// The most tickets one follower remembers as handed to a re-plan.
const MAX_HANDED: usize = 4_096;

/// A target ticket, by connection and canonical id.
type Key = (String, String);

/// What one follower remembers between steps (in process only).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ReplanMemory {
    /// Open tickets no step named that were handed to a re-plan.
    handed: BTreeSet<Key>,
    /// Tickets waiting for an update, with the latest plan update when each was seen.
    pending: BTreeMap<Key, Option<String>>,
}

impl ReplanMemory {
    /// A read of a ticket no step names. An open one not handed yet becomes pending
    /// (true); a closed or missing one is forgotten, so reopening it counts again.
    pub fn observe(&mut self, key: Key, open: bool, latest: Option<&str>) -> bool {
        if !open {
            self.handed.remove(&key);
            self.pending.remove(&key);
            return false;
        }
        if self.handed.contains(&key) {
            return false;
        }
        if self.handed.len() >= MAX_HANDED {
            self.handed.pop_first();
        }
        self.handed.insert(key.clone());
        self.pending.insert(key, latest.map(Into::into));
        true
    }

    /// Settle what no longer needs an update: tickets a step names now, and tickets an
    /// update newer than the one they recorded covered.
    pub fn settle(&mut self, planned: impl Fn(&Key) -> bool, latest: Option<&str>) {
        self.pending
            .retain(|key, after| !planned(key) && after.as_deref() == latest);
    }

    /// An update was asked for every pending ticket.
    pub fn asked(&mut self) {
        self.pending.clear();
    }

    pub fn pending(&self) -> usize {
        self.pending.len()
    }
}

/// What a follower step did about re-planning.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum AutoReplan {
    /// Nothing pending.
    #[default]
    Idle,
    /// Pending tickets wait for a drafting update or a planner at work.
    Deferred,
    /// An automatic Update Plan started (or the planner was given the update).
    Started,
    /// An automatic Update Plan was asked for and refused; the audit has the reason.
    Failed,
}

/// Whether a target ticket's `status` is open (not finished, archived, deleted or
/// moved), as `ops::is_open` reads a ticket.
fn status_open(ticket: &Value) -> bool {
    !matches!(
        ticket.get("status").and_then(Value::as_str),
        Some("completed" | "verified" | "deleted" | "archive" | "moved")
    )
}

impl OrchestrationRuntime {
    /// HS2-BQN0M5: the actor an automatic re-plan acts as, while it applies: the owner
    /// orchestrates, its placement is on (the scheduler's actor), and the open goals
    /// are only the standing default goal.
    pub(super) fn replan_actor(&self, owner_id: &str) -> Option<MutationActor> {
        let actor = self.scheduler_actor(owner_id).ok().flatten()?;
        let owner = self.owner(owner_id).ok()?;
        let (goals, _) = self.plan_goals(&owner, None).ok()?;
        Self::default_goal_slug(&goals)?;
        Some(actor)
    }

    /// HS2-BQN0M5: the project targets a server follows so that a new ticket there
    /// re-plans the default goal. Empty while an automatic re-plan does not apply.
    pub fn replan_targets(&self, owner_id: &str) -> Vec<String> {
        if self.replan_actor(owner_id).is_none() {
            return Vec::new();
        }
        self.declarations(owner_id)
            .map(|declared| {
                declared
                    .targets
                    .into_iter()
                    .filter(|target| {
                        target.state == DeclarationState::Active && target.site_of.is_none()
                    })
                    .map(|target| target.target_id)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The latest plan update's id and whether it still drafts, as `plan show` reads
    /// it (a drafting update whose planner ended is failed on the way).
    fn replan_latest(
        &self,
        owner: &Checkout,
        tx: &mut Transaction<'_>,
    ) -> Result<Option<(String, bool)>, OrchestrationError> {
        Ok(self
            .plan_update_status(owner, tx)?
            .map(|view| (view.update_id, view.state.is_open())))
    }

    /// HS2-BQN0M5: one follower step's part in re-planning. `events` are the page's
    /// ticket events (none after a handshake, an overflow or a reconnect), `named` the
    /// target tickets a step names or a run works now, `actor` the one
    /// [`Self::replan_actor`] answered for this step.
    pub(super) fn replan_step(
        &self,
        (owner_id, target_id): (&str, &str),
        events: &[super::TargetTicketEvent],
        named: &[RunTarget],
        (authorized, actor): (&Authorized, &MutationActor),
        memory: &mut ReplanMemory,
        tx: &mut Transaction<'_>,
    ) -> AutoReplan {
        let Ok(owner) = self.owner(owner_id) else {
            return AutoReplan::Idle;
        };
        let is_named = |(connection, native): &Key| {
            named
                .iter()
                .any(|ticket| &ticket.connection_id == connection && &ticket.native_id == native)
        };
        let mut candidates: Vec<Key> = Vec::new();
        for event in events {
            let key = (event.connection_id.clone(), event.native_id.clone());
            // A store reconciliation names no ticket; it cannot say what is new.
            if event.native_id.is_empty() || is_named(&key) || candidates.contains(&key) {
                continue;
            }
            candidates.push(key);
        }
        if !candidates.is_empty() {
            let latest = self
                .plan_updates(owner_id)
                .and_then(|store| store.latest())
                .ok()
                .flatten()
                .map(|update| update.update_id);
            let mut failed = None;
            for (connection_id, native_id) in candidates {
                let target = TargetTicket {
                    target_id: target_id.into(),
                    connection_id: connection_id.clone(),
                    native_id: native_id.clone(),
                };
                let open = match self.read_authorized(authorized, &target, tx) {
                    Ok(ticket) => status_open(&ticket),
                    // The target answered: it has no such ticket.
                    Err(OrchestrationError::TargetOperation(TargetOpError::NotFound)) => false,
                    // Opportunistic, as the follower's other reads: nothing changes.
                    Err(error) => {
                        failed = Some(error.code());
                        continue;
                    }
                };
                memory.observe((connection_id, native_id), open, latest.as_deref());
            }
            if let Ok(reference) = Self::target_ref(owner_id, target_id) {
                let _ = self.record_outcome(&reference, authorized, failed.is_none(), failed, tx);
            }
        }
        if memory.pending() == 0 {
            return AutoReplan::Idle;
        }
        let Ok(latest) = self.replan_latest(&owner, tx) else {
            return AutoReplan::Deferred;
        };
        memory.settle(is_named, latest.as_ref().map(|(id, _)| id.as_str()));
        if memory.pending() == 0 {
            return AutoReplan::Idle;
        }
        // One update at a time: a drafting one is not interrupted, and a planner still
        // at work on another proposal is not given this one.
        if latest.is_some_and(|(_, drafting)| drafting) {
            return AutoReplan::Deferred;
        }
        let Ok(planners) = self.planners(&owner, tx) else {
            return AutoReplan::Deferred;
        };
        let facts = Self::planner_facts(&planners);
        if planning_state(&facts) == PlanningState::Planning
            && !current_planner(&facts).is_some_and(|planner| planner.replannable())
        {
            return AutoReplan::Deferred;
        }
        // The newest planner's tool, model and effort; without one nothing is known to
        // run a planner in, and the next press of Update Plan names it.
        let Some(newest) = planners
            .iter()
            .max_by_key(|view| view.session.created_at_ms)
        else {
            return AutoReplan::Deferred;
        };
        let request = UpdatePlan {
            tool: newest.session.tool.clone(),
            model: newest.session.model.clone(),
            effort: newest.session.effort.clone(),
            operation_id: None,
            automatic: true,
        };
        memory.asked();
        match self.plan_update(owner_id, request, Some(actor), tx) {
            Ok(_) => AutoReplan::Started,
            Err(_) => AutoReplan::Failed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(native: &str) -> Key {
        ("conn".into(), native.into())
    }

    /// HS2-BQN0M5: the memory's transitions: new, repeated, closed then reopened,
    /// covered by a newer update, named by a step, and asked.
    #[test]
    fn hs2_bqn0m5_replan_memory_transitions() {
        let mut memory = ReplanMemory::default();
        assert!(memory.observe(key("a"), true, None));
        assert!(!memory.observe(key("a"), true, None), "handed once");
        assert!(memory.observe(key("b"), true, None));
        assert_eq!(memory.pending(), 2);
        // A step names `a` now; `b` still waits on the same latest update.
        memory.settle(|candidate| candidate == &key("a"), None);
        assert_eq!(memory.pending(), 1);
        // A newer update started after `b` was seen: it is covered.
        memory.settle(|_| false, Some("u1"));
        assert_eq!(memory.pending(), 0);
        // `b` closes and reopens: it counts again, recording the current update.
        assert!(!memory.observe(key("b"), false, Some("u1")));
        assert!(memory.observe(key("b"), true, Some("u1")));
        memory.settle(|_| false, Some("u1"));
        assert_eq!(memory.pending(), 1, "the same update did not cover it");
        memory.asked();
        assert_eq!(memory.pending(), 0);
        assert!(!memory.observe(key("b"), true, Some("u2")), "still handed");
        // A closed ticket never pending.
        assert!(!memory.observe(key("c"), false, None));
        assert_eq!(memory.pending(), 0);
    }

    #[test]
    fn hs2_bqn0m5_handed_tickets_are_bounded() {
        let mut memory = ReplanMemory::default();
        for n in 0..=MAX_HANDED {
            memory.observe(key(&format!("{n:05}")), true, None);
        }
        assert_eq!(memory.handed.len(), MAX_HANDED);
    }

    #[test]
    fn hs2_bqn0m5_open_statuses() {
        for (status, open) in [
            ("not_started", true),
            ("started", true),
            ("backlog", true),
            ("completed", false),
            ("verified", false),
            ("archive", false),
            ("deleted", false),
            ("moved", false),
        ] {
            assert_eq!(
                status_open(&serde_json::json!({ "status": status })),
                open,
                "{status}"
            );
        }
    }
}
