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
    ] {
        assert_eq!(
            index_ids(&ix.query(&q).unwrap()),
            ops_ids(&store, &q),
            "index diverged from ops::query for {q:?}"
        );
    }
}

#[test]
fn fts_matches_a_prefix_across_the_body() {
    let (_d, _s, ix) = seeded();
    let rows = ix
        .query(&TicketQuery {
            text: Some("flick".into()), // prefix of "flicker"
            ..Default::default()
        })
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].title, "Dashboard flicker");
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
