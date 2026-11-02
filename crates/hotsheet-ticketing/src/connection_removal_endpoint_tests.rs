// HS2-6H8ZY6: included in the existing private removal fixture module.
include!("connection_link_endpoint_tests.rs");
struct NoEndpointLinks;
impl crate::github_endpoint::LinkObserver for NoEndpointLinks {
    fn is_linked(&self, _: &str, _: &crate::github_endpoint::Link) -> Result<bool, crate::github_endpoint::EndpointError> { Ok(false) }
}
fn endpoint_decision(f: &Fixture, id: &str, pending: bool) {
    use crate::github_endpoint::{Endpoint, EndpointRegistry, Expected, Intent, Link, Observation, Request};
    let source = f.providers.load().unwrap().into_iter().find(|row| row.id == id).unwrap();
    let account = credential_of(&source).unwrap().to_string();
    let expected = Some(Expected::capture(&source).unwrap());
    EndpointRegistry::new(&f.providers).apply(&Request {
        operation_id: hotsheet_model::Ulid::new(), expected, connection: source,
        intent: Intent::Explicit { credential_ref: account, endpoint: Endpoint::Public },
        link: pending.then(|| Link { checkout_id: f.checkout_id.clone(), make_default: false }),
    }, &Observation::Unavailable, &NoEndpointLinks).unwrap();
}
fn persisted_pair(f: &Fixture) -> (Vec<u8>, Vec<u8>) {
    (std::fs::read(f.providers.path()).unwrap(), std::fs::read(f.checkouts.path()).unwrap())
}
fn external_provider_rows(f: &Fixture, rows: &[ProviderConnection]) {
    // Synthetic older/out-of-band writer; never an ordinary public guard bypass.
    std::fs::write(f.providers.path(), serde_json::to_vec(&serde_json::json!({"connections":rows})).unwrap()).unwrap();
}

#[test]
fn hs2_6h8zy6_pending_and_malformed_removal_refuse_before_any_unlink() {
    use crate::github_endpoint::EndpointError;
    for pending in [true, false] {
        for global in [true, false] {
            let f = fixture();
            if pending { endpoint_decision(&f, "github-docs", true); }
            else {
                let mut rows = f.providers.load().unwrap();
                rows.iter_mut().find(|row| row.id == "github-docs").unwrap().settings["github_endpoint"] = serde_json::Value::Null;
                external_provider_rows(&f, &rows);
            }
            let before = persisted_pair(&f);
            let error = if global { remove_provider_connection(&f.providers, &f.checkouts, "github-docs").unwrap_err() }
                else { detach_source(&f.providers, &f.checkouts, "one", "github-docs").unwrap_err() };
            assert!(matches!(error, ConnectionRemovalError::Provider(ProviderError::Endpoint(actual))
                if actual == if pending { EndpointError::Pending } else { EndpointError::Invalid }));
            assert_eq!(persisted_pair(&f), before);
        }
    }
}

#[test]
fn hs2_6h8zy6_account_eligibility_precedes_pending_and_eligible_refusal_publishes_nothing() {
    use crate::accounts::{AccountError, remove_unused_source};
    use crate::github_endpoint::EndpointError;
    let f = fixture(); endpoint_decision(&f, "github-docs", true); endpoint_decision(&f, "github-wiki", true);
    let before = persisted_pair(&f);
    assert!(matches!(remove_unused_source(&f.providers, &f.checkouts, "wrong-account", "github-docs"), Err(AccountError::SourceNotFound { .. })));
    assert!(matches!(remove_unused_source(&f.providers, &f.checkouts, "shared-pat", "missing"), Err(AccountError::SourceNotFound { .. })));
    assert!(matches!(remove_unused_source(&f.providers, &f.checkouts, "shared-pat", "github-docs"), Err(AccountError::SourceInUse { projects, .. }) if projects.len() == 2));
    assert!(matches!(remove_unused_source(&f.providers, &f.checkouts, "shared-pat", "github-wiki"), Err(AccountError::Provider(ProviderError::Endpoint(EndpointError::Pending)))));
    assert_eq!(persisted_pair(&f), before);
    let mut rows = f.providers.load().unwrap();
    rows.iter_mut().find(|row| row.id == "github-wiki").unwrap().provider = "git".into();
    external_provider_rows(&f, &rows);
    let before = persisted_pair(&f);
    assert!(matches!(remove_unused_source(&f.providers, &f.checkouts, "shared-pat", "github-wiki"), Err(AccountError::SourceNotFound { .. })));
    assert_eq!(persisted_pair(&f), before);
}

#[test]
fn hs2_6h8zy6_global_remove_uses_actual_git_kind_after_checkout_preparation() {
    let f = fixture(); let checkout_before = std::fs::read(f.checkouts.path()).unwrap();
    let error = remove_provider_connection_with_hook(&f.providers, &f.checkouts, "github-docs", || {
        let mut rows = f.providers.load().unwrap();
        rows.iter_mut().find(|row| row.id == "github-docs").unwrap().provider = "git".into();
        external_provider_rows(&f, &rows);
    }).unwrap_err();
    assert!(matches!(error, ConnectionRemovalError::GitSource(id) if id == "github-docs"));
    assert_eq!(std::fs::read(f.checkouts.path()).unwrap(), checkout_before);
    assert_eq!(f.providers.load().unwrap().iter().find(|row| row.id == "github-docs").unwrap().provider, "git");
}

#[test]
fn hs2_6h8zy6_external_unlink_preserves_git_identity_aliases_and_unrelated_metadata() {
    let f = fixture(); let git_root = f._dir.path().join("metadata-only.hs2");
    std::fs::create_dir(&git_root).unwrap();
    std::fs::write(git_root.join("hotsheet-store.json"), b"synthetic metadata must not be opened or changed").unwrap();
    let git = TicketSource::git(&git_root);
    let mut file: serde_json::Value = serde_json::from_slice(&std::fs::read(f.checkouts.path()).unwrap()).unwrap();
    let entry = file["checkouts"].as_array_mut().unwrap().iter_mut().find(|entry| entry["id"] == f.checkout_id).unwrap();
    entry["sources"].as_array_mut().unwrap().push(serde_json::to_value(&git).unwrap());
    entry["stores"] = serde_json::json!([git.locator.clone()]);
    entry["store_instance_ids"] = serde_json::json!({(git.connection_id.clone()):"synthetic-stable-identity"});
    entry["unverified_store_sources"] = serde_json::json!({(git.connection_id.clone()):"synthetic-prior-locator", "github-docs":"synthetic-obsolete-external"});
    entry["source_colors"] = serde_json::json!({(git.connection_id.clone()):"#22c55e", "github-docs":"#ef4444", "github-main":"#3b82f6"});
    entry["repository"] = serde_json::json!("synthetic-repository"); entry["permission_vault_registered"] = true.into();
    entry["default_source"] = serde_json::json!("github-docs");
    let project_root = f._dir.path().join("one"); std::fs::create_dir(project_root.join(".hotsheet2")).unwrap();
    let legacy_link = project_root.join(".hotsheet2/store");
    std::fs::write(&legacy_link, &git.locator).unwrap();
    file["sourceAliases"] = serde_json::json!({(f.checkout_id.clone()):{"historical-external":"github-docs"}});
    std::fs::write(f.checkouts.path(), serde_json::to_vec(&file).unwrap()).unwrap();
    let before = file.clone(); let metadata = std::fs::read(git_root.join("hotsheet-store.json")).unwrap();
    detach_source(&f.providers, &f.checkouts, "one", "github-docs").unwrap();
    let after: serde_json::Value = serde_json::from_slice(&std::fs::read(f.checkouts.path()).unwrap()).unwrap();
    let expected = file["checkouts"].as_array_mut().unwrap().iter_mut().find(|entry| entry["id"] == f.checkout_id).unwrap();
    expected["sources"].as_array_mut().unwrap().retain(|source| source["connection_id"] != "github-docs");
    expected["source_colors"].as_object_mut().unwrap().remove("github-docs");
    expected["unverified_store_sources"].as_object_mut().unwrap().remove("github-docs");
    expected.as_object_mut().unwrap().remove("default_source");
    assert_eq!(after, file, "all other Git, alias, repository, root and vault metadata must remain exact");
    assert_eq!(after["sourceAliases"], before["sourceAliases"]);
    assert_eq!(std::fs::read(git_root.join("hotsheet-store.json")).unwrap(), metadata);
    assert_eq!(std::fs::read_to_string(&legacy_link).unwrap(), git.locator);
    assert_eq!(f.checkouts.describe("one").unwrap().default_source.as_deref(), Some(git.connection_id.as_str()), "legacy linked Git wins over multiple remaining sources");
    assert_eq!(std::fs::read_dir(&git_root).unwrap().count(), 1, "no identity initialization, Git files or autocommit");
}

#[test]
fn hs2_6h8zy6_unlink_preserves_explicit_clear_and_sole_remaining_fallback() {
    for cleared in [true, false] {
        let f = fixture(); let mut file: serde_json::Value = serde_json::from_slice(&std::fs::read(f.checkouts.path()).unwrap()).unwrap();
        let entry = file["checkouts"].as_array_mut().unwrap().iter_mut().find(|entry| entry["id"] == f.checkout_id).unwrap();
        entry["default_source"] = serde_json::Value::Null;
        entry["default_source_cleared"] = cleared.into();
        std::fs::write(f.checkouts.path(), serde_json::to_vec(&file).unwrap()).unwrap();
        detach_source(&f.providers, &f.checkouts, "one", "github-docs").unwrap();
        let checkout = f.checkouts.resolve("one").unwrap();
        assert_eq!(checkout.default_source_cleared, cleared);
        assert_eq!(checkout.default_source.as_deref(), (!cleared).then_some("github-main"));
    }
}

#[test]
fn hs2_6h8zy6_strict_corrupt_registry_refuses_without_repair_or_backup() {
    let f = fixture(); let bytes = std::fs::read(f.checkouts.path()).unwrap();
    let corrupt = [bytes.as_slice(), bytes.as_slice()].concat();
    std::fs::write(f.checkouts.path(), &corrupt).unwrap();
    let provider_before = std::fs::read(f.providers.path()).unwrap();
    assert!(matches!(remove_provider_connection(&f.providers, &f.checkouts, "github-docs"), Err(ConnectionRemovalError::Checkout(CheckoutError::Invalid(_)))));
    assert_eq!(std::fs::read(f.checkouts.path()).unwrap(), corrupt);
    assert_eq!(std::fs::read(f.providers.path()).unwrap(), provider_before);
    assert!(!std::fs::read_dir(f._dir.path()).unwrap().any(|entry| entry.unwrap().file_name().to_string_lossy().contains(".corrupt-")));
}

#[test]
fn hs2_6h8zy6_missing_provider_cleanup_repeats_and_empty_then_refill_converges() {
    let f = fixture();
    let rows = f.providers.load().unwrap().into_iter().filter(|row| row.id != "github-docs").collect::<Vec<_>>();
    external_provider_rows(&f, &rows);
    let first = remove_provider_connection(&f.providers, &f.checkouts, "github-docs").unwrap();
    assert!(!first.removed_connection); assert_eq!(first.unlinked_checkouts.len(), 2);
    let empty = remove_provider_connection(&f.providers, &f.checkouts, "github-docs").unwrap();
    assert!(!empty.removed_connection && empty.unlinked_checkouts.is_empty());
    remove_provider_connection(&f.providers, &f.checkouts, "github-main").unwrap();
    assert!(f.checkouts.list().unwrap().iter().all(|checkout| checkout.sources.is_empty()));
    f.providers.update(|rows| { rows.push(connection("github-docs", "shared-pat", false)); Ok(()) }).unwrap();
    f.checkouts.add_registered_source(&f.providers, "one", TicketSource { connection_id:"github-docs".into(), provider:"github".into(), locator:"acme/github-docs".into() }, false).unwrap();
    let again = remove_provider_connection(&f.providers, &f.checkouts, "github-docs").unwrap();
    assert!(again.removed_connection); assert_eq!(again.unlinked_checkouts, vec![f.checkout_id]);
    assert!(f.checkouts.resolve("one").unwrap().sources.is_empty());
    assert_eq!(f.keys.get("shared-pat").unwrap(), "ghp_fixture");
}

#[test]
fn hs2_6h8zy6_checkout_publication_failure_keeps_provider_then_retry_succeeds() {
    let f = fixture(); let original = std::fs::read(f.checkouts.path()).unwrap();
    let saved = f._dir.path().join("held-checkouts.json");
    let providers_before = std::fs::read(f.providers.path()).unwrap();
    let error = remove_provider_connection_with_hook(&f.providers, &f.checkouts, "github-docs", || {
        std::fs::rename(f.checkouts.path(), &saved).unwrap();
        std::fs::create_dir(f.checkouts.path()).unwrap();
    }).unwrap_err();
    assert!(matches!(error, ConnectionRemovalError::Checkout(CheckoutError::Io(_))));
    assert_eq!(std::fs::read(f.providers.path()).unwrap(), providers_before);
    assert_eq!(std::fs::read(&saved).unwrap(), original);
    std::fs::remove_dir(f.checkouts.path()).unwrap(); std::fs::rename(&saved, f.checkouts.path()).unwrap();
    let report = remove_provider_connection(&f.providers, &f.checkouts, "github-docs").unwrap();
    assert!(report.removed_connection); assert_eq!(report.unlinked_checkouts.len(), 2);
}

#[test]
fn hs2_6h8zy6_pending_started_after_checkout_snapshot_still_prevents_unlink() {
    let f = fixture(); let before = std::fs::read(f.checkouts.path()).unwrap();
    let error = detach_source_with_hook(&f.providers, &f.checkouts, "one", "github-docs", || {
        endpoint_decision(&f, "github-docs", true);
    }).unwrap_err();
    assert!(matches!(error, ConnectionRemovalError::Provider(ProviderError::Endpoint(crate::github_endpoint::EndpointError::Pending))));
    assert_eq!(std::fs::read(f.checkouts.path()).unwrap(), before);
    let row = f.providers.load().unwrap().into_iter().find(|row| row.id == "github-docs").unwrap();
    assert!(crate::github_endpoint::State::load(&row).unwrap().unwrap().is_pending());
}

#[test]
fn hs2_6h8zy6_actual_provider_publish_failure_after_unlink_leaves_retryable_orphan() {
    let f = fixture(); let provider_before = std::fs::read(f.providers.path()).unwrap();
    let held = f._dir.path().join("held-providers.json");
    let mut links = f.checkouts.prepare_source_links().unwrap();
    // Exercise the actual typed owner/publisher boundary without a product fault seam.
    // The OS rejects replacement of this owned nonempty directory, on all platforms.
    let failed: Result<((), Option<ProviderConnection>), ConnectionRemovalError> = f.providers.update_connection_domain("github-docs", |rows| {
        for checkout in [f.checkout_id.as_str(), f.other_checkout_id.as_str()] {
            assert!(links.unlink_external(checkout, "github-docs"));
        }
        links.publish()?;
        rows.retain(|row| row.id != "github-docs");
        std::fs::rename(f.providers.path(), &held).unwrap();
        std::fs::create_dir(f.providers.path()).unwrap();
        std::fs::write(f.providers.path().join("owned-blocker"), b"synthetic").unwrap();
        Ok(())
    });
    assert!(matches!(failed, Err(ConnectionRemovalError::Provider(ProviderError::Store(crate::StoreError::Io(_))))));
    drop(links);
    assert_eq!(std::fs::read(&held).unwrap(), provider_before);
    assert!(f.checkouts.list().unwrap().iter().all(|checkout| checkout.source("github-docs").is_none()));
    std::fs::remove_file(f.providers.path().join("owned-blocker")).unwrap();
    std::fs::remove_dir(f.providers.path()).unwrap(); std::fs::rename(&held, f.providers.path()).unwrap();
    let report = remove_provider_connection(&f.providers, &f.checkouts, "github-docs").unwrap();
    assert!(report.removed_connection && report.unlinked_checkouts.is_empty());
    assert!(!remove_provider_connection(&f.providers, &f.checkouts, "github-docs").unwrap().removed_connection);
}

#[test]
fn hs2_6h8zy6_default_removal_does_not_choose_first_of_multiple() {
    let f = fixture();
    let mut extra = connection("synthetic-extra", "shared-pat", false);
    extra.provider = "jira".into();
    extra.locator = "SYN".into();
    let mut connections = f.providers.load().unwrap();
    connections.push(extra.clone());
    f.providers.save(&connections).unwrap();
    f.checkouts.add_registered_source(&f.providers, "one", TicketSource { connection_id: extra.id, provider: extra.provider, locator: extra.locator }, false).unwrap();
    detach_source(&f.providers, &f.checkouts, "one", "github-main").unwrap();
    let checkout = f.checkouts.resolve("one").unwrap();
    assert_eq!(checkout.sources.len(), 2); assert!(checkout.default_source.is_none());
    assert!(!checkout.default_source_cleared);
}

fn owned_legacy_store_git(root: &std::path::Path, args: &[&str]) -> String {
    let mut command = crate::git::command_in(root);
    command.env("GIT_CONFIG_GLOBAL", root.join(".git/empty-global-config"))
        .env("GIT_CONFIG_NOSYSTEM", "1");
    for name in ["GIT_AUTHOR_NAME", "GIT_AUTHOR_EMAIL", "GIT_AUTHOR_DATE",
        "GIT_COMMITTER_NAME", "GIT_COMMITTER_EMAIL", "GIT_COMMITTER_DATE"] {
        command.env_remove(name);
    }
    let output = command.args(args).output().unwrap();
    assert!(output.status.success(), "owned synthetic Git: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

#[test]
fn hs2_6h8zy6_valid_legacy_git_metadata_is_not_initialized_or_committed_by_external_unlink() {
    use crate::{FsStore, StoreMetadata};
    for global in [false, true] {
        let f = fixture(); let git_root = f._dir.path().join("valid-legacy.hs2");
        std::fs::create_dir(&git_root).unwrap();
        std::fs::create_dir(git_root.join("tickets")).unwrap();
        // Real current schema, with the optional historical instanceId omitted.
        // Retain its former valid identity in checkout metadata: an older rewrite
        // can lose the store field while the checkout still remembers that identity.
        let mut metadata = StoreMetadata::new("SYN");
        let prior_id = metadata.instance_id.take().unwrap();
        let bytes = serde_json::to_vec_pretty(&metadata).unwrap();
        let metadata_path = git_root.join("hotsheet-store.json");
        std::fs::write(&metadata_path, &bytes).unwrap();
        owned_legacy_store_git(&git_root, &["init", "--quiet", "-b", "main"]);
        std::fs::write(git_root.join(".git/empty-global-config"), b"").unwrap();
        std::fs::create_dir(git_root.join(".git/empty-hooks")).unwrap();
        for (key, value) in [("user.name", "Synthetic"), ("user.email", "synthetic@invalid.example"),
            ("commit.gpgsign", "false"), ("core.hooksPath", ".git/empty-hooks"),
            ("core.fsmonitor", "false"), ("core.untrackedCache", "false")] {
            owned_legacy_store_git(&git_root, &["config", key, value]);
        }
        owned_legacy_store_git(&git_root, &["add", "--", "hotsheet-store.json"]);
        owned_legacy_store_git(&git_root, &["commit", "--quiet", "-m", "Synthetic historical metadata"]);
        assert!(owned_legacy_store_git(&git_root, &["status", "--porcelain"]).is_empty());
        let head = owned_legacy_store_git(&git_root, &["rev-parse", "--verify", "HEAD"]);
        let index = std::fs::read(git_root.join(".git/index")).unwrap();
        let opened = FsStore::open_without_maintenance(&git_root).unwrap();
        assert_eq!(opened.metadata().unwrap(), metadata, "valid metadata must parse with no instance identity");

        // Positive metadata-only control: the SAME valid bytes really permit
        // ensure_instance_id to change them. This control has no .git, so direct
        // identity autocommit is a no-op; deferred flags also retain ticket policy.
        let control_root = f._dir.path().join("initialization-control");
        std::fs::create_dir(&control_root).unwrap();
        std::fs::write(control_root.join("hotsheet-store.json"), &bytes).unwrap();
        let control = FsStore::open_without_maintenance(&control_root).unwrap()
            .with_deferred_autocommit().with_deferred_push();
        let initialized = control.ensure_instance_id().unwrap();
        assert_eq!(control.metadata().unwrap().instance_id.as_deref(), Some(initialized.as_str()));
        assert_ne!(std::fs::read(control_root.join("hotsheet-store.json")).unwrap(), bytes);

        let git = TicketSource::git(&git_root);
        let mut file: serde_json::Value = serde_json::from_slice(&std::fs::read(f.checkouts.path()).unwrap()).unwrap();
        let entry = file["checkouts"].as_array_mut().unwrap().iter_mut().find(|entry| entry["id"] == f.checkout_id).unwrap();
        entry["sources"].as_array_mut().unwrap().push(serde_json::to_value(&git).unwrap());
        entry["stores"] = serde_json::json!([git.locator.clone()]);
        entry["store_instance_ids"] = serde_json::json!({(git.connection_id.clone()):prior_id.clone()});
        std::fs::write(f.checkouts.path(), serde_json::to_vec(&file).unwrap()).unwrap();
        if global {
            assert!(remove_provider_connection(&f.providers, &f.checkouts, "github-docs").unwrap().removed_connection);
        } else {
            assert!(detach_source(&f.providers, &f.checkouts, "one", "github-docs").unwrap().unlinked);
        }
        assert_eq!(std::fs::read(&metadata_path).unwrap(), bytes, "external unlink must not initialize valid historical Git metadata");
        assert!(opened.metadata().unwrap().instance_id.is_none());
        assert_eq!(std::fs::read(git_root.join(".git/index")).unwrap(), index, "inspect index before any subsequent Git read");
        assert_eq!(owned_legacy_store_git(&git_root, &["rev-parse", "--verify", "HEAD"]), head, "no autocommit");
        assert!(owned_legacy_store_git(&git_root, &["status", "--porcelain"]).is_empty());
        let after = f.checkouts.describe("one").unwrap();
        assert_eq!(after.source(&git.connection_id), Some(&git));
        assert_eq!(after.store_instance_ids.get(&git.connection_id), Some(&prior_id));
    }
}
