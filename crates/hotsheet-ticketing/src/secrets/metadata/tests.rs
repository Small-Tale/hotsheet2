#![cfg(any(unix, windows))]

use super::*;
use crate::secrets::{KeyRegistry, SecretStore, env_name};
use std::cell::{Cell, RefCell};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

struct TestHome {
    _parent: tempfile::TempDir,
    root: std::path::PathBuf,
}
impl TestHome {
    fn path(&self) -> &Path {
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
        // The parent may have an Administrators owner. The ACTUAL owner creates
        // the missing child with a protected current-user descriptor, no repair.
        let owner = crate::secrets::metadata_owner::Owner::prepare(&root).unwrap();
        let account = owner.account("hwc-fixture-setup").unwrap();
        // Seed through the actual publisher: inherited DACL alone does not set
        // a plain new file's owner when TokenDefaultOwner differs from TokenUser.
        owner.update(&account, |_| Ok(((), true))).unwrap();
        assert!(root.join("keys.json").is_file());
        root
    };
    TestHome {
        _parent: parent,
        root,
    }
}

#[derive(Default)]
struct Memory(RefCell<BTreeMap<String, String>>, Cell<usize>);
impl SecretStore for &Memory {
    fn set(&self, account: &str, value: &str) -> Result<(), SecretError> {
        self.1.set(self.1.get() + 1);
        self.0.borrow_mut().insert(account.into(), value.into());
        Ok(())
    }
    fn get(&self, account: &str) -> Result<Option<String>, SecretError> {
        Ok(self.0.borrow().get(account).cloned())
    }
    fn delete(&self, account: &str) -> Result<bool, SecretError> {
        self.1.set(self.1.get() + 1);
        Ok(self.0.borrow_mut().remove(account).is_some())
    }
}

fn path(home: &Path) -> std::path::PathBuf {
    home.join("keys.json")
}
fn bytes(home: &Path) -> Vec<u8> {
    std::fs::read(path(home)).unwrap()
}
fn generation(home: &Path, account: &str) -> Option<String> {
    decode(&bytes(home))
        .unwrap()
        .get(account)
        .and_then(|entry| entry.credential_generation.0.clone())
}
fn row(account: &str) -> KeyMetadata {
    KeyMetadata {
        provider: account.into(),
        env: env_name(account),
        site: Some("https://example.test".into()),
        identity: Some("synthetic-login".into()),
    }
}

// HS2-HWC0FS: literal old serde-map mutation algorithms from shipping 437ac3.
// This is a synthetic legacy-writer fixture, not an alternate production owner
// or a claim that an older executable was run. Unknown fields must be dropped by
// the ACTUAL public KeyMetadata Deserialize/Serialize derives, not manually stripped.
struct Old<'a> {
    home: &'a Path,
    native: &'a Memory,
}
impl Old<'_> {
    fn load(&self) -> BTreeMap<String, KeyMetadata> {
        serde_json::from_slice(&bytes(self.home)).unwrap()
    }
    fn write(&self, map: &BTreeMap<String, KeyMetadata>) {
        let mut bytes = serde_json::to_vec_pretty(map).unwrap();
        bytes.push(b'\n');
        std::fs::write(path(self.home), bytes).unwrap();
        #[cfg(unix)]
        std::fs::set_permissions(path(self.home), std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    fn set(&self, account: &str, value: &str) {
        self.native.set(account, value).unwrap();
        let mut map = self.load();
        let old = map.get(account);
        let site = old.and_then(|entry| entry.site.clone());
        let identity = old.and_then(|entry| entry.identity.clone());
        map.insert(
            account.into(),
            KeyMetadata {
                provider: account.into(),
                env: env_name(account),
                site,
                identity,
            },
        );
        self.write(&map);
    }
    fn site(&self, account: &str, site: &str) {
        let mut map = self.load();
        let entry = map.get_mut(account).unwrap();
        let site = site.trim().trim_end_matches('/');
        if entry.site.as_deref() == Some(site) {
            return;
        }
        entry.site = Some(site.into());
        self.write(&map);
    }
    fn identity(&self, account: &str, identity: &str) {
        let identity = identity.trim();
        if identity.is_empty() {
            return;
        }
        let mut map = self.load();
        let entry = map.get_mut(account).unwrap();
        if entry.identity.as_deref() == Some(identity) {
            return;
        }
        entry.identity = Some(identity.into());
        self.write(&map);
    }
    fn delete(&self, account: &str) {
        self.native.delete(account).unwrap();
        let mut map = self.load();
        if map.remove(account).is_some() {
            self.write(&map);
        }
    }
}

#[test]
fn hs2_hwc0fs_private_generation_is_old_serde_readable_and_absent_from_public_list() {
    let home = private_home();
    let native = Memory::default();
    let keys = KeyRegistry::new(home.path(), &native);
    keys.set("github-app-synthetic", "synthetic-value").unwrap();
    let old: BTreeMap<String, KeyMetadata> = serde_json::from_slice(&bytes(home.path())).unwrap();
    assert_eq!(
        serde_json::to_value(old.values().collect::<Vec<_>>()).unwrap(),
        serde_json::to_value(keys.list().unwrap()).unwrap()
    );
    assert!(generation(home.path(), "github-app-synthetic").is_some());
    assert!(
        serde_json::to_value(keys.list().unwrap()).unwrap()[0]
            .get("credential_generation")
            .is_none()
    );
}

#[test]
fn hs2_hwc0fs_invalid_optional_generation_is_uncacheable_not_invalid_public_metadata() {
    let invalid = [
        serde_json::Value::Null,
        serde_json::json!(false),
        serde_json::json!(7),
        serde_json::json!(1.25),
        serde_json::json!([]),
        serde_json::json!({"opaque": "shape"}),
        serde_json::json!(""),
        serde_json::json!("not-a-generation"),
        serde_json::json!("01arz3ndektsv4rrffq69g5fav"),
        serde_json::json!("Z1ARZ3NDEKTSV4RRFFQ69G5FAV"),
        serde_json::json!("0".repeat(25)),
        serde_json::json!("0".repeat(27)),
        serde_json::json!("x".repeat(4096)),
    ];
    for invalid in invalid {
        let home = private_home();
        let native = Memory::default();
        let account = "github-app-synthetic";
        let mut stored = serde_json::to_value(row(account)).unwrap();
        stored["credential_generation"] = invalid;
        let document = serde_json::json!({(account): stored});
        std::fs::write(path(home.path()), serde_json::to_vec(&document).unwrap()).unwrap();
        let keys = KeyRegistry::new(home.path(), &native);
        assert_eq!(keys.list().unwrap().len(), 1);
        assert!(
            !key_metadata_cache_observation(home.path(), account)
                .unwrap()
                .is_cacheable()
        );
        let original = bytes(home.path());
        keys.record_site(account, "https://example.test/").unwrap();
        keys.record_identity(account, "synthetic-login").unwrap();
        assert_eq!(
            bytes(home.path()),
            original,
            "true no-op must not normalize private metadata"
        );
        keys.record_site(account, "https://changed.example.test")
            .unwrap();
        assert!(generation(home.path(), account).is_none());
        assert!(
            !key_metadata_cache_observation(home.path(), account)
                .unwrap()
                .is_cacheable()
        );
        keys.set(account, "new-native-material").unwrap();
        assert!(
            key_metadata_cache_observation(home.path(), account)
                .unwrap()
                .is_cacheable()
        );
    }
}

#[test]
fn hs2_hwc0fs_set_rotates_generation_backfills_preserve_and_delete_removes() {
    let home = private_home();
    let native = Memory::default();
    let keys = KeyRegistry::new(home.path(), &native);
    let account = "github-app-synthetic";
    keys.set(account, "first").unwrap();
    let first = generation(home.path(), account).unwrap();
    keys.record_site(account, "https://example.test").unwrap();
    keys.record_identity(account, "synthetic-login").unwrap();
    assert_eq!(
        generation(home.path(), account).as_deref(),
        Some(first.as_str())
    );
    let before = bytes(home.path());
    keys.record_site(account, "https://example.test/").unwrap();
    keys.record_identity(account, " synthetic-login ").unwrap();
    assert_eq!(bytes(home.path()), before);
    let public = serde_json::to_value(keys.list().unwrap()).unwrap();
    keys.set(account, "second").unwrap();
    assert_ne!(generation(home.path(), account).unwrap(), first);
    assert_eq!(serde_json::to_value(keys.list().unwrap()).unwrap(), public);
    assert!(keys.delete(account).unwrap());
    assert!(decode(&bytes(home.path())).unwrap().is_empty());
    assert!(
        !key_metadata_cache_observation(home.path(), account)
            .unwrap()
            .is_cacheable()
    );
    assert!(!keys.delete(account).unwrap());
    assert!(
        !key_metadata_cache_observation(home.path(), account)
            .unwrap()
            .is_cacheable()
    );
}

#[test]
fn hs2_hwc0fs_legacy_backfill_does_not_fabricate_generation() {
    let home = private_home();
    let native = Memory::default();
    let account = "github-app-synthetic";
    std::fs::write(
        path(home.path()),
        serde_json::to_vec(&BTreeMap::from([(account, row(account))])).unwrap(),
    )
    .unwrap();
    let keys = KeyRegistry::new(home.path(), &native);
    keys.record_site(account, "https://changed.example.test")
        .unwrap();
    keys.record_identity(account, "changed-login").unwrap();
    assert!(generation(home.path(), account).is_none());
    assert!(
        !key_metadata_cache_observation(home.path(), account)
            .unwrap()
            .is_cacheable()
    );
}

#[test]
fn hs2_hwc0fs_all_four_successful_old_writer_algorithms_drop_generation() {
    for operation in ["set", "site", "identity", "delete"] {
        let home = private_home();
        let native = Memory::default();
        let keys = KeyRegistry::new(home.path(), &native);
        for account in ["github-app-first", "github-app-second"] {
            keys.set(account, "synthetic").unwrap();
        }
        let old = Old {
            home: home.path(),
            native: &native,
        };
        match operation {
            "set" => old.set("github-app-first", "replacement"),
            "site" => old.site("github-app-first", "https://changed.example.test"),
            "identity" => old.identity("github-app-first", "changed-login"),
            "delete" => old.delete("github-app-first"),
            _ => unreachable!(),
        }
        for account in ["github-app-first", "github-app-second"] {
            assert!(
                !key_metadata_cache_observation(home.path(), account)
                    .unwrap()
                    .is_cacheable()
            );
            assert!(generation(home.path(), account).is_none());
        }
        keys.set("github-app-first", "recovery").unwrap();
        assert!(
            key_metadata_cache_observation(home.path(), "github-app-first")
                .unwrap()
                .is_cacheable()
        );
        assert!(
            !key_metadata_cache_observation(home.path(), "github-app-second")
                .unwrap()
                .is_cacheable()
        );
    }
}

#[test]
fn hs2_hwc0fs_repeated_identical_old_maps_and_old_noops_have_distinct_eligibility() {
    let home = private_home();
    let native = Memory::default();
    let keys = KeyRegistry::new(home.path(), &native);
    let account = "github-app-synthetic";
    keys.set(account, "first").unwrap();
    keys.record_site(account, "https://example.test").unwrap();
    keys.record_identity(account, "synthetic-login").unwrap();
    let old = Old {
        home: home.path(),
        native: &native,
    };
    let before = bytes(home.path());
    old.site(account, " https://example.test/ ");
    old.identity(account, "synthetic-login");
    old.identity(account, " ");
    old.delete("missing");
    assert_eq!(bytes(home.path()), before);
    assert!(
        key_metadata_cache_observation(home.path(), account)
            .unwrap()
            .is_cacheable()
    );
    old.set(account, "older-one");
    let legacy = bytes(home.path());
    for value in ["older-two", "older-three", "older-four"] {
        old.set(account, value);
        assert_eq!(bytes(home.path()), legacy);
        assert!(
            !key_metadata_cache_observation(home.path(), account)
                .unwrap()
                .is_cacheable()
        );
        assert_eq!(keys.get(account).unwrap(), value);
    }
}

#[test]
fn hs2_hwc0fs_opaque_observation_compares_document_not_selected_or_native_authority() {
    let home = private_home();
    let native = Memory::default();
    // This case specifically observes absence, not the seeded empty document.
    #[cfg(windows)]
    std::fs::remove_file(path(home.path())).unwrap();
    assert!(!path(home.path()).exists());
    let missing = key_metadata_cache_observation(home.path(), "github-app-first").unwrap();
    assert!(!missing.is_cacheable());
    let keys = KeyRegistry::new(home.path(), &native);
    keys.set("github-app-first", "first").unwrap();
    let selected = key_metadata_cache_observation(home.path(), "github-app-first").unwrap();
    let absent = key_metadata_cache_observation(home.path(), "github-app-missing").unwrap();
    assert!(selected.is_cacheable());
    assert!(!absent.is_cacheable());
    assert!(selected.same_document(&absent));
    assert!(!selected.same_document(&missing));
    std::fs::write(path(home.path()), b"{").unwrap();
    assert!(
        !key_metadata_cache_observation(home.path(), "github-app-first")
            .unwrap()
            .is_cacheable()
    );
    std::fs::remove_file(path(home.path())).unwrap();
    std::fs::create_dir(path(home.path())).unwrap();
    assert!(
        key_metadata_cache_observation(home.path(), "github-app-first").is_err(),
        "genuine I/O error must not become cache authority"
    );
}

#[test]
fn hs2_hwc0fs_duplicate_reserved_generation_is_malformed_not_legacy_authority() {
    let home = private_home();
    let native = Memory::default();
    let raw = br#"{"github-app-synthetic":{"provider":"github-app-synthetic","env":"HOTSHEET_API_KEY_GITHUB_APP_SYNTHETIC","credential_generation":"01ARZ3NDEKTSV4RRFFQ69G5FAV","credential_generation":null}}"#;
    std::fs::write(path(home.path()), raw).unwrap();
    let old: BTreeMap<String, KeyMetadata> = serde_json::from_slice(raw).unwrap();
    assert_eq!(
        old.len(),
        1,
        "old projection ignored both previously unknown properties"
    );
    let keys = KeyRegistry::new(home.path(), &native);
    let account = "github-app-synthetic";
    assert!(matches!(keys.list(), Err(SecretError::Parse { .. })));
    assert!(matches!(
        keys.set(account, "synthetic"),
        Err(SecretError::Parse { .. })
    ));
    assert!(matches!(
        keys.delete(account),
        Err(SecretError::Parse { .. })
    ));
    assert!(matches!(
        keys.record_site(account, "https://example.test"),
        Err(SecretError::Parse { .. })
    ));
    assert!(matches!(
        keys.record_identity(account, "synthetic-login"),
        Err(SecretError::Parse { .. })
    ));
    assert_eq!(native.1.get(), 0);
    assert_eq!(bytes(home.path()), raw);
    assert!(
        !key_metadata_cache_observation(home.path(), account)
            .unwrap()
            .is_cacheable()
    );
    // An explicit well-formed old serde rewrite drops unknown fields; the new
    // owner never silently repairs a malformed reserved object during refusal.
    Old {
        home: home.path(),
        native: &native,
    }
    .write(&old);
    assert_eq!(keys.list().unwrap().len(), 1);
    assert!(
        !key_metadata_cache_observation(home.path(), account)
            .unwrap()
            .is_cacheable()
    );
    keys.set(account, "synthetic").unwrap();
    assert_eq!(native.1.get(), 1);
    assert!(
        key_metadata_cache_observation(home.path(), account)
            .unwrap()
            .is_cacheable()
    );
}
