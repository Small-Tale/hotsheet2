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

use hotsheet_model::{Confidence, Status, Timestamp};
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

/// The derived completion confidence of an external ticket: a completed or verified ticket
/// reports its newest scored note written in the current completion cycle, i.e. strictly
/// after `last_reopen` (the tracker's latest reopen, when it has one). This mirrors the git
/// provider's `ops::latest_confidence`: an old score never survives a reopen unless the next
/// completion reports a new one, while a completing note written just before the close
/// still counts (HS2-N3RMTV).
pub fn latest_confidence(
    status: Status,
    notes: &[ApiNote],
    last_reopen: Option<&str>,
) -> Option<u8> {
    if !matches!(status, Status::Completed | Status::Verified) {
        return None;
    }
    notes
        .iter()
        .filter(|note| note.confidence.is_some())
        .filter(|note| {
            last_reopen.is_none_or(|reopen| {
                chronological_cmp(&note.created_at, reopen) == std::cmp::Ordering::Greater
            })
        })
        .max_by(|left, right| chronological_cmp(&left.created_at, &right.created_at))
        .and_then(|note| note.confidence)
}

/// Whether a detail read must fetch the tracker's reopen history: only a completed or
/// verified ticket with at least one scored comment has a score a reopen could invalidate,
/// so every other read skips the extra request.
pub fn needs_reopen_history(status: Status, notes: &[ApiNote]) -> bool {
    matches!(status, Status::Completed | Status::Verified)
        && notes.iter().any(|note| note.confidence.is_some())
}

/// The latest of a tracker's reopen timestamps.
pub fn last_reopen<'a>(reopens: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    reopens.into_iter().max_by(|a, b| chronological_cmp(a, b))
}

/// Chronological order of two tracker timestamps, falling back to text order when either
/// is unparseable. Jira's `+0000` offsets are normalised to RFC3339 `+00:00` first.
pub fn chronological_cmp(left: &str, right: &str) -> std::cmp::Ordering {
    timestamp(left)
        .chronological_cmp(&timestamp(right))
        .unwrap_or_else(|| left.cmp(right))
}

fn timestamp(raw: &str) -> Timestamp {
    let parsed = Timestamp::new(raw);
    if parsed.is_valid() {
        return parsed;
    }
    let bytes = raw.as_bytes();
    let offset = bytes.len().checked_sub(5).map(|start| &bytes[start..]);
    match offset {
        Some([b'+' | b'-', rest @ ..]) if rest.iter().all(u8::is_ascii_digit) => {
            let split = raw.len() - 2;
            Timestamp::new(format!("{}:{}", &raw[..split], &raw[split..]))
        }
        _ => parsed,
    }
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
            actor: None,
            text: String::new(),
        };
        let notes = [
            note("2026-08-26T00:00:00Z", Some(40)),
            note("2026-08-26T00:02:00Z", Some(90)),
            note("2026-08-26T00:03:00Z", None),
            note("2026-08-26T00:01:00Z", Some(60)),
        ];
        assert_eq!(latest_confidence(Status::Completed, &notes, None), Some(90));
        assert_eq!(latest_confidence(Status::Verified, &notes, None), Some(90));
        assert_eq!(latest_confidence(Status::Started, &notes, None), None);
        assert_eq!(
            latest_confidence(Status::Completed, &notes[2..3], None),
            None
        );
    }

    fn note(created_at: &str, confidence: Option<u8>) -> ApiNote {
        ApiNote {
            id: created_at.into(),
            kind: hotsheet_model::NoteKind::Regular,
            created_at: created_at.into(),
            edited_at: created_at.into(),
            summary: None,
            confidence,
            actor: None,
            text: String::new(),
        }
    }

    /// HS2-N3RMTV: the completion-cycle transition matrix. Complete (scored) → reopen →
    /// re-complete without a score reports nothing; a new score after the reopen wins; a
    /// second reopen discards that one too; a completing note written before the close but
    /// after the reopen still counts.
    #[test]
    fn a_reopen_bounds_the_score_to_the_current_completion_cycle() {
        let first = note("2026-08-26T00:01:00Z", Some(80));
        let reopen = "2026-08-26T00:02:00Z";
        let unscored = note("2026-08-26T00:03:00Z", None);
        assert_eq!(
            latest_confidence(Status::Completed, std::slice::from_ref(&first), None),
            Some(80)
        );
        assert_eq!(
            latest_confidence(
                Status::Completed,
                &[first.clone(), unscored.clone()],
                Some(reopen)
            ),
            None,
            "re-completed without a new score"
        );
        let second = note("2026-08-26T00:04:00Z", Some(95));
        let both = [first.clone(), unscored, second.clone()];
        assert_eq!(
            latest_confidence(Status::Verified, &both, Some(reopen)),
            Some(95)
        );
        // Reopened again after the second score: nothing survives.
        let reopens = ["2026-08-26T00:02:00Z", "2026-08-26T00:05:00Z"];
        assert_eq!(
            latest_confidence(Status::Completed, &both, last_reopen(reopens)),
            None
        );
        // A note exactly at the reopen instant belongs to the old cycle (strictly after).
        let at_reopen = note(reopen, Some(70));
        assert_eq!(
            latest_confidence(Status::Completed, &[at_reopen], Some(reopen)),
            None
        );
        // Reopened (no longer completed): never a score.
        assert_eq!(latest_confidence(Status::Started, &[second], None), None);
    }

    #[test]
    fn history_is_needed_only_for_a_scored_completed_ticket() {
        let scored = [note("2026-08-26T00:01:00Z", Some(80))];
        let unscored = [note("2026-08-26T00:01:00Z", None)];
        assert!(needs_reopen_history(Status::Completed, &scored));
        assert!(needs_reopen_history(Status::Verified, &scored));
        assert!(!needs_reopen_history(Status::Completed, &unscored));
        assert!(!needs_reopen_history(Status::Completed, &[]));
        assert!(!needs_reopen_history(Status::Started, &scored));
        assert!(!needs_reopen_history(Status::NotStarted, &scored));
    }

    #[test]
    fn tracker_timestamps_compare_chronologically_across_offsets() {
        use std::cmp::Ordering;
        // Jira's `+0000`/`+0800` offsets are not RFC3339 but still order by instant.
        assert_eq!(
            chronological_cmp(
                "2026-08-26T08:00:00.000+0800",
                "2026-08-26T00:30:00.000+0000"
            ),
            Ordering::Less
        );
        assert_eq!(
            chronological_cmp("2026-08-26T00:00:00Z", "2026-08-26T00:00:00.000+0000"),
            Ordering::Equal
        );
        assert_eq!(
            chronological_cmp("2026-08-26T01:00:00.000Z", "2026-08-26T00:59:59Z"),
            Ordering::Greater
        );
        // Unparseable text falls back to text order instead of panicking.
        assert_eq!(chronological_cmp("b", "a"), Ordering::Greater);
        assert_eq!(
            last_reopen([
                "2026-08-26T09:00:00.000+0800",
                "2026-08-26T02:00:00.000+0000"
            ]),
            Some("2026-08-26T02:00:00.000+0000")
        );
        assert_eq!(last_reopen([]), None);
    }
}
