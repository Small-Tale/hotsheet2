//! Read/write the on-disk ticket file: **YAML frontmatter** + a Markdown **body**
//! (`details`) + an optional `## Notes` section. This is the parser the whole system
//! is built against (`docs/17-ticket-file-format.md` §17.4) and the migrator is
//! conformance-tested against (`docs/07` §7.2.1).
//!
//! Guarantees:
//! - **Round-trip stable** — `parse_file(to_file_string(t))` reproduces `t`, and
//!   re-serializing is byte-identical for a canonical file.
//! - **Unknown frontmatter keys preserved** in [`Ticket::extra`] (forward-compat).
//! - **Never panics** on arbitrary input — malformed files return a [`ParseError`];
//!   individual malformed notes are skipped rather than aborting the parse.

use serde_yaml::{Mapping, Value};

use crate::enums::NoteKind;
use crate::ids::Ulid;
use crate::ticket::{Note, Ticket};
use crate::timestamp::Timestamp;

/// Frontmatter keys the current schema defines. Anything else parsed from a file's
/// frontmatter is retained in [`Ticket::extra`]. Kept in sync with `Ticket`'s fields
/// by the `known_keys_cover_serialized_fields` test.
const KNOWN_KEYS: &[&str] = &[
    "id",
    "slug",
    "title",
    "category",
    "priority",
    "status",
    "up_next",
    "tags",
    "blocked_by",
    "blocked_reason",
    "created_at",
    "updated_at",
    "completed_at",
    "verified_at",
    "closed_at",
    "close_reason",
    "duplicate_of",
    "claimed_by",
    "claim_lease_expires_at",
    "worker_label",
    "claim_count",
    "assignees",
    "review_requests",
    "external",
    "moved_to_store",
    "legacy_number",
    "copied_from",
    "schema",
];

/// An error parsing a ticket file. Returned (never panicked) for any malformed input.
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    /// The file does not begin with a `---` frontmatter fence.
    #[error("file does not begin with a '---' YAML frontmatter fence")]
    MissingFrontmatter,
    /// The opening fence is never closed by a `---` line.
    #[error("frontmatter fence is not terminated by a closing '---'")]
    UnterminatedFrontmatter,
    /// The frontmatter is valid YAML but not a mapping of fields.
    #[error("frontmatter is not a YAML mapping")]
    FrontmatterNotMapping,
    /// The frontmatter is not valid YAML, or a required field is missing/mistyped.
    #[error("invalid frontmatter: {0}")]
    Yaml(#[from] serde_yaml::Error),
}

/// Serialize a ticket to its canonical on-disk form.
pub fn to_file_string(t: &Ticket) -> String {
    let frontmatter = frontmatter_to_string(t);

    let mut out = String::with_capacity(frontmatter.len() + t.details.len() + 64);
    out.push_str("---\n");
    out.push_str(&frontmatter); // serde_yaml output ends in '\n'
    out.push_str("---\n");

    let details = t.details.trim();
    if !details.is_empty() {
        out.push('\n');
        out.push_str(details);
        out.push('\n');
    }

    // Skip local-only feedback drafts and content-less notes: an empty-text note
    // can't be distinguished from its own timestamp on re-parse, so it isn't a
    // shape the shared file represents (proptest regression).
    let notes: Vec<&Note> = t
        .notes
        .iter()
        .filter(|n| n.kind != NoteKind::FeedbackDraft && !n.text.trim().is_empty())
        .collect();
    if !notes.is_empty() {
        out.push('\n');
        out.push_str(&notes_to_string(&notes));
    }

    out
}

/// Parse a ticket from its on-disk form.
pub fn parse_file(text: &str) -> Result<Ticket, ParseError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text); // tolerate a BOM
    let normalized = text.replace("\r\n", "\n");

    let rest = normalized
        .strip_prefix("---\n")
        .ok_or(ParseError::MissingFrontmatter)?;
    let (frontmatter, after) = match rest.split_once("\n---\n") {
        Some((fm, after)) => (fm, after),
        // A frontmatter-only file that ends right at the closing fence.
        None => (
            rest.strip_suffix("\n---")
                .ok_or(ParseError::UnterminatedFrontmatter)?,
            "",
        ),
    };

    let value: Value = serde_yaml::from_str(frontmatter)?;
    let mapping = value
        .as_mapping()
        .ok_or(ParseError::FrontmatterNotMapping)?
        .clone();

    let mut ticket: Ticket = serde_yaml::from_value(value)?;

    // Retain keys the current schema doesn't know (forward-compat, docs/17 §17.4).
    for (k, v) in &mapping {
        if let Some(key) = k.as_str() {
            if !KNOWN_KEYS.contains(&key) {
                ticket.extra.insert(key.to_string(), v.clone());
            }
        }
    }

    let (body, notes_section) = split_body_and_notes(after);
    ticket.details = body;
    ticket.notes = notes_section.map(parse_notes).unwrap_or_default();

    Ok(ticket)
}

// ---- frontmatter -----------------------------------------------------------------

fn frontmatter_to_string(t: &Ticket) -> String {
    let value = serde_yaml::to_value(t).expect("a Ticket always serializes to YAML");
    let mut mapping = match value {
        Value::Mapping(m) => m,
        _ => Mapping::new(),
    };
    // Unknown keys are re-emitted after the known ones, in sorted (BTreeMap) order.
    for (k, v) in &t.extra {
        mapping.insert(Value::String(k.clone()), v.clone());
    }
    serde_yaml::to_string(&Value::Mapping(mapping)).expect("a YAML mapping always serializes")
}

// ---- body / notes split ----------------------------------------------------------

/// Split the post-frontmatter region into the trimmed body and an optional notes
/// section (everything from a line that is exactly `## Notes`).
fn split_body_and_notes(after: &str) -> (String, Option<&str>) {
    match find_notes_header(after) {
        Some(idx) => (after[..idx].trim().to_string(), Some(&after[idx..])),
        None => (after.trim().to_string(), None),
    }
}

fn find_notes_header(s: &str) -> Option<usize> {
    let mut offset = 0;
    for line in s.split_inclusive('\n') {
        if line.strip_suffix('\n').unwrap_or(line) == "## Notes" {
            return Some(offset);
        }
        offset += line.len();
    }
    None
}

fn parse_notes(section: &str) -> Vec<Note> {
    let body = section.strip_prefix("## Notes").unwrap_or(section);
    let lines: Vec<&str> = body.lines().collect();

    let mut notes = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if let Some((id, kind)) = parse_note_marker(lines[i].trim()) {
            i += 1;
            let start = i;
            while i < lines.len() && parse_note_marker(lines[i].trim()).is_none() {
                i += 1;
            }
            if let Some(note) = build_note(id, kind, lines[start..i].join("\n").trim()) {
                notes.push(note);
            }
        } else {
            i += 1;
        }
    }
    notes
}

/// Parse `<!-- note: <ulid> [kind: <kind>] -->`. Returns `None` for a non-marker line
/// or an unparseable id (that block is then skipped — degrade, don't panic).
fn parse_note_marker(line: &str) -> Option<(Ulid, NoteKind)> {
    let inner = line
        .strip_prefix("<!--")?
        .strip_suffix("-->")?
        .trim()
        .strip_prefix("note:")?
        .trim();

    let mut tokens = inner.split_whitespace();
    let id = Ulid::from_string(tokens.next()?).ok()?;

    let mut kind = NoteKind::Regular;
    let rest: Vec<&str> = tokens.collect();
    if let Some(pos) = rest.iter().position(|t| *t == "kind:") {
        if let Some(k) = rest.get(pos + 1) {
            kind = parse_note_kind(k);
        }
    } else if let Some(k) = rest.iter().find_map(|t| t.strip_prefix("kind:")) {
        kind = parse_note_kind(k);
    }
    Some((id, kind))
}

fn build_note(id: Ulid, kind: NoteKind, block: &str) -> Option<Note> {
    if block.is_empty() {
        return None;
    }
    let (at, text) = match block.split_once(" — ") {
        Some((at, text)) => (Timestamp::new(at.trim()), text.trim().to_string()),
        None => (Timestamp::default(), block.to_string()),
    };
    Some(Note { id, kind, at, text })
}

fn notes_to_string(notes: &[&Note]) -> String {
    let mut out = String::from("## Notes\n");
    for n in notes {
        out.push_str("\n<!-- note: ");
        out.push_str(&n.id.to_string());
        if n.kind != NoteKind::Regular {
            out.push_str(" kind: ");
            out.push_str(note_kind_str(n.kind));
        }
        out.push_str(" -->\n");
        if n.at.is_empty() {
            out.push_str(&n.text);
        } else {
            out.push_str(n.at.as_str());
            out.push_str(" — ");
            out.push_str(&n.text);
        }
        out.push('\n');
    }
    out
}

fn note_kind_str(kind: NoteKind) -> &'static str {
    match kind {
        NoteKind::Regular => "regular",
        NoteKind::FeedbackNeeded => "feedback_needed",
        NoteKind::FeedbackDraft => "feedback_draft",
        NoteKind::Status => "status",
    }
}

fn parse_note_kind(s: &str) -> NoteKind {
    match s {
        "feedback_needed" => NoteKind::FeedbackNeeded,
        "feedback_draft" => NoteKind::FeedbackDraft,
        "status" => NoteKind::Status,
        _ => NoteKind::Regular,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enums::{CloseReason, Priority, ReviewKind, Status};
    use crate::ids::derive_slug;
    use crate::ticket::{ExternalLink, ReviewRequest};

    fn ulid(s: &str) -> Ulid {
        Ulid::from_string(s).unwrap()
    }

    fn sample() -> Ticket {
        let id = ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV");
        let mut t = Ticket::new(
            id,
            derive_slug(&id, "HS"),
            "Fix the dashboard flicker on project switch",
            "bug",
            "2026-08-19T14:03:11Z",
            "2026-08-19T15:20:44Z",
        );
        t.priority = Priority::High;
        t.status = Status::Started;
        t.up_next = true;
        t.tags = vec!["dashboard".into(), "ui".into()];
        t.assignees = vec!["alex@example.com".into()];
        t.details =
            "The dashboard flashes white for one frame when switching projects.".to_string();
        t.notes = vec![
            Note {
                id: ulid("01ARZ3NDEKTSV4RRFFQ69G5FB0"),
                kind: NoteKind::Regular,
                at: "2026-08-19T15:20:44Z".into(),
                text: "Reproduced on macOS; root cause is the pre-theme paint.".into(),
            },
            Note {
                id: ulid("01ARZ3NDEKTSV4RRFFQ69G5FB1"),
                kind: NoteKind::FeedbackNeeded,
                at: "2026-08-19T15:31:02Z".into(),
                text: "should the fix also cover the dashboard dedicated view?".into(),
            },
        ];
        t
    }

    #[test]
    fn round_trips_and_is_byte_idempotent() {
        let t = sample();
        let text = to_file_string(&t);
        let back = parse_file(&text).expect("parses");
        assert_eq!(back, t, "parse(serialize(t)) == t");
        assert_eq!(to_file_string(&back), text, "serialize is byte-idempotent");
    }

    #[test]
    fn serialized_form_is_shaped_as_expected() {
        let text = to_file_string(&sample());
        assert!(text.starts_with("---\nid: 01ARZ3NDEKTSV4RRFFQ69G5FAV\n"));
        assert!(text.contains("\n---\n\nThe dashboard flashes white"));
        assert!(text.contains("\n## Notes\n"));
        assert!(text.contains("<!-- note: 01ARZ3NDEKTSV4RRFFQ69G5FB0 -->"));
        assert!(text.contains("<!-- note: 01ARZ3NDEKTSV4RRFFQ69G5FB1 kind: feedback_needed -->"));
        // Regular notes omit the kind marker.
        assert!(!text.contains("kind: regular"));
    }

    #[test]
    fn minimal_ticket_round_trips_without_body_or_notes() {
        let id = ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV");
        let t = Ticket::new(id, "HS-XXXXXX", "t", "issue", "t0", "t1");
        let text = to_file_string(&t);
        assert!(!text.contains("## Notes"));
        let back = parse_file(&text).unwrap();
        assert_eq!(back, t);
        assert_eq!(to_file_string(&back), text);
    }

    #[test]
    fn full_close_and_provenance_fields_round_trip() {
        let mut t = sample();
        t.status = Status::Completed;
        t.completed_at = Some("2026-08-20T09:00:00Z".into());
        t.closed_at = Some("2026-08-20T09:00:00Z".into());
        t.close_reason = Some(CloseReason::Duplicate);
        t.duplicate_of = Some(ulid("01ARZ3NDEKTSV4RRFFQ69G5FC0"));
        t.blocked_by = vec![ulid("01ARZ3NDEKTSV4RRFFQ69G5FC1")];
        t.blocked_reason = Some("waiting on the theme refactor".into());
        t.claimed_by = Some("worker-1".into());
        t.claim_lease_expires_at = Some("2026-08-20T09:30:00Z".into());
        t.worker_label = Some("worktree-2".into());
        t.claim_count = 2;
        t.legacy_number = Some("HS-1234".into());
        t.copied_from = Some(ulid("01ARZ3NDEKTSV4RRFFQ69G5FC2"));
        t.review_requests = vec![ReviewRequest {
            who: "dana@example.com".into(),
            kind: ReviewKind::Feedback,
            by: ulid("01ARZ3NDEKTSV4RRFFQ69G5FC3"),
            at: "2026-08-20T08:00:00Z".into(),
        }];
        t.external = vec![ExternalLink {
            system: "github".into(),
            repo: "Small-Tale/hotsheet2".into(),
            id: "42".into(),
            url: "https://github.com/Small-Tale/hotsheet2/issues/42".into(),
            synced_at: "2026-08-20T07:00:00Z".into(),
            remote_hash: "abc123".into(),
        }];

        let text = to_file_string(&t);
        let back = parse_file(&text).expect("parses");
        assert_eq!(back, t);
        assert_eq!(to_file_string(&back), text);
    }

    #[test]
    fn unknown_frontmatter_keys_are_preserved() {
        let id = ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV");
        let text = format!(
            "---\nid: {id}\nslug: HS-XXXXXX\ntitle: t\ncategory: issue\n\
             created_at: t0\nupdated_at: t1\nschema: 1\nfuture_field: keep me\n\
             future_list:\n- a\n- b\n---\n\nbody\n"
        );
        let t = parse_file(&text).expect("parses");
        assert_eq!(
            t.extra.get("future_field").and_then(Value::as_str),
            Some("keep me")
        );
        // ...and they survive a re-serialize.
        assert!(to_file_string(&t).contains("future_field: keep me"));
        assert_eq!(parse_file(&to_file_string(&t)).unwrap(), t);
    }

    #[test]
    fn feedback_draft_notes_are_not_written_to_the_shared_file() {
        let mut t = sample();
        t.notes.push(Note {
            id: ulid("01ARZ3NDEKTSV4RRFFQ69G5FB9"),
            kind: NoteKind::FeedbackDraft,
            at: "2026-08-19T16:00:00Z".into(),
            text: "half-written reply".into(),
        });
        let text = to_file_string(&t);
        assert!(!text.contains("half-written reply"));
        assert!(!text.contains("feedback_draft"));
    }

    #[test]
    fn empty_text_notes_are_omitted() {
        // A note whose text is empty/whitespace carries nothing and would not
        // round-trip (its timestamp would re-parse as the text), so it is not
        // written — regression for the proptest counterexample.
        let mut t = sample();
        t.notes = vec![Note {
            id: ulid("01ARZ3NDEKTSV4RRFFQ69G5FBA"),
            kind: NoteKind::Regular,
            at: "2026-08-19T15:20:44Z".into(),
            text: "   ".into(),
        }];
        let text = to_file_string(&t);
        assert!(!text.contains("## Notes"));
        assert!(parse_file(&text).unwrap().notes.is_empty());
    }

    #[test]
    fn malformed_input_errors_rather_than_panics() {
        assert!(matches!(
            parse_file("no frontmatter here"),
            Err(ParseError::MissingFrontmatter)
        ));
        assert!(matches!(
            parse_file("---\nid: 01ARZ3NDEKTSV4RRFFQ69G5FAV\nno closing fence\n"),
            Err(ParseError::UnterminatedFrontmatter)
        ));
        assert!(parse_file("---\n[]\n---\n").is_err()); // sequence, not a mapping
        assert!(parse_file("---\ntitle: t\n---\n").is_err()); // missing required id
    }

    #[test]
    fn a_note_with_an_unparseable_id_is_skipped_not_fatal() {
        let id = ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV");
        let text = format!(
            "---\nid: {id}\nslug: HS-X\ntitle: t\ncategory: issue\n\
             created_at: t0\nupdated_at: t1\nschema: 1\n---\n\nbody\n\n## Notes\n\n\
             <!-- note: NOT-A-ULID -->\n2026-01-01T00:00:00Z — dropped\n\n\
             <!-- note: 01ARZ3NDEKTSV4RRFFQ69G5FB0 -->\n2026-01-01T00:00:00Z — kept\n"
        );
        let t = parse_file(&text).expect("parses despite one bad note");
        assert_eq!(t.notes.len(), 1);
        assert_eq!(t.notes[0].text, "kept");
    }

    #[test]
    fn known_keys_cover_serialized_fields() {
        // Guard against KNOWN_KEYS drifting from the struct: every key a fully
        // populated ticket emits must be recognized.
        let mut t = full_populated();
        t.extra.clear();
        let value = serde_yaml::to_value(&t).unwrap();
        for (k, _) in value.as_mapping().unwrap() {
            let key = k.as_str().unwrap();
            assert!(KNOWN_KEYS.contains(&key), "KNOWN_KEYS is missing `{key}`");
        }
    }

    fn full_populated() -> Ticket {
        let mut t = sample();
        t.completed_at = Some("x".into());
        t.verified_at = Some("x".into());
        t.closed_at = Some("x".into());
        t.close_reason = Some(CloseReason::Completed);
        t.duplicate_of = Some(ulid("01ARZ3NDEKTSV4RRFFQ69G5FC0"));
        t.blocked_by = vec![ulid("01ARZ3NDEKTSV4RRFFQ69G5FC1")];
        t.blocked_reason = Some("x".into());
        t.claimed_by = Some("w".into());
        t.claim_lease_expires_at = Some("x".into());
        t.worker_label = Some("w".into());
        t.claim_count = 1;
        t.review_requests = vec![ReviewRequest {
            who: "d@e.com".into(),
            kind: ReviewKind::Fyi,
            by: ulid("01ARZ3NDEKTSV4RRFFQ69G5FC3"),
            at: "x".into(),
        }];
        t.external = vec![ExternalLink {
            system: "github".into(),
            repo: "r".into(),
            id: "1".into(),
            url: "u".into(),
            synced_at: "x".into(),
            remote_hash: "h".into(),
        }];
        t.moved_to_store = Some("other".into());
        t.legacy_number = Some("HS-1".into());
        t.copied_from = Some(ulid("01ARZ3NDEKTSV4RRFFQ69G5FC2"));
        t
    }
}
