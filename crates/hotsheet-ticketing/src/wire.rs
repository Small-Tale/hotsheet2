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

use hotsheet_model::{CloseReason, NoteKind, Priority, ReviewRequest, Status, Ticket, Timestamp};
use serde::Serialize;

use crate::auto_context::{self, AutoContextEntry, TicketAutoContext};

/// The full ticket on the wire (unlike the frontmatter-only serde on [`Ticket`],
/// this carries the Markdown body and the notes).
#[derive(Debug, Clone, Serialize)]
pub struct ApiTicket {
    /// Project-scoped provider connection that owns this ticket.
    pub connection_id: String,
    /// Provider-native id (ULID for the default git provider).
    pub native_id: String,
    /// Unambiguous identity across every connected ticket system.
    pub qualified_id: String,
    /// Native browser URL when the provider has one.
    pub native_url: Option<String>,
    /// Provider-native optimistic-concurrency token (opaque to callers).
    pub concurrency_token: Option<String>,
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
    pub transfer_operation_id: Option<String>,
    pub transferred_from: Option<String>,
    /// A `moved` tombstone's redirect: the destination store this ULID now lives in (HS2-60).
    pub moved_to_store: Option<String>,
    /// When the move happened (tombstones only).
    pub moved_at: Option<String>,
    pub claimed_by: Option<String>,
    pub claim_lease_expires_at: Option<String>,
    pub worker_label: Option<String>,
    pub claim_count: u32,
    pub assignees: Vec<String>,
    pub review_requests: Vec<ReviewRequest>,
    pub schema: u32,
    pub notes: Vec<ApiNote>,
    pub attachments: Vec<ApiAttachment>,
    /// Computed standing guidance; never persisted in the ticket file.
    pub auto_context: Vec<TicketAutoContext>,
}

/// One note entry on the wire.
#[derive(Debug, Clone, Serialize)]
pub struct ApiNote {
    pub id: String,
    pub kind: NoteKind,
    pub created_at: String,
    pub edited_at: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApiAttachment {
    pub id: String,
    pub filename: String,
    pub created_at: String,
}

impl From<&Ticket> for ApiTicket {
    fn from(t: &Ticket) -> Self {
        Self::from_provider(t, "git", None)
    }
}

impl ApiTicket {
    pub fn from_provider(t: &Ticket, connection_id: &str, native_url: Option<String>) -> Self {
        let ts = |o: &Option<Timestamp>| o.as_ref().map(|x| x.as_str().to_string());
        let native_id = t.id.to_string();
        ApiTicket {
            connection_id: connection_id.to_string(),
            native_id: native_id.clone(),
            qualified_id: format!("{connection_id}:{native_id}"),
            native_url,
            concurrency_token: Some(t.updated_at.as_str().to_string()),
            id: native_id,
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
            transfer_operation_id: t.transfer_operation_id.clone(),
            transferred_from: t.transferred_from.clone(),
            moved_to_store: t.moved_to_store.clone(),
            moved_at: ts(&t.moved_at),
            claimed_by: t.claimed_by.clone(),
            claim_lease_expires_at: ts(&t.claim_lease_expires_at),
            worker_label: t.worker_label.clone(),
            claim_count: t.claim_count,
            assignees: t.assignees.clone(),
            review_requests: t.review_requests.clone(),
            schema: t.schema,
            notes: t
                .notes
                .iter()
                .map(|n| ApiNote {
                    id: n.id.to_string(),
                    kind: if n.is_feedback_needed_request() {
                        NoteKind::FeedbackNeeded
                    } else {
                        n.kind
                    },
                    created_at: n.created_at.as_str().to_string(),
                    edited_at: n.edited_at.as_str().to_string(),
                    text: n.text.clone(),
                })
                .collect(),
            attachments: t
                .attachments
                .iter()
                .map(|attachment| ApiAttachment {
                    id: attachment.id.to_string(),
                    filename: attachment.filename.clone(),
                    created_at: attachment.created_at.as_str().to_string(),
                })
                .collect(),
            auto_context: Vec::new(),
        }
    }

    pub fn with_auto_context(ticket: &Ticket, entries: &[AutoContextEntry]) -> Self {
        let mut wire = Self::from(ticket);
        wire.auto_context = auto_context::resolve(ticket, entries);
        wire
    }

    pub fn with_provider_auto_context(
        ticket: &Ticket,
        connection_id: &str,
        native_url: Option<String>,
        entries: &[AutoContextEntry],
    ) -> Self {
        let mut wire = Self::from_provider(ticket, connection_id, native_url);
        wire.auto_context = auto_context::resolve(ticket, entries);
        wire
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
    pub connection_id: String,
    pub native_id: String,
    pub qualified_id: String,
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
    /// True when the latest regular/feedback exchange ends in `feedback_needed` — it is
    /// waiting on the user. A later regular note clears the state. Computed here (a
    /// compact row has no notes) so lists need not load every ticket's notes.
    pub feedback_needed: bool,
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
    pub claim_lease_expires_at: Option<String>,
    pub worker_label: Option<String>,
    pub claim_count: u32,
    /// Computed standing guidance; never persisted in the index or ticket file.
    pub auto_context: Vec<TicketAutoContext>,
}

impl From<&Ticket> for TicketRow {
    fn from(t: &Ticket) -> Self {
        let ts = |o: &Option<Timestamp>| o.as_ref().map(|x| x.as_str().to_string());
        TicketRow {
            connection_id: "git".into(),
            native_id: t.id.to_string(),
            qualified_id: format!("git:{}", t.id),
            id: t.id.to_string(),
            slug: t.slug.clone(),
            title: t.title.clone(),
            details: t.details.clone(),
            category: Some(t.category.clone()),
            priority: Some(enum_str(&t.priority)),
            status: Some(enum_str(&t.status)),
            up_next: t.up_next,
            feedback_needed: t.feedback_needed(),
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
            claim_lease_expires_at: ts(&t.claim_lease_expires_at),
            worker_label: t.worker_label.clone(),
            claim_count: t.claim_count,
            auto_context: Vec::new(),
        }
    }
}

impl TicketRow {
    pub fn set_connection(&mut self, connection_id: &str) {
        self.connection_id = connection_id.to_string();
        self.native_id.clone_from(&self.id);
        self.qualified_id = format!("{connection_id}:{}", self.native_id);
    }

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

    pub fn add_auto_context(&mut self, entries: &[AutoContextEntry]) {
        self.auto_context = auto_context::resolve_fields(
            self.category.as_deref().unwrap_or_default(),
            &self.tags,
            entries,
        );
    }
}

/// A leaner-than-compact list projection (HS2-GY3GWT item 1): keep only `fields` on each
/// row object, for a truly minimal browse row (e.g. `slug,status,up_next,title`) without
/// changing the shared [`TicketRow`] wire shape. Operates on already-serialized rows so the
/// server and MCP share one implementation. Rules:
/// - `slug` is **always** kept, so a projected row is still identifiable even if the caller
///   forgot to ask for it.
/// - Unknown field names are ignored; a non-object element is left untouched.
/// - An empty `fields` list is a no-op (returns the full rows).
pub fn project_fields(rows: &mut [serde_json::Value], fields: &[String]) {
    use std::collections::HashSet;
    if fields.is_empty() {
        return;
    }
    let keep: HashSet<&str> = fields
        .iter()
        .map(String::as_str)
        .chain(std::iter::once("slug"))
        .collect();
    for row in rows.iter_mut() {
        if let Some(obj) = row.as_object_mut() {
            obj.retain(|k, _| keep.contains(k.as_str()));
        }
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
        let mut ticket = ticket();
        ticket.claimed_by = Some("codex-1".into());
        ticket.claim_lease_expires_at = Some(Timestamp::new("2026-08-20T00:30:00Z"));
        let row = TicketRow::from(&ticket);
        assert_eq!(row.priority.as_deref(), Some("high"));
        assert_eq!(row.status.as_deref(), Some("started"));
        assert_eq!(row.category.as_deref(), Some("bug"));
        assert!(row.up_next);
        assert!(row.slug.starts_with("HS-"));
        assert_eq!(row.claimed_by.as_deref(), Some("codex-1"));
        assert_eq!(
            row.claim_lease_expires_at.as_deref(),
            Some("2026-08-20T00:30:00Z")
        );
    }

    #[test]
    fn feedback_needed_row_flag_tracks_a_feedback_needed_note() {
        use hotsheet_model::Note;
        let note = |id, kind, created_at| Note {
            id: Ulid::from_string(id).unwrap(),
            kind,
            created_at: Timestamp::new(created_at),
            edited_at: Timestamp::new(created_at),
            text: "please confirm".into(),
        };

        // No notes → not waiting on the user.
        assert!(!TicketRow::from(&ticket()).feedback_needed);

        // A regular note alone doesn't raise the flag.
        let mut regular = ticket();
        regular.notes.push(note(
            "01ARZ3NDEKTSV4RRFFQ69G5FB1",
            NoteKind::Regular,
            "2026-08-20T00:00:00Z",
        ));
        assert!(!TicketRow::from(&regular).feedback_needed);

        // A feedback_needed note does — including on the compact list row and its JSON.
        let mut waiting = ticket();
        waiting.notes.push(note(
            "01ARZ3NDEKTSV4RRFFQ69G5FB1",
            NoteKind::Regular,
            "2026-08-20T00:00:00Z",
        ));
        waiting.notes.push(note(
            "01ARZ3NDEKTSV4RRFFQ69G5FB2",
            NoteKind::FeedbackNeeded,
            "2026-08-20T00:01:00Z",
        ));
        assert!(TicketRow::from(&waiting).feedback_needed);
        assert!(TicketRow::compact(&waiting).feedback_needed);
        let json = serde_json::to_value(TicketRow::compact(&waiting)).unwrap();
        assert_eq!(json["feedback_needed"], true);

        waiting.notes.push(note(
            "01ARZ3NDEKTSV4RRFFQ69G5FB3",
            NoteKind::Regular,
            "2026-08-20T00:02:00Z",
        ));
        assert!(!TicketRow::from(&waiting).feedback_needed);
    }

    #[test]
    fn legacy_feedback_prefix_is_normalized_on_full_and_compact_wire_shapes() {
        use hotsheet_model::Note;
        let mut waiting = ticket();
        waiting.notes.push(Note {
            id: Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FB1").unwrap(),
            kind: NoteKind::Regular,
            created_at: Timestamp::new("2026-08-20T00:00:00Z"),
            edited_at: Timestamp::new("2026-08-20T00:00:00Z"),
            text: "FEEDBACK NEEDED: choose one".into(),
        });

        assert!(TicketRow::from(&waiting).feedback_needed);
        assert_eq!(
            ApiTicket::from(&waiting).notes[0].kind,
            NoteKind::FeedbackNeeded
        );
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
    fn project_fields_keeps_only_requested_keys_plus_slug() {
        let row = serde_json::to_value(TicketRow::from(&ticket())).unwrap();
        let mut rows = vec![row];
        // Ask for status + title; slug is kept implicitly, everything else dropped.
        project_fields(&mut rows, &["status".into(), "title".into()]);
        let obj = rows[0].as_object().unwrap();
        let keys: std::collections::HashSet<&str> = obj.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            ["slug", "status", "title"].into_iter().collect(),
            "only the requested fields (+ slug) survive"
        );
        assert_eq!(obj["status"], "started");

        // An empty projection is a no-op (the full row is preserved).
        let mut full = vec![serde_json::to_value(TicketRow::from(&ticket())).unwrap()];
        let before = full[0].as_object().unwrap().len();
        project_fields(&mut full, &[]);
        assert_eq!(full[0].as_object().unwrap().len(), before);

        // An unknown field name is ignored (doesn't error, just isn't added).
        let mut r = vec![serde_json::to_value(TicketRow::from(&ticket())).unwrap()];
        project_fields(&mut r, &["nonesuch".into()]);
        let keys: Vec<&String> = r[0].as_object().unwrap().keys().collect();
        assert_eq!(
            keys,
            vec!["slug"],
            "only slug remains — the unknown key added nothing"
        );
    }

    #[test]
    fn api_ticket_carries_typed_enums_and_body() {
        let mut ticket = ticket();
        ticket.claimed_by = Some("codex-1".into());
        ticket.claim_lease_expires_at = Some(Timestamp::new("2026-08-20T00:30:00Z"));
        let api = ApiTicket::from(&ticket);
        assert_eq!(api.priority, Priority::High);
        assert_eq!(api.status, Status::Started);
        assert_eq!(api.title, "Fix flicker");
        assert_eq!(api.schema, ticket.schema);
        assert_eq!(
            api.claim_lease_expires_at.as_deref(),
            Some("2026-08-20T00:30:00Z")
        );
    }
}
