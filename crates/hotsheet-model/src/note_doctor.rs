//! Read-only, deterministic note diagnostics (HS2-F1QJ03).

use serde::Serialize;

use crate::enums::NoteKind;
use crate::feedback_choices::parse_feedback_choices;
use crate::{Note, Ticket, Ulid};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NoteFinding {
    pub id: &'static str,
    pub note_id: Ulid,
    pub description: &'static str,
    pub proposed_operation: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NoteDoctorReport {
    pub ticket_id: Ulid,
    pub findings: Vec<NoteFinding>,
}

fn finding(
    note: &Note,
    id: &'static str,
    description: &'static str,
    proposed_operation: &'static str,
) -> NoteFinding {
    NoteFinding {
        id,
        note_id: note.id,
        description,
        proposed_operation,
    }
}

fn reads_as_decision(text: &str) -> bool {
    text.lines()
        .filter(|line| !line.trim_start().starts_with('>'))
        .any(|line| {
            let line = line.trim().to_ascii_lowercase();
            [
                "decision:",
                "i choose ",
                "we choose ",
                "let's use ",
                "go with ",
                "approved:",
            ]
            .iter()
            .any(|prefix| line.starts_with(prefix))
        })
}

/// Analyze one note in the context of its ticket, or all notes when `note_id` is absent.
/// Findings are ordered by note order and then by the checks below. A note filter never
/// changes the context used to identify replies or details pointers.
pub fn diagnose_notes(ticket: &Ticket, note_id: Option<Ulid>) -> NoteDoctorReport {
    let mut findings = Vec::new();
    let mut unanswered: Option<&Note> = None;
    let request_ids: std::collections::HashSet<_> = ticket
        .notes
        .iter()
        .filter(|note| note.feedback_request || note.is_feedback_needed_request())
        .map(|note| note.id)
        .collect();
    let details_pointer = Note::details_request_feedback(&ticket.details);
    for note in &ticket.notes {
        let mut current = Vec::new();
        if Note::explicit_kind_suppresses_feedback_marker(
            note.kind,
            note.kind_explicit,
            note.response_to.is_some(),
            &note.text,
        ) {
            current.push(finding(
                note,
                "suppressed_feedback_marker",
                "An explicitly regular note contains an unquoted FEEDBACK NEEDED marker.",
                "Change the note kind to feedback_needed or remove the marker.",
            ));
        }
        if note.kind == NoteKind::FeedbackNeeded && !note.feedback_request {
            current.push(finding(
                note,
                "missing_feedback_request",
                "A feedback_needed note has no feedback_request flag.",
                "Mark this note as a feedback request.",
            ));
        }
        if note.is_feedback_needed_request()
            && note.text.contains("CHOICE")
            && parse_feedback_choices(&note.text).is_none()
        {
            current.push(finding(
                note,
                "unparsed_choice",
                "A feedback note contains CHOICE but no choice group parses.",
                "Repair the CHOICE syntax or remove the token.",
            ));
        }
        if note
            .response_to
            .is_some_and(|id| !request_ids.contains(&id))
        {
            current.push(finding(
                note,
                "orphaned_response",
                "The response_to target is not a feedback request in this ticket.",
                "Link this reply to an existing request or clear response_to.",
            ));
        }
        if matches!(note.kind, NoteKind::Activity | NoteKind::Status)
            && !note
                .summary
                .as_ref()
                .is_some_and(|summary| !summary.trim().is_empty())
        {
            current.push(finding(
                note,
                "missing_summary",
                "An activity or status note has no summary.",
                "Add a short plain-text note summary.",
            ));
        }
        if note.text.contains("\\n") {
            current.push(finding(
                note,
                "literal_backslash_n",
                "The note contains literal backslash-n text.",
                "Replace literal backslash-n with line breaks if it was accidental.",
            ));
        }
        if details_pointer && note.is_feedback_needed_request() {
            current.push(finding(note, "duplicate_details_pointer", "Ticket details also carry a FEEDBACK NEEDED pointer.", "Remove the redundant pointer from details after confirming this note is the request."));
        }
        if let Some(request) = unanswered {
            if note.response_to.is_none()
                && note.kind == NoteKind::Regular
                && reads_as_decision(&note.text)
            {
                current.push(finding(note, "possible_unlinked_decision", "This note reads as a decision after an unanswered feedback request but has no response_to link.", "Review the note and link it to the request if it answers it."));
            }
            if note.response_to == Some(request.id) {
                unanswered = None;
            }
        }
        if note.feedback_request {
            unanswered = Some(note);
        }
        if note_id.is_none_or(|id| id == note.id) {
            findings.extend(current);
        }
    }
    NoteDoctorReport {
        ticket_id: ticket.id,
        findings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Timestamp;

    fn note(number: u8, kind: NoteKind, text: &str) -> Note {
        let id = format!("01ARZ3NDEKTSV4RRFFQ69G5F{:02}", number);
        Note {
            id: Ulid::from_string(&id).unwrap(),
            kind,
            kind_explicit: false,
            created_at: Timestamp::new("2026-01-01T00:00:00Z"),
            edited_at: Timestamp::new("2026-01-01T00:00:00Z"),
            summary: None,
            confidence: None,
            feedback_for: None,
            feedback_request: false,
            response_to: None,
            ai_feedback: None,
            human_edited: false,
            actor: None,
            text: text.into(),
        }
    }

    fn ids(ticket: &Ticket) -> Vec<&'static str> {
        diagnose_notes(ticket, None)
            .findings
            .into_iter()
            .map(|finding| finding.id)
            .collect()
    }

    #[test]
    fn hs2_f1qj03_flags_each_problem_and_ignores_repaired_shapes() {
        let request = note(1, NoteKind::FeedbackNeeded, "FEEDBACK NEEDED\nCHOICE maybe");
        let mut suppressed = note(2, NoteKind::Regular, "FEEDBACK NEEDED");
        suppressed.kind_explicit = true;
        let mut orphan = note(3, NoteKind::Regular, "Answer");
        orphan.response_to = Some(note(9, NoteKind::Regular, "").id);
        let summaryless = note(4, NoteKind::Activity, "progress");
        let literal = note(5, NoteKind::Regular, r"line one\nline two");
        let ticket = Ticket {
            details: "FEEDBACK NEEDED: see note".into(),
            notes: vec![request.clone(), suppressed, orphan, summaryless, literal],
            ..Ticket::default()
        };
        let actual = ids(&ticket);
        for id in [
            "missing_feedback_request",
            "unparsed_choice",
            "duplicate_details_pointer",
            "suppressed_feedback_marker",
            "orphaned_response",
            "missing_summary",
            "literal_backslash_n",
        ] {
            assert!(actual.contains(&id), "missing {id}: {actual:?}");
        }
        let mut repaired = ticket;
        repaired.details.clear();
        repaired.notes[0].feedback_request = true;
        repaired.notes[0].text = "FEEDBACK NEEDED\nCHOICE A: One\nCHOICE B: Two".into();
        repaired.notes[1].text = "> FEEDBACK NEEDED".into();
        repaired.notes[2].response_to = Some(request.id);
        repaired.notes[3].summary = Some("Progress".into());
        repaired.notes[4].text = "line one\nline two".into();
        assert!(ids(&repaired).is_empty(), "{:?}", ids(&repaired));
    }

    #[test]
    fn hs2_f1qj03_decision_requires_unanswered_request_and_note_filter_keeps_context() {
        let mut request = note(1, NoteKind::FeedbackNeeded, "FEEDBACK NEEDED");
        request.feedback_request = true;
        let decision = note(2, NoteKind::Regular, "Decision: use A");
        let mut ticket = Ticket {
            notes: vec![request.clone(), decision.clone()],
            ..Ticket::default()
        };
        assert!(ids(&ticket).contains(&"possible_unlinked_decision"));
        let selected = diagnose_notes(&ticket, Some(decision.id));
        assert_eq!(selected.findings.len(), 1);
        assert_eq!(selected.findings[0].note_id, decision.id);
        ticket.notes[1].response_to = Some(request.id);
        assert!(!ids(&ticket).contains(&"possible_unlinked_decision"));
        ticket.notes[1].response_to = None;
        ticket.notes[0].feedback_request = false;
        ticket.notes[0].kind = NoteKind::Regular;
        assert!(!ids(&ticket).contains(&"possible_unlinked_decision"));
    }
}
