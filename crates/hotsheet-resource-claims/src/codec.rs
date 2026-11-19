//! Key providers for the claim backends. The keys and prefixed codecs themselves live in
//! `hotsheet_ticketing::claim_codec` (ticket claims share them) and are re-exported here.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use ring::rand::{SecureRandom, SystemRandom};

pub use hotsheet_ticketing::claim_codec::{
    ClaimKey, CodecError, ENC_PREFIX, HMAC_PREFIX, KEY_LEN, MetadataCodec, MetadataCodecs,
    MetadataValue, PLAIN_PREFIX, hex, unhex,
};

use crate::error::ClaimError;

impl From<CodecError> for ClaimError {
    fn from(error: CodecError) -> Self {
        match error {
            CodecError::Invalid(message) => Self::Invalid(message),
            CodecError::Unsupported(message) => Self::Unsupported(message),
        }
    }
}

/// Where a backend configuration's key comes from. The local machine backend reads a
/// private file; remote configurations use the OS credential store (HS2-JJWESD).
pub trait KeyProvider: Send + Sync {
    fn key(&self) -> Result<ClaimKey, ClaimError>;
}

/// A fixed key, for tests and for callers that already hold one.
#[derive(Debug, Clone)]
pub struct StaticKeyProvider(ClaimKey);

impl StaticKeyProvider {
    pub fn new(key: ClaimKey) -> Self {
        Self(key)
    }
}

impl KeyProvider for StaticKeyProvider {
    fn key(&self) -> Result<ClaimKey, ClaimError> {
        Ok(self.0.clone())
    }
}

/// A machine-local key file, created on first use with owner-only permissions.
#[derive(Debug, Clone)]
pub struct FileKeyProvider {
    path: PathBuf,
}

impl FileKeyProvider {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn read_existing(&self) -> Result<Option<ClaimKey>, ClaimError> {
        let metadata = match fs::symlink_metadata(&self.path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        if !metadata.is_file() {
            return Err(ClaimError::invalid("claim key must be a regular file"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o077 != 0 {
                return Err(ClaimError::invalid(
                    "claim key file must be readable by its owner only (mode 0600)",
                ));
            }
        }
        let mut bytes = Vec::new();
        fs::File::open(&self.path)?
            .take(KEY_LEN as u64 + 1)
            .read_to_end(&mut bytes)?;
        let bytes: [u8; KEY_LEN] = bytes.try_into().map_err(|_| ClaimError::Corrupt)?;
        Ok(Some(ClaimKey::from_bytes(bytes)))
    }
}

impl KeyProvider for FileKeyProvider {
    fn key(&self) -> Result<ClaimKey, ClaimError> {
        if let Some(key) = self.read_existing()? {
            return Ok(key);
        }
        let parent = self
            .path
            .parent()
            .ok_or_else(|| ClaimError::invalid("claim key path needs a parent"))?;
        fs::create_dir_all(parent)?;
        let mut bytes = [0u8; KEY_LEN];
        SystemRandom::new()
            .fill(&mut bytes)
            .map_err(|_| ClaimError::Unsupported("no secure random source".into()))?;
        // Stage a complete private file, then publish it without replacing one another
        // process published first: both processes end up reading the same key.
        let mut staged = tempfile::Builder::new()
            .prefix(".key-")
            .suffix(".tmp")
            .tempfile_in(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            staged
                .as_file()
                .set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        staged.write_all(&bytes)?;
        staged.as_file().sync_all()?;
        match staged.persist_noclobber(&self.path) {
            Ok(_) => Ok(ClaimKey::from_bytes(bytes)),
            Err(error) if error.error.kind() == std::io::ErrorKind::AlreadyExists => {
                self.read_existing()?.ok_or(ClaimError::Corrupt)
            }
            Err(error) => Err(error.error.into()),
        }
    }
}
