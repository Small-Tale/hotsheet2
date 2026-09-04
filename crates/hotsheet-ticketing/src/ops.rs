//! Ticket operations over a store — the single implementation of query / create /
//! update / close / claim that every surface (CLI, server, MCP shim) calls, so
//! behavior can't drift between them (`docs/04-core-server-cli.md` §4.5).
//!
//! These are policy-free and synchronous. Wall-clock time is **injected** as a
//! [`Timestamp`] by the caller (which owns a clock), keeping this layer testable.

use std::cmp::Ordering;
use std::collections::HashSet;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::str::FromStr;

use hotsheet_model::{
    CloseReason, Note, NoteKind, Priority, ReviewRequest, Status, Ticket, Timestamp, Ulid,
    derive_slug,
};

use crate::store::{FsStore, StoreError};

/// An error from a ticket operation (store I/O or a coordination-policy violation).
#[derive(Debug, thiserror::Error)]
pub enum OpError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("{slug} is claimed by '{holder}', not '{worker}'")]
    WrongWorker {
        slug: String,
        holder: String,
        worker: String,
    },
    #[error("{0} is not claimed")]
    NotClaimed(String),
    #[error("{slug} cannot be claimed: {reason}")]
    ClaimUnavailable { slug: String, reason: &'static str },
    #[error("a duplicate target is required when the close reason is `duplicate`")]
    DuplicateNeedsTarget,
    #[error("no ticket matching '{0}'")]
    UnknownTicket(String),
    #[error("a ticket cannot block itself ({0})")]
    SelfBlock(String),
    #[error("Not Working can only be reported for a completed ticket (found {0:?})")]
    NotWorkingRequiresCompleted(Status),
    #[error("a Not Working report requires a note or at least one evidence attachment")]
    EmptyNotWorkingReport,
}

// ---- query -----------------------------------------------------------------------

/// How to order query results.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SortKey {
    #[default]
    Id,
    Created,
    Updated,
    Priority,
    Status,
    Title,
}

impl FromStr for SortKey {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        Ok(match s {
            "id" => SortKey::Id,
            "created" => SortKey::Created,
            "updated" => SortKey::Updated,
            "priority" => SortKey::Priority,
            "status" => SortKey::Status,
            "title" => SortKey::Title,
            other => return Err(format!("invalid sort '{other}'")),
        })
    }
}

/// Filters + sort for a ticket query. Empty/`None` fields don't constrain.
#[derive(Debug, Clone, Default)]
pub struct TicketQuery {
    pub status: Option<Status>,
    pub priority: Option<Priority>,
    pub category: Option<String>,
    /// A ticket must carry every one of these tags.
    pub tags: Vec<String>,
    /// Case-insensitive substring across slug, title, tags, details, and note text.
    pub text: Option<String>,
    pub up_next_only: bool,
    /// Exclude terminal/hidden statuses (completed/verified/deleted/archive/moved).
    pub open_only: bool,
    /// Filter by a specific close reason (the structured, filterable close tag, HS2-61).
    pub close_reason: Option<CloseReason>,
    /// `Some(true)` = only closed tickets (a `close_reason` is set); `Some(false)` = only
    /// tickets with no close reason. `None` doesn't constrain. Distinct from `open_only`
    /// (status-based): a `close_reason` now implies a terminal status (HS2-3XHT9P), but a
    /// ticket can be `completed` *without* a close_reason (marked done, never formally
    /// closed), so the two filters still differ.
    pub closed: Option<bool>,
    /// Only tickets this person (git email) is an assignee of (HS2-20).
    pub assignee: Option<String>,
    /// Only tickets this person (git email) has a review request on (HS2-T84F9F).
    pub review_requested: Option<String>,
    /// Only tickets whose review was requested by this person (HS2-NZT80R).
    pub review_by: Option<String>,
    /// `Some(true)` = only claimed tickets (a `claimed_by` is set); `Some(false)` = only
    /// unclaimed. `None` doesn't constrain (HS2-89).
    pub claimed: Option<bool>,
    /// `Some(true)` = only **blocked** tickets (some `blocked_by` isn't a done ticket);
    /// `Some(false)` = only **unblocked**. `None` doesn't constrain (HS2-T84F9F, docs/03 §3.3).
    pub blocked: Option<bool>,
    /// Half-open `created_at` / `updated_at` range filters (ISO-8601 strings; inclusive
    /// lower, inclusive upper — lexical compare works on RFC3339). Custom-view dimensions.
    pub created_after: Option<String>,
    pub created_before: Option<String>,
    pub updated_after: Option<String>,
    pub updated_before: Option<String>,
    pub sort: SortKey,
    /// Cap the number of rows returned (after sort). `None` = no cap.
    pub limit: Option<usize>,
    /// Keyset cursor: return only rows that sort **strictly after** the ticket with this
    /// ULID, in the current `sort` order (with `id` as the stable tiebreaker — the same total
    /// order the index uses). `None` = start at the top. The general form of `limit`, so a
    /// client pages a large store without `OFFSET` (docs/03 §3.5, HS2-TCDTCH). If the cursor
    /// ticket no longer exists, the page is empty (the client should restart from the top).
    pub page_after: Option<Ulid>,
}

/// Run a query: read the store, filter, sort, and (if set) cap to `limit`.
pub fn query(store: &FsStore, q: &TicketQuery) -> Result<Vec<Ticket>, StoreError> {
    let mut tickets = store.list_tickets()?;
    let text = q.text.as_deref().map(str::to_lowercase);
    // The done set, for the blocked/unblocked filter (a blocker not in it → still blocking).
    let done: HashSet<Ulid> = tickets
        .iter()
        .filter(|t| is_done(t))
        .map(|t| t.id)
        .collect();
    tickets.retain(|t| {
        q.status.is_none_or(|s| t.status == s)
            && q.priority.is_none_or(|p| t.priority == p)
            && q.category.as_deref().is_none_or(|c| t.category == c)
            // Defensive read-side enforcement for legacy or hand-edited files: Up Next is
            // exclusively an active-work queue, never backlog/archive/completed/etc.
            && (!q.up_next_only || (t.up_next && t.status.is_active()))
            && (!q.open_only || is_open(t))
            // Moved tombstones are hidden from lists unless explicitly asked for (docs/03 §3.5).
            && (q.status == Some(Status::Moved) || t.status != Status::Moved)
            && q.close_reason.is_none_or(|r| t.close_reason == Some(r))
            && q.closed.is_none_or(|want| t.close_reason.is_some() == want)
            && q.assignee
                .as_deref()
                .is_none_or(|a| t.assignees.iter().any(|x| x == a))
            && q.review_requested
                .as_deref()
                .is_none_or(|who| t.review_requests.iter().any(|r| r.who == who))
            && q.review_by.as_deref().is_none_or(|who| {
                t.review_requests
                    .iter()
                    .any(|r| r.requested_by.as_deref() == Some(who))
            })
            && q.claimed.is_none_or(|want| t.claimed_by.is_some() == want)
            && q.blocked.is_none_or(|want| is_blocked(t, &done) == want)
            && q.created_after
                .as_deref()
                .is_none_or(|a| t.created_at.as_str() >= a)
            && q.created_before
                .as_deref()
                .is_none_or(|b| t.created_at.as_str() <= b)
            && q.updated_after
                .as_deref()
                .is_none_or(|a| t.updated_at.as_str() >= a)
            && q.updated_before
                .as_deref()
                .is_none_or(|b| t.updated_at.as_str() <= b)
            && q.tags.iter().all(|tag| t.tags.iter().any(|x| x == tag))
            && text.as_deref().is_none_or(|needle| matches_text(t, needle))
    });
    sort_tickets(&mut tickets, q.sort);
    // Keyset: drop everything up to and including the cursor row (HS2-TCDTCH). A missing
    // cursor id yields an empty page — the client restarts from the top.
    if let Some(cursor) = q.page_after {
        match tickets.iter().position(|t| t.id == cursor) {
            Some(pos) => {
                tickets.drain(..=pos);
            }
            None => tickets.clear(),
        }
    }
    if let Some(n) = q.limit {
        tickets.truncate(n);
    }
    Ok(tickets)
}

/// Resolve a ticket by ULID (exact) or by slug (case-insensitive).
pub fn resolve(store: &FsStore, needle: &str) -> Result<Option<Ticket>, StoreError> {
    if let Ok(id) = Ulid::from_string(needle) {
        return match store.read_ticket(&id) {
            Ok(t) => Ok(Some(t)),
            Err(e) if e.is_io_kind(std::io::ErrorKind::NotFound) => Ok(None),
            Err(e) => Err(e),
        };
    }
    let wanted = needle.to_uppercase();
    Ok(store
        .list_tickets()?
        .into_iter()
        .find(|t| t.slug.eq_ignore_ascii_case(&wanted)))
}

fn matches_text(t: &Ticket, needle_lower: &str) -> bool {
    t.slug.to_lowercase().contains(needle_lower)
        || t.title.to_lowercase().contains(needle_lower)
        || t.tags
            .iter()
            .any(|tag| tag.to_lowercase().contains(needle_lower))
        || t.details.to_lowercase().contains(needle_lower)
        || t.notes
            .iter()
            .any(|n| n.text.to_lowercase().contains(needle_lower))
}

fn sort_tickets(tickets: &mut [Ticket], key: SortKey) {
    // Every arm breaks ties by `id` last, so the order is total and deterministic — the same
    // `ORDER BY {col}, t.id` the index uses. Keyset pagination (`page_after`) relies on this.
    match key {
        SortKey::Id => tickets.sort_by_key(|t| t.id),
        SortKey::Created => tickets.sort_by(|a, b| {
            a.created_at
                .as_str()
                .cmp(b.created_at.as_str())
                .then(a.id.cmp(&b.id))
        }),
        SortKey::Updated => tickets.sort_by(|a, b| {
            a.updated_at
                .as_str()
                .cmp(b.updated_at.as_str())
                .then(a.id.cmp(&b.id))
        }),
        SortKey::Priority => tickets.sort_by(|a, b| {
            priority_rank(a.priority)
                .cmp(&priority_rank(b.priority))
                .then(a.id.cmp(&b.id))
        }),
        SortKey::Status => tickets.sort_by(|a, b| {
            (a.status as u8)
                .cmp(&(b.status as u8))
                .then(a.id.cmp(&b.id))
        }),
        SortKey::Title => tickets.sort_by(|a, b| {
            a.title
                .to_lowercase()
                .cmp(&b.title.to_lowercase())
                .then(a.id.cmp(&b.id))
        }),
    }
}

/// A ticket in a workflow-open state (not terminal/hidden).
pub fn is_open(t: &Ticket) -> bool {
    !matches!(
        t.status,
        Status::Completed | Status::Verified | Status::Deleted | Status::Archive | Status::Moved
    )
}

/// A ticket whose work is finished (a `blocked_by` dependency is satisfied by this).
pub fn is_done(t: &Ticket) -> bool {
    matches!(t.status, Status::Completed | Status::Verified)
}

/// Highest → lowest as 0 → 4, for sorting/selection.
pub fn priority_rank(p: Priority) -> u8 {
    match p {
        Priority::Highest => 0,
        Priority::High => 1,
        Priority::Default => 2,
        Priority::Low => 3,
        Priority::Lowest => 4,
    }
}

// ---- create ----------------------------------------------------------------------

/// Fields for a new ticket. `title` + `category` are the essentials; the rest default.
#[derive(Debug, Clone, Default)]
pub struct NewTicket {
    pub title: String,
    pub category: String,
    pub priority: Priority,
    pub status: Status,
    pub details: String,
    pub tags: Vec<String>,
    pub up_next: bool,
    /// Blockers, already resolved to ULIDs (see [`resolve_blockers`]).
    pub blocked_by: Vec<Ulid>,
}

/// Expand leading `[tag]` title shorthand for every core-backed creation path.
///
/// Existing tags keep their order. Extracted tags are whitespace-normalized with
/// hyphens and appended once. `\[` escapes a literal leading bracket, malformed or
/// empty groups stop extraction, and an all-tag title remains literal so creation
/// never silently produces an empty title.
pub fn normalize_new_ticket_input(mut new: NewTicket) -> NewTicket {
    let original = new.title.trim().to_string();
    if let Some(literal) = original.strip_prefix("\\[") {
        new.title = format!("[{literal}");
        dedupe_tags(&mut new.tags);
        return new;
    }

    let mut rest = original.as_str();
    let mut extracted = Vec::new();
    while let Some(after_open) = rest.strip_prefix('[') {
        let Some(close) = after_open.find(']') else {
            break;
        };
        let content = &after_open[..close];
        if content.is_empty() || content.contains('[') {
            break;
        }
        let tag = content.split_whitespace().collect::<Vec<_>>().join("-");
        if tag.is_empty() {
            break;
        }
        extracted.push(tag);
        rest = after_open[close + 1..].trim_start();
    }

    if !extracted.is_empty() && !rest.is_empty() {
        new.title = rest.trim().to_string();
        new.tags.extend(extracted);
    } else {
        new.title = original;
    }
    dedupe_tags(&mut new.tags);
    new
}

fn dedupe_tags(tags: &mut Vec<String>) {
    let mut unique = Vec::with_capacity(tags.len());
    for tag in tags.drain(..) {
        if !unique.contains(&tag) {
            unique.push(tag);
        }
    }
    *tags = unique;
}

/// Resolve slug-or-ULID `needles` to blocker ULIDs, rejecting unknown tickets and
/// (when `target` is given) a self-reference. Deduplicates while preserving order.
/// Surfaces call this to turn user-facing strings into the `Vec<Ulid>` the model
/// stores, mirroring how `duplicate_of` is resolved on close.
pub fn resolve_blockers(
    store: &FsStore,
    target: Option<&Ulid>,
    needles: &[String],
) -> Result<Vec<Ulid>, OpError> {
    let mut ids = Vec::with_capacity(needles.len());
    for n in needles {
        let t = resolve(store, n)?.ok_or_else(|| OpError::UnknownTicket(n.clone()))?;
        if target == Some(&t.id) {
            return Err(OpError::SelfBlock(t.slug));
        }
        if !ids.contains(&t.id) {
            ids.push(t.id);
        }
    }
    Ok(ids)
}

/// Create + write a ticket with a caller-minted id, at time `now`.
pub fn create(
    store: &FsStore,
    id: Ulid,
    prefix: &str,
    now: Timestamp,
    new: NewTicket,
) -> Result<Ticket, StoreError> {
    let new = normalize_new_ticket_input(new);
    let mut t = Ticket::new(
        id,
        derive_slug(&id, prefix),
        new.title,
        new.category,
        now.clone(),
        now,
    );
    t.priority = new.priority;
    t.status = new.status;
    t.details = new.details;
    t.tags = new.tags;
    t.up_next = new.up_next && t.status.is_active();
    t.blocked_by = new.blocked_by;
    store.write_ticket_committing(&t)?;
    Ok(t)
}

// ---- update ----------------------------------------------------------------------

/// A partial update; `None` leaves a field unchanged.
#[derive(Debug, Clone, Default)]
pub struct TicketPatch {
    pub title: Option<String>,
    pub details: Option<String>,
    pub category: Option<String>,
    pub priority: Option<Priority>,
    pub status: Option<Status>,
    pub tags: Option<Vec<String>>,
    pub up_next: Option<bool>,
    /// Replace the blocker set (already resolved to ULIDs); `Some(vec![])` clears it.
    pub blocked_by: Option<Vec<Ulid>>,
    /// Absent leaves the reason unchanged; present `None` clears it.
    pub blocked_reason: Option<Option<String>>,
}

/// Apply a patch to an existing ticket and write it. A move to a terminal status
/// stamps `completed_at`/`verified_at`. Bumps `updated_at` to `now`.
pub fn update(
    store: &FsStore,
    id: &Ulid,
    now: Timestamp,
    patch: TicketPatch,
) -> Result<Ticket, StoreError> {
    let mut t = store.read_ticket(id)?;
    let previous_status = t.status;
    if let Some(v) = patch.title {
        t.title = v;
    }
    if let Some(v) = patch.details {
        t.details = v;
    }
    if let Some(v) = patch.category {
        t.category = v;
    }
    if let Some(v) = patch.priority {
        t.priority = v;
    }
    if let Some(v) = patch.tags {
        t.tags = v;
    }
    if let Some(v) = patch.up_next {
        t.up_next = v;
    }
    if let Some(v) = patch.blocked_by {
        t.blocked_by = v;
    }
    if let Some(v) = patch.blocked_reason {
        t.blocked_reason = v.and_then(|reason| {
            let normalized = reason.trim();
            (!normalized.is_empty()).then(|| normalized.to_string())
        });
    }
    if let Some(s) = patch.status {
        t.status = s;
        match s {
            Status::Completed if t.completed_at.is_none() => t.completed_at = Some(now.clone()),
            Status::Verified if t.verified_at.is_none() => t.verified_at = Some(now.clone()),
            _ => {}
        }
        if s != previous_status {
            append_status_transition(&mut t, previous_status, s, &now);
        }
        // Leaving the active set (not_started/started) drops it off Up Next — applied after
        // any up_next in this same patch, so a move out of active always wins (HS2-55610S).
        if !s.is_active() {
            t.up_next = false;
        } else if t.close_reason.is_some() {
            // Reopening — moving back to an active status — clears the close annotation
            // (close_reason/closed_at/duplicate_of), per HS2-61.
            t.close_reason = None;
            t.closed_at = None;
            t.duplicate_of = None;
        }
    }
    // Also covers `--up-next` on an already-inactive ticket when no status is present in
    // this patch. Up Next is only meaningful for not_started/started.
    if !t.status.is_active() {
        t.up_next = false;
    }
    t.updated_at = now;
    store.write_ticket_committing(&t)?;
    Ok(t)
}

fn append_status_transition(ticket: &mut Ticket, from: Status, to: Status, now: &Timestamp) {
    let mut entropy = DefaultHasher::new();
    ticket.id.hash(&mut entropy);
    ticket.notes.len().hash(&mut entropy);
    (from as u8).hash(&mut entropy);
    (to as u8).hash(&mut entropy);
    now.as_str().hash(&mut entropy);
    let timestamp_ms = now
        .instant()
        .map(|instant| instant.unix_timestamp_nanos().max(0) as u64 / 1_000_000)
        .unwrap_or_default();
    ticket.notes.push(Note {
        id: Ulid::from_parts(timestamp_ms, entropy.finish() as u128),
        kind: NoteKind::Activity,
        created_at: now.clone(),
        edited_at: now.clone(),
        summary: Some(status_label(to).to_string()),
        text: format!(
            "Status changed from {} to {}",
            status_label(from),
            status_label(to)
        ),
    });
}

/// Prepare the single ticket mutation used by an atomic Not Working provider
/// operation. Persistence and evidence payload staging remain provider-owned.
pub fn prepare_not_working(
    ticket: &mut Ticket,
    now: Timestamp,
    note: Option<(Ulid, String)>,
    has_evidence: bool,
    reporter: Option<&str>,
) -> Result<(), OpError> {
    if ticket.status != Status::Completed {
        return Err(OpError::NotWorkingRequiresCompleted(ticket.status));
    }
    let note = note.and_then(|(id, text)| {
        let text = text.trim().to_string();
        (!text.is_empty()).then_some((id, text))
    });
    if note.is_none() && !has_evidence {
        return Err(OpError::EmptyNotWorkingReport);
    }
    let summary = note
        .as_ref()
        .map(|(_, text)| summarize_not_working(text))
        .unwrap_or_else(|| "Evidence attached for review.".into());
    if let Some((id, text)) = note {
        ticket.notes.push(Note {
            id,
            kind: NoteKind::Regular,
            created_at: now.clone(),
            edited_at: now.clone(),
            summary: None,
            text: format!("Not working: {text}"),
        });
    }
    let mut entropy = DefaultHasher::new();
    ticket.id.hash(&mut entropy);
    ticket.notes.len().hash(&mut entropy);
    "not-working-report".hash(&mut entropy);
    now.as_str().hash(&mut entropy);
    let timestamp_ms = now
        .instant()
        .map(|instant| instant.unix_timestamp_nanos().max(0) as u64 / 1_000_000)
        .unwrap_or_default();
    ticket.notes.push(Note {
        id: Ulid::from_parts(timestamp_ms, entropy.finish() as u128),
        kind: NoteKind::Activity,
        created_at: now.clone(),
        edited_at: now.clone(),
        summary: Some("Reported as not working".into()),
        text: reporter
            .filter(|value| !value.trim().is_empty())
            .map(|value| format!("{} reported as not working\n{summary}", value.trim()))
            .unwrap_or_else(|| format!("Reported as not working\n{summary}")),
    });
    let previous = ticket.status;
    ticket.status = Status::NotStarted;
    ticket.up_next = true;
    ticket.close_reason = None;
    ticket.closed_at = None;
    ticket.duplicate_of = None;
    append_status_transition(ticket, previous, ticket.status, &now);
    ticket.updated_at = now;
    Ok(())
}

fn summarize_not_working(text: &str) -> String {
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = compact.chars();
    let summary = chars.by_ref().take(140).collect::<String>();
    if chars.next().is_some() {
        format!("{}…", summary.trim_end())
    } else {
        summary
    }
}

fn status_label(status: Status) -> &'static str {
    match status {
        Status::NotStarted => "Not Started",
        Status::Started => "Started",
        Status::Completed => "Completed",
        Status::Verified => "Verified",
        Status::Backlog => "Backlog",
        Status::Archive => "Archive",
        Status::Deleted => "Deleted",
        Status::Moved => "Moved",
    }
}

/// Append a note to a ticket. The caller mints the note id (a timestamp-ordered
/// ULID, `docs/02` §2.6) and passes the wall clock as `now`.
pub fn add_note(
    store: &FsStore,
    id: &Ulid,
    note_id: Ulid,
    now: Timestamp,
    kind: NoteKind,
    text: String,
) -> Result<Ticket, StoreError> {
    add_note_with_summary(store, id, note_id, now, kind, None, text)
}

/// Append a note with an optional concise timeline headline. Whitespace is collapsed
/// so summaries remain a single plain-text line regardless of the calling surface.
pub fn add_note_with_summary(
    store: &FsStore,
    id: &Ulid,
    note_id: Ulid,
    now: Timestamp,
    kind: NoteKind,
    summary: Option<String>,
    text: String,
) -> Result<Ticket, StoreError> {
    let mut t = store.read_ticket(id)?;
    let kind = if kind == NoteKind::Regular && Note::text_requests_feedback(&text) {
        NoteKind::FeedbackNeeded
    } else {
        kind
    };
    t.notes.push(Note {
        id: note_id,
        kind,
        created_at: now.clone(),
        edited_at: now.clone(),
        summary: summary.and_then(|value| {
            let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
            (!value.is_empty()).then_some(value)
        }),
        text,
    });
    t.updated_at = now;
    store.write_ticket_committing(&t)?;
    Ok(t)
}

/// Edit an existing note without changing its creation time.
pub fn edit_note(
    store: &FsStore,
    ticket_id: &Ulid,
    note_id: &Ulid,
    now: Timestamp,
    text: String,
) -> Result<Ticket, StoreError> {
    let mut ticket = store.read_ticket(ticket_id)?;
    let note = ticket
        .notes
        .iter_mut()
        .find(|note| &note.id == note_id)
        .ok_or_else(|| {
            StoreError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("note {note_id}"),
            ))
        })?;
    note.text = text;
    note.edited_at = now.clone();
    ticket.updated_at = now;
    store.write_ticket_committing(&ticket)?;
    Ok(ticket)
}

/// Delete one note while preserving the ticket and remaining note order.
pub fn delete_note(
    store: &FsStore,
    ticket_id: &Ulid,
    note_id: &Ulid,
    now: Timestamp,
) -> Result<Ticket, StoreError> {
    let mut ticket = store.read_ticket(ticket_id)?;
    let before = ticket.notes.len();
    ticket.notes.retain(|note| &note.id != note_id);
    if ticket.notes.len() == before {
        return Err(StoreError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("note {note_id}"),
        )));
    }
    ticket.updated_at = now;
    store.write_ticket_committing(&ticket)?;
    Ok(ticket)
}

/// Record a close outcome (`docs/02` §2.6a). Closing **settles the status**: a
/// close_reason may never coexist with an active status (not_started/started), so an
/// active ticket is moved to `completed` (stamping `completed_at`). A ticket already
/// in another terminal status (verified/deleted/archive/moved) keeps that status —
/// closing a verified ticket doesn't downgrade it. The inverse (moving back to an
/// active status clears the close annotation) lives in [`update`] (HS2-3XHT9P).
pub fn close(
    store: &FsStore,
    id: &Ulid,
    now: Timestamp,
    reason: CloseReason,
    duplicate_of: Option<Ulid>,
) -> Result<Ticket, OpError> {
    if reason == CloseReason::Duplicate && duplicate_of.is_none() {
        return Err(OpError::DuplicateNeedsTarget);
    }
    let mut t = store.read_ticket(id)?;
    t.close_reason = Some(reason);
    t.closed_at = Some(now.clone());
    t.duplicate_of = duplicate_of;
    // A close_reason can't sit on an active status — settle it to `completed` (a ticket
    // already in another terminal status keeps it). This is the write-side half of the
    // invariant `update` enforces from the other direction (HS2-3XHT9P).
    if t.status.is_active() {
        let previous_status = t.status;
        t.status = Status::Completed;
        if t.completed_at.is_none() {
            t.completed_at = Some(now.clone());
        }
        append_status_transition(&mut t, previous_status, Status::Completed, &now);
    }
    // A closed ticket is no longer Up Next, whatever its status field (HS2-55610S).
    t.up_next = false;
    t.updated_at = now;
    store.write_ticket_committing(&t)?;
    Ok(t)
}

// ---- cross-store copy / move (docs/02 §2.13, HS2-60) -----------------------------

/// Copy a ticket into another store as a **new** ticket (new ULID, destination prefix),
/// carrying the same content + attachments and recording `copied_from` provenance. The
/// source is untouched. Workflow/coordination/close/move state is reset — a copy is a fresh
/// idea (HS1's copy = a new ticket number). `new_id` is caller-minted; `now` stamps it.
pub fn copy_ticket(
    src: &FsStore,
    dest: &FsStore,
    id: &Ulid,
    new_id: Ulid,
    now: Timestamp,
) -> Result<Ticket, OpError> {
    let orig = src.read_ticket(id)?;
    let dest_prefix = dest.metadata()?.ticket_prefix;

    let mut t = orig.clone();
    t.id = new_id;
    t.slug = derive_slug(&new_id, &dest_prefix);
    t.copied_from = Some(*id);
    t.created_at = now.clone();
    t.updated_at = now;
    // A fresh copy starts clean: no claim, no close/move annotation, off Up Next.
    t.status = Status::NotStarted;
    t.up_next = false;
    t.claimed_by = None;
    t.claim_lease_expires_at = None;
    t.worker_label = None;
    t.claim_count = 0;
    t.completed_at = None;
    t.verified_at = None;
    t.closed_at = None;
    t.close_reason = None;
    t.duplicate_of = None;
    t.moved_to_store = None;
    t.moved_at = None;

    dest.write_ticket_committing(&t)?;
    copy_attachments(src, dest, id, &new_id)?;
    Ok(t)
}

/// The result of a [`move_ticket`]: the live ticket now in the destination, and the
/// tombstone/redirect left in the source.
#[derive(Debug, Clone)]
pub struct MoveOutcome {
    pub moved: Ticket,
    pub tombstone: Ticket,
}

/// Move a ticket to another store, **keeping the same ULID** so references survive
/// (`docs/02` §2.13). The destination gets the ticket (destination prefix → new slug) +
/// attachments; the source keeps a `status: moved` tombstone pointing at `dest_id`. Note:
/// git never forgets — this does **not** purge the ticket from the source's history/remote
/// (see the retention caveat); callers surface that warning.
pub fn move_ticket(
    src: &FsStore,
    dest: &FsStore,
    id: &Ulid,
    dest_id: &str,
    now: Timestamp,
) -> Result<MoveOutcome, OpError> {
    let orig = src.read_ticket(id)?;
    let dest_prefix = dest.metadata()?.ticket_prefix;

    // Destination: same ULID, destination slug; it's the live instance.
    let mut moved = orig.clone();
    moved.slug = derive_slug(id, &dest_prefix);
    moved.updated_at = now.clone();
    moved.moved_to_store = None;
    moved.moved_at = None;
    dest.write_ticket_committing(&moved)?;
    copy_attachments(src, dest, id, id)?;

    // Source: a tombstone/redirect the UI hides (status = moved).
    let mut tombstone = orig;
    let previous_status = tombstone.status;
    tombstone.status = Status::Moved;
    if previous_status != Status::Moved {
        append_status_transition(&mut tombstone, previous_status, Status::Moved, &now);
    }
    tombstone.moved_to_store = Some(dest_id.to_string());
    tombstone.moved_at = Some(now.clone());
    tombstone.updated_at = now;
    tombstone.up_next = false;
    tombstone.claimed_by = None;
    tombstone.claim_lease_expires_at = None;
    src.write_ticket_committing(&tombstone)?;
    // The attachments now live in the destination; drop the source working-tree copy
    // (git history still retains them — that's the retention caveat).
    let _ = std::fs::remove_dir_all(src.attachment_dir(id));

    Ok(MoveOutcome { moved, tombstone })
}

/// Copy a ticket's attachment files from one store to another (best-effort: no attachments
/// dir → nothing to do).
fn copy_attachments(
    src: &FsStore,
    dest: &FsStore,
    src_id: &Ulid,
    dest_id: &Ulid,
) -> Result<(), OpError> {
    let from = src.attachment_dir(src_id);
    let to = dest.attachment_dir(dest_id);
    let entries = match std::fs::read_dir(&from) {
        Ok(entries) => entries,
        Err(ref e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(StoreError::Io(e).into()),
    };
    std::fs::create_dir_all(&to).map_err(StoreError::Io)?;
    for entry in entries {
        let entry = entry.map_err(StoreError::Io)?;
        copy_attachment_entry(&entry.path(), &to.join(entry.file_name()))?;
    }
    Ok(())
}

fn copy_attachment_entry(from: &std::path::Path, to: &std::path::Path) -> Result<(), OpError> {
    if from.is_dir() {
        std::fs::create_dir_all(to).map_err(StoreError::Io)?;
        for entry in std::fs::read_dir(from).map_err(StoreError::Io)? {
            let entry = entry.map_err(StoreError::Io)?;
            copy_attachment_entry(&entry.path(), &to.join(entry.file_name()))?;
        }
    } else {
        std::fs::copy(from, to).map_err(StoreError::Io)?;
    }
    Ok(())
}

// ---- human assignment (docs/10 §10.2, HS2-20) ------------------------------------

/// Set a ticket's `assignees` (people expected to *do* it) and/or **add** review requests
/// (people wanted in the loop). Assignees are **replaced** (`None` = leave unchanged); review
/// requests are **appended**, deduped by their own ULID `by`, so they set-union across
/// workers exactly like notes — two people adding a reviewer never conflict. Person identity
/// is the git email; the `people.json` roster (see [`crate::roster`]) maps it to a name.
pub fn assign(
    store: &FsStore,
    id: &Ulid,
    now: Timestamp,
    set_assignees: Option<Vec<String>>,
    mut add_reviews: Vec<ReviewRequest>,
) -> Result<Ticket, StoreError> {
    let mut t = store.read_ticket(id)?;
    if let Some(assignees) = set_assignees {
        let mut seen = HashSet::new();
        t.assignees = assignees
            .into_iter()
            .filter(|e| seen.insert(e.clone()))
            .collect();
    }
    let requester = crate::current_user_email(store.root());
    for r in &mut add_reviews {
        if r.requested_by.is_none() {
            r.requested_by.clone_from(&requester);
        }
    }
    for r in add_reviews {
        if !t.review_requests.iter().any(|x| x.by == r.by) {
            t.review_requests.push(r);
        }
    }
    t.updated_at = now;
    store.write_ticket_committing(&t)?;
    Ok(t)
}

// ---- claim / lease ---------------------------------------------------------------

/// Blocked while any `blocked_by` dependency isn't done.
pub fn is_blocked(t: &Ticket, done: &HashSet<Ulid>) -> bool {
    t.blocked_by.iter().any(|b| !done.contains(b))
}

/// A claim is available if unclaimed, lease-less (stale), or the lease is at/before `now`.
pub fn claim_available(t: &Ticket, now: &Timestamp) -> bool {
    match (&t.claimed_by, &t.claim_lease_expires_at) {
        (None, _) | (Some(_), None) => true,
        (Some(_), Some(exp)) => exp.chronological_cmp(now) != Some(Ordering::Greater),
    }
}

/// Claim the next available ticket for `worker` (open, unblocked, unclaimed/expired),
/// preferring Up Next, then priority, then creation order. `None` if nothing is free.
/// This is the LOCAL primitive; distributed CAS layers on top (HS2-84).
pub fn claim_next(
    store: &FsStore,
    now: &Timestamp,
    lease_expires: Timestamp,
    worker: &str,
    label: Option<String>,
) -> Result<Option<Ticket>, StoreError> {
    let tickets = store.list_tickets()?;
    let done: HashSet<Ulid> = tickets
        .iter()
        .filter(|t| is_done(t))
        .map(|t| t.id)
        .collect();

    let mut candidates: Vec<Ticket> = tickets
        .into_iter()
        .filter(|t| is_open(t) && !is_blocked(t, &done) && claim_available(t, now))
        .collect();
    candidates.sort_by(|a, b| {
        b.up_next
            .cmp(&a.up_next)
            .then(priority_rank(a.priority).cmp(&priority_rank(b.priority)))
            .then(a.id.cmp(&b.id))
    });

    let Some(mut t) = candidates.into_iter().next() else {
        return Ok(None);
    };
    t.claimed_by = Some(worker.to_string());
    t.claim_lease_expires_at = Some(lease_expires);
    t.worker_label = label;
    t.claim_count += 1;
    t.updated_at = now.clone();
    store.write_ticket_committing(&t)?;
    Ok(Some(t))
}

/// Claim one exact open, unblocked ticket. A retry by the live holder is idempotent:
/// it may extend the lease and update an explicit label, but does not increment the
/// historical claim count. Expired/stale claims are new acquisitions and do increment it.
pub fn claim(
    store: &FsStore,
    id: &Ulid,
    now: &Timestamp,
    lease_expires: Timestamp,
    worker: &str,
    label: Option<String>,
) -> Result<Ticket, OpError> {
    let t = prepare_claim(store, id, now, lease_expires, worker, label)?;
    store.write_ticket_committing(&t)?;
    Ok(t)
}

/// Claim one exact ticket and transition a Not Started ticket to Started in the
/// same durable write. This prevents callers from exposing a live worker claim
/// while the workflow status still says that nobody has begun the work.
pub fn claim_and_start(
    store: &FsStore,
    id: &Ulid,
    now: &Timestamp,
    lease_expires: Timestamp,
    worker: &str,
    label: Option<String>,
) -> Result<Ticket, OpError> {
    let mut t = prepare_claim(store, id, now, lease_expires, worker, label)?;
    if t.status == Status::NotStarted {
        let previous = t.status;
        t.status = Status::Started;
        append_status_transition(&mut t, previous, Status::Started, now);
    }
    store.write_ticket_committing(&t)?;
    Ok(t)
}

fn prepare_claim(
    store: &FsStore,
    id: &Ulid,
    now: &Timestamp,
    lease_expires: Timestamp,
    worker: &str,
    label: Option<String>,
) -> Result<Ticket, OpError> {
    let tickets = store.list_tickets()?;
    let mut t = tickets
        .iter()
        .find(|ticket| ticket.id == *id)
        .cloned()
        .ok_or_else(|| OpError::UnknownTicket(id.to_string()))?;
    if !is_open(&t) {
        return Err(OpError::ClaimUnavailable {
            slug: t.slug,
            reason: "ticket is not open",
        });
    }
    let done: HashSet<Ulid> = tickets
        .iter()
        .filter(|ticket| is_done(ticket))
        .map(|ticket| ticket.id)
        .collect();
    if is_blocked(&t, &done) {
        return Err(OpError::ClaimUnavailable {
            slug: t.slug,
            reason: "ticket has unresolved blockers",
        });
    }

    if !claim_available(&t, now) {
        let holder = t
            .claimed_by
            .as_deref()
            .expect("an unavailable claim has a holder");
        if holder != worker {
            return Err(OpError::WrongWorker {
                slug: t.slug,
                holder: holder.to_string(),
                worker: worker.to_string(),
            });
        }
        if t.claim_lease_expires_at
            .as_ref()
            .and_then(|current| current.chronological_cmp(&lease_expires))
            == Some(Ordering::Less)
        {
            t.claim_lease_expires_at = Some(lease_expires);
        }
        if label.is_some() {
            t.worker_label = label;
        }
    } else {
        t.claimed_by = Some(worker.to_string());
        t.claim_lease_expires_at = Some(lease_expires);
        t.worker_label = label;
        t.claim_count += 1;
    }
    t.updated_at = now.clone();
    Ok(t)
}

/// Release a claim. Only the holding `worker` may release unless `force`.
pub fn release(
    store: &FsStore,
    id: &Ulid,
    now: Timestamp,
    worker: &str,
    force: bool,
) -> Result<Ticket, OpError> {
    let mut t = store.read_ticket(id)?;
    match &t.claimed_by {
        None => return Ok(t), // already released — idempotent
        Some(holder) if holder != worker && !force => {
            return Err(OpError::WrongWorker {
                slug: t.slug.clone(),
                holder: holder.clone(),
                worker: worker.to_string(),
            });
        }
        _ => {}
    }
    t.claimed_by = None;
    t.claim_lease_expires_at = None;
    t.worker_label = None;
    t.updated_at = now;
    store.write_ticket_committing(&t)?;
    Ok(t)
}

/// Renew a claim's lease. Must be the holding `worker`.
pub fn renew(
    store: &FsStore,
    id: &Ulid,
    now: Timestamp,
    lease_expires: Timestamp,
    worker: &str,
) -> Result<Ticket, OpError> {
    let mut t = store.read_ticket(id)?;
    match &t.claimed_by {
        Some(holder) if holder == worker => {}
        Some(holder) => {
            return Err(OpError::WrongWorker {
                slug: t.slug.clone(),
                holder: holder.clone(),
                worker: worker.to_string(),
            });
        }
        None => return Err(OpError::NotClaimed(t.slug.clone())),
    }
    t.claim_lease_expires_at = Some(lease_expires);
    t.updated_at = now;
    store.write_ticket_committing(&t)?;
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::StoreMetadata;

    fn store() -> (tempfile::TempDir, FsStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
        (dir, store)
    }

    fn ts(s: &str) -> Timestamp {
        Timestamp::new(s)
    }

    #[test]
    fn new_ticket_title_shorthand_extracts_only_valid_leading_tags() {
        let normalized = normalize_new_ticket_input(NewTicket {
            title: "  [client] [Needs Review] Fix selection [literal]  ".into(),
            tags: vec!["client".into(), "existing".into()],
            ..Default::default()
        });
        assert_eq!(normalized.title, "Fix selection [literal]");
        assert_eq!(normalized.tags, ["client", "existing", "Needs-Review"]);

        let escaped = normalize_new_ticket_input(NewTicket {
            title: r"\[client] Literal title".into(),
            ..Default::default()
        });
        assert_eq!(escaped.title, "[client] Literal title");
        assert!(escaped.tags.is_empty());

        let malformed = normalize_new_ticket_input(NewTicket {
            title: "[client] [] stays literal".into(),
            ..Default::default()
        });
        assert_eq!(malformed.title, "[] stays literal");
        assert_eq!(malformed.tags, ["client"]);

        let all_tag = normalize_new_ticket_input(NewTicket {
            title: "[client] [server]".into(),
            ..Default::default()
        });
        assert_eq!(all_tag.title, "[client] [server]");
        assert!(all_tag.tags.is_empty());

        let embedded = normalize_new_ticket_input(NewTicket {
            title: "Fix [client] selection".into(),
            ..Default::default()
        });
        assert_eq!(embedded.title, "Fix [client] selection");
        assert!(embedded.tags.is_empty());
    }

    #[test]
    fn create_persists_normalized_title_shorthand() {
        let (_d, store) = store();
        let created = create(
            &store,
            Ulid::new(),
            "HS",
            ts("2026-08-19T00:00:00Z"),
            NewTicket {
                title: "[client] [Needs Review] Fix selection".into(),
                category: "bug".into(),
                tags: vec!["client".into()],
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(created.title, "Fix selection");
        assert_eq!(created.tags, ["client", "Needs-Review"]);
        let persisted = store.read_ticket(&created.id).unwrap();
        assert_eq!(persisted.title, created.title);
        assert_eq!(persisted.tags, created.tags);
    }

    #[test]
    fn query_limit_caps_after_sort() {
        let (_d, store) = store();
        for i in 0..5 {
            create(
                &store,
                Ulid::new(),
                "HS",
                ts("2026-08-19T00:00:00Z"),
                NewTicket {
                    title: format!("t{i}"),
                    category: "task".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        }
        let all = query(&store, &TicketQuery::default()).unwrap();
        assert_eq!(all.len(), 5);

        let capped = query(
            &store,
            &TicketQuery {
                limit: Some(2),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(capped.len(), 2);
        // The cap is applied after sort, so it's the first two by the sort key (id),
        // not an arbitrary two.
        assert_eq!(&capped[..], &all[..2]);

        // A limit larger than the result set is a no-op, not an error.
        let over = query(
            &store,
            &TicketQuery {
                limit: Some(99),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(over.len(), 5);
    }

    #[test]
    fn keyset_paging_walks_the_whole_store_without_gaps_or_overlap() {
        let (_d, store) = store();
        for i in 0..7 {
            create(
                &store,
                Ulid::new(),
                "HS",
                ts("2026-08-19T00:00:00Z"),
                NewTicket {
                    title: format!("t{i}"),
                    category: "task".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        }
        let all = query(&store, &TicketQuery::default()).unwrap();
        assert_eq!(all.len(), 7);

        // Page through in chunks of 3 using the last row's id as the next cursor, and
        // reassemble — it must equal the single-shot sorted list exactly (no gaps, no dupes).
        let mut paged: Vec<Ulid> = Vec::new();
        let mut cursor: Option<Ulid> = None;
        loop {
            let page = query(
                &store,
                &TicketQuery {
                    limit: Some(3),
                    page_after: cursor,
                    ..Default::default()
                },
            )
            .unwrap();
            if page.is_empty() {
                break;
            }
            cursor = Some(page.last().unwrap().id);
            paged.extend(page.iter().map(|t| t.id));
        }
        assert_eq!(paged, all.iter().map(|t| t.id).collect::<Vec<_>>());

        // The cursor is strictly exclusive: page_after the last row yields nothing.
        let after_last = query(
            &store,
            &TicketQuery {
                page_after: Some(all.last().unwrap().id),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(after_last.is_empty());

        // A stale cursor (a ULID not in the store) yields an empty page, not the whole list.
        let stale = query(
            &store,
            &TicketQuery {
                page_after: Some(Ulid::new()),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(stale.is_empty());
    }

    #[test]
    fn blocked_by_resolve_set_clear_and_reject() {
        let (_d, store) = store();
        let mk = |title: &str| {
            create(
                &store,
                Ulid::new(),
                "HS",
                ts("2026-08-21T00:00:00Z"),
                NewTicket {
                    title: title.into(),
                    category: "task".into(),
                    ..Default::default()
                },
            )
            .unwrap()
        };
        let a = mk("blocker");
        let b = mk("blocked");

        // resolve by slug + ULID, deduped, order preserved
        let ids =
            resolve_blockers(&store, Some(&b.id), &[a.slug.clone(), a.id.to_string()]).unwrap();
        assert_eq!(ids, vec![a.id]);

        // set via update, then read back off disk
        let set = update(
            &store,
            &b.id,
            ts("2026-08-21T00:01:00Z"),
            TicketPatch {
                blocked_by: Some(ids),
                blocked_reason: Some(Some("  Waiting for review  ".into())),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(set.blocked_by, vec![a.id]);
        assert_eq!(set.blocked_reason.as_deref(), Some("Waiting for review"));
        assert_eq!(store.read_ticket(&b.id).unwrap().blocked_by, vec![a.id]);

        // Some(vec![]) clears; None leaves unchanged
        let cleared = update(
            &store,
            &b.id,
            ts("2026-08-21T00:02:00Z"),
            TicketPatch {
                blocked_by: Some(vec![]),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(cleared.blocked_by.is_empty());
        assert_eq!(
            cleared.blocked_reason.as_deref(),
            Some("Waiting for review")
        );

        let reason_cleared = update(
            &store,
            &b.id,
            ts("2026-08-21T00:03:00Z"),
            TicketPatch {
                blocked_reason: Some(None),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(reason_cleared.blocked_reason, None);
        assert_eq!(store.read_ticket(&b.id).unwrap().blocked_reason, None);

        // unknown ticket + self-reference are rejected
        assert!(matches!(
            resolve_blockers(&store, Some(&b.id), &["HS-NOPE00".into()]),
            Err(OpError::UnknownTicket(_))
        ));
        assert!(matches!(
            resolve_blockers(&store, Some(&b.id), std::slice::from_ref(&b.slug)),
            Err(OpError::SelfBlock(_))
        ));
        // with no target (create-time), a self-reference can't be detected — allowed
        assert_eq!(
            resolve_blockers(&store, None, std::slice::from_ref(&b.slug)).unwrap(),
            vec![b.id]
        );
    }

    #[test]
    fn create_query_update_close_flow() {
        let (_d, store) = store();
        let id = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
        let t = create(
            &store,
            id,
            "HS",
            ts("2026-08-19T00:00:00Z"),
            NewTicket {
                title: "Fix flicker".into(),
                category: "bug".into(),
                tags: vec!["client-search".into()],
                up_next: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(t.slug.starts_with("HS-"));

        // query with filters
        let q = TicketQuery {
            up_next_only: true,
            text: Some("FLICK".into()),
            ..Default::default()
        };
        assert_eq!(query(&store, &q).unwrap().len(), 1);
        assert_eq!(
            query(
                &store,
                &TicketQuery {
                    text: Some(t.slug.clone()),
                    ..Default::default()
                }
            )
            .unwrap()
            .len(),
            1
        );
        assert_eq!(
            query(
                &store,
                &TicketQuery {
                    text: Some("CLIENT-SEARCH".into()),
                    ..Default::default()
                }
            )
            .unwrap()
            .len(),
            1
        );
        assert!(
            query(
                &store,
                &TicketQuery {
                    status: Some(Status::Completed),
                    ..Default::default()
                }
            )
            .unwrap()
            .is_empty()
        );

        // update -> completed stamps completed_at
        let u = update(
            &store,
            &id,
            ts("2026-08-19T01:00:00Z"),
            TicketPatch {
                status: Some(Status::Completed),
                priority: Some(Priority::High),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(u.status, Status::Completed);
        assert!(u.completed_at.is_some());

        // close
        let c = close(
            &store,
            &id,
            ts("2026-08-19T02:00:00Z"),
            CloseReason::Completed,
            None,
        )
        .unwrap();
        assert_eq!(c.close_reason, Some(CloseReason::Completed));
        assert!(c.closed_at.is_some());
    }

    #[test]
    fn create_persists_initial_backlog_status_and_normalizes_up_next() {
        let (_d, store) = store();
        let id = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
        let created = create(
            &store,
            id,
            "HS",
            ts("2026-08-19T00:00:00Z"),
            NewTicket {
                title: "Deferred work".into(),
                category: "task".into(),
                status: Status::Backlog,
                up_next: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(created.status, Status::Backlog);
        assert!(!created.up_next);
        let persisted = store.read_ticket(&id).unwrap();
        assert_eq!(persisted.status, Status::Backlog);
        assert!(!persisted.up_next);
    }

    #[test]
    fn status_changes_append_durable_activity_timeline_entries() {
        let (_d, store) = store();
        let id = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
        create(
            &store,
            id,
            "HS",
            ts("2026-08-19T00:00:00Z"),
            NewTicket::default(),
        )
        .unwrap();

        let started = update(
            &store,
            &id,
            ts("2026-08-19T00:01:00Z"),
            TicketPatch {
                status: Some(Status::Started),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(started.notes.len(), 1);
        assert_eq!(started.notes[0].kind, NoteKind::Activity);
        assert_eq!(
            started.notes[0].text,
            "Status changed from Not Started to Started"
        );

        let unchanged = update(
            &store,
            &id,
            ts("2026-08-19T00:02:00Z"),
            TicketPatch {
                status: Some(Status::Started),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            unchanged.notes.len(),
            1,
            "same-state patches are not events"
        );

        let closed = close(
            &store,
            &id,
            ts("2026-08-19T00:03:00Z"),
            CloseReason::Completed,
            None,
        )
        .unwrap();
        assert_eq!(closed.notes.len(), 2);
        assert_eq!(
            closed.notes[1].text,
            "Status changed from Started to Completed"
        );
    }

    #[test]
    fn add_note_appends_and_bumps_updated_at() {
        let (_d, store) = store();
        let id = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
        create(
            &store,
            id,
            "HS",
            ts("2026-08-19T00:00:00Z"),
            NewTicket::default(),
        )
        .unwrap();

        let n1 = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FB0").unwrap();
        let t = add_note(
            &store,
            &id,
            n1,
            ts("2026-08-19T01:00:00Z"),
            NoteKind::Regular,
            "did the thing".into(),
        )
        .unwrap();
        assert_eq!(t.notes.len(), 1);
        assert_eq!(t.notes[0].text, "did the thing");
        assert_eq!(t.updated_at.as_str(), "2026-08-19T01:00:00Z");

        // A second note appends (order preserved), persisted to disk.
        let n2 = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FB1").unwrap();
        add_note(
            &store,
            &id,
            n2,
            ts("t2"),
            NoteKind::Regular,
            "and again".into(),
        )
        .unwrap();
        let reread = store.read_ticket(&id).unwrap();
        assert_eq!(reread.notes.len(), 2);
        assert_eq!(reread.notes[1].text, "and again");

        let original_created = reread.notes[0].created_at.clone();
        let edited = edit_note(
            &store,
            &id,
            &n1,
            ts("2026-08-19T03:00:00Z"),
            "did the corrected thing".into(),
        )
        .unwrap();
        assert_eq!(edited.notes[0].text, "did the corrected thing");
        assert_eq!(edited.notes[0].created_at, original_created);
        assert_eq!(edited.notes[0].edited_at.as_str(), "2026-08-19T03:00:00Z");

        let deleted = delete_note(&store, &id, &n1, ts("2026-08-19T04:00:00Z")).unwrap();
        assert_eq!(deleted.notes.len(), 1);
        assert_eq!(deleted.notes[0].id, n2);
        assert_eq!(deleted.updated_at.as_str(), "2026-08-19T04:00:00Z");
        assert!(delete_note(&store, &id, &n1, ts("t5")).is_err());
    }

    #[test]
    fn add_note_promotes_the_legacy_feedback_prefix_to_the_typed_kind() {
        let (_d, store) = store();
        let id = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
        create(
            &store,
            id,
            "HS",
            ts("2026-08-19T00:00:00Z"),
            NewTicket::default(),
        )
        .unwrap();

        let ticket = add_note(
            &store,
            &id,
            Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FB0").unwrap(),
            ts("2026-08-19T01:00:00Z"),
            NoteKind::Regular,
            "FEEDBACK NEEDED: choose a layout".into(),
        )
        .unwrap();

        assert_eq!(ticket.notes[0].kind, NoteKind::FeedbackNeeded);
        assert!(store.read_ticket(&id).unwrap().feedback_needed());
    }

    #[test]
    fn add_note_promotes_hs1_style_embedded_marker_without_a_colon() {
        let (_d, store) = store();
        let id = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
        create(
            &store,
            id,
            "HS",
            ts("2026-08-19T00:00:00Z"),
            NewTicket::default(),
        )
        .unwrap();

        let ticket = add_note(
            &store,
            &id,
            Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FB0").unwrap(),
            ts("2026-08-19T01:00:00Z"),
            NoteKind::Regular,
            "Context first. FEEDBACK NEEDED choose a layout".into(),
        )
        .unwrap();

        assert_eq!(ticket.notes[0].kind, NoteKind::FeedbackNeeded);
    }

    #[test]
    fn close_duplicate_requires_a_target() {
        let (_d, store) = store();
        let id = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
        create(&store, id, "HS", ts("t0"), NewTicket::default()).unwrap();
        assert!(matches!(
            close(&store, &id, ts("t1"), CloseReason::Duplicate, None),
            Err(OpError::DuplicateNeedsTarget)
        ));
    }

    #[test]
    fn closing_clears_up_next_and_drops_it_from_the_queue() {
        let (_d, store) = store();
        let id = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
        create(
            &store,
            id,
            "HS",
            ts("t0"),
            NewTicket {
                title: "queued work".into(),
                up_next: true,
                ..Default::default()
            },
        )
        .unwrap();
        let up_next_q = TicketQuery {
            up_next_only: true,
            ..Default::default()
        };
        assert_eq!(
            query(&store, &up_next_q).unwrap().len(),
            1,
            "queued to start"
        );

        // close() clears up_next even though it leaves the status field untouched.
        let c = close(&store, &id, ts("t1"), CloseReason::Completed, None).unwrap();
        assert!(!c.up_next, "closing clears up_next");
        assert!(
            query(&store, &up_next_q).unwrap().is_empty(),
            "a closed ticket is off the Up Next queue"
        );
    }

    #[test]
    fn moving_out_of_active_clears_up_next_but_started_keeps_it() {
        let (_d, store) = store();
        let id = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
        create(
            &store,
            id,
            "HS",
            ts("t0"),
            NewTicket {
                up_next: true,
                ..Default::default()
            },
        )
        .unwrap();

        // Moving to `started` (still active) keeps the ticket Up Next.
        let s = update(
            &store,
            &id,
            ts("t1"),
            TicketPatch {
                status: Some(Status::Started),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(s.up_next, "started is active → still Up Next");

        // Moving to any non-active status clears it — even if the same patch re-sets it.
        let done = update(
            &store,
            &id,
            ts("t2"),
            TicketPatch {
                status: Some(Status::Completed),
                up_next: Some(true), // ignored: leaving active always wins
                ..Default::default()
            },
        )
        .unwrap();
        assert!(!done.up_next, "completed is not active → up_next cleared");

        // A later attempt to queue an already-inactive ticket is normalized too, even
        // when the patch does not include a status transition.
        let still_done = update(
            &store,
            &id,
            ts("t3"),
            TicketPatch {
                up_next: Some(true),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(!still_done.up_next);
        assert!(
            query(
                &store,
                &TicketQuery {
                    up_next_only: true,
                    ..Default::default()
                }
            )
            .unwrap()
            .is_empty()
        );
    }

    fn store_pfx(prefix: &str) -> (tempfile::TempDir, FsStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = FsStore::init(dir.path(), &StoreMetadata::new(prefix)).unwrap();
        (dir, store)
    }

    #[test]
    fn copy_makes_a_fresh_ticket_in_the_destination() {
        let (_sd, src) = store_pfx("HS");
        let (_dd, dest) = store_pfx("SEC");
        let id = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
        create(
            &src,
            id,
            "HS",
            ts("t0"),
            NewTicket {
                title: "idea".into(),
                up_next: true,
                ..Default::default()
            },
        )
        .unwrap();
        // A started/claimed source ticket — the copy must reset that.
        update(
            &src,
            &id,
            ts("t1"),
            TicketPatch {
                status: Some(Status::Started),
                ..Default::default()
            },
        )
        .unwrap();
        let attachment_id = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FB0").unwrap();
        src.write_attachment(
            &id,
            attachment_id,
            ts("2026-08-26T00:00:00Z"),
            "proof.txt",
            b"proof",
        )
        .unwrap();

        let new_id = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FB7").unwrap();
        let copy = copy_ticket(&src, &dest, &id, new_id, ts("t2")).unwrap();

        assert_eq!(copy.id, new_id, "new ULID");
        assert!(copy.slug.starts_with("SEC-"), "destination prefix slug");
        assert_eq!(copy.copied_from, Some(id), "records provenance");
        assert_eq!(copy.title, "idea", "content carried over");
        assert_eq!(
            copy.status,
            Status::NotStarted,
            "fresh copy resets workflow"
        );
        assert!(!copy.up_next);
        // Source untouched (still present, still started).
        assert_eq!(src.read_ticket(&id).unwrap().status, Status::Started);
        // The copy is a real ticket in the destination.
        assert_eq!(dest.read_ticket(&new_id).unwrap().slug, copy.slug);
        assert_eq!(copy.attachments[0].id, attachment_id);
        assert_eq!(
            copy.attachments[0].created_at.as_str(),
            "2026-08-26T00:00:00Z"
        );
        assert_eq!(
            std::fs::read(
                dest.attachment_dir(&new_id)
                    .join(attachment_id.to_string())
                    .join("proof.txt")
            )
            .unwrap(),
            b"proof"
        );
    }

    #[test]
    fn move_keeps_the_ulid_and_leaves_a_tombstone() {
        let (_sd, src) = store_pfx("HS");
        let (_dd, dest) = store_pfx("SEC");
        let id = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
        create(
            &src,
            id,
            "HS",
            ts("t0"),
            NewTicket {
                title: "portable".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let attachment_id = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FB0").unwrap();
        src.write_attachment(
            &id,
            attachment_id,
            ts("2026-08-26T00:00:00Z"),
            "move.txt",
            b"move",
        )
        .unwrap();

        let out = move_ticket(&src, &dest, &id, "/stores/sec", ts("t3")).unwrap();

        // Destination: SAME ULID (references survive), destination slug, live.
        assert_eq!(out.moved.id, id);
        assert!(out.moved.slug.starts_with("SEC-"));
        assert_eq!(out.moved.status, Status::NotStarted);
        assert_eq!(dest.read_ticket(&id).unwrap().title, "portable");
        assert_eq!(out.moved.attachments[0].id, attachment_id);
        assert_eq!(
            out.moved.attachments[0].created_at.as_str(),
            "2026-08-26T00:00:00Z"
        );
        assert!(
            dest.attachment_dir(&id)
                .join(attachment_id.to_string())
                .join("move.txt")
                .is_file()
        );
        // Source: a moved tombstone pointing at the destination.
        let tomb = src.read_ticket(&id).unwrap();
        assert_eq!(tomb.status, Status::Moved);
        assert_eq!(tomb.moved_to_store.as_deref(), Some("/stores/sec"));
        assert!(tomb.moved_at.is_some());
        assert_eq!(out.tombstone.status, Status::Moved);
    }

    #[test]
    fn assign_sets_people_and_unions_review_requests() {
        use hotsheet_model::{ReviewKind, ReviewRequest};
        let (_d, store) = store();
        std::process::Command::new("git")
            .arg("-C")
            .arg(store.root())
            .arg("init")
            .output()
            .unwrap();
        std::process::Command::new("git")
            .arg("-C")
            .arg(store.root())
            .args(["config", "user.email", "requester@x.co"])
            .output()
            .unwrap();
        let id = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
        create(&store, id, "HS", ts("t0"), NewTicket::default()).unwrap();

        let review = |uid: &str| ReviewRequest {
            who: "dana@x.co".into(),
            kind: ReviewKind::Feedback,
            by: Ulid::from_string(uid).unwrap(),
            at: ts("t1"),
            requested_by: None,
        };
        // Assign two people + one review request.
        let t = assign(
            &store,
            &id,
            ts("t1"),
            Some(vec!["alex@x.co".into(), "sam@x.co".into()]),
            vec![review("01ARZ3NDEKTSV4RRFFQ69G5FB0")],
        )
        .unwrap();
        assert_eq!(t.assignees, vec!["alex@x.co", "sam@x.co"]);
        assert_eq!(t.review_requests.len(), 1);
        assert!(t.review_requests[0].requested_by.is_some());

        // Re-assign replaces assignees but ADDS a new review (deduped by `by`).
        let t = assign(
            &store,
            &id,
            ts("t2"),
            Some(vec!["alex@x.co".into()]),
            vec![
                review("01ARZ3NDEKTSV4RRFFQ69G5FB0"), // same `by` → not duplicated
                review("01ARZ3NDEKTSV4RRFFQ69G5FB1"), // new `by` → added
            ],
        )
        .unwrap();
        assert_eq!(t.assignees, vec!["alex@x.co"], "assignees replaced");
        assert_eq!(t.review_requests.len(), 2, "review requests union by `by`");

        let requested = query(
            &store,
            &TicketQuery {
                review_by: t.review_requests[0].requested_by.clone(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(requested.len(), 1, "requester facet is queryable");

        // Filter by assignee.
        let mine = query(
            &store,
            &TicketQuery {
                assignee: Some("alex@x.co".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(mine.len(), 1);
        let theirs = query(
            &store,
            &TicketQuery {
                assignee: Some("nobody@x.co".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(theirs.is_empty());
    }

    #[test]
    fn query_filters_by_close_reason_and_closed() {
        let (_d, store) = store();
        let a = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FB0").unwrap();
        let b = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FB1").unwrap();
        create(&store, a, "HS", ts("t0"), NewTicket::default()).unwrap();
        create(&store, b, "HS", ts("t0"), NewTicket::default()).unwrap();
        close(&store, &a, ts("t1"), CloseReason::Obsolete, None).unwrap();

        let ids = |q: &TicketQuery| {
            query(&store, q)
                .unwrap()
                .iter()
                .map(|t| t.id)
                .collect::<Vec<_>>()
        };
        // closed=true selects only the closed one; closed=false only the open one.
        assert_eq!(
            ids(&TicketQuery {
                closed: Some(true),
                ..Default::default()
            }),
            vec![a]
        );
        assert_eq!(
            ids(&TicketQuery {
                closed: Some(false),
                ..Default::default()
            }),
            vec![b]
        );
        // by specific reason.
        assert_eq!(
            ids(&TicketQuery {
                close_reason: Some(CloseReason::Obsolete),
                ..Default::default()
            }),
            vec![a]
        );
        assert!(
            ids(&TicketQuery {
                close_reason: Some(CloseReason::Completed),
                ..Default::default()
            })
            .is_empty()
        );
    }

    #[test]
    fn reopening_clears_the_close_annotation() {
        let (_d, store) = store();
        let id = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
        let dup = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FB9").unwrap();
        create(&store, id, "HS", ts("t0"), NewTicket::default()).unwrap();
        create(&store, dup, "HS", ts("t0"), NewTicket::default()).unwrap();

        let c = close(&store, &id, ts("t1"), CloseReason::Duplicate, Some(dup)).unwrap();
        assert!(c.close_reason.is_some() && c.closed_at.is_some() && c.duplicate_of.is_some());

        // Reopening (back to an active status) clears close_reason/closed_at/duplicate_of.
        let r = update(
            &store,
            &id,
            ts("t2"),
            TicketPatch {
                status: Some(Status::Started),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(r.status, Status::Started);
        assert!(r.close_reason.is_none(), "close_reason cleared on reopen");
        assert!(r.closed_at.is_none(), "closed_at cleared on reopen");
        assert!(r.duplicate_of.is_none(), "duplicate_of cleared on reopen");
    }

    /// The forward half of the invariant (HS2-3XHT9P): closing an **active** ticket
    /// settles its status to `completed`; closing an already-terminal ticket keeps the
    /// terminal status. Walks the state matrix rather than one op from a clean start.
    #[test]
    fn closing_settles_status_and_never_leaves_active_plus_close_reason() {
        let (_d, store) = store();
        let mk = |hex: &str| {
            let id = Ulid::from_string(hex).unwrap();
            create(&store, id, "HS", ts("t0"), NewTicket::default()).unwrap();
            id
        };

        // not_started --close--> completed (+ completed_at stamped).
        let a = mk("01ARZ3NDEKTSV4RRFFQ69G5A00");
        let ca = close(&store, &a, ts("t1"), CloseReason::Obsolete, None).unwrap();
        assert_eq!(ca.status, Status::Completed);
        assert_eq!(ca.close_reason, Some(CloseReason::Obsolete));
        assert!(ca.completed_at.is_some(), "completed_at stamped on close");

        // started --close--> completed.
        let b = mk("01ARZ3NDEKTSV4RRFFQ69G5B00");
        update(
            &store,
            &b,
            ts("t1"),
            TicketPatch {
                status: Some(Status::Started),
                ..Default::default()
            },
        )
        .unwrap();
        let cb = close(&store, &b, ts("t2"), CloseReason::Completed, None).unwrap();
        assert_eq!(cb.status, Status::Completed, "started settles to completed");

        // verified --close--> stays verified (an already-terminal status is NOT downgraded).
        let c = mk("01ARZ3NDEKTSV4RRFFQ69G5C00");
        update(
            &store,
            &c,
            ts("t1"),
            TicketPatch {
                status: Some(Status::Verified),
                ..Default::default()
            },
        )
        .unwrap();
        let cc = close(&store, &c, ts("t2"), CloseReason::Obsolete, None).unwrap();
        assert_eq!(cc.status, Status::Verified, "terminal status preserved");
        assert_eq!(cc.close_reason, Some(CloseReason::Obsolete));

        // Adversarial sequence: close → reopen (clears) → re-close → reopen. The active
        // state must NEVER coexist with a close_reason at any step.
        let d = mk("01ARZ3NDEKTSV4RRFFQ69G5D00");
        let steps: &[(Option<Status>, Option<CloseReason>)] = &[
            (None, Some(CloseReason::Completed)), // close
            (Some(Status::Started), None),        // reopen
            (None, Some(CloseReason::Obsolete)),  // re-close
            (Some(Status::NotStarted), None),     // reopen to not_started
        ];
        for (clock, (set_status, do_close)) in (1..).zip(steps.iter()) {
            let t = if let Some(s) = set_status {
                update(
                    &store,
                    &d,
                    ts(&format!("t{clock}")),
                    TicketPatch {
                        status: Some(*s),
                        ..Default::default()
                    },
                )
                .unwrap()
            } else {
                close(
                    &store,
                    &d,
                    ts(&format!("t{clock}")),
                    do_close.unwrap(),
                    None,
                )
                .unwrap()
            };
            assert!(
                !(t.status.is_active() && t.close_reason.is_some()),
                "invariant violated: status={:?} + close_reason={:?}",
                t.status,
                t.close_reason
            );
        }
    }

    #[test]
    fn claim_next_release_renew() {
        let (_d, store) = store();
        for (i, c) in ["01ARZ3NDEKTSV4RRFFQ69G5FB0", "01ARZ3NDEKTSV4RRFFQ69G5FB1"]
            .iter()
            .enumerate()
        {
            let mut nt = NewTicket {
                title: format!("t{i}"),
                category: "task".into(),
                ..Default::default()
            };
            if i == 1 {
                nt.up_next = true; // preferred
            }
            create(
                &store,
                Ulid::from_string(c).unwrap(),
                "HS",
                ts("2026-08-19T00:00:00Z"),
                nt,
            )
            .unwrap();
        }
        let now = ts("2026-08-19T00:10:00Z");
        let lease = ts("2026-08-19T00:40:00Z");
        let claimed = claim_next(&store, &now, lease.clone(), "w1", None)
            .unwrap()
            .unwrap();
        assert_eq!(claimed.title, "t1", "Up Next is preferred");
        assert_eq!(claimed.claimed_by.as_deref(), Some("w1"));
        assert_eq!(claimed.claim_count, 1);

        // wrong worker can't release/renew
        assert!(matches!(
            release(&store, &claimed.id, now.clone(), "w2", false),
            Err(OpError::WrongWorker { .. })
        ));
        assert!(matches!(
            renew(&store, &claimed.id, now.clone(), lease.clone(), "w2"),
            Err(OpError::WrongWorker { .. })
        ));

        // holder renews then releases
        renew(&store, &claimed.id, now.clone(), lease.clone(), "w1").unwrap();
        let released = release(&store, &claimed.id, now, "w1", false).unwrap();
        assert!(released.claimed_by.is_none());
    }

    #[test]
    fn exact_claim_is_eligible_holder_idempotent_and_expiry_takeover_safe() {
        let (_d, store) = store();
        let blocker = create(
            &store,
            Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FC0").unwrap(),
            "HS",
            ts("2026-08-19T00:00:00Z"),
            NewTicket {
                title: "blocker".into(),
                category: "task".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let exact = create(
            &store,
            Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FC1").unwrap(),
            "HS",
            ts("2026-08-19T00:00:00Z"),
            NewTicket {
                title: "exact".into(),
                category: "task".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let blocked = create(
            &store,
            Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FC2").unwrap(),
            "HS",
            ts("2026-08-19T00:00:00Z"),
            NewTicket {
                title: "blocked".into(),
                category: "task".into(),
                blocked_by: vec![blocker.id],
                ..Default::default()
            },
        )
        .unwrap();
        let now = ts("2026-08-19T00:10:00Z");
        let first = claim(
            &store,
            &exact.id,
            &now,
            ts("2026-08-19T00:40:00Z"),
            "w1",
            Some("Agent one".into()),
        )
        .unwrap();
        assert_eq!(
            first.status,
            Status::NotStarted,
            "claiming never changes durable status"
        );
        assert_eq!(first.claim_count, 1);

        let retry = claim(
            &store,
            &exact.id,
            &now,
            ts("2026-08-19T00:50:00Z"),
            "w1",
            None,
        )
        .unwrap();
        assert_eq!(
            retry.claim_count, 1,
            "same-holder retry is not a new attempt"
        );
        assert_eq!(retry.worker_label.as_deref(), Some("Agent one"));
        assert_eq!(
            retry.claim_lease_expires_at.as_ref().unwrap().as_str(),
            "2026-08-19T00:50:00Z"
        );
        assert!(matches!(
            claim(
                &store,
                &exact.id,
                &now,
                ts("2026-08-19T01:00:00Z"),
                "w2",
                None
            ),
            Err(OpError::WrongWorker { .. })
        ));

        let takeover = claim(
            &store,
            &exact.id,
            &ts("2026-08-19T00:51:00Z"),
            ts("2026-08-19T01:21:00Z"),
            "w2",
            Some("Agent two".into()),
        )
        .unwrap();
        assert_eq!(takeover.claimed_by.as_deref(), Some("w2"));
        assert_eq!(takeover.claim_count, 2);
        assert!(matches!(
            claim(
                &store,
                &blocked.id,
                &now,
                ts("2026-08-19T00:40:00Z"),
                "w1",
                None
            ),
            Err(OpError::ClaimUnavailable {
                reason: "ticket has unresolved blockers",
                ..
            })
        ));
        let completed = update(
            &store,
            &blocker.id,
            ts("2026-08-19T00:11:00Z"),
            TicketPatch {
                status: Some(Status::Completed),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(matches!(
            claim(
                &store,
                &completed.id,
                &now,
                ts("2026-08-19T00:40:00Z"),
                "w1",
                None
            ),
            Err(OpError::ClaimUnavailable {
                reason: "ticket is not open",
                ..
            })
        ));
    }
}
