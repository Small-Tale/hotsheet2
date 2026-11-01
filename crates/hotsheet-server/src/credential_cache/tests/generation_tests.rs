use super::*;
use hotsheet_ticketing::KeyMetadata;
use std::collections::BTreeMap;
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::AtomicBool;

struct TestHome {
    _parent: tempfile::TempDir,
    root: PathBuf,
}
impl TestHome {
    fn path(&self) -> &std::path::Path {
        &self.root
    }
}
fn private_home() -> TestHome {
    let parent = tempfile::tempdir().unwrap();
    #[cfg(unix)]
    let root = parent.path().to_path_buf();
    #[cfg(windows)]
    let root = {
        let root = parent.path().join("private-key-home");
        // Exercise the real publisher using only an injected synthetic store.
        // No public ownership-success seam or default-ACL repair is introduced.
        KeyRegistry::new(&root, CountingStore::default())
            .delete("hwc-fixture-setup")
            .unwrap();
        // Retain its owner-published inode for later synthetic overwrites. A new
        // plain fs::write need not inherit the effective user's owner SID.
        assert!(root.join("keys.json").is_file());
        root
    };
    TestHome {
        _parent: parent,
        root,
    }
}

const ACCOUNT: &str = "github-app-hwc-synthetic";

#[derive(Clone, Default)]
struct Toggle {
    memory: CountingStore,
    denied: Arc<AtomicBool>,
}
impl SecretStore for Toggle {
    fn set(&self, account: &str, value: &str) -> Result<(), SecretError> {
        self.memory.set(account, value)
    }
    fn get(&self, account: &str) -> Result<Option<String>, SecretError> {
        if self.denied.load(Ordering::SeqCst) {
            self.memory.reads.fetch_add(1, Ordering::SeqCst);
            Err(SecretError::Backend("synthetic read refusal".into()))
        } else {
            self.memory.get(account)
        }
    }
    fn delete(&self, account: &str) -> Result<bool, SecretError> {
        self.memory.delete(account)
    }
}
fn reads(backend: &Toggle) -> usize {
    backend.memory.reads.load(Ordering::SeqCst)
}
fn stored(home: &std::path::Path) -> Vec<u8> {
    fs::read(home.join("keys.json")).unwrap()
}

// Literal old whole-map serde rewrite, not a manual generation-field removal.
// This is an injected legacy writer, never a production alternative owner.
fn old_set(home: &std::path::Path, backend: &Toggle, account: &str, value: &str) {
    backend.set(account, value).unwrap();
    let mut map: BTreeMap<String, KeyMetadata> = serde_json::from_slice(&stored(home)).unwrap();
    let previous = map.get(account);
    let site = previous.and_then(|entry| entry.site.clone());
    let identity = previous.and_then(|entry| entry.identity.clone());
    map.insert(
        account.into(),
        KeyMetadata {
            provider: account.into(),
            env: hotsheet_ticketing::secrets::env_name(account),
            site,
            identity,
        },
    );
    let mut bytes = serde_json::to_vec_pretty(&map).unwrap();
    bytes.push(b'\n');
    fs::write(home.join("keys.json"), bytes).unwrap();
    #[cfg(unix)]
    fs::set_permissions(home.join("keys.json"), fs::Permissions::from_mode(0o600)).unwrap();
}

#[test]
fn hs2_hwc0fs_two_independent_caches_refresh_same_public_metadata_replacement_and_denial() {
    for denied in [false, true] {
        let home = private_home();
        let backend = Toggle::default();
        let keys = KeyRegistry::new(home.path(), backend.clone());
        keys.set(ACCOUNT, "first").unwrap();
        let public = serde_json::to_value(keys.list().unwrap()).unwrap();
        let before = stored(home.path());
        backend.denied.store(denied, Ordering::SeqCst);
        let first = ManagedSecretCache::default().store(home.path().into(), backend.clone());
        let second = ManagedSecretCache::default().store(home.path().into(), backend.clone());
        for cache in [&first, &second] {
            for _ in 0..3 {
                let value = cache.get(ACCOUNT);
                if denied {
                    assert!(value.is_err());
                } else {
                    assert_eq!(value.unwrap().as_deref(), Some("first"));
                }
            }
        }
        assert_eq!(reads(&backend), 2);
        backend.denied.store(false, Ordering::SeqCst);
        keys.set(ACCOUNT, "replacement").unwrap();
        assert_ne!(stored(home.path()), before);
        assert_eq!(serde_json::to_value(keys.list().unwrap()).unwrap(), public);
        for cache in [&first, &second] {
            assert_eq!(cache.get(ACCOUNT).unwrap().as_deref(), Some("replacement"));
            assert_eq!(cache.get(ACCOUNT).unwrap().as_deref(), Some("replacement"));
        }
        assert_eq!(reads(&backend), 4);
    }
}

#[test]
fn hs2_hwc0fs_repeated_identical_legacy_maps_bypass_both_values_and_denials() {
    let home = private_home();
    let backend = Toggle::default();
    let keys = KeyRegistry::new(home.path(), backend.clone());
    keys.set(ACCOUNT, "first").unwrap();
    let cache = ManagedSecretCache::default().store(home.path().into(), backend.clone());
    assert_eq!(cache.get(ACCOUNT).unwrap().as_deref(), Some("first"));
    assert_eq!(reads(&backend), 1);
    old_set(home.path(), &backend, ACCOUNT, "old-one");
    let legacy = stored(home.path());
    let mut expected = 1;
    for value in ["old-two", "old-three", "old-four"] {
        old_set(home.path(), &backend, ACCOUNT, value);
        assert_eq!(stored(home.path()), legacy);
        for _ in 0..2 {
            assert_eq!(cache.get(ACCOUNT).unwrap().as_deref(), Some(value));
            expected += 1;
        }
        assert_eq!(reads(&backend), expected);
    }
    backend.denied.store(true, Ordering::SeqCst);
    for _ in 0..3 {
        assert!(cache.get(ACCOUNT).is_err());
        expected += 1;
    }
    assert_eq!(reads(&backend), expected);
    assert!(!cache.cache.0.lock_or_recover().values.contains_key(ACCOUNT));
    backend.denied.store(false, Ordering::SeqCst);
    keys.set(ACCOUNT, "fresh-generation").unwrap();
    assert_eq!(
        cache.get(ACCOUNT).unwrap().as_deref(),
        Some("fresh-generation")
    );
    expected += 1;
    assert_eq!(
        cache.get(ACCOUNT).unwrap().as_deref(),
        Some("fresh-generation")
    );
    assert_eq!(reads(&backend), expected);
}

#[test]
fn hs2_hwc0fs_missing_empty_invalid_and_absent_selected_rows_never_cache() {
    let cases = [
        "missing-file",
        "empty-map",
        "missing-row",
        "missing-generation",
        "null",
        "numeric",
        "object",
        "lowercase",
        "oversized",
        "malformed-json",
    ];
    for case in cases {
        let home = private_home();
        let backend = Toggle::default();
        backend.set(ACCOUNT, "synthetic").unwrap();
        if case == "missing-file" {
            #[cfg(windows)]
            fs::remove_file(home.path().join("keys.json")).unwrap();
            assert!(
                !home.path().join("keys.json").exists(),
                "true missing-file control"
            );
        }
        let base = serde_json::json!({"provider":ACCOUNT,"env":"HOTSHEET_API_KEY_SYNTHETIC"});
        let document = match case {
            "missing-file" => None,
            "malformed-json" => Some(b"{".to_vec()),
            "empty-map" => Some(b"{}".to_vec()),
            "missing-row" => Some(serde_json::to_vec(&serde_json::json!({"other":base})).unwrap()),
            _ => {
                let mut row = base;
                match case {
                    "missing-generation" => {}
                    "null" => row["credential_generation"] = serde_json::Value::Null,
                    "numeric" => row["credential_generation"] = serde_json::json!(7),
                    "object" => {
                        row["credential_generation"] = serde_json::json!({"unexpected":true})
                    }
                    "lowercase" => {
                        row["credential_generation"] =
                            serde_json::json!("01arz3ndektsv4rrffq69g5fav")
                    }
                    "oversized" => {
                        row["credential_generation"] = serde_json::json!("0".repeat(4096))
                    }
                    _ => unreachable!(),
                }
                Some(serde_json::to_vec(&serde_json::json!({(ACCOUNT):row})).unwrap())
            }
        };
        if let Some(bytes) = document {
            fs::write(home.path().join("keys.json"), bytes).unwrap();
        }
        let cache = ManagedSecretCache::default().store(home.path().into(), backend.clone());
        for _ in 0..3 {
            assert_eq!(cache.get(ACCOUNT).unwrap().as_deref(), Some("synthetic"));
        }
        backend.denied.store(true, Ordering::SeqCst);
        for _ in 0..3 {
            assert!(cache.get(ACCOUNT).is_err());
        }
        assert_eq!(
            reads(&backend),
            6,
            "ineligible case must not cache success or denial: {case}"
        );
        assert!(!cache.cache.0.lock_or_recover().values.contains_key(ACCOUNT));
    }
}

#[test]
fn hs2_hwc0fs_ineligible_selected_account_does_not_evict_unrelated_eligible_cache() {
    let home = private_home();
    let backend = Toggle::default();
    fs::write(home.path().join("keys.json"), b"{}").unwrap();
    old_set(home.path(), &backend, "github-app-legacy", "legacy");
    KeyRegistry::new(home.path(), backend.clone())
        .set(ACCOUNT, "generated")
        .unwrap();
    let cache = ManagedSecretCache::default().store(home.path().into(), backend.clone());
    assert_eq!(cache.get(ACCOUNT).unwrap().as_deref(), Some("generated"));
    assert_eq!(
        cache.get("github-app-legacy").unwrap().as_deref(),
        Some("legacy")
    );
    assert_eq!(cache.get(ACCOUNT).unwrap().as_deref(), Some("generated"));
    assert_eq!(reads(&backend), 2);
}

#[test]
fn hs2_hwc0fs_native_set_delete_phases_evict_without_seeding_old_generation() {
    let home = private_home();
    let backend = Toggle::default();
    KeyRegistry::new(home.path(), backend.clone())
        .set(ACCOUNT, "first")
        .unwrap();
    let cache = ManagedSecretCache::default().store(home.path().into(), backend.clone());
    assert_eq!(cache.get(ACCOUNT).unwrap().as_deref(), Some("first"));
    let before = stored(home.path());
    cache.set(ACCOUNT, "native-phase").unwrap();
    assert_eq!(
        stored(home.path()),
        before,
        "the native port has not committed metadata"
    );
    assert!(!cache.cache.0.lock_or_recover().values.contains_key(ACCOUNT));
    assert!(cache.delete(ACCOUNT).unwrap());
    assert!(!cache.cache.0.lock_or_recover().values.contains_key(ACCOUNT));
    let keys = KeyRegistry::new(home.path(), cache);
    keys.set(ACCOUNT, "committed").unwrap();
    assert_eq!(keys.get(ACCOUNT).unwrap(), "committed");
    assert_eq!(keys.get(ACCOUNT).unwrap(), "committed");
    assert_eq!(reads(&backend), 2);
}

#[test]
fn hs2_hwc0fs_changed_generation_during_native_get_is_not_retagged_or_cached() {
    struct Changing {
        memory: CountingStore,
        home: PathBuf,
        first: AtomicBool,
        action: &'static str,
    }
    impl SecretStore for Changing {
        fn set(&self, account: &str, value: &str) -> Result<(), SecretError> {
            self.memory.set(account, value)
        }
        fn get(&self, account: &str) -> Result<Option<String>, SecretError> {
            let value = self.memory.get(account)?;
            if self.first.swap(false, Ordering::SeqCst) {
                let keys = KeyRegistry::new(&self.home, self.memory.clone());
                match self.action {
                    "rotate" => keys.set(account, "fresh")?,
                    "delete" => {
                        keys.delete(account)?;
                    }
                    "drop" => {
                        let public: BTreeMap<String, KeyMetadata> =
                            serde_json::from_slice(&stored(&self.home)).unwrap();
                        fs::write(
                            self.home.join("keys.json"),
                            serde_json::to_vec(&public).unwrap(),
                        )?;
                    }
                    _ => unreachable!(),
                }
            }
            Ok(value)
        }
        fn delete(&self, account: &str) -> Result<bool, SecretError> {
            self.memory.delete(account)
        }
    }
    for action in ["rotate", "drop", "delete"] {
        let home = private_home();
        let memory = CountingStore::default();
        KeyRegistry::new(home.path(), memory.clone())
            .set(ACCOUNT, "old")
            .unwrap();
        let cache = ManagedSecretCache::default().store(
            home.path().into(),
            Changing {
                memory: memory.clone(),
                home: home.path().into(),
                first: true.into(),
                action,
            },
        );
        // The overlapping read is as-of, not a promised native/metadata transaction.
        assert_eq!(cache.get(ACCOUNT).unwrap().as_deref(), Some("old"));
        assert!(!cache.cache.0.lock_or_recover().values.contains_key(ACCOUNT));
        let expected = match action {
            "rotate" => Some("fresh"),
            "drop" => Some("old"),
            "delete" => None,
            _ => unreachable!(),
        };
        assert_eq!(cache.get(ACCOUNT).unwrap().as_deref(), expected);
        assert_eq!(cache.get(ACCOUNT).unwrap().as_deref(), expected);
        assert_eq!(
            memory.reads.load(Ordering::SeqCst),
            if action == "rotate" { 2 } else { 3 }
        );
        KeyRegistry::new(home.path(), memory.clone())
            .set(ACCOUNT, "recovered")
            .unwrap();
        let before = memory.reads.load(Ordering::SeqCst);
        assert_eq!(cache.get(ACCOUNT).unwrap().as_deref(), Some("recovered"));
        assert_eq!(cache.get(ACCOUNT).unwrap().as_deref(), Some("recovered"));
        assert_eq!(memory.reads.load(Ordering::SeqCst), before + 1);
    }
}
