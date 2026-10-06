//! Provider-neutral ticket contract (`docs/16`).
//!
//! Provider-native ids stay strings at this boundary. The built-in git provider is
//! the only adapter that translates them to ULIDs and delegates to [`crate::ops`].

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use hotsheet_model::{
    CloseReason, Confidence, NoteKind, Priority, ReviewRequest, Status, Timestamp, Ulid,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::checkout_order::{self, MergeKey};
use crate::ops::{self, NewTicket, NoteMetadataInput, TicketPatch, TicketQuery};
use crate::wire::{ApiAttachment, ApiTicket};
use crate::{FsStore, OpError, StoreError};

/// A ticket's durable identity. Native ids are meaningful only within a connection.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TicketRef {
    pub connection_id: String,
    pub native_id: String,
}

impl TicketRef {
    pub fn qualified(&self) -> String {
        format!("{}:{}", self.connection_id, self.native_id)
    }
}

/// A ticket reference that remains routable when it crosses a client project boundary.
/// Connection ids are project-scoped, so the project id is part of the persisted form.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProjectTicketRef {
    pub project_id: String,
    pub connection_id: String,
    pub native_id: String,
}

impl ProjectTicketRef {
    pub fn qualified(&self) -> String {
        format!(
            "@{}/{}",
            self.project_id,
            TicketRef {
                connection_id: self.connection_id.clone(),
                native_id: self.native_id.clone(),
            }
            .qualified()
        )
    }

    pub fn from_qualified(value: &str) -> Option<Self> {
        let (project_id, ticket) = value.strip_prefix('@')?.split_once('/')?;
        let (connection_id, native_id) = ticket.split_once(':')?;
        if project_id.is_empty() || connection_id.is_empty() || native_id.is_empty() {
            return None;
        }
        Some(Self {
            project_id: project_id.to_string(),
            connection_id: connection_id.to_string(),
            native_id: native_id.to_string(),
        })
    }
}

/// Stable connection id for a git store, shared by CLI/MCP/server surfaces.
pub fn git_connection_id(store: &FsStore) -> String {
    let root = store
        .root()
        .canonicalize()
        .unwrap_or_else(|_| store.root().to_path_buf());
    let mut hash = Sha256::new();
    hash.update(root.to_string_lossy().as_bytes());
    format!("{:x}", hash.finalize())[..16].to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderDescriptor {
    pub connection_id: String,
    pub provider: String,
    pub display_name: String,
    pub locator: String,
    pub default: bool,
    pub capabilities: ProviderCapabilities,
}

/// Durable, non-secret project configuration for one provider connection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderConnection {
    /// Empty on a create request asks the host to generate one ([`generate_connection_id`]).
    #[serde(default)]
    pub id: String,
    pub provider: String,
    pub locator: String,
    pub name: Option<String>,
    #[serde(default)]
    pub default: bool,
    #[serde(default)]
    pub settings: serde_json::Value,
    /// Temporarily switched off: Hot Sheet neither reads from nor writes to the provider,
    /// and its tickets are not shown until it is enabled again (HS2-SF6W34).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub disabled: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ProviderConnectionsFile {
    #[serde(default)]
    connections: Vec<ProviderConnection>,
}

/// File-backed project connection registry. It stores locators/settings only; secret
/// values remain in [`crate::KeyRegistry`] and settings carry references.
#[derive(Debug, Clone)]
pub struct ProviderConfigRegistry {
    path: PathBuf,
}

impl ProviderConfigRegistry {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn load(&self) -> Result<Vec<ProviderConnection>, ProviderError> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(StoreError::Io(e).into()),
        };
        let file: ProviderConnectionsFile =
            serde_json::from_str(&text).map_err(|e| ProviderError::Conflict {
                ticket: self.path.display().to_string(),
                message: format!("invalid provider configuration: {e}"),
            })?;
        validate_connections(&file.connections)?;
        Ok(file.connections)
    }

    pub fn save(&self, connections: &[ProviderConnection]) -> Result<(), ProviderError> {
        validate_connections(connections)?;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(StoreError::Io)?;
        }
        let text = serde_json::to_string_pretty(&ProviderConnectionsFile {
            connections: connections.to_vec(),
        })
        .expect("provider connection config is serializable");
        std::fs::write(&self.path, format!("{text}\n")).map_err(StoreError::Io)?;
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Disable or re-enable one connection (HS2-SF6W34). Idempotent; `None` when no such
    /// connection exists. Shared by the server route and the headless CLI.
    pub fn set_disabled(
        &self,
        connection_id: &str,
        disabled: bool,
    ) -> Result<Option<ProviderConnection>, ProviderError> {
        let mut connections = self.load()?;
        let Some(connection) = connections
            .iter_mut()
            .find(|connection| connection.id == connection_id)
        else {
            return Ok(None);
        };
        if connection.disabled != disabled {
            connection.disabled = disabled;
            let updated = connection.clone();
            self.save(&connections)?;
            return Ok(Some(updated));
        }
        Ok(Some(connection.clone()))
    }

    /// The connection's record when it is disabled; lets hosts refuse before contacting it.
    pub fn disabled(&self, connection_id: &str) -> Result<bool, ProviderError> {
        Ok(self
            .load()?
            .iter()
            .any(|connection| connection.id == connection_id && connection.disabled))
    }
}

/// A readable, unique connection id for a new `provider` connection to `locator`
/// (`github-small-tale-hotsheet2`, then `-2`, `-3`, … on collision). Users never have to
/// invent one (HS2-48GA17).
pub fn generate_connection_id(
    existing: &[ProviderConnection],
    provider: &str,
    locator: &str,
) -> String {
    let mut base = String::new();
    for c in format!("{provider}-{locator}").chars() {
        if c.is_ascii_alphanumeric() {
            base.push(c.to_ascii_lowercase());
        } else if !base.ends_with('-') {
            base.push('-');
        }
    }
    let mut base = base.trim_matches('-').chars().take(48).collect::<String>();
    while base.ends_with('-') {
        base.pop();
    }
    if base.is_empty() {
        base = "connection".into();
    }
    let taken = |id: &str| existing.iter().any(|connection| connection.id == id);
    if !taken(&base) {
        return base;
    }
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find(|candidate| !taken(candidate))
        .expect("an unbounded suffix search always finds a free id")
}

fn validate_connections(connections: &[ProviderConnection]) -> Result<(), ProviderError> {
    let mut ids = std::collections::HashSet::new();
    let mut defaults = 0;
    for connection in connections {
        if connection.id.is_empty()
            || !connection
                .id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        {
            return Err(ProviderError::Conflict {
                ticket: connection.id.clone(),
                message: "connection id must contain only letters, digits, '.', '_' or '-'".into(),
            });
        }
        if !ids.insert(&connection.id) {
            return Err(ProviderError::Conflict {
                ticket: connection.id.clone(),
                message: "duplicate provider connection id".into(),
            });
        }
        defaults += usize::from(connection.default);
    }
    if defaults > 1 {
        return Err(ProviderError::Conflict {
            ticket: "providers".into(),
            message: "at most one provider connection may be the default".into(),
        });
    }
    Ok(())
}

/// Structured feature discovery. A client must not infer support from provider id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderCapabilities {
    pub create: bool,
    pub update: bool,
    pub close: bool,
    pub notes: bool,
    pub note_edit: bool,
    pub note_delete: bool,
    pub attachments: bool,
    /// Existing attachments can be renamed, deleted, re-labelled, and annotated
    /// (HS2-HSA64D). A provider that can only append evidence (GitHub's assets
    /// repository) reports `attachments` without this.
    #[serde(default)]
    pub attachment_edit: bool,
    pub assignment: bool,
    pub review_requests: bool,
    pub dependencies: bool,
    pub up_next: bool,
    pub close_reasons: bool,
    pub claims: bool,
    pub atomic_batch: bool,
    /// One all-or-nothing note/evidence/reopen operation.
    #[serde(default)]
    pub not_working_report: bool,
    /// Notes can carry a structured completion confidence score (HS2-DWTJ43).
    /// Providers without Hot Sheet note metadata reject a score explicitly.
    #[serde(default)]
    pub note_confidence: bool,
    pub offline_mutation: bool,
    pub history: bool,
    pub watch: bool,
    pub provider_idempotency: bool,
    pub query_fields: Vec<String>,
}

impl ProviderCapabilities {
    pub fn git() -> Self {
        Self {
            create: true,
            update: true,
            close: true,
            notes: true,
            note_edit: true,
            note_delete: true,
            attachments: true,
            attachment_edit: true,
            assignment: true,
            review_requests: true,
            dependencies: true,
            up_next: true,
            close_reasons: true,
            claims: true,
            atomic_batch: true,
            not_working_report: true,
            note_confidence: true,
            offline_mutation: true,
            history: true,
            watch: true,
            provider_idempotency: false,
            query_fields: vec![
                "status",
                "priority",
                "category",
                "tags",
                "text",
                "up_next",
                "close_reason",
                "assignee",
                "review",
                "claimed",
                "blocked",
                "created_at",
                "updated_at",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProviderDraft {
    pub title: String,
    pub category: String,
    pub priority: Priority,
    pub status: Status,
    pub details: String,
    pub tags: Vec<String>,
    pub up_next: bool,
    pub blocked_by: Vec<String>,
    pub transfer: Option<TransferProvenance>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferProvenance {
    pub operation_id: String,
    pub source: TicketRef,
}

#[derive(Debug, Clone, Default)]
pub struct ProviderPatch {
    pub expected_token: Option<String>,
    pub title: Option<String>,
    pub details: Option<String>,
    pub category: Option<String>,
    pub priority: Option<Priority>,
    pub status: Option<Status>,
    pub tags: Option<Vec<String>>,
    pub up_next: Option<bool>,
    pub blocked_by: Option<Vec<String>>,
    pub blocked_reason: Option<Option<String>>,
    /// Who is making the change (HS2-32QDZ3). The git provider attributes the status
    /// activity it appends; external trackers attribute changes to their own account.
    pub actor: Option<hotsheet_model::NoteActor>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProviderTicketSummary {
    pub total: u64,
    pub queued: u64,
    pub backlog: u64,
    pub archive: u64,
    /// Soft-deleted tickets; clients show the Trash view only while this is non-zero.
    pub trash: u64,
    pub open: u64,
    pub up_next: u64,
    pub active: u64,
    pub started: u64,
    pub verified: u64,
    pub completed_today: u64,
    pub completion_trend: Vec<u64>,
}

impl ProviderTicketSummary {
    pub fn add_ticket(&mut self, ticket: &ApiTicket, now: &str, day_starts: &[String]) {
        if ticket.status == Status::Moved {
            // Moved tombstones remain excluded from the live-ticket total, but Archive
            // surfaces them and its navigation count must match those visible rows.
            self.archive += 1;
            return;
        }
        self.total += 1;
        if ticket.status != Status::Backlog
            && !matches!(
                ticket.status,
                Status::Archive | Status::Deleted | Status::Moved
            )
        {
            self.queued += 1;
        }
        if ticket.status == Status::Backlog {
            self.backlog += 1;
        }
        if ticket.status == Status::Archive {
            self.archive += 1;
        }
        if ticket.status == Status::Deleted {
            self.trash += 1;
        }
        if matches!(ticket.status, Status::NotStarted | Status::Started) {
            self.open += 1;
        }
        if ticket.status == Status::Started {
            self.started += 1;
        }
        if ticket.status == Status::Verified {
            self.verified += 1;
        }
        if ticket.up_next && matches!(ticket.status, Status::NotStarted | Status::Started) {
            self.up_next += 1;
        }
        if ticket.claimed_by.is_some()
            && ticket
                .claim_lease_expires_at
                .as_ref()
                .is_some_and(|expiry| expiry.as_str() > now)
            && matches!(ticket.status, Status::NotStarted | Status::Started)
        {
            self.active += 1;
        }
        if let Some(completed) = ticket.completed_at.as_deref() {
            if day_starts
                .get(day_starts.len().saturating_sub(2))
                .zip(day_starts.last())
                .is_some_and(|(today, tomorrow)| completed >= today && completed < tomorrow)
            {
                self.completed_today += 1;
            }
            if self.completion_trend.len() < day_starts.len().saturating_sub(1) {
                self.completion_trend.resize(day_starts.len() - 1, 0);
            }
            if let Some(index) = day_starts.windows(2).position(|bounds| {
                completed >= bounds[0].as_str() && completed < bounds[1].as_str()
            }) {
                self.completion_trend[index] += 1;
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProviderTicketPage {
    pub items: Vec<ApiTicket>,
    pub next_cursor: Option<String>,
}

/// One row of a value-keyset provider read (HS2-74H84S). `resume` is a provider-owned hint
/// positioned at or before this row; passing it back lets the adapter skip rows it already
/// scanned, but adapters must stay correct when the hint is stale.
#[derive(Debug, Clone)]
pub struct ProviderKeysetItem {
    pub ticket: ApiTicket,
    pub resume: Option<String>,
}

/// A bounded value-keyset read: rows strictly after the requested key, in checkout order.
/// `exhausted` means no further matching row existed after the last returned item.
#[derive(Debug, Clone, Default)]
pub struct ProviderKeysetPage {
    pub items: Vec<ProviderKeysetItem>,
    pub exhausted: bool,
}

fn after_matches(ticket: &ApiTicket, after: Option<&MergeKey>, query: &TicketQuery) -> bool {
    after.is_none_or(|after| {
        checkout_order::compare(
            &MergeKey::from_ticket(ticket),
            after,
            query.sort,
            query.descending,
        )
        .is_gt()
    })
}

/// Value-keyset page over a complete provider result (the fallback for providers or sorts
/// without a native keyset): keep rows strictly after `after`, in checkout order.
#[must_use]
pub fn keyset_page_from_rows(
    rows: Vec<ApiTicket>,
    query: &TicketQuery,
    after: Option<&MergeKey>,
    limit: usize,
) -> ProviderKeysetPage {
    let mut rows = rows
        .into_iter()
        .filter(|ticket| after_matches(ticket, after, query))
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| {
        checkout_order::compare(
            &MergeKey::from_ticket(left),
            &MergeKey::from_ticket(right),
            query.sort,
            query.descending,
        )
    });
    let exhausted = rows.len() <= limit;
    rows.truncate(limit);
    ProviderKeysetPage {
        items: rows
            .into_iter()
            .map(|ticket| ProviderKeysetItem {
                ticket,
                resume: None,
            })
            .collect(),
        exhausted,
    }
}

/// Value-keyset page over native pages already ordered by the checkout order for `query`.
///
/// `fetch(cursor)` returns one native page (unfiltered rows plus the next native cursor);
/// `None` is the first page. The walk starts at `resume` (the native cursor of the page that
/// held the last consumed row), drops rows at or before `after`, and applies provider-neutral
/// filters. Page cursors are positional, so an earlier deletion can shift unconsumed rows onto
/// earlier pages: when the resumed page's first row already sorts after `after` (or the page
/// vanished), the walk restarts from the first page. Rows are therefore never skipped or
/// repeated, whatever else was inserted or deleted.
pub fn keyset_page_from_native_pages(
    query: &TicketQuery,
    after: Option<&MergeKey>,
    resume: Option<&str>,
    limit: usize,
    mut fetch: impl FnMut(Option<&str>) -> Result<(Vec<ApiTicket>, Option<String>), ProviderError>,
) -> Result<ProviderKeysetPage, ProviderError> {
    let mut cursor = resume.map(str::to_owned);
    let mut first_fetch = true;
    let mut items = Vec::new();
    loop {
        let (rows, next) = fetch(cursor.as_deref())?;
        if first_fetch && cursor.is_some() {
            first_fetch = false;
            let resumed_past_key = match (after, rows.first()) {
                (_, None) => true,
                (Some(_), Some(head)) => after_matches(head, after, query),
                (None, Some(_)) => false,
            };
            if resumed_past_key {
                cursor = None;
                continue;
            }
        }
        first_fetch = false;
        let rows = rows
            .into_iter()
            .filter(|ticket| after_matches(ticket, after, query))
            .collect::<Vec<_>>();
        for ticket in filter_provider_ticket_page(rows, query) {
            if items.len() == limit {
                return Ok(ProviderKeysetPage {
                    items,
                    exhausted: false,
                });
            }
            items.push(ProviderKeysetItem {
                ticket,
                resume: cursor.clone(),
            });
        }
        match next {
            Some(next) if cursor.as_deref() != Some(next.as_str()) => {
                if items.len() == limit {
                    return Ok(ProviderKeysetPage {
                        items,
                        exhausted: false,
                    });
                }
                cursor = Some(next);
            }
            Some(_) => {
                return Err(ProviderError::Conflict {
                    ticket: String::new(),
                    message: "provider returned a non-advancing ticket cursor".into(),
                });
            }
            None => {
                return Ok(ProviderKeysetPage {
                    items,
                    exhausted: true,
                });
            }
        }
    }
}

/// Apply provider-neutral filters to one bounded native page. Adapters remain responsible
/// for rejecting fields their native records cannot represent and for requesting a stable
/// native ordering before calling this helper.
pub fn filter_provider_ticket_page(
    mut tickets: Vec<ApiTicket>,
    query: &TicketQuery,
) -> Vec<ApiTicket> {
    tickets.retain(|ticket| {
        provider_text_matches(ticket, query.text.as_deref())
            && query.status.is_none_or(|value| ticket.status == value)
            && query.collection.is_none_or(|collection| match collection {
                crate::TicketCollection::Queue => !matches!(
                    ticket.status,
                    Status::Backlog | Status::Archive | Status::Deleted | Status::Moved
                ),
                crate::TicketCollection::Archive => {
                    matches!(ticket.status, Status::Archive | Status::Moved)
                }
                crate::TicketCollection::Trash => ticket.status == Status::Deleted,
            })
            && query.priority.is_none_or(|value| ticket.priority == value)
            && query
                .category
                .as_deref()
                .is_none_or(|value| ticket.category == value)
            && query.tags.iter().all(|tag| ticket.tags.contains(tag))
            && (!query.open_only || matches!(ticket.status, Status::NotStarted | Status::Started))
            && query
                .close_reason
                .is_none_or(|value| ticket.close_reason == Some(value))
            && query
                .closed
                .is_none_or(|value| ticket.close_reason.is_some() == value)
            && query
                .assignee
                .as_deref()
                .is_none_or(|value| ticket.assignees.iter().any(|assignee| assignee == value))
            && query
                .created_after
                .as_deref()
                .is_none_or(|value| ticket.created_at.as_str() >= value)
            && query
                .created_before
                .as_deref()
                .is_none_or(|value| ticket.created_at.as_str() <= value)
            && query
                .updated_after
                .as_deref()
                .is_none_or(|value| ticket.updated_at.as_str() >= value)
            && query
                .updated_before
                .as_deref()
                .is_none_or(|value| ticket.updated_at.as_str() <= value)
            && crate::ops::confidence_in_range(
                ticket
                    .latest_confidence
                    .and_then(|score| Confidence::new(u64::from(score)).ok()),
                query.min_confidence,
                query.max_confidence,
            )
    });
    tickets
        .sort_by(|left, right| compare_provider_tickets(left, right, query.sort, query.descending));
    if let Some(limit) = query.limit {
        tickets.truncate(limit);
    }
    tickets
}

/// Match the fields available in a provider's list response with the local index's
/// word-prefix, all-terms search semantics. Detail-only notes are not fetched for lists.
pub fn provider_text_matches(ticket: &ApiTicket, text: Option<&str>) -> bool {
    let Some(text) = text else { return true };
    let terms = text
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|term| !term.is_empty())
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>();
    if terms.is_empty() {
        return true;
    }
    let searchable = std::iter::once(ticket.slug.as_str())
        .chain([ticket.title.as_str(), ticket.details.as_str()])
        .chain(ticket.tags.iter().map(String::as_str))
        .flat_map(|field| field.split(|character: char| !character.is_ascii_alphanumeric()))
        .filter(|word| !word.is_empty())
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>();
    terms
        .iter()
        .all(|term| searchable.iter().any(|word| word.starts_with(term)))
}

/// The query without paging or keyset limits, for adapters that read a complete result.
#[must_use]
pub fn unbounded_query(query: &TicketQuery) -> TicketQuery {
    let mut unbounded = query.clone();
    unbounded.limit = None;
    unbounded.page_after = None;
    unbounded.after_key = None;
    unbounded
}

/// Total provider order used by adapters and checkout-level k-way pagination. Categorical
/// directions affect only their primary key; ties stay recent-first and then use qualified
/// identity so different providers cannot compare equal.
pub fn compare_provider_tickets(
    left: &ApiTicket,
    right: &ApiTicket,
    sort: crate::SortKey,
    descending: bool,
) -> std::cmp::Ordering {
    let directed = |order: std::cmp::Ordering| {
        if descending { order.reverse() } else { order }
    };
    match sort {
        crate::SortKey::Id => directed(left.native_id.cmp(&right.native_id))
            .then_with(|| directed(left.qualified_id.cmp(&right.qualified_id))),
        crate::SortKey::Created => directed(left.created_at.cmp(&right.created_at))
            .then_with(|| directed(left.qualified_id.cmp(&right.qualified_id))),
        crate::SortKey::Updated => directed(left.updated_at.cmp(&right.updated_at))
            .then_with(|| directed(left.qualified_id.cmp(&right.qualified_id))),
        crate::SortKey::Priority => directed((left.priority as u8).cmp(&(right.priority as u8)))
            .then_with(|| right.updated_at.cmp(&left.updated_at))
            .then_with(|| left.qualified_id.cmp(&right.qualified_id)),
        crate::SortKey::Status => directed((left.status as u8).cmp(&(right.status as u8)))
            .then_with(|| right.updated_at.cmp(&left.updated_at))
            .then_with(|| left.qualified_id.cmp(&right.qualified_id)),
        crate::SortKey::Title => directed(
            crate::checkout_order::title_fold(&left.title)
                .cmp(&crate::checkout_order::title_fold(&right.title)),
        )
        .then_with(|| right.updated_at.cmp(&left.updated_at))
        .then_with(|| left.qualified_id.cmp(&right.qualified_id)),
        crate::SortKey::Confidence => directed(
            crate::checkout_order::confidence_rank(left.latest_confidence).cmp(
                &crate::checkout_order::confidence_rank(right.latest_confidence),
            ),
        )
        .then_with(|| right.updated_at.cmp(&left.updated_at))
        .then_with(|| left.qualified_id.cmp(&right.qualified_id)),
    }
}

/// Caller-owned time/id inputs keep provider implementations deterministic in tests.
/// `generated_id` is also the provider-neutral idempotency key: adapters must return the
/// existing object when a create/note request with the same id is retried. Providers whose
/// remote API chooses ids should persist or recognize the caller id in remote provenance.
#[derive(Debug, Clone)]
pub struct MutationContext {
    pub now: Timestamp,
    pub generated_id: Ulid,
}

#[derive(Debug, Clone)]
pub struct ProviderEvidence {
    pub id: Ulid,
    pub filename: String,
    pub created_at: Timestamp,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct NotWorkingReport {
    pub expected_token: Option<String>,
    pub note: Option<(Ulid, String)>,
    pub evidence: Vec<ProviderEvidence>,
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("ticket provider connection '{0}' was not found")]
    UnknownConnection(String),
    #[error("ticket '{native_id}' was not found in provider connection '{connection_id}'")]
    NotFound {
        connection_id: String,
        native_id: String,
    },
    #[error("provider connection '{connection_id}' does not support '{capability}'")]
    Unsupported {
        connection_id: String,
        capability: &'static str,
    },
    #[error("provider authentication failed for '{connection_id}': {message}")]
    Authentication {
        connection_id: String,
        message: String,
    },
    #[error("provider conflict for '{ticket}': {message}")]
    Conflict { ticket: String, message: String },
    #[error("provider '{connection_id}' is rate limited{retry}", retry = retry_after_seconds.map(|n| format!("; retry after {n}s")).unwrap_or_default())]
    RateLimited {
        connection_id: String,
        retry_after_seconds: Option<u64>,
    },
    #[error(
        "provider connection '{connection_id}' is disabled; enable it to read or change its tickets"
    )]
    Disabled { connection_id: String },
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Operation(#[from] OpError),
    /// `id` does not have the shape `provider` uses for native ids (e.g. a git ULID sent to
    /// GitHub, which numbers its issues), so it cannot name a ticket in that provider.
    #[error("invalid native id '{id}' for the {provider} provider")]
    InvalidNativeId { provider: &'static str, id: String },
}

/// Synchronous domain boundary; async network adapters are wrapped at the host edge.
pub trait TicketProvider: Send + Sync {
    fn descriptor(&self) -> ProviderDescriptor;
    fn supports_note_edit(&self) -> bool {
        self.descriptor().capabilities.note_edit
    }
    fn supports_note_delete(&self) -> bool {
        self.descriptor().capabilities.note_delete
    }
    fn supports_note_confidence(&self) -> bool {
        self.descriptor().capabilities.note_confidence
    }
    fn query(&self, query: &TicketQuery) -> Result<Vec<ApiTicket>, ProviderError>;
    /// Return one bounded page. The cursor is provider-owned and opaque to the host.
    fn query_page(
        &self,
        query: &TicketQuery,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<ProviderTicketPage, ProviderError> {
        let mut unbounded = query.clone();
        unbounded.limit = None;
        unbounded.page_after = None;
        let rows = self.query(&unbounded)?;
        let offset =
            cursor
                .unwrap_or("0")
                .parse::<usize>()
                .map_err(|_| ProviderError::Conflict {
                    ticket: self.descriptor().connection_id,
                    message: "invalid provider page cursor".into(),
                })?;
        let end = offset.saturating_add(limit).min(rows.len());
        Ok(ProviderTicketPage {
            items: rows[offset.min(rows.len())..end].to_vec(),
            next_cursor: (end < rows.len()).then(|| end.to_string()),
        })
    }
    /// Value-keyset read (HS2-74H84S): up to `limit` rows sorting strictly after `after` in
    /// the shared `checkout_order` total order. `resume` is a hint from a previous
    /// [`ProviderKeysetItem`]; implementations must remain correct when it is stale, so
    /// inserts, edits, and deletions elsewhere never skip or repeat an unchanged row.
    fn query_after(
        &self,
        query: &TicketQuery,
        after: Option<&MergeKey>,
        _resume: Option<&str>,
        limit: usize,
    ) -> Result<ProviderKeysetPage, ProviderError> {
        Ok(keyset_page_from_rows(
            self.query(&unbounded_query(query))?,
            query,
            after,
            limit,
        ))
    }
    /// Aggregate navigation counts without requiring the host to retain provider rows.
    fn summary(
        &self,
        now: &str,
        day_starts: &[String],
    ) -> Result<ProviderTicketSummary, ProviderError> {
        let mut summary = ProviderTicketSummary::default();
        for ticket in self.query(&TicketQuery::default())? {
            summary.add_ticket(&ticket, now, day_starts);
        }
        Ok(summary)
    }
    fn find_transfer(&self, operation_id: &str) -> Result<Option<ApiTicket>, ProviderError>;
    fn get(&self, native_id: &str) -> Result<ApiTicket, ProviderError>;
    fn create(
        &self,
        ctx: MutationContext,
        draft: ProviderDraft,
    ) -> Result<ApiTicket, ProviderError>;
    fn update(
        &self,
        native_id: &str,
        now: Timestamp,
        patch: ProviderPatch,
    ) -> Result<ApiTicket, ProviderError>;
    fn add_note(
        &self,
        native_id: &str,
        ctx: MutationContext,
        kind: NoteKind,
        text: String,
    ) -> Result<ApiTicket, ProviderError>;
    /// Append a note with a concise timeline headline when the provider can preserve
    /// Hot Sheet note metadata. External trackers may fall back to their ordinary
    /// comment representation without losing the full note body.
    fn add_note_with_summary(
        &self,
        native_id: &str,
        ctx: MutationContext,
        kind: NoteKind,
        _summary: Option<String>,
        text: String,
    ) -> Result<ApiTicket, ProviderError> {
        self.add_note(native_id, ctx, kind, text)
    }
    /// Append a note with every optional metadata field. A confidence score fails
    /// explicitly on a provider whose `note_confidence` capability is off rather than
    /// being silently dropped (HS2-DWTJ43); the summary keeps its best-effort fallback.
    fn add_note_with_metadata(
        &self,
        native_id: &str,
        ctx: MutationContext,
        kind: NoteKind,
        metadata: NoteMetadataInput,
        text: String,
    ) -> Result<ApiTicket, ProviderError> {
        if metadata.confidence.is_some() && !self.supports_note_confidence() {
            return Err(ProviderError::Unsupported {
                connection_id: self.descriptor().connection_id,
                capability: "note_confidence",
            });
        }
        self.add_note_with_summary(native_id, ctx, kind, metadata.summary, text)
    }
    fn report_not_working(
        &self,
        _native_id: &str,
        _now: Timestamp,
        _report: NotWorkingReport,
    ) -> Result<ApiTicket, ProviderError> {
        Err(ProviderError::Unsupported {
            connection_id: self.descriptor().connection_id,
            capability: "not_working_report",
        })
    }
    fn edit_note(
        &self,
        _native_id: &str,
        _note_id: &str,
        _now: Timestamp,
        _text: String,
    ) -> Result<ApiTicket, ProviderError> {
        Err(ProviderError::Unsupported {
            connection_id: self.descriptor().connection_id,
            capability: "note_edit",
        })
    }
    /// Edit a note's text and/or completion confidence (HS2-CY4CWC). A confidence change
    /// fails explicitly unless the provider overrides this to carry it; a text-only edit
    /// delegates to [`TicketProvider::edit_note`].
    fn edit_note_with_metadata(
        &self,
        native_id: &str,
        note_id: &str,
        now: Timestamp,
        edit: ops::NoteEditInput,
    ) -> Result<ApiTicket, ProviderError> {
        if edit.confidence.is_some() {
            return Err(ProviderError::Unsupported {
                connection_id: self.descriptor().connection_id,
                capability: "note_confidence",
            });
        }
        match edit.text {
            Some(text) => self.edit_note(native_id, note_id, now, text),
            None => self.get(native_id),
        }
    }
    fn delete_note(
        &self,
        _native_id: &str,
        _note_id: &str,
        _now: Timestamp,
    ) -> Result<ApiTicket, ProviderError> {
        Err(ProviderError::Unsupported {
            connection_id: self.descriptor().connection_id,
            capability: "note_delete",
        })
    }
    fn attachment_bytes(
        &self,
        _native_id: &str,
        _attachment_id: &str,
    ) -> Result<Vec<u8>, ProviderError> {
        Err(ProviderError::Unsupported {
            connection_id: self.descriptor().connection_id,
            capability: "attachments",
        })
    }
    fn add_attachment(
        &self,
        _native_id: &str,
        _attachment: ApiAttachment,
        _bytes: Vec<u8>,
    ) -> Result<ApiTicket, ProviderError> {
        Err(ProviderError::Unsupported {
            connection_id: self.descriptor().connection_id,
            capability: "attachments",
        })
    }
    fn close(
        &self,
        native_id: &str,
        now: Timestamp,
        reason: CloseReason,
        duplicate_of: Option<String>,
    ) -> Result<ApiTicket, ProviderError>;
    fn assign(
        &self,
        native_id: &str,
        now: Timestamp,
        assignees: Option<Vec<String>>,
        reviews: Vec<ReviewRequest>,
    ) -> Result<ApiTicket, ProviderError>;
    fn claim_next(
        &self,
        now: Timestamp,
        lease_expires: Timestamp,
        worker: &str,
        label: Option<String>,
    ) -> Result<Option<ApiTicket>, ProviderError>;
    fn release(
        &self,
        native_id: &str,
        now: Timestamp,
        worker: &str,
        force: bool,
    ) -> Result<ApiTicket, ProviderError>;
    fn renew(
        &self,
        native_id: &str,
        now: Timestamp,
        lease_expires: Timestamp,
        worker: &str,
    ) -> Result<ApiTicket, ProviderError>;
}

#[derive(Debug, Clone)]
pub struct GitProvider {
    connection_id: String,
    display_name: String,
    store: FsStore,
    is_default: bool,
    transfer_lock: Arc<Mutex<()>>,
    #[cfg(test)]
    fail_close_once: Arc<std::sync::atomic::AtomicBool>,
    #[cfg(test)]
    test_capabilities: Option<ProviderCapabilities>,
}

impl GitProvider {
    pub fn new(connection_id: impl Into<String>, store: FsStore) -> Self {
        Self {
            connection_id: connection_id.into(),
            display_name: "Git tickets".into(),
            store,
            is_default: false,
            transfer_lock: Arc::new(Mutex::new(())),
            #[cfg(test)]
            fail_close_once: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            #[cfg(test)]
            test_capabilities: None,
        }
    }

    pub fn with_default(mut self, is_default: bool) -> Self {
        self.is_default = is_default;
        self
    }

    pub fn store(&self) -> &FsStore {
        &self.store
    }

    #[cfg(test)]
    fn with_close_failure_once(self) -> Self {
        self.fail_close_once
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self
    }

    #[cfg(test)]
    fn with_test_capabilities(mut self, capabilities: ProviderCapabilities) -> Self {
        self.test_capabilities = Some(capabilities);
        self
    }

    fn ticket(&self, native_id: &str) -> Result<hotsheet_model::Ticket, ProviderError> {
        ops::resolve(&self.store, native_id)?.ok_or_else(|| ProviderError::NotFound {
            connection_id: self.connection_id.clone(),
            native_id: native_id.to_string(),
        })
    }

    fn blockers(&self, values: &[String]) -> Result<Vec<Ulid>, ProviderError> {
        Ok(ops::resolve_blockers(&self.store, None, values)?)
    }

    fn capabilities(&self) -> ProviderCapabilities {
        #[cfg(test)]
        if let Some(capabilities) = &self.test_capabilities {
            return capabilities.clone();
        }
        ProviderCapabilities::git()
    }
}

impl TicketProvider for GitProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            connection_id: self.connection_id.clone(),
            provider: "git".into(),
            display_name: self.display_name.clone(),
            locator: self.store.root().display().to_string(),
            default: self.is_default,
            capabilities: self.capabilities(),
        }
    }

    fn supports_note_edit(&self) -> bool {
        true
    }

    fn query(&self, query: &TicketQuery) -> Result<Vec<ApiTicket>, ProviderError> {
        Ok(ops::query(&self.store, query)?
            .iter()
            .map(|ticket| ApiTicket::from_provider(ticket, &self.connection_id, None))
            .collect())
    }

    fn find_transfer(&self, operation_id: &str) -> Result<Option<ApiTicket>, ProviderError> {
        Ok(self
            .store
            .list_tickets()?
            .into_iter()
            .find(|ticket| ticket.transfer_operation_id.as_deref() == Some(operation_id))
            .as_ref()
            .map(|ticket| ApiTicket::from_provider(ticket, &self.connection_id, None)))
    }

    fn get(&self, native_id: &str) -> Result<ApiTicket, ProviderError> {
        let ticket = self.ticket(native_id)?;
        Ok(ApiTicket::from_provider(&ticket, &self.connection_id, None))
    }

    fn create(
        &self,
        ctx: MutationContext,
        draft: ProviderDraft,
    ) -> Result<ApiTicket, ProviderError> {
        let prefix = self.store.metadata()?.ticket_prefix;
        let blocked_by = self.blockers(&draft.blocked_by)?;
        let _transfer_guard = if draft.transfer.is_some() {
            Some(
                self.transfer_lock
                    .lock()
                    .map_err(|_| ProviderError::Conflict {
                        ticket: self.connection_id.clone(),
                        message: "transfer lock poisoned".into(),
                    })?,
            )
        } else {
            None
        };
        if let Some(transfer) = &draft.transfer
            && let Some(existing) = self.find_transfer(&transfer.operation_id)?
        {
            if existing.transferred_from.as_deref() == Some(&transfer.source.qualified()) {
                return Ok(existing);
            }
            return Err(ProviderError::Conflict {
                ticket: transfer.operation_id.clone(),
                message: "transfer operation id is already associated with another source".into(),
            });
        }
        let mut ticket = ops::create(
            &self.store,
            ctx.generated_id,
            &prefix,
            ctx.now,
            NewTicket {
                title: draft.title,
                category: draft.category,
                priority: draft.priority,
                status: draft.status,
                details: draft.details,
                tags: draft.tags,
                up_next: draft.up_next,
                blocked_by,
            },
        )?;
        if let Some(transfer) = draft.transfer {
            ticket.transfer_operation_id = Some(transfer.operation_id);
            ticket.transferred_from = Some(transfer.source.qualified());
            self.store.write_ticket_committing(&ticket)?;
        }
        Ok(ApiTicket::from_provider(&ticket, &self.connection_id, None))
    }

    fn update(
        &self,
        native_id: &str,
        now: Timestamp,
        patch: ProviderPatch,
    ) -> Result<ApiTicket, ProviderError> {
        let ticket = self.ticket(native_id)?;
        if patch
            .expected_token
            .as_deref()
            .is_some_and(|token| token != ticket.updated_at.as_str())
        {
            return Err(ProviderError::Conflict {
                ticket: native_id.into(),
                message: "ticket changed since it was read".into(),
            });
        }
        let blocked_by = patch
            .blocked_by
            .as_deref()
            .map(|v| self.blockers(v))
            .transpose()?;
        let updated = ops::update(
            &self.store,
            &ticket.id,
            now,
            TicketPatch {
                actor: patch.actor,
                title: patch.title,
                details: patch.details,
                category: patch.category,
                priority: patch.priority,
                status: patch.status,
                tags: patch.tags,
                up_next: patch.up_next,
                blocked_by,
                blocked_reason: patch.blocked_reason,
            },
        )?;
        Ok(ApiTicket::from_provider(
            &updated,
            &self.connection_id,
            None,
        ))
    }

    fn add_note(
        &self,
        native_id: &str,
        ctx: MutationContext,
        kind: NoteKind,
        text: String,
    ) -> Result<ApiTicket, ProviderError> {
        let ticket = self.ticket(native_id)?;
        if ticket.notes.iter().any(|note| note.id == ctx.generated_id) {
            return Ok(ApiTicket::from_provider(&ticket, &self.connection_id, None));
        }
        let updated = ops::add_note(
            &self.store,
            &ticket.id,
            ctx.generated_id,
            ctx.now,
            kind,
            text,
        )?;
        Ok(ApiTicket::from_provider(
            &updated,
            &self.connection_id,
            None,
        ))
    }

    fn add_note_with_summary(
        &self,
        native_id: &str,
        ctx: MutationContext,
        kind: NoteKind,
        summary: Option<String>,
        text: String,
    ) -> Result<ApiTicket, ProviderError> {
        self.add_note_with_metadata(
            native_id,
            ctx,
            kind,
            NoteMetadataInput {
                summary,
                confidence: None,
                actor: None,
            },
            text,
        )
    }

    fn add_note_with_metadata(
        &self,
        native_id: &str,
        ctx: MutationContext,
        kind: NoteKind,
        metadata: NoteMetadataInput,
        text: String,
    ) -> Result<ApiTicket, ProviderError> {
        if metadata.confidence.is_some() && !self.supports_note_confidence() {
            return Err(ProviderError::Unsupported {
                connection_id: self.connection_id.clone(),
                capability: "note_confidence",
            });
        }
        let ticket = self.ticket(native_id)?;
        if ticket.notes.iter().any(|note| note.id == ctx.generated_id) {
            return Ok(ApiTicket::from_provider(&ticket, &self.connection_id, None));
        }
        let updated = ops::add_note_with_metadata(
            &self.store,
            &ticket.id,
            ctx.generated_id,
            ctx.now,
            kind,
            metadata,
            text,
        )?;
        Ok(ApiTicket::from_provider(
            &updated,
            &self.connection_id,
            None,
        ))
    }

    fn report_not_working(
        &self,
        native_id: &str,
        now: Timestamp,
        report: NotWorkingReport,
    ) -> Result<ApiTicket, ProviderError> {
        if !self.capabilities().not_working_report {
            return Err(ProviderError::Unsupported {
                connection_id: self.connection_id.clone(),
                capability: "not_working_report",
            });
        }
        let mut ticket = self.ticket(native_id)?;
        if report
            .expected_token
            .as_deref()
            .is_some_and(|token| token != ticket.updated_at.as_str())
        {
            return Err(ProviderError::Conflict {
                ticket: native_id.into(),
                message: "ticket changed since it was read".into(),
            });
        }
        if report.evidence.iter().any(|item| {
            ticket
                .attachments
                .iter()
                .any(|current| current.id == item.id)
        }) {
            return Err(ProviderError::Conflict {
                ticket: native_id.into(),
                message: "evidence attachment id already exists".into(),
            });
        }
        let reporter = crate::current_user_name(self.store.root());
        let evidence_batch = format!("batch-{}", hotsheet_model::Ulid::new());
        ops::prepare_not_working(
            &mut ticket,
            now,
            report.note,
            !report.evidence.is_empty(),
            reporter.as_deref(),
        )?;
        let evidence = report
            .evidence
            .into_iter()
            .map(|item| crate::store::AtomicAttachment {
                id: item.id,
                filename: item.filename,
                created_at: item.created_at,
                bytes: item.bytes,
            })
            .collect::<Vec<_>>();
        for item in &evidence {
            ticket.attachments.push(hotsheet_model::Attachment {
                id: item.id,
                filename: item.sanitized_filename(),
                created_at: item.created_at.clone(),
                batch_id: Some(evidence_batch.clone()),
                batch_label: None,
                actor: Some(hotsheet_model::AttachmentActor {
                    identity: None,
                    display_name: reporter.clone(),
                    role: hotsheet_model::AttachmentActorRole::Human,
                }),
                purpose: Some(hotsheet_model::AttachmentPurpose::ProblemEvidence),
                annotations: Vec::new(),
            });
        }
        ticket.attachments.sort_by(|a, b| {
            a.created_at
                .chronological_cmp(&b.created_at)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.id.cmp(&b.id))
        });
        ticket = self
            .store
            .write_ticket_with_attachments_atomic(&ticket, &evidence)?;
        Ok(ApiTicket::from_provider(&ticket, &self.connection_id, None))
    }

    fn edit_note(
        &self,
        native_id: &str,
        note_id: &str,
        now: Timestamp,
        text: String,
    ) -> Result<ApiTicket, ProviderError> {
        self.edit_note_with_metadata(
            native_id,
            note_id,
            now,
            ops::NoteEditInput {
                text: Some(text),
                confidence: None,
            },
        )
    }

    fn edit_note_with_metadata(
        &self,
        native_id: &str,
        note_id: &str,
        now: Timestamp,
        edit: ops::NoteEditInput,
    ) -> Result<ApiTicket, ProviderError> {
        if edit.confidence.is_some() && !self.supports_note_confidence() {
            return Err(ProviderError::Unsupported {
                connection_id: self.connection_id.clone(),
                capability: "note_confidence",
            });
        }
        let ticket = self.ticket(native_id)?;
        let note_id = Ulid::from_string(note_id).map_err(|_| ProviderError::InvalidNativeId {
            provider: "git",
            id: note_id.into(),
        })?;
        let updated = ops::edit_note_with_metadata(&self.store, &ticket.id, &note_id, now, edit)?;
        Ok(ApiTicket::from_provider(
            &updated,
            &self.connection_id,
            None,
        ))
    }

    fn delete_note(
        &self,
        native_id: &str,
        note_id: &str,
        now: Timestamp,
    ) -> Result<ApiTicket, ProviderError> {
        let ticket = self.ticket(native_id)?;
        let note_id = Ulid::from_string(note_id).map_err(|_| ProviderError::InvalidNativeId {
            provider: "git",
            id: note_id.into(),
        })?;
        let updated = ops::delete_note(&self.store, &ticket.id, &note_id, now)?;
        Ok(ApiTicket::from_provider(
            &updated,
            &self.connection_id,
            None,
        ))
    }

    fn attachment_bytes(
        &self,
        native_id: &str,
        attachment_id: &str,
    ) -> Result<Vec<u8>, ProviderError> {
        let ticket = self.ticket(native_id)?;
        let attachment_id =
            Ulid::from_string(attachment_id).map_err(|_| ProviderError::InvalidNativeId {
                provider: "git",
                id: attachment_id.into(),
            })?;
        let attachment = ticket
            .attachments
            .iter()
            .find(|item| item.id == attachment_id)
            .ok_or_else(|| ProviderError::NotFound {
                connection_id: self.connection_id.clone(),
                native_id: attachment_id.to_string(),
            })?;
        Ok(std::fs::read(
            self.store
                .attachment_dir(&ticket.id)
                .join(attachment.id.to_string())
                .join(&attachment.filename),
        )
        .map_err(StoreError::Io)?)
    }

    fn add_attachment(
        &self,
        native_id: &str,
        attachment: ApiAttachment,
        bytes: Vec<u8>,
    ) -> Result<ApiTicket, ProviderError> {
        let ticket = self.ticket(native_id)?;
        let attachment_id =
            Ulid::from_string(&attachment.id).map_err(|_| ProviderError::InvalidNativeId {
                provider: "git",
                id: attachment.id,
            })?;
        let (updated, _) = self.store.write_attachment_with_metadata(
            &ticket.id,
            attachment_id,
            Timestamp::new(attachment.created_at),
            &attachment.filename,
            &bytes,
            hotsheet_model::AttachmentMetadata {
                batch_id: attachment.batch_id,
                batch_label: attachment.batch_label,
                actor: attachment.actor,
                purpose: attachment.purpose,
            },
        )?;
        Ok(ApiTicket::from_provider(
            &updated,
            &self.connection_id,
            None,
        ))
    }

    fn close(
        &self,
        native_id: &str,
        now: Timestamp,
        reason: CloseReason,
        duplicate_of: Option<String>,
    ) -> Result<ApiTicket, ProviderError> {
        #[cfg(test)]
        if self
            .fail_close_once
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            return Err(ProviderError::Conflict {
                ticket: native_id.into(),
                message: "injected close failure".into(),
            });
        }
        let ticket = self.ticket(native_id)?;
        let duplicate = duplicate_of
            .map(|reference| {
                if !reference.starts_with('@') {
                    let native_id = reference
                        .strip_prefix(&format!("{}:", self.connection_id))
                        .unwrap_or(&reference);
                    self.ticket(native_id)?;
                }
                Ok::<_, ProviderError>(reference)
            })
            .transpose()?;
        let updated = ops::close(&self.store, &ticket.id, now, reason, duplicate)?;
        Ok(ApiTicket::from_provider(
            &updated,
            &self.connection_id,
            None,
        ))
    }

    fn assign(
        &self,
        native_id: &str,
        now: Timestamp,
        assignees: Option<Vec<String>>,
        reviews: Vec<ReviewRequest>,
    ) -> Result<ApiTicket, ProviderError> {
        let ticket = self.ticket(native_id)?;
        let updated = ops::assign(&self.store, &ticket.id, now, assignees, reviews)?;
        Ok(ApiTicket::from_provider(
            &updated,
            &self.connection_id,
            None,
        ))
    }

    fn claim_next(
        &self,
        now: Timestamp,
        lease_expires: Timestamp,
        worker: &str,
        label: Option<String>,
    ) -> Result<Option<ApiTicket>, ProviderError> {
        Ok(
            ops::claim_next(&self.store, &now, lease_expires, worker, label)?
                .as_ref()
                .map(|ticket| ApiTicket::from_provider(ticket, &self.connection_id, None)),
        )
    }

    fn release(
        &self,
        native_id: &str,
        now: Timestamp,
        worker: &str,
        force: bool,
    ) -> Result<ApiTicket, ProviderError> {
        let ticket = self.ticket(native_id)?;
        let updated = ops::release(&self.store, &ticket.id, now, worker, force)?;
        Ok(ApiTicket::from_provider(
            &updated,
            &self.connection_id,
            None,
        ))
    }

    fn renew(
        &self,
        native_id: &str,
        now: Timestamp,
        lease_expires: Timestamp,
        worker: &str,
    ) -> Result<ApiTicket, ProviderError> {
        let ticket = self.ticket(native_id)?;
        let updated = ops::renew(&self.store, &ticket.id, now, lease_expires, worker)?;
        Ok(ApiTicket::from_provider(
            &updated,
            &self.connection_id,
            None,
        ))
    }
}

/// Project-scoped provider routing and aggregation.
#[derive(Clone, Default)]
pub struct ProviderRegistry {
    providers: Arc<Mutex<HashMap<String, Arc<dyn TicketProvider>>>>,
    transfer_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
}

impl ProviderRegistry {
    pub fn register(&self, provider: Arc<dyn TicketProvider>) -> Result<(), ProviderError> {
        let id = provider.descriptor().connection_id;
        let mut providers = self.providers.lock().map_err(|_| ProviderError::Conflict {
            ticket: id.clone(),
            message: "provider registry lock poisoned".into(),
        })?;
        providers.insert(id, provider);
        Ok(())
    }

    pub fn get(&self, connection_id: &str) -> Result<Arc<dyn TicketProvider>, ProviderError> {
        self.providers
            .lock()
            .ok()
            .and_then(|p| p.get(connection_id).cloned())
            .ok_or_else(|| ProviderError::UnknownConnection(connection_id.into()))
    }

    pub fn descriptors(&self) -> Vec<ProviderDescriptor> {
        let mut values = self
            .providers
            .lock()
            .map(|p| p.values().map(|v| v.descriptor()).collect::<Vec<_>>())
            .unwrap_or_default();
        values.sort_by(|a, b| a.connection_id.cmp(&b.connection_id));
        values
    }

    pub fn query_all(&self, query: &TicketQuery) -> Vec<ProviderQueryResult> {
        self.descriptors()
            .into_iter()
            .map(|descriptor| {
                let result = self
                    .get(&descriptor.connection_id)
                    .and_then(|provider| provider.query(query));
                ProviderQueryResult { descriptor, result }
            })
            .collect()
    }

    fn transfer_lock(&self, operation_id: &str) -> Result<Arc<Mutex<()>>, ProviderError> {
        let mut locks = self
            .transfer_locks
            .lock()
            .map_err(|_| ProviderError::Conflict {
                ticket: operation_id.into(),
                message: "transfer lock registry poisoned".into(),
            })?;
        Ok(locks
            .entry(operation_id.into())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone())
    }
}

pub struct ProviderQueryResult {
    pub descriptor: ProviderDescriptor,
    pub result: Result<Vec<ApiTicket>, ProviderError>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TransferOutcome {
    pub operation_id: String,
    pub source: TicketRef,
    pub destination: TicketRef,
    pub moved: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum TransferError {
    #[error(transparent)]
    Provider(#[from] ProviderError),
    #[error("transfer operation id must not be empty")]
    EmptyOperationId,
    #[error("cross-provider dependencies require an explicit remap")]
    DependenciesNeedMapping,
    #[error("destination provider '{connection_id}' cannot represent source field '{field}'")]
    UnsupportedField {
        connection_id: String,
        field: &'static str,
    },
    #[error("destination ticket {destination} was created, but source close failed: {message}")]
    SourceCloseFailed {
        destination: String,
        message: String,
    },
}

fn transfer_ulid(operation_id: &str, suffix: &str) -> Ulid {
    let mut hash = Sha256::new();
    hash.update(operation_id.as_bytes());
    hash.update([0]);
    hash.update(suffix.as_bytes());
    let bytes: [u8; 16] = hash.finalize()[..16].try_into().expect("sha prefix");
    Ulid::from(u128::from_be_bytes(bytes))
}

pub fn copy_between(
    registry: &ProviderRegistry,
    source: TicketRef,
    destination_connection: &str,
    operation_id: &str,
    now: Timestamp,
) -> Result<TransferOutcome, TransferError> {
    if operation_id.trim().is_empty() {
        return Err(TransferError::EmptyOperationId);
    }
    let transfer_lock = registry.transfer_lock(operation_id)?;
    let _guard = transfer_lock.lock().map_err(|_| ProviderError::Conflict {
        ticket: operation_id.into(),
        message: "transfer operation lock poisoned".into(),
    })?;
    let source_provider = registry.get(&source.connection_id)?;
    let destination = registry.get(destination_connection)?;
    let ticket = source_provider.get(&source.native_id)?;
    let capabilities = destination.descriptor().capabilities;
    let attachments = ticket.attachments.clone();
    if !ticket.blocked_by.is_empty() {
        return Err(TransferError::DependenciesNeedMapping);
    }
    for (present, supported, field) in [
        (!ticket.notes.is_empty(), capabilities.notes, "notes"),
        // Checked up front so an attachment-less destination fails before the copy exists
        // rather than after creating it (HS2-HSA64D).
        (
            !attachments.is_empty(),
            capabilities.attachments,
            "attachments",
        ),
        (
            !ticket.assignees.is_empty(),
            capabilities.assignment,
            "assignees",
        ),
        (
            !ticket.review_requests.is_empty(),
            capabilities.review_requests,
            "review_requests",
        ),
    ] {
        if present && !supported {
            return Err(TransferError::UnsupportedField {
                connection_id: destination_connection.into(),
                field,
            });
        }
    }
    if ticket
        .notes
        .iter()
        .any(|note| note.edited_at != note.created_at)
        && !destination.supports_note_edit()
    {
        return Err(TransferError::UnsupportedField {
            connection_id: destination_connection.into(),
            field: "edited notes",
        });
    }
    if ticket.notes.iter().any(|note| note.confidence.is_some())
        && !destination.supports_note_confidence()
    {
        return Err(TransferError::UnsupportedField {
            connection_id: destination_connection.into(),
            field: "note confidence",
        });
    }
    let draft = ProviderDraft {
        title: ticket.title,
        category: ticket.category,
        priority: ticket.priority,
        status: Status::NotStarted,
        details: ticket.details,
        tags: ticket.tags,
        up_next: false,
        blocked_by: vec![],
        transfer: Some(TransferProvenance {
            operation_id: operation_id.into(),
            source: source.clone(),
        }),
    };
    let created = destination.create(
        MutationContext {
            now: now.clone(),
            generated_id: transfer_ulid(operation_id, destination_connection),
        },
        draft,
    )?;
    for note in ticket.notes {
        let generated_id = transfer_ulid(operation_id, &format!("note:{}", note.id));
        destination.add_note_with_metadata(
            &created.native_id,
            MutationContext {
                now: Timestamp::new(note.created_at.clone()),
                generated_id,
            },
            note.kind,
            NoteMetadataInput {
                summary: note.summary.clone(),
                confidence: note
                    .confidence
                    .and_then(|value| Confidence::new(u64::from(value)).ok()),
                actor: note.actor.clone(),
            },
            note.text.clone(),
        )?;
        if note.edited_at != note.created_at {
            destination.edit_note(
                &created.native_id,
                &generated_id.to_string(),
                Timestamp::new(note.edited_at),
                note.text,
            )?;
        }
    }
    for attachment in attachments {
        let bytes = source_provider.attachment_bytes(&source.native_id, &attachment.id)?;
        destination.add_attachment(&created.native_id, attachment, bytes)?;
    }
    if !ticket.assignees.is_empty() || !ticket.review_requests.is_empty() {
        destination.assign(
            &created.native_id,
            now,
            Some(ticket.assignees),
            ticket.review_requests,
        )?;
    }
    Ok(TransferOutcome {
        operation_id: operation_id.into(),
        source,
        destination: TicketRef {
            connection_id: destination_connection.into(),
            native_id: created.native_id,
        },
        moved: false,
    })
}

pub fn move_between(
    registry: &ProviderRegistry,
    source: TicketRef,
    destination_connection: &str,
    operation_id: &str,
    now: Timestamp,
) -> Result<TransferOutcome, TransferError> {
    let mut outcome = copy_between(
        registry,
        source.clone(),
        destination_connection,
        operation_id,
        now.clone(),
    )?;
    registry
        .get(&source.connection_id)?
        .close(&source.native_id, now, CloseReason::Obsolete, None)
        .map_err(|error| TransferError::SourceCloseFailed {
            destination: outcome.destination.qualified(),
            message: error.to_string(),
        })?;
    outcome.moved = true;
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StoreMetadata;

    fn git_provider() -> (tempfile::TempDir, GitProvider) {
        let dir = tempfile::tempdir().unwrap();
        let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
        (dir, GitProvider::new("local", store).with_default(true))
    }

    fn ctx(id: Ulid, at: &str) -> MutationContext {
        MutationContext {
            now: Timestamp::new(at),
            generated_id: id,
        }
    }

    #[test]
    fn project_ticket_refs_round_trip_colon_bearing_native_ids() {
        let reference = ProjectTicketRef {
            project_id: "kerf-a1b2".into(),
            connection_id: "jira-eng".into(),
            native_id: "ENG:42".into(),
        };
        assert_eq!(reference.qualified(), "@kerf-a1b2/jira-eng:ENG:42");
        assert_eq!(
            ProjectTicketRef::from_qualified(&reference.qualified()),
            Some(reference)
        );
        assert!(ProjectTicketRef::from_qualified("01ARZ3NDEKTSV4RRFFQ69G5FC0").is_none());
    }

    /// Seven provider tickets in creation order, for keyset helper tests.
    fn created_rows() -> Vec<ApiTicket> {
        let (_dir, provider) = git_provider();
        for index in 1..=7 {
            provider
                .create(
                    ctx(Ulid::new(), &format!("2026-08-26T00:0{index}:00Z")),
                    ProviderDraft {
                        title: format!("ticket {index}"),
                        category: "task".into(),
                        priority: Priority::Default,
                        status: Status::NotStarted,
                        details: String::new(),
                        tags: vec![],
                        up_next: false,
                        blocked_by: vec![],
                        transfer: None,
                    },
                )
                .unwrap();
        }
        provider
            .query(&TicketQuery {
                sort: crate::SortKey::Created,
                ..Default::default()
            })
            .unwrap()
    }

    fn titles(page: &ProviderKeysetPage) -> Vec<String> {
        page.items
            .iter()
            .map(|item| item.ticket.title.clone())
            .collect()
    }

    /// Serve `rows` as native pages of two, with `page:N` cursors, counting fetches.
    fn native_pages(
        rows: &[ApiTicket],
        fetches: &std::cell::Cell<usize>,
        cursor: Option<&str>,
    ) -> Result<(Vec<ApiTicket>, Option<String>), ProviderError> {
        fetches.set(fetches.get() + 1);
        let page = cursor.map_or(0, |cursor| {
            cursor["page:".len()..].parse::<usize>().unwrap()
        });
        let start = (page * 2).min(rows.len());
        let end = (start + 2).min(rows.len());
        Ok((
            rows[start..end].to_vec(),
            (end < rows.len()).then(|| format!("page:{}", page + 1)),
        ))
    }

    #[test]
    fn full_scan_keyset_pages_resume_strictly_after_a_value() {
        let rows = created_rows();
        let query = TicketQuery {
            sort: crate::SortKey::Created,
            ..Default::default()
        };
        let first = keyset_page_from_rows(rows.clone(), &query, None, 3);
        assert_eq!(titles(&first), ["ticket 1", "ticket 2", "ticket 3"]);
        assert!(!first.exhausted);
        let last = MergeKey::from_ticket(&first.items[2].ticket);
        // The boundary row is gone and an earlier row was deleted: the rest still follows.
        let shifted = rows
            .iter()
            .filter(|row| row.title != "ticket 3" && row.title != "ticket 1")
            .cloned()
            .collect::<Vec<_>>();
        let rest = keyset_page_from_rows(shifted, &query, Some(&last), 10);
        assert_eq!(
            titles(&rest),
            ["ticket 4", "ticket 5", "ticket 6", "ticket 7"]
        );
        assert!(rest.exhausted);
        let exact = keyset_page_from_rows(rows, &query, Some(&last), 4);
        assert!(
            exact.exhausted,
            "exactly `limit` remaining rows exhausts the source"
        );
    }

    #[test]
    fn native_keyset_pages_use_a_valid_hint_and_restart_from_a_stale_one() {
        let rows = created_rows();
        let query = TicketQuery {
            sort: crate::SortKey::Created,
            ..Default::default()
        };
        let fetches = std::cell::Cell::new(0);
        let first = keyset_page_from_native_pages(&query, None, None, 3, |cursor| {
            native_pages(&rows, &fetches, cursor)
        })
        .unwrap();
        assert_eq!(titles(&first), ["ticket 1", "ticket 2", "ticket 3"]);
        assert!(!first.exhausted);
        assert_eq!(first.items[2].resume.as_deref(), Some("page:1"));
        let last = MergeKey::from_ticket(&first.items[2].ticket);

        // A valid hint skips earlier pages entirely.
        fetches.set(0);
        let resumed =
            keyset_page_from_native_pages(&query, Some(&last), Some("page:1"), 10, |cursor| {
                native_pages(&rows, &fetches, cursor)
            })
            .unwrap();
        assert_eq!(
            titles(&resumed),
            ["ticket 4", "ticket 5", "ticket 6", "ticket 7"]
        );
        assert!(resumed.exhausted);
        assert_eq!(fetches.get(), 3, "pages 1..=3, never page 0");

        // Two earlier deletions move the successor before the hinted page: restart.
        let shifted = rows[2..].to_vec();
        fetches.set(0);
        let restarted =
            keyset_page_from_native_pages(&query, Some(&last), Some("page:1"), 10, |cursor| {
                native_pages(&shifted, &fetches, cursor)
            })
            .unwrap();
        assert_eq!(
            titles(&restarted),
            ["ticket 4", "ticket 5", "ticket 6", "ticket 7"]
        );
        assert_eq!(fetches.get(), 4, "stale page, then pages 0..=2");

        // A hinted page that no longer exists also restarts instead of ending the source.
        let short = rows[..4].to_vec();
        let vanished =
            keyset_page_from_native_pages(&query, Some(&last), Some("page:5"), 10, |cursor| {
                native_pages(&short, &fetches, cursor)
            })
            .unwrap();
        assert_eq!(titles(&vanished), ["ticket 4"]);
        assert!(vanished.exhausted);

        // A provider whose cursor does not advance is an error, not an endless loop.
        let repeating = keyset_page_from_native_pages(&query, None, None, 10, |_| {
            Ok((vec![], Some("page:0".into())))
        });
        assert!(matches!(repeating, Err(ProviderError::Conflict { .. })));
        let stuck = keyset_page_from_native_pages(&query, None, Some("page:0"), 10, |_| {
            Ok((rows[..1].to_vec(), Some("page:0".into())))
        });
        assert!(matches!(stuck, Err(ProviderError::Conflict { .. })));
    }

    #[test]
    fn default_provider_pages_and_summaries_preserve_the_legacy_contract() {
        let (_dir, provider) = git_provider();
        for (index, status, up_next) in [
            (1, Status::NotStarted, true),
            (2, Status::Started, false),
            (3, Status::Backlog, false),
        ] {
            provider
                .create(
                    ctx(Ulid::new(), &format!("2026-08-26T00:0{index}:00Z")),
                    ProviderDraft {
                        title: format!("ticket {index}"),
                        category: "task".into(),
                        priority: Priority::Default,
                        status,
                        details: String::new(),
                        tags: vec![],
                        up_next,
                        blocked_by: vec![],
                        transfer: None,
                    },
                )
                .unwrap();
        }
        let first = provider
            .query_page(&TicketQuery::default(), None, 2)
            .unwrap();
        assert_eq!(first.items.len(), 2);
        assert_eq!(first.next_cursor.as_deref(), Some("2"));
        let second = provider
            .query_page(&TicketQuery::default(), first.next_cursor.as_deref(), 2)
            .unwrap();
        assert_eq!(second.items.len(), 1);
        assert!(second.next_cursor.is_none());
        let summary = provider.summary("2026-08-27T00:00:00Z", &[]).unwrap();
        assert_eq!(
            (
                summary.total,
                summary.queued,
                summary.backlog,
                summary.open,
                summary.started,
                summary.verified,
                summary.up_next
            ),
            (3, 2, 1, 2, 1, 0, 1)
        );
    }

    #[test]
    fn provider_categorical_pages_keep_recent_first_ties() {
        let (_dir, provider) = git_provider();
        for (title, at) in [
            ("same title", "2026-08-26T00:00:00Z"),
            ("Same Title", "2026-08-27T00:00:00Z"),
        ] {
            provider
                .create(
                    ctx(Ulid::new(), at),
                    ProviderDraft {
                        title: title.into(),
                        category: "task".into(),
                        priority: Priority::High,
                        status: Status::Started,
                        details: String::new(),
                        tags: vec![],
                        up_next: false,
                        blocked_by: vec![],
                        transfer: None,
                    },
                )
                .unwrap();
        }
        let mut query = TicketQuery {
            sort: crate::SortKey::Title,
            ..TicketQuery::default()
        };
        let rows =
            filter_provider_ticket_page(provider.query(&TicketQuery::default()).unwrap(), &query);
        assert_eq!(rows[0].updated_at, "2026-08-27T00:00:00Z");
        query.sort = crate::SortKey::Priority;
        query.descending = true;
        let rows =
            filter_provider_ticket_page(provider.query(&TicketQuery::default()).unwrap(), &query);
        assert_eq!(rows[0].updated_at, "2026-08-27T00:00:00Z");
    }

    #[test]
    fn git_provider_conforms_to_create_get_query_update_note_close() {
        let (_dir, provider) = git_provider();
        let id = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
        let created = provider
            .create(
                ctx(id, "2026-08-26T00:00:00Z"),
                ProviderDraft {
                    title: "provider ticket".into(),
                    category: "task".into(),
                    priority: Priority::High,
                    status: Status::NotStarted,
                    details: "details".into(),
                    tags: vec!["provider".into()],
                    up_next: true,
                    blocked_by: vec![],
                    transfer: None,
                },
            )
            .unwrap();
        assert_eq!(created.connection_id, "local");
        assert_eq!(created.qualified_id, format!("local:{id}"));
        assert_eq!(
            provider.get(&id.to_string()).unwrap().title,
            "provider ticket"
        );
        assert_eq!(provider.query(&TicketQuery::default()).unwrap().len(), 1);

        provider
            .update(
                &id.to_string(),
                Timestamp::new("2026-08-26T00:01:00Z"),
                ProviderPatch {
                    status: Some(Status::Started),
                    ..Default::default()
                },
            )
            .unwrap();
        provider
            .add_note(
                &id.to_string(),
                ctx(Ulid::new(), "2026-08-26T00:02:00Z"),
                NoteKind::Regular,
                "worked".into(),
            )
            .unwrap();
        let closed = provider
            .close(
                &id.to_string(),
                Timestamp::new("2026-08-26T00:03:00Z"),
                CloseReason::Completed,
                None,
            )
            .unwrap();
        assert_eq!(closed.close_reason, Some(CloseReason::Completed));
        assert_eq!(closed.notes.len(), 3);
        assert_eq!(
            closed.notes[0].text,
            "Status changed from Not Started to Started"
        );
        assert_eq!(closed.notes[1].text, "worked");
        assert_eq!(
            closed.notes[2].text,
            "Status changed from Started to Completed"
        );

        let claim_id = Ulid::new();
        provider
            .create(
                ctx(claim_id, "2026-08-26T00:04:00Z"),
                ProviderDraft {
                    title: "claimable".into(),
                    category: "task".into(),
                    priority: Priority::Default,
                    status: Status::NotStarted,
                    details: String::new(),
                    tags: vec![],
                    up_next: true,
                    blocked_by: vec![],
                    transfer: None,
                },
            )
            .unwrap();
        let claimed = provider
            .claim_next(
                Timestamp::new("2026-08-26T00:05:00Z"),
                Timestamp::new("2026-08-26T00:35:00Z"),
                "worker-a",
                None,
            )
            .unwrap()
            .unwrap();
        assert_eq!(claimed.native_id, claim_id.to_string());
        provider
            .renew(
                &claimed.native_id,
                Timestamp::new("2026-08-26T00:06:00Z"),
                Timestamp::new("2026-08-26T00:36:00Z"),
                "worker-a",
            )
            .unwrap();
        assert!(
            provider
                .release(
                    &claimed.native_id,
                    Timestamp::new("2026-08-26T00:07:00Z"),
                    "worker-a",
                    false,
                )
                .unwrap()
                .claimed_by
                .is_none()
        );
    }

    #[test]
    fn git_provider_preserves_initial_backlog_status() {
        let (_dir, provider) = git_provider();
        let id = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
        let created = provider
            .create(
                ctx(id, "2026-08-26T00:00:00Z"),
                ProviderDraft {
                    title: "deferred provider ticket".into(),
                    category: "task".into(),
                    priority: Priority::Default,
                    status: Status::Backlog,
                    details: String::new(),
                    tags: vec![],
                    up_next: true,
                    blocked_by: vec![],
                    transfer: None,
                },
            )
            .unwrap();
        assert_eq!(created.status, Status::Backlog);
        assert!(!created.up_next);
        assert_eq!(
            provider.get(&id.to_string()).unwrap().status,
            Status::Backlog
        );
    }

    #[test]
    fn registry_aggregates_success_and_partial_provider_failure() {
        struct Failing;
        impl TicketProvider for Failing {
            fn descriptor(&self) -> ProviderDescriptor {
                ProviderDescriptor {
                    connection_id: "down".into(),
                    provider: "fake".into(),
                    display_name: "Down".into(),
                    locator: "test".into(),
                    default: false,
                    capabilities: ProviderCapabilities::git(),
                }
            }
            fn query(&self, _: &TicketQuery) -> Result<Vec<ApiTicket>, ProviderError> {
                Err(ProviderError::Authentication {
                    connection_id: "down".into(),
                    message: "denied".into(),
                })
            }
            fn find_transfer(&self, _: &str) -> Result<Option<ApiTicket>, ProviderError> {
                unreachable!()
            }
            fn get(&self, _: &str) -> Result<ApiTicket, ProviderError> {
                unreachable!()
            }
            fn create(
                &self,
                _: MutationContext,
                _: ProviderDraft,
            ) -> Result<ApiTicket, ProviderError> {
                unreachable!()
            }
            fn update(
                &self,
                _: &str,
                _: Timestamp,
                _: ProviderPatch,
            ) -> Result<ApiTicket, ProviderError> {
                unreachable!()
            }
            fn add_note(
                &self,
                _: &str,
                _: MutationContext,
                _: NoteKind,
                _: String,
            ) -> Result<ApiTicket, ProviderError> {
                unreachable!()
            }
            fn close(
                &self,
                _: &str,
                _: Timestamp,
                _: CloseReason,
                _: Option<String>,
            ) -> Result<ApiTicket, ProviderError> {
                unreachable!()
            }
            fn assign(
                &self,
                _: &str,
                _: Timestamp,
                _: Option<Vec<String>>,
                _: Vec<ReviewRequest>,
            ) -> Result<ApiTicket, ProviderError> {
                unreachable!()
            }
            fn claim_next(
                &self,
                _: Timestamp,
                _: Timestamp,
                _: &str,
                _: Option<String>,
            ) -> Result<Option<ApiTicket>, ProviderError> {
                unreachable!()
            }
            fn release(
                &self,
                _: &str,
                _: Timestamp,
                _: &str,
                _: bool,
            ) -> Result<ApiTicket, ProviderError> {
                unreachable!()
            }
            fn renew(
                &self,
                _: &str,
                _: Timestamp,
                _: Timestamp,
                _: &str,
            ) -> Result<ApiTicket, ProviderError> {
                unreachable!()
            }
        }

        let (_dir, git) = git_provider();
        let registry = ProviderRegistry::default();
        registry.register(Arc::new(git)).unwrap();
        registry.register(Arc::new(Failing)).unwrap();
        let results = registry.query_all(&TicketQuery::default());
        assert_eq!(results.len(), 2);
        assert!(
            results
                .iter()
                .any(|r| r.descriptor.connection_id == "local" && r.result.is_ok())
        );
        assert!(
            results
                .iter()
                .any(|r| r.descriptor.connection_id == "down" && r.result.is_err())
        );
    }

    #[test]
    fn generated_connection_ids_are_readable_valid_and_unique() {
        let connection = |id: &str| ProviderConnection {
            id: id.into(),
            provider: "github".into(),
            locator: "x/y".into(),
            name: None,
            default: false,
            settings: serde_json::Value::Null,
            disabled: false,
        };
        assert_eq!(
            generate_connection_id(&[], "github", "Small-Tale/hotsheet2"),
            "github-small-tale-hotsheet2"
        );
        let existing = [
            connection("github-small-tale-hotsheet2"),
            connection("github-small-tale-hotsheet2-2"),
        ];
        assert_eq!(
            generate_connection_id(&existing, "github", "small-tale/hotsheet2"),
            "github-small-tale-hotsheet2-3"
        );
        assert_eq!(generate_connection_id(&[], "jira", "OPS"), "jira-ops");
        assert_eq!(generate_connection_id(&[], "", "///"), "connection");
        let long = generate_connection_id(&[], "gitlab", &"group/".repeat(30));
        assert!(long.len() <= 48 && !long.ends_with('-'));
        // Every generated id passes the registry's own validation.
        for id in [long, generate_connection_id(&existing, "github", "a b/c.d")] {
            validate_connections(&[connection(&id)]).unwrap();
        }
    }

    #[test]
    fn config_registry_toggles_disabled_idempotently_and_omits_the_default_flag() {
        // HS2-SF6W34: disabled is off by default, round-trips, and never rewrites other fields.
        let dir = tempfile::tempdir().unwrap();
        let registry = ProviderConfigRegistry::new(dir.path().join("providers.json"));
        registry
            .save(&[ProviderConnection {
                id: "github-main".into(),
                provider: "github".into(),
                locator: "acme/repo".into(),
                name: Some("Issues".into()),
                default: true,
                settings: serde_json::json!({"credential": {"secret": "github-app-1"}}),
                disabled: false,
            }])
            .unwrap();
        let text = || std::fs::read_to_string(registry.path()).unwrap();
        assert!(
            !text().contains("disabled"),
            "an enabled connection omits the flag"
        );
        assert!(!registry.disabled("github-main").unwrap());
        for _ in 0..2 {
            let off = registry.set_disabled("github-main", true).unwrap().unwrap();
            assert!(off.disabled && off.default && off.locator == "acme/repo");
            assert!(registry.disabled("github-main").unwrap());
        }
        assert!(text().contains("\"disabled\": true"));
        let on = registry
            .set_disabled("github-main", false)
            .unwrap()
            .unwrap();
        assert!(!on.disabled);
        assert!(!text().contains("disabled"));
        assert!(registry.set_disabled("missing", true).unwrap().is_none());
        assert!(!registry.disabled("missing").unwrap());
    }

    #[test]
    fn config_registry_round_trips_non_secret_connections_and_rejects_ambiguity() {
        let dir = tempfile::tempdir().unwrap();
        let registry = ProviderConfigRegistry::new(dir.path().join("providers.json"));
        let connections = vec![ProviderConnection {
            id: "github-main".into(),
            provider: "github".into(),
            locator: "small-tale/hotsheet2".into(),
            name: Some("Public issues".into()),
            default: true,
            settings: serde_json::json!({"credential":{"secret":"github-small-tale"}}),
            disabled: false,
        }];
        registry.save(&connections).unwrap();
        assert_eq!(registry.load().unwrap(), connections);
        let text = std::fs::read_to_string(registry.path()).unwrap();
        assert!(!text.contains("token"));

        let mut duplicate_default = connections.clone();
        duplicate_default.push(ProviderConnection {
            id: "jira".into(),
            provider: "jira".into(),
            locator: "example.atlassian.net/ENG".into(),
            name: None,
            default: true,
            settings: serde_json::Value::Null,
            disabled: false,
        });
        assert!(registry.save(&duplicate_default).is_err());
    }

    #[test]
    fn concurrent_transfer_retries_create_one_destination_and_move_closes_source() {
        let (_source_dir, source) = git_provider();
        let destination_dir = tempfile::tempdir().unwrap();
        let destination_store =
            FsStore::init(destination_dir.path(), &StoreMetadata::new("DST")).unwrap();
        let destination = GitProvider::new("destination", destination_store.clone());
        let source_id = Ulid::new();
        source
            .create(
                ctx(source_id, "2026-08-26T01:00:00Z"),
                ProviderDraft {
                    title: "transfer me".into(),
                    category: "task".into(),
                    priority: Priority::Default,
                    status: Status::Started,
                    details: "body".into(),
                    tags: vec!["cross-provider".into()],
                    up_next: true,
                    blocked_by: vec![],
                    transfer: None,
                },
            )
            .unwrap();
        let source_note_id = Ulid::new();
        let source_attachment_id = Ulid::new();
        source
            .store
            .write_attachment(
                &source_id,
                source_attachment_id,
                Timestamp::new("2026-08-26T01:00:05Z"),
                "provider-proof.txt",
                b"provider proof",
            )
            .unwrap();
        source
            .add_note(
                &source_id.to_string(),
                ctx(source_note_id, "2026-08-26T01:00:10Z"),
                NoteKind::Activity,
                "preserve this note".into(),
            )
            .unwrap();
        source
            .edit_note(
                &source_id.to_string(),
                &source_note_id.to_string(),
                Timestamp::new("2026-08-26T01:00:15Z"),
                "preserve this edited note".into(),
            )
            .unwrap();
        source
            .assign(
                &source_id.to_string(),
                Timestamp::new("2026-08-26T01:00:20Z"),
                Some(vec!["dev@example.com".into()]),
                vec![],
            )
            .unwrap();
        let registry = ProviderRegistry::default();
        registry.register(Arc::new(source.clone())).unwrap();
        registry.register(Arc::new(destination)).unwrap();
        let source_ref = TicketRef {
            connection_id: "local".into(),
            native_id: source_id.to_string(),
        };
        let (a, b) = std::thread::scope(|scope| {
            let first = scope.spawn(|| {
                copy_between(
                    &registry,
                    source_ref.clone(),
                    "destination",
                    "operation-1",
                    Timestamp::new("2026-08-26T01:01:00Z"),
                )
                .unwrap()
            });
            let second = scope.spawn(|| {
                copy_between(
                    &registry,
                    source_ref.clone(),
                    "destination",
                    "operation-1",
                    Timestamp::new("2026-08-26T01:02:00Z"),
                )
                .unwrap()
            });
            (first.join().unwrap(), second.join().unwrap())
        });
        assert_eq!(a.destination, b.destination);
        assert_eq!(destination_store.list_tickets().unwrap().len(), 1);
        let copied = registry
            .get("destination")
            .unwrap()
            .get(&a.destination.native_id)
            .unwrap();
        assert_eq!(copied.notes.len(), 1);
        assert_eq!(copied.attachments.len(), 1);
        assert_eq!(copied.attachments[0].id, source_attachment_id.to_string());
        assert_eq!(copied.attachments[0].created_at, "2026-08-26T01:00:05Z");
        assert_eq!(copied.notes[0].kind, NoteKind::Activity);
        assert_eq!(copied.notes[0].text, "preserve this edited note");
        assert_eq!(copied.notes[0].created_at, "2026-08-26T01:00:10Z");
        assert_eq!(copied.notes[0].edited_at, "2026-08-26T01:00:15Z");
        assert_eq!(copied.assignees, ["dev@example.com"]);
        assert_eq!(copied.status, Status::NotStarted);
        assert!(!copied.up_next);
        let moved = move_between(
            &registry,
            source_ref.clone(),
            "destination",
            "operation-1",
            Timestamp::new("2026-08-26T01:03:00Z"),
        )
        .unwrap();
        assert!(moved.moved);
        assert_eq!(
            source.get(&source_id.to_string()).unwrap().close_reason,
            Some(CloseReason::Obsolete)
        );
    }

    #[test]
    fn move_reports_created_destination_and_retry_recovers_after_source_close_failure() {
        let (_source_dir, source) = git_provider();
        let source = source.with_close_failure_once();
        let destination_dir = tempfile::tempdir().unwrap();
        let destination_store =
            FsStore::init(destination_dir.path(), &StoreMetadata::new("DST")).unwrap();
        let source_id = Ulid::new();
        source
            .create(
                ctx(source_id, "2026-08-26T02:00:00Z"),
                ProviderDraft {
                    title: "recoverable move".into(),
                    category: "task".into(),
                    priority: Priority::Default,
                    status: Status::NotStarted,
                    details: String::new(),
                    tags: vec![],
                    up_next: false,
                    blocked_by: vec![],
                    transfer: None,
                },
            )
            .unwrap();
        let registry = ProviderRegistry::default();
        registry.register(Arc::new(source.clone())).unwrap();
        registry
            .register(Arc::new(GitProvider::new(
                "destination",
                destination_store.clone(),
            )))
            .unwrap();
        let source_ref = TicketRef {
            connection_id: "local".into(),
            native_id: source_id.to_string(),
        };
        let error = move_between(
            &registry,
            source_ref.clone(),
            "destination",
            "recover-op",
            Timestamp::new("2026-08-26T02:01:00Z"),
        )
        .unwrap_err();
        let TransferError::SourceCloseFailed { destination, .. } = error else {
            panic!("expected partial failure")
        };
        assert!(destination.starts_with("destination:"));
        assert_eq!(destination_store.list_tickets().unwrap().len(), 1);

        let recovered = move_between(
            &registry,
            source_ref,
            "destination",
            "recover-op",
            Timestamp::new("2026-08-26T02:02:00Z"),
        )
        .unwrap();
        assert_eq!(recovered.destination.qualified(), destination);
        assert_eq!(destination_store.list_tickets().unwrap().len(), 1);
        assert_eq!(
            source.get(&source_id.to_string()).unwrap().close_reason,
            Some(CloseReason::Obsolete)
        );
    }

    #[test]
    fn transfer_rejects_source_fields_the_destination_cannot_represent() {
        let (_source_dir, source) = git_provider();
        let destination_dir = tempfile::tempdir().unwrap();
        let destination_store =
            FsStore::init(destination_dir.path(), &StoreMetadata::new("DST")).unwrap();
        let mut capabilities = ProviderCapabilities::git();
        capabilities.notes = false;
        let destination = GitProvider::new("destination", destination_store.clone())
            .with_test_capabilities(capabilities);
        let source_id = Ulid::new();
        source
            .create(
                ctx(source_id, "2026-08-26T03:00:00Z"),
                ProviderDraft {
                    title: "has unsupported note".into(),
                    category: "task".into(),
                    priority: Priority::Default,
                    status: Status::NotStarted,
                    details: String::new(),
                    tags: vec![],
                    up_next: false,
                    blocked_by: vec![],
                    transfer: None,
                },
            )
            .unwrap();
        source
            .add_note(
                &source_id.to_string(),
                ctx(Ulid::new(), "2026-08-26T03:01:00Z"),
                NoteKind::Regular,
                "must not disappear".into(),
            )
            .unwrap();
        let registry = ProviderRegistry::default();
        registry.register(Arc::new(source)).unwrap();
        registry.register(Arc::new(destination)).unwrap();
        let error = copy_between(
            &registry,
            TicketRef {
                connection_id: "local".into(),
                native_id: source_id.to_string(),
            },
            "destination",
            "unsupported-op",
            Timestamp::new("2026-08-26T03:02:00Z"),
        )
        .unwrap_err();
        assert!(matches!(
            error,
            TransferError::UnsupportedField { field: "notes", .. }
        ));
        assert!(destination_store.list_tickets().unwrap().is_empty());
    }

    /// HS2-HSA64D: attachments are checked with the other fields, so a destination
    /// without attachment support refuses the copy before creating anything.
    #[test]
    fn transfer_rejects_attachments_before_creating_the_destination_ticket() {
        let (_source_dir, source) = git_provider();
        let destination_dir = tempfile::tempdir().unwrap();
        let destination_store =
            FsStore::init(destination_dir.path(), &StoreMetadata::new("DST")).unwrap();
        let mut capabilities = ProviderCapabilities::git();
        capabilities.attachments = false;
        capabilities.attachment_edit = false;
        let destination = GitProvider::new("destination", destination_store.clone())
            .with_test_capabilities(capabilities);
        let source_id = Ulid::new();
        source
            .create(
                ctx(source_id, "2026-08-26T03:00:00Z"),
                ProviderDraft {
                    title: "has evidence".into(),
                    category: "task".into(),
                    priority: Priority::Default,
                    status: Status::NotStarted,
                    details: String::new(),
                    tags: vec![],
                    up_next: false,
                    blocked_by: vec![],
                    transfer: None,
                },
            )
            .unwrap();
        source
            .add_attachment(
                &source_id.to_string(),
                ApiAttachment {
                    id: Ulid::new().to_string(),
                    filename: "proof.txt".into(),
                    created_at: "2026-08-26T03:01:00Z".into(),
                    batch_id: None,
                    batch_label: None,
                    actor: None,
                    purpose: None,
                    annotations: vec![],
                },
                b"evidence".to_vec(),
            )
            .unwrap();
        let registry = ProviderRegistry::default();
        registry.register(Arc::new(source)).unwrap();
        registry.register(Arc::new(destination)).unwrap();
        let error = copy_between(
            &registry,
            TicketRef {
                connection_id: "local".into(),
                native_id: source_id.to_string(),
            },
            "destination",
            "attachment-op",
            Timestamp::new("2026-08-26T03:02:00Z"),
        )
        .unwrap_err();
        assert!(matches!(
            error,
            TransferError::UnsupportedField {
                field: "attachments",
                ..
            }
        ));
        assert!(destination_store.list_tickets().unwrap().is_empty());
    }

    /// HS2-DWTJ43: a confidence score is preserved by a capable provider (including
    /// across a transfer) and fails explicitly, never silently, everywhere else.
    #[test]
    fn note_confidence_is_preserved_or_rejected_explicitly() {
        let (_source_dir, source) = git_provider();
        let source_id = Ulid::new();
        let draft = || ProviderDraft {
            title: "scored".into(),
            category: "task".into(),
            priority: Priority::Default,
            status: Status::NotStarted,
            details: String::new(),
            tags: vec![],
            up_next: false,
            blocked_by: vec![],
            transfer: None,
        };
        source
            .create(ctx(source_id, "2026-08-26T03:00:00Z"), draft())
            .unwrap();
        assert!(source.supports_note_confidence());
        let scored = source
            .add_note_with_metadata(
                &source_id.to_string(),
                ctx(Ulid::new(), "2026-08-26T03:01:00Z"),
                NoteKind::Regular,
                NoteMetadataInput {
                    summary: None,
                    confidence: Some(Confidence::new(73).unwrap()),
                    actor: None,
                },
                "## Confidence\n73".into(),
            )
            .unwrap();
        assert_eq!(scored.notes.last().unwrap().confidence, Some(73));

        // A provider without the capability rejects the score before writing anything.
        let (_plain_dir, plain) = git_provider();
        let mut capabilities = ProviderCapabilities::git();
        capabilities.note_confidence = false;
        let plain = plain.with_test_capabilities(capabilities.clone());
        let plain_id = Ulid::new();
        plain
            .create(ctx(plain_id, "2026-08-26T03:00:00Z"), draft())
            .unwrap();
        let error = plain
            .add_note_with_metadata(
                &plain_id.to_string(),
                ctx(Ulid::new(), "2026-08-26T03:01:00Z"),
                NoteKind::Regular,
                NoteMetadataInput {
                    summary: None,
                    confidence: Some(Confidence::new(10).unwrap()),
                    actor: None,
                },
                "scored".into(),
            )
            .unwrap_err();
        assert!(matches!(
            error,
            ProviderError::Unsupported {
                capability: "note_confidence",
                ..
            }
        ));
        assert!(plain.get(&plain_id.to_string()).unwrap().notes.is_empty());
        // ...while an unscored note still works there.
        plain
            .add_note_with_metadata(
                &plain_id.to_string(),
                ctx(Ulid::new(), "2026-08-26T03:02:00Z"),
                NoteKind::Regular,
                NoteMetadataInput::default(),
                "unscored".into(),
            )
            .unwrap();

        // HS2-CY4CWC: editing a score follows the same capability gate, and a capable
        // provider sets, keeps across a text edit, and clears it.
        let unscored_id = plain.get(&plain_id.to_string()).unwrap().notes[0]
            .id
            .clone();
        let edit_error = plain
            .edit_note_with_metadata(
                &plain_id.to_string(),
                &unscored_id,
                Timestamp::new("2026-08-26T03:03:00Z"),
                crate::ops::NoteEditInput {
                    text: None,
                    confidence: Some(Some(Confidence::new(10).unwrap())),
                },
            )
            .unwrap_err();
        assert!(matches!(
            edit_error,
            ProviderError::Unsupported {
                capability: "note_confidence",
                ..
            }
        ));
        let scored_note = scored.notes.last().unwrap().id.clone();
        let edit = |confidence, text: Option<&str>| {
            source
                .edit_note_with_metadata(
                    &source_id.to_string(),
                    &scored_note,
                    Timestamp::new("2026-08-26T03:04:00Z"),
                    crate::ops::NoteEditInput {
                        text: text.map(str::to_owned),
                        confidence,
                    },
                )
                .unwrap()
                .notes
                .last()
                .unwrap()
                .clone()
        };
        let corrected = edit(Some(Some(Confidence::new(55).unwrap())), None);
        assert_eq!(corrected.confidence, Some(55));
        assert_eq!(corrected.text, "## Confidence\n73");
        let reworded = edit(None, Some("## Confidence\n55"));
        assert_eq!(reworded.confidence, Some(55));
        assert_eq!(reworded.text, "## Confidence\n55");
        assert_eq!(edit(Some(None), None).confidence, None);
        assert_eq!(
            edit(Some(Some(Confidence::new(73).unwrap())), None).confidence,
            Some(73)
        );

        // Transfers carry the score to a capable destination and refuse an incapable one.
        let capable_dir = tempfile::tempdir().unwrap();
        let capable_store = FsStore::init(capable_dir.path(), &StoreMetadata::new("CAP")).unwrap();
        let incapable_dir = tempfile::tempdir().unwrap();
        let incapable_store =
            FsStore::init(incapable_dir.path(), &StoreMetadata::new("INC")).unwrap();
        let registry = ProviderRegistry::default();
        registry.register(Arc::new(source)).unwrap();
        registry
            .register(Arc::new(GitProvider::new("capable", capable_store)))
            .unwrap();
        registry
            .register(Arc::new(
                GitProvider::new("incapable", incapable_store.clone())
                    .with_test_capabilities(capabilities),
            ))
            .unwrap();
        let source_ref = TicketRef {
            connection_id: "local".into(),
            native_id: source_id.to_string(),
        };
        let error = copy_between(
            &registry,
            source_ref.clone(),
            "incapable",
            "confidence-refused",
            Timestamp::new("2026-08-26T03:03:00Z"),
        )
        .unwrap_err();
        assert!(matches!(
            error,
            TransferError::UnsupportedField {
                field: "note confidence",
                ..
            }
        ));
        assert!(incapable_store.list_tickets().unwrap().is_empty());
        let copied = copy_between(
            &registry,
            source_ref,
            "capable",
            "confidence-copied",
            Timestamp::new("2026-08-26T03:04:00Z"),
        )
        .unwrap();
        assert_eq!(
            registry
                .get("capable")
                .unwrap()
                .get(&copied.destination.native_id)
                .unwrap()
                .notes
                .iter()
                .find_map(|note| note.confidence),
            Some(73)
        );
    }

    #[test]
    fn git_provider_reports_not_working_with_note_evidence_and_reopen_in_one_write() {
        let (_dir, provider) = git_provider();
        crate::git::command()
            .arg("-C")
            .arg(provider.store.root())
            .arg("init")
            .output()
            .unwrap();
        crate::git::command()
            .arg("-C")
            .arg(provider.store.root())
            .args(["config", "user.name", "Hot Sheet"])
            .output()
            .unwrap();
        let id = Ulid::new();
        provider
            .create(
                ctx(id, "2026-08-26T04:00:00Z"),
                ProviderDraft {
                    title: "completed work".into(),
                    category: "bug".into(),
                    priority: Priority::Default,
                    status: Status::NotStarted,
                    details: String::new(),
                    tags: vec![],
                    up_next: false,
                    blocked_by: vec![],
                    transfer: None,
                },
            )
            .unwrap();
        provider
            .update(
                &id.to_string(),
                Timestamp::new("2026-08-26T04:01:00Z"),
                ProviderPatch {
                    status: Some(Status::Completed),
                    ..ProviderPatch::default()
                },
            )
            .unwrap();
        let evidence_id = Ulid::new();
        let duplicate_evidence_id = Ulid::new();
        let result = provider
            .report_not_working(
                &id.to_string(),
                Timestamp::new("2026-08-26T04:02:00Z"),
                NotWorkingReport {
                    expected_token: Some("2026-08-26T04:01:00Z".into()),
                    note: Some((Ulid::new(), "regressed after restart".into())),
                    evidence: vec![
                        ProviderEvidence {
                            id: evidence_id,
                            filename: "../proof.txt".into(),
                            created_at: Timestamp::new("2026-08-26T04:02:00Z"),
                            bytes: b"proof".to_vec(),
                        },
                        ProviderEvidence {
                            id: duplicate_evidence_id,
                            filename: "proof.txt".into(),
                            created_at: Timestamp::new("2026-08-26T04:02:01Z"),
                            bytes: b"second proof".to_vec(),
                        },
                    ],
                },
            )
            .unwrap();
        assert_eq!(result.status, Status::NotStarted);
        assert!(result.up_next);
        let added = &result.notes[result.notes.len() - 2..];
        assert_eq!(added[0].kind, NoteKind::Activity);
        assert_eq!(
            added[0].text,
            "Hot Sheet reported as not working\nregressed after restart"
        );
        assert_eq!(added[0].summary.as_deref(), Some("Reported as not working"));
        assert_eq!(added[1].kind, NoteKind::Regular);
        assert_eq!(added[1].text, "Not working: regressed after restart");
        assert!(
            !result
                .notes
                .iter()
                .any(|note| note.text == "Status changed from Completed to Not Started")
        );
        assert_eq!(result.attachments[0].filename, "proof.txt");
        assert_eq!(result.attachments[1].filename, "proof (2).txt");
        assert_eq!(
            provider
                .attachment_bytes(&id.to_string(), &evidence_id.to_string())
                .unwrap(),
            b"proof"
        );
        assert_eq!(
            provider
                .attachment_bytes(&id.to_string(), &duplicate_evidence_id.to_string())
                .unwrap(),
            b"second proof"
        );
    }

    #[test]
    fn not_working_validation_failure_leaves_active_ticket_and_files_unchanged() {
        let (_dir, provider) = git_provider();
        let id = Ulid::new();
        provider
            .create(
                ctx(id, "2026-08-26T05:00:00Z"),
                ProviderDraft {
                    title: "still in progress".into(),
                    category: "bug".into(),
                    priority: Priority::Default,
                    status: Status::NotStarted,
                    details: String::new(),
                    tags: vec![],
                    up_next: false,
                    blocked_by: vec![],
                    transfer: None,
                },
            )
            .unwrap();
        let error = provider
            .report_not_working(
                &id.to_string(),
                Timestamp::new("2026-08-26T05:01:00Z"),
                NotWorkingReport {
                    expected_token: None,
                    note: Some((Ulid::new(), "not done".into())),
                    evidence: vec![],
                },
            )
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("only be reported for a completed, verified, or archived ticket")
        );
        let unchanged = provider.get(&id.to_string()).unwrap();
        assert_eq!(unchanged.status, Status::NotStarted);
        assert!(unchanged.notes.is_empty());
        assert!(unchanged.attachments.is_empty());
    }

    #[test]
    fn not_working_publish_failure_leaves_completed_ticket_and_prior_files_unchanged() {
        let (_dir, provider) = git_provider();
        let id = Ulid::new();
        provider
            .create(
                ctx(id, "2026-08-26T06:00:00Z"),
                ProviderDraft {
                    title: "completed".into(),
                    category: "bug".into(),
                    priority: Priority::Default,
                    status: Status::NotStarted,
                    details: String::new(),
                    tags: vec![],
                    up_next: false,
                    blocked_by: vec![],
                    transfer: None,
                },
            )
            .unwrap();
        provider
            .update(
                &id.to_string(),
                Timestamp::new("2026-08-26T06:01:00Z"),
                ProviderPatch {
                    status: Some(Status::Completed),
                    ..ProviderPatch::default()
                },
            )
            .unwrap();
        let evidence_id = Ulid::new();
        let conflict = provider
            .store
            .attachment_dir(&id)
            .join(evidence_id.to_string());
        std::fs::create_dir_all(&conflict).unwrap();
        std::fs::write(conflict.join("existing.txt"), b"keep").unwrap();
        let error = provider
            .report_not_working(
                &id.to_string(),
                Timestamp::new("2026-08-26T06:02:00Z"),
                NotWorkingReport {
                    expected_token: None,
                    note: Some((Ulid::new(), "regressed".into())),
                    evidence: vec![ProviderEvidence {
                        id: evidence_id,
                        filename: "proof.txt".into(),
                        created_at: Timestamp::new("2026-08-26T06:02:00Z"),
                        bytes: b"new".to_vec(),
                    }],
                },
            )
            .unwrap_err();
        assert!(error.to_string().contains("already exists"));
        let unchanged = provider.get(&id.to_string()).unwrap();
        assert_eq!(unchanged.status, Status::Completed);
        assert!(!unchanged.up_next);
        assert!(
            unchanged
                .notes
                .iter()
                .all(|note| !note.text.starts_with("Not working:"))
        );
        assert!(unchanged.attachments.is_empty());
        assert_eq!(
            std::fs::read(conflict.join("existing.txt")).unwrap(),
            b"keep"
        );
    }

    #[test]
    fn transfer_operation_id_cannot_collide_across_sources() {
        let (_source_dir, source) = git_provider();
        let destination_dir = tempfile::tempdir().unwrap();
        let destination_store =
            FsStore::init(destination_dir.path(), &StoreMetadata::new("DST")).unwrap();
        let ids = [Ulid::new(), Ulid::new()];
        for (index, id) in ids.iter().enumerate() {
            source
                .create(
                    ctx(*id, &format!("2026-08-26T04:0{index}:00Z")),
                    ProviderDraft {
                        title: format!("source {index}"),
                        category: "task".into(),
                        priority: Priority::Default,
                        status: Status::NotStarted,
                        details: String::new(),
                        tags: vec![],
                        up_next: false,
                        blocked_by: vec![],
                        transfer: None,
                    },
                )
                .unwrap();
        }
        let registry = ProviderRegistry::default();
        registry.register(Arc::new(source)).unwrap();
        registry
            .register(Arc::new(GitProvider::new(
                "destination",
                destination_store.clone(),
            )))
            .unwrap();
        let source_ref = |id: Ulid| TicketRef {
            connection_id: "local".into(),
            native_id: id.to_string(),
        };
        copy_between(
            &registry,
            source_ref(ids[0]),
            "destination",
            "shared-op",
            Timestamp::new("2026-08-26T04:03:00Z"),
        )
        .unwrap();
        let error = copy_between(
            &registry,
            source_ref(ids[1]),
            "destination",
            "shared-op",
            Timestamp::new("2026-08-26T04:04:00Z"),
        )
        .unwrap_err();
        assert!(matches!(
            error,
            TransferError::Provider(ProviderError::Conflict { .. })
        ));
        assert_eq!(destination_store.list_tickets().unwrap().len(), 1);
    }
}
