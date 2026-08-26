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
    "moved_at",
    "copied_from",
    "transfer_operation_id",
    "transferred_from",
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
    /// A bounded canonical ticket has content after its notes container. A newer
    /// schema may define such sections; this reader refuses to silently discard it.
    #[error("unsupported content follows the bounded Notes section")]
    TrailingContent,
}

const BODY_BEGIN: &str = "<!-- hotsheet:body:begin -->";
const BODY_END: &str = "<!-- hotsheet:body:end -->";
const NOTES_BEGIN: &str = "<!-- hotsheet:notes:begin -->";
const NOTES_END: &str = "<!-- hotsheet:notes:end -->";
const NOTE_BEGIN_PREFIX: &str = "<!-- hotsheet:note:begin ";
const NOTE_END: &str = "<!-- hotsheet:note:end -->";

/// Serialize a ticket to its canonical on-disk form.
pub fn to_file_string(t: &Ticket) -> String {
    let frontmatter = frontmatter_to_string(t);

    let mut out = String::with_capacity(frontmatter.len() + t.details.len() + 160);
    out.push_str("---\n");
    out.push_str(&frontmatter); // serde_yaml output ends in '\n'
    out.push_str("---\n");

    out.push('\n');
    out.push_str(BODY_BEGIN);
    out.push('\n');
    let details = t.details.trim();
    if !details.is_empty() {
        out.push_str(&escape_content(details));
        out.push('\n');
    }
    out.push_str(BODY_END);
    out.push('\n');

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
            // HS1 identifiers were briefly persisted by the importer. They are not
            // an HS2 identity and deliberately disappear on the next canonical write.
            if key == "legacy_number" {
                continue;
            }
            if !KNOWN_KEYS.contains(&key) {
                ticket.extra.insert(key.to_string(), v.clone());
            }
        }
    }

    if after.trim_start().starts_with(BODY_BEGIN) {
        let (body, notes) = parse_bounded_content(after)?;
        ticket.details = body;
        ticket.notes = notes;
    } else {
        let (body, notes_section) = split_legacy_body_and_notes(after);
        ticket.details = body;
        ticket.notes = notes_section.map(parse_legacy_notes).unwrap_or_default();
    }

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
fn split_legacy_body_and_notes(after: &str) -> (String, Option<&str>) {
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

fn parse_legacy_notes(section: &str) -> Vec<Note> {
    let body = section.strip_prefix("## Notes").unwrap_or(section);
    let lines: Vec<&str> = body.lines().collect();

    let mut notes = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if let Some(metadata) = parse_note_marker(lines[i].trim()) {
            i += 1;
            let start = i;
            while i < lines.len() && parse_note_marker(lines[i].trim()).is_none() {
                i += 1;
            }
            if let Some(note) = build_note(metadata, lines[start..i].join("\n").trim()) {
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
fn parse_note_marker(line: &str) -> Option<NoteMetadata> {
    let inner = line
        .strip_prefix("<!--")?
        .strip_suffix("-->")?
        .trim()
        .strip_prefix("note:")?
        .trim();

    let mut tokens = inner.split_whitespace();
    let id = Ulid::from_string(tokens.next()?).ok()?;

    Some(parse_note_metadata(id, tokens.collect()))
}

#[derive(Debug)]
struct NoteMetadata {
    id: Ulid,
    kind: NoteKind,
    created_at: Option<Timestamp>,
    edited_at: Option<Timestamp>,
}

fn parse_note_metadata(id: Ulid, tokens: Vec<&str>) -> NoteMetadata {
    let value_after = |key: &str| {
        tokens
            .iter()
            .position(|token| *token == key)
            .and_then(|position| tokens.get(position + 1))
            .copied()
    };
    NoteMetadata {
        id,
        kind: value_after("kind:").map_or(NoteKind::Regular, parse_note_kind),
        created_at: value_after("created_at:").map(Timestamp::new),
        edited_at: value_after("edited_at:").map(Timestamp::new),
    }
}

fn build_note(metadata: NoteMetadata, block: &str) -> Option<Note> {
    if block.is_empty() {
        return None;
    }
    let (legacy_at, text) = if metadata.created_at.is_none() {
        match block.split_once(" — ") {
            Some((at, text)) => (Some(Timestamp::new(at.trim())), text.trim().to_string()),
            None => (None, block.to_string()),
        }
    } else {
        (None, block.to_string())
    };
    let created_at = metadata
        .created_at
        .or(legacy_at)
        .unwrap_or_else(|| note_id_timestamp(metadata.id));
    let edited_at = metadata.edited_at.unwrap_or_else(|| created_at.clone());
    Some(Note {
        id: metadata.id,
        kind: metadata.kind,
        created_at,
        edited_at,
        text,
    })
}

fn note_id_timestamp(id: Ulid) -> Timestamp {
    let nanos = i128::from(id.timestamp_ms()) * 1_000_000;
    time::OffsetDateTime::from_unix_timestamp_nanos(nanos)
        .map(Timestamp::from_datetime)
        .unwrap_or_default()
}

fn notes_to_string(notes: &[&Note]) -> String {
    let mut out = format!("{NOTES_BEGIN}\n## Notes\n");
    for n in notes {
        out.push('\n');
        out.push_str(NOTE_BEGIN_PREFIX);
        out.push_str(&n.id.to_string());
        if n.kind != NoteKind::Regular {
            out.push_str(" kind: ");
            out.push_str(note_kind_str(n.kind));
        }
        out.push_str(" created_at: ");
        out.push_str(n.created_at.as_str());
        out.push_str(" edited_at: ");
        out.push_str(n.edited_at.as_str());
        out.push_str(" -->\n");
        out.push_str(&escape_content(&n.text));
        out.push('\n');
        out.push_str(NOTE_END);
        out.push('\n');
    }
    out.push_str(NOTES_END);
    out.push('\n');
    out
}

fn parse_bounded_content(after: &str) -> Result<(String, Vec<Note>), ParseError> {
    let Some((_, body_and_rest)) = split_once_marker_line(after, BODY_BEGIN) else {
        unreachable!("caller checked for the body marker")
    };
    let Some((body, rest)) = split_once_marker_line(body_and_rest, BODY_END) else {
        return Err(ParseError::TrailingContent);
    };
    let details = unescape_content(body.trim());
    let rest = rest.trim();
    if rest.is_empty() {
        return Ok((details, Vec::new()));
    }
    let Some((before_notes, notes_and_rest)) = split_once_marker_line(rest, NOTES_BEGIN) else {
        return Err(ParseError::TrailingContent);
    };
    if !before_notes.trim().is_empty() {
        return Err(ParseError::TrailingContent);
    }
    let Some((notes, trailing)) = split_once_marker_line(notes_and_rest, NOTES_END) else {
        return Err(ParseError::TrailingContent);
    };
    if !trailing.trim().is_empty() {
        return Err(ParseError::TrailingContent);
    }
    Ok((details, parse_bounded_notes(notes)))
}

fn parse_bounded_notes(section: &str) -> Vec<Note> {
    let section = section.trim().strip_prefix("## Notes").unwrap_or(section);
    let mut notes = Vec::new();
    let mut rest = section;
    while let Some((_, marker, content)) = split_once_prefixed_marker_line(rest, NOTE_BEGIN_PREFIX)
    {
        let Some(metadata) = marker
            .strip_prefix(NOTE_BEGIN_PREFIX)
            .and_then(|value| value.strip_suffix("-->"))
        else {
            break;
        };
        let Some((block, after_end)) = split_once_marker_line(content, NOTE_END) else {
            break;
        };
        if let Some(metadata) = parse_bounded_note_metadata(metadata.trim()) {
            if let Some(mut note) = build_note(metadata, block.trim()) {
                note.text = unescape_content(&note.text);
                notes.push(note);
            }
        }
        rest = after_end;
    }
    notes
}

fn split_once_marker_line<'a>(text: &'a str, marker: &str) -> Option<(&'a str, &'a str)> {
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let bare = line.strip_suffix('\n').unwrap_or(line);
        if bare == marker {
            return Some((&text[..offset], &text[offset + line.len()..]));
        }
        offset += line.len();
    }
    None
}

fn split_once_prefixed_marker_line<'a>(
    text: &'a str,
    prefix: &str,
) -> Option<(&'a str, &'a str, &'a str)> {
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let bare = line.strip_suffix('\n').unwrap_or(line);
        if bare.starts_with(prefix) {
            return Some((&text[..offset], bare, &text[offset + line.len()..]));
        }
        offset += line.len();
    }
    None
}

fn parse_bounded_note_metadata(metadata: &str) -> Option<NoteMetadata> {
    let mut tokens = metadata.split_whitespace();
    let id = Ulid::from_string(tokens.next()?).ok()?;
    Some(parse_note_metadata(id, tokens.collect()))
}

/// Prefix structural-looking user lines with one backslash. Existing leading
/// backslashes are doubled, making the transform exact and reversible.
fn escape_content(content: &str) -> String {
    content
        .lines()
        .map(|line| {
            let bare = line.trim_start_matches('\\');
            if bare.starts_with("<!-- hotsheet:") {
                format!("\\{line}")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn unescape_content(content: &str) -> String {
    content
        .lines()
        .map(|line| {
            let without_one = line.strip_prefix('\\').unwrap_or(line);
            if without_one
                .trim_start_matches('\\')
                .starts_with("<!-- hotsheet:")
            {
                without_one
            } else {
                line
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn note_kind_str(kind: NoteKind) -> &'static str {
    match kind {
        NoteKind::Regular => "regular",
        NoteKind::FeedbackNeeded => "feedback_needed",
        NoteKind::FeedbackDraft => "feedback_draft",
        NoteKind::Status => "status",
        NoteKind::Activity => "activity",
    }
}

fn parse_note_kind(s: &str) -> NoteKind {
    match s {
        "feedback_needed" => NoteKind::FeedbackNeeded,
        "feedback_draft" => NoteKind::FeedbackDraft,
        "status" => NoteKind::Status,
        "activity" => NoteKind::Activity,
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
                created_at: "2026-08-19T15:20:44Z".into(),
                edited_at: "2026-08-19T15:20:44Z".into(),
                text: "Reproduced on macOS; root cause is the pre-theme paint.".into(),
            },
            Note {
                id: ulid("01ARZ3NDEKTSV4RRFFQ69G5FB1"),
                kind: NoteKind::FeedbackNeeded,
                created_at: "2026-08-19T15:31:02Z".into(),
                edited_at: "2026-08-19T15:31:02Z".into(),
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
        assert!(text.contains(&format!(
            "\n---\n\n{BODY_BEGIN}\nThe dashboard flashes white"
        )));
        assert!(text.contains(BODY_END));
        assert!(text.contains(NOTES_BEGIN));
        assert!(text.contains("\n## Notes\n"));
        assert!(text.contains("<!-- hotsheet:note:begin 01ARZ3NDEKTSV4RRFFQ69G5FB0 created_at: 2026-08-19T15:20:44Z edited_at: 2026-08-19T15:20:44Z -->"));
        assert!(text.contains(
            "<!-- hotsheet:note:begin 01ARZ3NDEKTSV4RRFFQ69G5FB1 kind: feedback_needed created_at: 2026-08-19T15:31:02Z edited_at: 2026-08-19T15:31:02Z -->"
        ));
        assert!(text.contains(NOTE_END));
        assert!(text.contains(NOTES_END));
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
    fn retired_hs1_number_is_read_but_not_retained() {
        let canonical = to_file_string(&sample());
        let old = canonical.replacen("schema: 1", "legacy_number: HS-1234\nschema: 1", 1);
        let parsed = parse_file(&old).unwrap();
        assert!(!parsed.extra.contains_key("legacy_number"));
        assert!(!to_file_string(&parsed).contains("legacy_number"));
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
        t.copied_from = Some(ulid("01ARZ3NDEKTSV4RRFFQ69G5FC2"));
        t.review_requests = vec![ReviewRequest {
            who: "dana@example.com".into(),
            kind: ReviewKind::Feedback,
            by: ulid("01ARZ3NDEKTSV4RRFFQ69G5FC3"),
            at: "2026-08-20T08:00:00Z".into(),
            requested_by: Some("requester@example.com".into()),
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
            created_at: "2026-08-19T16:00:00Z".into(),
            edited_at: "2026-08-19T16:00:00Z".into(),
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
            created_at: "2026-08-19T15:20:44Z".into(),
            edited_at: "2026-08-19T15:20:44Z".into(),
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
    fn arbitrary_markdown_and_structural_lookalikes_round_trip() {
        let mut t = sample();
        t.details = format!(
            "# Details\n\n## Notes\n\n{BODY_END}\n\\{NOTES_BEGIN}\n<script>alert('x')</script>"
        );
        t.notes[0].text = format!(
            "this is my note\n\n# What about this\n\n{NOTE_END}\n{NOTES_END}\n\\{BODY_END}"
        );

        let encoded = to_file_string(&t);
        assert!(encoded.contains(&format!("\\{BODY_END}")));
        assert!(encoded.contains(&format!("\\{NOTE_END}")));
        assert_eq!(parse_file(&encoded).unwrap(), t);
    }

    #[test]
    fn bounded_notes_reject_a_later_unknown_section_instead_of_swallowing_it() {
        let mut encoded = to_file_string(&sample());
        encoded.push_str("\n## Future section\nkeep me\n");
        assert!(matches!(
            parse_file(&encoded),
            Err(ParseError::TrailingContent)
        ));
    }

    #[test]
    fn legacy_one_sided_notes_remain_readable() {
        let encoded = to_file_string(&sample());
        let frontmatter_end = encoded.find("\n---\n").unwrap() + 5;
        let legacy = format!(
            "{}\nlegacy details\n\n## Notes\n\n<!-- note: 01ARZ3NDEKTSV4RRFFQ69G5FB0 -->\n2026-08-19T15:20:44Z — legacy note\n",
            &encoded[..frontmatter_end]
        );
        let parsed = parse_file(&legacy).unwrap();
        assert_eq!(parsed.details, "legacy details");
        assert_eq!(parsed.notes[0].text, "legacy note");
        assert_eq!(parsed.notes[0].created_at.as_str(), "2026-08-19T15:20:44Z");
        assert_eq!(parsed.notes[0].edited_at, parsed.notes[0].created_at);
        assert!(to_file_string(&parsed).contains(NOTES_END));
    }

    #[test]
    fn activity_kind_and_distinct_note_timestamps_round_trip() {
        let mut ticket = sample();
        ticket.notes = vec![Note {
            id: ulid("01ARZ3NDEKTSV4RRFFQ69G5FB2"),
            kind: NoteKind::Activity,
            created_at: "2026-08-19T15:20:44Z".into(),
            edited_at: "2026-08-19T16:00:00Z".into(),
            text: "completed investigation".into(),
        }];
        let encoded = to_file_string(&ticket);
        assert!(encoded.contains("kind: activity"));
        assert_eq!(parse_file(&encoded).unwrap(), ticket);
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
            requested_by: None,
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
        t.copied_from = Some(ulid("01ARZ3NDEKTSV4RRFFQ69G5FC2"));
        t
    }
}
