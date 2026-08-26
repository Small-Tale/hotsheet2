//! The **semantic 3-way merge** for ticket files (`docs/02` §2.7, HS2-18) — the core
//! logic behind the `hotsheet merge-driver` git driver. It merges two versions of one
//! ticket against their common ancestor **field by field**, so git never dumps
//! `<<<<<<<` markers over structured data:
//!
//! - **Scalars** (title/status/priority/…/close fields): if only one side changed a field
//!   vs. base, take that side; if both changed it, the side with the newer `updated_at`
//!   wins (last-writer-wins per field) — never a conflict.
//! - **Sets** (`tags`, `blocked_by`, `assignees`): 3-way **set union**, honoring a
//!   base-relative delete (an item removed on one side and untouched on the other stays
//!   removed).
//! - **Notes**: **union by note-ULID**, sorted by ULID (= chronological); a note edited on
//!   both sides resolves by its own newest timestamp.
//! - **Body** (`details` prose): if only one side changed it, take that side; if **both**
//!   changed it, the caller runs a plain text 3-way merge on just the body — the only place
//!   a human-visible conflict can still surface.
//!
//! This module is **pure** (no I/O): the effectful driver (read files, run the body text
//! merge, write the result) lives in `hotsheet-cli`. Kept pure so it is proptest-tested.

use std::collections::BTreeMap;

use hotsheet_model::{Attachment, Note, Ticket};

/// How the `details` body resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodyMerge {
    /// Only one side (or neither) changed the body — resolved to this text, no conflict.
    Resolved(String),
    /// Both sides changed the body — the caller must run a text 3-way merge on these.
    Conflict {
        base: String,
        ours: String,
        theirs: String,
    },
}

/// The result of merging one ticket: the merged ticket (with everything but a conflicting
/// body already resolved) plus how the body resolved.
#[derive(Debug, Clone)]
pub struct MergeOutcome {
    pub ticket: Ticket,
    pub body: BodyMerge,
}

impl MergeOutcome {
    /// Whether the body needs a human (both sides rewrote the prose).
    pub fn has_body_conflict(&self) -> bool {
        matches!(self.body, BodyMerge::Conflict { .. })
    }
}

/// Merge `ours` and `theirs` against their common ancestor `base` (`docs/02` §2.7). When a
/// `BodyMerge::Resolved` is returned the ticket's `details` already holds the merged body;
/// on `BodyMerge::Conflict` `details` is left as `ours` for the caller to overwrite with a
/// text-merged (marker-carrying) body.
pub fn merge_tickets(base: &Ticket, ours: &Ticket, theirs: &Ticket) -> MergeOutcome {
    // Ties (equal timestamps) resolve to `ours` — deterministic and stable.
    let ours_wins = ours.updated_at.as_str() >= theirs.updated_at.as_str();

    let mut m = ours.clone();

    // Identity (normally identical across all three — same file).
    m.id = pick3(&base.id, &ours.id, &theirs.id, ours_wins);
    m.slug = pick3(&base.slug, &ours.slug, &theirs.slug, ours_wins);
    m.schema = pick3(&base.schema, &ours.schema, &theirs.schema, ours_wins);

    // Scalars — last-writer-wins per field.
    m.title = pick3(&base.title, &ours.title, &theirs.title, ours_wins);
    m.category = pick3(&base.category, &ours.category, &theirs.category, ours_wins);
    m.priority = pick3(&base.priority, &ours.priority, &theirs.priority, ours_wins);
    m.status = pick3(&base.status, &ours.status, &theirs.status, ours_wins);
    m.up_next = pick3(&base.up_next, &ours.up_next, &theirs.up_next, ours_wins);
    m.blocked_reason = pick3(
        &base.blocked_reason,
        &ours.blocked_reason,
        &theirs.blocked_reason,
        ours_wins,
    );
    m.completed_at = pick3(
        &base.completed_at,
        &ours.completed_at,
        &theirs.completed_at,
        ours_wins,
    );
    m.verified_at = pick3(
        &base.verified_at,
        &ours.verified_at,
        &theirs.verified_at,
        ours_wins,
    );

    // Close outcome (all three move together on a close/reopen; still per-field safe).
    m.closed_at = pick3(
        &base.closed_at,
        &ours.closed_at,
        &theirs.closed_at,
        ours_wins,
    );
    m.close_reason = pick3(
        &base.close_reason,
        &ours.close_reason,
        &theirs.close_reason,
        ours_wins,
    );
    m.duplicate_of = pick3(
        &base.duplicate_of,
        &ours.duplicate_of,
        &theirs.duplicate_of,
        ours_wins,
    );

    // Coordination — lease-based and expiring, so newest simply wins (§2.7 note).
    m.claimed_by = pick3(
        &base.claimed_by,
        &ours.claimed_by,
        &theirs.claimed_by,
        ours_wins,
    );
    m.claim_lease_expires_at = pick3(
        &base.claim_lease_expires_at,
        &ours.claim_lease_expires_at,
        &theirs.claim_lease_expires_at,
        ours_wins,
    );
    m.worker_label = pick3(
        &base.worker_label,
        &ours.worker_label,
        &theirs.worker_label,
        ours_wins,
    );
    m.claim_count = pick3(
        &base.claim_count,
        &ours.claim_count,
        &theirs.claim_count,
        ours_wins,
    );

    // Tombstone / provenance scalars.
    m.moved_to_store = pick3(
        &base.moved_to_store,
        &ours.moved_to_store,
        &theirs.moved_to_store,
        ours_wins,
    );
    m.moved_at = pick3(&base.moved_at, &ours.moved_at, &theirs.moved_at, ours_wins);
    m.copied_from = pick3(
        &base.copied_from,
        &ours.copied_from,
        &theirs.copied_from,
        ours_wins,
    );
    m.transfer_operation_id = pick3(
        &base.transfer_operation_id,
        &ours.transfer_operation_id,
        &theirs.transfer_operation_id,
        ours_wins,
    );
    m.transferred_from = pick3(
        &base.transferred_from,
        &ours.transferred_from,
        &theirs.transferred_from,
        ours_wins,
    );

    // Timestamps: the merged ticket is as fresh as the newest input.
    m.created_at = pick3(
        &base.created_at,
        &ours.created_at,
        &theirs.created_at,
        ours_wins,
    );
    m.updated_at = if ours_wins {
        ours.updated_at.clone()
    } else {
        theirs.updated_at.clone()
    };

    // Sets — 3-way union with base-relative deletes honored.
    m.tags = union3(&base.tags, &ours.tags, &theirs.tags);
    m.blocked_by = union3(&base.blocked_by, &ours.blocked_by, &theirs.blocked_by);
    m.assignees = union3(&base.assignees, &ours.assignees, &theirs.assignees);

    // Review requests each carry their own ULID (`by`), so — like notes — they **union**:
    // two people adding a reviewer never conflict (docs/10 §10.2, HS2-20).
    m.review_requests = merge_reviews(&ours.review_requests, &theirs.review_requests);
    m.external = pick3(&base.external, &ours.external, &theirs.external, ours_wins);
    m.attachments = merge_attachments(&ours.attachments, &theirs.attachments, ours_wins);
    m.extra = pick3(&base.extra, &ours.extra, &theirs.extra, ours_wins);

    // Notes — union by id, newest-timestamp wins per id, sorted by id (chronological).
    m.notes = merge_notes(&ours.notes, &theirs.notes);

    // Body prose.
    let body = merge_body(&base.details, &ours.details, &theirs.details);
    if let BodyMerge::Resolved(text) = &body {
        m.details = text.clone();
    }

    MergeOutcome { ticket: m, body }
}

fn merge_attachments(
    ours: &[Attachment],
    theirs: &[Attachment],
    ours_wins: bool,
) -> Vec<Attachment> {
    let mut by_id: BTreeMap<hotsheet_model::Ulid, Attachment> = BTreeMap::new();
    for attachment in ours.iter().chain(theirs.iter()) {
        by_id
            .entry(attachment.id)
            .and_modify(|current| {
                let candidate_is_ours = ours.iter().any(|item| item == attachment);
                if candidate_is_ours == ours_wins {
                    *current = attachment.clone();
                }
            })
            .or_insert_with(|| attachment.clone());
    }
    let mut attachments: Vec<_> = by_id.into_values().collect();
    attachments.sort_by(|a, b| {
        a.created_at
            .chronological_cmp(&b.created_at)
            .unwrap_or_else(|| a.created_at.as_str().cmp(b.created_at.as_str()))
            .then(a.id.cmp(&b.id))
    });
    attachments
}

/// 3-way pick for a single scalar: unchanged side wins; both-changed → last-writer-wins.
fn pick3<T: PartialEq + Clone>(base: &T, ours: &T, theirs: &T, ours_wins: bool) -> T {
    if ours == theirs || theirs == base {
        ours.clone() // both agree, or only ours changed → ours
    } else if ours == base {
        theirs.clone() // only theirs changed
    } else if ours_wins {
        ours.clone() // both changed → newer updated_at wins
    } else {
        theirs.clone()
    }
}

/// 3-way set union preserving order (ours first, then theirs-only): an item is kept unless
/// it was present in `base` and removed on a side (a delete beats an untouched keep).
fn union3<T: Clone + PartialEq>(base: &[T], ours: &[T], theirs: &[T]) -> Vec<T> {
    let keep = |x: &T| {
        let removed_ours = base.contains(x) && !ours.contains(x);
        let removed_theirs = base.contains(x) && !theirs.contains(x);
        !removed_ours && !removed_theirs
    };
    let mut out: Vec<T> = Vec::new();
    for x in ours.iter().chain(theirs.iter()) {
        if keep(x) && !out.contains(x) {
            out.push(x.clone());
        }
    }
    out
}

/// Union notes by ULID; a note on both sides resolves by its newest edit timestamp. The
/// The final order is explicit creation time then ULID, including imported notes whose
/// provider-native ids may not encode creation time.
fn merge_notes(ours: &[Note], theirs: &[Note]) -> Vec<Note> {
    let mut by_id: BTreeMap<hotsheet_model::Ulid, Note> = BTreeMap::new();
    for n in ours.iter().chain(theirs.iter()) {
        by_id
            .entry(n.id)
            .and_modify(|cur| {
                if n.edited_at.as_str() > cur.edited_at.as_str() {
                    *cur = n.clone();
                }
            })
            .or_insert_with(|| n.clone());
    }
    let mut notes: Vec<_> = by_id.into_values().collect();
    notes.sort_by(|a, b| {
        a.created_at
            .chronological_cmp(&b.created_at)
            .unwrap_or_else(|| a.created_at.as_str().cmp(b.created_at.as_str()))
            .then(a.id.cmp(&b.id))
    });
    notes
}

/// Union review requests by their own ULID (`by`), keeping the newer timestamp on a
/// collision, and sort by that id (chronological) — the same clean union as notes.
fn merge_reviews(
    ours: &[hotsheet_model::ReviewRequest],
    theirs: &[hotsheet_model::ReviewRequest],
) -> Vec<hotsheet_model::ReviewRequest> {
    let mut by_id: BTreeMap<hotsheet_model::Ulid, hotsheet_model::ReviewRequest> = BTreeMap::new();
    for r in ours.iter().chain(theirs.iter()) {
        by_id
            .entry(r.by)
            .and_modify(|cur| {
                if r.at.as_str() > cur.at.as_str() {
                    *cur = r.clone();
                }
            })
            .or_insert_with(|| r.clone());
    }
    by_id.into_values().collect()
}

/// Decide the body: take the changed side, or flag a conflict when both changed it.
fn merge_body(base: &str, ours: &str, theirs: &str) -> BodyMerge {
    if ours == theirs || theirs == base {
        BodyMerge::Resolved(ours.to_string())
    } else if ours == base {
        BodyMerge::Resolved(theirs.to_string())
    } else {
        BodyMerge::Conflict {
            base: base.to_string(),
            ours: ours.to_string(),
            theirs: theirs.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hotsheet_model::{Priority, Status, Ticket, Timestamp, Ulid};

    fn ulid(s: &str) -> Ulid {
        Ulid::from_string(s).unwrap()
    }
    fn ts(s: &str) -> Timestamp {
        Timestamp::from(s)
    }

    fn base_ticket() -> Ticket {
        let mut t = Ticket::new(
            ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV"),
            "HS-1",
            "title",
            "bug",
            ts("2026-08-19T00:00:00Z"),
            ts("2026-08-19T00:00:00Z"),
        );
        t.details = "original body".into();
        t.tags = vec!["a".into(), "b".into()];
        t
    }

    #[test]
    fn only_one_side_changed_a_scalar_takes_that_side() {
        let base = base_ticket();
        let mut ours = base.clone();
        ours.status = Status::Started;
        ours.updated_at = ts("2026-08-19T01:00:00Z");
        let theirs = base.clone(); // untouched
        let m = merge_tickets(&base, &ours, &theirs).ticket;
        assert_eq!(m.status, Status::Started);
    }

    #[test]
    fn both_changed_a_scalar_newer_updated_at_wins() {
        let base = base_ticket();
        let mut ours = base.clone();
        ours.priority = Priority::High;
        ours.updated_at = ts("2026-08-19T01:00:00Z");
        let mut theirs = base.clone();
        theirs.priority = Priority::Low;
        theirs.updated_at = ts("2026-08-19T02:00:00Z"); // newer
        let m = merge_tickets(&base, &ours, &theirs).ticket;
        assert_eq!(m.priority, Priority::Low, "the newer side wins");
        assert_eq!(m.updated_at.as_str(), "2026-08-19T02:00:00Z");
    }

    #[test]
    fn tags_union_with_base_relative_delete() {
        let base = base_ticket(); // tags [a, b]
        let mut ours = base.clone();
        ours.tags = vec!["a".into(), "b".into(), "c".into()]; // added c
        let mut theirs = base.clone();
        theirs.tags = vec!["b".into()]; // removed a
        let m = merge_tickets(&base, &ours, &theirs).ticket;
        // a removed on theirs (untouched on ours) → gone; b kept; c added.
        assert_eq!(m.tags, vec!["b".to_string(), "c".to_string()]);
    }

    #[test]
    fn notes_union_by_id_sorted() {
        let base = base_ticket();
        let mut ours = base.clone();
        ours.notes = vec![Note {
            id: ulid("01ARZ3NDEKTSV4RRFFQ69G5FB0"),
            kind: hotsheet_model::NoteKind::Regular,
            created_at: ts("2026-08-19T01:00:00Z"),
            edited_at: ts("2026-08-19T01:00:00Z"),
            text: "ours note".into(),
        }];
        let mut theirs = base.clone();
        theirs.notes = vec![Note {
            id: ulid("01ARZ3NDEKTSV4RRFFQ69G5FB1"),
            kind: hotsheet_model::NoteKind::Regular,
            created_at: ts("2026-08-19T02:00:00Z"),
            edited_at: ts("2026-08-19T02:00:00Z"),
            text: "theirs note".into(),
        }];
        let m = merge_tickets(&base, &ours, &theirs).ticket;
        assert_eq!(m.notes.len(), 2, "both appends kept");
        assert!(m.notes[0].id < m.notes[1].id, "sorted by id");
    }

    #[test]
    fn concurrent_note_edits_keep_the_latest_edit_and_original_creation() {
        let mut base = base_ticket();
        let id = ulid("01ARZ3NDEKTSV4RRFFQ69G5FB0");
        base.notes = vec![Note {
            id,
            kind: hotsheet_model::NoteKind::Activity,
            created_at: ts("2026-08-19T01:00:00Z"),
            edited_at: ts("2026-08-19T01:00:00Z"),
            text: "started".into(),
        }];
        let mut ours = base.clone();
        ours.notes[0].edited_at = ts("2026-08-19T02:00:00Z");
        ours.notes[0].text = "ours".into();
        let mut theirs = base.clone();
        theirs.notes[0].edited_at = ts("2026-08-19T03:00:00Z");
        theirs.notes[0].text = "theirs".into();
        let merged = merge_tickets(&base, &ours, &theirs).ticket;
        assert_eq!(merged.notes[0].text, "theirs");
        assert_eq!(merged.notes[0].created_at.as_str(), "2026-08-19T01:00:00Z");
        assert_eq!(merged.notes[0].edited_at.as_str(), "2026-08-19T03:00:00Z");
    }

    #[test]
    fn concurrent_attachment_additions_union_and_same_id_rename_uses_newer_side() {
        let base = base_ticket();
        let shared_id = ulid("01ARZ3NDEKTSV4RRFFQ69G5FB0");
        let mut ours = base.clone();
        ours.updated_at = ts("2026-08-19T02:00:00Z");
        ours.attachments = vec![Attachment {
            id: shared_id,
            filename: "ours.png".into(),
            created_at: ts("2026-08-19T01:00:00Z"),
        }];
        let mut theirs = base.clone();
        theirs.updated_at = ts("2026-08-19T03:00:00Z");
        theirs.attachments = vec![
            Attachment {
                id: shared_id,
                filename: "renamed.png".into(),
                created_at: ts("2026-08-19T01:00:00Z"),
            },
            Attachment {
                id: ulid("01ARZ3NDEKTSV4RRFFQ69G5FB1"),
                filename: "theirs.txt".into(),
                created_at: ts("2026-08-19T02:00:00Z"),
            },
        ];
        let merged = merge_tickets(&base, &ours, &theirs).ticket;
        assert_eq!(merged.attachments.len(), 2);
        assert_eq!(merged.attachments[0].filename, "renamed.png");
        assert_eq!(
            merged.attachments[0].created_at.as_str(),
            "2026-08-19T01:00:00Z"
        );
    }

    #[test]
    fn body_conflict_only_when_both_change_it() {
        let base = base_ticket();
        // one side changes body → resolved
        let mut ours = base.clone();
        ours.details = "ours body".into();
        let one = merge_tickets(&base, &ours, &base.clone());
        assert_eq!(one.body, BodyMerge::Resolved("ours body".into()));
        assert!(!one.has_body_conflict());

        // both change body → conflict, and the caller gets all three versions
        let mut theirs = base.clone();
        theirs.details = "theirs body".into();
        let two = merge_tickets(&base, &ours, &theirs);
        assert!(two.has_body_conflict());
    }

    #[test]
    fn review_requests_union_by_their_own_id() {
        use hotsheet_model::{ReviewKind, ReviewRequest};
        let review = |uid: &str, at: &str| ReviewRequest {
            who: "dana@x.co".into(),
            kind: ReviewKind::Review,
            by: ulid(uid),
            at: ts(at),
            requested_by: None,
        };
        let base = base_ticket();
        let mut ours = base.clone();
        ours.review_requests = vec![review("01ARZ3NDEKTSV4RRFFQ69G5FB0", "2026-08-19T01:00:00Z")];
        let mut theirs = base.clone();
        theirs.review_requests = vec![review("01ARZ3NDEKTSV4RRFFQ69G5FB1", "2026-08-19T02:00:00Z")];
        // Two people each add a reviewer → both kept, no conflict (union by `by`).
        let m = merge_tickets(&base, &ours, &theirs).ticket;
        assert_eq!(m.review_requests.len(), 2);
        assert!(
            m.review_requests[0].by < m.review_requests[1].by,
            "sorted by id"
        );
    }

    #[test]
    fn merging_identical_sides_is_idempotent() {
        let base = base_ticket();
        let mut x = base.clone();
        x.status = Status::Completed;
        x.tags = vec!["a".into(), "z".into()];
        x.updated_at = ts("2026-08-19T03:00:00Z");
        // merge(base, x, x) == x (no spurious changes)
        let m = merge_tickets(&base, &x, &x).ticket;
        assert_eq!(m.status, x.status);
        assert_eq!(m.tags, x.tags);
        assert_eq!(m.details, x.details);
    }
}
