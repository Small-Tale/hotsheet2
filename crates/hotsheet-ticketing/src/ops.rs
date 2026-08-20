//! Ticket operations over a store — the single implementation of query / create /
//! update / close / claim that every surface (CLI, server, MCP shim) calls, so
//! behavior can't drift between them (`docs/04-core-server-cli.md` §4.5).
//!
//! These are policy-free and synchronous. Wall-clock time is **injected** as a
//! [`Timestamp`] by the caller (which owns a clock), keeping this layer testable.

use std::cmp::Ordering;
use std::collections::HashSet;
use std::str::FromStr;

use hotsheet_model::{CloseReason, Priority, Status, Ticket, Timestamp, Ulid, derive_slug};

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
    #[error("a duplicate target is required when the close reason is `duplicate`")]
    DuplicateNeedsTarget,
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
    /// Case-insensitive substring across title, details, and note text.
    pub text: Option<String>,
    pub up_next_only: bool,
    /// Exclude terminal/hidden statuses (completed/verified/deleted/archive/moved).
    pub open_only: bool,
    pub sort: SortKey,
}

/// Run a query: read the store, filter, and sort.
pub fn query(store: &FsStore, q: &TicketQuery) -> Result<Vec<Ticket>, StoreError> {
    let mut tickets = store.list_tickets()?;
    let text = q.text.as_deref().map(str::to_lowercase);
    tickets.retain(|t| {
        q.status.is_none_or(|s| t.status == s)
            && q.priority.is_none_or(|p| t.priority == p)
            && q.category.as_deref().is_none_or(|c| t.category == c)
            && (!q.up_next_only || t.up_next)
            && (!q.open_only || is_open(t))
            && q.tags.iter().all(|tag| t.tags.iter().any(|x| x == tag))
            && text.as_deref().is_none_or(|needle| matches_text(t, needle))
    });
    sort_tickets(&mut tickets, q.sort);
    Ok(tickets)
}

/// Resolve a ticket by ULID (exact) or by slug (case-insensitive).
pub fn resolve(store: &FsStore, needle: &str) -> Result<Option<Ticket>, StoreError> {
    if let Ok(id) = Ulid::from_string(needle) {
        return match store.read_ticket(&id) {
            Ok(t) => Ok(Some(t)),
            Err(StoreError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
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
    t.title.to_lowercase().contains(needle_lower)
        || t.details.to_lowercase().contains(needle_lower)
        || t.notes
            .iter()
            .any(|n| n.text.to_lowercase().contains(needle_lower))
}

fn sort_tickets(tickets: &mut [Ticket], key: SortKey) {
    match key {
        SortKey::Id => tickets.sort_by(|a, b| a.id.cmp(&b.id)),
        SortKey::Created => {
            tickets.sort_by(|a, b| a.created_at.as_str().cmp(b.created_at.as_str()))
        }
        SortKey::Updated => {
            tickets.sort_by(|a, b| a.updated_at.as_str().cmp(b.updated_at.as_str()))
        }
        SortKey::Priority => tickets.sort_by_key(|t| priority_rank(t.priority)),
        SortKey::Status => tickets.sort_by_key(|t| t.status as u8),
        SortKey::Title => {
            tickets.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
        }
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
    pub details: String,
    pub tags: Vec<String>,
    pub up_next: bool,
}

/// Create + write a ticket with a caller-minted id, at time `now`.
pub fn create(
    store: &FsStore,
    id: Ulid,
    prefix: &str,
    now: Timestamp,
    new: NewTicket,
) -> Result<Ticket, StoreError> {
    let mut t = Ticket::new(
        id,
        derive_slug(&id, prefix),
        new.title,
        new.category,
        now.clone(),
        now,
    );
    t.priority = new.priority;
    t.details = new.details;
    t.tags = new.tags;
    t.up_next = new.up_next;
    store.write_ticket(&t)?;
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
    if let Some(s) = patch.status {
        t.status = s;
        match s {
            Status::Completed if t.completed_at.is_none() => t.completed_at = Some(now.clone()),
            Status::Verified if t.verified_at.is_none() => t.verified_at = Some(now.clone()),
            _ => {}
        }
    }
    t.updated_at = now;
    store.write_ticket(&t)?;
    Ok(t)
}

/// Record a close outcome (orthogonal to status; `docs/02` §2.6a).
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
    t.updated_at = now;
    store.write_ticket(&t)?;
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
    store.write_ticket(&t)?;
    Ok(Some(t))
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
    store.write_ticket(&t)?;
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
    store.write_ticket(&t)?;
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
}
