//! The milestone roll-up (HS2-A820C1, docs/38 §38.4): progress, risk and clarity signals,
//! and one health word, derived from a milestone's tickets when read. Never stored.
//!
//! [`milestone_rollup`] is pure: it takes the milestone, its member rows, the time, the
//! project time zone and the thresholds, so the CLI, MCP, the server and the orchestrator
//! compute the same answer. Callers gather members across every source the reading
//! checkout links ([`crate::milestone_project`]).

use std::collections::HashSet;

use jiff::tz::TimeZone;
use serde::Serialize;

use crate::milestones::{CloseReason, Milestone, MilestoneState};
use crate::project_time_zone::{deadline_instant_ms, is_due_within, is_overdue};
use crate::wire::TicketRow;

/// Signal thresholds (docs/38 §38.4). v1 uses the documented defaults; making them project
/// settings is HS2-G07KZX.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RollupThresholds {
    /// A started member with no live claim and no update for this long is `stale`.
    pub stale_hours: u32,
    /// A finished member whose latest confidence is below this is `low_confidence`.
    pub confidence_floor: u8,
    /// `at_risk` when due within this many days ...
    pub due_soon_days: u32,
    /// ... with less than this percent finished.
    pub due_soon_percent: u8,
}

impl Default for RollupThresholds {
    fn default() -> Self {
        Self {
            stale_hours: 48,
            confidence_floor: 70,
            due_soon_days: 7,
            due_soon_percent: 80,
        }
    }
}

/// One word for how a milestone is doing, derived, never set by hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Health {
    Done,
    Dropped,
    OffTrack,
    AtRisk,
    Unclear,
    OnTrack,
}

/// Member counts by bucket. `members` excludes `dropped`; `awaiting_ci` is the part of
/// `started` in `final_testing`. `percent` is `finished / members`, floored, absent with
/// no members ("No tickets").
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Progress {
    pub members: usize,
    pub not_started: usize,
    pub started: usize,
    pub awaiting_ci: usize,
    pub finished: usize,
    pub dropped: usize,
    /// Sources that could not be read; their members are not counted (named, never
    /// silently dropped).
    pub unavailable: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percent: Option<u8>,
}

/// The ticket a signal or list names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MemberRef {
    pub connection_id: String,
    pub id: String,
    pub slug: String,
    pub title: String,
}

impl MemberRef {
    fn of(row: &TicketRow) -> Self {
        Self {
            connection_id: row.connection_id.clone(),
            id: row.id.clone(),
            slug: row.slug.clone(),
            title: row.title.clone(),
        }
    }
}

/// One risk or clarity signal on a member.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Signal {
    pub ticket: MemberRef,
    /// `blocked`, `stale`, `low_confidence`, `red_gate`, `overdue` (risk) or
    /// `feedback_needed`, `no_details` (clarity).
    pub code: &'static str,
    pub detail: String,
}

/// Everything derived for one milestone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MilestoneRollup {
    pub progress: Progress,
    pub risk: Vec<Signal>,
    pub clarity: Vec<Signal>,
    pub health: Health,
    /// The deadline instant of the milestone's own due date in the project zone, Unix ms.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due_at_ms: Option<i64>,
    /// The milestone's own date has passed.
    pub overdue: bool,
    /// Members not yet finished (and not dropped), for Close Milestone… and Move Open
    /// Tickets to….
    pub open_members: Vec<MemberRef>,
}

/// What the roll-up reads besides the members.
#[derive(Debug, Clone, Copy)]
pub struct RollupContext<'a> {
    /// Now, Unix milliseconds.
    pub now_ms: i64,
    /// The project time zone (docs/38 §38.3).
    pub zone: &'a TimeZone,
    pub thresholds: RollupThresholds,
    /// Ids of tickets in the reading project that are not finished, so a member whose
    /// `blocked_by` names one is `blocked`.
    pub unfinished_ids: &'a HashSet<String>,
    /// Ids of members whose latest orchestration run's gates rolled up `failed`
    /// (docs/26 §26.13). Fed by HS2-DXZRHA; empty until then.
    pub red_gates: &'a HashSet<String>,
}

/// How one member counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Bucket {
    /// A `moved` tombstone: its destination counts instead.
    Skip,
    Dropped,
    NotStarted,
    Started {
        awaiting_ci: bool,
    },
    Finished,
}

fn bucket(row: &TicketRow) -> Bucket {
    let dropped_reason = matches!(
        row.close_reason.as_deref(),
        Some("not_planned" | "duplicate" | "obsolete" | "works_as_designed")
    );
    match row.status.as_deref() {
        Some("moved") => Bucket::Skip,
        Some("deleted") => Bucket::Dropped,
        _ if dropped_reason => Bucket::Dropped,
        Some("completed" | "verified" | "archive") => Bucket::Finished,
        Some("started") => Bucket::Started {
            awaiting_ci: row.started_phase.as_deref() == Some("final_testing"),
        },
        _ => Bucket::NotStarted,
    }
}

/// The deadline instant of a `YYYY-MM-DD` date in `zone`; `None` for anything else.
pub fn due_deadline_ms(due_on: Option<&str>, zone: &TimeZone) -> Option<i64> {
    let date = due_on.and_then(|text| crate::project_time_zone::parse_due_on(text).ok())?;
    Some(deadline_instant_ms(date, zone))
}

/// Whether a milestone in this state still lends its date to its tickets' effective due
/// (docs/38 §38.3: "while that milestone is open"). A conflicted file is still open.
pub fn lends_due_date(milestone: &Milestone) -> bool {
    matches!(
        milestone.state,
        MilestoneState::Open | MilestoneState::Conflicted
    )
}

/// A ticket's effective due date: the earlier of its own `due_on` and its milestone's while
/// that milestone is open (docs/38 §38.3). Dates that are not `YYYY-MM-DD` are ignored.
pub fn effective_due_on<'a>(
    own: Option<&'a str>,
    milestone: Option<&'a Milestone>,
) -> Option<&'a str> {
    let valid = |date: Option<&'a str>| {
        date.filter(|text| crate::project_time_zone::parse_due_on(text).is_ok())
    };
    let own = valid(own);
    let lent = valid(
        milestone
            .filter(|milestone| lends_due_date(milestone))
            .and_then(|milestone| milestone.due_on.as_deref()),
    );
    match (own, lent) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

fn live_claim(row: &TicketRow, now_ms: i64) -> bool {
    row.claimed_by.is_some()
        && row
            .claim_lease_expires_at
            .as_deref()
            .and_then(rfc3339_ms)
            .is_none_or(|expires| expires > now_ms)
}

fn rfc3339_ms(text: &str) -> Option<i64> {
    let at = text.parse::<jiff::Timestamp>().ok()?;
    Some(at.as_millisecond())
}

/// Compute the roll-up of `milestone` over `members` (every ticket naming its id, in any
/// status; `unavailable` names sources that could not be read).
pub fn milestone_rollup(
    milestone: &Milestone,
    members: &[TicketRow],
    unavailable: &[String],
    context: &RollupContext<'_>,
) -> MilestoneRollup {
    let now = context.now_ms;
    let thresholds = context.thresholds;
    let mut progress = Progress {
        unavailable: unavailable.to_vec(),
        ..Progress::default()
    };
    let mut risk = Vec::new();
    let mut clarity = Vec::new();
    let mut open_members = Vec::new();
    let mut ordered: Vec<&TicketRow> = members.iter().collect();
    ordered.sort_by(|a, b| (&a.slug, &a.id).cmp(&(&b.slug, &b.id)));
    for row in ordered {
        let bucket = bucket(row);
        let open = matches!(bucket, Bucket::NotStarted | Bucket::Started { .. });
        match bucket {
            Bucket::Skip => continue,
            Bucket::Dropped => {
                progress.dropped += 1;
                continue;
            }
            Bucket::NotStarted => progress.not_started += 1,
            Bucket::Started { awaiting_ci } => {
                progress.started += 1;
                progress.awaiting_ci += usize::from(awaiting_ci);
            }
            Bucket::Finished => progress.finished += 1,
        }
        let member = MemberRef::of(row);
        let flag = |list: &mut Vec<Signal>, code: &'static str, detail: String| {
            list.push(Signal {
                ticket: member.clone(),
                code,
                detail,
            });
        };
        if open {
            open_members.push(member.clone());
            let reason = row
                .blocked_reason
                .as_deref()
                .map(str::trim)
                .filter(|reason| !reason.is_empty());
            let waiting = row
                .blocked_by
                .iter()
                .filter(|id| context.unfinished_ids.contains(*id))
                .count();
            if let Some(reason) = reason {
                flag(&mut risk, "blocked", reason.to_string());
            } else if waiting > 0 {
                flag(
                    &mut risk,
                    "blocked",
                    format!(
                        "Waiting on {waiting} unfinished ticket{}",
                        if waiting == 1 { "" } else { "s" }
                    ),
                );
            }
        }
        if matches!(bucket, Bucket::Started { .. }) && !live_claim(row, now) {
            let idle_ms = row
                .updated_at
                .as_deref()
                .and_then(rfc3339_ms)
                .map(|updated| now.saturating_sub(updated));
            let limit = i64::from(thresholds.stale_hours) * 3_600_000;
            if let Some(idle) = idle_ms.filter(|idle| *idle >= limit) {
                flag(
                    &mut risk,
                    "stale",
                    format!("No update for {} hours", idle / 3_600_000),
                );
            }
        }
        if bucket == Bucket::Finished
            && let Some(score) = row
                .latest_confidence
                .filter(|score| *score < thresholds.confidence_floor)
        {
            flag(&mut risk, "low_confidence", format!("Confidence {score}"));
        }
        if context.red_gates.contains(&row.id) {
            flag(
                &mut risk,
                "red_gate",
                "Its latest orchestration run's gates failed".into(),
            );
        }
        if open
            && let Some(due) = effective_due_on(row.due_on.as_deref(), Some(milestone))
            && due_deadline_ms(Some(due), context.zone)
                .is_some_and(|deadline| is_overdue(deadline, now))
        {
            flag(&mut risk, "overdue", format!("Due {due}"));
        }
        if row.feedback_needed {
            flag(
                &mut clarity,
                "feedback_needed",
                "Waiting on an answer to FEEDBACK NEEDED".into(),
            );
        }
        if open && row.details.trim().is_empty() {
            flag(&mut clarity, "no_details", "No details".into());
        }
    }
    progress.members = progress.not_started + progress.started + progress.finished;
    progress.percent = (progress.members > 0)
        .then(|| u8::try_from(progress.finished * 100 / progress.members).unwrap_or(100));
    let due_at_ms = due_deadline_ms(milestone.due_on.as_deref(), context.zone);
    let overdue = due_at_ms.is_some_and(|deadline| is_overdue(deadline, now));
    let all_finished = progress.members > 0 && progress.finished == progress.members;
    let health = match (milestone.state, milestone.closed_reason) {
        (MilestoneState::Closed, Some(CloseReason::NotPlanned)) => Health::Dropped,
        (MilestoneState::Closed, _) => Health::Done,
        _ if all_finished => Health::Done,
        _ if overdue && !open_members.is_empty() => Health::OffTrack,
        _ if !risk.is_empty()
            || (due_at_ms.is_some_and(|deadline| {
                is_due_within(deadline, now, thresholds.due_soon_days)
            }) && progress.percent.unwrap_or(0) < thresholds.due_soon_percent) =>
        {
            Health::AtRisk
        }
        _ if !clarity.is_empty() => Health::Unclear,
        _ => Health::OnTrack,
    };
    MilestoneRollup {
        progress,
        risk,
        clarity,
        health,
        due_at_ms,
        overdue,
        open_members,
    }
}

#[cfg(test)]
#[path = "milestone_rollup_tests.rs"]
mod tests;
