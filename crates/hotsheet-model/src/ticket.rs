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
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}
