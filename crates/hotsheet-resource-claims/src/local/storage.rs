//! Bounded, atomic, owner-only persistence for one namespace's claim state.

use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use hotsheet_ticketing::FileLock;
use serde::{Deserialize, Serialize};

use crate::error::ClaimError;

pub(crate) const SCHEMA_VERSION: u64 = 1;
/// Largest state file read or written.
pub(crate) const MAX_STATE_BYTES: usize = 4 * 1024 * 1024;
/// Most resources (live or released) one namespace remembers. A released resource keeps
/// its record, because the record holds the fencing counter.
pub(crate) const MAX_RESOURCES: usize = 4096;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct State {
    pub(crate) schema_version: u64,
    #[serde(default)]
    pub(crate) resources: BTreeMap<String, Record>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct Record {
    /// The fencing token of the latest grant; it only ever grows.
    pub(crate) fencing: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) claim: Option<StoredClaim>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct StoredClaim {
    pub(crate) claim_id: String,
    /// Keyed digest of the renewal token; the token itself is never stored.
    pub(crate) token_digest: String,
    pub(crate) acquired_at_ms: u64,
    pub(crate) expires_at_ms: u64,
    #[serde(default)]
    pub(crate) metadata: BTreeMap<String, String>,
}

pub(crate) struct NamespaceStore {
    dir: PathBuf,
}

impl NamespaceStore {
    pub(crate) fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub(crate) fn waiters_dir(&self) -> PathBuf {
        self.dir.join("waiters")
    }

    fn state_path(&self) -> PathBuf {
        self.dir.join("state.json")
    }

    fn check_dir(&self) -> Result<bool, ClaimError> {
        match fs::symlink_metadata(&self.dir) {
            Ok(metadata) if metadata.is_dir() => Ok(true),
            Ok(_) => Err(ClaimError::invalid(
                "claim namespace storage must be a real directory",
            )),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error.into()),
        }
    }

    fn create_dir(&self) -> Result<(), ClaimError> {
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&self.dir)?;
        self.check_dir().map(|_| ())
    }

    fn open(path: &Path, create: bool) -> std::io::Result<File> {
        let mut options = OpenOptions::new();
        options
            .read(true)
            .write(create)
            .create(create)
            .truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        let file = options.open(path)?;
        if !file.metadata()?.is_file() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "regular file required",
            ));
        }
        Ok(file)
    }

    /// Take the namespace's exclusive OS lock. With `create` unset an absent namespace
    /// is `Ok(None)` and nothing is created.
    pub(crate) fn lock(&self, create: bool) -> Result<Option<FileLock>, ClaimError> {
        if !self.check_dir()? {
            if !create {
                return Ok(None);
            }
            self.create_dir()?;
        }
        let file = Self::open(&self.dir.join("state.lock"), true)?;
        Ok(Some(FileLock::acquire(file)?))
    }

    /// The last published state. Reads take no lock (publication is an atomic rename)
    /// and never create an absent namespace.
    pub(crate) fn read(&self) -> Result<State, ClaimError> {
        let empty = State {
            schema_version: SCHEMA_VERSION,
            resources: BTreeMap::new(),
        };
        if !self.check_dir()? {
            return Ok(empty);
        }
        let file = match Self::open(&self.state_path(), false) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(empty),
            Err(error) => return Err(error.into()),
        };
        let mut bytes = Vec::new();
        file.take(MAX_STATE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_STATE_BYTES {
            return Err(ClaimError::Corrupt);
        }
        let state: State = serde_json::from_slice(&bytes).map_err(|_| ClaimError::Corrupt)?;
        if state.schema_version != SCHEMA_VERSION {
            return Err(ClaimError::Corrupt);
        }
        Ok(state)
    }

    /// Publish `state` by staging a complete private file and renaming it into place.
    /// A crash before the rename leaves the previous state intact.
    pub(crate) fn write(&self, state: &State) -> Result<(), ClaimError> {
        let bytes = serde_json::to_vec_pretty(state).map_err(|_| ClaimError::Corrupt)?;
        if bytes.len() > MAX_STATE_BYTES {
            return Err(ClaimError::Capacity("claim namespace state is full".into()));
        }
        let mut staged = tempfile::Builder::new()
            .prefix(".state-")
            .suffix(".tmp")
            .tempfile_in(&self.dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            staged
                .as_file()
                .set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        staged.write_all(&bytes)?;
        staged.as_file().sync_all()?;
        hotsheet_sync::files::persist(staged, self.state_path()).map_err(ClaimError::Storage)?;
        #[cfg(unix)]
        File::open(&self.dir)?.sync_all()?;
        Ok(())
    }
}
