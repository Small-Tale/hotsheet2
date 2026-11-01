use super::{File, Path, PathBuf, SecretError, io, refusal};
#[cfg(test)]
use super::{Metadata, Owner, Step, decode};
use crate::file_lock::FileLock;
#[cfg(test)]
use sha2::{Digest, Sha256};
use std::fs::{self, Metadata as FileMetadata, OpenOptions};
use std::io::Read;
#[cfg(test)]
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};

#[derive(Clone, Copy, PartialEq, Eq)]
struct Identity {
    device: u64,
    inode: u64,
}
impl Identity {
    fn owned(metadata: &FileMetadata) -> Result<Self, SecretError> {
        // SAFETY: geteuid has no arguments or owned resources.
        if metadata.uid() != unsafe { libc::geteuid() } {
            return Err(refusal());
        }
        Ok(Self {
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }
    fn directory(path: &Path) -> Result<Self, SecretError> {
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() || metadata.mode() & 0o022 != 0 {
            return Err(refusal());
        }
        Self::owned(&metadata)
    }
    fn regular(metadata: &FileMetadata) -> Result<Self, SecretError> {
        if !metadata.is_file() || metadata.nlink() != 1 {
            return Err(refusal());
        }
        Self::owned(metadata)
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Image {
    identity: Identity,
    size: u64,
    modified: (i64, i64),
    changed: (i64, i64),
}
impl Image {
    fn from(metadata: &FileMetadata) -> Result<Self, SecretError> {
        Ok(Self {
            identity: Identity::regular(metadata)?,
            size: metadata.len(),
            modified: (metadata.mtime(), metadata.mtime_nsec()),
            changed: (metadata.ctime(), metadata.ctime_nsec()),
        })
    }
}
fn image_at(path: &Path) -> Result<Option<Image>, SecretError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(Image::from(&metadata)?)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}
pub(super) struct CheckedLock {
    lock: FileLock,
    path: PathBuf,
    identity: Identity,
}
impl CheckedLock {
    fn acquire(path: PathBuf) -> Result<Self, SecretError> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(&path)?;
        let metadata = file.metadata()?;
        let identity = Identity::regular(&metadata)?;
        if metadata.mode() & 0o077 != 0 {
            return Err(refusal());
        }
        let lock = FileLock::acquire(file)?;
        let result = Self {
            lock,
            path,
            identity,
        };
        result.check()?;
        Ok(result)
    }
    pub(super) fn check(&self) -> Result<(), SecretError> {
        let opened = self.lock.file().metadata()?;
        if opened.mode() & 0o077 != 0
            || Identity::regular(&opened)? != self.identity
            || image_at(&self.path)?.map(|image| image.identity) != Some(self.identity)
        {
            return Err(refusal());
        }
        Ok(())
    }
}
pub(super) struct Directories {
    root: Identity,
    accounts: Identity,
}

// Only newly created path components, not a filesystem walk. Sync them and their
// parent entries before any native SecretStore mutation.
fn create_tree(path: &Path, created: &mut Vec<PathBuf>) -> Result<(), SecretError> {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => return Ok(()),
        Ok(_) => return Err(refusal()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    create_tree(parent, created)?;
    match fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => created.push(path.to_path_buf()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            if !fs::metadata(path)?.is_dir() {
                return Err(refusal());
            }
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}
fn sync_created(created: &[PathBuf]) -> Result<(), SecretError> {
    for path in created.iter().rev() {
        let path = path.canonicalize()?;
        Identity::directory(&path)?;
        File::open(&path)?.sync_all()?;
        if let Some(parent) = path.parent() {
            File::open(parent)?.sync_all()?;
        }
    }
    Ok(())
}
impl Directories {
    pub(super) fn prepare(requested: &Path) -> Result<(PathBuf, PathBuf, Self), SecretError> {
        let mut created = Vec::new();
        create_tree(requested, &mut created)?;
        let root = requested.canonicalize()?;
        let root_identity = Identity::directory(&root)?;
        let accounts = root.join("key-metadata-locks");
        create_tree(&accounts, &mut created)?;
        let accounts_identity = Identity::directory(&accounts)?;
        sync_created(&created)?;
        Ok((
            root,
            accounts,
            Self {
                root: root_identity,
                accounts: accounts_identity,
            },
        ))
    }
    pub(super) fn check(&self, root: &Path, accounts: &Path) -> Result<(), SecretError> {
        if Identity::directory(root)? != self.root
            || Identity::directory(accounts)? != self.accounts
        {
            return Err(refusal());
        }
        Ok(())
    }
    pub(super) fn lock(&self, path: PathBuf) -> Result<CheckedLock, SecretError> {
        CheckedLock::acquire(path)
    }
    pub(super) fn image_at(&self, path: &Path) -> Result<Option<Image>, SecretError> {
        image_at(path)
    }
    pub(super) fn read(&self, path: &Path) -> Result<(Vec<u8>, Option<Image>), SecretError> {
        let mut file = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
        {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                if image_at(path)?.is_some() {
                    return Err(refusal());
                }
                return Ok((Vec::new(), None));
            }
            Err(error) => return Err(error.into()),
        };
        let before = Image::from(&file.metadata()?)?;
        let limit = before.size.checked_add(1).ok_or_else(refusal)?;
        let mut bytes = Vec::new();
        (&mut file).take(limit).read_to_end(&mut bytes)?;
        if u64::try_from(bytes.len()).ok() != Some(before.size)
            || Image::from(&file.metadata()?)? != before
            || image_at(path)? != Some(before)
        {
            return Err(refusal());
        }
        Ok((bytes, Some(before)))
    }
    pub(super) fn stage(&self, root: &Path) -> Result<tempfile::NamedTempFile, SecretError> {
        let mut builder = tempfile::Builder::new();
        builder
            .prefix(".keys-")
            .suffix(".tmp")
            .permissions(fs::Permissions::from_mode(0o600));
        Ok(builder.tempfile_in(root)?)
    }
    pub(super) fn rename(
        &self,
        stage: tempfile::NamedTempFile,
        target: &Path,
    ) -> Result<File, SecretError> {
        Ok(stage.persist(target).map_err(|error| error.error)?)
    }
    pub(super) fn finish(
        &self,
        published: &File,
        root: &Path,
        target: &Path,
    ) -> Result<(), SecretError> {
        let identity = Identity::regular(&published.metadata()?)?;
        if image_at(target)?.map(|image| image.identity) != Some(identity) {
            return Err(refusal());
        }
        File::open(root)?.sync_all()?;
        Ok(())
    }
}
#[cfg(test)]
mod tests;
