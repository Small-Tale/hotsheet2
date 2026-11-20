//! HS2-1M8E9D: where a machine keeps a store's provider configuration, and how the first
//! resolve seeds it. Synthetic stores and records only.

use super::*;
use crate::StoreMetadata;

fn store() -> (tempfile::TempDir, FsStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    (dir, store)
}

#[test]
fn hs2_1m8e9d_the_machine_copy_is_keyed_by_store_identity_under_the_home() {
    let home = tempfile::tempdir().unwrap();
    let (_one_dir, one) = store();
    let (_two_dir, two) = store();
    let id = one.metadata().unwrap().instance_id.unwrap();
    let registry = ProviderConfigRegistry::for_store_in(home.path(), one.root()).unwrap();
    assert_eq!(
        registry.path(),
        home.path().join("stores").join(&id).join("providers.json")
    );
    let other = ProviderConfigRegistry::for_store_in(home.path(), two.root()).unwrap();
    assert_ne!(registry.path(), other.path());
    // Nothing is written into either store.
    assert!(!one.root().join(PROVIDER_CONFIG_FILE).exists());
    assert!(!one.root().join("providers.json.lock").exists());
}

#[test]
fn hs2_1m8e9d_a_plain_store_seeds_from_its_file_then_never_again() {
    let home = tempfile::tempdir().unwrap();
    let (_dir, store) = store();
    let legacy = store.root().join(PROVIDER_CONFIG_FILE);
    std::fs::write(&legacy, b"{\"connections\":[]}\n").unwrap();
    let registry = unseeded(home.path(), store.root());
    assert_eq!(
        registry.seed_from_store(store.root()).unwrap(),
        ProviderConfigSeed::StoreFile
    );
    std::fs::remove_file(&legacy).unwrap();
    assert_eq!(
        registry.seed_from_store(store.root()).unwrap(),
        ProviderConfigSeed::Existing
    );
    assert!(registry.load().unwrap().is_empty());
}

#[test]
fn hs2_1m8e9d_a_store_without_a_file_or_history_seeds_an_empty_list() {
    let home = tempfile::tempdir().unwrap();
    let (_dir, store) = store();
    let registry = unseeded(home.path(), store.root());
    assert_eq!(
        registry.seed_from_store(store.root()).unwrap(),
        ProviderConfigSeed::Empty
    );
    assert!(registry.load().unwrap().is_empty());
}

#[test]
fn hs2_1m8e9d_a_malformed_legacy_file_is_copied_and_reports_as_before() {
    let home = tempfile::tempdir().unwrap();
    let (_dir, store) = store();
    std::fs::write(store.root().join(PROVIDER_CONFIG_FILE), b"not json").unwrap();
    let registry = ProviderConfigRegistry::for_store_in(home.path(), store.root()).unwrap();
    assert_eq!(std::fs::read(registry.path()).unwrap(), b"not json");
    assert!(registry.load().is_err());
}

#[test]
fn hs2_1m8e9d_a_legacy_store_gets_an_identity_and_a_non_store_keeps_its_file() {
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
    assert!(!home.path().join(MACHINE_STORE_CONFIG_DIR).exists());
}

/// This machine's registry for the store, before any seed.
fn unseeded(home: &Path, store_root: &Path) -> ProviderConfigRegistry {
    let key = store_config_key(store_root).unwrap();
    ProviderConfigRegistry::new(machine_provider_config_path(home, &key))
}
