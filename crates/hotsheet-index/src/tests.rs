use std::collections::HashSet;

use hotsheet_model::{Priority, Status, Timestamp, Ulid};
use hotsheet_ticketing::{
    FsStore, NewTicket, StoreMetadata, TicketCollection, TicketPatch, TicketQuery, ops,
};

use super::*;

fn ulid(s: &str) -> Ulid {
    Ulid::from_string(s).unwrap()
}

/// A store with three tickets + a rebuilt index over it.
fn seeded() -> (tempfile::TempDir, FsStore, Index) {
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    let now = Timestamp::new("2026-08-19T00:00:00Z");

    ops::create(
        &store,
        ulid("01ARZ3NDEKTSV4RRFFQ69G5FB0"),
        "HS",
        now.clone(),
        NewTicket {
            title: "Dashboard flicker".into(),
            category: "bug".into(),
            priority: Priority::High,
            tags: vec!["ui".into()],
            up_next: true,
            ..Default::default()
        },
    )
    .unwrap();
    ops::create(
        &store,
        ulid("01ARZ3NDEKTSV4RRFFQ69G5FB1"),
        "HS",
        now.clone(),
        NewTicket {
            title: "API pagination".into(),
            category: "feature".into(),
            tags: vec!["api".into()],
            ..Default::default()
        },
    )
    .unwrap();
    let c = ops::create(
        &store,
        ulid("01ARZ3NDEKTSV4RRFFQ69G5FB2"),
        "HS",
        now.clone(),
        NewTicket {
            title: "Old thing".into(),
            category: "task".into(),
            ..Default::default()
        },
    )
    .unwrap();
    ops::update(
        &store,
        &c.id,
        now,
        TicketPatch {
            status: Some(Status::Completed),
            ..Default::default()
        },
    )
    .unwrap();

    let index = Index::open_in_memory("s1").unwrap();
    index.rebuild_from_store(&store).unwrap();
    (dir, store, index)
}

fn index_ids(rows: &[TicketRow]) -> HashSet<String> {
    rows.iter().map(|r| r.id.clone()).collect()
}
fn ops_ids(store: &FsStore, q: &TicketQuery) -> HashSet<String> {
    ops::query(store, q)
        .unwrap()
        .into_iter()
        .map(|t| t.id.to_string())
        .collect()
}

#[test]
fn rebuild_indexes_every_ticket() {
    let (_d, _s, ix) = seeded();
    assert_eq!(ix.query(&TicketQuery::default()).unwrap().len(), 3);
}

#[test]
fn ticket_count_tracks_every_indexed_row_through_upsert_delete_and_rebuild() {
    let (_d, store, ix) = seeded();
    // Completed tickets count too: the count mirrors the full store walk.
    assert_eq!(ix.ticket_count().unwrap(), 3);
    let first = ulid("01ARZ3NDEKTSV4RRFFQ69G5FB0");
    ix.delete(&first).unwrap();
    assert_eq!(ix.ticket_count().unwrap(), 2);
    // Deleting an absent row is a no-op; a rebuild restores the file-backed population.
    ix.delete(&first).unwrap();
    assert_eq!(ix.ticket_count().unwrap(), 2);
    ix.rebuild_from_store(&store).unwrap();
    assert_eq!(ix.ticket_count().unwrap(), 3);
    // An empty index counts zero.
    let other = Index::open_in_memory("s2").unwrap();
    assert_eq!(other.ticket_count().unwrap(), 0);
}

#[test]
fn reopened_ticket_projects_cleared_lifecycle_timestamps() {
    let (_d, store, ix) = seeded();
    let id = ulid("01ARZ3NDEKTSV4RRFFQ69G5FB2");
    let reopened = ops::update(
        &store,
        &id,
        Timestamp::new("2026-08-19T02:00:00Z"),
        TicketPatch {
            status: Some(Status::NotStarted),
            ..Default::default()
        },
    )
    .unwrap();
    ix.upsert(&reopened, "reopened.md", "reopened").unwrap();

    let rows = ix
        .query(&TicketQuery {
            status: Some(Status::NotStarted),
            ..Default::default()
        })
        .unwrap();
    let row = rows.iter().find(|row| row.id == id.to_string()).unwrap();
    assert_eq!(row.completed_at, None);
    assert_eq!(row.verified_at, None);
}

#[test]
fn summary_aggregates_navigation_counts_without_loading_rows() {
    let (_d, _s, ix) = seeded();
    let summary = ix
        .summary(
            "2026-08-19T12:00:00Z",
            &[
                "2026-08-18T00:00:00Z".into(),
                "2026-08-19T00:00:00Z".into(),
                "2026-08-20T00:00:00Z".into(),
            ],
        )
        .unwrap();
    assert_eq!(summary.total, 3);
    assert_eq!(summary.queued, 3);
    assert_eq!(summary.backlog, 0);
    assert_eq!(summary.archive, 0);
    assert_eq!(summary.open, 2);
    assert_eq!(summary.up_next, 1);
    assert_eq!(summary.active, 0);
    assert_eq!(summary.started, 0);
    assert_eq!(summary.verified, 0);
    assert_eq!(summary.completed_today, 1);
    assert_eq!(summary.completion_trend, vec![0, 1]);
}

#[test]
fn duplicate_backlinks_follow_retarget_reopen_delete_and_refill() {
    let (_dir, store, ix) = seeded();
    let target = "01ARZ3NDEKTSV4RRFFQ69G5FB1";
    let qualified = format!("@project/source:{target}");
    let other = format!("@other/source:{target}");
    let mut ticket = store
        .read_ticket(&ulid("01ARZ3NDEKTSV4RRFFQ69G5FB0"))
        .unwrap();
    let save = |ticket: &Ticket| ix.upsert(ticket, "ticket.md", "hash").unwrap();
    assert!(
        ix.duplicate_backlinks(&qualified, target)
            .unwrap()
            .is_empty()
    );
    ticket.close_reason = Some(hotsheet_model::CloseReason::Duplicate);
    ticket.duplicate_of = Some(qualified.clone());
    save(&ticket);
    save(&ticket);
    assert_eq!(ix.duplicate_backlinks(&qualified, target).unwrap().len(), 1);
    assert!(ix.duplicate_backlinks(&other, target).unwrap().is_empty());
    ticket.duplicate_of = Some(other.clone());
    ticket.title = "Retargeted and renamed".into();
    save(&ticket);
    assert!(
        ix.duplicate_backlinks(&qualified, target)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        ix.duplicate_backlinks(&other, target).unwrap()[0].title,
        ticket.title
    );
    ticket.duplicate_of = Some(target.into());
    save(&ticket);
    assert_eq!(ix.duplicate_backlinks(&qualified, target).unwrap().len(), 1);
    assert_eq!(ix.duplicate_backlinks(&other, target).unwrap().len(), 1);
    ticket.close_reason = None;
    save(&ticket);
    assert!(
        ix.duplicate_backlinks(&qualified, target)
            .unwrap()
            .is_empty()
    );
    ticket.close_reason = Some(hotsheet_model::CloseReason::Duplicate);
    save(&ticket);
    ix.delete(&ticket.id).unwrap();
    assert!(
        ix.duplicate_backlinks(&qualified, target)
            .unwrap()
            .is_empty()
    );
    save(&ticket);
    assert_eq!(
        ix.duplicate_backlinks(&qualified, target).unwrap()[0].id,
        ticket.id.to_string()
    );

    let plan: String = ix.conn.query_row(
        "EXPLAIN QUERY PLAN SELECT id, slug, title FROM tickets WHERE store_id=?1 AND close_reason='duplicate' AND duplicate_of IN (?2,?3)",
        params![ix.store_id, qualified, target], |row| row.get(3),
    ).unwrap();
    assert!(plan.contains("idx_tickets_duplicate_target"), "{plan}");
}

#[test]
fn duplicate_backlinks_reconcile_external_changes_and_restore_after_schema_upgrade() {
    let (_dir, store, _) = seeded();
    let db_dir = tempfile::tempdir().unwrap();
    let db = db_dir.path().join("backlinks.sqlite");
    let id = ulid("01ARZ3NDEKTSV4RRFFQ69G5FB0");
    let target = "01ARZ3NDEKTSV4RRFFQ69G5FB1";
    let qualified = format!("@project/source:{target}");
    let ix = Index::open_reconciled(&db, &store).unwrap();
    assert!(
        ix.duplicate_backlinks(&qualified, target)
            .unwrap()
            .is_empty()
    );
    ops::close(
        &store,
        &id,
        Timestamp::new("2026-09-22T00:00:00Z"),
        hotsheet_model::CloseReason::Duplicate,
        Some(qualified.clone()),
    )
    .unwrap();
    ix.reconcile(&store).unwrap();
    assert_eq!(ix.duplicate_backlinks(&qualified, target).unwrap().len(), 1);
    ix.conn
        .execute(
            "UPDATE index_meta SET value='14' WHERE key='schema_version'",
            [],
        )
        .unwrap();
    drop(ix);
    let restored = Index::open_reconciled(&db, &store).unwrap();
    assert_eq!(
        restored
            .duplicate_backlinks(&qualified, target)
            .unwrap()
            .len(),
        1
    );
    std::fs::remove_file(store.ticket_path(&id)).unwrap();
    restored.reconcile(&store).unwrap();
    assert!(
        restored
            .duplicate_backlinks(&qualified, target)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn trash_is_counted_and_queried_separately_from_archive() {
    let (_d, store, _) = seeded();
    let now = Timestamp::new("2026-08-20T00:00:00Z");
    for (id, status) in [
        ("01ARZ3NDEKTSV4RRFFQ69G5FB0", Status::Archive),
        ("01ARZ3NDEKTSV4RRFFQ69G5FB1", Status::Deleted),
        ("01ARZ3NDEKTSV4RRFFQ69G5FB2", Status::Moved),
    ] {
        ops::update(
            &store,
            &ulid(id),
            now.clone(),
            TicketPatch {
                status: Some(status),
                ..Default::default()
            },
        )
        .unwrap();
    }
    let ix = Index::open_in_memory("s1").unwrap();
    ix.rebuild_from_store(&store).unwrap();

    let summary = ix.summary(now.as_str(), &[]).unwrap();
    assert_eq!(
        (
            summary.total,
            summary.queued,
            summary.archive,
            summary.trash
        ),
        (2, 0, 2, 1)
    );
    for (collection, expected) in [
        (
            TicketCollection::Archive,
            HashSet::from([
                "01ARZ3NDEKTSV4RRFFQ69G5FB0".to_string(),
                "01ARZ3NDEKTSV4RRFFQ69G5FB2".to_string(),
            ]),
        ),
        (
            TicketCollection::Trash,
            HashSet::from(["01ARZ3NDEKTSV4RRFFQ69G5FB1".to_string()]),
        ),
    ] {
        let q = TicketQuery {
            collection: Some(collection),
            ..Default::default()
        };
        assert_eq!(
            index_ids(&ix.query(&q).unwrap()),
            expected,
            "{collection:?}"
        );
        assert_eq!(ops_ids(&store, &q), expected, "{collection:?}");
    }
}

#[test]
fn structured_filters_match_the_file_scan() {
    let (_d, store, ix) = seeded();
    for q in [
        TicketQuery {
            status: Some(Status::Completed),
            ..Default::default()
        },
        TicketQuery {
            priority: Some(Priority::High),
            ..Default::default()
        },
        TicketQuery {
            category: Some("feature".into()),
            ..Default::default()
        },
        TicketQuery {
            tags: vec!["ui".into()],
            ..Default::default()
        },
        TicketQuery {
            up_next_only: true,
            ..Default::default()
        },
        TicketQuery {
            open_only: true,
            ..Default::default()
        },
        TicketQuery {
            collection: Some(TicketCollection::Queue),
            ..Default::default()
        },
        TicketQuery {
            collection: Some(TicketCollection::Archive),
            ..Default::default()
        },
        TicketQuery {
            collection: Some(TicketCollection::Trash),
            ..Default::default()
        },
        // A cap must pick the same rows on both paths (both order by id, then cap).
        TicketQuery {
            limit: Some(2),
            ..Default::default()
        },
    ] {
        assert_eq!(
            index_ids(&ix.query(&q).unwrap()),
            ops_ids(&store, &q),
            "index diverged from ops::query for {q:?}"
        );
    }
}

#[test]
fn limit_caps_the_sql_result() {
    let (_d, _s, ix) = seeded();
    assert_eq!(
        ix.query(&TicketQuery {
            limit: Some(2),
            ..Default::default()
        })
        .unwrap()
        .len(),
        2
    );
    // A limit over the row count is a no-op.
    assert_eq!(
        ix.query(&TicketQuery {
            limit: Some(50),
            ..Default::default()
        })
        .unwrap()
        .len(),
        3
    );
}

#[test]
fn keyset_paging_matches_the_file_scan_and_is_exclusive() {
    let (_d, store, ix) = seeded();
    // Ids FB0 < FB1 < FB2 (default sort = id). Paging after FB0 yields FB1, FB2 — in order.
    let after_first = TicketQuery {
        page_after: Some(ulid("01ARZ3NDEKTSV4RRFFQ69G5FB0")),
        ..Default::default()
    };
    let ordered = |rows: &[TicketRow]| rows.iter().map(|r| r.id.clone()).collect::<Vec<_>>();
    let idx_rows = ix.query(&after_first).unwrap();
    assert_eq!(
        ordered(&idx_rows),
        vec![
            "01ARZ3NDEKTSV4RRFFQ69G5FB1".to_string(),
            "01ARZ3NDEKTSV4RRFFQ69G5FB2".to_string(),
        ]
    );
    // Same ordered result on the file-scan path (keyset parity, not just filter parity).
    let ops_rows: Vec<String> = ops::query(&store, &after_first)
        .unwrap()
        .into_iter()
        .map(|t| t.id.to_string())
        .collect();
    assert_eq!(ordered(&idx_rows), ops_rows);

    // Exclusive: after the last id → empty on both paths.
    let after_last = TicketQuery {
        page_after: Some(ulid("01ARZ3NDEKTSV4RRFFQ69G5FB2")),
        ..Default::default()
    };
    assert!(ix.query(&after_last).unwrap().is_empty());
    assert!(ops::query(&store, &after_last).unwrap().is_empty());

    // A stale cursor (not in the store) → empty page, never the whole list, on both paths.
    let stale = TicketQuery {
        page_after: Some(ulid("01ARZ3NDEKTSV4RRFFQ69G5FBZ")),
        ..Default::default()
    };
    assert!(ix.query(&stale).unwrap().is_empty());
    assert!(ops::query(&store, &stale).unwrap().is_empty());

    // Descending pages use the inverse keyset comparison and remain index/file-scan equivalent.
    let descending_after_first = TicketQuery {
        descending: true,
        page_after: Some(ulid("01ARZ3NDEKTSV4RRFFQ69G5FB2")),
        ..Default::default()
    };
    let idx_rows = ix.query(&descending_after_first).unwrap();
    assert_eq!(
        ordered(&idx_rows),
        vec![
            "01ARZ3NDEKTSV4RRFFQ69G5FB1".to_string(),
            "01ARZ3NDEKTSV4RRFFQ69G5FB0".to_string(),
        ]
    );
    assert_eq!(
        ordered(&idx_rows),
        ops::query(&store, &descending_after_first)
            .unwrap()
            .into_iter()
            .map(|ticket| ticket.id.to_string())
            .collect::<Vec<_>>()
    );

    // Workspace sorts use a mixed total order: directed primary key, recent-first
    // secondary key, then ascending id. Its descending keyset must walk that same order.
    let priority_descending = TicketQuery {
        sort: SortKey::Priority,
        descending: true,
        ..Default::default()
    };
    let full = ix.query(&priority_descending).unwrap();
    assert_eq!(
        ordered(&full),
        ops::query(&store, &priority_descending)
            .unwrap()
            .into_iter()
            .map(|ticket| ticket.id.to_string())
            .collect::<Vec<_>>()
    );
    let after_priority_first = TicketQuery {
        page_after: Some(ulid(&full[0].id)),
        ..priority_descending
    };
    assert_eq!(
        ordered(&ix.query(&after_priority_first).unwrap()),
        ordered(&full[1..])
    );

    // The mixed-order cursor predicate remains grouped with structured filters; its OR
    // branches must not leak rows from another status into later pages.
    let open_after_priority_first = TicketQuery {
        status: Some(Status::NotStarted),
        sort: SortKey::Priority,
        descending: true,
        page_after: Some(ulid("01ARZ3NDEKTSV4RRFFQ69G5FB1")),
        ..Default::default()
    };
    let filtered = ix.query(&open_after_priority_first).unwrap();
    assert_eq!(
        ordered(&filtered),
        vec!["01ARZ3NDEKTSV4RRFFQ69G5FB0".to_string()]
    );
    assert_eq!(
        ordered(&filtered),
        ops::query(&store, &open_after_priority_first)
            .unwrap()
            .into_iter()
            .map(|ticket| ticket.id.to_string())
            .collect::<Vec<_>>()
    );
}

/// A fixture exercising blocked/review/moved/date filters, + the index-vs-file-scan parity.
#[test]
fn blocked_review_moved_and_date_filters_match_the_file_scan() {
    use hotsheet_model::{ReviewKind, ReviewRequest};

    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    let early = Timestamp::new("2026-08-10T00:00:00Z");
    let late = Timestamp::new("2026-08-20T00:00:00Z");
    let mk = |id: &str, ts: &Timestamp, blockers: Vec<Ulid>| {
        ops::create(
            &store,
            ulid(id),
            "HS",
            ts.clone(),
            NewTicket {
                title: format!("t-{id}"),
                blocked_by: blockers,
                ..Default::default()
            },
        )
        .unwrap()
    };
    let a = mk("01ARZ3NDEKTSV4RRFFQ69G5FB0", &early, vec![]); // open blocker
    let b = mk("01ARZ3NDEKTSV4RRFFQ69G5FB1", &early, vec![a.id]);
    let c = mk("01ARZ3NDEKTSV4RRFFQ69G5FB2", &late, vec![]);
    let _d = mk("01ARZ3NDEKTSV4RRFFQ69G5FB3", &late, vec![c.id]);
    let m = mk("01ARZ3NDEKTSV4RRFFQ69G5FB4", &late, vec![]);
    let r = mk("01ARZ3NDEKTSV4RRFFQ69G5FB5", &late, vec![]);

    // B has an explanatory reason and is blocked. D has only a dependency edge and is not.
    ops::update(
        &store,
        &b.id,
        early.clone(),
        TicketPatch {
            blocked_reason: Some(Some("Waiting for A".into())),
            ..Default::default()
        },
    )
    .unwrap();
    // C is done; M is a moved tombstone.
    ops::update(
        &store,
        &c.id,
        late.clone(),
        TicketPatch {
            status: Some(Status::Completed),
            ..Default::default()
        },
    )
    .unwrap();
    ops::update(
        &store,
        &m.id,
        late.clone(),
        TicketPatch {
            status: Some(Status::Moved),
            ..Default::default()
        },
    )
    .unwrap();
    // R carries a review request for alice.
    ops::assign(
        &store,
        &r.id,
        late.clone(),
        None,
        vec![ReviewRequest {
            who: "alice@example.com".into(),
            kind: ReviewKind::Review,
            by: ulid("01ARZ3NDEKTSV4RRFFQ69G5FB9"),
            at: late.clone(),
            requested_by: Some("requester@example.com".into()),
        }],
    )
    .unwrap();

    let ix = Index::open_in_memory("s1").unwrap();
    ix.rebuild_from_store(&store).unwrap();

    for q in [
        TicketQuery {
            blocked: Some(true),
            ..Default::default()
        },
        TicketQuery {
            blocked: Some(false),
            ..Default::default()
        },
        TicketQuery {
            review_requested: Some("alice@example.com".into()),
            ..Default::default()
        },
        TicketQuery {
            review_by: Some("requester@example.com".into()),
            ..Default::default()
        },
        // Default list excludes the moved tombstone …
        TicketQuery::default(),
        // … but an explicit status=moved surfaces it.
        TicketQuery {
            status: Some(Status::Moved),
            ..Default::default()
        },
        TicketQuery {
            created_after: Some("2026-08-15T00:00:00Z".into()),
            ..Default::default()
        },
        TicketQuery {
            created_before: Some("2026-08-15T00:00:00Z".into()),
            ..Default::default()
        },
    ] {
        assert_eq!(
            index_ids(&ix.query(&q).unwrap()),
            ops_ids(&store, &q),
            "index diverged from ops::query for {q:?}"
        );
    }

    // Spot-check the semantics (not just parity): B's reason blocks, D's bare edge does not.
    let blocked = ops_ids(
        &store,
        &TicketQuery {
            blocked: Some(true),
            ..Default::default()
        },
    );
    assert!(
        blocked.contains("01ARZ3NDEKTSV4RRFFQ69G5FB1"),
        "B blocked by its visible reason"
    );
    assert!(
        !blocked.contains("01ARZ3NDEKTSV4RRFFQ69G5FB3"),
        "D's dependency edge alone does not create a hidden block"
    );
    let all = index_ids(&ix.query(&TicketQuery::default()).unwrap());
    assert!(
        !all.contains("01ARZ3NDEKTSV4RRFFQ69G5FB4"),
        "moved tombstone hidden by default"
    );
}

#[test]
fn fts_matches_prefixes_across_identity_and_content() {
    let (_d, store, ix) = seeded();
    let first = ulid("01ARZ3NDEKTSV4RRFFQ69G5FB0");
    let updated = ops::add_note(
        &store,
        &first,
        ulid("01ARZ3NDEKTSV4RRFFQ69G5FC0"),
        Timestamp::new("2026-08-19T00:01:00Z"),
        hotsheet_model::NoteKind::Regular,
        "Mentions HS2-QQRY00 in a note".into(),
    )
    .unwrap();
    ix.upsert(&updated, "first.md", "updated").unwrap();

    for (text, expected_title) in [
        ("flick", "Dashboard flicker"),
        (&updated.slug, "Dashboard flicker"),
        ("ui", "Dashboard flicker"),
        ("QQRY00", "Dashboard flicker"),
    ] {
        let rows = ix
            .query(&TicketQuery {
                text: Some(text.into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(rows.len(), 1, "query {text}");
        assert_eq!(rows[0].title, expected_title);
    }
}

#[test]
fn fts_and_resolution_find_a_retained_legacy_number() {
    // HS2-4H2ZR1: an imported ticket keeps its HS1 number so old references (in text or a
    // navigate-to-ref) resolve/search against the new ticket.
    let (_d, store, ix) = seeded();
    let id = ulid("01ARZ3NDEKTSV4RRFFQ69G5FB0");
    let mut ticket = store.read_ticket(&id).unwrap();
    ticket.legacy_number = Some("HS-8675309".into());
    ix.upsert(&ticket, "first.md", "with-legacy").unwrap();

    // Full-text search surfaces the ticket by its old number.
    let rows = ix
        .query(&TicketQuery {
            text: Some("8675309".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, id.to_string());
    // The queried row carries the legacy number on the wire so clients can link legacy
    // references from already-loaded rows without an extra lookup (HS2-XB5R3Y).
    assert_eq!(rows[0].legacy_number.as_deref(), Some("HS-8675309"));

    // Exact reference resolution maps the legacy number (case-insensitive) to the new id.
    assert_eq!(ix.resolve_id("HS-8675309").unwrap(), Some(id));
    assert_eq!(ix.resolve_id("hs-8675309").unwrap(), Some(id));
    // A number no ticket carries does not resolve.
    assert_eq!(ix.resolve_id("HS-0000000").unwrap(), None);
}

#[test]
fn fts_matches_attachment_filenames() {
    let (_d, store, ix) = seeded();
    let id = ulid("01ARZ3NDEKTSV4RRFFQ69G5FB0");
    let mut ticket = store.read_ticket(&id).unwrap();
    ticket.attachments.push(hotsheet_model::Attachment {
        id: ulid("01ARZ3NDEKTSV4RRFFQ69G5FC1"),
        filename: "server-details-narrow.png".into(),
        created_at: Timestamp::new("2026-08-19T00:02:00Z"),
        batch_id: None,
        batch_label: None,
        actor: None,
        purpose: None,
        annotations: vec![hotsheet_model::MediaAnnotation {
            id: "region-1".into(),
            x: 10,
            y: 20,
            width: 30,
            height: 40,
            start_ms: None,
            end_ms: None,
            text: "Inspect this edge".into(),
        }],
    });
    ticket.completed_at = Some(Timestamp::new("2026-09-01T03:00:00Z"));
    ticket.verified_at = Some(Timestamp::new("2026-09-01T04:00:00Z"));
    ix.upsert(&ticket, "first.md", "with-attachment").unwrap();

    let rows = ix
        .query(&TicketQuery {
            text: Some("server-details".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].slug, ticket.slug);
    for query in [
        TicketQuery {
            has_attachment: Some(true),
            ..Default::default()
        },
        TicketQuery {
            has_media_annotation: Some(true),
            ..Default::default()
        },
        TicketQuery {
            attachment_patterns: vec!["*.png".into()],
            ..Default::default()
        },
        TicketQuery {
            attachment_patterns: vec!["details-narrow".into()],
            ..Default::default()
        },
        TicketQuery {
            completed_after: Some("2026-09-01T02:59:00Z".into()),
            completed_before: Some("2026-09-01T03:01:00Z".into()),
            ..Default::default()
        },
        TicketQuery {
            verified_after: Some("2026-09-01T03:59:00Z".into()),
            verified_before: Some("2026-09-01T04:01:00Z".into()),
            ..Default::default()
        },
    ] {
        assert_eq!(
            ix.query(&query).unwrap().len(),
            1,
            "structured attachment query: {query:?}"
        );
    }
    assert!(
        ix.query(&TicketQuery {
            attachment_patterns: vec!["*.svg".into()],
            ..Default::default()
        })
        .unwrap()
        .is_empty()
    );
}

#[test]
fn upsert_updates_the_hash_and_delete_removes() {
    let (_d, store, ix) = seeded();
    let id = ulid("01ARZ3NDEKTSV4RRFFQ69G5FB0");
    assert!(ix.content_hash(&id).unwrap().is_some());

    ix.upsert(&store.read_ticket(&id).unwrap(), "p.md", "newhash")
        .unwrap();
    assert_eq!(ix.content_hash(&id).unwrap().as_deref(), Some("newhash"));

    ix.delete(&id).unwrap();
    assert!(ix.content_hash(&id).unwrap().is_none());
    assert_eq!(ix.query(&TicketQuery::default()).unwrap().len(), 2);
}

#[test]
fn recording_a_source_hash_preserves_the_last_healthy_projection() {
    let (_d, _store, ix) = seeded();
    let id = ulid("01ARZ3NDEKTSV4RRFFQ69G5FB0");
    let title = ix
        .query(&TicketQuery::default())
        .unwrap()
        .into_iter()
        .find(|row| row.id == id.to_string())
        .unwrap()
        .title;

    ix.record_source_hash(&id, "corrupt-bytes").unwrap();

    assert_eq!(
        ix.content_hash(&id).unwrap().as_deref(),
        Some("corrupt-bytes")
    );
    assert_eq!(
        ix.query(&TicketQuery::default())
            .unwrap()
            .into_iter()
            .find(|row| row.id == id.to_string())
            .unwrap()
            .title,
        title
    );
}

#[test]
fn feedback_needed_flag_round_trips_and_reconciles_both_ways() {
    use hotsheet_model::NoteKind;
    let (_d, store, ix) = seeded();
    let id = ulid("01ARZ3NDEKTSV4RRFFQ69G5FB0");
    let now = Timestamp::new("2026-08-19T01:00:00Z");

    let row = |ix: &Index| {
        ix.query(&TicketQuery::default())
            .unwrap()
            .into_iter()
            .find(|r| r.id == id.to_string())
            .unwrap()
    };

    // No feedback_needed note yet.
    assert!(!row(&ix).feedback_needed);

    // Add a feedback_needed note and reconcile: the flag flips on.
    ops::add_note(
        &store,
        &id,
        ulid("01ARZ3NDEKTSV4RRFFQ69G5FC0"),
        now.clone(),
        NoteKind::FeedbackNeeded,
        "please confirm".into(),
    )
    .unwrap();
    ix.reconcile(&store).unwrap();
    assert!(
        row(&ix).feedback_needed,
        "flag on after a feedback_needed note"
    );

    // A later regular note is the response and clears the unresolved feedback state.
    ops::add_note(
        &store,
        &id,
        ulid("01ARZ3NDEKTSV4RRFFQ69G5FC1"),
        Timestamp::new("2026-08-19T02:00:00Z"),
        NoteKind::Regular,
        "confirmed".into(),
    )
    .unwrap();
    ix.reconcile(&store).unwrap();
    assert!(
        !row(&ix).feedback_needed,
        "flag clears after a later regular response"
    );
}

#[test]
fn hs1_style_feedback_marker_sets_the_indexed_flag() {
    use hotsheet_model::{Note, NoteKind};
    let (_d, store, ix) = seeded();
    let id = ulid("01ARZ3NDEKTSV4RRFFQ69G5FB0");

    // Reproduce HS1's relaxed read shape in an early HS2 regular note: introductory
    // context is allowed and the marker's colon is optional.
    let mut ticket = store.read_ticket(&id).unwrap();
    ticket.notes.push(Note {
        id: ulid("01ARZ3NDEKTSV4RRFFQ69G5FC0"),
        created_at: Timestamp::new("2026-08-19T01:00:00Z"),
        edited_at: Timestamp::new("2026-08-19T01:00:00Z"),
        kind: NoteKind::Regular,
        summary: None,
        confidence: None,
        text: "Context first. FEEDBACK NEEDED choose one".into(),
    });
    store.write_ticket(&ticket).unwrap();
    ix.reconcile(&store).unwrap();

    let indexed = ix
        .query(&TicketQuery::default())
        .unwrap()
        .into_iter()
        .find(|row| row.id == id.to_string())
        .unwrap();
    assert!(indexed.feedback_needed);
}

#[test]
fn marked_description_sets_the_indexed_flag_until_a_regular_response() {
    use hotsheet_model::NoteKind;
    let (_d, store, ix) = seeded();
    let id = ulid("01ARZ3NDEKTSV4RRFFQ69G5FB0");
    let mut ticket = store.read_ticket(&id).unwrap();
    ticket.details = "FEEDBACK NEEDED: choose one\n\nCHOICE:\n- A\n- B".into();
    store.write_ticket(&ticket).unwrap();
    ix.reconcile(&store).unwrap();
    let row = |ix: &Index| {
        ix.query(&TicketQuery::default())
            .unwrap()
            .into_iter()
            .find(|row| row.id == id.to_string())
            .unwrap()
    };
    assert!(row(&ix).feedback_needed);
    ops::add_note(
        &store,
        &id,
        ulid("01ARZ3NDEKTSV4RRFFQ69G5FC0"),
        Timestamp::new("2026-08-19T01:00:00Z"),
        NoteKind::Regular,
        "Use B".into(),
    )
    .unwrap();
    ix.reconcile(&store).unwrap();
    assert!(!row(&ix).feedback_needed);
}

#[test]
fn a_stale_schema_version_triggers_a_rebuild() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("i.sqlite");
    drop(Index::open(&db, "s1").unwrap());

    // Simulate an older/newer schema.
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute(
        "UPDATE index_meta SET value='999' WHERE key='schema_version'",
        [],
    )
    .unwrap();
    drop(conn);

    // Reopening rebuilds cleanly (empty, no crash).
    let ix = Index::open(&db, "s1").unwrap();
    assert_eq!(ix.query(&TicketQuery::default()).unwrap().len(), 0);
}

#[test]
fn open_reconciled_restores_then_picks_up_offline_edits() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    let now = Timestamp::new("2026-08-19T00:00:00Z");
    let a = ops::create(
        &store,
        ulid("01ARZ3NDEKTSV4RRFFQ69G5FB0"),
        "HS",
        now.clone(),
        NewTicket {
            title: "one".into(),
            category: "task".into(),
            ..Default::default()
        },
    )
    .unwrap();

    let db = dir.path().join("index.sqlite");
    // First open builds from the store.
    {
        let ix = Index::open_reconciled(&db, &store).unwrap();
        assert_eq!(ix.query(&TicketQuery::default()).unwrap().len(), 1);
    }

    // Offline (no index running): add a ticket + edit the first.
    ops::create(
        &store,
        ulid("01ARZ3NDEKTSV4RRFFQ69G5FB1"),
        "HS",
        now.clone(),
        NewTicket {
            title: "two".into(),
            category: "task".into(),
            ..Default::default()
        },
    )
    .unwrap();
    ops::update(
        &store,
        &a.id,
        now,
        TicketPatch {
            up_next: Some(true),
            ..Default::default()
        },
    )
    .unwrap();

    // Reopen: restores the kept rows + reconciles the delta.
    let ix = Index::open_reconciled(&db, &store).unwrap();
    assert_eq!(
        ix.query(&TicketQuery::default()).unwrap().len(),
        2,
        "the new ticket was reconciled in"
    );
    assert_eq!(
        ix.query(&TicketQuery {
            up_next_only: true,
            ..Default::default()
        })
        .unwrap()
        .len(),
        1,
        "the edit was reconciled in"
    );
}

#[test]
fn reconcile_deletes_rows_whose_file_is_gone() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    let a = ops::create(
        &store,
        ulid("01ARZ3NDEKTSV4RRFFQ69G5FB0"),
        "HS",
        Timestamp::new("2026-08-19T00:00:00Z"),
        NewTicket {
            title: "gone soon".into(),
            category: "task".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let db = dir.path().join("i.sqlite");
    let ix = Index::open_reconciled(&db, &store).unwrap();
    assert_eq!(ix.query(&TicketQuery::default()).unwrap().len(), 1);

    std::fs::remove_file(store.ticket_path(&a.id)).unwrap();
    assert_eq!(ix.reconcile(&store).unwrap(), (0, 1));
    assert!(ix.query(&TicketQuery::default()).unwrap().is_empty());
}

#[test]
fn a_corrupt_index_file_is_deleted_and_rebuilt() {
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    ops::create(
        &store,
        ulid("01ARZ3NDEKTSV4RRFFQ69G5FB0"),
        "HS",
        Timestamp::new("2026-08-19T00:00:00Z"),
        NewTicket {
            title: "survivor".into(),
            category: "task".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let db = dir.path().join("corrupt.sqlite");
    std::fs::write(&db, b"definitely not a sqlite database").unwrap();

    let ix = Index::open_reconciled(&db, &store).unwrap();
    assert_eq!(ix.query(&TicketQuery::default()).unwrap().len(), 1);
}

#[test]
fn a_corrupt_ticket_file_is_skipped_not_fatal_on_rebuild_and_reconcile() {
    // Resilience (HS2-PRVPCQ): the server's project-open path builds the index over the
    // store. A single unparseable ticket file (here: a `notes:begin` with no `notes:end`)
    // must not abort the whole index build — the healthy tickets still index and the bad
    // one is skipped, so the web app can still open the project.
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    ops::create(
        &store,
        ulid("01ARZ3NDEKTSV4RRFFQ69G5FB0"),
        "HS",
        Timestamp::new("2026-08-19T00:00:00Z"),
        NewTicket {
            title: "survivor".into(),
            category: "task".into(),
            ..Default::default()
        },
    )
    .unwrap();

    // Plant a corrupt ticket file straight to disk (the writer would never produce it).
    let bad_id = ulid("3ZARZ3NDEKTSV4RRFFQ69G5FAV");
    let bad_path = store.ticket_path(&bad_id);
    std::fs::create_dir_all(bad_path.parent().unwrap()).unwrap();
    std::fs::write(
        &bad_path,
        format!(
            "---\nid: {bad_id}\nslug: HS-BROKEN\ntitle: broken\ncategory: bug\n\
             created_at: 2026-08-19T00:00:00Z\nupdated_at: 2026-08-19T00:00:00Z\nschema: 1\n---\n\n\
             <!-- hotsheet:body:begin -->\nbody\n<!-- hotsheet:body:end -->\n\n\
             <!-- hotsheet:notes:begin -->\n## Notes\n\nunterminated\n"
        ),
    )
    .unwrap();

    // Full rebuild indexes only the healthy ticket, and does not error.
    let ix = Index::open_in_memory("s1").unwrap();
    assert_eq!(ix.rebuild_from_store(&store).unwrap(), 1);
    assert_eq!(ix.query(&TicketQuery::default()).unwrap().len(), 1);

    // The full "open the project" path (open + reconcile) also survives the bad file.
    let db = dir.path().join("index.sqlite");
    let ix = Index::open_reconciled(&db, &store).unwrap();
    assert_eq!(ix.query(&TicketQuery::default()).unwrap().len(), 1);

    // Healing the file makes it index on the next reconcile — no restart needed.
    ops::create(
        &store,
        bad_id,
        "HS",
        Timestamp::new("2026-08-19T00:00:00Z"),
        NewTicket {
            title: "healed".into(),
            category: "bug".into(),
            ..Default::default()
        },
    )
    .unwrap();
    ix.reconcile(&store).unwrap();
    assert_eq!(ix.query(&TicketQuery::default()).unwrap().len(), 2);
}

// ---- git-diff fast path (docs/03 §3.4, HS2-90) ------------------------------------

/// A store that is a real git repo (as `hotsheet init` makes it), so autocommit + HEAD
/// tracking work.
fn git_store() -> (tempfile::TempDir, FsStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    let ok = std::process::Command::new("git")
        .args(["-C"])
        .arg(dir.path())
        .args(["init", "-q", "-b", "main"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    assert!(ok, "git init");
    (dir, store)
}

/// A committing create → the store has a HEAD and a clean tree.
fn commit_ticket(store: &FsStore, id: &str, title: &str) {
    ops::create(
        store,
        ulid(id),
        "HS",
        Timestamp::new("2026-08-19T00:00:00Z"),
        NewTicket {
            title: title.into(),
            category: "task".into(),
            ..Default::default()
        },
    )
    .unwrap();
}

#[test]
fn reconcile_uses_the_git_diff_fast_path_on_a_clean_head_move() {
    let (_dir, store) = git_store();
    commit_ticket(&store, "01ARZ3NDEKTSV4RRFFQ69G5FB0", "first");

    // Baseline reconcile records HEAD + indexes the first ticket.
    let ix = Index::open_in_memory(store.root().display().to_string()).unwrap();
    assert_eq!(ix.reconcile(&store).unwrap(), (1, 0));

    // No change since last reconcile → HEAD unchanged + clean tree → zero-work fast path.
    assert!(store.is_working_tree_clean());
    assert_eq!(
        ix.reconcile(&store).unwrap(),
        (0, 0),
        "unchanged HEAD is a no-op"
    );

    // A committed add moves HEAD; the fast path reconciles only the one new ticket.
    commit_ticket(&store, "01ARZ3NDEKTSV4RRFFQ69G5FB1", "second");
    assert_eq!(ix.reconcile(&store).unwrap(), (1, 0), "only the delta");
    assert_eq!(ix.query(&TicketQuery::default()).unwrap().len(), 2);
}

#[test]
fn changed_ticket_ids_between_two_commits_lists_the_delta() {
    let (_dir, store) = git_store();
    commit_ticket(&store, "01ARZ3NDEKTSV4RRFFQ69G5FB0", "first");
    let base = store.head_commit().unwrap();
    commit_ticket(&store, "01ARZ3NDEKTSV4RRFFQ69G5FB1", "second");
    let head = store.head_commit().unwrap();

    let changed = store.changed_ticket_ids_between(&base, &head).unwrap();
    assert_eq!(
        changed,
        vec![ulid("01ARZ3NDEKTSV4RRFFQ69G5FB1")],
        "only the new ticket"
    );
}

#[test]
fn dirty_tree_reconciles_only_changed_ticket_ids() {
    let (_dir, store) = git_store();
    commit_ticket(&store, "01ARZ3NDEKTSV4RRFFQ69G5FB0", "committed");
    let ix = Index::open_in_memory(store.root().display().to_string()).unwrap();
    assert_eq!(ix.reconcile(&store).unwrap(), (1, 0));

    // Write a ticket WITHOUT committing. Git identifies the new file, so reconciliation
    // picks it up without walking every ticket in the store.
    let mut t = store
        .read_ticket(&ulid("01ARZ3NDEKTSV4RRFFQ69G5FB0"))
        .unwrap();
    t.id = ulid("01ARZ3NDEKTSV4RRFFQ69G5FB2");
    t.slug = "HS-UNCOMMD".into();
    t.title = "uncommitted".into();
    store.write_ticket(&t).unwrap();
    assert!(
        !store.is_working_tree_clean(),
        "uncommitted write dirties the tree"
    );

    assert_eq!(
        ix.reconcile(&store).unwrap(),
        (1, 0),
        "dirty delta indexes the uncommitted add"
    );
    assert_eq!(ix.query(&TicketQuery::default()).unwrap().len(), 2);
}

#[test]
fn reverting_a_dirty_ticket_refreshes_its_index_row() {
    let (_dir, store) = git_store();
    let id = ulid("01ARZ3NDEKTSV4RRFFQ69G5FB0");
    commit_ticket(&store, "01ARZ3NDEKTSV4RRFFQ69G5FB0", "committed");
    let ix = Index::open_in_memory(store.root().display().to_string()).unwrap();
    assert_eq!(ix.reconcile(&store).unwrap(), (1, 0));

    let path = store.ticket_path(&id);
    let committed = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, committed.replace("title: committed", "title: dirty")).unwrap();
    assert_eq!(ix.reconcile(&store).unwrap(), (1, 0));
    assert_eq!(ix.query(&TicketQuery::default()).unwrap()[0].title, "dirty");

    // The current status is clean after restoring the committed bytes. The remembered
    // previous dirty set still forces this ticket to refresh instead of leaving stale data.
    std::fs::write(&path, committed).unwrap();
    assert!(store.is_working_tree_clean());
    assert_eq!(ix.reconcile(&store).unwrap(), (1, 0));
    assert_eq!(
        ix.query(&TicketQuery::default()).unwrap()[0].title,
        "committed"
    );
}

// ---- facet filters (HS2-89) -------------------------------------------------------

#[test]
fn assignee_facet_and_claimed_filters() {
    let (_d, store, ix) = seeded();
    let now = Timestamp::new("2026-08-22T00:00:00Z");
    let fb0 = ulid("01ARZ3NDEKTSV4RRFFQ69G5FB0");
    let _fb1 = ulid("01ARZ3NDEKTSV4RRFFQ69G5FB1");

    // Assign one ticket to Dana, claim another; reindex so the facets + columns update.
    ops::assign(
        &store,
        &fb0,
        now.clone(),
        Some(vec!["dana@x.co".into()]),
        vec![],
    )
    .unwrap();
    ops::claim_next_with_eta(
        &store,
        &now,
        Timestamp::new("2026-08-22T01:00:00Z"),
        "worker-a",
        None,
        Some(Timestamp::new("2026-08-22T00:45:00Z")),
    )
    .unwrap();
    ix.reconcile(&store).unwrap();

    // Assignee filter now resolves index-side (via the assignees facet), not silently ignored.
    let dana = ix
        .query(&TicketQuery {
            assignee: Some("dana@x.co".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(dana.len(), 1);
    assert_eq!(dana[0].id, fb0.to_string());
    // A non-assignee matches nothing.
    assert!(
        ix.query(&TicketQuery {
            assignee: Some("nobody@x.co".into()),
            ..Default::default()
        })
        .unwrap()
        .is_empty()
    );

    // claimed=true → exactly the claimed one; claimed=false → the rest.
    let claimed = ix
        .query(&TicketQuery {
            claimed: Some(true),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(claimed.len(), 1);
    assert_eq!(
        claimed[0].id,
        fb0.to_string(),
        "claim_next took the Up Next ticket (FB0)"
    );
    assert_eq!(claimed[0].claimed_by.as_deref(), Some("worker-a"));
    assert_eq!(
        claimed[0].claim_lease_expires_at.as_deref(),
        Some("2026-08-22T01:00:00Z")
    );
    // The index carries the live claim's ETA to list rows (HS2-DQQ0AX).
    assert_eq!(
        claimed[0].claim_eta_at.as_deref(),
        Some("2026-08-22T00:45:00Z")
    );
    // ... and when the live claim began, for progress toward that ETA (HS2-XQMDQB).
    assert_eq!(
        claimed[0].claim_started_at.as_deref(),
        Some("2026-08-22T00:00:00Z")
    );
    let unclaimed = ix
        .query(&TicketQuery {
            claimed: Some(false),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(unclaimed.len(), 2);
}

/// Value-keyset reads (HS2-74H84S) must select exactly the rows that
/// `checkout_order::compare` places strictly after an arbitrary key — including keys whose
/// row was edited or purged, and keys from another source that tie on every sort value —
/// so the checkout merge can resume every source from one global key.
#[test]
fn value_keyset_matches_the_checkout_order_for_every_sort_and_arbitrary_keys() {
    use hotsheet_ticketing::checkout_order::{AfterKey, MergeKey, compare};

    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    // Deliberate ties: shared timestamps, priorities, case-folded titles, and statuses.
    let seeds = [
        (
            "01ARZ3NDEKTSV4RRFFQ69G5FB0",
            "2026-08-19T00:00:00Z",
            "beta",
            Priority::High,
            Status::NotStarted,
        ),
        (
            "01ARZ3NDEKTSV4RRFFQ69G5FB1",
            "2026-08-19T00:00:00Z",
            "Beta",
            Priority::High,
            Status::Started,
        ),
        (
            "01ARZ3NDEKTSV4RRFFQ69G5FB2",
            "2026-08-18T00:00:00Z",
            "alpha",
            Priority::Low,
            Status::NotStarted,
        ),
        (
            "01ARZ3NDEKTSV4RRFFQ69G5FB3",
            "2026-08-20T00:00:00Z",
            "Élan",
            Priority::High,
            Status::Backlog,
        ),
        (
            "01ARZ3NDEKTSV4RRFFQ69G5FB4",
            "2026-08-19T00:00:00Z",
            "zeta",
            Priority::Default,
            Status::NotStarted,
        ),
        (
            "01ARZ3NDEKTSV4RRFFQ69G5FB5",
            "2026-08-18T00:00:00Z",
            "BETA",
            Priority::Default,
            Status::Started,
        ),
    ];
    for (id, now, title, priority, status) in seeds {
        ops::create(
            &store,
            ulid(id),
            "HS",
            Timestamp::new(now),
            NewTicket {
                title: title.into(),
                category: "task".into(),
                priority,
                status,
                ..Default::default()
            },
        )
        .unwrap();
    }
    // HS2-RD4M29: completed tickets with tied, distinct, and missing confidence scores.
    for (id, now, score) in [
        (
            "01ARZ3NDEKTSV4RRFFQ69G5FB3",
            "2026-08-20T00:00:00Z",
            Some(40),
        ),
        (
            "01ARZ3NDEKTSV4RRFFQ69G5FB4",
            "2026-08-19T00:00:00Z",
            Some(40),
        ),
        ("01ARZ3NDEKTSV4RRFFQ69G5FB5", "2026-08-18T00:00:00Z", None),
        (
            "01ARZ3NDEKTSV4RRFFQ69G5FB2",
            "2026-08-18T00:00:00Z",
            Some(95),
        ),
    ] {
        complete_with_score(&store, id, now, score);
    }
    let index = Index::open_in_memory("s1").unwrap();
    index.rebuild_from_store(&store).unwrap();

    let keyed = |rows: Vec<TicketRow>| {
        rows.into_iter()
            .map(|mut row| {
                row.set_connection("store-m");
                MergeKey::from_row(&row)
            })
            .collect::<Vec<_>>()
    };
    for sort in [
        SortKey::Id,
        SortKey::Created,
        SortKey::Updated,
        SortKey::Priority,
        SortKey::Status,
        SortKey::Title,
        SortKey::Confidence,
    ] {
        for descending in [false, true] {
            let base = TicketQuery {
                sort,
                descending,
                ..Default::default()
            };
            let full = keyed(index.query(&base).unwrap());
            let mut expected_order = full.clone();
            expected_order.sort_by(|left, right| compare(left, right, sort, descending));
            assert_eq!(
                full, expected_order,
                "index order is checkout order: {sort:?} {descending}"
            );

            // Probe with every row's own key, the same values under a lower and a higher
            // foreign connection (cross-source ties), and edited copies (the boundary row's
            // sort values changed since it was emitted).
            let mut probes = Vec::new();
            for key in &full {
                probes.push(key.clone());
                for connection in ["a-store", "z-store"] {
                    probes.push(MergeKey {
                        qualified_id: format!("{connection}:{}", key.native_id),
                        ..key.clone()
                    });
                }
                probes.push(MergeKey {
                    updated_at: "2026-08-19T00:00:00.5Z".into(),
                    created_at: "2026-08-19T00:00:00.5Z".into(),
                    native_id: format!("{}X", key.native_id),
                    qualified_id: format!("store-m:{}X", key.native_id),
                    ..key.clone()
                });
            }
            for probe in probes {
                let query = TicketQuery {
                    after_key: Some(AfterKey {
                        key: probe.clone(),
                        connection_id: "store-m".into(),
                    }),
                    ..base.clone()
                };
                let expected = full
                    .iter()
                    .filter(|key| compare(key, &probe, sort, descending).is_gt())
                    .cloned()
                    .collect::<Vec<_>>();
                assert_eq!(
                    keyed(index.query(&query).unwrap()),
                    expected,
                    "index {sort:?} descending={descending} after {probe:?}"
                );
                let scanned = ops::query(&store, &query)
                    .unwrap()
                    .into_iter()
                    .map(|ticket| ticket.id.to_string())
                    .collect::<HashSet<_>>();
                assert_eq!(
                    scanned,
                    expected
                        .iter()
                        .map(|key| key.native_id.clone())
                        .collect::<HashSet<_>>(),
                    "file scan {sort:?} descending={descending} after {probe:?}"
                );
            }
        }
    }
}

#[test]
fn index_file_name_is_scoped_by_schema_version_so_builds_never_share_a_file() {
    // HS2-8ZM4PT: an older build rebuilding the shared file at its own schema dropped
    // columns under a newer running process. Each schema generation owns its own file.
    let name = index_file_name("0123456789abcdef");
    assert_eq!(name, format!("0123456789abcdef.v{SCHEMA_VERSION}.sqlite"));
    assert_ne!(
        name, "0123456789abcdef.sqlite",
        "must not collide with the unversioned legacy file"
    );

    // A file another (older) generation rebuilt at its schema is left alone: this build's
    // file keeps its schema and rows.
    let (dir, store, _) = seeded();
    let ours = dir.path().join(index_file_name("k"));
    let older = dir.path().join(format!("k.v{}.sqlite", SCHEMA_VERSION - 1));
    let ix = Index::open_reconciled(&ours, &store).unwrap();
    let before = ix.ticket_count().unwrap();
    {
        let conn = Connection::open(&older).unwrap();
        conn.execute_batch(
            "CREATE TABLE index_meta(key TEXT PRIMARY KEY, value TEXT); \
             INSERT INTO index_meta VALUES('schema_version','0'); CREATE TABLE tickets(id TEXT);",
        )
        .unwrap();
    }
    let reopened = Index::open_reconciled(&ours, &store).unwrap();
    assert_eq!(reopened.ticket_count().unwrap(), before);
    assert_eq!(ix.ticket_count().unwrap(), before);
}

#[test]
fn concurrent_opens_of_a_fresh_index_file_all_succeed_and_keep_one_file() {
    // HS2-SY8T90: the server and the app's setup refresh opened the same brand-new index
    // at once. Unserialized schema creation raced (`PRIMARY KEY constraint failed` on
    // `schema_version`), and the loser then deleted the winner's live file.
    let (dir, store, _) = seeded();
    let expected = Index::open_in_memory("x")
        .unwrap()
        .rebuild_from_store(&store)
        .unwrap();
    for round in 0..8 {
        let path = dir.path().join(format!("race-{round}.sqlite"));
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(6));
        let handles: Vec<_> = (0..6)
            .map(|_| {
                let (path, root, barrier) =
                    (path.clone(), store.root().to_path_buf(), barrier.clone());
                std::thread::spawn(move || {
                    let store = FsStore::open(&root).unwrap();
                    barrier.wait();
                    let index = Index::open_reconciled(&path, &store).expect("concurrent open");
                    index.ticket_count().unwrap()
                })
            })
            .collect();
        for handle in handles {
            assert_eq!(handle.join().unwrap(), expected, "round {round}");
        }
        // One shared file survives with the full schema and rows.
        let reopened = Index::open_reconciled(&path, &store).unwrap();
        assert_eq!(reopened.ticket_count().unwrap(), expected);
    }
}

#[test]
fn an_unreadable_index_file_is_still_rebuilt_but_a_busy_one_is_not_deleted() {
    let (dir, store, _) = seeded();
    let expected = Index::open_in_memory("x")
        .unwrap()
        .rebuild_from_store(&store)
        .unwrap();

    // Garbage bytes are genuine corruption: discard and rebuild.
    let garbage = dir.path().join("garbage.sqlite");
    std::fs::write(&garbage, vec![0x5a; 8192]).unwrap();
    assert_eq!(
        Index::open_reconciled(&garbage, &store)
            .unwrap()
            .ticket_count()
            .unwrap(),
        expected
    );

    // A writer holding the lock makes a second opener wait (busy timeout), not delete it.
    let held = dir.path().join("held.sqlite");
    let first = Index::open_reconciled(&held, &store).unwrap();
    first.conn.execute_batch("BEGIN IMMEDIATE").unwrap();
    let opener = {
        let (held, root) = (held.clone(), store.root().to_path_buf());
        std::thread::spawn(move || {
            let store = FsStore::open(&root).unwrap();
            Index::open_reconciled(&held, &store).map(|index| index.ticket_count().unwrap())
        })
    };
    std::thread::sleep(std::time::Duration::from_millis(300));
    first.conn.execute_batch("COMMIT").unwrap();
    assert_eq!(opener.join().unwrap().unwrap(), expected);
    assert_eq!(
        first.ticket_count().unwrap(),
        expected,
        "the first process's file was not deleted"
    );
}

/// HS2-Y0PAEM: index files from older schema generations (and unversioned pre-HS2-8ZM4PT
/// files) are pruned with their sidecars; the current generation, newer generations, and
/// unrelated files are kept.
#[test]
fn prune_removes_only_idle_older_generation_index_files() {
    let (_store_dir, store, _) = seeded();
    let dir = tempfile::tempdir().unwrap();
    let older = dir
        .path()
        .join(format!("aaaa.v{}.sqlite", SCHEMA_VERSION - 1));
    let legacy = dir.path().join("bbbb.sqlite");
    let current = dir.path().join(index_file_name("aaaa"));
    let newer = dir
        .path()
        .join(format!("aaaa.v{}.sqlite", SCHEMA_VERSION + 1));
    let unrelated = dir.path().join("notes.txt");
    for path in [&older, &legacy, &current, &newer] {
        // Real WAL-mode index files, closed again (like a crashed or exited older build).
        drop(Index::open_reconciled(path, &store).unwrap());
    }
    std::fs::write(&unrelated, "keep me").unwrap();
    std::fs::write(sidecar(&legacy, "-wal"), b"").unwrap();
    std::fs::write(sidecar(&legacy, "-shm"), b"").unwrap();

    let report = prune_stale_index_files(dir.path()).unwrap();
    let mut removed = report.removed.clone();
    removed.sort();
    let mut expected = vec![older.clone(), legacy.clone()];
    expected.sort();
    assert_eq!(removed, expected);
    assert!(report.in_use.is_empty());
    for gone in [&older, &legacy] {
        assert!(!gone.exists(), "{} pruned", gone.display());
        for suffix in ["-wal", "-shm", "-journal"] {
            assert!(!sidecar(gone, suffix).exists(), "{suffix} sidecar pruned");
        }
    }
    for kept in [&current, &newer, &unrelated] {
        assert!(kept.exists(), "{} kept", kept.display());
    }
    // The current generation is still a working index.
    let reopened = Index::open_reconciled(&current, &store).unwrap();
    assert!(reopened.ticket_count().unwrap() > 0);

    // Idempotent: a second prune has nothing left to do.
    assert_eq!(
        prune_stale_index_files(dir.path()).unwrap(),
        PruneReport::default()
    );
}

/// HS2-Y0PAEM: a stale file a live connection holds open (an older build still running)
/// is never deleted; it is reported in use and pruned once that connection goes away.
/// Walks open → prune (kept) → write while held → prune (kept) → second opener → close →
/// prune (removed).
#[test]
fn prune_never_deletes_a_stale_index_a_live_connection_holds() {
    let (_store_dir, store, _) = seeded();
    let dir = tempfile::tempdir().unwrap();
    let held_path = dir
        .path()
        .join(format!("cccc.v{}.sqlite", SCHEMA_VERSION - 1));
    let held = Index::open_reconciled(&held_path, &store).unwrap();
    let count = held.ticket_count().unwrap();

    let report = prune_stale_index_files(dir.path()).unwrap();
    assert_eq!(report.in_use, vec![held_path.clone()]);
    assert!(report.removed.is_empty());
    assert!(held_path.exists());

    // The holder keeps working normally after the failed probe (its locks are intact).
    held.rebuild_from_store(&store).unwrap();
    assert_eq!(held.ticket_count().unwrap(), count);
    let again = prune_stale_index_files(dir.path()).unwrap();
    assert_eq!(again.in_use, vec![held_path.clone()]);

    // Another opener still reads the same live file (not a recreated empty one).
    let second = Index::open_reconciled(&held_path, &store).unwrap();
    assert_eq!(second.ticket_count().unwrap(), count);
    drop(second);
    assert_eq!(
        prune_stale_index_files(dir.path()).unwrap().in_use,
        vec![held_path.clone()],
        "still held by the first connection"
    );

    drop(held);
    let report = prune_stale_index_files(dir.path()).unwrap();
    assert_eq!(report.removed, vec![held_path.clone()]);
    assert!(!held_path.exists());
    assert!(!sidecar(&held_path, "-wal").exists());
}

const PRUNE_HOLDER_ENV: &str = "HS2_PRUNE_TEST_HOLD_INDEX";

/// Child-process half of [`prune_never_deletes_a_stale_index_another_process_holds`]:
/// a no-op unless that test re-executes this binary with [`PRUNE_HOLDER_ENV`] set. It then
/// opens the given index (as an older build's server would), reports `ready`, and holds
/// the file open until its stdin closes.
#[test]
fn prune_holder_child_process() {
    let Ok(path) = std::env::var(PRUNE_HOLDER_ENV) else {
        return;
    };
    let index = Index::open(std::path::Path::new(&path), "holder").unwrap();
    assert!(index.ticket_count().is_ok());
    println!("ready");
    use std::io::Write as _;
    std::io::stdout().flush().unwrap();
    let mut sink = String::new();
    let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut sink);
    drop(index);
}

/// HS2-Y0PAEM: the real hazard is another *process* (an older build's server) using a
/// stale generation's file. SQLite's file locks are the evidence: while that process holds
/// the file the prune keeps it; once the process exits, the prune removes it.
#[test]
fn prune_never_deletes_a_stale_index_another_process_holds() {
    use std::io::BufRead as _;
    let dir = tempfile::tempdir().unwrap();
    let held_path = dir
        .path()
        .join(format!("eeee.v{}.sqlite", SCHEMA_VERSION - 1));
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "tests::prune_holder_child_process",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(PRUNE_HOLDER_ENV, &held_path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdout = std::io::BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    while stdout.read_line(&mut line).unwrap() > 0 && !line.contains("ready") {
        line.clear();
    }
    assert!(line.contains("ready"), "holder process opened the index");

    let report = prune_stale_index_files(dir.path()).unwrap();
    assert_eq!(report.in_use, vec![held_path.clone()]);
    assert!(held_path.exists(), "a file another process holds is kept");

    drop(child.stdin.take());
    assert!(child.wait().unwrap().success());
    let report = prune_stale_index_files(dir.path()).unwrap();
    assert_eq!(report.removed, vec![held_path.clone()]);
    assert!(!held_path.exists());
}

/// HS2-Y0PAEM: a stale file that is not a database at all is removed; a missing index
/// directory is an empty report, not an error.
#[test]
fn prune_removes_corrupt_stale_files_and_tolerates_a_missing_dir() {
    let dir = tempfile::tempdir().unwrap();
    let garbage = dir.path().join("dddd.v1.sqlite");
    std::fs::write(&garbage, vec![0x5a; 8192]).unwrap();
    let report = prune_stale_index_files(dir.path()).unwrap();
    assert_eq!(report.removed, vec![garbage.clone()]);
    assert!(!garbage.exists());

    assert_eq!(
        prune_stale_index_files(&dir.path().join("missing")).unwrap(),
        PruneReport::default()
    );
}

/// Complete a ticket at `now` with an optional scored completing note (HS2-RD4M29).
fn complete_with_score(store: &FsStore, id: &str, now: &str, score: Option<u64>) {
    let id = ulid(id);
    ops::update(
        store,
        &id,
        Timestamp::new(now),
        TicketPatch {
            status: Some(Status::Completed),
            ..Default::default()
        },
    )
    .unwrap();
    ops::add_note_with_metadata(
        store,
        &id,
        Ulid::new(),
        Timestamp::new(now),
        hotsheet_model::NoteKind::Regular,
        ops::NoteMetadataInput {
            summary: None,
            confidence: score.map(|value| hotsheet_model::Confidence::new(value).unwrap()),
        },
        "done".into(),
    )
    .unwrap();
}

#[test]
fn confidence_column_filters_and_sorts_like_the_file_scan() {
    let (_d, store, index) = seeded();
    // FB2 is already completed: give it a score, then complete FB1 with a low one.
    complete_with_score(
        &store,
        "01ARZ3NDEKTSV4RRFFQ69G5FB2",
        "2026-08-19T00:00:00Z",
        Some(88),
    );
    complete_with_score(
        &store,
        "01ARZ3NDEKTSV4RRFFQ69G5FB1",
        "2026-08-19T00:00:00Z",
        Some(35),
    );
    for id in ["01ARZ3NDEKTSV4RRFFQ69G5FB1", "01ARZ3NDEKTSV4RRFFQ69G5FB2"] {
        let ticket = store.read_ticket(&ulid(id)).unwrap();
        index.upsert(&ticket, "x", &format!("hash-{id}")).unwrap();
    }
    let rows = index.query(&TicketQuery::default()).unwrap();
    let score = |id: &str| {
        rows.iter()
            .find(|row| row.id == id)
            .unwrap()
            .latest_confidence
    };
    assert_eq!(score("01ARZ3NDEKTSV4RRFFQ69G5FB2"), Some(88));
    assert_eq!(score("01ARZ3NDEKTSV4RRFFQ69G5FB1"), Some(35));
    assert_eq!(score("01ARZ3NDEKTSV4RRFFQ69G5FB0"), None);
    for (min, max, expected) in [
        (Some(50), None, vec!["01ARZ3NDEKTSV4RRFFQ69G5FB2"]),
        (None, Some(50), vec!["01ARZ3NDEKTSV4RRFFQ69G5FB1"]),
        (
            Some(35),
            Some(88),
            vec!["01ARZ3NDEKTSV4RRFFQ69G5FB1", "01ARZ3NDEKTSV4RRFFQ69G5FB2"],
        ),
        (
            Some(0),
            Some(100),
            vec!["01ARZ3NDEKTSV4RRFFQ69G5FB1", "01ARZ3NDEKTSV4RRFFQ69G5FB2"],
        ),
        (Some(89), Some(100), vec![]),
    ] {
        let q = TicketQuery {
            min_confidence: min,
            max_confidence: max,
            ..Default::default()
        };
        let expected = expected
            .into_iter()
            .map(String::from)
            .collect::<HashSet<_>>();
        assert_eq!(
            index_ids(&index.query(&q).unwrap()),
            expected,
            "{min:?}..{max:?}"
        );
        assert_eq!(ops_ids(&store, &q), expected, "file scan {min:?}..{max:?}");
    }
    // Least confident first, unscored last; reopening drops the derived score.
    let sorted = index
        .query(&TicketQuery {
            sort: SortKey::Confidence,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(sorted[0].id, "01ARZ3NDEKTSV4RRFFQ69G5FB1");
    assert_eq!(sorted[1].id, "01ARZ3NDEKTSV4RRFFQ69G5FB2");
    assert!(sorted[2].latest_confidence.is_none());
    let reopened = ops::update(
        &store,
        &ulid("01ARZ3NDEKTSV4RRFFQ69G5FB1"),
        Timestamp::new("2026-08-20T00:00:00Z"),
        TicketPatch {
            status: Some(Status::Started),
            ..Default::default()
        },
    )
    .unwrap();
    index.upsert(&reopened, "x", "hash-reopened").unwrap();
    let low = TicketQuery {
        max_confidence: Some(50),
        ..Default::default()
    };
    assert!(index.query(&low).unwrap().is_empty());
    assert!(ops_ids(&store, &low).is_empty());
}
