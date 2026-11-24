//! HS2-NPTKS8: a session the scheduler started confirms it took up its role only by
//! joining the target's coordination pool (HS2-2DPGDY). One whose role prompt failed,
//! or that has not joined within `placement.join_minutes`, is wound down at its safe
//! point through the ordinary wind-down, so its step's run has no live session and the
//! next pass places it again. The decision is the pure `decide_join`; a person's
//! session and a session started by hand are never touched.
use hotsheet_ticketing::orchestration::{
    JoinInput, JoinState, MAX_JOIN_REPLACEMENTS, OrchestratedSession, Placement, PlacementAction,
    PlacementReason, PolicyBody, RoleBlocker, RoleUnplaced, Schedule, SessionOwner, SessionRole,
    SessionState, decide_join,
};

use std::collections::BTreeMap;

use super::*;

/// The detail of a kept session whose run had its replacements.
pub const REPLACEMENTS_EXHAUSTED: &str = "replacements_exhausted";

/// HS2-4JQ41V: how long a project's integrator is not started again by the scheduler
/// once its integrators failed to take up the role until their replacements were
/// exhausted. The project reports `integrator_unavailable` meanwhile.
pub const INTEGRATOR_UNAVAILABLE_MS: u64 = 60 * 60 * 1000;

/// HS2-4JQ41V: the scheduler's integrators of one target, latest generation first.
fn integrators_of(
    records: &[OrchestratedSession],
) -> BTreeMap<&str, Vec<(u32, &OrchestratedSession)>> {
    let mut by_target: BTreeMap<&str, Vec<(u32, &OrchestratedSession)>> = BTreeMap::new();
    for session in records.iter().filter(|session| {
        session.owner == SessionOwner::Orchestrator && session.role == SessionRole::Integrator
    }) {
        let base = format!("sched-role-integrator-{}", session.target_id);
        if let Some(n) = super::session_executor::generation(&base, &session.operation_id) {
            by_target
                .entry(session.target_id.as_str())
                .or_default()
                .push((n, session));
        }
    }
    for sessions in by_target.values_mut() {
        sessions.sort_by_key(|(n, _)| std::cmp::Reverse(*n));
    }
    by_target
}

/// HS2-4JQ41V: stopped integrators that never joined, counted down from the latest
/// generation until one that joined or still runs.
fn failed_in_a_row<'a>(sessions: impl Iterator<Item = &'a OrchestratedSession>) -> u32 {
    let failed = sessions
        .take_while(|session| {
            session.state == SessionState::Stopped && session.joined_at_ms.is_none()
        })
        .count();
    u32::try_from(failed).unwrap_or(u32::MAX)
}

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
        Self::integrator_joining(&records, join_ms, schedule, now_ms);
    }

    /// HS2-4JQ41V: a scheduler-started integrator whose role prompt failed, or that
    /// did not join its pool in time, is wound down and replaced by a new generation
    /// once the target's cooldown allows. When replacements are exhausted the last one
    /// is wound down as well, never kept in its slot, and the project reports
    /// `integrator_unavailable` until [`INTEGRATOR_UNAVAILABLE_MS`] after it stopped,
    /// starting none in that time.
    fn integrator_joining(
        records: &[OrchestratedSession],
        join_ms: u64,
        schedule: &mut Schedule,
        now_ms: u64,
    ) {
        for (target_id, sessions) in integrators_of(records) {
            let Some(&(_, latest)) = sessions.first() else {
                continue;
            };
            let unavailable_until = if latest.state == SessionState::Running {
                let decided = decide_join(&JoinInput {
                    joined: latest.joined_at_ms.is_some(),
                    role_prompt: latest.role_prompt,
                    running_since_ms: latest
                        .history
                        .iter()
                        .find(|entry| entry.to == SessionState::Running)
                        .map_or(latest.created_at_ms, |entry| entry.at_ms),
                    join_ms,
                    replaced: failed_in_a_row(sessions.iter().skip(1).map(|(_, s)| *s)),
                    now_ms,
                });
                if let Some(at) = decided.retry_at_ms {
                    schedule.retry_at_ms =
                        Some(schedule.retry_at_ms.map_or(at, |known| known.min(at)));
                }
                let Some(placement) = schedule.placements.iter_mut().find(|placement| {
                    placement.session_id.as_deref() == Some(latest.session_id.as_str())
                }) else {
                    continue;
                };
                if placement.action == PlacementAction::WindDown
                    || !(decided.wind_down || decided.exhausted)
                {
                    continue;
                }
                *placement = Placement {
                    action: PlacementAction::WindDown,
                    reason: if decided.state == JoinState::RolePromptFailed {
                        PlacementReason::RolePromptFailed
                    } else {
                        PlacementReason::NeverJoined
                    },
                    detail: Some(if decided.exhausted {
                        REPLACEMENTS_EXHAUSTED.into()
                    } else {
                        SessionRole::Integrator.code().into()
                    }),
                    steps: Vec::new(),
                    score: None,
                    retry_at_ms: None,
                    provisional: false,
                    sites: Vec::new(),
                    ..placement.clone()
                };
                if !decided.exhausted {
                    continue;
                }
                now_ms.saturating_add(INTEGRATOR_UNAVAILABLE_MS)
            } else if latest.state.is_terminal()
                && failed_in_a_row(sessions.iter().map(|(_, s)| *s)) > MAX_JOIN_REPLACEMENTS
            {
                let until = latest
                    .updated_at_ms
                    .saturating_add(INTEGRATOR_UNAVAILABLE_MS);
                if until <= now_ms {
                    continue;
                }
                until
            } else {
                continue;
            };
            // No integrator starts there in this pass; the project says why.
            schedule.placements.retain(|placement| {
                !(placement.action == PlacementAction::Start
                    && placement.role == Some(SessionRole::Integrator)
                    && placement.target_id == target_id)
            });
            schedule.roles_unplaced.retain(|entry| {
                !(entry.target_id == target_id && entry.role == SessionRole::Integrator)
            });
            schedule.roles_unplaced.push(RoleUnplaced {
                target_id: target_id.to_owned(),
                role: SessionRole::Integrator,
                reason: RoleBlocker::IntegratorUnavailable,
                detail: Some(REPLACEMENTS_EXHAUSTED.into()),
                retry_at_ms: Some(unavailable_until),
            });
            schedule.retry_at_ms = Some(
                schedule
                    .retry_at_ms
                    .map_or(unavailable_until, |known| known.min(unavailable_until)),
            );
        }
    }
}
