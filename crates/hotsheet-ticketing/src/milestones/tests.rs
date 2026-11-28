//! HS2-9PYNCN: the milestone registry's format, rules, and lock (docs/38 §38.2, §38.10
//! "Registry unit"). Every test uses its own temporary checkout without Git, so no write is
//! committed; publication is covered in `git_tests.rs`.

use super::*;

fn registry() -> (tempfile::TempDir, MilestoneRegistry) {
    let dir = tempfile::tempdir().unwrap();
    let registry = MilestoneRegistry::for_checkout(dir.path());
    (dir, registry)
}

fn file_text(registry: &MilestoneRegistry, id: &str) -> String {
    fs::read_to_string(registry.entry_path(id)).unwrap()
}

#[test]
fn hs2_9pyncn_round_trip_writes_the_documented_layout_in_key_order() {
    let (dir, registry) = registry();
    assert!(registry.list().unwrap().is_empty(), "no directory yet");
    let created = registry
        .create(
            "  Toolchain   2.0 ",
            Some("Ship the new build toolchain to every package."),
            Some("2026-12-04"),
        )
        .unwrap();
    assert!(is_milestone_id(&created.id));
    assert_eq!(created.title, "Toolchain 2.0");
    assert_eq!(created.state, MilestoneState::Open);
    assert_eq!(
        file_text(&registry, &created.id),
        "{\n  \"title\": \"Toolchain 2.0\",\n  \"description\": \"Ship the new build toolchain to \
         every package.\",\n  \"due_on\": \"2026-12-04\",\n  \"state\": \"open\"\n}\n"
    );
    // Closing adds the two closed keys after `state`, one per line.
    let closed = registry.close(&created.id, CloseReason::Completed).unwrap();
    assert!(closed.changed);
    let at = closed.milestone.closed_at.clone().unwrap();
    assert!(at.ends_with('Z') && at.len() == 20, "{at}");
    assert_eq!(
        file_text(&registry, &created.id),
        format!(
            "{{\n  \"title\": \"Toolchain 2.0\",\n  \"description\": \"Ship the new build \
             toolchain to every package.\",\n  \"due_on\": \"2026-12-04\",\n  \"state\": \
             \"closed\",\n  \"closed_reason\": \"completed\",\n  \"closed_at\": \"{at}\"\n}}\n"
        )
    );
    // Reading it back gives the same record, and the lock stays outside the registry.
    assert_eq!(
        registry.get(&created.id).unwrap().unwrap(),
        closed.milestone
    );
    assert!(dir.path().join(MILESTONE_REGISTRY_LOCK).exists());
    assert!(!registry.dir().join("milestones.lock").exists());
    // The serialized shape omits unset fields.
    let open = registry.create("Bare", None, None).unwrap();
    assert_eq!(
        file_text(&registry, &open.id),
        "{\n  \"title\": \"Bare\",\n  \"state\": \"open\"\n}\n"
    );
    let wire = serde_json::to_value(&open).unwrap();
    assert_eq!(
        wire,
        serde_json::json!({"id": open.id, "title": "Bare", "state": "open", "duplicate": false, "named": true})
    );
}

#[test]
fn hs2_9pyncn_unknown_keys_survive_every_write_verbatim() {
    let (_dir, registry) = registry();
    let id = "7K3M9P2QXA";
    fs::create_dir_all(registry.dir()).unwrap();
    // A newer build's file: an unknown scalar, an unknown nested value, and known keys out
    // of order.
    fs::write(
        registry.entry_path(id),
        "{\"state\":\"open\",\"zeta\":{\"list\":[1,{\"deep\":true}],\"n\":null},\"title\":\"Alpha\",\"color\":\"teal\"}",
    )
    .unwrap();
    registry
        .edit(
            id,
            &MilestoneEdit {
                due_on: Some(Some("2027-01-31".into())),
                ..MilestoneEdit::default()
            },
        )
        .unwrap();
    let text = file_text(&registry, id);
    assert!(
        text.starts_with(
            "{\n  \"title\": \"Alpha\",\n  \"due_on\": \"2027-01-31\",\n  \"state\": \"open\",\n"
        ),
        "{text}"
    );
    let value: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["color"], "teal");
    assert_eq!(
        value["zeta"],
        serde_json::json!({"list": [1, {"deep": true}], "n": null})
    );
    // Close, reopen, and rename keep them too; every line is one key or nested JSON.
    registry.close(id, CloseReason::NotPlanned).unwrap();
    registry.reopen(id).unwrap();
    registry
        .edit(
            id,
            &MilestoneEdit {
                title: Some("Beta".into()),
                ..MilestoneEdit::default()
            },
        )
        .unwrap();
    let value: Value = serde_json::from_str(&file_text(&registry, id)).unwrap();
    assert_eq!(value["color"], "teal");
    assert_eq!(value["zeta"]["list"][1]["deep"], true);
    assert_eq!(value["title"], "Beta");
    assert!(value.get("closed_reason").is_none() && value.get("closed_at").is_none());
}

#[test]
fn hs2_9pyncn_title_rules_and_uniqueness() {
    let (_dir, registry) = registry();
    for (input, expected) in [
        ("", "a milestone needs a title"),
        ("   ", "a milestone needs a title"),
        ("a\nb", "one line"),
        ("tab\there", "one line"),
    ] {
        let error = registry.create(input, None, None).unwrap_err();
        assert_eq!(error.code(), "invalid_milestone_title", "{input:?}");
        assert!(error.to_string().contains(expected), "{error}");
    }
    let long = "x".repeat(121);
    assert_eq!(
        registry.create(&long, None, None).unwrap_err().code(),
        "invalid_milestone_title"
    );
    let at_limit = registry.create(&"y".repeat(120), None, None).unwrap();
    assert_eq!(at_limit.title.chars().count(), 120);

    let alpha = registry.create("Release  Alpha", None, None).unwrap();
    // Same title in another case or spacing is taken.
    let taken = registry.create(" release alpha ", None, None).unwrap_err();
    assert_eq!(taken.code(), "milestone_title_taken");
    assert!(taken.to_string().contains(&alpha.id), "{taken}");
    // Renaming onto another milestone's title is refused; recasing its own is allowed.
    let beta = registry.create("Beta", None, None).unwrap();
    let rename = |id: &str, title: &str| {
        registry.edit(
            id,
            &MilestoneEdit {
                title: Some(title.into()),
                ..MilestoneEdit::default()
            },
        )
    };
    assert_eq!(
        rename(&beta.id, "RELEASE ALPHA").unwrap_err().code(),
        "milestone_title_taken"
    );
    let recased = rename(&alpha.id, "release ALPHA").unwrap();
    assert!(recased.changed);
    assert_eq!(recased.milestone.title, "release ALPHA");
    // A closed milestone's title stays taken.
    registry.close(&alpha.id, CloseReason::Completed).unwrap();
    assert_eq!(
        registry
            .create("Release Alpha", None, None)
            .unwrap_err()
            .code(),
        "milestone_title_taken"
    );
    // Resolution by id or by title in any case or spacing.
    assert_eq!(registry.resolve(&beta.id).unwrap().id, beta.id);
    assert_eq!(registry.resolve("  beta ").unwrap().id, beta.id);
    assert_eq!(
        registry.resolve("Gamma").unwrap_err().code(),
        "milestone_not_found"
    );
}

#[test]
fn hs2_9pyncn_two_clones_creating_one_title_are_both_listed_as_duplicates() {
    let (_dir, registry) = registry();
    fs::create_dir_all(registry.dir()).unwrap();
    // The merge of two clones kept both files.
    fs::write(
        registry.entry_path("0000000001"),
        "{\"title\":\"Sprint\",\"state\":\"open\"}",
    )
    .unwrap();
    fs::write(
        registry.entry_path("ZZZZZZZZZZ"),
        "{\"title\":\"sprint\",\"state\":\"closed\"}",
    )
    .unwrap();
    let other = registry.create("Other", None, None).unwrap();
    let listed = registry.list().unwrap();
    let flags: Vec<(&str, bool)> = listed
        .iter()
        .map(|milestone| (milestone.id.as_str(), milestone.duplicate))
        .collect();
    assert_eq!(
        flags,
        vec![
            (other.id.as_str(), false),
            ("0000000001", true),
            ("ZZZZZZZZZZ", true)
        ]
    );
    // A closed file without a reason reads as closed as completed.
    assert_eq!(listed[2].closed_reason, Some(CloseReason::Completed));
    // A duplicated title needs the id.
    assert_eq!(
        registry.resolve("SPRINT").unwrap_err().code(),
        "milestone_ambiguous"
    );
    assert!(registry.resolve("0000000001").unwrap().duplicate);
    // Removing one clears the flag on the other.
    assert!(registry.remove("ZZZZZZZZZZ").unwrap().is_some());
    assert!(!registry.resolve("SPRINT").unwrap().duplicate);
    assert!(registry.remove("ZZZZZZZZZZ").unwrap().is_none());
}

#[test]
fn hs2_9pyncn_close_reopen_sequences_add_and_remove_the_closed_keys() {
    let (_dir, registry) = registry();
    let id = registry.create("Cycle", None, None).unwrap().id;
    // close -> close again (no-op) -> other reason (keeps closed_at) -> reopen -> reopen
    // again (no-op) -> close -> reopen.
    let first = registry.close(&id, CloseReason::Completed).unwrap();
    assert!(first.changed);
    let at = first.milestone.closed_at.clone();
    let repeat = registry.close(&id, CloseReason::Completed).unwrap();
    assert!(!repeat.changed);
    assert_eq!(repeat.milestone, first.milestone);
    let other = registry.close(&id, CloseReason::NotPlanned).unwrap();
    assert!(other.changed);
    assert_eq!(other.milestone.closed_reason, Some(CloseReason::NotPlanned));
    assert_eq!(other.milestone.closed_at, at);
    let reopened = registry.reopen(&id).unwrap();
    assert!(reopened.changed);
    assert_eq!(reopened.milestone.state, MilestoneState::Open);
    assert_eq!(reopened.milestone.closed_reason, None);
    assert_eq!(reopened.milestone.closed_at, None);
    assert_eq!(
        file_text(&registry, &id),
        "{\n  \"title\": \"Cycle\",\n  \"state\": \"open\"\n}\n"
    );
    assert!(!registry.reopen(&id).unwrap().changed);
    assert!(
        registry
            .close(&id, CloseReason::NotPlanned)
            .unwrap()
            .changed
    );
    assert!(registry.reopen(&id).unwrap().changed);
    // Unknown ids and reasons.
    assert_eq!(
        registry
            .close("7K3M9P2QXA", CloseReason::Completed)
            .unwrap_err()
            .code(),
        "milestone_not_found"
    );
    assert_eq!(
        registry.reopen("nope").unwrap_err().code(),
        "milestone_not_found"
    );
    assert_eq!(
        CloseReason::parse("done").unwrap_err().code(),
        "invalid_milestone_close_reason"
    );
    assert_eq!(
        CloseReason::parse("not-planned").unwrap(),
        CloseReason::NotPlanned
    );
}

#[test]
fn hs2_9pyncn_edit_sets_clears_and_ignores_repeats() {
    let (_dir, registry) = registry();
    let id = registry.create("Edits", None, None).unwrap().id;
    let set = |edit: MilestoneEdit| registry.edit(&id, &edit).unwrap();
    let dated = set(MilestoneEdit {
        due_on: Some(Some(" 2028-02-29 ".into())),
        ..MilestoneEdit::default()
    });
    assert!(dated.changed);
    assert_eq!(dated.milestone.due_on.as_deref(), Some("2028-02-29"));
    // The same value again, an empty edit, and an empty description on none all no-op.
    for edit in [
        MilestoneEdit {
            due_on: Some(Some("2028-02-29".into())),
            ..MilestoneEdit::default()
        },
        MilestoneEdit::default(),
        MilestoneEdit {
            description: Some(Some("  ".into())),
            ..MilestoneEdit::default()
        },
        MilestoneEdit {
            title: Some(" Edits ".into()),
            ..MilestoneEdit::default()
        },
    ] {
        assert!(!set(edit).changed);
    }
    let described = set(MilestoneEdit {
        description: Some(Some("Line one\n\n- item".into())),
        ..MilestoneEdit::default()
    });
    assert_eq!(
        described.milestone.description.as_deref(),
        Some("Line one\n\n- item")
    );
    let cleared = set(MilestoneEdit {
        description: Some(None),
        due_on: Some(None),
        ..MilestoneEdit::default()
    });
    assert!(cleared.changed);
    assert_eq!(cleared.milestone.description, None);
    assert_eq!(cleared.milestone.due_on, None);
    for bad in [
        "2026-02-30",
        "2026-13-01",
        "26-01-01",
        "2026-1-01",
        "tomorrow",
        "",
    ] {
        let error = registry
            .edit(
                &id,
                &MilestoneEdit {
                    due_on: Some(Some(bad.into())),
                    ..MilestoneEdit::default()
                },
            )
            .unwrap_err();
        assert_eq!(error.code(), "invalid_due_on", "{bad}");
    }
    assert_eq!(
        registry
            .create("Bad date", None, Some("2026-02-29"))
            .unwrap_err()
            .code(),
        "invalid_due_on"
    );
}

#[test]
fn hs2_9pyncn_conflict_markers_and_unparseable_files_read_as_conflicted_and_refuse_writes() {
    let (_dir, registry) = registry();
    let kept = registry.create("Kept", None, None).unwrap();
    let dir = registry.dir();
    let cases = [
        (
            "7K3M9P2QX1",
            "{\n<<<<<<< HEAD\n  \"title\": \"Ours\",\n=======\n  \"title\": \"Theirs\",\n>>>>>>> branch\n  \"state\": \"open\"\n}\n",
            "conflict markers",
        ),
        ("7K3M9P2QX2", "{ not json", "not valid JSON"),
        ("7K3M9P2QX3", "[1, 2]", "not a JSON object"),
        ("7K3M9P2QX4", "{\"state\":\"open\"}", "no \"title\""),
        (
            "7K3M9P2QX5",
            "{\"title\":\"T\",\"state\":\"paused\"}",
            "invalid \"state\"",
        ),
        (
            "7K3M9P2QX6",
            "{\"title\":\"T\",\"state\":\"open\",\"due_on\":\"soon\"}",
            "invalid \"due_on\"",
        ),
        (
            "7K3M9P2QX7",
            "{\"title\":\"T\",\"state\":\"closed\",\"closed_reason\":\"won\"}",
            "invalid \"closed_reason\"",
        ),
        (
            "7K3M9P2QX8",
            "{\"title\":\"T\",\"state\":\"closed\",\"closed_at\":\"yesterday\"}",
            "invalid \"closed_at\"",
        ),
    ];
    for (id, text, _) in cases {
        fs::write(dir.join(format!("{id}.json")), text).unwrap();
    }
    // Foreign files are not milestones at all.
    fs::write(dir.join("README.md"), "notes").unwrap();
    fs::write(dir.join("not-an-id.json"), "{}").unwrap();
    let listed = registry.list().unwrap();
    assert_eq!(listed.len(), 1 + cases.len());
    assert_eq!(registry.resolve("Kept").unwrap().id, kept.id);
    for (id, text, detail) in cases {
        let milestone = registry.get(id).unwrap().unwrap();
        assert_eq!(milestone.state, MilestoneState::Conflicted, "{id}");
        assert_eq!(milestone.title, format!("Milestone {id}"));
        assert!(!milestone.duplicate);
        let conflict = milestone.conflict.unwrap();
        assert_eq!(conflict.file, format!(".hotsheet2/milestones/{id}.json"));
        assert!(
            conflict.detail.contains(detail),
            "{id}: {}",
            conflict.detail
        );
        // Every write is refused and the file is untouched.
        let refusals = [
            registry
                .edit(
                    id,
                    &MilestoneEdit {
                        title: Some("Fixed".into()),
                        ..MilestoneEdit::default()
                    },
                )
                .unwrap_err(),
            registry.close(id, CloseReason::Completed).unwrap_err(),
            registry.reopen(id).unwrap_err(),
            registry.remove(id).unwrap_err(),
        ];
        for error in refusals {
            assert_eq!(error.code(), "milestone_registry_conflict", "{id}");
            assert!(error.to_string().contains(&conflict.file), "{error}");
        }
        assert_eq!(
            fs::read_to_string(dir.join(format!("{id}.json"))).unwrap(),
            text
        );
        // A conflicted milestone is reached by id; its placeholder is no title.
        assert_eq!(registry.resolve(id).unwrap().id, id);
    }
    assert_eq!(
        registry.resolve("Milestone 7K3M9P2QX1").unwrap_err().code(),
        "milestone_not_found"
    );
    // A conflicted file does not reserve a title: "Ours" is free.
    registry.create("Ours", None, None).unwrap();
    // Resolving the conflict by hand makes it writable again.
    fs::write(
        dir.join("7K3M9P2QX1.json"),
        "{\n  \"title\": \"Theirs\",\n  \"state\": \"open\"\n}\n",
    )
    .unwrap();
    assert_eq!(
        registry.get("7K3M9P2QX1").unwrap().unwrap().state,
        MilestoneState::Open
    );
    assert!(
        registry
            .close("7K3M9P2QX1", CloseReason::Completed)
            .unwrap()
            .changed
    );
}

#[test]
fn hs2_9pyncn_the_lock_serializes_concurrent_creates_of_one_title() {
    let (_dir, registry) = registry();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
    let handles: Vec<_> = (0..8)
        .map(|round| {
            let registry = registry.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                // Every thread races for "Shared"; each also creates its own title.
                let shared = registry.create(
                    if round % 2 == 0 { "Shared" } else { " shared " },
                    None,
                    None,
                );
                let own = registry.create(&format!("Own {round}"), None, None);
                (shared.map(|m| m.id), own.map(|m| m.id))
            })
        })
        .collect();
    let results: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();
    let winners: Vec<_> = results
        .iter()
        .filter(|(shared, _)| shared.is_ok())
        .collect();
    assert_eq!(winners.len(), 1, "exactly one create of a title wins");
    for (shared, own) in &results {
        if let Err(error) = shared {
            assert_eq!(error.code(), "milestone_title_taken");
        }
        assert!(own.is_ok());
    }
    let listed = registry.list().unwrap();
    assert_eq!(listed.len(), 9);
    assert!(listed.iter().all(|milestone| !milestone.duplicate));
}

#[test]
fn hs2_9pyncn_concurrent_edits_of_one_milestone_each_apply_whole() {
    let (_dir, registry) = registry();
    let id = registry.create("Busy", None, None).unwrap().id;
    let handles: Vec<_> = (0..6)
        .map(|round| {
            let registry = registry.clone();
            let id = id.clone();
            std::thread::spawn(move || {
                for _ in 0..5 {
                    if round % 2 == 0 {
                        registry.close(&id, CloseReason::Completed).unwrap();
                        registry.reopen(&id).unwrap();
                    } else {
                        registry
                            .edit(
                                &id,
                                &MilestoneEdit {
                                    due_on: Some(Some(format!("2027-0{}-01", round + 1))),
                                    ..MilestoneEdit::default()
                                },
                            )
                            .unwrap();
                    }
                }
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }
    // Never torn: the file always parses as a whole milestone.
    let milestone = registry.get(&id).unwrap().unwrap();
    assert_eq!(milestone.state, MilestoneState::Open);
    assert!(milestone.due_on.is_some());
    assert!(parse_entry(&file_text(&registry, &id)).is_ok());
}

#[test]
fn hs2_9pyncn_list_orders_by_due_date_then_title_with_undated_last() {
    let (_dir, registry) = registry();
    let undated_b = registry.create("b undated", None, None).unwrap();
    let late = registry.create("Late", None, Some("2027-06-01")).unwrap();
    let undated_a = registry.create("A undated", None, None).unwrap();
    let early = registry.create("Early", None, Some("2026-12-01")).unwrap();
    let ids: Vec<String> = registry
        .list()
        .unwrap()
        .into_iter()
        .map(|milestone| milestone.id)
        .collect();
    assert_eq!(ids, vec![early.id, late.id, undated_a.id, undated_b.id]);
}
