// HS2-6H8ZY6 B3a: real checkout/provider owners; synthetic data only.
use std::path::Path;
fn b3_source(row: &ProviderConnection) -> TicketSource {
    TicketSource { connection_id: row.id.clone(), provider: row.provider.clone(), locator: row.locator.clone() }
}
fn b3_row(f: &Fixture, id: &str) -> ProviderConnection {
    f.providers.load().unwrap().into_iter().find(|row| row.id == id).unwrap()
}
fn b3_checkout_edit(f: &Fixture, edit: impl FnOnce(&mut serde_json::Value)) {
    let mut file: serde_json::Value = serde_json::from_slice(&std::fs::read(f.checkouts.path()).unwrap()).unwrap();
    edit(&mut file);
    std::fs::write(f.checkouts.path(), serde_json::to_vec(&file).unwrap()).unwrap();
}
fn b3_pending(error: CheckoutError) {
    assert!(matches!(error, CheckoutError::Endpoint(crate::github_endpoint::EndpointError::Pending)));
}

#[test]
fn hs2_6h8zy6_pending_github_link_uses_the_exact_typed_candidate_and_finalizes() {
    use crate::github_endpoint::{
        Endpoint, EndpointError, EndpointRegistry, Intent, Link, LinkObserver, Observation,
        Request,
    };
    struct Links<'a>(&'a CheckoutRegistry);
    impl LinkObserver for Links<'_> {
        fn is_linked(&self, connection_id: &str, link: &Link) -> Result<bool, EndpointError> {
            let checkout = self
                .0
                .resolve(&link.checkout_id)
                .map_err(|_| EndpointError::Storage)?;
            Ok(checkout.source(connection_id).is_some()
                && (!link.make_default
                    || checkout.default_source.as_deref() == Some(connection_id)))
        }
    }

    let f = fixture();
    let connection = connection("github-new", "shared-pat", false);
    let link = Link {
        checkout_id: f.checkout_id.clone(),
        make_default: true,
    };
    let core = EndpointRegistry::new(&f.providers);
    let mutation = core
        .apply(
            &Request {
                operation_id: hotsheet_model::Ulid::new(),
                expected: None,
                connection,
                intent: Intent::Explicit {
                    credential_ref: "shared-pat".into(),
                    endpoint: Endpoint::Public,
                },
                link: Some(link.clone()),
            },
            &Observation::Unavailable,
            &Links(&f.checkouts),
        )
        .unwrap();
    let recovery = mutation.recovery.unwrap();
    let pending = mutation.connection;
    let before = persisted_pair(&f);
    b3_pending(
        f.checkouts
            .add_registered_connection(&f.providers, &f.checkout_id, &pending, true)
            .unwrap_err(),
    );
    let mut stale = pending.clone();
    stale.name = Some("intervening metadata".into());
    assert!(matches!(
        f.checkouts
            .add_pending_github_connection(&f.providers, &stale, &link, &recovery),
        Err(CheckoutError::Endpoint(EndpointError::Conflict))
    ));
    let wrong_link = Link {
        make_default: false,
        ..link.clone()
    };
    assert!(matches!(
        f.checkouts
            .add_pending_github_connection(&f.providers, &pending, &wrong_link, &recovery),
        Err(CheckoutError::Endpoint(EndpointError::Conflict))
    ));
    assert_eq!(persisted_pair(&f), before);

    let linked = f
        .checkouts
        .add_pending_github_connection(&f.providers, &pending, &link, &recovery)
        .unwrap();
    assert_eq!(linked.default_source.as_deref(), Some("github-new"));
    assert!(linked.source("github-new").is_some());
    let completed = core.finalize(&recovery, &Links(&f.checkouts)).unwrap();
    assert_eq!(completed.id, "github-new");
    assert!(core.finalize(&recovery, &Links(&f.checkouts)).is_ok());
    assert_eq!(f.providers.load().unwrap().iter().find(|row| row.id == completed.id), Some(&completed));
}

#[test]
fn hs2_6h8zy6_b3b_open_rejects_conflicting_requested_routes_before_lossy_merge() {
    use crate::checkouts::{OpenSourceMode, OpenSourceSelection};
    for mode in [OpenSourceMode::Discovered, OpenSourceMode::SelectedGitStore, OpenSourceMode::Explicit] {
        let f = fixture();
        // Retained A is pending, but a metadata refresh of unchanged B must
        // remain valid. The conflict is in the request, not endpoint authority.
        endpoint_decision(&f, "github-main", true);
        let actual = b3_source(&b3_row(&f, "github-docs"));
        let mut wrong = actual.clone(); wrong.locator = "synthetic/wrong-request-route".into();
        let before = persisted_pair(&f);
        let root = f._dir.path().join("one");
        let result = f.checkouts.open_registered_sources(&f.providers, &root,
            Some("would-change-alias"), Some("synthetic-new-repository".into()),
            OpenSourceSelection { sources: vec![wrong, actual.clone()], default_source: None, mode });
        assert!(matches!(result, Err(CheckoutError::Invalid(_))), "conflicting request must refuse in {mode:?}");
        assert_eq!(persisted_pair(&f), before, "no checkout/provider publication in {mode:?}");
        if mode != OpenSourceMode::Explicit {
            let refreshed = f.checkouts.open_registered_sources(&f.providers, &root,
                Some("actual-metadata-refresh"), Some("synthetic-reviewed-repository".into()),
                OpenSourceSelection { sources: vec![actual.clone()], default_source: None, mode }).unwrap();
            assert_eq!(refreshed.alias, "actual-metadata-refresh");
            assert_eq!(refreshed.repository.as_deref(), Some("synthetic-reviewed-repository"));
            assert_eq!(refreshed.source(&actual.connection_id), Some(&actual));
            assert_eq!(refreshed.default_source.as_deref(), Some("github-main"));
            assert!(refreshed.source("github-main").is_some());
            assert_eq!(std::fs::read(f.providers.path()).unwrap(), before.0, "metadata refresh does not edit the pending provider");
            assert_eq!(f.checkouts.describe(&refreshed.id).unwrap(), refreshed);
        }
    }
}

#[test]
fn hs2_6h8zy6_b3a_collision_eligibility_precedes_pending_without_map_or_byte_changes() {
    for existing_kind in ["git", "jira"] {
        let f = fixture(); endpoint_decision(&f, "github-docs", true);
        b3_checkout_edit(&f, |file| {
            let entry = file["checkouts"].as_array_mut().unwrap().iter_mut().find(|entry| entry["id"] == f.checkout_id).unwrap();
            entry["sources"].as_array_mut().unwrap().iter_mut().find(|source| source["connection_id"] == "github-docs").unwrap()["provider"] = existing_kind.into();
            entry["store_instance_ids"] = serde_json::json!({"github-docs":"synthetic-retained-id"});
            entry["unverified_store_sources"] = serde_json::json!({"github-docs":"prior-locator"});
            entry["source_colors"] = serde_json::json!({"github-docs":"#22c55e"});
            entry["permission_vault_registered"] = true.into();
            file["sourceAliases"] = serde_json::json!({(f.checkout_id.clone()):{"old-docs":"github-docs"}});
        });
        let before = persisted_pair(&f); let row = b3_row(&f, "github-docs");
        assert!(matches!(f.checkouts.add_registered_source(&f.providers, "one", b3_source(&row), false), Err(CheckoutError::Invalid(_))));
        assert_eq!(persisted_pair(&f), before);
        assert!(matches!(f.checkouts.add_registered_connection(&f.providers, "one", &row, false), Err(CheckoutError::Invalid(_))));
        assert_eq!(persisted_pair(&f), before);
        assert!(matches!(f.checkouts.update_registered_source_locator(&f.providers, &row), Err(CheckoutError::Invalid(_))));
        assert_eq!(persisted_pair(&f), before);
    }
    let f = fixture(); endpoint_decision(&f, "github-docs", true);
    let mut wrong = b3_source(&b3_row(&f, "github-docs")); wrong.provider = "gitlab".into();
    let before = persisted_pair(&f);
    assert!(matches!(f.checkouts.add_registered_source(&f.providers, "two", wrong, false), Err(CheckoutError::Invalid(_))));
    assert_eq!(persisted_pair(&f), before);
}

#[test]
fn hs2_6h8zy6_b3a_targeted_pending_noops_and_displaced_defaults_refuse_without_publication() {
    for action in ["add-noop", "exact-noop", "locator-noop", "select-noop", "select", "displace", "clear"] {
        let f = fixture();
        let pending = if ["select-noop", "displace", "clear"].contains(&action) { "github-main" } else { "github-docs" };
        endpoint_decision(&f, pending, true); let row = b3_row(&f, "github-docs");
        let before = persisted_pair(&f);
        let error = match action {
            "add-noop" => f.checkouts.add_registered_source(&f.providers, "one", b3_source(&row), false).unwrap_err(),
            "exact-noop" => f.checkouts.add_registered_connection(&f.providers, "one", &row, false).unwrap_err(),
            "locator-noop" => f.checkouts.update_registered_source_locator(&f.providers, &row).unwrap_err(),
            "select-noop" => f.checkouts.set_registered_default_source(&f.providers, "one", Some("github-main")).unwrap_err(),
            "clear" => f.checkouts.set_registered_default_source(&f.providers, "one", None).unwrap_err(),
            _ => f.checkouts.set_registered_default_source(&f.providers, "one", Some("github-docs")).unwrap_err(),
        };
        b3_pending(error); assert_eq!(persisted_pair(&f), before, "{action}");
    }
}

#[test]
fn hs2_6h8zy6_b3a_unrelated_pending_and_effective_default_clear_refill_are_independent() {
    let f = fixture(); endpoint_decision(&f, "github-main", true);
    let row = b3_row(&f, "github-wiki"); let provider_bytes = std::fs::read(f.providers.path()).unwrap();
    let added = f.checkouts.add_registered_connection(&f.providers, "one", &row, false).unwrap();
    assert_eq!(added.default_source.as_deref(), Some("github-main"));
    assert!(added.source("github-main").is_some()); assert!(added.source("github-wiki").is_some());
    assert_eq!(std::fs::read(f.providers.path()).unwrap(), provider_bytes);
    // A raw legacy omission still has an EFFECTIVE sole pending default. Adding B
    // must preserve A rather than treating normalization as new authority over A.
    b3_checkout_edit(&f, |file| {
        let entry = file["checkouts"].as_array_mut().unwrap().iter_mut().find(|entry| entry["id"] == f.checkout_id).unwrap();
        entry["sources"].as_array_mut().unwrap().retain(|source| source["connection_id"] == "github-main");
        entry.as_object_mut().unwrap().remove("default_source");
        entry["default_source_cleared"] = false.into();
    });
    let added = f.checkouts.add_registered_connection(&f.providers, "one", &row, false).unwrap();
    assert_eq!(added.default_source.as_deref(), Some("github-main"));
    assert_eq!(std::fs::read(f.providers.path()).unwrap(), provider_bytes);
    // Explicitly cleared state targets no retained pending source. Refill does
    // not silently undo that user decision; selecting B explicitly can do so.
    b3_checkout_edit(&f, |file| {
        let entry = file["checkouts"].as_array_mut().unwrap().iter_mut().find(|entry| entry["id"] == f.checkout_id).unwrap();
        entry.as_object_mut().unwrap().remove("default_source"); entry["default_source_cleared"] = true.into();
    });
    let cleared = f.checkouts.set_registered_default_source(&f.providers, "one", None).unwrap();
    assert!(cleared.default_source.is_none() && cleared.default_source_cleared);
    let bytes = persisted_pair(&f);
    f.checkouts.set_registered_default_source(&f.providers, "one", None).unwrap();
    assert_eq!(persisted_pair(&f), bytes, "repeated clear is publication-free");
    let row = b3_row(&f, "github-docs");
    let refill = f.checkouts.add_registered_connection(&f.providers, "one", &row, false).unwrap();
    assert!(refill.default_source.is_none() && refill.default_source_cleared);
    let selected = f.checkouts.set_registered_default_source(&f.providers, "one", Some("github-docs")).unwrap();
    assert_eq!(selected.default_source.as_deref(), Some("github-docs")); assert!(!selected.default_source_cleared);
    let cleared = f.checkouts.set_registered_default_source(&f.providers, "one", None).unwrap();
    assert!(cleared.default_source.is_none() && cleared.default_source_cleared);
}

#[test]
fn hs2_6h8zy6_b3a_exact_saved_candidate_rejects_receipt_label_delete_and_kind_interleavings() {
    for change in ["receipt", "label", "delete", "kind"] {
        let f = fixture(); endpoint_decision(&f, "github-docs", false);
        let saved = b3_row(&f, "github-docs");
        let mut rows = f.providers.load().unwrap();
        if change == "delete" { rows.retain(|row| row.id != saved.id); }
        else {
            let row = rows.iter_mut().find(|row| row.id == saved.id).unwrap();
            match change {
                "receipt" => {
                    assert!(!row.settings["github_endpoint"]["operation"].is_null());
                    row.settings["github_endpoint"]["operation"] = serde_json::Value::Null;
                    assert!(crate::github_endpoint::State::load(row).unwrap().is_some());
                },
                "label" => row.name = Some("Later owner edit".into()),
                "kind" => row.provider = "jira".into(),
                _ => unreachable!(),
            }
        }
        external_provider_rows(&f, &rows);
        let before = persisted_pair(&f);
        for error in [f.checkouts.add_registered_connection(&f.providers, "one", &saved, false).unwrap_err(),
            f.checkouts.update_registered_source_locator(&f.providers, &saved).unwrap_err()] {
            if ["receipt", "label"].contains(&change) {
                assert!(matches!(error, CheckoutError::Endpoint(crate::github_endpoint::EndpointError::Conflict)));
            } else { assert!(matches!(error, CheckoutError::Invalid(_))); }
            assert_eq!(persisted_pair(&f), before, "{change}");
        }
    }
}

#[test]
fn hs2_6h8zy6_b3a_actual_normalized_return_links_but_prewrite_clone_and_non_github_stale_do_not() {
    let f = fixture(); endpoint_decision(&f, "github-docs", false);
    let stale = b3_row(&f, "github-docs");
    let actual = f.providers.update_connection("github-docs", |rows| {
        let row = rows.iter_mut().find(|row| row.id == "github-docs").unwrap();
        row.name = Some("Committed label".into());
        row.disabled = true;
        row.settings.as_object_mut().unwrap().remove("github_endpoint"); Ok(())
    }).unwrap().unwrap();
    assert_eq!(actual, b3_row(&f, "github-docs"));
    assert!(actual.disabled, "ordinary metadata linkage must not be confused with traffic permission");
    assert!(crate::github_endpoint::State::load(&actual).unwrap().unwrap().operation.is_none());
    let before = persisted_pair(&f);
    assert!(matches!(f.checkouts.add_registered_connection(&f.providers, "one", &stale, false), Err(CheckoutError::Endpoint(crate::github_endpoint::EndpointError::Conflict))));
    assert_eq!(persisted_pair(&f), before);
    assert!(f.checkouts.add_registered_connection(&f.providers, "one", &actual, false).unwrap().source(&actual.id).is_some());
    assert!(f.checkouts.update_registered_source_locator(&f.providers, &actual).unwrap().is_empty());
    let mut jira = connection("jira-new", "unused", false); jira.provider = "jira".into(); jira.locator = "SYN".into();
    f.providers.update(|rows| { rows.push(jira.clone()); Ok(()) }).unwrap();
    let linked = f.checkouts.add_registered_connection(&f.providers, "one", &jira, false).unwrap();
    assert_eq!(linked.source(&jira.id), Some(&b3_source(&jira)));
    let updated = f.providers.update_connection(&jira.id, |rows| {
        rows.iter_mut().find(|row| row.id == jira.id).unwrap().locator = "NEXT".into(); Ok(())
    }).unwrap().unwrap();
    let before = persisted_pair(&f);
    assert!(matches!(f.checkouts.update_registered_source_locator(&f.providers, &jira), Err(CheckoutError::Invalid(_))));
    assert_eq!(persisted_pair(&f), before);
    assert_eq!(f.checkouts.update_registered_source_locator(&f.providers, &updated).unwrap(), vec![f.checkout_id.clone()]);
    assert_eq!(f.checkouts.describe("one").unwrap().source(&jira.id).unwrap().locator, "NEXT");
}

#[test]
fn hs2_6h8zy6_b3a_strict_corrupt_checkout_never_repairs_on_add_default_or_locator() {
    for operation in ["add", "default", "locator"] {
        let f = fixture(); let row = b3_row(&f, "github-docs");
        let original = std::fs::read(f.checkouts.path()).unwrap();
        let corrupt = [original.as_slice(), original.as_slice()].concat();
        std::fs::write(f.checkouts.path(), &corrupt).unwrap();
        let before = persisted_pair(&f);
        let error = match operation {
            "add" => f.checkouts.add_registered_connection(&f.providers, "one", &row, false).unwrap_err(),
            "default" => f.checkouts.set_registered_default_source(&f.providers, "one", Some(&row.id)).unwrap_err(),
            _ => f.checkouts.update_registered_source_locator(&f.providers, &row).unwrap_err(),
        };
        assert!(matches!(error, CheckoutError::Invalid(_))); assert_eq!(persisted_pair(&f), before);
        assert!(!std::fs::read_dir(f._dir.path()).unwrap().any(|entry| entry.unwrap().file_name().to_string_lossy().contains(".corrupt-")));
    }
}

fn b3_git(f: &Fixture, with_identity: bool) -> (TicketSource, String, Vec<u8>, Vec<u8>) {
    let root = f._dir.path().join("b3-owned.hs2"); std::fs::create_dir(&root).unwrap();
    std::fs::create_dir(root.join("tickets")).unwrap();
    let mut metadata = crate::StoreMetadata::new("SYN");
    let former = metadata.instance_id.clone().unwrap();
    if !with_identity { metadata.instance_id = None; }
    let bytes = serde_json::to_vec_pretty(&metadata).unwrap();
    std::fs::write(root.join("hotsheet-store.json"), &bytes).unwrap();
    owned_legacy_store_git(&root, &["init", "--quiet", "-b", "main"]);
    std::fs::write(root.join(".git/empty-global-config"), b"").unwrap();
    std::fs::create_dir(root.join(".git/empty-hooks")).unwrap();
    for (key, value) in [("user.name", "Synthetic"), ("user.email", "synthetic@invalid.example"),
        ("commit.gpgsign", "false"), ("core.hooksPath", ".git/empty-hooks"), ("core.fsmonitor", "false"), ("core.untrackedCache", "false")] {
        owned_legacy_store_git(&root, &["config", key, value]);
    }
    owned_legacy_store_git(&root, &["add", "--", "hotsheet-store.json"]);
    owned_legacy_store_git(&root, &["commit", "--quiet", "-m", "Synthetic B3 baseline"]);
    let git = TicketSource::git(&root);
    b3_checkout_edit(f, |file| {
        let entry = file["checkouts"].as_array_mut().unwrap().iter_mut().find(|entry| entry["id"] == f.checkout_id).unwrap();
        entry["sources"].as_array_mut().unwrap().push(serde_json::to_value(&git).unwrap());
        entry["stores"] = serde_json::json!([git.locator]);
        entry["store_instance_ids"] = serde_json::json!({(git.connection_id.clone()):former});
        entry["source_colors"] = serde_json::json!({(git.connection_id.clone()):"#22c55e"});
        entry["permission_vault_registered"] = true.into();
        file["sourceAliases"] = serde_json::json!({(f.checkout_id.clone()):{"historical-git":git.connection_id}});
    });
    let head = owned_legacy_store_git(&root, &["rev-parse", "--verify", "HEAD"]);
    let index = std::fs::read(root.join(".git/index")).unwrap();
    (git, head, index, bytes)
}

#[test]
fn hs2_6h8zy6_b3a_external_publication_preserves_retained_valid_git_without_instance_id() {
    let f = fixture(); let (git, head, index, metadata) = b3_git(&f, false);
    let before: serde_json::Value = serde_json::from_slice(&std::fs::read(f.checkouts.path()).unwrap()).unwrap();
    let added = b3_row(&f, "github-wiki");
    f.checkouts.add_registered_connection(&f.providers, "one", &added, false).unwrap();
    f.checkouts.set_registered_default_source(&f.providers, "one", Some("github-docs")).unwrap();
    let updated = f.providers.update_connection("github-docs", |rows| {
        rows.iter_mut().find(|row| row.id == "github-docs").unwrap().locator = "acme/new-docs".into(); Ok(())
    }).unwrap().unwrap();
    assert_eq!(f.checkouts.update_registered_source_locator(&f.providers, &updated).unwrap().len(), 2);
    assert_eq!(std::fs::read(Path::new(&git.locator).join("hotsheet-store.json")).unwrap(), metadata);
    assert_eq!(std::fs::read(Path::new(&git.locator).join(".git/index")).unwrap(), index, "check before any Git command");
    assert_eq!(owned_legacy_store_git(Path::new(&git.locator), &["rev-parse", "--verify", "HEAD"]), head);
    let after: serde_json::Value = serde_json::from_slice(&std::fs::read(f.checkouts.path()).unwrap()).unwrap();
    assert_eq!(after["sourceAliases"], before["sourceAliases"]);
    let find = |file: &serde_json::Value| file["checkouts"].as_array().unwrap().iter().find(|entry| entry["id"] == f.checkout_id).unwrap().clone();
    let old = find(&before); let current = find(&after);
    for field in ["stores", "store_instance_ids", "unverified_store_sources", "source_colors", "permission_vault_registered", "root", "repository", "alias"] {
        assert_eq!(current[field], old[field], "retained {field}");
    }
    assert_eq!(f.checkouts.describe("one").unwrap().source(&git.connection_id), Some(&git));
}

#[test]
fn hs2_6h8zy6_b3a_git_default_displacement_is_readonly_and_requires_recorded_matching_identity() {
    for condition in ["valid", "missing-recorded", "missing-store-id", "wrong-recorded", "pending-old"] {
        let f = fixture(); let (git, head, index, metadata) = b3_git(&f, condition != "missing-store-id");
        if condition == "pending-old" { endpoint_decision(&f, "github-main", true); }
        if ["missing-recorded", "wrong-recorded"].contains(&condition) {
            b3_checkout_edit(&f, |file| {
                let entry = file["checkouts"].as_array_mut().unwrap().iter_mut().find(|entry| entry["id"] == f.checkout_id).unwrap();
                if condition == "missing-recorded" { entry["store_instance_ids"].as_object_mut().unwrap().remove(&git.connection_id); }
                else { entry["store_instance_ids"][&git.connection_id] = ulid::Ulid::new().to_string().into(); }
            });
        }
        let before = persisted_pair(&f);
        let selected = f.checkouts.set_registered_default_source(&f.providers, "one", Some(&git.connection_id));
        match condition {
            "valid" => assert_eq!(selected.unwrap().default_source.as_deref(), Some(git.connection_id.as_str())),
            "pending-old" => { b3_pending(selected.unwrap_err()); assert_eq!(persisted_pair(&f), before); },
            _ => { assert!(matches!(selected, Err(CheckoutError::Invalid(_)))); assert_eq!(persisted_pair(&f), before); },
        }
        assert_eq!(std::fs::read(Path::new(&git.locator).join("hotsheet-store.json")).unwrap(), metadata);
        assert_eq!(std::fs::read(Path::new(&git.locator).join(".git/index")).unwrap(), index);
        assert_eq!(owned_legacy_store_git(Path::new(&git.locator), &["rev-parse", "--verify", "HEAD"]), head);
    }
}
