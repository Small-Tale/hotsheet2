//! Phase-duration report (HS2-JS7E61): how long completed tickets spent in each
//! `started_phase`, measured two ways from history the store already keeps.
//!
//! - **Wall time** sums each closed phase span from the `Started phase changed …` activity
//!   notes ([`crate::phase_timing::phase_spans`]).
//! - **Active time** keeps only the part of those spans covered by a live claim. A `claim`
//!   event opens a claimed interval, a `renew` moves its deadline, and a `release`, a
//!   takeover `claim`, or the lease deadline (whichever comes first) ends it. Unclaimed
//!   and idle gaps between claims therefore do not inflate the numbers.
//!
//! The report aggregates per phase over tickets completed in a date range: how many tickets
//! recorded the phase, and the median, 75th, and 90th percentile and total of their
//! per-ticket times. It is the baseline for the `testing-strategy` tickets.

use std::collections::BTreeMap;

use hotsheet_model::{ClaimEventKind, StartedPhase, Status, Ticket, Timestamp};
use serde::Serialize;
use time::OffsetDateTime;

use crate::phase_timing::{PhaseSpan, median_ms, phase_spans, phase_spans_from_activity};

/// Every Started phase, in workflow order.
pub const PHASES: [StartedPhase; 6] = [
    StartedPhase::Analyzing,
    StartedPhase::Planning,
    StartedPhase::Working,
    StartedPhase::InitialTesting,
    StartedPhase::Integrating,
    StartedPhase::FinalTesting,
];

/// The wire name of a phase (`initial_testing`).
pub fn phase_key(phase: StartedPhase) -> &'static str {
    match phase {
        StartedPhase::Analyzing => "analyzing",
        StartedPhase::Planning => "planning",
        StartedPhase::Working => "working",
        StartedPhase::InitialTesting => "initial_testing",
        StartedPhase::Integrating => "integrating",
        StartedPhase::FinalTesting => "final_testing",
    }
}

/// The display name of a phase (`Initial testing`).
pub fn phase_label(phase: StartedPhase) -> &'static str {
    match phase {
        StartedPhase::Analyzing => "Analyzing",
        StartedPhase::Planning => "Planning",
        StartedPhase::Working => "Working",
        StartedPhase::InitialTesting => "Initial testing",
        StartedPhase::Integrating => "Integrating",
        StartedPhase::FinalTesting => "Final testing",
    }
}

/// A half-open `[start, end)` time interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Interval {
    pub start: OffsetDateTime,
    pub end: OffsetDateTime,
}

fn push_closed(
    out: &mut Vec<Interval>,
    start: OffsetDateTime,
    deadline: Option<OffsetDateTime>,
    at: OffsetDateTime,
) {
    let end = deadline.map_or(at, |deadline| deadline.min(at));
    if end > start {
        out.push(Interval { start, end });
    }
}

/// The ticket's claimed intervals, oldest first and non-overlapping. An interval still open
/// at the end of the history ends at its lease deadline, capped at `now`. An open interval
/// without a recorded deadline (legacy history) contributes nothing past its last event.
pub fn claimed_intervals(ticket: &Ticket, now: OffsetDateTime) -> Vec<Interval> {
    let mut events: Vec<_> = ticket
        .claim_history
        .iter()
        .filter_map(|event| Some((event.at.instant()?, event)))
        .collect();
    // Merges can interleave appended events; a stable sort keeps same-instant order.
    events.sort_by_key(|(at, _)| *at);
    let mut out = Vec::new();
    // (start, deadline, last event time) of the live claim, if any.
    let mut open: Option<(OffsetDateTime, Option<OffsetDateTime>, OffsetDateTime)> = None;
    for (at, event) in events {
        let deadline = event.lease_expires_at.as_ref().and_then(Timestamp::instant);
        match event.kind {
            ClaimEventKind::Claim => {
                if let Some((start, old, _)) = open.take() {
                    push_closed(&mut out, start, old, at);
                }
                open = Some((at, deadline, at));
            }
            ClaimEventKind::Renew => match open.as_mut() {
                Some((_, old, last)) if old.is_none_or(|old| old >= at) => {
                    *old = deadline.or(*old);
                    *last = at;
                }
                _ => {
                    // The lease lapsed before this renew (or no claim was recorded): the
                    // lapsed interval ended at its deadline and a new one starts here.
                    if let Some((start, old, _)) = open.take() {
                        push_closed(&mut out, start, old, at);
                    }
                    open = Some((at, deadline, at));
                }
            },
            ClaimEventKind::Release => {
                if let Some((start, old, _)) = open.take() {
                    push_closed(&mut out, start, old, at);
                }
            }
        }
    }
    if let Some((start, deadline, last)) = open {
        let end = deadline.map_or(last, |deadline| deadline.min(now));
        if end > start {
            out.push(Interval { start, end });
        }
    }
    out
}

fn millis(from: OffsetDateTime, to: OffsetDateTime) -> u64 {
    let nanos = (to - from).whole_nanoseconds();
    if nanos <= 0 {
        0
    } else {
        (nanos / 1_000_000) as u64
    }
}

/// Milliseconds of `[start, end)` covered by `intervals`.
pub fn covered_ms(start: OffsetDateTime, end: OffsetDateTime, intervals: &[Interval]) -> u64 {
    intervals
        .iter()
        .map(|interval| millis(interval.start.max(start), interval.end.min(end)))
        .sum()
}

/// One ticket's total time in one phase.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct PhaseTime {
    /// Closed spans of this phase (a phase can be re-entered).
    pub spans: usize,
    pub wall_ms: u64,
    pub active_ms: u64,
}

/// Per-phase time for one completed ticket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TicketPhaseTimes {
    pub slug: String,
    pub completed_at: String,
    pub phases: BTreeMap<&'static str, PhaseTime>,
}

/// Per-phase wall and active time over the ticket's closed phase spans.
pub fn ticket_phase_times(
    ticket: &Ticket,
    now: OffsetDateTime,
) -> BTreeMap<&'static str, PhaseTime> {
    let intervals = claimed_intervals(ticket, now);
    let mut phases: BTreeMap<&'static str, PhaseTime> = BTreeMap::new();
    for span in phase_spans(ticket) {
        let (Some(start), Some(end)) = (
            span.start.instant(),
            span.end.as_ref().and_then(Timestamp::instant),
        ) else {
            continue;
        };
        let time = phases.entry(phase_key(span.phase)).or_default();
        time.spans += 1;
        time.wall_ms += millis(start, end);
        time.active_ms += covered_ms(start, end, &intervals);
    }
    phases
}

/// Distribution of per-ticket times in one phase.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Distribution {
    pub median_ms: u64,
    pub p75_ms: u64,
    pub p90_ms: u64,
    pub total_ms: u64,
}

/// Nearest-rank percentile of sorted values (`fraction` in `(0, 1]`).
fn percentile(sorted: &[u64], fraction: f64) -> u64 {
    let rank = (fraction * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

/// The distribution of `values`; `None` when there are none.
pub fn distribution(mut values: Vec<u64>) -> Option<Distribution> {
    if values.is_empty() {
        return None;
    }
    values.sort_unstable();
    Some(Distribution {
        median_ms: median_ms(values.clone())?,
        p75_ms: percentile(&values, 0.75),
        p90_ms: percentile(&values, 0.90),
        total_ms: values.iter().sum(),
    })
}

/// One phase's row in the report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PhaseStats {
    pub phase: &'static str,
    pub label: &'static str,
    /// Tickets in range with at least one closed span of this phase.
    pub tickets: usize,
    pub wall: Distribution,
    pub active: Distribution,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PhaseReport {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub until: Option<String>,
    /// Completed, verified, or archived-after-completion tickets completed in the range.
    pub completed_tickets: usize,
    /// Those with at least one closed phase span.
    pub tickets_with_phases: usize,
    /// One row per phase that any ticket recorded, in workflow order.
    pub phases: Vec<PhaseStats>,
    /// Per-ticket breakdown for drill-down, oldest completion first.
    pub tickets: Vec<TicketPhaseTimes>,
}

/// When a ticket was completed, if that falls in `[since, until)`. Completed work is usually
/// archived later, so an archived ticket with a completion time counts; a reopened ticket
/// (back to Not Started/Started) does not.
fn completed_in(
    ticket: &Ticket,
    since: Option<OffsetDateTime>,
    until: Option<OffsetDateTime>,
) -> Option<OffsetDateTime> {
    if !matches!(
        ticket.status,
        Status::Completed | Status::Verified | Status::Archive
    ) {
        return None;
    }
    let at = ticket.completed_at.as_ref()?.instant()?;
    let after = since.is_none_or(|since| at >= since);
    let before = until.is_none_or(|until| at < until);
    (after && before).then_some(at)
}

/// Build the report over tickets completed in `[since, until)` (open-ended when `None`).
pub fn phase_report(
    tickets: &[Ticket],
    since: Option<&Timestamp>,
    until: Option<&Timestamp>,
    now: OffsetDateTime,
) -> PhaseReport {
    let since_at = since.and_then(Timestamp::instant);
    let until_at = until.and_then(Timestamp::instant);
    let mut rows: Vec<(OffsetDateTime, TicketPhaseTimes)> = tickets
        .iter()
        .filter_map(|ticket| {
            let at = completed_in(ticket, since_at, until_at)?;
            Some((
                at,
                TicketPhaseTimes {
                    slug: ticket.slug.clone(),
                    completed_at: ticket.completed_at.as_ref()?.as_str().to_string(),
                    phases: ticket_phase_times(ticket, now),
                },
            ))
        })
        .collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.slug.cmp(&b.1.slug)));
    let rows: Vec<TicketPhaseTimes> = rows.into_iter().map(|(_, row)| row).collect();
    let phases = PHASES
        .iter()
        .filter_map(|&phase| {
            let key = phase_key(phase);
            let times: Vec<PhaseTime> = rows
                .iter()
                .filter_map(|row| row.phases.get(key).copied())
                .collect();
            Some(PhaseStats {
                phase: key,
                label: phase_label(phase),
                tickets: times.len(),
                wall: distribution(times.iter().map(|time| time.wall_ms).collect())?,
                active: distribution(times.iter().map(|time| time.active_ms).collect())?,
            })
        })
        .collect();
    PhaseReport {
        since: since.map(|at| at.as_str().to_string()),
        until: until.map(|at| at.as_str().to_string()),
        completed_tickets: rows.len(),
        tickets_with_phases: rows.iter().filter(|row| !row.phases.is_empty()).count(),
        phases,
        tickets: rows,
    }
}

/// The phases a ticket never recorded among `working` and `initial_testing`, the two the
/// guidance most often sees skipped. Drives the completion nudge.
pub fn unrecorded_core_phases(ticket: &Ticket) -> Vec<StartedPhase> {
    missing_core_phases(&phase_spans(ticket))
}

/// [`unrecorded_core_phases`] over already-derived phase spans.
pub fn missing_core_phases(spans: &[PhaseSpan]) -> Vec<StartedPhase> {
    let recorded: Vec<StartedPhase> = spans.iter().map(|span| span.phase).collect();
    [StartedPhase::Working, StartedPhase::InitialTesting]
        .into_iter()
        .filter(|phase| !recorded.contains(phase))
        .collect()
}

/// The reminder for a ticket just completed whose phase history skipped `working` or
/// `initial_testing` (HS2-JS7E61), shared by the CLI, MCP, and HTTP completion paths
/// (HS2-8N7QZ2). `None` for a ticket that never recorded a phase or recorded both.
pub fn completion_phase_reminder(slug: &str, spans: &[PhaseSpan]) -> Option<String> {
    if spans.is_empty() {
        return None;
    }
    let missing = missing_core_phases(spans);
    if missing.is_empty() {
        return None;
    }
    let names: Vec<&str> = missing.iter().map(|phase| phase_key(*phase)).collect();
    Some(format!(
        "note: {slug} completed without recording {}. Record each started_phase transition as it \
         happens; `phase-report` measures time per phase.",
        names.join(" or ")
    ))
}

/// Whether a ticket's Completed status is a real completion rather than a close as not
/// planned, duplicate, obsolete, or works as designed (which also land in Completed).
fn completed_as_done(status: Status, reason: Option<hotsheet_model::CloseReason>) -> bool {
    status == Status::Completed
        && reason.is_none_or(|reason| reason == hotsheet_model::CloseReason::Completed)
}

/// [`completion_phase_reminder`] for a model ticket that is now Completed (as done).
pub fn ticket_completion_phase_reminder(ticket: &Ticket) -> Option<String> {
    if !completed_as_done(ticket.status, ticket.close_reason) {
        return None;
    }
    completion_phase_reminder(ticket.slug.as_str(), &phase_spans(ticket))
}

/// [`completion_phase_reminder`] for a wire ticket a client received after completing it.
pub fn api_completion_phase_reminder(ticket: &crate::ApiTicket) -> Option<String> {
    if !completed_as_done(ticket.status, ticket.close_reason) {
        return None;
    }
    let spans = phase_spans_from_activity(
        ticket
            .notes
            .iter()
            .filter(|note| note.kind == hotsheet_model::NoteKind::Activity)
            .map(|note| (Timestamp::new(note.created_at.clone()), note.text.as_str())),
    );
    completion_phase_reminder(&ticket.slug, &spans)
}

/// A report range bound: a `YYYY-MM-DD` day (midnight UTC) or an RFC 3339 instant. The
/// error names the parameter (`--since`, `since`) the caller used.
pub fn parse_report_bound(name: &str, raw: &str) -> Result<Timestamp, String> {
    let text = if raw.len() == 10 {
        format!("{raw}T00:00:00Z")
    } else {
        raw.to_string()
    };
    let at = Timestamp::new(text);
    if at.instant().is_none() {
        return Err(format!(
            "{name} must be YYYY-MM-DD or an RFC 3339 timestamp, got '{raw}'"
        ));
    }
    Ok(at)
}

/// Compact human duration: `45s`, `12.5m`, `3.2h`.
pub fn format_duration(ms: u64) -> String {
    let seconds = ms as f64 / 1000.0;
    if seconds < 60.0 {
        format!("{seconds:.0}s")
    } else if seconds < 3600.0 {
        format!("{:.1}m", seconds / 60.0)
    } else {
        format!("{:.1}h", seconds / 3600.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hotsheet_model::{ClaimEvent, Note, NoteKind, Ulid, derive_slug};

    fn at(text: &str) -> OffsetDateTime {
        Timestamp::new(text).instant().unwrap()
    }

    fn ticket() -> Ticket {
        let id = Ulid::new();
        let now = Timestamp::new("2026-10-01T00:00:00Z");
        Ticket::new(id, derive_slug(&id, "HS"), "T", "task", now.clone(), now)
    }

    fn with_claims(mut t: Ticket, events: &[(ClaimEventKind, &str, &str, Option<&str>)]) -> Ticket {
        for (kind, worker, when, lease) in events {
            t.claim_history.push(ClaimEvent {
                id: Ulid::new(),
                kind: *kind,
                worker: (*worker).to_string(),
                at: Timestamp::new(*when),
                lease_expires_at: lease.map(Timestamp::new),
                eta_at: None,
                worker_label: None,
            });
        }
        t
    }

    fn with_phases(mut t: Ticket, events: &[(&str, &str)]) -> Ticket {
        for (when, text) in events {
            t.notes.push(Note {
                id: Ulid::new(),
                kind: NoteKind::Activity,
                created_at: Timestamp::new(*when),
                edited_at: Timestamp::new(*when),
                summary: None,
                confidence: None,
                feedback_for: None,
                ai_feedback: None,
                human_edited: false,
                actor: None,
                text: (*text).to_string(),
            });
        }
        t
    }

    fn spans(intervals: &[Interval]) -> Vec<(OffsetDateTime, OffsetDateTime)> {
        intervals.iter().map(|i| (i.start, i.end)).collect()
    }

    const NOW: &str = "2026-10-02T00:00:00Z";
    use ClaimEventKind::{Claim, Release, Renew};

    #[test]
    fn claim_renew_release_is_one_interval_ending_at_the_release() {
        let t = with_claims(
            ticket(),
            &[
                (
                    Claim,
                    "w",
                    "2026-10-01T00:00:00Z",
                    Some("2026-10-01T00:30:00Z"),
                ),
                (
                    Renew,
                    "w",
                    "2026-10-01T00:20:00Z",
                    Some("2026-10-01T00:50:00Z"),
                ),
                (Release, "w", "2026-10-01T00:40:00Z", None),
            ],
        );
        assert_eq!(
            spans(&claimed_intervals(&t, at(NOW))),
            vec![(at("2026-10-01T00:00:00Z"), at("2026-10-01T00:40:00Z"))]
        );
    }

    #[test]
    fn an_unreleased_lease_ends_at_its_deadline_and_a_live_one_at_now() {
        let expired = with_claims(
            ticket(),
            &[(
                Claim,
                "w",
                "2026-10-01T00:00:00Z",
                Some("2026-10-01T00:30:00Z"),
            )],
        );
        assert_eq!(
            spans(&claimed_intervals(&expired, at(NOW))),
            vec![(at("2026-10-01T00:00:00Z"), at("2026-10-01T00:30:00Z"))]
        );
        let live = at("2026-10-01T00:10:00Z");
        assert_eq!(
            spans(&claimed_intervals(&expired, live)),
            vec![(at("2026-10-01T00:00:00Z"), live)]
        );
    }

    #[test]
    fn idle_gaps_between_claims_are_excluded() {
        let t = with_claims(
            ticket(),
            &[
                (
                    Claim,
                    "a",
                    "2026-10-01T00:00:00Z",
                    Some("2026-10-01T00:30:00Z"),
                ),
                (Release, "a", "2026-10-01T00:10:00Z", None),
                (
                    Claim,
                    "b",
                    "2026-10-01T01:00:00Z",
                    Some("2026-10-01T01:30:00Z"),
                ),
                (Release, "b", "2026-10-01T01:05:00Z", None),
            ],
        );
        let intervals = claimed_intervals(&t, at(NOW));
        assert_eq!(intervals.len(), 2);
        assert_eq!(
            covered_ms(
                at("2026-10-01T00:00:00Z"),
                at("2026-10-01T02:00:00Z"),
                &intervals
            ),
            15 * 60_000
        );
    }

    #[test]
    fn a_lapsed_lease_ends_at_its_deadline_even_when_renewed_or_released_later() {
        let t = with_claims(
            ticket(),
            &[
                (
                    Claim,
                    "w",
                    "2026-10-01T00:00:00Z",
                    Some("2026-10-01T00:30:00Z"),
                ),
                // Renewed after expiry: the first interval ended at 00:30; a new one starts.
                (
                    Renew,
                    "w",
                    "2026-10-01T01:00:00Z",
                    Some("2026-10-01T01:30:00Z"),
                ),
                // Released after that lease lapsed too: it ends at its 01:30 deadline.
                (Release, "w", "2026-10-01T02:00:00Z", None),
                // A stray release with nothing open changes nothing.
                (Release, "w", "2026-10-01T03:00:00Z", None),
            ],
        );
        assert_eq!(
            spans(&claimed_intervals(&t, at(NOW))),
            vec![
                (at("2026-10-01T00:00:00Z"), at("2026-10-01T00:30:00Z")),
                (at("2026-10-01T01:00:00Z"), at("2026-10-01T01:30:00Z")),
            ]
        );
    }

    #[test]
    fn a_takeover_claim_closes_the_previous_holder_without_double_counting() {
        let t = with_claims(
            ticket(),
            &[
                (
                    Claim,
                    "a",
                    "2026-10-01T00:00:00Z",
                    Some("2026-10-01T00:30:00Z"),
                ),
                // b steals after a's lease lapsed; a's interval ended at 00:30, not 00:45.
                (
                    Claim,
                    "b",
                    "2026-10-01T00:45:00Z",
                    Some("2026-10-01T01:15:00Z"),
                ),
                (Release, "b", "2026-10-01T01:00:00Z", None),
            ],
        );
        assert_eq!(
            spans(&claimed_intervals(&t, at(NOW))),
            vec![
                (at("2026-10-01T00:00:00Z"), at("2026-10-01T00:30:00Z")),
                (at("2026-10-01T00:45:00Z"), at("2026-10-01T01:00:00Z")),
            ]
        );
    }

    #[test]
    fn renew_without_a_claim_and_out_of_order_events_still_walk_in_time_order() {
        let t = with_claims(
            ticket(),
            &[
                (Release, "w", "2026-10-01T00:20:00Z", None),
                (
                    Renew,
                    "w",
                    "2026-10-01T00:00:00Z",
                    Some("2026-10-01T00:30:00Z"),
                ),
            ],
        );
        assert_eq!(
            spans(&claimed_intervals(&t, at(NOW))),
            vec![(at("2026-10-01T00:00:00Z"), at("2026-10-01T00:20:00Z"))]
        );
    }

    #[test]
    fn legacy_claims_without_a_deadline_count_only_up_to_their_last_event() {
        let t = with_claims(
            ticket(),
            &[
                (Claim, "w", "2026-10-01T00:00:00Z", None),
                (Renew, "w", "2026-10-01T00:10:00Z", None),
            ],
        );
        assert_eq!(
            spans(&claimed_intervals(&t, at(NOW))),
            vec![(at("2026-10-01T00:00:00Z"), at("2026-10-01T00:10:00Z"))]
        );
        assert!(
            claimed_intervals(
                &with_claims(ticket(), &[(Claim, "w", "2026-10-01T00:00:00Z", None)]),
                at(NOW)
            )
            .is_empty()
        );
    }

    fn completed_ticket(completed: &str) -> Ticket {
        let mut t = with_phases(
            ticket(),
            &[
                (
                    "2026-10-01T00:00:00Z",
                    "Started phase changed from Unspecified to Analyzing",
                ),
                (
                    "2026-10-01T00:10:00Z",
                    "Started phase changed from Analyzing to Working",
                ),
                (
                    "2026-10-01T01:10:00Z",
                    "Started phase changed from Working to Initial testing",
                ),
                (
                    "2026-10-01T01:20:00Z",
                    "Started phase changed from Initial testing to Working",
                ),
                (
                    "2026-10-01T01:30:00Z",
                    "Started phase changed from Working to Initial testing",
                ),
                (
                    "2026-10-01T01:40:00Z",
                    "Status changed from Started to Completed",
                ),
            ],
        );
        t = with_claims(
            t,
            &[
                (
                    Claim,
                    "w",
                    "2026-10-01T00:00:00Z",
                    Some("2026-10-01T00:30:00Z"),
                ),
                (Release, "w", "2026-10-01T00:20:00Z", None),
                // Idle from 00:20 to 01:00 while Working.
                (
                    Claim,
                    "w",
                    "2026-10-01T01:00:00Z",
                    Some("2026-10-01T02:00:00Z"),
                ),
                (Release, "w", "2026-10-01T01:40:00Z", None),
            ],
        );
        t.status = Status::Completed;
        t.completed_at = Some(Timestamp::new(completed));
        t
    }

    #[test]
    fn ticket_times_sum_re_entered_phases_and_exclude_idle_time() {
        let times = ticket_phase_times(&completed_ticket("2026-10-01T01:40:00Z"), at(NOW));
        assert_eq!(
            times["analyzing"],
            PhaseTime {
                spans: 1,
                wall_ms: 10 * 60_000,
                active_ms: 10 * 60_000
            }
        );
        // Working 00:10–01:10 and 01:20–01:30: 70 min wall, claimed 00:10–00:20 + 01:00–01:10 + 01:20–01:30.
        assert_eq!(
            times["working"],
            PhaseTime {
                spans: 2,
                wall_ms: 70 * 60_000,
                active_ms: 30 * 60_000
            }
        );
        assert_eq!(
            times["initial_testing"],
            PhaseTime {
                spans: 2,
                wall_ms: 20 * 60_000,
                active_ms: 20 * 60_000
            }
        );
        assert!(!times.contains_key("integrating"));
    }

    #[test]
    fn report_filters_by_completion_range_and_aggregates_per_ticket() {
        let mut fast = completed_ticket("2026-10-01T01:40:00Z");
        fast.notes.retain(|note| {
            !note.text.contains("to Working") && !note.text.contains("to Initial testing")
        });
        let slow = completed_ticket("2026-10-03T00:00:00Z");
        let mut reopened = completed_ticket("2026-10-01T05:00:00Z");
        reopened.status = Status::Started;
        let mut archived = completed_ticket("2026-10-01T06:00:00Z");
        archived.status = Status::Archive;
        let mut never_completed = completed_ticket("2026-10-01T07:00:00Z");
        never_completed.status = Status::Archive;
        never_completed.completed_at = None;
        let out_of_range = completed_ticket("2026-09-01T00:00:00Z");
        let report = phase_report(
            &[
                slow.clone(),
                fast.clone(),
                reopened,
                archived.clone(),
                never_completed,
                out_of_range,
            ],
            Some(&Timestamp::new("2026-09-15T00:00:00Z")),
            None,
            at("2026-10-04T00:00:00Z"),
        );
        assert_eq!(report.completed_tickets, 3);
        assert_eq!(report.tickets_with_phases, 3);
        assert_eq!(
            report
                .tickets
                .iter()
                .map(|row| row.slug.clone())
                .collect::<Vec<_>>(),
            vec![fast.slug.clone(), archived.slug.clone(), slow.slug.clone()],
            "oldest completion first; archived-after-completion counts, reopened does not"
        );
        let analyzing = &report.phases[0];
        assert_eq!((analyzing.phase, analyzing.tickets), ("analyzing", 3));
        // `fast` analyzed 00:00–01:40 (its only phase); the other two analyzed 10 min.
        assert_eq!(analyzing.wall.median_ms, 10 * 60_000);
        assert_eq!(analyzing.wall.p90_ms, 100 * 60_000);
        assert_eq!(analyzing.wall.total_ms, 120 * 60_000);
        // `fast` was claimed 00:00–00:20 and 01:00–01:40 of its 100 analyzing minutes.
        assert_eq!(
            analyzing.active.p90_ms,
            60 * 60_000,
            "only claimed time counts"
        );
        let working = report
            .phases
            .iter()
            .find(|row| row.phase == "working")
            .unwrap();
        assert_eq!(working.tickets, 2);
        assert!(
            report.phases.iter().all(|row| row.phase != "planning"),
            "unrecorded phases are omitted"
        );

        let until = phase_report(
            &[slow, fast],
            None,
            Some(&Timestamp::new("2026-10-02T00:00:00Z")),
            at(NOW),
        );
        assert_eq!(until.completed_tickets, 1);
        assert_eq!(until.until.as_deref(), Some("2026-10-02T00:00:00Z"));
    }

    #[test]
    fn distribution_uses_nearest_rank_percentiles() {
        let d = distribution((1..=10).collect()).unwrap();
        assert_eq!(
            d,
            Distribution {
                median_ms: 5,
                p75_ms: 8,
                p90_ms: 9,
                total_ms: 55
            }
        );
        assert_eq!(
            distribution(vec![7]).unwrap(),
            Distribution {
                median_ms: 7,
                p75_ms: 7,
                p90_ms: 7,
                total_ms: 7
            }
        );
        assert_eq!(distribution(Vec::new()), None);
    }

    #[test]
    fn unrecorded_core_phases_drive_the_completion_nudge() {
        assert!(unrecorded_core_phases(&completed_ticket("2026-10-01T01:40:00Z")).is_empty());
        let skipped = with_phases(
            ticket(),
            &[
                (
                    "2026-10-01T00:00:00Z",
                    "Started phase changed from Unspecified to Analyzing",
                ),
                (
                    "2026-10-01T00:10:00Z",
                    "Started phase changed from Analyzing to Integrating",
                ),
            ],
        );
        assert_eq!(
            unrecorded_core_phases(&skipped),
            vec![StartedPhase::Working, StartedPhase::InitialTesting]
        );
    }

    #[test]
    fn durations_format_compactly() {
        assert_eq!(format_duration(45_000), "45s");
        assert_eq!(format_duration(4 * 60_000 + 6_000), "4.1m");
        assert_eq!(format_duration(3 * 3_600_000 + 720_000), "3.2h");
    }

    /// HS2-8N7QZ2: the shared bound parser and the completion reminder behave the same for
    /// model tickets and for the wire tickets MCP and HTTP callers receive.
    #[test]
    fn report_bounds_and_the_completion_reminder_are_shared() {
        assert_eq!(
            parse_report_bound("since", "2026-10-01").unwrap().as_str(),
            "2026-10-01T00:00:00Z"
        );
        assert_eq!(
            parse_report_bound("until", "2026-10-01T08:30:00Z")
                .unwrap()
                .as_str(),
            "2026-10-01T08:30:00Z"
        );
        assert_eq!(
            parse_report_bound("--since", "soon").unwrap_err(),
            "--since must be YYYY-MM-DD or an RFC 3339 timestamp, got 'soon'"
        );
        let span = |phase| PhaseSpan {
            phase,
            start: Timestamp::new("2026-10-16T08:00:00Z"),
            end: None,
        };
        assert_eq!(completion_phase_reminder("HS-1", &[]), None);
        assert_eq!(
            completion_phase_reminder(
                "HS-1",
                &[
                    span(StartedPhase::Working),
                    span(StartedPhase::InitialTesting)
                ]
            ),
            None
        );
        let reminder = completion_phase_reminder("HS-1", &[span(StartedPhase::Analyzing)]).unwrap();
        assert!(
            reminder
                .starts_with("note: HS-1 completed without recording working or initial_testing.")
        );
        assert!(
            completion_phase_reminder("HS-1", &[span(StartedPhase::Working)])
                .unwrap()
                .contains("without recording initial_testing.")
        );
    }
}
