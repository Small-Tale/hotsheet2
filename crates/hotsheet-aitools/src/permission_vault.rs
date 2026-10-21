//! Encrypted, machine-local permission-rule storage. The JSON envelope contains no rule
//! fields; the 256-bit AEAD key lives in the OS credential store.

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use base64::Engine;
#[cfg(not(test))]
use hotsheet_ticketing::secrets::OsKeychain;
use hotsheet_ticketing::secrets::SecretStore;
use ring::aead::{self, Aad, LessSafeKey, Nonce, UnboundKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::permission::{ActionKey, StoredRule};

const VERSION: u8 = 3;
const ACCOUNT_PREFIX: &str = "permission-rules-v3-";
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

fn key_with(path: &Path, secrets: &dyn SecretStore) -> io::Result<ActionKey> {
    let account = account(path)?;
    if let Some(encoded) = secrets.get(&account).map_err(io::Error::other)? {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| invalid("invalid permission-rule credential"))?;
        return bytes
            .try_into()
            .map_err(|_| invalid("invalid permission-rule credential"));
    }
    // A missing key for a v3 envelope means the old approvals are unrecoverable. Do not
    // replace it and accidentally activate a different rule set under a fresh key.
    if let Ok(mut file) = File::open(path) {
        let mut first = [0];
        if file.read(&mut first)? != 1 || first != *b"[" {
            return Err(invalid("permission-rule credential missing"));
        }
    }
    let mut key = [0; 32];
    getrandom::fill(&mut key).map_err(io::Error::other)?;
    secrets
        .set(
            &account,
            &base64::engine::general_purpose::STANDARD.encode(key),
        )
        .map_err(io::Error::other)?;
    Ok(key)
}

#[cfg(not(test))]
fn with_secrets<T>(f: impl FnOnce(&dyn SecretStore) -> io::Result<T>) -> io::Result<T> {
    f(&OsKeychain)
}

#[cfg(test)]
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
    let _lock = Lock::acquire(path)?;
    with_secrets(|store| {
        let key = key_with(path, store)?;
        let mut rules = read_locked(path, &key)?;
        if dedupe(&mut rules) {
            write_locked(path, &key, &rules)?;
        }
        Ok(rules)
    })
}

pub(super) fn mutate<T>(
    path: &Path,
    change: impl FnOnce(&mut Vec<StoredRule>) -> io::Result<T>,
) -> io::Result<T> {
    let _lock = Lock::acquire(path)?;
    with_secrets(|store| {
        let key = key_with(path, store)?;
        let mut rules = read_locked(path, &key)?;
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
        .retain(|rule| seen.insert((rule.project.clone(), rule.tool.clone(), rule.action.clone())));
    rules.reverse();
    rules.len() != before
}

fn read_locked(path: &Path, key: &ActionKey) -> io::Result<Vec<StoredRule>> {
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
    if envelope.version != VERSION {
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
            Aad::from(b"hotsheet-permissions-v3"),
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
    let mut nonce = [0; 12];
    getrandom::fill(&mut nonce).map_err(io::Error::other)?;
    let mut ciphertext = serde_json::to_vec(rules).map_err(io::Error::other)?;
    if ciphertext.len() > MAX_PLAINTEXT_BYTES {
        return Err(invalid("permission-rule data exceeds size limit"));
    }
    cipher(key)?
        .seal_in_place_append_tag(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(b"hotsheet-permissions-v3"),
            &mut ciphertext,
        )
        .map_err(|_| invalid("permission-rule encryption failed"))?;
    let envelope = Envelope {
        version: VERSION,
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
            assert!(store.delete(&account(&path)?).map_err(io::Error::other)?);
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
