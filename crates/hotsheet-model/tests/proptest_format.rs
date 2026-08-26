//! Property + fuzz-style tests for the ticket file format (`docs/17` §17.4,
//! `docs/12` §12.7.2):
//!  1. **semantic round-trip** — `parse_file(to_file_string(t)) == t`
//!  2. **byte-idempotent** re-serialize
//!  3. **never panics** on arbitrary input.
//!
//! Generation includes multiline Markdown and structural-looking lines. Canonical
//! writers escape those lines before placing them inside explicit bounded blocks.
//! `feedback_draft` notes remain excluded because they are intentionally local-only.

use hotsheet_model::{
    CloseReason, ExternalLink, Note, NoteKind, Priority, ReviewKind, ReviewRequest, Status, Ticket,
    Timestamp, Ulid, parse_file, to_file_string,
};
use proptest::collection::vec;
use proptest::option;
use proptest::prelude::*;

/// A frontmatter scalar string: no boundary whitespace, no newline (so it round-trips
/// through YAML unambiguously).
fn scalar() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9 ._/@:-]{0,24}".prop_map(|s| s.trim().to_string())
}

/// Multiline Markdown, including headings and HTML-ish punctuation.
fn body() -> impl Strategy<Value = String> {
    prop_oneof![
        "[ -~\n]{0,100}".prop_map(|s| s.trim().to_string()),
        Just("## Notes\n<!-- hotsheet:body:end -->".to_string()),
    ]
}

fn arb_ts() -> impl Strategy<Value = Timestamp> {
    (
        2000u32..2100,
        1u32..13,
        1u32..29,
        0u32..24,
        0u32..60,
        0u32..60,
    )
        .prop_map(|(y, mo, d, h, mi, s)| {
            Timestamp::new(format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z"))
        })
}

fn arb_opt_ts() -> impl Strategy<Value = Option<Timestamp>> {
    option::of(arb_ts())
}

fn arb_ulid() -> impl Strategy<Value = Ulid> {
    any::<u128>().prop_map(Ulid::from)
}

fn arb_priority() -> impl Strategy<Value = Priority> {
    prop_oneof![
        Just(Priority::Highest),
        Just(Priority::High),
        Just(Priority::Default),
        Just(Priority::Low),
        Just(Priority::Lowest),
    ]
}

fn arb_status() -> impl Strategy<Value = Status> {
    prop_oneof![
        Just(Status::NotStarted),
        Just(Status::Started),
        Just(Status::Completed),
        Just(Status::Verified),
        Just(Status::Backlog),
        Just(Status::Archive),
        Just(Status::Deleted),
        Just(Status::Moved),
    ]
}

fn arb_close_reason() -> impl Strategy<Value = Option<CloseReason>> {
    option::of(prop_oneof![
        Just(CloseReason::Completed),
        Just(CloseReason::NotPlanned),
        Just(CloseReason::Duplicate),
        Just(CloseReason::Obsolete),
    ])
}

fn arb_review_kind() -> impl Strategy<Value = ReviewKind> {
    prop_oneof![
        Just(ReviewKind::Work),
        Just(ReviewKind::Feedback),
        Just(ReviewKind::Review),
        Just(ReviewKind::Fyi),
    ]
}

/// A shared note kind (feedback_draft is local-only and never written to the file).
fn arb_shared_note_kind() -> impl Strategy<Value = NoteKind> {
    prop_oneof![
        Just(NoteKind::Regular),
        Just(NoteKind::FeedbackNeeded),
        Just(NoteKind::Status),
    ]
}

fn arb_note() -> impl Strategy<Value = Note> {
    // Note text is non-empty after trimming, but otherwise permits multiline Markdown
    // and canonical marker lookalikes.
    // An empty-text note has no round-trip meaning and isn't written to the file
    // (see `format::tests::empty_text_notes_are_omitted`).
    (
        arb_ulid(),
        arb_shared_note_kind(),
        arb_ts(),
        prop_oneof![
            "[!-~][ -~\n]{0,80}".prop_map(|s| s.trim().to_string()),
            Just("# heading\n<!-- hotsheet:note:end -->\n## later".to_string()),
        ],
    )
        .prop_map(|(id, kind, at, text)| Note {
            id,
            kind,
            created_at: at.clone(),
            edited_at: at,
            text: text.trim().to_string(),
        })
}

fn arb_review() -> impl Strategy<Value = ReviewRequest> {
    (scalar(), arb_review_kind(), arb_ulid(), arb_ts()).prop_map(|(who, kind, by, at)| {
        ReviewRequest {
            who,
            kind,
            by,
            at,
            requested_by: None,
        }
    })
}

fn arb_external() -> impl Strategy<Value = ExternalLink> {
    (scalar(), scalar(), scalar(), scalar(), arb_ts(), scalar()).prop_map(
        |(system, repo, id, url, synced_at, remote_hash)| ExternalLink {
            system,
            repo,
            id,
            url,
            synced_at,
            remote_hash,
        },
    )
}

fn arb_ticket() -> impl Strategy<Value = Ticket> {
    let core = (
        arb_ulid(),
        scalar(),
        scalar(),
        scalar(),
        arb_priority(),
        arb_status(),
        any::<bool>(),
        body(),
    );
    let times = (arb_ts(), arb_ts(), arb_opt_ts(), arb_opt_ts(), arb_opt_ts());
    let close_coord = (
        arb_close_reason(),
        option::of(arb_ulid()),
        option::of(scalar()),
        arb_opt_ts(),
        option::of(scalar()),
        any::<u32>(),
        option::of(scalar()),
        option::of(arb_ulid()),
    );
    let collections = (
        vec(scalar(), 0..3),
        vec(arb_ulid(), 0..3),
        option::of(scalar()),
        vec(scalar(), 0..3),
        vec(arb_review(), 0..2),
        vec(arb_external(), 0..2),
        vec(arb_note(), 0..3),
    );

    (core, times, close_coord, collections).prop_map(
        |(
            (id, slug, title, category, priority, status, up_next, details),
            (created_at, updated_at, completed_at, verified_at, closed_at),
            (
                close_reason,
                duplicate_of,
                claimed_by,
                claim_lease_expires_at,
                worker_label,
                claim_count,
                moved_to_store,
                copied_from,
            ),
            (tags, blocked_by, blocked_reason, assignees, review_requests, external, notes),
        )| {
            let mut t = Ticket::new(id, slug, title, category, created_at, updated_at);
            t.priority = priority;
            t.status = status;
            t.up_next = up_next;
            t.details = details;
            t.completed_at = completed_at;
            t.verified_at = verified_at;
            t.closed_at = closed_at;
            t.close_reason = close_reason;
            t.duplicate_of = duplicate_of;
            t.claimed_by = claimed_by;
            t.claim_lease_expires_at = claim_lease_expires_at;
            t.worker_label = worker_label;
            t.claim_count = claim_count;
            t.moved_to_store = moved_to_store;
            t.copied_from = copied_from;
            t.tags = tags;
            t.blocked_by = blocked_by;
            t.blocked_reason = blocked_reason;
            t.assignees = assignees;
            t.review_requests = review_requests;
            t.external = external;
            t.notes = notes;
            t
        },
    )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn semantic_round_trip(t in arb_ticket()) {
        let text = to_file_string(&t);
        let back = parse_file(&text).expect("a serialized ticket must parse");
        prop_assert_eq!(back, t);
    }

    #[test]
    fn byte_idempotent(t in arb_ticket()) {
        let text = to_file_string(&t);
        let reparsed = parse_file(&text).expect("a serialized ticket must parse");
        prop_assert_eq!(to_file_string(&reparsed), text);
    }

    /// Arbitrary text must never panic the parser (degrade-not-panic).
    #[test]
    fn parse_never_panics_on_text(s in ".*") {
        let _ = parse_file(&s);
    }

    /// Arbitrary bytes (as lossy UTF-8) must never panic the parser.
    #[test]
    fn parse_never_panics_on_bytes(bytes in vec(any::<u8>(), 0..512)) {
        let s = String::from_utf8_lossy(&bytes);
        let _ = parse_file(&s);
    }

    /// Frontmatter-shaped-but-arbitrary content must never panic.
    #[test]
    fn parse_never_panics_on_framed(fm in ".*", bodyish in ".*") {
        let _ = parse_file(&format!("---\n{fm}\n---\n\n{bodyish}\n"));
    }
}
