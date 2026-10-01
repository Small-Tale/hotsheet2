//! Provider-neutral comment encoding for Hot Sheet notes written to external trackers
//! (HS2-5YNASC).
//!
//! GitHub Issues, GitLab, and Jira comments have no note metadata, so a Hot Sheet-authored
//! comment carries it in plain text:
//!
//! ```text
//! <note text>
//!
//! Confidence: 82%
//!
//! <!-- hotsheet-note-id:<ulid> -->
//! ```
//!
//! The `Confidence: NN%` trailer is human-readable in the tracker and is parsed back into
//! the note's `confidence`. Parsing is deliberately strict so ordinary prose never becomes a
//! score: only a comment carrying the Hot Sheet note marker is considered, the trailer must
//! be the final non-empty line before the marker, and it must match exactly
//! `Confidence: <integer 0-100>%`.

use hotsheet_model::{Confidence, Status};
use hotsheet_ticketing::wire::ApiNote;

/// Prefix of the HTML comment that identifies a Hot Sheet-authored comment.
pub const NOTE_MARKER_PREFIX: &str = "<!-- hotsheet-note-id:";
const SEPARATOR_MARKER: &str = "\n\n<!-- hotsheet-note-id:";
const TRAILER_PREFIX: &str = "Confidence: ";

/// The idempotency marker for a note id.
pub fn note_marker(note_id: impl std::fmt::Display) -> String {
    format!("{NOTE_MARKER_PREFIX}{note_id} -->")
}

/// The comment body for a Hot Sheet note: its text, the optional confidence trailer, and
/// the idempotency marker.
pub fn compose_comment(
    text: &str,
    confidence: Option<Confidence>,
    note_id: impl std::fmt::Display,
) -> String {
    let marker = note_marker(note_id);
    match confidence {
        Some(confidence) => format!(
            "{text}\n\n{TRAILER_PREFIX}{}%\n\n{marker}",
            confidence.get()
        ),
        None => format!("{text}\n\n{marker}"),
    }
}

/// Split a comment body into its visible note text and the trailer's confidence.
/// Comments without the Hot Sheet marker are returned unchanged and never carry a score.
pub fn parse_comment(body: &str) -> (String, Option<u8>) {
    let Some((text, _)) = body.split_once(SEPARATOR_MARKER) else {
        return (body.to_owned(), None);
    };
    // The trailer is its own paragraph after non-empty note text (a scored note always has
    // text), so a note consisting only of `Confidence: NN%` prose is never misread.
    let parsed = text.rsplit_once("\n\n").and_then(|(before, last_line)| {
        let before = before.trim_end();
        (!before.is_empty())
            .then(|| parse_trailer(last_line))
            .flatten()
            .map(|confidence| (before.to_owned(), Some(confidence)))
    });
    parsed.unwrap_or_else(|| (text.to_owned(), None))
}

fn parse_trailer(line: &str) -> Option<u8> {
    let digits = line.strip_prefix(TRAILER_PREFIX)?.strip_suffix('%')?;
    if digits.is_empty() || digits.len() > 3 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Confidence::new(digits.parse().ok()?)
        .ok()
        .map(Confidence::get)
}

/// The derived completion confidence for a provider whose comments carry no reopen history:
/// a completed or verified ticket reports its newest scored note.
pub fn latest_confidence(status: Status, notes: &[ApiNote]) -> Option<u8> {
    if !matches!(status, Status::Completed | Status::Verified) {
        return None;
    }
    notes
        .iter()
        .filter(|note| note.confidence.is_some())
        .max_by(|left, right| left.created_at.as_str().cmp(right.created_at.as_str()))
        .and_then(|note| note.confidence)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn score(value: u64) -> Option<Confidence> {
        Some(Confidence::new(value).unwrap())
    }

    #[test]
    fn composed_comments_round_trip_text_and_score() {
        for (text, confidence) in [
            ("## Result\nDone.\n\n## Confidence\n82", score(82)),
            ("single line", score(0)),
            ("edge", score(100)),
            ("unscored", None),
            ("Confidence: 82%", None),
            ("first paragraph\n\nsecond paragraph", score(40)),
        ] {
            let body = compose_comment(text, confidence, "01ARZ3NDEKTSV4RRFFQ69G5FAV");
            assert!(body.ends_with("<!-- hotsheet-note-id:01ARZ3NDEKTSV4RRFFQ69G5FAV -->"));
            assert_eq!(
                parse_comment(&body),
                (text.to_owned(), confidence.map(Confidence::get)),
                "{body:?}"
            );
        }
        assert_eq!(
            compose_comment("Done.", score(82), "id"),
            "Done.\n\nConfidence: 82%\n\n<!-- hotsheet-note-id:id -->"
        );
    }

    #[test]
    fn ordinary_prose_never_becomes_a_score() {
        for body in [
            // No Hot Sheet marker: a human comment is never parsed.
            "Looks good.\n\nConfidence: 82%",
            // Not the last line before the marker.
            "Confidence: 82%\n\nMore text.\n\n<!-- hotsheet-note-id:x -->",
            // Not anchored: prose on the same line.
            "My Confidence: 82%\n\n<!-- hotsheet-note-id:x -->",
            "Confidence: 82% sure\n\n<!-- hotsheet-note-id:x -->",
            // Not an integer from 0 to 100.
            "Confidence: 101%\n\n<!-- hotsheet-note-id:x -->",
            "Confidence: 82.5%\n\n<!-- hotsheet-note-id:x -->",
            "Confidence: -1%\n\n<!-- hotsheet-note-id:x -->",
            "Confidence: %\n\n<!-- hotsheet-note-id:x -->",
            "Confidence: 0082%\n\n<!-- hotsheet-note-id:x -->",
            "Confidence: 82\n\n<!-- hotsheet-note-id:x -->",
            "confidence: 82%\n\n<!-- hotsheet-note-id:x -->",
            // The trailer must be its own paragraph, not the tail of one...
            "We are done.\nConfidence: 82%\n\n<!-- hotsheet-note-id:x -->",
            // ...and must follow note text.
            "Confidence: 82%\n\n<!-- hotsheet-note-id:x -->",
            "\n\nConfidence: 82%\n\n<!-- hotsheet-note-id:x -->",
        ] {
            let (text, confidence) = parse_comment(body);
            assert_eq!(confidence, None, "{body:?}");
            let expected = body.split(SEPARATOR_MARKER).next().unwrap();
            assert_eq!(text, expected, "{body:?}");
        }
    }

    #[test]
    fn latest_confidence_reports_the_newest_scored_note_of_a_completed_ticket() {
        let note = |created_at: &str, confidence: Option<u8>| ApiNote {
            id: created_at.into(),
            kind: hotsheet_model::NoteKind::Regular,
            created_at: created_at.into(),
            edited_at: created_at.into(),
            summary: None,
            confidence,
            text: String::new(),
        };
        let notes = [
            note("2026-08-26T00:00:00Z", Some(40)),
            note("2026-08-26T00:02:00Z", Some(90)),
            note("2026-08-26T00:03:00Z", None),
            note("2026-08-26T00:01:00Z", Some(60)),
        ];
        assert_eq!(latest_confidence(Status::Completed, &notes), Some(90));
        assert_eq!(latest_confidence(Status::Verified, &notes), Some(90));
        assert_eq!(latest_confidence(Status::Started, &notes), None);
        assert_eq!(latest_confidence(Status::Completed, &notes[2..3]), None);
    }
}
