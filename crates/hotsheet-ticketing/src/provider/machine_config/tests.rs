//! HS2-7QPJ05: one provider configuration per machine, and the migration of HS2-1M8E9D's
//! per-store copies and older store-root files into it. Synthetic stores, homes and
//! records only; no keychain is touched.

use super::*;
use crate::StoreMetadata;

fn store() -> (tempfile::TempDir, FsStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    (dir, store)
}

fn connection(id: &str) -> ProviderConnection {
    ProviderConnection {
        id: id.into(),
        provider: "github".into(),
        locator: format!("synthetic/{id}"),
        name: None,
        default: false,
        settings: serde_json::json!({}),
        disabled: false,
    }
}

fn defaulted(id: &str) -> ProviderConnection {
    ProviderConnection {
        default: true,
        ..connection(id)
    }
}

fn write_connections(path: &Path, connections: &[ProviderConnection]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let text =
        serde_json::to_string_pretty(&serde_json::json!({ "connections": connections })).unwrap();
    std::fs::write(path, text).unwrap();
}

/// HS2-1M8E9D's per-store copy for `key` under `home`.
fn per_store(home: &Path, key: &str, connections: &[ProviderConnection]) -> PathBuf {
    let path = legacy_store_provider_config_path(home, key);
    write_connections(&path, connections);
    path
}

fn ids(registry: &ProviderConfigRegistry) -> Vec<String> {
    registry
        .load()
        .unwrap()
        .into_iter()
        .map(|connection| connection.id)
        .collect()
}

fn ledger(home: &Path) -> ProviderMigrationLedger {
    ProviderMigrationLedger::load(home).unwrap()
}

#[test]
fn hs2_7qpj05_every_store_resolves_the_one_machine_registry() {
    let home = tempfile::tempdir().unwrap();
    let (_one_dir, one) = store();
    let (_two_dir, two) = store();
    let first = ProviderConfigRegistry::for_store_in(home.path(), one.root()).unwrap();
    let second = ProviderConfigRegistry::for_store_in(home.path(), two.root()).unwrap();
    assert_eq!(first.path(), home.path().join(PROVIDER_CONFIG_FILE));
    assert_eq!(first.path(), second.path());
    assert_eq!(
        first.path(),
        ProviderConfigRegistry::for_machine(home.path()).path()
    );
    // A write through one store is visible through the other.
    first.save(&[connection("github-shared")]).unwrap();
    assert_eq!(ids(&second), ["github-shared"]);
    // Nothing is written into either store.
    for store in [&one, &two] {
        assert!(!store.root().join(PROVIDER_CONFIG_FILE).exists());
        assert!(!store.root().join("providers.json.lock").exists());
    }
}

#[test]
fn hs2_7qpj05_per_store_copies_merge_in_id_order_once() {
    let home = tempfile::tempdir().unwrap();
    per_store(home.path(), "b-store", &[connection("github-b")]);
    per_store(home.path(), "a-store", &[connection("github-a")]);
    let report = migrate_machine_provider_config(home.path(), None).unwrap();
    assert_eq!(
        report.merged,
        [
            ("a-store".to_string(), ProviderConfigSeed::MachineStoreFile),
            ("b-store".to_string(), ProviderConfigSeed::MachineStoreFile),
        ]
    );
    assert!(report.warnings.is_empty());
    let registry = ProviderConfigRegistry::for_machine(home.path());
    assert_eq!(ids(&registry), ["github-a", "github-b"]);
    let recorded = ledger(home.path());
    assert_eq!(recorded.stores.len(), 2);
    assert_eq!(recorded.stores["a-store"].added, 1);

    // Repeating changes nothing, and the source files are never removed.
    let before = std::fs::read(registry.path()).unwrap();
    for _ in 0..3 {
        assert_eq!(
            migrate_machine_provider_config(home.path(), None).unwrap(),
            ProviderMigrationReport::default()
        );
    }
    assert_eq!(std::fs::read(registry.path()).unwrap(), before);
    assert!(legacy_store_provider_config_path(home.path(), "a-store").is_file());
    assert_eq!(ledger(home.path()), recorded);
}

#[test]
fn hs2_7qpj05_identical_records_merge_once_and_conflicts_keep_the_first() {
    let home = tempfile::tempdir().unwrap();
    let shared = connection("github-shared");
    let mut changed = connection("github-shared");
    changed.locator = "synthetic/elsewhere".into();
    per_store(home.path(), "a-store", std::slice::from_ref(&shared));
    per_store(home.path(), "b-store", std::slice::from_ref(&shared));
    per_store(
        home.path(),
        "c-store",
        &[changed.clone(), connection("github-c")],
    );
    let report = migrate_machine_provider_config(home.path(), None).unwrap();
    let registry = ProviderConfigRegistry::for_machine(home.path());
    assert_eq!(ids(&registry), ["github-shared", "github-c"]);
    assert_eq!(registry.load().unwrap()[0], shared);
    // The dropped record is preserved verbatim with a warning.
    assert_eq!(report.warnings.len(), 1);
    let warning = &report.warnings[0];
    assert_eq!(warning.kind, ProviderMigrationWarningKind::Conflict);
    assert_eq!(warning.store, "c-store");
    assert_eq!(warning.connection_id.as_deref(), Some("github-shared"));
    assert_eq!(warning.dropped.as_ref(), Some(&changed));
    assert_eq!(ledger(home.path()).warnings, report.warnings);
    assert_eq!(ledger(home.path()).stores["b-store"].added, 0);
}

#[test]
fn hs2_7qpj05_conflict_resolution_does_not_depend_on_which_client_resolves_first() {
    let mut outcomes = Vec::new();
    for resolve_through_second in [false, true] {
        let home = tempfile::tempdir().unwrap();
        let (_one_dir, one) = store();
        let (_two_dir, two) = store();
        let one_key = store_config_key(one.root()).unwrap();
        let two_key = store_config_key(two.root()).unwrap();
        let mut one_record = connection("github-x");
        one_record.name = Some(format!("from {one_key}"));
        let mut two_record = connection("github-x");
        two_record.name = Some(format!("from {two_key}"));
        per_store(home.path(), &one_key, &[one_record]);
        per_store(home.path(), &two_key, &[two_record]);
        let root = if resolve_through_second {
            two.root()
        } else {
            one.root()
        };
        let registry = ProviderConfigRegistry::for_store_in(home.path(), root).unwrap();
        let kept = registry.load().unwrap()[0].name.clone().unwrap();
        let first_key = one_key.clone().min(two_key.clone());
        assert_eq!(kept, format!("from {first_key}"));
        outcomes.push(ledger(home.path()).warnings.len());
    }
    assert_eq!(outcomes, [1, 1]);
}

#[test]
fn hs2_7qpj05_a_second_default_is_merged_without_its_flag() {
    let home = tempfile::tempdir().unwrap();
    per_store(home.path(), "a-store", &[defaulted("github-a")]);
    per_store(home.path(), "b-store", &[defaulted("github-b")]);
    let report = migrate_machine_provider_config(home.path(), None).unwrap();
    let connections = ProviderConfigRegistry::for_machine(home.path())
        .load()
        .unwrap();
    assert!(connections[0].default);
    assert!(!connections[1].default);
    assert_eq!(report.warnings.len(), 1);
    assert_eq!(
        report.warnings[0].kind,
        ProviderMigrationWarningKind::DefaultCleared
    );
    // A record that differs only in its default flag is the same connection.
    let home = tempfile::tempdir().unwrap();
    per_store(home.path(), "a-store", &[defaulted("github-a")]);
    per_store(home.path(), "b-store", &[connection("github-a")]);
    let report = migrate_machine_provider_config(home.path(), None).unwrap();
    assert!(report.warnings.is_empty());
}

#[test]
fn hs2_7qpj05_interleaved_stores_and_new_per_store_copies_merge_into_existing_entries() {
    let home = tempfile::tempdir().unwrap();
    let (_one_dir, one) = store();
    let (_two_dir, two) = store();
    std::fs::write(
        one.root().join(PROVIDER_CONFIG_FILE),
        serde_json::to_string(&serde_json::json!({"connections": [connection("github-one")]}))
            .unwrap(),
    )
    .unwrap();
    let registry = ProviderConfigRegistry::for_store_in(home.path(), one.root()).unwrap();
    assert_eq!(ids(&registry), ["github-one"]);
    // A connection added through the CLI before the second store is first resolved.
    registry
        .update(|connections| {
            connections.push(connection("github-cli"));
            Ok(())
        })
        .unwrap();
    // An older build wrote a per-store copy for the second store meanwhile.
    let two_key = store_config_key(two.root()).unwrap();
    per_store(
        home.path(),
        &two_key,
        &[connection("github-cli"), connection("github-two")],
    );
    let resolved = ProviderConfigRegistry::for_store_in(home.path(), two.root()).unwrap();
    assert_eq!(ids(&resolved), ["github-one", "github-cli", "github-two"]);
    // Resolving the first store again changes nothing.
    ProviderConfigRegistry::for_store_in(home.path(), one.root()).unwrap();
    assert_eq!(ids(&resolved), ["github-one", "github-cli", "github-two"]);
    let recorded = ledger(home.path());
    let one_key = store_config_key(one.root()).unwrap();
    assert_eq!(
        recorded.stores[&one_key].source,
        ProviderConfigSeed::StoreFile
    );
    assert_eq!(
        recorded.stores[&two_key].source,
        ProviderConfigSeed::MachineStoreFile
    );
}

#[test]
fn hs2_7qpj05_empty_then_refill_never_brings_back_removed_entries() {
    let home = tempfile::tempdir().unwrap();
    let (_dir, store) = store();
    let key = store_config_key(store.root()).unwrap();
    per_store(home.path(), &key, &[connection("github-old")]);
    let registry = ProviderConfigRegistry::for_store_in(home.path(), store.root()).unwrap();
    assert_eq!(ids(&registry), ["github-old"]);
    registry.save(&[]).unwrap();
    for _ in 0..2 {
        let again = ProviderConfigRegistry::for_store_in(home.path(), store.root()).unwrap();
        assert!(ids(&again).is_empty());
    }
    registry.save(&[connection("github-new")]).unwrap();
    let again = ProviderConfigRegistry::for_store_in(home.path(), store.root()).unwrap();
    assert_eq!(ids(&again), ["github-new"]);
    // Deleting the registry file empties it; the ledger still keeps the old copy out.
    std::fs::remove_file(registry.path()).unwrap();
    let again = ProviderConfigRegistry::for_store_in(home.path(), store.root()).unwrap();
    assert!(ids(&again).is_empty());
}

#[test]
fn hs2_7qpj05_a_store_without_configuration_is_recorded_as_empty() {
    let home = tempfile::tempdir().unwrap();
    let (_dir, store) = store();
    let registry = ProviderConfigRegistry::for_store_in(home.path(), store.root()).unwrap();
    assert!(ids(&registry).is_empty());
    // Nothing to merge writes no registry, only the ledger entry.
    assert!(!registry.path().exists());
    let key = store_config_key(store.root()).unwrap();
    assert_eq!(
        ledger(home.path()).stores[&key].source,
        ProviderConfigSeed::Empty
    );
    // A store-root file that appears later is not merged again.
    std::fs::write(
        store.root().join(PROVIDER_CONFIG_FILE),
        serde_json::to_string(&serde_json::json!({"connections": [connection("github-late")]}))
            .unwrap(),
    )
    .unwrap();
    let again = ProviderConfigRegistry::for_store_in(home.path(), store.root()).unwrap();
    assert!(ids(&again).is_empty());
}

#[test]
fn hs2_7qpj05_a_corrupt_source_is_skipped_warned_once_and_retried() {
    let home = tempfile::tempdir().unwrap();
    let broken = per_store(home.path(), "a-store", &[]);
    std::fs::write(&broken, b"{\"connections\": [").unwrap();
    per_store(home.path(), "b-store", &[connection("github-b")]);
    let report = migrate_machine_provider_config(home.path(), None).unwrap();
    // The healthy store still merges.
    assert_eq!(
        report.merged,
        [("b-store".to_string(), ProviderConfigSeed::MachineStoreFile)]
    );
    assert_eq!(report.warnings.len(), 1);
    assert_eq!(
        report.warnings[0].kind,
        ProviderMigrationWarningKind::InvalidSource
    );
    assert_eq!(report.warnings[0].store, "a-store");
    // The same failure is not recorded twice.
    let report = migrate_machine_provider_config(home.path(), None).unwrap();
    assert!(report.warnings.is_empty() && report.merged.is_empty());
    assert_eq!(ledger(home.path()).warnings.len(), 1);
    assert!(!ledger(home.path()).stores.contains_key("a-store"));

    // A source with duplicate ids is invalid too.
    per_store(
        home.path(),
        "c-store",
        &[connection("github-c"), connection("github-c")],
    );
    let report = migrate_machine_provider_config(home.path(), None).unwrap();
    assert_eq!(report.warnings[0].store, "c-store");

    // Once repaired, it merges.
    write_connections(&broken, &[connection("github-a")]);
    let report = migrate_machine_provider_config(home.path(), None).unwrap();
    assert_eq!(
        report.merged,
        [("a-store".to_string(), ProviderConfigSeed::MachineStoreFile)]
    );
    assert_eq!(
        ids(&ProviderConfigRegistry::for_machine(home.path())),
        ["github-b", "github-a"]
    );
}

#[test]
fn hs2_7qpj05_a_partial_registry_is_reported_and_never_overwritten() {
    let home = tempfile::tempdir().unwrap();
    let registry = ProviderConfigRegistry::for_machine(home.path());
    std::fs::write(registry.path(), b"{\"connections\": [{\"id\":").unwrap();
    per_store(home.path(), "a-store", &[connection("github-a")]);
    assert!(migrate_machine_provider_config(home.path(), None).is_err());
    assert_eq!(
        std::fs::read(registry.path()).unwrap(),
        b"{\"connections\": [{\"id\":"
    );
    assert!(!provider_migration_ledger_path(home.path()).exists());
    // After a repair the pending copy merges.
    std::fs::write(registry.path(), b"{\"connections\": []}").unwrap();
    migrate_machine_provider_config(home.path(), None).unwrap();
    assert_eq!(ids(&registry), ["github-a"]);
}

#[test]
fn hs2_7qpj05_a_corrupt_ledger_fails_instead_of_merging_again() {
    let home = tempfile::tempdir().unwrap();
    per_store(home.path(), "a-store", &[connection("github-a")]);
    migrate_machine_provider_config(home.path(), None).unwrap();
    let registry = ProviderConfigRegistry::for_machine(home.path());
    registry.save(&[]).unwrap();
    std::fs::write(provider_migration_ledger_path(home.path()), b"not json").unwrap();
    let error = migrate_machine_provider_config(home.path(), None).unwrap_err();
    assert!(error.to_string().contains("migration ledger"), "{error}");
    assert!(ids(&registry).is_empty());
}

#[test]
fn hs2_7qpj05_a_crash_between_registry_and_ledger_writes_merges_to_the_same_result() {
    let home = tempfile::tempdir().unwrap();
    per_store(home.path(), "a-store", &[connection("github-a")]);
    per_store(home.path(), "b-store", &[connection("github-b")]);
    migrate_machine_provider_config(home.path(), None).unwrap();
    let registry = ProviderConfigRegistry::for_machine(home.path());
    let merged = std::fs::read(registry.path()).unwrap();
    // The registry was published but the ledger never was.
    std::fs::remove_file(provider_migration_ledger_path(home.path())).unwrap();
    let report = migrate_machine_provider_config(home.path(), None).unwrap();
    assert_eq!(report.merged.len(), 2);
    assert!(report.warnings.is_empty());
    assert_eq!(std::fs::read(registry.path()).unwrap(), merged);
}

#[test]
fn hs2_7qpj05_concurrent_resolves_merge_exactly_once() {
    let home = tempfile::tempdir().unwrap();
    for index in 0..6 {
        per_store(
            home.path(),
            &format!("store-{index}"),
            &[
                connection(&format!("github-{index}")),
                connection("github-shared"),
            ],
        );
    }
    let threads = (0..8)
        .map(|_| {
            let home = home.path().to_path_buf();
            std::thread::spawn(move || migrate_machine_provider_config(&home, None).unwrap())
        })
        .collect::<Vec<_>>();
    let merged: usize = threads
        .into_iter()
        .map(|thread| thread.join().unwrap().merged.len())
        .sum();
    assert_eq!(merged, 6);
    let registry = ProviderConfigRegistry::for_machine(home.path());
    assert_eq!(registry.load().unwrap().len(), 7);
    assert!(ledger(home.path()).warnings.is_empty());
}

#[test]
fn hs2_7qpj05_a_legacy_store_gets_an_identity_and_a_non_store_keeps_its_file() {
    let home = tempfile::tempdir().unwrap();
    let (_dir, store) = store();
    let mut legacy = store.metadata().unwrap();
    legacy.instance_id = None;
    std::fs::write(
        store.root().join("hotsheet-store.json"),
        serde_json::to_string_pretty(&legacy).unwrap(),
    )
    .unwrap();
    let key = store_config_key(store.root()).unwrap();
    assert_eq!(
        store.metadata().unwrap().instance_id.as_deref(),
        Some(key.as_str())
    );
    assert_eq!(store_config_key(store.root()).unwrap(), key);

    // A host-configuration directory that is not a store is never synced: its file stays.
    let plain = tempfile::tempdir().unwrap();
    let registry = ProviderConfigRegistry::for_store_in(home.path(), plain.path()).unwrap();
    assert_eq!(registry.path(), plain.path().join(PROVIDER_CONFIG_FILE));
    assert!(!registry.path().exists());
    assert!(!home.path().join(PROVIDER_CONFIG_FILE).exists());
    assert!(!provider_migration_ledger_path(home.path()).exists());
}

#[test]
fn hs2_7qpj05_a_per_store_directory_without_a_file_is_not_a_source() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(MACHINE_STORE_CONFIG_DIR).join("a-store")).unwrap();
    std::fs::write(
        home.path()
            .join(MACHINE_STORE_CONFIG_DIR)
            .join("a-store")
            .join("providers.json.lock"),
        b"",
    )
    .unwrap();
    assert_eq!(
        migrate_machine_provider_config(home.path(), None).unwrap(),
        ProviderMigrationReport::default()
    );
    assert!(!provider_migration_ledger_path(home.path()).exists());
}
