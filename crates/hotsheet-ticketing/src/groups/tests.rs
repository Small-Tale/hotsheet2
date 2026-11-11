use super::*;
use crate::ops::{self, NewTicket, TicketPatch};
use crate::store::StoreMetadata;

fn ts(raw: &str) -> Timestamp {
    Timestamp::new(raw)
}

fn store() -> (tempfile::TempDir, FsStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    (dir, store)
}

fn create(store: &FsStore, title: &str, group: Option<&str>) -> Ticket {
    ops::create(
        store,
        Ulid::new(),
        "HS",
        ts("2026-11-01T00:00:00Z"),
        NewTicket {
            title: title.into(),
            category: "task".into(),
            group: group.map(str::to_string),
            ..Default::default()
        },
    )
    .unwrap()
}

fn set_group(store: &FsStore, id: &Ulid, group: Option<&str>) -> Result<Ticket, StoreError> {
    ops::update(
        store,
        id,
        ts("2026-11-02T00:00:00Z"),
        TicketPatch {
            group: Some(group.map(str::to_string)),
            ..Default::default()
        },
    )
}

fn groups_of(store: &FsStore) -> Vec<(String, usize)> {
    GroupDirectory::scan(store)
        .unwrap()
        .summaries()
        .into_iter()
        .map(|group| (group.name, group.count))
        .collect()
}

#[test]
fn hs2_pdshnn_create_and_update_set_clear_and_validate_the_group() {
    let (_dir, store) = store();
    let created = create(&store, "one", Some("  Board   drag & drop "));
    assert_eq!(created.group.as_deref(), Some("Board drag & drop"));
    assert_eq!(
        store.read_ticket(&created.id).unwrap().group.as_deref(),
        Some("Board drag & drop")
    );
    assert_eq!(create(&store, "blank", Some("   ")).group, None);

    let plain = create(&store, "two", None);
    assert_eq!(plain.group, None);
    let grouped = set_group(&store, &plain.id, Some(" Sprint  12 ")).unwrap();
    assert_eq!(grouped.group.as_deref(), Some("Sprint 12"));
    assert_eq!(
        grouped.updated_at, plain.updated_at,
        "regrouping is organization and keeps updated_at"
    );

    // Absent leaves it; an unrelated edit keeps the group.
    let retitled = ops::update(
        &store,
        &plain.id,
        ts("2026-11-03T00:00:00Z"),
        TicketPatch {
            title: Some("two again".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(retitled.group.as_deref(), Some("Sprint 12"));

    for bad in ["a\nb", "tab\there", &"x".repeat(121)] {
        let error = set_group(&store, &plain.id, Some(bad)).unwrap_err();
        assert!(matches!(error, StoreError::InvalidGroupName(_)), "{bad:?}");
        assert_eq!(
            store.read_ticket(&plain.id).unwrap().group.as_deref(),
            Some("Sprint 12"),
            "a refused name writes nothing"
        );
    }
    let invalid_create = ops::create(
        &store,
        Ulid::new(),
        "HS",
        ts("2026-11-01T00:00:00Z"),
        NewTicket {
            title: "bad".into(),
            group: Some("line\nbreak".into()),
            ..Default::default()
        },
    );
    assert!(matches!(
        invalid_create,
        Err(StoreError::InvalidGroupName(_))
    ));

    // Clearing by null and by empty text, repeated, then refilling.
    assert_eq!(set_group(&store, &plain.id, None).unwrap().group, None);
    assert_eq!(set_group(&store, &plain.id, None).unwrap().group, None);
    assert_eq!(
        set_group(&store, &plain.id, Some("Again"))
            .unwrap()
            .group
            .as_deref(),
        Some("Again")
    );
    assert_eq!(
        set_group(&store, &plain.id, Some("  ")).unwrap().group,
        None
    );
}

#[test]
fn hs2_pdshnn_directory_counts_live_members_and_sorts_without_case() {
    let (_dir, store) = store();
    let a = create(&store, "a", Some("beta"));
    create(&store, "b", Some("Alpha"));
    create(&store, "c", Some("alpha"));
    create(&store, "d", Some("Émile"));
    create(&store, "e", None);
    assert_eq!(
        groups_of(&store),
        vec![
            ("Alpha".to_string(), 2),
            ("beta".to_string(), 1),
            ("Émile".to_string(), 1)
        ],
        "mixed spellings are one group, named by its earliest member"
    );

    // A group lives only while a non-deleted ticket names it; restoring brings it back.
    ops::update(
        &store,
        &a.id,
        ts("2026-11-02T00:00:00Z"),
        TicketPatch {
            status: Some(Status::Deleted),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(!groups_of(&store).iter().any(|(name, _)| name == "beta"));
    let directory = GroupDirectory::scan(&store).unwrap();
    assert_eq!(directory.canonical_name("beta", &[]), None);
    assert_eq!(directory.member_ids("beta"), vec![a.id]);
    ops::update(
        &store,
        &a.id,
        ts("2026-11-03T00:00:00Z"),
        TicketPatch {
            status: Some(Status::NotStarted),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(groups_of(&store).iter().any(|(name, _)| name == "beta"));

    // Empty, then refilled.
    let (_dir, empty) = self::store();
    assert!(groups_of(&empty).is_empty());
    let only = create(&empty, "only", Some("Solo"));
    assert_eq!(groups_of(&empty), vec![("Solo".to_string(), 1)]);
    set_group(&empty, &only.id, None).unwrap();
    assert!(groups_of(&empty).is_empty());
}

#[test]
fn hs2_pdshnn_a_requested_name_reuses_the_existing_spelling() {
    let (_dir, store) = store();
    let first = create(&store, "first", Some("Board Drag"));
    let second = create(&store, "second", None);
    let directory = GroupDirectory::scan(&store).unwrap();

    assert_eq!(
        resolve_group_name(&directory, "  board   DRAG ", &[second.id]).unwrap(),
        Some("Board Drag".to_string())
    );
    assert_eq!(
        resolve_group_name(&directory, "Brand new", &[second.id]).unwrap(),
        Some("Brand new".to_string())
    );
    // The only member is the ticket being patched: the new spelling wins.
    assert_eq!(
        resolve_group_name(&directory, "BOARD DRAG", &[first.id]).unwrap(),
        Some("BOARD DRAG".to_string())
    );
    assert_eq!(resolve_group_name(&directory, "  ", &[]).unwrap(), None);
    assert_eq!(
        resolve_group_name(&directory, "a\nb", &[]),
        Err(GroupNameError::ControlCharacter)
    );
    assert_eq!(resolve_group_change(&directory, None, &[]).unwrap(), None);
    assert_eq!(
        resolve_group_change(&directory, Some(None), &[]).unwrap(),
        Some(None)
    );
    assert_eq!(
        resolve_group_change(&directory, Some(Some("board drag".into())), &[second.id]).unwrap(),
        Some(Some("Board Drag".to_string()))
    );

    // A trashed member does not pin the spelling.
    ops::update(
        &store,
        &first.id,
        ts("2026-11-02T00:00:00Z"),
        TicketPatch {
            status: Some(Status::Deleted),
            ..Default::default()
        },
    )
    .unwrap();
    let directory = GroupDirectory::scan(&store).unwrap();
    assert_eq!(
        resolve_group_name(&directory, "board drag", &[second.id]).unwrap(),
        Some("board drag".to_string())
    );
}

#[test]
fn hs2_8cdfkr_rename_moves_every_member_and_refuses_conflicts() {
    let (_dir, store) = store();
    let a = create(&store, "a", Some("Old"));
    let b = create(&store, "b", Some("old"));
    let other = create(&store, "other", Some("Taken"));
    let loose = create(&store, "loose", None);
    let trashed = create(&store, "trashed", Some("Old"));
    ops::update(
        &store,
        &trashed.id,
        ts("2026-11-02T00:00:00Z"),
        TicketPatch {
            status: Some(Status::Deleted),
            ..Default::default()
        },
    )
    .unwrap();
    let now = || ts("2026-11-05T00:00:00Z");

    assert!(matches!(
        rename_group(&store, "missing", "New", now()),
        Err(GroupError::UnknownGroup(name)) if name == "missing"
    ));
    assert!(matches!(
        rename_group(&store, "Old", " TAKEN ", now()),
        Err(GroupError::NameTaken(name)) if name == "Taken"
    ));
    assert!(matches!(
        rename_group(&store, "Old", "  ", now()),
        Err(GroupError::EmptyName)
    ));
    assert!(matches!(
        rename_group(&store, "Old", "a\nb", now()),
        Err(GroupError::InvalidName(_))
    ));
    assert_eq!(
        store.read_ticket(&a.id).unwrap().group.as_deref(),
        Some("Old")
    );

    // A change of only case and spacing of the same group is allowed, and unifies spellings.
    let cased = rename_group(&store, "old", "  OLD ", now()).unwrap();
    assert_eq!(cased.name, "OLD");
    assert_eq!(cased.write.updated, 2);
    for id in [&a.id, &b.id, &trashed.id] {
        assert_eq!(store.read_ticket(id).unwrap().group.as_deref(), Some("OLD"));
    }

    let renamed = rename_group(&store, "OLD", "New   name", now()).unwrap();
    assert_eq!(renamed.name, "New name");
    assert_eq!(renamed.write.updated, 2);
    assert_eq!(
        renamed.write.tickets.len(),
        3,
        "the trashed member follows its group"
    );
    let after = store.read_ticket(&a.id).unwrap();
    assert_eq!(after.group.as_deref(), Some("New name"));
    assert_eq!(after.updated_at, a.updated_at);
    assert_eq!(
        store.read_ticket(&other.id).unwrap().group.as_deref(),
        Some("Taken")
    );
    assert_eq!(store.read_ticket(&loose.id).unwrap().group, None);
    assert_eq!(
        groups_of(&store),
        vec![("New name".to_string(), 2), ("Taken".to_string(), 1)]
    );
    // The old name is gone; repeating the rename is a 404-class error, not a silent success.
    assert!(matches!(
        rename_group(&store, "OLD", "Else", now()),
        Err(GroupError::UnknownGroup(_))
    ));
    // Renaming to the name it already has rewrites nothing.
    let same = rename_group(&store, "new name", "New name", now()).unwrap();
    assert_eq!((same.write.updated, same.write.tickets.len()), (2, 0));
}

#[test]
fn hs2_8cdfkr_ungroup_clears_every_member_once() {
    let (_dir, store) = store();
    let a = create(&store, "a", Some("Sprint"));
    let b = create(&store, "b", Some("SPRINT"));
    let keep = create(&store, "keep", Some("Other"));
    let now = || ts("2026-11-05T00:00:00Z");

    let write = ungroup(&store, " sprint ", now()).unwrap();
    assert_eq!(write.updated, 2);
    assert_eq!(write.tickets.len(), 2);
    assert_eq!(store.read_ticket(&a.id).unwrap().group, None);
    assert_eq!(store.read_ticket(&b.id).unwrap().group, None);
    assert_eq!(
        store.read_ticket(&keep.id).unwrap().group.as_deref(),
        Some("Other")
    );
    assert!(matches!(
        ungroup(&store, "sprint", now()),
        Err(GroupError::UnknownGroup(_))
    ));
    // Refill: the name is free again.
    set_group(&store, &a.id, Some("sprint")).unwrap();
    assert_eq!(
        groups_of(&store),
        vec![("Other".to_string(), 1), ("sprint".to_string(), 1)]
    );
}

#[test]
fn hs2_8cdfkr_a_stale_directory_never_rewrites_a_ticket_that_left_the_group() {
    let (_dir, store) = store();
    let stays = create(&store, "stays", Some("Old"));
    let leaves = create(&store, "leaves", Some("Old"));
    let directory = GroupDirectory::scan(&store).unwrap();
    // Between the directory read and the write, one member moves to another group.
    set_group(&store, &leaves.id, Some("Elsewhere")).unwrap();
    let plan = plan_rename(&directory, "Old", "New").unwrap();
    let write = set_group_of_members(
        &store,
        &directory,
        &plan.from_key,
        Some(&plan.name),
        ts("2026-11-05T00:00:00Z"),
    )
    .unwrap();
    assert_eq!(write.updated, 1);
    assert_eq!(
        store.read_ticket(&stays.id).unwrap().group.as_deref(),
        Some("New")
    );
    assert_eq!(
        store.read_ticket(&leaves.id).unwrap().group.as_deref(),
        Some("Elsewhere")
    );
}

#[test]
fn hs2_pdshnn_query_filters_by_group_and_has_group() {
    let (_dir, store) = store();
    let a = create(&store, "a", Some("Board Drag"));
    let b = create(&store, "b", Some("Other"));
    let c = create(&store, "c", None);
    let ids = |query: ops::TicketQuery| -> Vec<Ulid> {
        let mut ids: Vec<_> = ops::query(&store, &query)
            .unwrap()
            .into_iter()
            .map(|ticket| ticket.id)
            .collect();
        ids.sort();
        ids
    };
    let sorted = |mut ids: Vec<Ulid>| {
        ids.sort();
        ids
    };
    assert_eq!(
        ids(ops::TicketQuery {
            group: Some("  board   DRAG".into()),
            ..Default::default()
        }),
        vec![a.id]
    );
    assert!(
        ids(ops::TicketQuery {
            group: Some("board".into()),
            ..Default::default()
        })
        .is_empty()
    );
    assert!(
        ids(ops::TicketQuery {
            group: Some("   ".into()),
            ..Default::default()
        })
        .is_empty()
    );
    assert_eq!(
        ids(ops::TicketQuery {
            has_group: Some(true),
            ..Default::default()
        }),
        sorted(vec![a.id, b.id])
    );
    assert_eq!(
        ids(ops::TicketQuery {
            has_group: Some(false),
            ..Default::default()
        }),
        vec![c.id]
    );
}
