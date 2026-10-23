//! Machine-local encrypted secrets behind one native credential-store item.
//!
//! Legacy per-account items are copied on first read and retained for recovery. Once
//! an envelope exists, a missing or corrupt master never falls back to a legacy item.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use base64::Engine;
use ring::{aead, hkdf};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{NativeKeychain, SecretError, SecretStore};

const ROOT_ACCOUNT: &str = "machine-secrets-v1-master";
const ROTATION_ACCOUNT: &str = "machine-secrets-v1-rotation";
const VERSION: u8 = 1;
const MAX_ENVELOPE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_SECRET_BYTES: usize = 1024 * 1024;

struct CachedRoot {
    revision: Option<Vec<u8>>,
    value: Result<[u8; 32], String>,
}

type RootCache = Arc<Mutex<HashMap<PathBuf, CachedRoot>>>;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    version: u8,
    account: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    deleted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    nonce: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    ciphertext: Option<String>,
}

pub(super) fn with_machine_store<T>(
    f: impl FnOnce(&MachineSecrets<NativeKeychain>) -> Result<T, SecretError>,
) -> Result<T, SecretError> {
    static CACHE: OnceLock<RootCache> = OnceLock::new();
    let home = std::env::var_os("HOTSHEET_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(std::env::temp_dir)
                .join(".hotsheet2")
        });
    f(&MachineSecrets::with_cache(
        home,
        NativeKeychain,
        Arc::clone(CACHE.get_or_init(|| Arc::new(Mutex::new(HashMap::new())))),
    ))
}

pub(super) struct MachineSecrets<S> {
    home: PathBuf,
    native: S,
    roots: RootCache,
}

impl<S: SecretStore> MachineSecrets<S> {
    #[cfg(test)]
    fn new(home: PathBuf, native: S) -> Self {
        Self::with_cache(home, native, Arc::new(Mutex::new(HashMap::new())))
    }

    fn with_cache(home: PathBuf, native: S, roots: RootCache) -> Self {
        Self {
            home,
            native,
            roots,
        }
    }

    fn directory(&self) -> PathBuf {
        self.home.join("machine-secrets")
    }

    fn prepare(&self) -> Result<PathBuf, SecretError> {
        let directory = self.directory();
        fs::create_dir_all(&directory)?;
        if fs::symlink_metadata(&directory)?.file_type().is_symlink() {
            return Err(SecretError::Backend(
                "machine secret directory must not be a symlink".into(),
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
        }
        Ok(directory.canonicalize()?)
    }

    fn envelope_path(directory: &Path, account: &str) -> PathBuf {
        directory.join(format!("{:x}.json", Sha256::digest(account.as_bytes())))
    }

    fn revision(directory: &Path) -> Result<Option<Vec<u8>>, SecretError> {
        match fs::read(directory.join("revision")) {
            Ok(bytes) if bytes.len() <= 128 => Ok(Some(bytes)),
            Ok(_) => Err(SecretError::Backend(
                "machine master revision exceeds size limit".into(),
            )),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    fn bump_revision(directory: &Path) -> Result<Vec<u8>, SecretError> {
        let mut revision = [0; 16];
        getrandom::fill(&mut revision).map_err(|error| SecretError::Backend(error.to_string()))?;
        let bytes = format!("{:x}", Sha256::digest(revision)).into_bytes();
        let mut staged = tempfile::NamedTempFile::new_in(directory)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            staged
                .as_file()
                .set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        staged.write_all(&bytes)?;
        staged.as_file().sync_all()?;
        staged
            .persist(directory.join("revision"))
            .map_err(|error| error.error)?;
        #[cfg(unix)]
        File::open(directory)?.sync_all()?;
        Ok(bytes)
    }

    fn forget_root(&self, directory: &Path) {
        self.roots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(directory);
    }

    fn remember_root(&self, directory: &Path, value: [u8; 32]) -> Result<(), SecretError> {
        let revision = Self::revision(directory)?;
        self.roots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(
                directory.to_path_buf(),
                CachedRoot {
                    revision,
                    value: Ok(value),
                },
            );
        Ok(())
    }

    fn root(&self, directory: &Path, may_create: bool) -> Result<[u8; 32], SecretError> {
        let revision = Self::revision(directory)?;
        let mut roots = self
            .roots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(cached) = roots.get(directory) {
            if cached.revision == revision {
                return cached.value.clone().map_err(SecretError::Backend);
            }
        }
        let native = match self.native.get(ROOT_ACCOUNT) {
            Ok(value) => value,
            Err(error) => {
                roots.insert(
                    directory.to_path_buf(),
                    CachedRoot {
                        revision,
                        value: Err(error.to_string()),
                    },
                );
                return Err(error);
            }
        };
        let value = match native {
            Some(encoded) => match decode_root(&encoded) {
                Ok(value) => value,
                Err(error) => {
                    roots.insert(
                        directory.to_path_buf(),
                        CachedRoot {
                            revision,
                            value: Err(error.to_string()),
                        },
                    );
                    return Err(error);
                }
            },
            None if may_create => {
                // Envelopes already present prove that a missing root was lost or denied.
                // Never mint an unrelated replacement that would strand their contents.
                for entry in fs::read_dir(directory)? {
                    if entry?.path().extension().is_some_and(|ext| ext == "json") {
                        let message =
                            "machine secret master missing; restore the original Keychain item";
                        roots.insert(
                            directory.to_path_buf(),
                            CachedRoot {
                                revision,
                                value: Err(message.into()),
                            },
                        );
                        return Err(SecretError::Backend(message.into()));
                    }
                }
                let mut key = [0; 32];
                getrandom::fill(&mut key)
                    .map_err(|error| SecretError::Backend(error.to_string()))?;
                self.native.set(
                    ROOT_ACCOUNT,
                    &base64::engine::general_purpose::STANDARD.encode(key),
                )?;
                let revision = Self::bump_revision(directory)?;
                roots.insert(
                    directory.to_path_buf(),
                    CachedRoot {
                        revision: Some(revision),
                        value: Ok(key),
                    },
                );
                return Ok(key);
            }
            None => {
                let message = "machine secret master missing; restore the original Keychain item";
                roots.insert(
                    directory.to_path_buf(),
                    CachedRoot {
                        revision,
                        value: Err(message.into()),
                    },
                );
                return Err(SecretError::Backend(message.into()));
            }
        };
        roots.insert(
            directory.to_path_buf(),
            CachedRoot {
                revision,
                value: Ok(value),
            },
        );
        Ok(value)
    }

    fn read_envelope(path: &Path) -> Result<Option<Envelope>, SecretError> {
        match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(SecretError::Backend(
                    "machine secret envelope must not be a symlink".into(),
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        }
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        if file.metadata()?.len() > MAX_ENVELOPE_BYTES {
            return Err(SecretError::Backend(
                "machine secret envelope exceeds size limit".into(),
            ));
        }
        let mut bytes = Vec::new();
        file.take(MAX_ENVELOPE_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_ENVELOPE_BYTES {
            return Err(SecretError::Backend(
                "machine secret envelope exceeds size limit".into(),
            ));
        }
        let envelope: Envelope = serde_json::from_slice(&bytes)
            .map_err(|_| SecretError::Backend("invalid machine secret envelope".into()))?;
        if envelope.version != VERSION {
            return Err(SecretError::Backend(
                "unsupported machine secret envelope".into(),
            ));
        }
        Ok(Some(envelope))
    }

    fn read_account(path: &Path, account: &str) -> Result<Option<Envelope>, SecretError> {
        let envelope = Self::read_envelope(path)?;
        if envelope
            .as_ref()
            .is_some_and(|envelope| envelope.account != account)
        {
            return Err(SecretError::Backend(
                "machine secret envelope account mismatch".into(),
            ));
        }
        Ok(envelope)
    }

    fn write_envelope(path: &Path, envelope: &Envelope) -> Result<(), SecretError> {
        let parent = path.parent().expect("envelope parent");
        let mut staged = tempfile::NamedTempFile::new_in(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            staged
                .as_file()
                .set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        serde_json::to_writer(&mut staged, envelope)
            .map_err(|error| SecretError::Backend(error.to_string()))?;
        staged.write_all(b"\n")?;
        staged.as_file().sync_all()?;
        staged.persist(path).map_err(|error| error.error)?;
        #[cfg(unix)]
        File::open(parent)?.sync_all()?;
        Ok(())
    }

    fn key(root: &[u8; 32], account: &str) -> Result<[u8; 32], SecretError> {
        let salt = hkdf::Salt::new(hkdf::HKDF_SHA256, b"hotsheet-machine-secrets-v1");
        let prk = salt.extract(root);
        let mut key = [0; 32];
        prk.expand(&[account.as_bytes()], hkdf::HKDF_SHA256)
            .map_err(|_| SecretError::Backend("invalid machine secret key".into()))?
            .fill(&mut key)
            .map_err(|_| SecretError::Backend("invalid machine secret key".into()))?;
        Ok(key)
    }

    fn cipher(root: &[u8; 32], account: &str) -> Result<aead::LessSafeKey, SecretError> {
        Ok(aead::LessSafeKey::new(
            aead::UnboundKey::new(&aead::AES_256_GCM, &Self::key(root, account)?)
                .map_err(|_| SecretError::Backend("invalid machine secret key".into()))?,
        ))
    }

    fn encrypt(root: &[u8; 32], account: &str, value: &str) -> Result<Envelope, SecretError> {
        if value.len() > MAX_SECRET_BYTES {
            return Err(SecretError::Backend(
                "machine secret exceeds size limit".into(),
            ));
        }
        let mut nonce = [0; 12];
        getrandom::fill(&mut nonce).map_err(|error| SecretError::Backend(error.to_string()))?;
        let mut bytes = value.as_bytes().to_vec();
        Self::cipher(root, account)?
            .seal_in_place_append_tag(
                aead::Nonce::assume_unique_for_key(nonce),
                aead::Aad::from(account.as_bytes()),
                &mut bytes,
            )
            .map_err(|_| SecretError::Backend("machine secret encryption failed".into()))?;
        Ok(Envelope {
            version: VERSION,
            account: account.into(),
            deleted: false,
            nonce: Some(base64::engine::general_purpose::STANDARD.encode(nonce)),
            ciphertext: Some(base64::engine::general_purpose::STANDARD.encode(bytes)),
        })
    }

    fn decrypt(root: &[u8; 32], account: &str, envelope: &Envelope) -> Result<String, SecretError> {
        let decode = |text: &Option<String>| {
            text.as_ref()
                .ok_or_else(|| SecretError::Backend("invalid machine secret envelope".into()))
                .and_then(|text| {
                    base64::engine::general_purpose::STANDARD
                        .decode(text)
                        .map_err(|_| SecretError::Backend("invalid machine secret envelope".into()))
                })
        };
        let nonce: [u8; 12] = decode(&envelope.nonce)?
            .try_into()
            .map_err(|_| SecretError::Backend("invalid machine secret nonce".into()))?;
        let mut bytes = decode(&envelope.ciphertext)?;
        let plaintext = Self::cipher(root, account)?
            .open_in_place(
                aead::Nonce::assume_unique_for_key(nonce),
                aead::Aad::from(account.as_bytes()),
                &mut bytes,
            )
            .map_err(|_| SecretError::Backend("machine secret authentication failed".into()))?;
        String::from_utf8(plaintext.to_vec())
            .map_err(|_| SecretError::Backend("machine secret is not UTF-8".into()))
    }

    fn rotation_path(directory: &Path) -> PathBuf {
        directory.join("rotation.json")
    }

    fn old_root(directory: &Path, current: &[u8; 32]) -> Result<Option<[u8; 32]>, SecretError> {
        let Some(wrap) = Self::read_account(&Self::rotation_path(directory), ROTATION_ACCOUNT)?
        else {
            return Ok(None);
        };
        if wrap.deleted {
            return Err(SecretError::Backend(
                "invalid machine master rotation".into(),
            ));
        }
        decode_root(&Self::decrypt(current, ROTATION_ACCOUNT, &wrap)?).map(Some)
    }

    fn decrypt_current(
        directory: &Path,
        current: &[u8; 32],
        envelope: &Envelope,
    ) -> Result<String, SecretError> {
        match Self::decrypt(current, &envelope.account, envelope) {
            Ok(value) => Ok(value),
            Err(error) => match Self::old_root(directory, current)? {
                Some(previous) => Self::decrypt(&previous, &envelope.account, envelope),
                None => Err(error),
            },
        }
    }

    fn finish_rotation(directory: &Path, current: &[u8; 32]) -> Result<bool, SecretError> {
        match Self::old_root(directory, current) {
            Ok(None) => return Ok(false),
            Ok(Some(_)) => {}
            Err(error) => {
                // The process may have stopped after writing the wrap but before
                // replacing the native root. In that case the old root still opens
                // every envelope, and removing the unused wrap is safe.
                let all_current = fs::read_dir(directory)?.try_fold(true, |all, entry| {
                    let path = entry?.path();
                    if !all
                        || path == Self::rotation_path(directory)
                        || path.extension().is_none_or(|ext| ext != "json")
                    {
                        return Ok::<_, SecretError>(all);
                    }
                    let envelope = Self::read_envelope(&path)?
                        .ok_or_else(|| SecretError::Backend("machine secret disappeared".into()))?;
                    reserved(&envelope.account)?;
                    if path != Self::envelope_path(directory, &envelope.account) {
                        return Err(SecretError::Backend(
                            "machine secret envelope filename mismatch".into(),
                        ));
                    }
                    Ok(envelope.deleted
                        || Self::decrypt(current, &envelope.account, &envelope).is_ok())
                })?;
                if !all_current {
                    return Err(error);
                }
                fs::remove_file(Self::rotation_path(directory))?;
                #[cfg(unix)]
                File::open(directory)?.sync_all()?;
                return Ok(true);
            }
        }
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            if path == Self::rotation_path(directory)
                || path.extension().is_none_or(|ext| ext != "json")
            {
                continue;
            }
            let Some(envelope) = Self::read_envelope(&path)? else {
                continue;
            };
            reserved(&envelope.account)?;
            if path != Self::envelope_path(directory, &envelope.account) {
                return Err(SecretError::Backend(
                    "machine secret envelope filename mismatch".into(),
                ));
            }
            if envelope.deleted {
                continue;
            }
            let value = Self::decrypt_current(directory, current, &envelope)?;
            Self::write_envelope(&path, &Self::encrypt(current, &envelope.account, &value)?)?;
        }
        fs::remove_file(Self::rotation_path(directory))?;
        #[cfg(unix)]
        File::open(directory)?.sync_all()?;
        Ok(true)
    }

    pub(super) fn export_master(&self) -> Result<String, SecretError> {
        let directory = self.prepare()?;
        let _lock = Lock::acquire(&directory)?;
        self.forget_root(&directory);
        let root = self.root(&directory, false)?;
        Ok(base64::engine::general_purpose::STANDARD.encode(root))
    }

    pub(super) fn restore_master(&self, encoded: &str) -> Result<(), SecretError> {
        let candidate = decode_root(encoded.trim())?;
        let directory = self.prepare()?;
        let _lock = Lock::acquire(&directory)?;
        if let Some(existing) = self.native.get(ROOT_ACCOUNT)? {
            if decode_root(&existing)? == candidate {
                Self::bump_revision(&directory)?;
                self.remember_root(&directory, candidate)?;
                return Ok(());
            }
            return Err(SecretError::Backend(
                "machine master already exists; use rotation instead of restore".into(),
            ));
        }
        let mut verified = false;
        for entry in fs::read_dir(&directory)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_none_or(|ext| ext != "json")
                || path == Self::rotation_path(&directory)
            {
                continue;
            }
            let Some(envelope) = Self::read_envelope(&path)? else {
                continue;
            };
            reserved(&envelope.account)?;
            if path != Self::envelope_path(&directory, &envelope.account) {
                return Err(SecretError::Backend(
                    "machine secret envelope filename mismatch".into(),
                ));
            }
            if !envelope.deleted {
                Self::decrypt_current(&directory, &candidate, &envelope)?;
                verified = true;
            }
        }
        if !verified {
            return Err(SecretError::Backend(
                "no encrypted secret available to verify the restored master".into(),
            ));
        }
        self.native.set(ROOT_ACCOUNT, encoded.trim())?;
        Self::bump_revision(&directory)?;
        self.remember_root(&directory, candidate)?;
        Ok(())
    }

    pub(super) fn rotate_master(&self) -> Result<(), SecretError> {
        let directory = self.prepare()?;
        let _lock = Lock::acquire(&directory)?;
        self.forget_root(&directory);
        let current = self.root(&directory, false)?;
        // An interrupted prior rotation finishes under its original recovery wrap.
        if Self::rotation_path(&directory).exists() {
            Self::bump_revision(&directory)?;
            self.remember_root(&directory, current)?;
        }
        Self::finish_rotation(&directory, &current)?;
        let mut next = [0; 32];
        getrandom::fill(&mut next).map_err(|error| SecretError::Backend(error.to_string()))?;
        let wrap = Self::encrypt(
            &next,
            ROTATION_ACCOUNT,
            &base64::engine::general_purpose::STANDARD.encode(current),
        )?;
        let rotation_path = Self::rotation_path(&directory);
        Self::write_envelope(&rotation_path, &wrap)?;
        if let Err(error) = self.native.set(
            ROOT_ACCOUNT,
            &base64::engine::general_purpose::STANDARD.encode(next),
        ) {
            // The old native master still opens every existing envelope.
            let _ = fs::remove_file(&rotation_path);
            return Err(error);
        }
        Self::bump_revision(&directory)?;
        self.remember_root(&directory, next)?;
        Self::finish_rotation(&directory, &next)?;
        Ok(())
    }

    pub(super) fn resume_rotation(&self) -> Result<bool, SecretError> {
        let directory = self.prepare()?;
        let _lock = Lock::acquire(&directory)?;
        self.forget_root(&directory);
        let current = self.root(&directory, false)?;
        if Self::rotation_path(&directory).exists() {
            Self::bump_revision(&directory)?;
            self.remember_root(&directory, current)?;
        }
        Self::finish_rotation(&directory, &current)
    }

    pub(super) fn cleanup_legacy(&self, encoded_backup: &str) -> Result<usize, SecretError> {
        let directory = self.prepare()?;
        let _lock = Lock::acquire(&directory)?;
        self.forget_root(&directory);
        let current = self.root(&directory, false)?;
        if decode_root(encoded_backup.trim())? != current {
            return Err(SecretError::Backend(
                "backup does not match the current machine master".into(),
            ));
        }
        if Self::rotation_path(&directory).exists() {
            Self::bump_revision(&directory)?;
            self.remember_root(&directory, current)?;
        }
        Self::finish_rotation(&directory, &current)?;
        let mut accounts = Vec::new();
        for entry in fs::read_dir(&directory)? {
            let path = entry?.path();
            if path.extension().is_none_or(|ext| ext != "json") {
                continue;
            }
            let envelope = Self::read_envelope(&path)?
                .ok_or_else(|| SecretError::Backend("machine secret disappeared".into()))?;
            reserved(&envelope.account)?;
            if path != Self::envelope_path(&directory, &envelope.account) {
                return Err(SecretError::Backend(
                    "machine secret envelope filename mismatch".into(),
                ));
            }
            if !envelope.deleted {
                Self::decrypt(&current, &envelope.account, &envelope)?;
            }
            accounts.push(envelope.account);
        }
        let mut deleted = 0;
        for account in accounts {
            deleted += usize::from(self.native.delete(&account)?);
        }
        Ok(deleted)
    }
}

impl<S: SecretStore> SecretStore for MachineSecrets<S> {
    fn set(&self, account: &str, secret: &str) -> Result<(), SecretError> {
        reserved(account)?;
        let directory = self.prepare()?;
        let _lock = Lock::acquire(&directory)?;
        let root = self.root(&directory, true)?;
        let path = Self::envelope_path(&directory, account);
        if let Some(existing) = Self::read_account(&path, account)? {
            if !existing.deleted {
                Self::decrypt_current(&directory, &root, &existing)?;
            }
        }
        Self::write_envelope(&path, &Self::encrypt(&root, account, secret)?)?;
        let saved = Self::read_account(&path, account)?.expect("just wrote envelope");
        if Self::decrypt(&root, account, &saved)? != secret {
            return Err(SecretError::Backend(
                "machine secret verification failed".into(),
            ));
        }
        Ok(())
    }

    fn get(&self, account: &str) -> Result<Option<String>, SecretError> {
        reserved(account)?;
        let directory = self.prepare()?;
        let _lock = Lock::acquire(&directory)?;
        let path = Self::envelope_path(&directory, account);
        if let Some(envelope) = Self::read_account(&path, account)? {
            if envelope.deleted {
                return Ok(None);
            }
            let root = self.root(&directory, false)?;
            return Self::decrypt_current(&directory, &root, &envelope).map(Some);
        }
        let Some(legacy) = self.native.get(account)? else {
            return Ok(None);
        };
        let root = self.root(&directory, true)?;
        Self::write_envelope(&path, &Self::encrypt(&root, account, &legacy)?)?;
        let saved = Self::read_account(&path, account)?.expect("just wrote envelope");
        if Self::decrypt(&root, account, &saved)? != legacy {
            return Err(SecretError::Backend(
                "machine secret migration verification failed".into(),
            ));
        }
        Ok(Some(legacy))
    }

    fn delete(&self, account: &str) -> Result<bool, SecretError> {
        reserved(account)?;
        let directory = self.prepare()?;
        let _lock = Lock::acquire(&directory)?;
        let path = Self::envelope_path(&directory, account);
        let root = self.root(&directory, true)?;
        let existed = match Self::read_account(&path, account)? {
            Some(envelope) if !envelope.deleted => {
                Self::decrypt_current(&directory, &root, &envelope)?;
                true
            }
            _ => false,
        };
        // A durable tombstone prevents a retained legacy item from resurfacing after
        // deletion or after an interrupted deletion.
        Self::write_envelope(
            &path,
            &Envelope {
                version: VERSION,
                account: account.into(),
                deleted: true,
                nonce: None,
                ciphertext: None,
            },
        )?;
        let legacy_deleted = self.native.delete(account)?;
        Ok(existed || legacy_deleted)
    }
}

fn reserved(account: &str) -> Result<(), SecretError> {
    if account == ROOT_ACCOUNT || account == ROTATION_ACCOUNT {
        Err(SecretError::Backend(
            "machine master account is reserved".into(),
        ))
    } else {
        Ok(())
    }
}

fn decode_root(encoded: &str) -> Result<[u8; 32], SecretError> {
    base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| SecretError::Backend("invalid machine secret master".into()))?
        .try_into()
        .map_err(|_| SecretError::Backend("invalid machine secret master".into()))
}

struct Lock(File);

impl Lock {
    fn acquire(directory: &Path) -> Result<Self, SecretError> {
        let path = directory.join(".lock");
        let mut options = OpenOptions::new();
        options.write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(path)?;
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } != 0 {
                return Err(std::io::Error::last_os_error().into());
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
                return Err(std::io::Error::last_os_error().into());
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
    use crate::secrets::KeyRegistry;

    #[derive(Clone, Default)]
    struct Memory {
        items: Arc<Mutex<HashMap<String, String>>>,
        reads: Arc<Mutex<Vec<String>>>,
    }

    impl SecretStore for Memory {
        fn set(&self, account: &str, secret: &str) -> Result<(), SecretError> {
            self.items
                .lock()
                .unwrap()
                .insert(account.into(), secret.into());
            Ok(())
        }

        fn get(&self, account: &str) -> Result<Option<String>, SecretError> {
            self.reads.lock().unwrap().push(account.into());
            Ok(self.items.lock().unwrap().get(account).cloned())
        }

        fn delete(&self, account: &str) -> Result<bool, SecretError> {
            Ok(self.items.lock().unwrap().remove(account).is_some())
        }
    }

    fn store(home: &tempfile::TempDir, native: &Memory) -> MachineSecrets<Memory> {
        MachineSecrets::new(home.path().into(), native.clone())
    }

    #[test]
    fn hs2_5a6jns_migrates_accounts_and_vault_to_one_master_read_on_next_start() {
        let home = tempfile::tempdir().unwrap();
        let native = Memory::default();
        for account in [
            "github-app-one",
            "github-app-two",
            "permission-rules-v4-master-x",
        ] {
            native
                .set(account, &format!("synthetic-{account}"))
                .unwrap();
        }
        let first = store(&home, &native);
        for account in [
            "github-app-one",
            "github-app-two",
            "permission-rules-v4-master-x",
        ] {
            assert_eq!(
                first.get(account).unwrap().as_deref(),
                Some(format!("synthetic-{account}").as_str())
            );
        }
        assert_eq!(
            native
                .reads
                .lock()
                .unwrap()
                .iter()
                .filter(|account| *account == ROOT_ACCOUNT)
                .count(),
            1
        );
        native.reads.lock().unwrap().clear();
        let second = store(&home, &native);
        for account in [
            "github-app-one",
            "github-app-two",
            "permission-rules-v4-master-x",
        ] {
            assert_eq!(
                second.get(account).unwrap().as_deref(),
                Some(format!("synthetic-{account}").as_str())
            );
        }
        assert_eq!(
            native.reads.lock().unwrap().as_slice(),
            [ROOT_ACCOUNT],
            "subsequent startup reads one native item"
        );
        let files = fs::read_dir(home.path().join("machine-secrets"))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
            .count();
        assert_eq!(files, 3);
    }

    #[test]
    fn hs2_5a6jns_missing_master_or_corrupt_envelope_never_uses_legacy_fallback() {
        let home = tempfile::tempdir().unwrap();
        let native = Memory::default();
        native.set("github-app-one", "synthetic-old").unwrap();
        store(&home, &native).get("github-app-one").unwrap();
        let directory = home.path().join("machine-secrets");
        let path = MachineSecrets::<Memory>::envelope_path(&directory, "github-app-one");
        let intact = fs::read(&path).unwrap();
        native.set("github-app-one", "synthetic-new").unwrap();
        fs::write(&path, b"{invalid").unwrap();
        native.reads.lock().unwrap().clear();
        assert!(store(&home, &native).get("github-app-one").is_err());
        assert!(native.reads.lock().unwrap().is_empty());
        assert!(
            store(&home, &native)
                .set("github-app-one", "synthetic-new")
                .is_err()
        );
        assert!(store(&home, &native).delete("github-app-one").is_err());

        fs::write(&path, intact).unwrap();
        store(&home, &native)
            .set("github-app-one", "synthetic-new")
            .unwrap();
        native.delete(ROOT_ACCOUNT).unwrap();
        native.reads.lock().unwrap().clear();
        assert!(store(&home, &native).get("github-app-one").is_err());
        assert_eq!(native.reads.lock().unwrap().as_slice(), [ROOT_ACCOUNT]);
        assert!(store(&home, &native).set("another", "value").is_err());
        assert!(store(&home, &native).delete("github-app-one").is_err());
        assert!(native.get("github-app-one").unwrap().is_some());
        assert!(
            !fs::read(&path)
                .unwrap()
                .windows(b"synthetic-new".len())
                .any(|window| window == b"synthetic-new")
        );
    }

    #[test]
    fn hs2_5a6jns_refresh_delete_and_tombstone_survive_restart() {
        let home = tempfile::tempdir().unwrap();
        let native = Memory::default();
        native.set("github-app-one", "synthetic-old").unwrap();
        let current = store(&home, &native);
        assert_eq!(
            current.get("github-app-one").unwrap().as_deref(),
            Some("synthetic-old")
        );
        current
            .set("github-app-one", "synthetic-refreshed")
            .unwrap();
        assert_eq!(
            store(&home, &native)
                .get("github-app-one")
                .unwrap()
                .as_deref(),
            Some("synthetic-refreshed")
        );
        assert!(current.delete("github-app-one").unwrap());
        assert_eq!(store(&home, &native).get("github-app-one").unwrap(), None);
        // Even if an interrupted external cleanup leaves a stale native item,
        // the durable tombstone does not resurrect it.
        native.set("github-app-one", "synthetic-stale").unwrap();
        assert_eq!(store(&home, &native).get("github-app-one").unwrap(), None);
        current.set("github-app-one", "synthetic-new").unwrap();
        assert_eq!(
            store(&home, &native)
                .get("github-app-one")
                .unwrap()
                .as_deref(),
            Some("synthetic-new")
        );
    }

    #[test]
    fn hs2_5a6jns_concurrent_migration_creates_one_root_and_distinct_envelopes() {
        let home = tempfile::tempdir().unwrap();
        let native = Memory::default();
        for account in ["one", "two", "three", "four"] {
            native
                .set(account, &format!("synthetic-{account}"))
                .unwrap();
        }
        let cache: RootCache = Arc::new(Mutex::new(HashMap::new()));
        std::thread::scope(|threads| {
            for account in ["one", "two", "three", "four"] {
                let native = native.clone();
                let home = &home;
                let cache = Arc::clone(&cache);
                threads.spawn(move || {
                    assert_eq!(
                        MachineSecrets::with_cache(home.path().into(), native, cache,)
                            .get(account)
                            .unwrap()
                            .as_deref(),
                        Some(format!("synthetic-{account}").as_str())
                    );
                });
            }
        });
        assert_eq!(
            native
                .reads
                .lock()
                .unwrap()
                .iter()
                .filter(|account| *account == ROOT_ACCOUNT)
                .count(),
            1
        );
    }

    #[test]
    fn hs2_5a6jns_rotation_resumes_after_native_root_changes() {
        let home = tempfile::tempdir().unwrap();
        let native = Memory::default();
        let initial = store(&home, &native);
        initial.set("github-app-one", "synthetic-one").unwrap();
        initial
            .set("permission-rules-v4-master-x", "synthetic-vault")
            .unwrap();
        let old_backup = initial.export_master().unwrap();
        initial.rotate_master().unwrap();
        assert_ne!(initial.export_master().unwrap(), old_backup);
        assert_eq!(
            store(&home, &native)
                .get("github-app-one")
                .unwrap()
                .as_deref(),
            Some("synthetic-one")
        );

        let directory = home.path().join("machine-secrets");
        let current = decode_root(&initial.export_master().unwrap()).unwrap();
        let mut next = [0; 32];
        getrandom::fill(&mut next).unwrap();
        MachineSecrets::<Memory>::write_envelope(
            &directory.join("rotation.json"),
            &MachineSecrets::<Memory>::encrypt(
                &next,
                ROTATION_ACCOUNT,
                &base64::engine::general_purpose::STANDARD.encode(current),
            )
            .unwrap(),
        )
        .unwrap();
        native
            .set(
                ROOT_ACCOUNT,
                &base64::engine::general_purpose::STANDARD.encode(next),
            )
            .unwrap();
        let restarted = store(&home, &native);
        assert_eq!(
            restarted.get("github-app-one").unwrap().as_deref(),
            Some("synthetic-one")
        );
        assert!(restarted.resume_rotation().unwrap());
        assert!(!directory.join("rotation.json").exists());
        assert_eq!(
            store(&home, &native)
                .get("permission-rules-v4-master-x")
                .unwrap()
                .as_deref(),
            Some("synthetic-vault")
        );
    }

    #[test]
    fn hs2_5a6jns_restore_requires_a_backup_that_authenticates_data() {
        let home = tempfile::tempdir().unwrap();
        let native = Memory::default();
        let initial = store(&home, &native);
        initial.set("github-app-one", "synthetic-one").unwrap();
        let backup = initial.export_master().unwrap();
        native.delete(ROOT_ACCOUNT).unwrap();
        let restarted = store(&home, &native);
        assert!(restarted.get("github-app-one").is_err());
        assert!(
            restarted
                .restore_master(&base64::engine::general_purpose::STANDARD.encode([7u8; 32]))
                .is_err()
        );
        restarted.restore_master(&backup).unwrap();
        assert_eq!(
            restarted.get("github-app-one").unwrap().as_deref(),
            Some("synthetic-one")
        );
    }

    #[test]
    fn hs2_5a6jns_rotation_recovers_when_wrap_was_written_before_native_update() {
        let home = tempfile::tempdir().unwrap();
        let native = Memory::default();
        let initial = store(&home, &native);
        initial.set("one", "synthetic-one").unwrap();
        let original = initial.export_master().unwrap();
        let directory = home.path().join("machine-secrets");
        MachineSecrets::<Memory>::write_envelope(
            &directory.join("rotation.json"),
            &MachineSecrets::<Memory>::encrypt(&[9u8; 32], ROTATION_ACCOUNT, &original).unwrap(),
        )
        .unwrap();
        let restarted = store(&home, &native);
        assert_eq!(
            restarted.get("one").unwrap().as_deref(),
            Some("synthetic-one")
        );
        assert!(restarted.resume_rotation().unwrap());
        assert!(!directory.join("rotation.json").exists());
        assert_eq!(restarted.export_master().unwrap(), original);
    }

    #[test]
    fn hs2_5a6jns_denied_master_prompts_only_once_per_process() {
        #[derive(Clone)]
        struct Denied {
            native: Memory,
            attempts: Arc<Mutex<usize>>,
        }
        impl SecretStore for Denied {
            fn set(&self, account: &str, secret: &str) -> Result<(), SecretError> {
                self.native.set(account, secret)
            }
            fn get(&self, account: &str) -> Result<Option<String>, SecretError> {
                if account == ROOT_ACCOUNT {
                    *self.attempts.lock().unwrap() += 1;
                    return Err(SecretError::Backend("synthetic denial".into()));
                }
                self.native.get(account)
            }
            fn delete(&self, account: &str) -> Result<bool, SecretError> {
                self.native.delete(account)
            }
        }
        let home = tempfile::tempdir().unwrap();
        let native = Memory::default();
        store(&home, &native).set("one", "synthetic-one").unwrap();
        let attempts = Arc::new(Mutex::new(0));
        let denied = MachineSecrets::new(
            home.path().into(),
            Denied {
                native,
                attempts: Arc::clone(&attempts),
            },
        );
        for _ in 0..3 {
            assert!(denied.get("one").is_err());
        }
        assert_eq!(*attempts.lock().unwrap(), 1);
    }

    #[test]
    fn hs2_5a6jns_registry_flow_keeps_metadata_and_secret_values_separate() {
        let home = tempfile::tempdir().unwrap();
        let native = Memory::default();
        let keys = KeyRegistry::new(home.path(), store(&home, &native));
        keys.set("github-app-one", "synthetic-token-one").unwrap();
        keys.set("github-app-two", "synthetic-token-two").unwrap();
        assert_eq!(keys.list().unwrap().len(), 2);
        let metadata = fs::read_to_string(home.path().join("keys.json")).unwrap();
        assert!(!metadata.contains("synthetic-token"));
        let reopened = KeyRegistry::new(home.path(), store(&home, &native));
        assert_eq!(
            reopened.get("github-app-one").unwrap(),
            "synthetic-token-one"
        );
        reopened
            .set("github-app-one", "synthetic-refreshed")
            .unwrap();
        assert_eq!(keys.get("github-app-one").unwrap(), "synthetic-refreshed");
        assert!(reopened.delete("github-app-two").unwrap());
        assert!(matches!(
            keys.get("github-app-two"),
            Err(SecretError::NotFound(_))
        ));
        assert_eq!(keys.list().unwrap().len(), 1);
    }

    #[test]
    fn hs2_5a6jns_cleanup_requires_matching_backup_and_authenticated_envelopes() {
        let home = tempfile::tempdir().unwrap();
        let native = Memory::default();
        for account in ["github-app-one", "permission-rules-v4-master-x"] {
            native
                .set(account, &format!("synthetic-{account}"))
                .unwrap();
        }
        let current = store(&home, &native);
        current.get("github-app-one").unwrap();
        current.get("permission-rules-v4-master-x").unwrap();
        let backup = current.export_master().unwrap();
        assert!(
            current
                .cleanup_legacy(&base64::engine::general_purpose::STANDARD.encode([3u8; 32]))
                .is_err()
        );
        assert!(native.items.lock().unwrap().contains_key("github-app-one"));

        let path = MachineSecrets::<Memory>::envelope_path(
            &home.path().join("machine-secrets"),
            "github-app-one",
        );
        let intact = fs::read(&path).unwrap();
        fs::write(&path, b"{broken").unwrap();
        assert!(current.cleanup_legacy(&backup).is_err());
        assert!(
            native
                .items
                .lock()
                .unwrap()
                .contains_key("permission-rules-v4-master-x")
        );
        fs::write(&path, intact).unwrap();
        assert_eq!(current.cleanup_legacy(&backup).unwrap(), 2);
        assert_eq!(native.items.lock().unwrap().len(), 1);
        assert!(native.items.lock().unwrap().contains_key(ROOT_ACCOUNT));
        assert_eq!(
            store(&home, &native)
                .get("github-app-one")
                .unwrap()
                .as_deref(),
            Some("synthetic-github-app-one")
        );
    }

    #[test]
    fn hs2_5a6jns_running_process_refreshes_root_after_external_rotation() {
        let home = tempfile::tempdir().unwrap();
        let native = Memory::default();
        let server = store(&home, &native);
        let cli = store(&home, &native);
        server.set("github-app-one", "synthetic-one").unwrap();
        assert_eq!(
            server.get("github-app-one").unwrap().as_deref(),
            Some("synthetic-one")
        );
        cli.rotate_master().unwrap();
        server.set("github-app-two", "synthetic-two").unwrap();
        assert_eq!(
            store(&home, &native)
                .get("github-app-two")
                .unwrap()
                .as_deref(),
            Some("synthetic-two")
        );
        assert_eq!(
            server.get("github-app-one").unwrap().as_deref(),
            Some("synthetic-one")
        );
    }

    #[test]
    fn hs2_5a6jns_interrupted_rotation_recovers_a_stale_process_write() {
        let home = tempfile::tempdir().unwrap();
        let native = Memory::default();
        let server = store(&home, &native);
        server.set("one", "synthetic-one").unwrap();
        let old = decode_root(&server.export_master().unwrap()).unwrap();
        let directory = home.path().join("machine-secrets");
        let mut next = [0; 32];
        getrandom::fill(&mut next).unwrap();
        MachineSecrets::<Memory>::write_envelope(
            &directory.join("rotation.json"),
            &MachineSecrets::<Memory>::encrypt(
                &next,
                ROTATION_ACCOUNT,
                &base64::engine::general_purpose::STANDARD.encode(old),
            )
            .unwrap(),
        )
        .unwrap();
        native
            .set(
                ROOT_ACCOUNT,
                &base64::engine::general_purpose::STANDARD.encode(next),
            )
            .unwrap();
        // The old process has not seen a revision change yet, but the recovery
        // wrap still protects the value it writes with the cached old root.
        server.set("two", "synthetic-two").unwrap();
        let restarted = store(&home, &native);
        assert!(restarted.resume_rotation().unwrap());
        assert_eq!(
            restarted.get("two").unwrap().as_deref(),
            Some("synthetic-two")
        );
        server.set("three", "synthetic-three").unwrap();
        assert_eq!(
            restarted.get("three").unwrap().as_deref(),
            Some("synthetic-three")
        );
    }
}
