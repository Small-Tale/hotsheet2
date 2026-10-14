//! Started-phase timing derived from existing activity history (HS2-YDGM04).
//!
//! Every `started_phase` change appends an `activity` note
//! (`Started phase changed from X to Y`), and leaving Started appends
//! `Status changed from Started to Z`. Walking those notes in time order yields each
//! ticket's phase spans without any new storage. Closed spans of the testing phases feed a
//! per-store median that clients render as "time left in this phase"; the still-open span
//! supplies when the ticket's current phase began.

use std::collections::HashMap;

use hotsheet_model::{NoteKind, StartedPhase, Status, Ticket, Timestamp};

/// Fewest closed spans of a phase before its typical duration is offered as an estimate.
pub const MIN_PHASE_SPANS_FOR_ESTIMATE: usize = 3;

/// The phases whose remaining time is estimated.
pub const ESTIMATED_PHASES: [StartedPhase; 2] =
    [StartedPhase::InitialTesting, StartedPhase::FinalTesting];

/// Whether a phase is one whose remaining time is estimated.
pub fn is_estimated_phase(phase: StartedPhase) -> bool {
    ESTIMATED_PHASES.contains(&phase)
}

/// One continuous stay in a Started phase. `end` is `None` while the span is still open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhaseSpan {
    pub phase: StartedPhase,
    pub start: Timestamp,
    pub end: Option<Timestamp>,
}

impl PhaseSpan {
    /// Closed duration in milliseconds; `None` while open or when a bound is unparseable.
    pub fn duration_ms(&self) -> Option<u64> {
        let end = self.end.as_ref()?.instant()?;
        let start = self.start.instant()?;
        let nanos = (end - start).whole_nanoseconds();
        (nanos >= 0).then_some((nanos / 1_000_000) as u64)
    }
}

fn phase_from_label(label: &str) -> Option<StartedPhase> {
    match label {
        "Analyzing" => Some(StartedPhase::Analyzing),
        "Planning" => Some(StartedPhase::Planning),
        "Working" => Some(StartedPhase::Working),
        "Initial testing" => Some(StartedPhase::InitialTesting),
        "Integrating" => Some(StartedPhase::Integrating),
        "Final testing" => Some(StartedPhase::FinalTesting),
        _ => None,
    }
}

enum Transition {
    /// Entered `Some(phase)`, or cleared to Unspecified.
    Phase(Option<StartedPhase>),
    /// The ticket left Started; any open span ends.
    LeftStarted,
}

fn transition(text: &str) -> Option<Transition> {
    if let Some(rest) = text.strip_prefix("Started phase changed from ") {
        let (_, to) = rest.rsplit_once(" to ")?;
        return Some(Transition::Phase(phase_from_label(to.trim())));
    }
    let rest = text.strip_prefix("Status changed from ")?;
    let (from, to) = rest.split_once(" to ")?;
    (from == "Started" && to.trim() != "Started").then_some(Transition::LeftStarted)
}

/// Every phase span recorded in a ticket's activity notes, oldest first.
pub fn phase_spans(ticket: &Ticket) -> Vec<PhaseSpan> {
    let mut events: Vec<(&Timestamp, Transition)> = ticket
        .notes
        .iter()
        .filter(|note| note.kind == NoteKind::Activity)
        .filter_map(|note| Some((&note.created_at, transition(&note.text)?)))
        .collect();
    // Notes are appended in time order, but merges can interleave them; a stable sort by
    // instant keeps same-instant transitions in their recorded order.
    events.sort_by_key(|(at, _)| at.instant());
    let mut spans = Vec::new();
    let mut open: Option<PhaseSpan> = None;
    for (at, event) in events {
        if let Some(mut span) = open.take() {
            span.end = Some(at.clone());
            spans.push(span);
        }
        if let Transition::Phase(Some(phase)) = event {
            open = Some(PhaseSpan {
                phase,
                start: at.clone(),
                end: None,
            });
        }
    }
    spans.extend(open);
    spans
}

/// When the ticket's current Started phase began: the start of the still-open span, when it
/// matches the ticket's live phase. `None` outside Started or without matching history.
pub fn started_phase_since(ticket: &Ticket) -> Option<Timestamp> {
    let phase = ticket.started_phase?;
    if ticket.status != Status::Started {
        return None;
    }
    phase_spans(ticket)
        .pop()
        .filter(|span| span.end.is_none() && span.phase == phase)
        .map(|span| span.start)
}

/// Closed span durations of the estimated phases, for indexing.
pub fn estimated_phase_durations(ticket: &Ticket) -> Vec<(StartedPhase, u64)> {
    phase_spans(ticket)
        .into_iter()
        .filter(|span| is_estimated_phase(span.phase))
        .filter_map(|span| Some((span.phase, span.duration_ms()?)))
        .collect()
}

/// The median of `values` (mean of the middle pair for an even count).
pub fn median_ms(mut values: Vec<u64>) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    values.sort_unstable();
    let mid = values.len() / 2;
    Some(if values.len() % 2 == 1 {
        values[mid]
    } else {
        values[mid - 1] / 2 + values[mid] / 2 + (values[mid - 1] % 2 + values[mid] % 2) / 2
    })
}

/// Typical duration per estimated phase from closed spans, omitting phases with fewer than
/// [`MIN_PHASE_SPANS_FOR_ESTIMATE`] spans.
pub fn typical_phase_durations(
    spans: impl IntoIterator<Item = (StartedPhase, u64)>,
) -> HashMap<StartedPhase, u64> {
    let mut by_phase: HashMap<StartedPhase, Vec<u64>> = HashMap::new();
    for (phase, duration) in spans {
        if is_estimated_phase(phase) {
            by_phase.entry(phase).or_default().push(duration);
        }
    }
    by_phase
        .into_iter()
        .filter(|(_, values)| values.len() >= MIN_PHASE_SPANS_FOR_ESTIMATE)
        .filter_map(|(phase, values)| Some((phase, median_ms(values)?)))
        .collect()
}

/// Fill a list row's typical-duration estimate from per-store typical durations.
pub fn apply_phase_estimate(row: &mut crate::TicketRow, typical: &HashMap<StartedPhase, u64>) {
    row.started_phase_estimate_ms = None;
    if row.status.as_deref() != Some("started") || row.started_phase_since.is_none() {
        return;
    }
    let phase = match row.started_phase.as_deref() {
        Some("initial_testing") => StartedPhase::InitialTesting,
        Some("final_testing") => StartedPhase::FinalTesting,
        _ => return,
    };
    row.started_phase_estimate_ms = typical.get(&phase).copied();
}

#[cfg(test)]
mod tests {
    use super::*;
    use hotsheet_model::{Note, Ulid, derive_slug};

    fn ticket(events: &[(&str, &str)]) -> Ticket {
        let id = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
        let now = Timestamp::new("2026-10-01T00:00:00Z");
        let mut t = Ticket::new(id, derive_slug(&id, "HS"), "T", "task", now.clone(), now);
        for (at, text) in events {
            t.notes.push(Note {
                id: Ulid::new(),
                kind: NoteKind::Activity,
                created_at: Timestamp::new(*at),
                edited_at: Timestamp::new(*at),
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

    const MIN: u64 = 60_000;

    #[test]
    fn spans_close_on_next_phase_and_on_leaving_started() {
        let t = ticket(&[
            (
                "2026-10-01T00:00:00Z",
                "Status changed from Not Started to Started",
            ),
            (
                "2026-10-01T00:00:00Z",
                "Started phase changed from Unspecified to Working",
            ),
            (
                "2026-10-01T00:10:00Z",
                "Started phase changed from Working to Initial testing",
            ),
            (
                "2026-10-01T00:16:00Z",
                "Started phase changed from Initial testing to Integrating",
            ),
            (
                "2026-10-01T00:18:00Z",
                "Started phase changed from Integrating to Final testing",
            ),
            (
                "2026-10-01T00:30:00Z",
                "Status changed from Started to Completed",
            ),
        ]);
        let durations = estimated_phase_durations(&t);
        assert_eq!(
            durations,
            vec![
                (StartedPhase::InitialTesting, 6 * MIN),
                (StartedPhase::FinalTesting, 12 * MIN)
            ]
        );
        assert_eq!(phase_spans(&t).len(), 4);
        assert!(phase_spans(&t).iter().all(|span| span.end.is_some()));
    }

    #[test]
    fn reversed_and_repeated_transitions_are_separate_spans() {
        let t = ticket(&[
            (
                "2026-10-01T00:00:00Z",
                "Started phase changed from Working to Initial testing",
            ),
            (
                "2026-10-01T00:05:00Z",
                "Started phase changed from Initial testing to Working",
            ),
            (
                "2026-10-01T00:20:00Z",
                "Started phase changed from Working to Initial testing",
            ),
            (
                "2026-10-01T00:24:00Z",
                "Started phase changed from Initial testing to Unspecified",
            ),
            (
                "2026-10-01T00:25:00Z",
                "Started phase changed from Unspecified to Initial testing",
            ),
        ]);
        assert_eq!(
            estimated_phase_durations(&t),
            vec![
                (StartedPhase::InitialTesting, 5 * MIN),
                (StartedPhase::InitialTesting, 4 * MIN)
            ]
        );
        // The last span is still open: excluded from durations, reported as `since`.
        let mut live = t.clone();
        live.status = Status::Started;
        live.started_phase = Some(StartedPhase::InitialTesting);
        assert_eq!(
            started_phase_since(&live),
            Some(Timestamp::new("2026-10-01T00:25:00Z"))
        );
        live.started_phase = Some(StartedPhase::Working);
        assert_eq!(
            started_phase_since(&live),
            None,
            "history disagrees with phase"
        );
        live.started_phase = Some(StartedPhase::InitialTesting);
        live.status = Status::Completed;
        assert_eq!(started_phase_since(&live), None);
    }

    #[test]
    fn out_of_order_notes_sort_by_time_and_non_activity_is_ignored() {
        let mut t = ticket(&[
            (
                "2026-10-01T00:09:00Z",
                "Started phase changed from Final testing to Working",
            ),
            (
                "2026-10-01T00:00:00Z",
                "Started phase changed from Working to Final testing",
            ),
        ]);
        let mut regular = t.notes[0].clone();
        regular.kind = NoteKind::Regular;
        regular.created_at = Timestamp::new("2026-10-01T00:01:00Z");
        regular.text = "Status changed from Started to Completed".into();
        t.notes.push(regular);
        assert_eq!(
            estimated_phase_durations(&t),
            vec![(StartedPhase::FinalTesting, 9 * MIN)]
        );
    }

    #[test]
    fn status_reentry_without_phase_does_not_resume_a_span() {
        let t = ticket(&[
            (
                "2026-10-01T00:00:00Z",
                "Started phase changed from Working to Final testing",
            ),
            (
                "2026-10-01T00:03:00Z",
                "Status changed from Started to Not Started",
            ),
            (
                "2026-10-01T00:30:00Z",
                "Status changed from Not Started to Started",
            ),
        ]);
        assert_eq!(
            estimated_phase_durations(&t),
            vec![(StartedPhase::FinalTesting, 3 * MIN)]
        );
    }

    #[test]
    fn median_and_minimum_span_threshold() {
        assert_eq!(median_ms(vec![]), None);
        assert_eq!(median_ms(vec![5]), Some(5));
        assert_eq!(median_ms(vec![9, 1, 5]), Some(5));
        assert_eq!(median_ms(vec![1, 2, 3, 4]), Some(2));
        assert_eq!(median_ms(vec![3, 4]), Some(3));
        assert_eq!(median_ms(vec![u64::MAX, u64::MAX]), Some(u64::MAX));
        let two = [
            (StartedPhase::InitialTesting, 1),
            (StartedPhase::InitialTesting, 3),
        ];
        assert!(typical_phase_durations(two).is_empty());
        let typical = typical_phase_durations([
            (StartedPhase::InitialTesting, 10),
            (StartedPhase::InitialTesting, 30),
            (StartedPhase::InitialTesting, 20),
            (StartedPhase::FinalTesting, 7),
            (StartedPhase::Working, 1),
            (StartedPhase::Working, 1),
            (StartedPhase::Working, 1),
        ]);
        assert_eq!(typical.get(&StartedPhase::InitialTesting), Some(&20));
        assert_eq!(typical.get(&StartedPhase::FinalTesting), None);
        assert_eq!(typical.get(&StartedPhase::Working), None);
    }

    #[test]
    fn row_estimate_requires_started_testing_phase_with_since() {
        let mut typical = HashMap::new();
        typical.insert(StartedPhase::FinalTesting, 42);
        let mut t = ticket(&[(
            "2026-10-01T00:00:00Z",
            "Started phase changed from Integrating to Final testing",
        )]);
        t.status = Status::Started;
        t.started_phase = Some(StartedPhase::FinalTesting);
        let mut row = crate::TicketRow::from(&t);
        assert_eq!(
            row.started_phase_since.as_deref(),
            Some("2026-10-01T00:00:00Z")
        );
        apply_phase_estimate(&mut row, &typical);
        assert_eq!(row.started_phase_estimate_ms, Some(42));
        row.started_phase = Some("initial_testing".into());
        apply_phase_estimate(&mut row, &typical);
        assert_eq!(row.started_phase_estimate_ms, None);
        row.started_phase = Some("final_testing".into());
        row.started_phase_since = None;
        apply_phase_estimate(&mut row, &typical);
        assert_eq!(row.started_phase_estimate_ms, None);
    }
}
