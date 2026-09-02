use std::collections::HashSet;

use hotsheet_model::{Priority, Status, Timestamp, Ulid};
use hotsheet_ticketing::{FsStore, NewTicket, StoreMetadata, TicketPatch, TicketQuery, ops};

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
    let _b = mk("01ARZ3NDEKTSV4RRFFQ69G5FB1", &early, vec![a.id]); // blocked (A open)
    let c = mk("01ARZ3NDEKTSV4RRFFQ69G5FB2", &late, vec![]);
    let _d = mk("01ARZ3NDEKTSV4RRFFQ69G5FB3", &late, vec![c.id]); // becomes unblocked once C done
    let m = mk("01ARZ3NDEKTSV4RRFFQ69G5FB4", &late, vec![]);
    let r = mk("01ARZ3NDEKTSV4RRFFQ69G5FB5", &late, vec![]);

    // C is done (satisfies D's blocker); M is a moved tombstone.
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

    // Spot-check the semantics (not just parity): B is blocked, D is unblocked, M is hidden.
    let blocked = ops_ids(
        &store,
        &TicketQuery {
            blocked: Some(true),
            ..Default::default()
        },
    );
    assert!(
        blocked.contains("01ARZ3NDEKTSV4RRFFQ69G5FB1"),
        "B blocked by open A"
    );
    assert!(
        !blocked.contains("01ARZ3NDEKTSV4RRFFQ69G5FB3"),
        "D unblocked (C done)"
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
fn dirty_tree_falls_back_to_the_full_walk() {
    let (_dir, store) = git_store();
    commit_ticket(&store, "01ARZ3NDEKTSV4RRFFQ69G5FB0", "committed");
    let ix = Index::open_in_memory(store.root().display().to_string()).unwrap();
    assert_eq!(ix.reconcile(&store).unwrap(), (1, 0));

    // Write a ticket WITHOUT committing — the tree is now dirty, so the fast path is
    // skipped and the full hash-walk still picks up the uncommitted file.
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
        "full walk indexes the uncommitted add"
    );
    assert_eq!(ix.query(&TicketQuery::default()).unwrap().len(), 2);
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
    ops::claim_next(
        &store,
        &now,
        Timestamp::new("2026-08-22T01:00:00Z"),
        "worker-a",
        None,
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
    let unclaimed = ix
        .query(&TicketQuery {
            claimed: Some(false),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(unclaimed.len(), 2);
}
