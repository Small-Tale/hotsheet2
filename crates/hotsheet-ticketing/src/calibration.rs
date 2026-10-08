//! Completion-confidence calibration (HS2-RD4M29): compare the confidence an author reported
//! when completing a ticket with what happened next, so the rubric bands can be tuned.
//!
//! Each completion is one event, read from the ticket's own history: an automatic
//! "Status changed … to Completed" activity note closes a cycle, and its score is the newest
//! scored note of that cycle (the same rule as `ops::latest_confidence`). The outcome is
//! `reopened` if a later reopen (a transition back to Not Started/Started, or a Not Working
//! report) ends that cycle, `verified` if it reached Verified without a reopen, and
//! `pending` while it is still awaiting either.

use hotsheet_model::{Status, Ticket};
use serde::Serialize;

use crate::ops;

/// What happened after one completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Verified,
    Reopened,
    Pending,
}

/// One completion event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Completion {
    pub slug: String,
    pub completed_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<u8>,
    pub outcome: Outcome,
}

/// Totals for one rubric band (or for unscored completions).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct BandStats {
    /// `verified` (90-100), `assumed` (70-89), `partial` (40-69), `unverified` (<40), or
    /// `unscored`.
    pub band: &'static str,
    pub range: &'static str,
    pub completions: usize,
    pub verified: usize,
    pub reopened: usize,
    pub pending: usize,
    /// Reopened share of the completions with a known outcome; `None` until one resolves.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reopen_rate: Option<f64>,
    /// Mean reported score of the band's completions; `None` for unscored.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mean_confidence: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct CalibrationReport {
    pub completions: usize,
    pub scored: usize,
    pub bands: Vec<BandStats>,
    /// Every completion event, oldest first, for drill-down.
    pub events: Vec<Completion>,
}

const BANDS: [(&str, &str, u8, u8); 4] = [
    ("verified", "90-100", 90, 100),
    ("assumed", "70-89", 70, 89),
    ("partial", "40-69", 40, 69),
    ("unverified", "0-39", 0, 39),
];

/// The completion events of one ticket, from its history.
#[must_use]
pub fn ticket_completions(ticket: &Ticket) -> Vec<Completion> {
    let mut notes = ticket.notes.iter().collect::<Vec<_>>();
    notes.sort_by(|a, b| {
        a.created_at
            .chronological_cmp(&b.created_at)
            .unwrap_or_else(|| a.created_at.as_str().cmp(b.created_at.as_str()))
            .then(a.id.cmp(&b.id))
    });
    let mut completions: Vec<Completion> = Vec::new();
    let mut cycle_score = None;
    let mut open: Option<usize> = None;
    for note in notes {
        if let Some(score) = note.confidence {
            cycle_score = Some(score.get());
            if let Some(index) = open {
                completions[index].confidence = cycle_score;
            }
        }
        if ops::note_reopens_ticket(note) {
            if let Some(index) = open.take() {
                completions[index].outcome = Outcome::Reopened;
            }
            cycle_score = None;
            continue;
        }
        match ops::transition_target(note) {
            Some(Status::Completed) if open.is_none() => {
                completions.push(Completion {
                    slug: ticket.slug.clone(),
                    completed_at: note.created_at.as_str().to_owned(),
                    confidence: cycle_score,
                    outcome: Outcome::Pending,
                });
                open = Some(completions.len() - 1);
            }
            Some(Status::Verified) => {
                if open.is_none() {
                    // Verified straight from active work is also a completion.
                    completions.push(Completion {
                        slug: ticket.slug.clone(),
                        completed_at: note.created_at.as_str().to_owned(),
                        confidence: cycle_score,
                        outcome: Outcome::Pending,
                    });
                    open = Some(completions.len() - 1);
                }
                if let Some(index) = open {
                    completions[index].outcome = Outcome::Verified;
                }
            }
            _ => {}
        }
    }
    completions
}

/// Aggregate completion events across tickets into per-band calibration.
#[must_use]
pub fn calibration(tickets: &[Ticket]) -> CalibrationReport {
    let mut events = tickets
        .iter()
        .flat_map(ticket_completions)
        .collect::<Vec<_>>();
    events.sort_by(|a, b| {
        a.completed_at
            .cmp(&b.completed_at)
            .then(a.slug.cmp(&b.slug))
    });
    let tally = |band: &'static str, range: &'static str, members: Vec<&Completion>| {
        let count = |outcome| members.iter().filter(|e| e.outcome == outcome).count();
        let (verified, reopened, pending) = (
            count(Outcome::Verified),
            count(Outcome::Reopened),
            count(Outcome::Pending),
        );
        let scores = members
            .iter()
            .filter_map(|event| event.confidence)
            .map(f64::from)
            .collect::<Vec<_>>();
        BandStats {
            band,
            range,
            completions: members.len(),
            verified,
            reopened,
            pending,
            reopen_rate: (verified + reopened > 0)
                .then(|| reopened as f64 / (verified + reopened) as f64),
            mean_confidence: (!scores.is_empty())
                .then(|| scores.iter().sum::<f64>() / scores.len() as f64),
        }
    };
    let mut bands = BANDS
        .iter()
        .map(|(band, range, low, high)| {
            tally(
                band,
                range,
                events
                    .iter()
                    .filter(|e| e.confidence.is_some_and(|c| (*low..=*high).contains(&c)))
                    .collect(),
            )
        })
        .collect::<Vec<_>>();
    bands.push(tally(
        "unscored",
        "-",
        events.iter().filter(|e| e.confidence.is_none()).collect(),
    ));
    CalibrationReport {
        completions: events.len(),
        scored: events.iter().filter(|e| e.confidence.is_some()).count(),
        bands,
        events,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::{NoteMetadataInput, TicketPatch};
    use crate::{FsStore, NewTicket, StoreMetadata};
    use hotsheet_model::{Confidence, NoteKind, Timestamp, Ulid};

    struct Fixture {
        _dir: tempfile::TempDir,
        store: FsStore,
    }

    impl Fixture {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
            Self { _dir: dir, store }
        }
        fn ticket(&self, title: &str) -> Ulid {
            let id = Ulid::new();
            ops::create(
                &self.store,
                id,
                "HS",
                Timestamp::new("2026-09-01T00:00:00Z"),
                NewTicket {
                    title: title.into(),
                    ..Default::default()
                },
            )
            .unwrap();
            id
        }
        fn score(&self, id: &Ulid, at: &str, value: u64) {
            ops::add_note_with_metadata(
                &self.store,
                id,
                Ulid::new(),
                Timestamp::new(at),
                NoteKind::Regular,
                NoteMetadataInput {
                    human_edited: false,
                    summary: None,
                    confidence: Some(Confidence::new(value).unwrap()),
                    actor: None,
                    ai_feedback: None,
                },
                "## Confidence".into(),
            )
            .unwrap();
        }
        fn status(&self, id: &Ulid, at: &str, status: Status) {
            ops::update(
                &self.store,
                id,
                Timestamp::new(at),
                TicketPatch {
                    status: Some(status),
                    ..Default::default()
                },
            )
            .unwrap();
        }
        fn tickets(&self) -> Vec<Ticket> {
            self.store.list_tickets().unwrap()
        }
    }

    #[test]
    fn completions_follow_cycles_reopens_and_verification() {
        let f = Fixture::new();
        // Scored 95, completed, reopened; rescored 60, completed, verified.
        let a = f.ticket("cycled");
        f.score(&a, "2026-09-01T01:00:00Z", 95);
        f.status(&a, "2026-09-01T01:01:00Z", Status::Completed);
        f.status(&a, "2026-09-01T02:00:00Z", Status::Started);
        f.score(&a, "2026-09-01T03:00:00Z", 60);
        f.status(&a, "2026-09-01T03:01:00Z", Status::Completed);
        f.status(&a, "2026-09-01T04:00:00Z", Status::Verified);
        // Completed unscored and still pending; then a corrected score lands after.
        let b = f.ticket("pending");
        f.status(&b, "2026-09-01T05:00:00Z", Status::Completed);
        // Reopen clears the cycle score, so the next unscored completion stays unscored.
        let c = f.ticket("stale score");
        f.score(&c, "2026-09-01T06:00:00Z", 30);
        f.status(&c, "2026-09-01T06:01:00Z", Status::Completed);
        f.status(&c, "2026-09-01T07:00:00Z", Status::Started);
        f.status(&c, "2026-09-01T08:00:00Z", Status::Completed);
        f.score(&c, "2026-09-01T08:01:00Z", 85);

        let report = calibration(&f.tickets());
        let outcomes = report
            .events
            .iter()
            .map(|e| (e.confidence, e.outcome))
            .collect::<Vec<_>>();
        assert_eq!(
            outcomes,
            [
                (Some(95), Outcome::Reopened),
                (Some(60), Outcome::Verified),
                (None, Outcome::Pending),
                (Some(30), Outcome::Reopened),
                // A score written after the flip still belongs to that completion.
                (Some(85), Outcome::Pending),
            ]
        );
        assert_eq!(report.completions, 5);
        assert_eq!(report.scored, 4);
        let band = |name: &str| {
            report
                .bands
                .iter()
                .find(|b| b.band == name)
                .unwrap()
                .clone()
        };
        assert_eq!(band("verified").reopened, 1);
        assert_eq!(band("verified").reopen_rate, Some(1.0));
        assert_eq!(band("partial").verified, 1);
        assert_eq!(band("partial").reopen_rate, Some(0.0));
        assert_eq!(band("assumed").pending, 1);
        assert_eq!(band("assumed").reopen_rate, None);
        assert_eq!(band("assumed").mean_confidence, Some(85.0));
        assert_eq!(band("unverified").reopened, 1);
        assert_eq!(band("unscored").completions, 1);
        assert_eq!(band("unscored").mean_confidence, None);
    }

    #[test]
    fn an_empty_store_reports_empty_bands() {
        let report = calibration(&[]);
        assert_eq!(report.completions, 0);
        assert_eq!(report.bands.len(), 5);
        assert!(report.bands.iter().all(|band| band.completions == 0));
    }
}
