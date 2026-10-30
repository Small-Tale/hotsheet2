use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::contract::{MAX_REGISTRY_BYTES, bounded_text, valid_id};
use super::*;
use crate::FileLock;

// HS2-1Y45MZ: active credentials and write-ahead/cleanup refs share one budget,
// so revocation can always transfer an active obligation without needing a slot.
pub(crate) const MAX_CREDENTIAL_OBLIGATIONS: usize = 256;

#[derive(Clone)]
pub struct RemoteRegistry {
    pub(crate) path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RegistryFile {
    pub version: u32,
    pub servers: Vec<Binding>,
    pub pending_cleanup: Vec<String>,
}

impl Default for RegistryFile {
    fn default() -> Self {
        Self {
            version: 1,
            servers: Vec::new(),
            pending_cleanup: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Binding {
    pub id: String,
    pub label: String,
    pub origin: String,
    pub identity: ReviewedRemoteIdentity,
    pub revision: u64,
    pub revoked: bool,
    pub credential_ref: Option<String>,
    pub observation_generation: String,
    pub last_failure: Option<RemoteFailure>,
    pub checked_at_ms: Option<u64>,
    pub confirmed_at_ms: Option<u64>,
    pub checkouts: Vec<RemoteCheckout>,
    pub history: Vec<RemoteReview>,
}

impl Binding {
    pub(crate) fn view(&self, file: &RegistryFile) -> RemoteView {
        RemoteView {
            id: self.id.clone(),
            label: self.label.clone(),
            origin: self.origin.clone(),
            identity: self.identity.clone(),
            revision: self.revision,
            revoked: self.revoked,
            last_failure: self.last_failure,
            checked_at_ms: self.checked_at_ms,
            confirmed_at_ms: self.confirmed_at_ms,
            last_confirmed_checkouts: self.checkouts.clone(),
            stale: true,
            credential_cleanup_pending: file
                .pending_cleanup
                .iter()
                .any(|item| item.starts_with(&format!("remote-{}-", self.id))),
            review_history: self.history.clone(),
        }
    }
}

impl RemoteRegistry {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn list(&self) -> Result<Vec<RemoteView>, RemoteError> {
        let file = self.read()?;
        Ok(file
            .servers
            .iter()
            .map(|server| server.view(&file))
            .collect())
    }

    pub(crate) fn lock(&self) -> Result<FileLock, RemoteError> {
        ensure_parent(&self.path)?;
        let lock = self.path.with_extension("json.lock");
        reject_symlink(&lock)?;
        Ok(FileLock::acquire(private_open(&lock, true)?)?)
    }

    pub(crate) fn read(&self) -> Result<RegistryFile, RemoteError> {
        let Some(contents) = read_bounded(&self.path)? else {
            return Ok(RegistryFile::default());
        };
        let version: serde_json::Value =
            serde_json::from_slice(&contents).map_err(|_| RemoteError::Corrupt)?;
        if version.get("version").and_then(serde_json::Value::as_u64) != Some(1) {
            return Err(RemoteError::UnsupportedVersion);
        }
        let file: RegistryFile =
            serde_json::from_value(version).map_err(|_| RemoteError::Corrupt)?;
        validate_file(&file)?;
        Ok(file)
    }

    pub(crate) fn write(&self, file: &RegistryFile) -> Result<(), RemoteError> {
        validate_file(file)?;
        let bytes = serde_json::to_vec_pretty(file).map_err(|_| RemoteError::Corrupt)?;
        atomic_write(&self.path, &bytes)
    }
}

pub(crate) fn credential_obligations(file: &RegistryFile) -> usize {
    file.pending_cleanup.len()
        + file
            .servers
            .iter()
            .filter(|binding| binding.credential_ref.is_some())
            .count()
}

fn validate_file(file: &RegistryFile) -> Result<(), RemoteError> {
    if file.version != 1 {
        return Err(RemoteError::UnsupportedVersion);
    }
    let mut ids = HashSet::new();
    let mut references = HashSet::new();
    let mut origins = HashSet::new();
    if file.servers.len() > 64
        || file.pending_cleanup.len() > MAX_CREDENTIAL_OBLIGATIONS
        || credential_obligations(file) > MAX_CREDENTIAL_OBLIGATIONS
    {
        return Err(RemoteError::Corrupt);
    }
    for server in &file.servers {
        valid_id(&server.id).map_err(|_| RemoteError::Corrupt)?;
        valid_id(&server.identity.server_id).map_err(|_| RemoteError::Corrupt)?;
        valid_id(&server.observation_generation).map_err(|_| RemoteError::Corrupt)?;
        if !ids.insert(&server.id)
            || !bounded_text(&server.label, 256)
            || normalize_remote_origin(&server.origin).ok().as_deref()
                != Some(server.origin.as_str())
            || server.revision == 0
            || server.history.is_empty()
            || server.history.len() > 64
            || server.revoked != server.credential_ref.is_none()
            || (server.origin.starts_with("https://") != server.identity.ca_fingerprint.is_some())
            || !valid_identity(&server.identity)
            || (!server.revoked && !origins.insert(&server.origin))
        {
            return Err(RemoteError::Corrupt);
        }
        if let Some(reference) = &server.credential_ref {
            if reference != &format!("remote-{}-{}", server.id, server.revision)
                || !references.insert(reference)
            {
                return Err(RemoteError::Corrupt);
            }
        }
        let mut last_revision = 0;
        for review in &server.history {
            if review.revision <= last_revision
                || review.revision > server.revision
                || !valid_identity(&review.identity)
                || normalize_remote_origin(&review.origin).ok().as_deref()
                    != Some(review.origin.as_str())
                || (review.origin.starts_with("https://")
                    != review.identity.ca_fingerprint.is_some())
            {
                return Err(RemoteError::Corrupt);
            }
            last_revision = review.revision;
        }
        let last = server.history.last().ok_or(RemoteError::Corrupt)?;
        if last.identity != server.identity || last.origin != server.origin {
            return Err(RemoteError::Corrupt);
        }
        validate_remote_checkouts(&server.checkouts).map_err(|_| RemoteError::Corrupt)?;
    }
    let mut pending = HashSet::new();
    for reference in &file.pending_cleanup {
        if !valid_credential_ref(reference)
            || references.contains(reference)
            || !pending.insert(reference)
        {
            return Err(RemoteError::Corrupt);
        }
    }
    Ok(())
}

fn valid_credential_ref(value: &str) -> bool {
    let Some((id, revision)) = value
        .strip_prefix("remote-")
        .and_then(|value| value.rsplit_once('-'))
    else {
        return false;
    };
    valid_id(id).is_ok() && revision.parse::<u64>().is_ok_and(|value| value > 0)
}

fn valid_identity(identity: &ReviewedRemoteIdentity) -> bool {
    valid_id(&identity.server_id).is_ok()
        && identity.ca_fingerprint.as_deref().is_none_or(|value| {
            value.len() == 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
}

pub(crate) fn reject_symlink(path: &Path) -> Result<(), RemoteError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(RemoteError::Invalid("registry paths must be regular files"))
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn private_open(path: &Path, write: bool) -> Result<File, RemoteError> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(write)
        .create(write)
        .truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    Ok(options.open(path)?)
}

fn ensure_parent(path: &Path) -> Result<&Path, RemoteError> {
    let parent = path
        .parent()
        .ok_or(RemoteError::Invalid("registry needs a parent directory"))?;
    fs::create_dir_all(parent)?;
    if fs::symlink_metadata(parent)?.file_type().is_symlink() {
        return Err(RemoteError::Invalid(
            "registry directory must not be a symlink",
        ));
    }
    Ok(parent)
}

fn read_bounded(path: &Path) -> Result<Option<Vec<u8>>, RemoteError> {
    reject_symlink(path)?;
    let file = match private_open(path, false) {
        Ok(file) => file,
        Err(RemoteError::Storage(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    file.take((MAX_REGISTRY_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_REGISTRY_BYTES {
        return Err(RemoteError::Corrupt);
    }
    Ok(Some(bytes))
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), RemoteError> {
    if bytes.len() > MAX_REGISTRY_BYTES {
        return Err(RemoteError::Invalid("registry size limit exceeded"));
    }
    let parent = ensure_parent(path)?;
    reject_symlink(path)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(path)
        .map_err(|error| RemoteError::Storage(error.error))?;
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(())
}

/// A durable machine identity; creation is serialized and never inferred from an instance/store.
pub fn server_identity(
    home: &Path,
    mint: impl FnOnce() -> String,
) -> Result<RemoteIdentity, RemoteError> {
    let path = home.join("server-identity.json");
    let registry = RemoteRegistry::new(&path);
    let _lock = registry.lock()?;
    if let Some(bytes) = read_bounded(&path)? {
        let value: RemoteIdentity =
            serde_json::from_slice(&bytes).map_err(|_| RemoteError::Corrupt)?;
        value.validate().map_err(RemoteError::Verification)?;
        return Ok(value);
    }
    let value = RemoteIdentity {
        generation: "hs2".into(),
        protocol: REMOTE_PROTOCOL_VERSION,
        server_id: mint(),
    };
    value.validate().map_err(RemoteError::Verification)?;
    atomic_write(
        &path,
        &serde_json::to_vec(&value).map_err(|_| RemoteError::Corrupt)?,
    )?;
    Ok(value)
}
