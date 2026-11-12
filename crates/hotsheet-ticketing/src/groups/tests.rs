use super::*;
use crate::ops::{self, NewTicket, TicketPatch};
use crate::store::StoreMetadata;
use hotsheet_model::Timestamp;

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
        .summaries(None)
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

fn registry() -> (tempfile::TempDir, GroupRegistry) {
    let dir = tempfile::tempdir().unwrap();
    let registry = GroupRegistry::for_checkout(dir.path());
    (dir, registry)
}

fn trash(store: &FsStore, id: &Ulid) {
    ops::update(
        store,
        id,
        ts("2026-11-02T00:00:00Z"),
        TicketPatch {
            status: Some(Status::Deleted),
            ..Default::default()
        },
    )
    .unwrap();
}

fn group_of(store: &FsStore, id: &Ulid) -> Option<String> {
    store.read_ticket(id).unwrap().group
}

/// `(id, name, count, named, legacy, duplicate)` per listed group.
fn listing(
    stores: &[&FsStore],
    registry: Option<&GroupRegistry>,
) -> Vec<(String, String, usize, bool, bool, bool)> {
    let directory = GroupDirectory::scan_all(stores.iter().copied()).unwrap();
    list_groups(&directory, registry)
        .unwrap()
        .into_iter()
        .map(|g| (g.id, g.name, g.count, g.named, g.legacy, g.duplicate))
        .collect()
}

#[test]
fn hs2_9kzsd4_names_resolve_through_the_registry_and_stores_take_ids_only() {
    let (_dir, registry) = registry();
    // A name creates one entry; any spelling of it reuses that entry.
    let id = resolve_group_input(Some(&registry), "  Board   drag ")
        .unwrap()
        .unwrap();
    assert!(hotsheet_model::is_group_id(&id));
    assert_eq!(registry.name(&id).unwrap().as_deref(), Some("Board drag"));
    assert_eq!(
        resolve_group_input(Some(&registry), "BOARD DRAG").unwrap(),
        Some(id.clone())
    );
    // An id is used as that id, known or not.
    assert_eq!(
        resolve_group_input(Some(&registry), &id).unwrap(),
        Some(id.clone())
    );
    assert_eq!(
        resolve_group_input(None, " 7K3M9P2QXA ")
            .unwrap()
            .as_deref(),
        Some("7K3M9P2QXA")
    );
    // Without a project, a name is refused rather than written as a legacy value.
    assert!(matches!(
        resolve_group_input(None, "Sprint"),
        Err(GroupError::RegistryRequired(name)) if name == "Sprint"
    ));
    assert_eq!(resolve_group_input(None, "   ").unwrap(), None);
    assert!(matches!(
        resolve_group_input(Some(&registry), "a\nb"),
        Err(GroupError::InvalidName(_))
    ));
    assert_eq!(resolve_group_change(None, None).unwrap(), None);
    assert_eq!(resolve_group_change(None, Some(None)).unwrap(), Some(None));
    assert_eq!(
        resolve_group_change(Some(&registry), Some(Some("board drag".into()))).unwrap(),
        Some(Some(id.clone()))
    );
    assert_eq!(registry.names().unwrap().len(), 1, "no duplicate entry");
    // Create (no tickets) is the same find-or-create.
    assert_eq!(
        create_group(Some(&registry), "board DRAG").unwrap(),
        (id, "Board drag".to_string(), false)
    );
    let (other, name, created) = create_group(Some(&registry), " Fresh ").unwrap();
    assert!(created && name == "Fresh" && hotsheet_model::is_group_id(&other));
    assert!(matches!(
        create_group(Some(&registry), "  "),
        Err(GroupError::EmptyName)
    ));
    assert!(matches!(
        create_group(None, "x"),
        Err(GroupError::RegistryRequired(_))
    ));
}

#[test]
fn hs2_9kzsd4_listing_names_ids_flags_placeholders_legacy_and_duplicates() {
    let (_dir, store) = store();
    let (_reg, registry) = registry();
    let sprint = registry.find_or_create("Sprint").unwrap().0;
    // Empty, then refilled: a registry entry alone is not a visible group.
    assert!(listing(&[&store], Some(&registry)).is_empty());
    let a = create(&store, "a", Some(&sprint));
    create(&store, "b", Some(&sprint));
    // An id this project does not name (shared store, registry not pulled yet).
    create(&store, "c", Some("7K3M9P2QXA"));
    // An unmigrated name.
    create(&store, "d", Some("Legacy work"));
    assert_eq!(
        listing(&[&store], Some(&registry)),
        vec![
            (
                "7K3M9P2QXA".into(),
                "Group 7K3M9P2QXA".into(),
                1,
                false,
                false,
                false
            ),
            (
                "Legacy work".into(),
                "Legacy work".into(),
                1,
                true,
                true,
                false
            ),
            (sprint.clone(), "Sprint".into(), 2, true, false, false),
        ]
    );
    // A store with no project lists ids under placeholders.
    assert_eq!(
        listing(&[&store], None)
            .into_iter()
            .map(|g| (g.1, g.3))
            .collect::<Vec<_>>(),
        vec![
            ("Group 7K3M9P2QXA".to_string(), false),
            (format!("Group {sprint}"), false),
            ("Legacy work".to_string(), true),
        ]
    );
    // Two clones each created "sprint": both are listed and flagged; the placeholder and
    // a legacy name with another key are not duplicates.
    registry.set_name("0000000001", "SPRINT").unwrap();
    create(&store, "e", Some("0000000001"));
    let listed = listing(&[&store], Some(&registry));
    let duplicates: Vec<_> = listed.iter().filter(|g| g.5).map(|g| g.0.clone()).collect();
    assert_eq!(duplicates, vec!["0000000001".to_string(), sprint.clone()]);
    // A legacy name with the same key as a registry name is a duplicate too.
    create(&store, "f", Some("sprint"));
    assert_eq!(
        listing(&[&store], Some(&registry))
            .iter()
            .filter(|g| g.5)
            .count(),
        3
    );
    // Trash does not keep a group listed.
    trash(&store, &a.id);
    assert_eq!(
        listing(&[&store], Some(&registry))
            .iter()
            .find(|g| g.0 == sprint)
            .map(|g| g.2),
        Some(1)
    );
}

#[test]
fn hs2_9kzsd4_find_group_by_id_name_and_refuses_ambiguity() {
    let (_dir, store) = store();
    let (_reg, registry) = registry();
    let one = registry.find_or_create("Sprint").unwrap().0;
    registry.set_name("0000000001", "sprint").unwrap();
    create(&store, "a", Some(&one));
    create(&store, "b", Some("0000000001"));
    create(&store, "c", Some("Old Name"));
    let directory = GroupDirectory::scan(&store).unwrap();
    let names = registry.names().unwrap();
    assert_eq!(find_group(&directory, Some(&names), &one).unwrap().key, one);
    assert!(matches!(
        find_group(&directory, Some(&names), " SPRINT "),
        Err(GroupError::Ambiguous(_))
    ));
    let legacy = find_group(&directory, Some(&names), "old   name").unwrap();
    assert_eq!(legacy.key, "old name");
    assert!(legacy.summary.legacy);
    assert!(matches!(
        find_group(&directory, Some(&names), "missing"),
        Err(GroupError::UnknownGroup(name)) if name == "missing"
    ));
    assert!(matches!(
        find_group(&directory, Some(&names), "  "),
        Err(GroupError::UnknownGroup(_))
    ));
    // An id with no live member is not a group.
    assert!(matches!(
        find_group(&directory, Some(&names), "ZZZZZZZZZZ"),
        Err(GroupError::UnknownGroup(_))
    ));
}

#[test]
fn hs2_9kzsd4_rename_is_a_registry_edit_that_touches_no_ticket() {
    let (_dir, store) = store();
    let (_reg, registry) = registry();
    let old = registry.find_or_create("Old").unwrap().0;
    let taken = registry.find_or_create("Taken").unwrap().0;
    let a = create(&store, "a", Some(&old));
    let trashed = create(&store, "trashed", Some(&old));
    trash(&store, &trashed.id);
    create(&store, "other", Some(&taken));
    let directory = GroupDirectory::scan(&store).unwrap();
    let rename = |group: &str, to: &str| {
        let directory = GroupDirectory::scan(&store).unwrap();
        rename_group(&[&store], &directory, Some(&registry), group, to)
    };

    assert!(matches!(
        rename("missing", "New"),
        Err(GroupError::UnknownGroup(_))
    ));
    assert!(matches!(rename(&old, " TAKEN "), Err(GroupError::NameTaken(n)) if n == "Taken"));
    assert!(matches!(rename(&old, "  "), Err(GroupError::EmptyName)));
    assert!(matches!(
        rename(&old, "a\nb"),
        Err(GroupError::InvalidName(_))
    ));
    assert!(matches!(
        rename_group(&[&store], &directory, None, &old, "x"),
        Err(GroupError::RegistryRequired(_))
    ));

    let before = store.read_ticket(&a.id).unwrap();
    let renamed = rename("old", "New   name").unwrap();
    assert_eq!(renamed.id.as_deref(), Some(old.as_str()));
    assert_eq!(renamed.name.as_deref(), Some("New name"));
    assert_eq!(renamed.updated, 0);
    assert!(renamed.writes.iter().all(|write| write.tickets.is_empty()));
    assert_eq!(
        store.read_ticket(&a.id).unwrap(),
        before,
        "no ticket changes"
    );
    assert_eq!(group_of(&store, &trashed.id).as_deref(), Some(old.as_str()));
    assert_eq!(registry.name(&old).unwrap().as_deref(), Some("New name"));
    // Only case and spacing of its own name: allowed. Repeating is idempotent.
    assert_eq!(
        rename(&old, "NEW NAME").unwrap().name.as_deref(),
        Some("NEW NAME")
    );
    assert_eq!(
        rename(&old, "NEW NAME").unwrap().name.as_deref(),
        Some("NEW NAME")
    );
    // Renaming to the name of an unlisted (memberless) entry is allowed.
    registry.find_or_create("Unused").unwrap();
    assert_eq!(
        rename(&old, "unused").unwrap().name.as_deref(),
        Some("unused")
    );
}

#[test]
fn hs2_9kzsd4_rename_names_a_placeholder_and_converts_a_legacy_group() {
    let (_dir, store) = store();
    let (_reg, registry) = registry();
    let shared = create(&store, "shared", Some("7K3M9P2QXA"));
    let rename = |group: &str, to: &str| {
        let directory = GroupDirectory::scan(&store).unwrap();
        rename_group(&[&store], &directory, Some(&registry), group, to)
    };
    // A placeholder can be found by its id (or its placeholder name) and named here.
    let named = rename("Group 7K3M9P2QXA", "Shared work").unwrap();
    assert_eq!(named.id.as_deref(), Some("7K3M9P2QXA"));
    assert_eq!(group_of(&store, &shared.id).as_deref(), Some("7K3M9P2QXA"));
    assert_eq!(
        registry.name("7K3M9P2QXA").unwrap().as_deref(),
        Some("Shared work")
    );

    // A legacy group gets a registry entry and its members (Trash too) move onto it.
    let a = create(&store, "a", Some("Old"));
    let b = create(&store, "b", Some("old"));
    let gone = create(&store, "gone", Some("OLD"));
    trash(&store, &gone.id);
    let converted = rename("old", "Renamed").unwrap();
    let id = converted.id.clone().unwrap();
    assert!(hotsheet_model::is_group_id(&id));
    assert_eq!(converted.updated, 2);
    assert_eq!(converted.writes[0].tickets.len(), 3);
    for ticket in [&a, &b, &gone] {
        assert_eq!(group_of(&store, &ticket.id), Some(id.clone()));
    }
    for ticket in [&a, &b] {
        assert_eq!(
            store.read_ticket(&ticket.id).unwrap().updated_at,
            ticket.updated_at
        );
    }
    assert_eq!(registry.name(&id).unwrap().as_deref(), Some("Renamed"));
}

#[test]
fn hs2_9kzsd4_ungroup_clears_members_and_keeps_the_registry_entry() {
    let (_dir, store) = store();
    let (_reg, registry) = registry();
    let sprint = registry.find_or_create("Sprint").unwrap().0;
    let a = create(&store, "a", Some(&sprint));
    let b = create(&store, "b", Some(&sprint));
    let keep = create(&store, "keep", Some("Other"));
    let ungroup_named = |group: &str| {
        let directory = GroupDirectory::scan(&store).unwrap();
        ungroup(&[&store], &directory, Some(&registry), group)
    };
    let outcome = ungroup_named(" sprint ").unwrap();
    assert_eq!(outcome.updated, 2);
    assert_eq!(outcome.writes[0].tickets.len(), 2);
    assert_eq!(group_of(&store, &a.id), None);
    assert_eq!(group_of(&store, &b.id), None);
    assert_eq!(group_of(&store, &keep.id).as_deref(), Some("Other"));
    assert!(matches!(
        ungroup_named("sprint"),
        Err(GroupError::UnknownGroup(_))
    ));
    // The entry stays, so the name comes back with the same id.
    assert_eq!(
        resolve_group_input(Some(&registry), "Sprint").unwrap(),
        Some(sprint.clone())
    );
    // Legacy groups ungroup by name too, and a store with no project ungroups by id.
    let directory = GroupDirectory::scan(&store).unwrap();
    assert_eq!(
        ungroup(&[&store], &directory, None, "other")
            .unwrap()
            .updated,
        1
    );
    set_group(&store, &a.id, Some(&sprint)).unwrap();
    let directory = GroupDirectory::scan(&store).unwrap();
    assert_eq!(
        ungroup(&[&store], &directory, None, &sprint)
            .unwrap()
            .updated,
        1
    );
}

#[test]
fn hs2_9kzsd4_merge_moves_duplicate_members_onto_one_id() {
    let (_dir, store) = store();
    let (_reg, registry) = registry();
    let keep = registry.find_or_create("Sprint").unwrap().0;
    registry.set_name("0000000001", "sprint").unwrap();
    let a = create(&store, "a", Some(&keep));
    let b = create(&store, "b", Some("0000000001"));
    let c = create(&store, "c", Some("0000000001"));
    trash(&store, &c.id);
    create(&store, "legacy", Some("Legacy"));
    let merge = |from: &str, into: &str| {
        let directory = GroupDirectory::scan(&store).unwrap();
        merge_groups(&[&store], &directory, Some(&registry), from, into)
    };
    assert!(matches!(
        merge(&keep, &keep),
        Err(GroupError::InvalidMerge(_))
    ));
    assert!(matches!(
        merge(&keep, "legacy"),
        Err(GroupError::InvalidMerge(_))
    ));
    assert!(matches!(
        merge("sprint", &keep),
        Err(GroupError::Ambiguous(_))
    ));
    assert!(matches!(
        merge("nope", &keep),
        Err(GroupError::UnknownGroup(_))
    ));

    let merged = merge("0000000001", &keep).unwrap();
    assert_eq!(merged.id.as_deref(), Some(keep.as_str()));
    assert_eq!(merged.name.as_deref(), Some("Sprint"));
    assert_eq!(merged.updated, 1);
    assert_eq!(
        merged.writes[0].tickets.len(),
        2,
        "the trashed member follows"
    );
    for id in [&a.id, &b.id, &c.id] {
        assert_eq!(group_of(&store, id), Some(keep.clone()));
    }
    let listed = listing(&[&store], Some(&registry));
    assert!(listed.iter().all(|g| !g.5), "no duplicates remain");
    // Neither registry entry was deleted.
    assert!(registry.name("0000000001").unwrap().is_some());
    // A legacy group can be merged into an id group by name.
    let merged = merge("legacy", &keep).unwrap();
    assert_eq!(merged.updated, 1);
    assert_eq!(listing(&[&store], Some(&registry)).len(), 1);
}

#[test]
fn hs2_9kzsd4_migration_is_explicit_idempotent_and_covers_every_state() {
    // Empty store and empty registry: nothing to do.
    let (_dir, empty) = store();
    let (_reg, registry) = registry();
    let directory = GroupDirectory::scan(&empty).unwrap();
    let outcome = migrate_groups(&[&empty], &directory, &registry).unwrap();
    assert_eq!(
        (outcome.created, outcome.reused, outcome.updated),
        (0, 0, 0)
    );
    assert!(registry.names().unwrap().is_empty());

    // Legacy only, mixed spellings and Trash, plus an id group, a registry entry that
    // already names one legacy key, and an ungrouped ticket.
    let (_dir, store) = store();
    let first = create(&store, "first", Some("Board Drag"));
    let second = create(&store, "second", Some("board  drag"));
    let trashed = create(&store, "trashed", Some("BOARD DRAG"));
    trash(&store, &trashed.id);
    let other = create(&store, "other", Some("Search"));
    let existing = registry.find_or_create("search").unwrap().0;
    let modern = registry.find_or_create("Modern").unwrap().0;
    let already = create(&store, "already", Some(&modern));
    let loose = create(&store, "loose", None);
    let unchanged_already = store.read_ticket(&already.id).unwrap();

    let directory = GroupDirectory::scan(&store).unwrap();
    assert!(directory.has_legacy());
    let outcome = migrate_groups(&[&store], &directory, &registry).unwrap();
    assert_eq!(
        (outcome.created, outcome.reused, outcome.updated),
        (1, 1, 4)
    );
    let board = group_of(&store, &first.id).unwrap();
    assert!(hotsheet_model::is_group_id(&board));
    assert_eq!(
        registry.name(&board).unwrap().as_deref(),
        Some("Board Drag")
    );
    for ticket in [&second, &trashed] {
        assert_eq!(group_of(&store, &ticket.id), Some(board.clone()));
    }
    assert_eq!(group_of(&store, &other.id), Some(existing.clone()));
    assert_eq!(store.read_ticket(&already.id).unwrap(), unchanged_already);
    assert_eq!(group_of(&store, &loose.id), None);
    for ticket in [&first, &second, &other] {
        assert_eq!(
            store.read_ticket(&ticket.id).unwrap().updated_at,
            ticket.updated_at,
            "migration keeps updated_at"
        );
    }
    let directory = GroupDirectory::scan(&store).unwrap();
    assert!(!directory.has_legacy());
    assert_eq!(
        listing(&[&store], Some(&registry))
            .into_iter()
            .map(|g| (g.1, g.2))
            .collect::<Vec<_>>(),
        vec![
            ("Board Drag".to_string(), 2),
            ("Modern".to_string(), 1),
            ("search".to_string(), 1)
        ]
    );

    // Rerun: no legacy value, no registry change, no write.
    let entries = registry.names().unwrap();
    let rerun = migrate_groups(&[&store], &directory, &registry).unwrap();
    assert_eq!((rerun.created, rerun.reused, rerun.updated), (0, 0, 0));
    assert_eq!(registry.names().unwrap(), entries);

    // A legacy value written later (an old client) migrates onto the same entry.
    let late = create(&store, "late", Some("board drag"));
    let directory = GroupDirectory::scan(&store).unwrap();
    let again = migrate_groups(&[&store], &directory, &registry).unwrap();
    assert_eq!((again.created, again.reused, again.updated), (0, 1, 1));
    assert_eq!(group_of(&store, &late.id), Some(board));
}

#[test]
fn hs2_9kzsd4_one_namespace_spans_several_stores() {
    let (_a, alpha) = store();
    let (_b, beta) = store();
    let (_reg, registry) = registry();
    let x = create(&alpha, "x", Some("Shared"));
    let y = create(&beta, "y", Some("shared"));
    let stores = [&alpha, &beta];
    let directory = GroupDirectory::scan_all(stores.iter().copied()).unwrap();
    let migrated = migrate_groups(&stores, &directory, &registry).unwrap();
    assert_eq!((migrated.created, migrated.updated), (1, 2));
    let id = group_of(&alpha, &x.id).unwrap();
    assert_eq!(group_of(&beta, &y.id), Some(id.clone()));
    assert_eq!(
        listing(&stores, Some(&registry)),
        vec![(id.clone(), "Shared".into(), 2, true, false, false)]
    );
    let directory = GroupDirectory::scan_all(stores.iter().copied()).unwrap();
    let ungrouped = ungroup(&stores, &directory, Some(&registry), "shared").unwrap();
    assert_eq!(ungrouped.updated, 2);
    assert_eq!(
        ungrouped
            .writes
            .iter()
            .map(|w| w.tickets.len())
            .collect::<Vec<_>>(),
        vec![1, 1]
    );
}

#[test]
fn hs2_8cdfkr_a_stale_directory_never_rewrites_a_ticket_that_left_the_group() {
    let (_dir, store) = store();
    let (_reg, registry) = registry();
    let old = registry.find_or_create("Old").unwrap().0;
    let elsewhere = registry.find_or_create("Elsewhere").unwrap().0;
    let stays = create(&store, "stays", Some(&old));
    let leaves = create(&store, "leaves", Some(&old));
    let directory = GroupDirectory::scan(&store).unwrap();
    // Between the directory read and the write, one member moves to another group.
    set_group(&store, &leaves.id, Some(&elsewhere)).unwrap();
    let outcome = ungroup(&[&store], &directory, Some(&registry), &old).unwrap();
    assert_eq!(outcome.updated, 1);
    assert_eq!(group_of(&store, &stays.id), None);
    assert_eq!(group_of(&store, &leaves.id), Some(elsewhere));
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

    // HS2-9KZSD4: an id matches by id; a name matches the ids a registry resolves it to.
    let d = create(&store, "d", Some("7K3M9P2QXA"));
    assert_eq!(
        ids(ops::TicketQuery {
            group: Some(" 7K3M9P2QXA ".into()),
            ..Default::default()
        }),
        vec![d.id]
    );
    assert!(
        ids(ops::TicketQuery {
            group: Some("Sprint".into()),
            ..Default::default()
        })
        .is_empty(),
        "a name alone does not match an id group"
    );
    assert_eq!(
        ids(ops::TicketQuery {
            group: Some("Sprint".into()),
            group_ids: vec!["7K3M9P2QXA".into()],
            ..Default::default()
        }),
        vec![d.id]
    );
    assert_eq!(
        ids(ops::TicketQuery {
            group: Some("board drag".into()),
            group_ids: vec!["7K3M9P2QXA".into()],
            ..Default::default()
        }),
        sorted(vec![a.id, d.id]),
        "legacy names and resolved ids both match"
    );
}
