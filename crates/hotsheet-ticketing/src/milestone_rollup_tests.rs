//! Roll-up state matrix (HS2-A820C1, docs/38 §38.10 "Roll-up unit").

use std::collections::HashSet;

use hotsheet_model::{CloseReason as TicketClose, StartedPhase, Status, Ticket, Ulid};
use jiff::tz::TimeZone;

use super::*;
use crate::milestones::{CloseReason, Milestone, MilestoneState};

const ID: &str = "7K3M9P2QXA";
/// 2026-11-10T00:00:00Z.
const NOW: &str = "2026-11-10T00:00:00Z";

fn now_ms() -> i64 {
    NOW.parse::<jiff::Timestamp>().unwrap().as_millisecond()
}

fn milestone(due: Option<&str>) -> Milestone {
    let mut m = Milestone::unknown(ID);
    m.title = "Toolchain 2.0".into();
    m.named = true;
    m.state = MilestoneState::Open;
    m.due_on = due.map(str::to_string);
    m
}

fn ticket(n: u8, status: Status) -> Ticket {
    let id = Ulid::from_string(&format!("01ARZ3NDEKTSV4RRFFQ69G5F{n:02}")).unwrap();
    let mut t = Ticket::new(
        id,
        format!("HS-{n}"),
        format!("Ticket {n}"),
        "task",
        "2026-11-01T00:00:00Z",
        "2026-11-09T12:00:00Z",
    );
    t.status = status;
    t.details = "Has details.".into();
    t.milestone = Some(ID.into());
    if status == Status::Started {
        t.started_phase = Some(StartedPhase::Working);
    }
    t
}

fn rows(tickets: &[Ticket]) -> Vec<TicketRow> {
    tickets.iter().map(TicketRow::from).collect()
}

struct Fixture {
    zone: TimeZone,
    unfinished: HashSet<String>,
    red: HashSet<String>,
    now_ms: i64,
    thresholds: RollupThresholds,
}

impl Fixture {
    fn new() -> Self {
        Self {
            zone: TimeZone::UTC,
            unfinished: HashSet::new(),
            red: HashSet::new(),
            now_ms: now_ms(),
            thresholds: RollupThresholds::default(),
        }
    }

    fn roll(&self, milestone: &Milestone, tickets: &[Ticket]) -> MilestoneRollup {
        self.roll_rows(milestone, &rows(tickets))
    }

    fn roll_rows(&self, milestone: &Milestone, rows: &[TicketRow]) -> MilestoneRollup {
        milestone_rollup(
            milestone,
            rows,
            &[],
            &RollupContext {
                now_ms: self.now_ms,
                zone: &self.zone,
                thresholds: self.thresholds,
                unfinished_ids: &self.unfinished,
                red_gates: &self.red,
            },
        )
    }
}

fn codes(signals: &[Signal]) -> Vec<(&str, &'static str)> {
    signals
        .iter()
        .map(|signal| (signal.ticket.slug.as_str(), signal.code))
        .collect()
}

#[test]
fn hs2_a820c1_each_status_and_close_reason_lands_in_one_bucket() {
    let f = Fixture::new();
    let m = milestone(None);
    let mut final_testing = ticket(3, Status::Started);
    final_testing.started_phase = Some(StartedPhase::FinalTesting);
    let closed = |n, status, reason| {
        let mut t = ticket(n, status);
        t.close_reason = Some(reason);
        t
    };
    let tickets = vec![
        ticket(1, Status::NotStarted),
        ticket(2, Status::Backlog),
        ticket(4, Status::Started),
        final_testing,
        ticket(5, Status::Completed),
        ticket(6, Status::Verified),
        closed(7, Status::Archive, TicketClose::Completed),
        ticket(8, Status::Archive),
        closed(9, Status::Completed, TicketClose::NotPlanned),
        closed(10, Status::Archive, TicketClose::Duplicate),
        closed(11, Status::Completed, TicketClose::Obsolete),
        closed(12, Status::Completed, TicketClose::WorksAsDesigned),
        ticket(13, Status::Deleted),
        ticket(14, Status::Moved),
    ];
    let p = f.roll(&m, &tickets).progress;
    assert_eq!(
        (
            p.not_started,
            p.started,
            p.awaiting_ci,
            p.finished,
            p.dropped
        ),
        (2, 2, 1, 4, 5)
    );
    assert_eq!(p.members, 8, "dropped and moved tombstones are not members");
    assert_eq!(p.percent, Some(50));
}

#[test]
fn hs2_a820c1_empty_then_refill_reopen_trash_restore_and_move() {
    let f = Fixture::new();
    let m = milestone(None);
    // Empty: no percent ("No tickets"), on track.
    let empty = f.roll(&m, &[]);
    assert_eq!((empty.progress.members, empty.progress.percent), (0, None));
    assert_eq!(empty.health, Health::OnTrack);

    // Refill with one finished ticket: done.
    let mut a = ticket(1, Status::Completed);
    let r = f.roll(&m, std::slice::from_ref(&a));
    assert_eq!((r.progress.percent, r.health), (Some(100), Health::Done));
    assert!(r.open_members.is_empty());

    // Reopen it: open again, no longer done.
    a.status = Status::NotStarted;
    let r = f.roll(&m, std::slice::from_ref(&a));
    assert_eq!((r.progress.percent, r.health), (Some(0), Health::OnTrack));
    assert_eq!(r.open_members.len(), 1);

    // Trash, then restore.
    a.status = Status::Deleted;
    let r = f.roll(&m, std::slice::from_ref(&a));
    assert_eq!((r.progress.members, r.progress.dropped), (0, 1));
    a.status = Status::NotStarted;
    assert_eq!(f.roll(&m, std::slice::from_ref(&a)).progress.members, 1);

    // Move to another store: the tombstone is skipped and the destination counts once,
    // even when it reads from another source.
    let mut tombstone = a.clone();
    tombstone.status = Status::Moved;
    let mut destination = TicketRow::from(&a);
    destination.connection_id = "other-store".into();
    let r = milestone_rollup(
        &m,
        &[TicketRow::from(&tombstone), destination],
        &[],
        &RollupContext {
            now_ms: f.now_ms,
            zone: &f.zone,
            thresholds: f.thresholds,
            unfinished_ids: &f.unfinished,
            red_gates: &f.red,
        },
    );
    assert_eq!((r.progress.members, r.progress.dropped), (1, 0));
    assert_eq!(r.open_members[0].connection_id, "other-store");
}

#[test]
fn hs2_a820c1_unreadable_sources_are_named_and_unknown_ids_roll_up() {
    let f = Fixture::new();
    let unknown = Milestone::unknown("ZZZZZZZZZZ");
    let mut t = ticket(1, Status::Started);
    t.milestone = Some("ZZZZZZZZZZ".into());
    let r = milestone_rollup(
        &unknown,
        &rows(&[t]),
        &["jira-1".to_string()],
        &RollupContext {
            now_ms: f.now_ms,
            zone: &f.zone,
            thresholds: f.thresholds,
            unfinished_ids: &f.unfinished,
            red_gates: &f.red,
        },
    );
    assert_eq!(r.progress.unavailable, vec!["jira-1".to_string()]);
    assert_eq!(r.progress.started, 1);
    assert_eq!(r.health, Health::OnTrack);
}

#[test]
fn hs2_a820c1_stale_signal_at_below_and_above_the_threshold() {
    let f = Fixture::new();
    let m = milestone(None);
    let started = |updated: &str| {
        let mut t = ticket(1, Status::Started);
        t.updated_at = updated.into();
        t
    };
    // 48 h is the default: just under is fine, at and over is stale.
    for (updated, stale) in [
        ("2026-11-08T00:00:01Z", false),
        ("2026-11-08T00:00:00Z", true),
        ("2026-11-01T00:00:00Z", true),
    ] {
        let r = f.roll(&m, &[started(updated)]);
        assert_eq!(
            codes(&r.risk).contains(&("HS-1", "stale")),
            stale,
            "{updated}"
        );
    }
    // A live claim is activity; an expired one is not.
    let mut claimed = started("2026-11-01T00:00:00Z");
    claimed.claimed_by = Some("w".into());
    claimed.claim_lease_expires_at = Some("2026-11-10T00:30:00Z".into());
    assert!(f.roll(&m, std::slice::from_ref(&claimed)).risk.is_empty());
    claimed.claim_lease_expires_at = Some("2026-11-09T23:00:00Z".into());
    assert_eq!(codes(&f.roll(&m, &[claimed]).risk), vec![("HS-1", "stale")]);
    // Not started tickets are never stale.
    let mut idle = ticket(2, Status::NotStarted);
    idle.updated_at = "2026-01-01T00:00:00Z".into();
    assert!(f.roll(&m, &[idle]).risk.is_empty());
}

fn confident(score: u8) -> TicketRow {
    let mut row = TicketRow::from(&ticket(1, Status::Completed));
    row.latest_confidence = Some(score);
    row
}

#[test]
fn hs2_a820c1_low_confidence_at_below_and_above_the_floor() {
    let f = Fixture::new();
    let m = milestone(None);
    for (score, low) in [(69, true), (70, false), (95, false)] {
        let r = f.roll_rows(&m, &[confident(score)]);
        assert_eq!(
            codes(&r.risk).contains(&("HS-1", "low_confidence")),
            low,
            "{score}"
        );
        // A risk signal on an all-finished milestone still reads done.
        assert_eq!(r.health, Health::Done);
    }
}

#[test]
fn hs2_a820c1_blocked_red_gate_and_clarity_signals() {
    let mut f = Fixture::new();
    let m = milestone(None);
    let mut reasoned = ticket(1, Status::NotStarted);
    reasoned.blocked_reason = Some("  Waiting on legal  ".into());
    let blocker = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FZZ").unwrap();
    let mut edged = ticket(2, Status::Started);
    edged.blocked_by = vec![blocker];
    let mut questions = ticket(3, Status::NotStarted);
    questions.details = "FEEDBACK NEEDED: which API?".into();
    let mut empty = ticket(4, Status::NotStarted);
    empty.details = "   ".into();
    let mut done_empty = ticket(5, Status::Completed);
    done_empty.details = String::new();
    let tickets = [reasoned, edged, questions, empty, done_empty];

    // The blocker edge counts only while that ticket is unfinished.
    let r = f.roll(&m, &tickets);
    assert_eq!(codes(&r.risk), vec![("HS-1", "blocked")]);
    assert_eq!(r.risk[0].detail, "Waiting on legal");
    f.unfinished.insert(blocker.to_string());
    let r = f.roll(&m, &tickets);
    assert_eq!(
        codes(&r.risk),
        vec![("HS-1", "blocked"), ("HS-2", "blocked")]
    );
    assert_eq!(
        codes(&r.clarity),
        vec![("HS-3", "feedback_needed"), ("HS-4", "no_details")],
        "a finished ticket's empty body is not a clarity gap"
    );
    assert_eq!(r.health, Health::AtRisk);

    f.red.insert(tickets[4].id.to_string());
    let r = f.roll(&m, &tickets);
    assert!(codes(&r.risk).contains(&("HS-5", "red_gate")));
}

#[test]
fn hs2_a820c1_overdue_reads_the_effective_due_in_the_project_zone() {
    let mut f = Fixture::new();
    // The milestone's date lends itself while open; the earlier date wins.
    let m = milestone(Some("2026-11-09"));
    let a = ticket(1, Status::NotStarted);
    let mut b = ticket(2, Status::NotStarted);
    b.due_on = Some("2026-11-20".into());
    let r = f.roll(&m, &[a.clone(), b.clone()]);
    assert_eq!(
        codes(&r.risk),
        vec![("HS-1", "overdue"), ("HS-2", "overdue")]
    );
    assert!(r.overdue);
    assert_eq!(
        r.health,
        Health::OffTrack,
        "due date past with open members"
    );

    // Exactly at the deadline instant it is not overdue yet; one ms later it is.
    let undated = milestone(None);
    let mut own = ticket(3, Status::Started);
    own.updated_at = "2026-11-09T23:00:00Z".into();
    own.due_on = Some("2026-11-09".into());
    f.now_ms = now_ms() - 1;
    assert!(f.roll(&undated, std::slice::from_ref(&own)).risk.is_empty());
    f.now_ms = now_ms();
    assert_eq!(
        codes(&f.roll(&undated, std::slice::from_ref(&own)).risk),
        vec![("HS-3", "overdue")]
    );
    // In a zone west of UTC the same date has not ended yet.
    f.zone = TimeZone::get("America/New_York").unwrap();
    assert!(f.roll(&undated, &[own]).risk.is_empty());

    // A closed milestone lends no date, and finished tickets are never overdue.
    let mut closed = milestone(Some("2026-11-01"));
    closed.state = MilestoneState::Closed;
    closed.closed_reason = Some(CloseReason::Completed);
    f.zone = TimeZone::UTC;
    assert!(f.roll(&closed, &[a]).risk.is_empty());
    let finished = ticket(4, Status::Completed);
    assert!(f.roll(&m, &[finished]).risk.is_empty());
}

#[test]
fn hs2_a820c1_health_precedence() {
    let f = Fixture::new();
    let open = ticket(1, Status::NotStarted);
    let finished = ticket(2, Status::Completed);

    // Closed as completed is done whatever the members; closed as not planned is dropped.
    let mut m = milestone(Some("2026-11-01"));
    m.state = MilestoneState::Closed;
    m.closed_reason = Some(CloseReason::Completed);
    assert_eq!(f.roll(&m, std::slice::from_ref(&open)).health, Health::Done);
    m.closed_reason = Some(CloseReason::NotPlanned);
    assert_eq!(
        f.roll(&m, std::slice::from_ref(&open)).health,
        Health::Dropped
    );

    // Open: every member finished beats an overdue date.
    let m = milestone(Some("2026-11-01"));
    assert_eq!(
        f.roll(&m, std::slice::from_ref(&finished)).health,
        Health::Done
    );
    assert_eq!(
        f.roll(&m, &[open.clone(), finished.clone()]).health,
        Health::OffTrack
    );

    // Due within 7 days: at risk under 80 % finished, on track at 80 %.
    let soon = milestone(Some("2026-11-15"));
    assert_eq!(
        f.roll(&soon, &[open.clone(), finished.clone()]).health,
        Health::AtRisk
    );
    let mut many: Vec<Ticket> = (10..14).map(|n| ticket(n, Status::Completed)).collect();
    many.push(open.clone());
    assert_eq!(f.roll(&soon, &many).health, Health::OnTrack);
    // Far away: on track; a clarity signal alone is unclear.
    let later = milestone(Some("2027-03-01"));
    assert_eq!(
        f.roll(&later, std::slice::from_ref(&open)).health,
        Health::OnTrack
    );
    let mut unclear = open.clone();
    unclear.details.clear();
    assert_eq!(f.roll(&later, &[unclear.clone()]).health, Health::Unclear);
    // A risk signal outranks clarity.
    unclear.blocked_reason = Some("x".into());
    assert_eq!(f.roll(&later, &[unclear]).health, Health::AtRisk);
}

#[test]
fn hs2_a820c1_effective_due_takes_the_earlier_valid_date() {
    let open = milestone(Some("2026-12-04"));
    assert_eq!(effective_due_on(None, Some(&open)), Some("2026-12-04"));
    assert_eq!(
        effective_due_on(Some("2026-12-01"), Some(&open)),
        Some("2026-12-01")
    );
    assert_eq!(
        effective_due_on(Some("2026-12-09"), Some(&open)),
        Some("2026-12-04")
    );
    assert_eq!(
        effective_due_on(Some("not a date"), Some(&open)),
        Some("2026-12-04")
    );
    let mut closed = open.clone();
    closed.state = MilestoneState::Closed;
    assert_eq!(effective_due_on(None, Some(&closed)), None);
    assert_eq!(
        effective_due_on(Some("2026-12-09"), Some(&closed)),
        Some("2026-12-09")
    );
    let mut conflicted = open;
    conflicted.state = MilestoneState::Conflicted;
    assert_eq!(
        effective_due_on(None, Some(&conflicted)),
        Some("2026-12-04")
    );
}
