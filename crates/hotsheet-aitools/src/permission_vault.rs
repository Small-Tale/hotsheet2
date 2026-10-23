//! Encrypted, machine-local permission-rule storage. The JSON envelope contains no rule
//! fields; the 256-bit AEAD key lives in the OS credential store.

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use base64::Engine;
#[cfg(not(any(test, feature = "test-permission-vault")))]
use hotsheet_ticketing::secrets::OsKeychain;
use hotsheet_ticketing::secrets::SecretStore;
use ring::aead::{self, Aad, LessSafeKey, Nonce, UnboundKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::permission::{ActionKey, StoredRule};

const VERSION: u8 = 4;
const LEGACY_VERSION: u8 = 3;
const ACCOUNT_PREFIX: &str = "permission-rules-v3-";
const MASTER_PREFIX: &str = "permission-rules-v4-master-";
const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_PLAINTEXT_BYTES: usize = 5 * 1024 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    version: u8,
    nonce: String,
    ciphertext: String,
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

pub(super) fn account(path: &Path) -> io::Result<String> {
    let parent = path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .canonicalize()?;
    let file = path
        .file_name()
        .ok_or_else(|| invalid("invalid permission-rule path"))?;
    let mut hash = Sha256::new();
    hash.update(parent.to_string_lossy().as_bytes());
    hash.update([0]);
    hash.update(file.to_string_lossy().as_bytes());
    Ok(format!("{ACCOUNT_PREFIX}{:x}", hash.finalize()))
}

fn legacy_key_with(path: &Path, secrets: &dyn SecretStore) -> io::Result<ActionKey> {
    let account = account(path)?;
    if let Some(encoded) = secrets.get(&account).map_err(io::Error::other)? {
        return decode_key(encoded);
    }
    Err(invalid("permission-rule credential missing"))
}

pub(super) fn master_account(path: &Path) -> io::Result<String> {
    let parent = path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .canonicalize()?;
    Ok(format!(
        "{MASTER_PREFIX}{:x}",
        Sha256::digest(parent.to_string_lossy().as_bytes())
    ))
}

fn decode_key(encoded: String) -> io::Result<ActionKey> {
    base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| invalid("invalid permission-rule credential"))?
        .try_into()
        .map_err(|_| invalid("invalid permission-rule credential"))
}

fn existing_v4_vault(path: &Path) -> io::Result<bool> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    for entry in fs::read_dir(parent)? {
        let entry = entry?;
        let candidate = entry.path();
        if candidate
            .extension()
            .is_none_or(|extension| extension != "json")
            || candidate == path
        {
            continue;
        }
        if fs::metadata(&candidate)?.len() > MAX_FILE_BYTES {
            continue;
        }
        let bytes = fs::read(&candidate)?;
        if serde_json::from_slice::<Envelope>(&bytes)
            .is_ok_and(|envelope| envelope.version == VERSION)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn master_key_with(
    path: &Path,
    secrets: &dyn SecretStore,
    may_create: bool,
) -> io::Result<ActionKey> {
    // Every rules file in this directory shares one master. Serialize creation across
    // processes, including simultaneous first opens of different hosted stores.
    let _lock = Lock::acquire(
        &path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(".permission-master"),
    )?;
    let master_account = master_account(path)?;
    let master = match secrets.get(&master_account).map_err(io::Error::other)? {
        Some(encoded) => decode_key(encoded)?,
        None if may_create => {
            // If the master was removed, a newly opened store must not replace it and
            // make already encrypted stores permanently unreadable.
            if existing_v4_vault(path)? {
                return Err(invalid("permission-rule credential missing"));
            }
            let mut key = [0; 32];
            getrandom::fill(&mut key).map_err(io::Error::other)?;
            secrets
                .set(
                    &master_account,
                    &base64::engine::general_purpose::STANDARD.encode(key),
                )
                .map_err(io::Error::other)?;
            key
        }
        None => return Err(invalid("permission-rule credential missing")),
    };
    let salt = ring::hkdf::Salt::new(ring::hkdf::HKDF_SHA256, b"hotsheet-permissions-v4");
    let key = salt.extract(&master);
    let identity = account(path)?;
    let info = [identity.as_bytes()];
    let derived = key
        .expand(&info, ring::hkdf::HKDF_SHA256)
        .map_err(|_| invalid("invalid permission-rule key"))?;
    let mut bytes = [0; 32];
    derived
        .fill(&mut bytes)
        .map_err(|_| invalid("invalid permission-rule key"))?;
    Ok(bytes)
}

fn file_version(path: &Path) -> io::Result<Option<u8>> {
    if fs::metadata(path).is_ok_and(|info| info.len() > MAX_FILE_BYTES) {
        return Err(invalid("permission-rule file exceeds size limit"));
    }
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if bytes.starts_with(b"[") {
        return Ok(None);
    }
    let envelope: Envelope =
        serde_json::from_slice(&bytes).map_err(|_| invalid("invalid permission-rule envelope"))?;
    Ok(Some(envelope.version))
}

fn key_with(path: &Path, secrets: &dyn SecretStore) -> io::Result<ActionKey> {
    match file_version(path)? {
        Some(LEGACY_VERSION) => {
            let legacy_key = legacy_key_with(path, secrets)?;
            let rules = read_locked(path, &legacy_key, LEGACY_VERSION)?;
            let key = master_key_with(path, secrets, true)?;
            write_locked(path, &key, &rules)?;
            Ok(key)
        }
        Some(VERSION) => master_key_with(path, secrets, false),
        None => master_key_with(path, secrets, true),
        Some(_) => Err(invalid("unsupported permission-rule version")),
    }
}

/// A Keychain approval belongs to the process, not to every vault access: remember each
/// key once it is read or created so later loads, mutations, and health checks never
/// prompt again. A denied read is also remembered for this process, so a refused
/// startup request cannot produce an immediate chain of identical prompts. Missing
/// keys are re-checked because another process may create them.
#[derive(Clone)]
#[cfg(any(test, not(feature = "test-permission-vault")))]
enum CachedRead {
    Present(String),
    Error(String),
}

#[cfg(any(test, not(feature = "test-permission-vault")))]
struct CachedSecrets<'a, S> {
    backend: S,
    cache: &'a std::sync::Mutex<std::collections::HashMap<String, CachedRead>>,
}

#[cfg(any(test, not(feature = "test-permission-vault")))]
impl<S: SecretStore> SecretStore for CachedSecrets<'_, S> {
    fn set(
        &self,
        account: &str,
        value: &str,
    ) -> Result<(), hotsheet_ticketing::secrets::SecretError> {
        self.backend.set(account, value)?;
        self.cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(account.into(), CachedRead::Present(value.into()));
        Ok(())
    }

    fn get(
        &self,
        account: &str,
    ) -> Result<Option<String>, hotsheet_ticketing::secrets::SecretError> {
        let mut cache = self
            .cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(value) = cache.get(account) {
            return match value {
                CachedRead::Present(value) => Ok(Some(value.clone())),
                CachedRead::Error(error) => Err(hotsheet_ticketing::secrets::SecretError::Backend(
                    error.clone(),
                )),
            };
        }
        let value = self.backend.get(account);
        match &value {
            Ok(Some(value)) => {
                cache.insert(account.into(), CachedRead::Present(value.clone()));
            }
            Err(error) => {
                cache.insert(account.into(), CachedRead::Error(error.to_string()));
            }
            Ok(None) => {}
        }
        value
    }

    fn delete(&self, account: &str) -> Result<bool, hotsheet_ticketing::secrets::SecretError> {
        self.cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(account);
        self.backend.delete(account)
    }
}

#[cfg(not(any(test, feature = "test-permission-vault")))]
fn with_secrets<T>(f: impl FnOnce(&dyn SecretStore) -> io::Result<T>) -> io::Result<T> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    static KEYS: OnceLock<Mutex<HashMap<String, CachedRead>>> = OnceLock::new();
    f(&CachedSecrets {
        backend: OsKeychain,
        cache: KEYS.get_or_init(|| Mutex::new(HashMap::new())),
    })
}

#[cfg(any(test, feature = "test-permission-vault"))]
pub(super) fn with_secrets<T>(f: impl FnOnce(&dyn SecretStore) -> io::Result<T>) -> io::Result<T> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    struct TestSecrets(&'static Mutex<HashMap<String, String>>);
    impl SecretStore for TestSecrets {
        fn set(
            &self,
            account: &str,
            value: &str,
        ) -> Result<(), hotsheet_ticketing::secrets::SecretError> {
            self.0.lock().unwrap().insert(account.into(), value.into());
            Ok(())
        }
        fn get(
            &self,
            account: &str,
        ) -> Result<Option<String>, hotsheet_ticketing::secrets::SecretError> {
            Ok(self.0.lock().unwrap().get(account).cloned())
        }
        fn delete(&self, account: &str) -> Result<bool, hotsheet_ticketing::secrets::SecretError> {
            Ok(self.0.lock().unwrap().remove(account).is_some())
        }
    }
    static TEST_KEYS: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    f(&TestSecrets(
        TEST_KEYS.get_or_init(|| Mutex::new(HashMap::new())),
    ))
}

pub(super) fn key_for(path: &Path) -> io::Result<ActionKey> {
    let _lock = Lock::acquire(path)?;
    with_secrets(|store| key_with(path, store))
}

pub(super) fn load(path: &Path) -> io::Result<Vec<StoredRule>> {
    load_with_key(path).map(|(_, rules)| rules)
}

pub(super) fn load_with_key(path: &Path) -> io::Result<(ActionKey, Vec<StoredRule>)> {
    let _lock = Lock::acquire(path)?;
    with_secrets(|store| {
        let key = key_with(path, store)?;
        let mut rules = read_locked(path, &key, VERSION)?;
        if dedupe(&mut rules) {
            write_locked(path, &key, &rules)?;
        }
        Ok((key, rules))
    })
}

pub(super) fn load_existing_with_key(path: &Path) -> io::Result<(ActionKey, Vec<StoredRule>)> {
    let _lock = Lock::acquire(path)?;
    if !path.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "permission-rule vault missing",
        ));
    }
    with_secrets(|store| {
        let key = key_with(path, store)?;
        let mut rules = read_locked(path, &key, VERSION)?;
        if dedupe(&mut rules) {
            write_locked(path, &key, &rules)?;
        }
        Ok((key, rules))
    })
}

pub(super) fn mutate<T>(
    path: &Path,
    change: impl FnOnce(&mut Vec<StoredRule>) -> io::Result<T>,
) -> io::Result<T> {
    let _lock = Lock::acquire(path)?;
    with_secrets(|store| {
        let key = key_with(path, store)?;
        let mut rules = read_locked(path, &key, VERSION)?;
        dedupe(&mut rules);
        let value = change(&mut rules)?;
        write_locked(path, &key, &rules)?;
        Ok(value)
    })
}

/// Change a registered vault only when its authenticated file still exists. Migration must
/// never recreate a missing destination and mistake that for an empty checkout vault.
pub(super) fn mutate_existing<T>(
    path: &Path,
    change: impl FnOnce(&mut Vec<StoredRule>) -> io::Result<T>,
) -> io::Result<T> {
    let _lock = Lock::acquire(path)?;
    if !path.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "permission-rule vault missing",
        ));
    }
    with_secrets(|store| {
        let key = key_with(path, store)?;
        let mut rules = read_locked(path, &key, VERSION)?;
        dedupe(&mut rules);
        let value = change(&mut rules)?;
        write_locked(path, &key, &rules)?;
        Ok(value)
    })
}

fn dedupe(rules: &mut Vec<StoredRule>) -> bool {
    let before = rules.len();
    let mut seen = HashSet::new();
    rules.reverse();
    rules
        .retain(|rule| seen.insert((rule.project.clone(), rule.tool.clone(), rule.key_material())));
    rules.reverse();
    rules.len() != before
}

fn read_locked(path: &Path, key: &ActionKey, version: u8) -> io::Result<Vec<StoredRule>> {
    if fs::metadata(path).is_ok_and(|info| info.len() > MAX_FILE_BYTES) {
        return Err(invalid("permission-rule file exceeds size limit"));
    }
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    // The old fingerprint-only format is unreleased. Scrub it atomically and require
    // approval again; even raw legacy entries must never be activated or inferred.
    if bytes.starts_with(b"[") {
        write_locked(path, key, &[])?;
        return Ok(Vec::new());
    }
    let envelope: Envelope =
        serde_json::from_slice(&bytes).map_err(|_| invalid("invalid permission-rule envelope"))?;
    if envelope.version != version {
        return Err(invalid("unsupported permission-rule version"));
    }
    let nonce: [u8; 12] = base64::engine::general_purpose::STANDARD
        .decode(envelope.nonce)
        .map_err(|_| invalid("invalid permission-rule nonce"))?
        .try_into()
        .map_err(|_| invalid("invalid permission-rule nonce"))?;
    let mut ciphertext = base64::engine::general_purpose::STANDARD
        .decode(envelope.ciphertext)
        .map_err(|_| invalid("invalid permission-rule ciphertext"))?;
    let cipher = cipher(key)?;
    let plaintext = cipher
        .open_in_place(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(if version == LEGACY_VERSION {
                b"hotsheet-permissions-v3".as_slice()
            } else {
                b"hotsheet-permissions-v4".as_slice()
            }),
            &mut ciphertext,
        )
        .map_err(|_| invalid("permission-rule authentication failed"))?;
    serde_json::from_slice(plaintext).map_err(|_| invalid("invalid permission-rule data"))
}

fn cipher(key: &ActionKey) -> io::Result<LessSafeKey> {
    Ok(LessSafeKey::new(
        UnboundKey::new(&aead::AES_256_GCM, key)
            .map_err(|_| invalid("invalid permission-rule key"))?,
    ))
}

fn write_locked(path: &Path, key: &ActionKey, rules: &[StoredRule]) -> io::Result<()> {
    write_locked_version(path, key, rules, VERSION)
}

fn write_locked_version(
    path: &Path,
    key: &ActionKey,
    rules: &[StoredRule],
    version: u8,
) -> io::Result<()> {
    let mut nonce = [0; 12];
    getrandom::fill(&mut nonce).map_err(io::Error::other)?;
    let mut ciphertext = serde_json::to_vec(rules).map_err(io::Error::other)?;
    if ciphertext.len() > MAX_PLAINTEXT_BYTES {
        return Err(invalid("permission-rule data exceeds size limit"));
    }
    cipher(key)?
        .seal_in_place_append_tag(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(if version == LEGACY_VERSION {
                b"hotsheet-permissions-v3".as_slice()
            } else {
                b"hotsheet-permissions-v4".as_slice()
            }),
            &mut ciphertext,
        )
        .map_err(|_| invalid("permission-rule encryption failed"))?;
    let envelope = Envelope {
        version,
        nonce: base64::engine::general_purpose::STANDARD.encode(nonce),
        ciphertext: base64::engine::general_purpose::STANDARD.encode(ciphertext),
    };
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let old_key = parent.join(".action-key");
    if old_key.exists() {
        fs::remove_file(old_key)?;
    }
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temp.as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    serde_json::to_writer(&mut temp, &envelope).map_err(io::Error::other)?;
    temp.write_all(b"\n")?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|error| error.error)?;
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(())
}

struct Lock(File);

impl Lock {
    fn acquire(path: &Path) -> io::Result<Self> {
        let lock_path = PathBuf::from(format!("{}.lock", path.display()));
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut options = OpenOptions::new();
        options.write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(lock_path)?;
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
                return Err(io::Error::last_os_error());
            }
        }
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::Storage::FileSystem::{LOCKFILE_EXCLUSIVE_LOCK, LockFileEx};
            use windows_sys::Win32::System::IO::OVERLAPPED;
            let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
            if unsafe {
                LockFileEx(
                    file.as_raw_handle(),
                    LOCKFILE_EXCLUSIVE_LOCK,
                    0,
                    u32::MAX,
                    u32::MAX,
                    &mut overlapped,
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(Self(file))
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            let _ = unsafe { libc::flock(self.0.as_raw_fd(), libc::LOCK_UN) };
        }
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::Storage::FileSystem::UnlockFileEx;
            use windows_sys::Win32::System::IO::OVERLAPPED;
            let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
            let _ = unsafe {
                UnlockFileEx(
                    self.0.as_raw_handle(),
                    0,
                    u32::MAX,
                    u32::MAX,
                    &mut overlapped,
                )
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permission::Decision;
    use std::sync::Arc;

    fn rule(action: &str) -> StoredRule {
        StoredRule {
            project: "project".into(),
            tool: "Bash".into(),
            action: action.into(),
            pattern: None,
            description: None,
            decision: Decision::Allow,
        }
    }

    #[test]
    fn encrypted_round_trip_rejects_tampering_and_keeps_key_out_of_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rules.json");
        mutate(&path, |rules| {
            rules.push(rule("TOKEN=secret npm test"));
            Ok(())
        })
        .unwrap();
        let original = fs::read(&path).unwrap();
        assert!(
            !original
                .windows(b"TOKEN=secret".len())
                .any(|v| v == b"TOKEN=secret")
        );
        assert!(!dir.path().join(".action-key").exists());
        assert_eq!(load(&path).unwrap(), vec![rule("TOKEN=secret npm test")]);

        let mut envelope: Envelope = serde_json::from_slice(&original).unwrap();
        let mut bytes = base64::engine::general_purpose::STANDARD
            .decode(&envelope.ciphertext)
            .unwrap();
        bytes[0] ^= 1;
        envelope.ciphertext = base64::engine::general_purpose::STANDARD.encode(bytes);
        fs::write(&path, serde_json::to_vec(&envelope).unwrap()).unwrap();
        assert_eq!(load(&path).unwrap_err().kind(), io::ErrorKind::InvalidData);
        assert_eq!(
            fs::read(&path).unwrap(),
            serde_json::to_vec(&envelope).unwrap()
        );
        assert!(
            mutate(&path, |_| Ok(())).is_err(),
            "a corrupt file is not overwritten"
        );
    }

    #[test]
    fn missing_key_never_replaces_a_key_for_existing_ciphertext() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rules.json");
        mutate(&path, |rules| {
            rules.push(rule("ls"));
            Ok(())
        })
        .unwrap();
        let original = fs::read(&path).unwrap();
        with_secrets(|store| {
            assert!(
                store
                    .delete(&master_account(&path)?)
                    .map_err(io::Error::other)?
            );
            Ok(())
        })
        .unwrap();
        assert!(key_for(&path).is_err());
        assert!(load(&path).is_err());
        assert!(
            mutate(&path, |rules| {
                rules.push(rule("pwd"));
                Ok(())
            })
            .is_err()
        );
        assert_eq!(fs::read(&path).unwrap(), original);
        // Recovery is explicit: discard the unreadable file, then approve anew.
        fs::remove_file(&path).unwrap();
        mutate(&path, |rules| {
            rules.push(rule("pwd"));
            Ok(())
        })
        .unwrap();
        assert_eq!(load(&path).unwrap(), vec![rule("pwd")]);
    }

    #[test]
    fn hosted_vaults_share_one_master_but_keep_distinct_keys_and_migrate_v3() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first.json");
        let second = dir.path().join("second.json");
        fs::write(
            dir.path().join("hotsheet-store.json"),
            r#"{"version":2,"store":"HS"}"#,
        )
        .unwrap();
        let legacy_key = [7; 32];
        with_secrets(|store| {
            store
                .set(
                    &account(&first)?,
                    &base64::engine::general_purpose::STANDARD.encode(legacy_key),
                )
                .map_err(io::Error::other)
        })
        .unwrap();
        write_locked_version(&first, &legacy_key, &[rule("old approval")], LEGACY_VERSION).unwrap();

        let first_key = key_for(&first).unwrap();
        assert_eq!(file_version(&first).unwrap(), Some(VERSION));
        assert_eq!(load(&first).unwrap(), vec![rule("old approval")]);
        mutate(&second, |rules| {
            rules.push(rule("other approval"));
            Ok(())
        })
        .unwrap();
        let second_key = key_for(&second).unwrap();
        assert_ne!(first_key, second_key);
        assert_eq!(load(&second).unwrap(), vec![rule("other approval")]);
        assert_eq!(load(&first).unwrap(), vec![rule("old approval")]);

        // A v3 item is no longer read once its file has moved to the shared master.
        with_secrets(|store| {
            assert!(store.delete(&account(&first)?).map_err(io::Error::other)?);
            Ok(())
        })
        .unwrap();
        assert_eq!(load(&first).unwrap(), vec![rule("old approval")]);
        // Removing the master disables both existing vaults instead of activating them
        // with a replacement key.
        with_secrets(|store| {
            assert!(
                store
                    .delete(&master_account(&first)?)
                    .map_err(io::Error::other)?
            );
            Ok(())
        })
        .unwrap();
        assert!(load(&first).is_err());
        assert!(load(&second).is_err());
        let third = dir.path().join("third.json");
        assert!(key_for(&third).is_err());
        with_secrets(|store| {
            assert!(
                store
                    .get(&master_account(&third)?)
                    .map_err(io::Error::other)?
                    .is_none()
            );
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn cached_secrets_read_the_backend_once_per_present_key() {
        use std::collections::HashMap;
        use std::sync::Mutex;
        use std::sync::atomic::{AtomicUsize, Ordering};

        #[derive(Default)]
        struct Counting {
            values: Mutex<HashMap<String, String>>,
            gets: AtomicUsize,
        }
        impl SecretStore for &Counting {
            fn set(
                &self,
                account: &str,
                value: &str,
            ) -> Result<(), hotsheet_ticketing::secrets::SecretError> {
                self.values
                    .lock()
                    .unwrap()
                    .insert(account.into(), value.into());
                Ok(())
            }
            fn get(
                &self,
                account: &str,
            ) -> Result<Option<String>, hotsheet_ticketing::secrets::SecretError> {
                self.gets.fetch_add(1, Ordering::SeqCst);
                Ok(self.values.lock().unwrap().get(account).cloned())
            }
            fn delete(
                &self,
                account: &str,
            ) -> Result<bool, hotsheet_ticketing::secrets::SecretError> {
                Ok(self.values.lock().unwrap().remove(account).is_some())
            }
        }

        let backend = Counting::default();
        let cache = Mutex::new(HashMap::new());
        let store = CachedSecrets {
            backend: &backend,
            cache: &cache,
        };
        // A missing key is never cached, so a later creation is still seen.
        assert_eq!(store.get("a").unwrap(), None);
        assert_eq!(store.get("a").unwrap(), None);
        assert_eq!(backend.gets.load(Ordering::SeqCst), 2);
        backend
            .values
            .lock()
            .unwrap()
            .insert("a".into(), "one".into());
        for _ in 0..5 {
            assert_eq!(store.get("a").unwrap().as_deref(), Some("one"));
        }
        assert_eq!(backend.gets.load(Ordering::SeqCst), 3);
        // A key this process creates is served without reading it back.
        store.set("b", "two").unwrap();
        assert_eq!(store.get("b").unwrap().as_deref(), Some("two"));
        assert_eq!(backend.gets.load(Ordering::SeqCst), 3);
        // Deleting forgets the cached value; refill then reads the backend again.
        assert!(store.delete("a").unwrap());
        assert_eq!(store.get("a").unwrap(), None);
        store.set("a", "three").unwrap();
        assert_eq!(store.get("a").unwrap().as_deref(), Some("three"));
        assert_eq!(backend.gets.load(Ordering::SeqCst), 4);

        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first.json");
        let second = dir.path().join("second.json");
        assert_ne!(
            key_with(&first, &store).unwrap(),
            key_with(&second, &store).unwrap()
        );
        assert_eq!(
            key_with(&first, &store).unwrap(),
            key_with(&first, &store).unwrap()
        );
        assert_eq!(
            backend.gets.load(Ordering::SeqCst),
            5,
            "all hosted stores read one master credential"
        );
    }

    #[test]
    fn denied_credential_is_not_requested_again_during_the_process() {
        use std::collections::HashMap;
        use std::sync::Mutex;
        use std::sync::atomic::{AtomicUsize, Ordering};

        struct Denied(AtomicUsize);
        impl SecretStore for &Denied {
            fn set(
                &self,
                _account: &str,
                _value: &str,
            ) -> Result<(), hotsheet_ticketing::secrets::SecretError> {
                unreachable!()
            }
            fn get(
                &self,
                _account: &str,
            ) -> Result<Option<String>, hotsheet_ticketing::secrets::SecretError> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Err(hotsheet_ticketing::secrets::SecretError::Backend(
                    "user denied access".into(),
                ))
            }
            fn delete(
                &self,
                _account: &str,
            ) -> Result<bool, hotsheet_ticketing::secrets::SecretError> {
                unreachable!()
            }
        }

        let backend = Denied(AtomicUsize::new(0));
        let cache = Mutex::new(HashMap::new());
        let store = CachedSecrets {
            backend: &backend,
            cache: &cache,
        };
        for _ in 0..20 {
            assert!(store.get("permission-rules-v4-master").is_err());
        }
        assert_eq!(backend.0.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn concurrent_writers_keep_every_rule_and_legacy_values_are_discarded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rules.json");
        fs::write(
            &path,
            b"[{\"tool\":\"Bash\",\"action\":\"secret\",\"decision\":\"allow\"}]",
        )
        .unwrap();
        fs::write(dir.path().join(".action-key"), b"obsolete").unwrap();
        assert!(load(&path).unwrap().is_empty());
        assert!(!fs::read_to_string(&path).unwrap().contains("secret"));
        assert!(!dir.path().join(".action-key").exists());

        let path = Arc::new(path);
        let workers: Vec<_> = (0..12)
            .map(|index| {
                let path = Arc::clone(&path);
                std::thread::spawn(move || {
                    mutate(&path, |rules| {
                        rules.push(rule(&format!("command-{index}")));
                        Ok(())
                    })
                    .unwrap()
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.len(), 12);
        for index in 0..12 {
            assert!(
                loaded
                    .iter()
                    .any(|entry| entry.action == format!("command-{index}"))
            );
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&*path).unwrap().permissions().mode() & 0o077,
                0
            );
        }
    }
}
