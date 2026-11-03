//! Bounded regular-file persistence. Reads never create an absent declaration.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use super::OrchestrationError;
use crate::FileLock;

pub(crate) enum PublishStage {
    Staged,
    Published,
}

#[derive(Clone)]
pub(crate) struct Storage {
    pub(crate) path: PathBuf,
    anchor: PathBuf,
    private: bool,
}

impl Storage {
    pub(crate) fn anchor(&self) -> &Path {
        &self.anchor
    }
    pub(crate) fn shared(checkout: &Path) -> Self {
        Self {
            path: checkout.join(".hotsheet2/orchestration.json"),
            anchor: checkout.into(),
            private: false,
        }
    }

    pub(crate) fn machine(home: &Path) -> Self {
        Self {
            path: home.join("orchestration-bindings.json"),
            anchor: home.into(),
            private: true,
        }
    }

    fn check_directories(&self) -> Result<(), OrchestrationError> {
        let parent = self
            .path
            .parent()
            .ok_or(OrchestrationError::Invalid("storage needs a parent"))?;
        let relative = parent
            .strip_prefix(&self.anchor)
            .map_err(|_| OrchestrationError::Invalid("storage escaped its owner"))?;
        let mut current = self.anchor.clone();
        for part in std::iter::once(None).chain(relative.components().map(Some)) {
            if let Some(part) = part {
                current.push(part.as_os_str());
            }
            match fs::symlink_metadata(&current) {
                Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => {
                    return Err(OrchestrationError::Invalid(
                        "storage directories must be real directories",
                    ));
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }

    fn ensure_parent(&self) -> Result<&Path, OrchestrationError> {
        self.check_directories()?;
        if !self.private && !self.anchor.is_dir() {
            return Err(OrchestrationError::Invalid(
                "registered checkout directory is unavailable",
            ));
        }
        let parent = self
            .path
            .parent()
            .ok_or(OrchestrationError::Invalid("storage needs a parent"))?;
        fs::create_dir_all(parent)?;
        self.check_directories()?;
        Ok(parent)
    }

    fn open(&self, path: &Path, create: bool) -> Result<File, std::io::Error> {
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
                .mode(if self.private { 0o600 } else { 0o644 })
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

    pub(crate) fn lock(&self) -> Result<FileLock, OrchestrationError> {
        self.ensure_parent()?;
        let path = self.path.with_extension("json.lock");
        reject_nonregular(&path)?;
        Ok(FileLock::acquire(self.open(&path, true)?)?)
    }

    pub(crate) fn read(&self, limit: usize) -> Result<Option<Vec<u8>>, OrchestrationError> {
        self.check_directories()?;
        reject_nonregular(&self.path)?;
        let file = match self.open(&self.path, false) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let mut bytes = Vec::new();
        file.take((limit + 1) as u64).read_to_end(&mut bytes)?;
        if bytes.len() > limit {
            return Err(OrchestrationError::Corrupt);
        }
        Ok(Some(bytes))
    }

    pub(crate) fn write(&self, bytes: &[u8], limit: usize) -> Result<(), OrchestrationError> {
        self.write_with_hook(bytes, limit, |_| Ok(()))
    }

    pub(crate) fn write_with_hook(
        &self,
        bytes: &[u8],
        limit: usize,
        mut hook: impl FnMut(PublishStage) -> std::io::Result<()>,
    ) -> Result<(), OrchestrationError> {
        if bytes.len() > limit {
            return Err(OrchestrationError::Capacity);
        }
        let parent = self.ensure_parent()?;
        reject_nonregular(&self.path)?;
        let mut file = tempfile::Builder::new()
            .prefix(".orchestration-")
            .suffix(".tmp")
            .tempfile_in(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = if self.private {
                0o600
            } else {
                fs::metadata(&self.path)
                    .map(|m| m.permissions().mode() & 0o777)
                    .unwrap_or(0o644)
            };
            file.as_file()
                .set_permissions(fs::Permissions::from_mode(mode))?;
        }
        file.write_all(bytes)?;
        file.as_file().sync_all()?;
        hook(PublishStage::Staged)?;
        self.check_directories()?;
        reject_nonregular(&self.path)?;
        file.persist(&self.path)
            .map_err(|error| OrchestrationError::Storage(error.error))?;
        hook(PublishStage::Published)?;
        #[cfg(unix)]
        File::open(parent)?.sync_all()?;
        Ok(())
    }
}

fn reject_nonregular(path: &Path) -> Result<(), OrchestrationError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() || metadata.file_type().is_symlink() => Err(
            OrchestrationError::Invalid("storage paths must be regular files"),
        ),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn decode<T: serde::de::DeserializeOwned>(
    bytes: &[u8],
) -> Result<T, OrchestrationError> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| OrchestrationError::Corrupt)?;
    let found = value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .ok_or(OrchestrationError::Corrupt)?;
    if found != 1 {
        return Err(OrchestrationError::UnsupportedVersion { found });
    }
    serde_json::from_value(value).map_err(|_| OrchestrationError::Corrupt)
}
