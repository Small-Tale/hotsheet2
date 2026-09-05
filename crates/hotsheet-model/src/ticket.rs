//! The `Ticket` aggregate + its sub-records. The full field schema (types, tiers,
//! required/optional) is `docs/17-ticket-file-format.md`.
//!
//! The `Serialize`/`Deserialize` derives here cover the **YAML frontmatter only**:
//! `details` is the Markdown body and `notes` is the `## Notes` section, so both are
//! `#[serde(skip)]` and handled by [`crate::format`]. Unknown frontmatter keys are
//! retained in [`Ticket::extra`] (forward-compat, docs/17 §17.4).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::enums::{CloseReason, NoteKind, Priority, ReviewKind, Status};
use crate::ids::Ulid;
use crate::timestamp::Timestamp;

/// A single note entry. `FeedbackDraft` notes are stored locally (per-user overlay),
/// not in the committed file (`docs/02` §2.6 / `docs/17` §17.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    pub id: Ulid,
    pub kind: NoteKind,
    pub created_at: Timestamp,
    pub edited_at: Timestamp,
    /// Concise, plain-text timeline headline. Optional for files written before
    /// HS2-A32EAK and for note kinds that do not appear in the timeline.
    pub summary: Option<String>,
    pub text: String,
}

impl Note {
    /// Whether regular-note text uses HS1's historical feedback marker.
    ///
    /// HS1 deliberately accepted the all-caps phrase anywhere in the note and did
    /// not require a colon. Keep the match case-sensitive so ordinary prose such as
    /// "feedback needed from the user" does not accidentally open an exchange.
    pub fn text_requests_feedback(text: &str) -> bool {
        text.contains("FEEDBACK NEEDED")
    }

    /// Whether this note opens a feedback exchange.
    ///
    /// Early HS2 automation wrote the historical `FEEDBACK NEEDED:` convention as a
    /// regular note. Keep those files meaningful while new writes use the first-class
    /// `feedback_needed` kind.
    pub fn is_feedback_needed_request(&self) -> bool {
        self.kind == NoteKind::FeedbackNeeded
            || (self.kind == NoteKind::Regular && Self::text_requests_feedback(&self.text))
    }
}

/// Durable metadata for one attachment payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    pub id: Ulid,
    pub filename: String,
    pub created_at: Timestamp,
}

/// A request for a specific person's involvement (`docs/10` §10.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewRequest {
    pub who: String,
    pub kind: ReviewKind,
    pub by: Ulid,
    pub at: Timestamp,
    /// Git email of the person who requested the review. Optional for files written
    /// before HS2-NZT80R; new writes populate it from the store's git identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_by: Option<String>,
}

/// A link to a ticket's counterpart in an external tracker (`docs/16` §16.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalLink {
    pub system: String,
    pub repo: String,
    pub id: String,
    pub url: String,
    pub synced_at: Timestamp,
    pub remote_hash: String,
}

/// A ticket. Shared frontmatter fields serialize to YAML; `details` renders as the
/// Markdown body and `notes` under `## Notes`. Optional/empty fields are omitted for
/// clean diffs. Construct via [`Ticket::new`] and fill the rest.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Ticket {
    pub id: Ulid,
    pub slug: String,
    pub title: String,
    pub category: String,
    #[serde(default)]
    pub priority: Priority,
    #[serde(default)]
    pub status: Status,
    #[serde(default, skip_serializing_if = "is_false")]
    pub up_next: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_by: Vec<Ulid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_reason: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<Timestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified_at: Option<Timestamp>,

    // Close outcome (`docs/02` §2.6a) — separate optional field, orthogonal to status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub closed_at: Option<Timestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub close_reason: Option<CloseReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplicate_of: Option<Ulid>,

    // Coordination (`docs/05` §5.7) — ephemeral, lease-expiring.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim_lease_expires_at: Option<Timestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_label: Option<String>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub claim_count: u32,

    // Assignment (`docs/10` §10.2).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assignees: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub review_requests: Vec<ReviewRequest>,

    // External sync (`docs/16` §16.2).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub external: Vec<ExternalLink>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<Attachment>,

    // Move tombstone (`docs/02` §2.13) — only on a `Status::Moved` record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub moved_to_store: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub moved_at: Option<Timestamp>,

    // Provenance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copied_from: Option<Ulid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transfer_operation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transferred_from: Option<String>,

    pub schema: u32,

    /// The Markdown body — **not** frontmatter.
    #[serde(skip)]
    pub details: String,

    /// The `## Notes` section — **not** frontmatter.
    #[serde(skip)]
    pub notes: Vec<Note>,

    /// Frontmatter keys the current schema doesn't recognize, preserved verbatim for
    /// forward-compat (docs/17 §17.4). Populated + re-emitted by [`crate::format`];
    /// empty for a canonical current-schema file.
    #[serde(skip)]
    pub extra: BTreeMap<String, serde_yaml::Value>,
}

impl Ticket {
    /// A new ticket with the required fields set and everything else defaulted
    /// (including `schema = SCHEMA_VERSION`). Callers fill optional fields after.
    pub fn new(
        id: Ulid,
        slug: impl Into<String>,
        title: impl Into<String>,
        category: impl Into<String>,
        created_at: impl Into<Timestamp>,
        updated_at: impl Into<Timestamp>,
    ) -> Self {
        Self {
            id,
            slug: slug.into(),
            title: title.into(),
            category: category.into(),
            created_at: created_at.into(),
            updated_at: updated_at.into(),
            schema: crate::SCHEMA_VERSION,
            ..Self::default()
        }
    }

    /// Whether the ticket's latest feedback exchange is still waiting for a response.
    /// A marked description starts the exchange when no regular/feedback note exists.
    /// A regular note answers the preceding feedback request; activity/status notes do
    /// not change that state, and a later feedback request opens it again.
    pub fn feedback_needed(&self) -> bool {
        self.notes
            .iter()
            .filter(|note| matches!(note.kind, NoteKind::Regular | NoteKind::FeedbackNeeded))
            .max_by(|a, b| {
                a.created_at
                    .chronological_cmp(&b.created_at)
                    .unwrap_or_else(|| a.created_at.as_str().cmp(b.created_at.as_str()))
                    .then(a.id.cmp(&b.id))
            })
            .map_or_else(
                || Note::text_requests_feedback(&self.details),
                Note::is_feedback_needed_request,
            )
    }
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(id: &str, kind: NoteKind, created_at: &str) -> Note {
        Note {
            id: Ulid::from_string(id).unwrap(),
            kind,
            created_at: Timestamp::new(created_at),
            edited_at: Timestamp::new(created_at),
            summary: None,
            text: String::new(),
        }
    }

    #[test]
    fn latest_regular_or_feedback_note_controls_feedback_needed() {
        let mut ticket = Ticket::default();
        ticket.notes.push(note(
            "01ARZ3NDEKTSV4RRFFQ69G5FA1",
            NoteKind::FeedbackNeeded,
            "2026-08-20T00:00:00Z",
        ));
        assert!(ticket.feedback_needed());

        ticket.notes.push(note(
            "01ARZ3NDEKTSV4RRFFQ69G5FA2",
            NoteKind::Activity,
            "2026-08-20T00:01:00Z",
        ));
        assert!(ticket.feedback_needed());

        ticket.notes.push(note(
            "01ARZ3NDEKTSV4RRFFQ69G5FA3",
            NoteKind::Regular,
            "2026-08-20T00:02:00Z",
        ));
        assert!(!ticket.feedback_needed());

        ticket.notes.push(note(
            "01ARZ3NDEKTSV4RRFFQ69G5FA4",
            NoteKind::FeedbackNeeded,
            "2026-08-20T00:03:00Z",
        ));
        assert!(ticket.feedback_needed());
    }

    #[test]
    fn legacy_feedback_prefix_participates_in_the_feedback_exchange() {
        let mut ticket = Ticket::default();
        let mut legacy = note(
            "01ARZ3NDEKTSV4RRFFQ69G5FA1",
            NoteKind::Regular,
            "2026-08-20T00:00:00Z",
        );
        legacy.text = "  FEEDBACK NEEDED: which behavior should we use?".into();
        ticket.notes.push(legacy);
        assert!(ticket.feedback_needed());

        ticket.notes.push(note(
            "01ARZ3NDEKTSV4RRFFQ69G5FA2",
            NoteKind::Activity,
            "2026-08-20T00:01:00Z",
        ));
        assert!(ticket.feedback_needed());

        ticket.notes.push(note(
            "01ARZ3NDEKTSV4RRFFQ69G5FA3",
            NoteKind::Regular,
            "2026-08-20T00:02:00Z",
        ));
        assert!(!ticket.feedback_needed());
    }

    #[test]
    fn legacy_feedback_marker_matches_hs1s_flexible_read_rules() {
        for text in [
            "FEEDBACK NEEDED: choose one",
            "Context first. FEEDBACK NEEDED: choose one",
            "FEEDBACK NEEDED choose one",
            "IMMEDIATE FEEDBACK NEEDED: choose one",
        ] {
            assert!(Note::text_requests_feedback(text), "{text:?}");
        }

        assert!(!Note::text_requests_feedback(
            "I think feedback needed from the user before continuing."
        ));
    }

    #[test]
    fn marked_description_opens_feedback_until_a_regular_note_answers_it() {
        let mut ticket = Ticket {
            details: "Context. FEEDBACK NEEDED: choose a direction\n\nCHOICE:\n- A\n- B".into(),
            ..Ticket::default()
        };
        assert!(ticket.feedback_needed());
        ticket.notes.push(note(
            "01ARZ3NDEKTSV4RRFFQ69G5FA1",
            NoteKind::Activity,
            "2026-08-20T00:00:00Z",
        ));
        assert!(ticket.feedback_needed());
        ticket.notes.push(note(
            "01ARZ3NDEKTSV4RRFFQ69G5FA2",
            NoteKind::Regular,
            "2026-08-20T00:01:00Z",
        ));
        assert!(!ticket.feedback_needed());
    }
}
