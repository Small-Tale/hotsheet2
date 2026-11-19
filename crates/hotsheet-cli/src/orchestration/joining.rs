//! HS2-NPTKS8: a session the scheduler started confirms it took up its role only by
//! joining the target's coordination pool (HS2-2DPGDY). One whose role prompt failed,
//! or that has not joined within `placement.join_minutes`, is wound down at its safe
//! point through the ordinary wind-down, so its step's run has no live session and the
//! next pass places it again. The decision is the pure `decide_join`; a person's
//! session and a session started by hand are never touched.
use hotsheet_ticketing::orchestration::{
    JoinInput, Placement, PlacementAction, PlacementReason, PolicyBody, Schedule, SessionOwner,
    SessionRole, SessionState, decide_join,
};

use super::*;

/// The detail of a kept session whose run had its replacements.
pub const REPLACEMENTS_EXHAUSTED: &str = "replacements_exhausted";

impl OrchestrationRuntime {
    /// Rewrite a computed schedule for the sessions that have not joined their role:
    /// each one to act on gets a `wind_down` (reason `never_joined` or
    /// `role_prompt_failed`) in place of its own placement, one whose run had its
    /// replacements is kept with detail `replacements_exhausted`, and the end of each
    /// waiting session's time to join becomes a retry time. Reads this machine's
    /// records only; what a pass applies passes the gate and the host's safe point
    /// like any wind-down.
    pub(super) fn scheduler_joining(
        &self,
        owner_id: &str,
        policy: &PolicyBody,
        schedule: &mut Schedule,
        now_ms: u64,
    ) {
        let (Ok(records), Ok(runs)) = (
            self.session_list(owner_id, false),
            self.run_list(owner_id, true),
        ) else {
            return;
        };
        let join_ms = u64::from(policy.placement.join_minutes) * 60 * 1000;
        for session in records.iter().filter(|session| {
            session.state == SessionState::Running
                && session.owner == SessionOwner::Orchestrator
                && session.role == SessionRole::Worker
                && session.operation_id.starts_with("sched-start-")
        }) {
            // Only a session still working an unfinished run holds work back; one
            // whose run finished is the idle wind-down's.
            let Some(run_id) = session
                .run_id
                .as_ref()
                .filter(|run_id| runs.iter().any(|run| &run.run_id == *run_id))
            else {
                continue;
            };
            // Sessions of this run already stopped without ever joining.
            let replaced = records
                .iter()
                .filter(|other| {
                    other.session_id != session.session_id
                        && other.run_id.as_ref() == Some(run_id)
                        && other.state == SessionState::Stopped
                        && other.joined_at_ms.is_none()
                })
                .count();
            let decided = decide_join(&JoinInput {
                joined: session.joined_at_ms.is_some(),
                role_prompt: session.role_prompt,
                running_since_ms: session
                    .history
                    .iter()
                    .find(|entry| entry.to == SessionState::Running)
                    .map_or(session.created_at_ms, |entry| entry.at_ms),
                join_ms,
                replaced: u32::try_from(replaced).unwrap_or(u32::MAX),
                now_ms,
            });
            if let Some(at) = decided.retry_at_ms {
                schedule.retry_at_ms = Some(schedule.retry_at_ms.map_or(at, |known| known.min(at)));
            }
            let Some(placement) = schedule.placements.iter_mut().find(|placement| {
                placement.session_id.as_deref() == Some(session.session_id.as_str())
            }) else {
                continue;
            };
            // A wind-down the plan already decided stands.
            if placement.action == PlacementAction::WindDown {
                continue;
            }
            let reason = if decided.state
                == hotsheet_ticketing::orchestration::JoinState::RolePromptFailed
            {
                PlacementReason::RolePromptFailed
            } else {
                PlacementReason::NeverJoined
            };
            if decided.wind_down {
                *placement = Placement {
                    action: PlacementAction::WindDown,
                    reason,
                    detail: None,
                    steps: Vec::new(),
                    score: None,
                    retry_at_ms: None,
                    provisional: false,
                    sites: Vec::new(),
                    ..placement.clone()
                };
            } else if decided.exhausted {
                placement.reason = reason;
                placement.detail = Some(REPLACEMENTS_EXHAUSTED.into());
            }
        }
    }
}
