//! Wire DTOs — the **single** definition of a ticket's JSON shape, shared across
//! every surface (server REST/WS, the MCP shim's HTTP *and* direct-core backends),
//! so no surface re-derives it and the shapes can't drift (`docs/04` §4.2, wire
//! SSOT). `From<&Ticket>` is the one mapping from the domain model to the wire.
//!
//! Two shapes:
//! - [`ApiTicket`] — the full ticket (frontmatter + Markdown body + notes), returned
//!   by get/create/update/close.
//! - [`TicketRow`] — the compact list row (no notes), returned by a query. The index
//!   builds the same struct from its SQL columns, and a serverless scan builds it via
//!   [`TicketRow::from`], so a list looks identical whichever path produced it.

use hotsheet_model::{CloseReason, NoteKind, Priority, Status, Ticket, Timestamp};
use serde::Serialize;

/// The full ticket on the wire (unlike the frontmatter-only serde on [`Ticket`],
/// this carries the Markdown body and the notes).
#[derive(Debug, Clone, Serialize)]
pub struct ApiTicket {
    pub id: String,
    pub slug: String,
    pub title: String,
    pub details: String,
    pub category: String,
    pub priority: Priority,
    pub status: Status,
    pub up_next: bool,
    pub tags: Vec<String>,
    pub blocked_by: Vec<String>,
    pub blocked_reason: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
    pub verified_at: Option<String>,
    pub closed_at: Option<String>,
    pub close_reason: Option<CloseReason>,
    pub duplicate_of: Option<String>,
    /// Provenance of a cross-store copy: the source ticket's ULID (HS2-60).
    pub copied_from: Option<String>,
    /// A `moved` tombstone's redirect: the destination store this ULID now lives in (HS2-60).
    pub moved_to_store: Option<String>,
    /// When the move happened (tombstones only).
    pub moved_at: Option<String>,
    pub claimed_by: Option<String>,
    pub worker_label: Option<String>,
    pub claim_count: u32,
    pub assignees: Vec<String>,
    pub legacy_number: Option<String>,
    pub schema: u32,
    pub notes: Vec<ApiNote>,
}

/// One note entry on the wire.
#[derive(Debug, Clone, Serialize)]
pub struct ApiNote {
    pub id: String,
    pub kind: NoteKind,
    pub at: String,
    pub text: String,
}

impl From<&Ticket> for ApiTicket {
    fn from(t: &Ticket) -> Self {
        let ts = |o: &Option<Timestamp>| o.as_ref().map(|x| x.as_str().to_string());
        ApiTicket {
            id: t.id.to_string(),
            slug: t.slug.clone(),
            title: t.title.clone(),
            details: t.details.clone(),
            category: t.category.clone(),
            priority: t.priority,
            status: t.status,
            up_next: t.up_next,
            tags: t.tags.clone(),
            blocked_by: t.blocked_by.iter().map(|u| u.to_string()).collect(),
            blocked_reason: t.blocked_reason.clone(),
            created_at: t.created_at.as_str().to_string(),
            updated_at: t.updated_at.as_str().to_string(),
            completed_at: ts(&t.completed_at),
            verified_at: ts(&t.verified_at),
            closed_at: ts(&t.closed_at),
            close_reason: t.close_reason,
            duplicate_of: t.duplicate_of.map(|u| u.to_string()),
            copied_from: t.copied_from.map(|u| u.to_string()),
            moved_to_store: t.moved_to_store.clone(),
            moved_at: ts(&t.moved_at),
            claimed_by: t.claimed_by.clone(),
            worker_label: t.worker_label.clone(),
            claim_count: t.claim_count,
            assignees: t.assignees.clone(),
            legacy_number: t.legacy_number.clone(),
            schema: t.schema,
            notes: t
                .notes
                .iter()
                .map(|n| ApiNote {
                    id: n.id.to_string(),
                    kind: n.kind,
                    at: n.at.as_str().to_string(),
                    text: n.text.clone(),
                })
                .collect(),
        }
    }
}

/// A query result row — enough to draw a list without touching disk. Notes aren't
/// carried (search hits them via FTS; the file is authoritative). Enum fields are the
/// wire string (e.g. `"high"`, `"not_started"`) so the index's SQL rows and a
/// serverless scan agree byte-for-byte.
///
/// The Markdown `details` body is the one large field, so a **compact** list omits it
/// ([`TicketRow::compact`] blanks it and it's skipped when empty). Ask for the body
/// per-ticket via get, or with a non-compact list.
#[derive(Debug, Clone, Serialize)]
pub struct TicketRow {
    pub id: String,
    pub slug: String,
    pub title: String,
    /// The Markdown body. Omitted from the wire when empty (a compact list clears it).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub details: String,
    pub category: Option<String>,
    pub priority: Option<String>,
    pub status: Option<String>,
    pub up_next: bool,
    pub tags: Vec<String>,
    pub blocked_by: Vec<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub completed_at: Option<String>,
    pub verified_at: Option<String>,
    pub closed_at: Option<String>,
    pub close_reason: Option<String>,
    pub duplicate_of: Option<String>,
    pub claimed_by: Option<String>,
    pub worker_label: Option<String>,
    pub claim_count: u32,
    pub legacy_number: Option<String>,
}

impl From<&Ticket> for TicketRow {
    fn from(t: &Ticket) -> Self {
        let ts = |o: &Option<Timestamp>| o.as_ref().map(|x| x.as_str().to_string());
        TicketRow {
            id: t.id.to_string(),
            slug: t.slug.clone(),
            title: t.title.clone(),
            details: t.details.clone(),
            category: Some(t.category.clone()),
            priority: Some(enum_str(&t.priority)),
            status: Some(enum_str(&t.status)),
            up_next: t.up_next,
            tags: t.tags.clone(),
            blocked_by: t.blocked_by.iter().map(|u| u.to_string()).collect(),
            created_at: Some(t.created_at.as_str().to_string()),
            updated_at: Some(t.updated_at.as_str().to_string()),
            completed_at: ts(&t.completed_at),
            verified_at: ts(&t.verified_at),
            closed_at: ts(&t.closed_at),
            close_reason: t.close_reason.as_ref().map(enum_str),
            duplicate_of: t.duplicate_of.map(|u| u.to_string()),
            claimed_by: t.claimed_by.clone(),
            worker_label: t.worker_label.clone(),
            claim_count: t.claim_count,
            legacy_number: t.legacy_number.clone(),
        }
    }
}

impl TicketRow {
    /// A list row without the Markdown `details` body — the compact projection used
    /// for browsing (the body is the one large field; fetch it per-ticket via get).
    pub fn compact(t: &Ticket) -> Self {
        let mut row = TicketRow::from(t);
        row.details.clear();
        row
    }

    /// Drop the `details` body from this row in place (compact an already-built row,
    /// e.g. one produced by the index's SQL).
    pub fn make_compact(&mut self) {
        self.details.clear();
    }
}

/// The wire string of a serde-string enum (`"high"`, `"not_started"`, …) — the same
/// form the index stores, so rows match regardless of how they were built.
fn enum_str<T: Serialize>(v: &T) -> String {
    serde_json::to_value(v)
        .ok()
        .and_then(|x| x.as_str().map(String::from))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use hotsheet_model::{Ticket, Ulid, derive_slug};

    fn ticket() -> Ticket {
        let id = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
        let now = Timestamp::new("2026-08-20T00:00:00Z");
        let mut t = Ticket::new(
            id,
            derive_slug(&id, "HS"),
            "Fix flicker",
            "bug",
            now.clone(),
            now,
        );
        t.priority = Priority::High;
        t.status = Status::Started;
        t.up_next = true;
        t
    }

    #[test]
    fn row_enum_fields_are_wire_strings() {
        let row = TicketRow::from(&ticket());
        assert_eq!(row.priority.as_deref(), Some("high"));
        assert_eq!(row.status.as_deref(), Some("started"));
        assert_eq!(row.category.as_deref(), Some("bug"));
        assert!(row.up_next);
        assert!(row.slug.starts_with("HS-"));
    }

    #[test]
    fn compact_row_drops_the_body_and_serialization_omits_it() {
        let mut t = ticket();
        t.details = "a long markdown body".into();

        // A full row carries the body and serializes it.
        let full = TicketRow::from(&t);
        assert_eq!(full.details, "a long markdown body");
        let full_json = serde_json::to_value(&full).unwrap();
        assert_eq!(full_json["details"], "a long markdown body");

        // A compact row clears the body, and the empty string is skipped on the wire
        // (the key is absent, not `""`).
        let compact = TicketRow::compact(&t);
        assert_eq!(compact.details, "");
        let compact_json = serde_json::to_value(&compact).unwrap();
        assert!(compact_json.get("details").is_none());
        // Everything else the list needs is still there.
        assert_eq!(compact_json["slug"], full_json["slug"]);
        assert_eq!(compact_json["status"], "started");
    }

    #[test]
    fn api_ticket_carries_typed_enums_and_body() {
        let api = ApiTicket::from(&ticket());
        assert_eq!(api.priority, Priority::High);
        assert_eq!(api.status, Status::Started);
        assert_eq!(api.title, "Fix flicker");
        assert_eq!(api.schema, ticket().schema);
    }
}
