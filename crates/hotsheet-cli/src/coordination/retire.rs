//! Retire stale pool records whose ticket is already closed (HS2-V9BXWF).
//!
//! A closed (completed, verified, archived, deleted or moved) ticket can no longer reach
//! `done` through the integrator transition table when its record is a review return or
//! a findings handoff, because verification needs a claim and claims refuse closed
//! tickets. This one supported step records an inspection note on each such ticket and
//! moves the record to `done`, keeping it for audit, so nobody edits the pool file.

use super::*;
use hotsheet_model::{NoteActor, NoteKind, Status, Ticket, Timestamp, Ulid};
use serde::{Deserialize, Serialize};

/// First line of the durable inspection note that authorizes one retirement.
pub const RETIREMENT_HEADING: &str = "## Coordination retirement";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetiredAssignment {
    pub assignment: String,
    pub worker: String,
    pub ticket: TicketRef,
    pub slug: String,
    pub previous_phase: Phase,
    pub ticket_status: Status,
    pub review_returned: bool,
    /// The inspection note; `None` only in a dry run.
    pub note: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkippedRetirement {
    pub assignment: String,
    pub ticket: TicketRef,
    pub reason: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetirementReport {
    pub dry_run: bool,
    pub retired: Vec<RetiredAssignment>,
    /// Closed or unverifiable tickets left untouched, each with an explicit reason.
    pub skipped: Vec<SkippedRetirement>,
}

fn wire_name<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}

enum Candidate {
    Retire(Box<Ticket>),
    Skip(String),
    Open,
}

impl Coordinator {
    /// Retire every non-done assignment whose selected-source ticket is closed and has no
    /// live claim. Idempotent: a rerun finds nothing, and a run interrupted after writing a
    /// note reuses that note instead of adding another.
    pub fn retire_finished(
        &self,
        integrator: &str,
        generation: &str,
        now: time::OffsetDateTime,
        actor: Option<NoteActor>,
        dry_run: bool,
    ) -> Result<RetirementReport> {
        ensure!(
            self.source
                .join(hotsheet_ticketing::STORE_METADATA_FILE)
                .exists(),
            "retiring finished assignments needs the selected local ticket source; provider ticket status is unavailable (HS2-3ETJ26)"
        );
        let store = FsStore::open_without_maintenance(&self.source)?;
        let connection = git_connection_id(&store);
        let state = self.state()?;
        Self::integrator(&state, integrator, generation, now.unix_timestamp())?;
        let mut report = RetirementReport {
            dry_run,
            ..RetirementReport::default()
        };
        for a in state.assignments.values().filter(|a| a.phase.pending()) {
            let ticket = match Self::retirement_candidate(&store, &connection, a, now) {
                Candidate::Open => continue,
                Candidate::Skip(reason) => {
                    report.skipped.push(SkippedRetirement {
                        assignment: a.id.clone(),
                        ticket: a.ticket.clone(),
                        reason,
                    });
                    continue;
                }
                Candidate::Retire(ticket) => ticket,
            };
            let note = if dry_run {
                None
            } else {
                Some(Self::retirement_note(
                    &store,
                    &ticket,
                    a,
                    actor.clone(),
                    now,
                )?)
            };
            report.retired.push(RetiredAssignment {
                assignment: a.id.clone(),
                worker: a.worker.clone(),
                ticket: a.ticket.clone(),
                slug: ticket.slug.clone(),
                previous_phase: a.phase,
                ticket_status: ticket.status,
                review_returned: a.returned_for_review(),
                note,
            });
        }
        if dry_run || report.retired.is_empty() {
            return Ok(report);
        }
        // Ticket transaction, then pool lock: the order review returns and reservations use.
        // Everything is checked again so a reopen, claim or concurrent transition after the
        // notes were written leaves that record untouched.
        store.with_ticket_transaction(|| {
            self.change(|state| {
                Self::integrator(state, integrator, generation, now.unix_timestamp())?;
                let mut confirmed = Vec::new();
                for retired in std::mem::take(&mut report.retired) {
                    let current = state.assignments.get(&retired.assignment);
                    let still = current.is_some_and(|a| a.phase == retired.previous_phase)
                        && current.is_some_and(|a| {
                            matches!(
                                Self::retirement_candidate(&store, &connection, a, now),
                                Candidate::Retire(_)
                            )
                        });
                    if !still {
                        report.skipped.push(SkippedRetirement {
                            assignment: retired.assignment.clone(),
                            ticket: retired.ticket.clone(),
                            reason: "assignment or ticket changed during retirement; rerun to re-inspect".into(),
                        });
                        continue;
                    }
                    let a = state
                        .assignments
                        .get_mut(&retired.assignment)
                        .expect("assignment checked above");
                    a.phase = Phase::Done;
                    a.retirement_note.clone_from(&retired.note);
                    a.record_transition(TransitionKind::Done, now, Role::Integrator, integrator)?;
                    confirmed.push(retired);
                }
                report.retired = confirmed;
                Ok(())
            })
        })?;
        Ok(report)
    }

    fn retirement_candidate(
        store: &FsStore,
        connection: &str,
        a: &Assignment,
        now: time::OffsetDateTime,
    ) -> Candidate {
        if a.ticket.connection_id != connection {
            return Candidate::Skip(
                "ticket belongs to another source; its status cannot be verified here".into(),
            );
        }
        let Ok(id) = a.ticket.native_id.parse::<Ulid>() else {
            return Candidate::Skip("assignment names an invalid local ticket id".into());
        };
        let ticket = match store.read_ticket(&id) {
            Ok(ticket) => ticket,
            Err(error) => return Candidate::Skip(format!("ticket could not be read: {error}")),
        };
        if ops::is_open(&ticket) {
            return Candidate::Open;
        }
        if !ops::claim_available(&ticket, &Timestamp::from_datetime(now)) {
            return Candidate::Skip("closed ticket still has a live claim".into());
        }
        Candidate::Retire(Box::new(ticket))
    }

    /// Reuse the regular inspection note naming this assignment, or append one.
    fn retirement_note(
        store: &FsStore,
        ticket: &Ticket,
        a: &Assignment,
        actor: Option<NoteActor>,
        now: time::OffsetDateTime,
    ) -> Result<String> {
        if let Some(note) = ticket.notes.iter().find(|note| {
            note.kind == NoteKind::Regular
                && note.text.lines().next() == Some(RETIREMENT_HEADING)
                && note.text.split_whitespace().any(|word| word == a.id)
        }) {
            return Ok(note.id.to_string());
        }
        let evidence = match (&a.handoff, &a.commit) {
            (Some(HandoffEvidence::Code { commit }), _) | (None, Some(commit)) => {
                format!("code commit {commit}")
            }
            (Some(HandoffEvidence::Findings { note_id }), _) => format!("findings note {note_id}"),
            (None, None) => "none".into(),
        };
        let text = format!(
            "{RETIREMENT_HEADING}\n\n\
             - assignment: {id}\n\
             - worker: {worker} (generation {generation})\n\
             - previous phase: {phase}\n\
             - ticket status: {status}\n\
             - handoff evidence: {evidence}\n\
             - recovery or review-return note: {review}\n\n\
             This ticket is closed, so the coordination record no longer holds work, \
             integration or CI. It is kept for audit with phase done.",
            id = a.id,
            worker = a.worker,
            generation = a.generation,
            phase = wire_name(&a.phase),
            status = wire_name(&ticket.status),
            review = a.recovery_note.as_deref().unwrap_or("none"),
        );
        let note_id = Ulid::new();
        ops::add_note_with_metadata(
            store,
            &ticket.id,
            note_id,
            Timestamp::from_datetime(now),
            NoteKind::Regular,
            ops::NoteMetadataInput {
                kind_explicit: true,
                summary: Some(format!("Retired stale coordination assignment {}", a.id)),
                actor,
                ..ops::NoteMetadataInput::default()
            },
            text,
        )?;
        Ok(note_id.to_string())
    }
}
