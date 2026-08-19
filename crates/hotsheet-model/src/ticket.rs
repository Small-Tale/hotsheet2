//! The `Ticket` aggregate + its sub-records. The full field schema (types, tiers,
//! required/optional) is `docs/17-ticket-file-format.md`.

use serde::{Deserialize, Serialize};

use crate::enums::{CloseReason, NoteKind, Priority, ReviewKind, Status};
use crate::ids::Ulid;

/// An RFC3339 timestamp.
///
/// Kept as a `String` for now; a typed timestamp lands with the Markdown+YAML
/// parser (tracked under HS2-3) so we don't bake a time-crate choice into the
/// scaffold.
pub type Timestamp = String;

/// A single note entry. `FeedbackDraft` notes are stored locally (per-user overlay),
/// not in the committed file (`docs/02` §2.6 / `docs/17` §17.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub id: Ulid,
    #[serde(default)]
    pub kind: NoteKind,
    pub at: Timestamp,
    pub text: String,
}

/// A request for a specific person's involvement (`docs/10` §10.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewRequest {
    pub who: String,
    pub kind: ReviewKind,
    pub by: Ulid,
    pub at: Timestamp,
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

/// A ticket. Shared fields serialize to the file's YAML frontmatter; `notes` render
/// under the `## Notes` section. Optional/empty fields are omitted for clean diffs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ticket {
    pub id: Ulid,
    pub slug: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub details: String,
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

    // Move tombstone (`docs/02` §2.13) — only on a `Status::Moved` record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub moved_to_store: Option<String>,

    // Migration / provenance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_number: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copied_from: Option<Ulid>,

    pub schema: u32,

    /// Rendered from the `## Notes` section, not literal frontmatter.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<Note>,
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
    use crate::{SCHEMA_VERSION, ids::derive_slug};

    fn sample_id() -> Ulid {
        Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap()
    }

    #[test]
    fn ticket_round_trips_via_json() {
        let id = sample_id();
        let t = Ticket {
            id,
            slug: derive_slug(&id, "HS"),
            title: "Fix the dashboard flicker".into(),
            details: "The gutter paints var(--bg) before the theme applies.".into(),
            category: "bug".into(),
            priority: Priority::High,
            status: Status::Started,
            up_next: true,
            tags: vec!["dashboard".into(), "ui".into()],
            blocked_by: vec![],
            blocked_reason: None,
            created_at: "2026-08-19T14:03:11Z".into(),
            updated_at: "2026-08-19T15:20:44Z".into(),
            completed_at: None,
            verified_at: None,
            closed_at: None,
            close_reason: None,
            duplicate_of: None,
            claimed_by: Some("worker-1".into()),
            claim_lease_expires_at: Some("2026-08-19T15:50:44Z".into()),
            worker_label: Some("worktree-2".into()),
            claim_count: 1,
            assignees: vec!["alex@example.com".into()],
            review_requests: vec![ReviewRequest {
                who: "dana@example.com".into(),
                kind: ReviewKind::Feedback,
                by: id,
                at: "2026-08-19T15:31:02Z".into(),
            }],
            external: vec![],
            moved_to_store: None,
            legacy_number: Some("HS-1234".into()),
            copied_from: None,
            schema: SCHEMA_VERSION,
            notes: vec![Note {
                id,
                kind: NoteKind::FeedbackNeeded,
                at: "2026-08-19T15:31:02Z".into(),
                text: "should the fix also cover the dedicated view?".into(),
            }],
        };
        let json = serde_json::to_string(&t).unwrap();
        let back: Ticket = serde_json::from_str(&json).unwrap();
        assert_eq!(t, back);
    }

    #[test]
    fn minimal_ticket_uses_defaults_and_omits_empties() {
        let json = r#"{
            "id":"01ARZ3NDEKTSV4RRFFQ69G5FAV","slug":"HS-XXXXXX","title":"t",
            "category":"issue","created_at":"t0","updated_at":"t1","schema":1
        }"#;
        let t: Ticket = serde_json::from_str(json).unwrap();
        assert_eq!(t.priority, Priority::Default);
        assert_eq!(t.status, Status::NotStarted);
        assert!(!t.up_next);
        assert!(t.tags.is_empty() && t.notes.is_empty() && t.assignees.is_empty());
        assert_eq!(t.claim_count, 0);

        // Empty/optional fields are omitted from the serialized form.
        let out = serde_json::to_string(&t).unwrap();
        assert!(!out.contains("up_next"));
        assert!(!out.contains("tags"));
        assert!(!out.contains("claimed_by"));
    }
}
