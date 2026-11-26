//! HS2-NPTKS8: a session the scheduler started confirms it took up its role only by
//! joining the target's coordination pool (HS2-2DPGDY). One whose role prompt failed,
//! or that has not joined within `placement.join_minutes`, is wound down at its safe
//! point through the ordinary wind-down, so its step's run has no live session and the
//! next pass places it again. The decision is the pure `decide_join`; a person's
//! session and a session started by hand are never touched.
use hotsheet_ticketing::orchestration::{
    Blocker, JoinInput, JoinState, MAX_JOIN_REPLACEMENTS, OrchestratedSession, Placement,
    PlacementAction, PlacementReason, PolicyBody, RoleBlocker, RoleUnplaced, Schedule,
    SessionOwner, SessionRole, SessionState, Unplaced, decide_join,
};

use std::collections::BTreeMap;

use super::*;

/// The detail of a kept session whose run had its replacements.
pub const REPLACEMENTS_EXHAUSTED: &str = "replacements_exhausted";

/// HS2-4JQ41V: how long a project's integrator (or, HS2-TCXKEM, dispatcher) is not
/// started again by the scheduler once its sessions of that role failed to take it up
/// until their replacements were exhausted. The project reports `integrator_unavailable`
/// or `dispatcher_unavailable` meanwhile.
pub const INTEGRATOR_UNAVAILABLE_MS: u64 = 60 * 60 * 1000;

/// The scheduler-started roles with a bounded join check, and the reason a project
/// reports while one is held back (HS2-4JQ41V integrator, HS2-TCXKEM dispatcher).
const JOIN_CHECKED_ROLES: [(SessionRole, RoleBlocker); 2] = [
    (SessionRole::Integrator, RoleBlocker::IntegratorUnavailable),
    (SessionRole::Dispatcher, RoleBlocker::DispatcherUnavailable),
];

/// The scheduler's sessions of `role` on one target, latest generation first.
fn role_sessions_of(
    records: &[OrchestratedSession],
    role: SessionRole,
) -> BTreeMap<&str, Vec<(u32, &OrchestratedSession)>> {
    let mut by_target: BTreeMap<&str, Vec<(u32, &OrchestratedSession)>> = BTreeMap::new();
    for session in records
        .iter()
        .filter(|session| session.owner == SessionOwner::Orchestrator && session.role == role)
    {
        let base = format!("sched-role-{}-{}", role.code(), session.target_id);
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

/// HS2-4JQ41V: role sessions that ended without ever joining, counted down from the
/// latest generation until one that joined or still runs. HS2-D1W59D: one lost before
/// it joined (its tool crashed at launch, or its terminal died) is a failed attempt
/// like one wound down for not joining, so a tool that always crashes at start uses up
/// the same `MAX_JOIN_REPLACEMENTS` and is never restarted without bound. One the host
/// refused to start (`Failed`) is the refusal backoff's, not counted here.
fn failed_in_a_row<'a>(sessions: impl Iterator<Item = &'a OrchestratedSession>) -> u32 {
    let failed = sessions
        .take_while(|session| {
            matches!(session.state, SessionState::Stopped | SessionState::Lost)
                && session.joined_at_ms.is_none()
        })
        .count();
    u32::try_from(failed).unwrap_or(u32::MAX)
}

/// HS2-ADASY5: the operation base of a scheduler-started worker session: its
/// `sched-start-<step>` without the generation suffix of a replacement.
fn worker_base(operation_id: &str) -> &str {
    match operation_id.rsplit_once('.') {
        Some((base, n)) if n.parse::<u32>().is_ok_and(|n| n >= 2) => base,
        _ => operation_id,
    }
}

/// The scheduler's worker sessions under `base`, latest generation first.
fn worker_generations<'a>(
    records: &'a [OrchestratedSession],
    base: &str,
) -> Vec<&'a OrchestratedSession> {
    let mut sessions: Vec<(u32, &OrchestratedSession)> = records
        .iter()
        .filter(|session| {
            session.owner == SessionOwner::Orchestrator && session.role == SessionRole::Worker
        })
        .filter_map(|session| {
            super::session_executor::generation(base, &session.operation_id).map(|n| (n, session))
        })
        .collect();
    sessions.sort_by_key(|(n, _)| std::cmp::Reverse(*n));
    sessions.into_iter().map(|(_, session)| session).collect()
}

/// The latest scheduler-started worker session under `base`.
fn latest_worker<'a>(
    records: &'a [OrchestratedSession],
    base: &str,
) -> Option<&'a OrchestratedSession> {
    worker_generations(records, base).into_iter().next()
}

/// The worker sessions under `base` that worked `run_id`, latest generation first, for
/// [`failed_in_a_row`].
fn worker_failures<'a>(
    records: &'a [OrchestratedSession],
    base: &str,
    run_id: &'a str,
) -> impl Iterator<Item = &'a OrchestratedSession> {
    worker_generations(records, base)
        .into_iter()
        .filter(move |session| session.run_id.as_deref() == Some(run_id))
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
            // HS2-ADASY5: sessions of this run that ended without ever joining, in a
            // row before this one, whether wound down or lost.
            let replaced = worker_failures(&records, worker_base(&session.operation_id), run_id)
                .skip_while(|other| other.session_id != session.session_id)
                .skip(1);
            let decided = decide_join(&JoinInput {
                joined: session.joined_at_ms.is_some(),
                role_prompt: session.role_prompt,
                running_since_ms: session
                    .history
                    .iter()
                    .find(|entry| entry.to == SessionState::Running)
                    .map_or(session.created_at_ms, |entry| entry.at_ms),
                join_ms,
                replaced: failed_in_a_row(replaced),
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
        Self::worker_exhausted(&records, &runs, schedule, now_ms);
        for (role, blocker) in JOIN_CHECKED_ROLES {
            Self::role_joining(&records, role, blocker, join_ms, schedule, now_ms);
        }
    }

    /// HS2-ADASY5: a step whose run had its replacements, every session the scheduler
    /// started for it having ended without joining (wound down for not joining, or lost
    /// before it joined: its AI tool crashed at launch), is not started again for
    /// [`INTEGRATOR_UNAVAILABLE_MS`] after the last of them ended. Its start is taken out
    /// of the pass and the step reports `never_joined`, detail `replacements_exhausted`,
    /// with that retry time. One start follows the hour; a session that joined starts
    /// the count over.
    fn worker_exhausted(
        records: &[OrchestratedSession],
        runs: &[hotsheet_ticketing::orchestration::Run],
        schedule: &mut Schedule,
        now_ms: u64,
    ) {
        let mut held: Vec<(String, String, u64)> = Vec::new();
        for placement in &schedule.placements {
            if placement.action != PlacementAction::Start
                || placement
                    .role
                    .is_some_and(|role| role != SessionRole::Worker)
            {
                continue;
            }
            for step_id in &placement.steps {
                let base = format!("sched-start-{step_id}");
                // The latest session started for the step names the run it worked.
                let Some(run_id) = latest_worker(records, &base).and_then(|latest| {
                    latest
                        .run_id
                        .as_ref()
                        .filter(|run_id| runs.iter().any(|run| &run.run_id == *run_id))
                }) else {
                    continue;
                };
                let failed: Vec<&OrchestratedSession> =
                    worker_failures(records, &base, run_id).collect();
                if failed_in_a_row(failed.iter().copied()) <= MAX_JOIN_REPLACEMENTS {
                    continue;
                }
                let until = failed[0]
                    .updated_at_ms
                    .saturating_add(INTEGRATOR_UNAVAILABLE_MS);
                if until > now_ms {
                    held.push((step_id.clone(), placement.target_id.clone(), until));
                }
            }
        }
        for (step_id, target_id, until) in held {
            for placement in &mut schedule.placements {
                if placement.action == PlacementAction::Start {
                    placement.steps.retain(|step| step != &step_id);
                }
            }
            schedule.placements.retain(|placement| {
                placement.action != PlacementAction::Start || !placement.steps.is_empty()
            });
            let score = schedule
                .ranking
                .iter()
                .find(|ranked| ranked.step_id == step_id)
                .map(|ranked| ranked.score)
                .unwrap_or_default();
            schedule.unplaced.retain(|item| item.step_id != step_id);
            schedule.unplaced.push(Unplaced {
                step_id,
                target_id,
                reason: Blocker::NeverJoined,
                detail: Some(REPLACEMENTS_EXHAUSTED.into()),
                retry_at_ms: Some(until),
                score,
                sites: Vec::new(),
            });
            schedule.retry_at_ms =
                Some(schedule.retry_at_ms.map_or(until, |known| known.min(until)));
        }
    }

    /// HS2-4JQ41V, HS2-TCXKEM: a scheduler-started integrator or dispatcher whose role
    /// prompt failed, or that did not join its pool in time, is wound down and replaced by
    /// a new generation once the target's cooldown allows. When replacements are exhausted
    /// the last one is wound down as well, never kept in its slot, and the project reports
    /// `blocker` (`integrator_unavailable` or `dispatcher_unavailable`) until
    /// [`INTEGRATOR_UNAVAILABLE_MS`] after it stopped, starting none of that role then.
    fn role_joining(
        records: &[OrchestratedSession],
        role: SessionRole,
        blocker: RoleBlocker,
        join_ms: u64,
        schedule: &mut Schedule,
        now_ms: u64,
    ) {
        for (target_id, sessions) in role_sessions_of(records, role) {
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
                        role.code().into()
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
            // No session of this role starts there in this pass; the project says why.
            schedule.placements.retain(|placement| {
                !(placement.action == PlacementAction::Start
                    && placement.role == Some(role)
                    && placement.target_id == target_id)
            });
            schedule
                .roles_unplaced
                .retain(|entry| !(entry.target_id == target_id && entry.role == role));
            schedule.roles_unplaced.push(RoleUnplaced {
                target_id: target_id.to_owned(),
                role,
                reason: blocker,
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
