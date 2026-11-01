//! Windows live-handle ownership. No timestamp identity, ACL repair or no-op locks.

use super::{File, Path, PathBuf, SecretError, io, refusal};
use crate::file_lock::FileLock;
use std::{
    io::Read,
    mem,
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle},
    },
    ptr,
};
use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, CREATE_NEW, CreateDirectoryW, CreateFileW,
    FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_ATTRIBUTE_TAG_INFO, FILE_BASIC_INFO, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_GENERIC_READ, FILE_GENERIC_WRITE, FILE_ID_INFO,
    FILE_INFO_BY_HANDLE_CLASS, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, FILE_STANDARD_INFO, FILE_TYPE_DISK, FileAttributeTagInfo, FileBasicInfo,
    FileIdInfo, FileStandardInfo, GetFileInformationByHandle, GetFileInformationByHandleEx,
    GetFileType, GetVolumeInformationByHandleW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    MoveFileExW, OPEN_ALWAYS, OPEN_EXISTING, READ_CONTROL,
};
use windows_sys::Win32::System::SystemServices::FILE_PERSISTENT_ACLS;

mod security;
use security::Principal;

fn wide(path: &Path) -> Result<Vec<u16>, SecretError> {
    let mut result: Vec<_> = path.as_os_str().encode_wide().collect();
    if result.contains(&0) {
        return Err(refusal());
    }
    result.push(0);
    Ok(result)
}
fn supported_path(path: &Path) -> Result<PathBuf, SecretError> {
    use std::path::{Component, Prefix};
    let path = std::path::absolute(path)?;
    match path.components().next() {
        Some(Component::Prefix(prefix))
            if matches!(
                prefix.kind(),
                Prefix::Disk(_)
                    | Prefix::VerbatimDisk(_)
                    | Prefix::UNC(_, _)
                    | Prefix::VerbatimUNC(_, _)
            ) =>
        {
            Ok(path)
        }
        _ => Err(refusal()),
    }
}
fn info<T: Default>(file: &File, class: FILE_INFO_BY_HANDLE_CLASS) -> Result<T, SecretError> {
    let mut result = T::default();
    // SAFETY: private callers pair each information class with its exact SDK type.
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            class,
            (&mut result as *mut T).cast(),
            mem::size_of::<T>() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error().into());
    }
    Ok(result)
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Identity {
    volume: u64,
    file: [u8; 16],
}
fn identity(file: &File, directory: bool) -> Result<Identity, SecretError> {
    // SAFETY: file owns a live handle; GetFileType only borrows it.
    if unsafe { GetFileType(file.as_raw_handle()) } != FILE_TYPE_DISK {
        return Err(refusal());
    }
    let tag: FILE_ATTRIBUTE_TAG_INFO = info(file, FileAttributeTagInfo)?;
    let standard: FILE_STANDARD_INFO = info(file, FileStandardInfo)?;
    if tag.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || standard.DeletePending
        || (tag.FileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0) != directory
        || standard.Directory != directory
    {
        return Err(refusal());
    }
    if !directory {
        let mut legacy = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: output is the correctly sized initialized SDK structure.
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut legacy) } == 0 {
            return Err(io::Error::last_os_error().into());
        }
        if legacy.nNumberOfLinks != 1 || standard.NumberOfLinks != 1 {
            return Err(refusal());
        }
    }
    let actual: FILE_ID_INFO = info(file, FileIdInfo)?;
    if actual.FileId.Identifier.iter().all(|byte| *byte == 0) {
        return Err(refusal());
    }
    Ok(Identity {
        volume: actual.VolumeSerialNumber,
        file: actual.FileId.Identifier,
    })
}
fn acl_capability(file: &File) -> Result<(), SecretError> {
    let mut flags = 0;
    // SAFETY: optional name outputs are null/zero; flags is a valid scalar output.
    if unsafe {
        GetVolumeInformationByHandleW(
            file.as_raw_handle(),
            ptr::null_mut(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut flags,
            ptr::null_mut(),
            0,
        )
    } == 0
    {
        return Err(io::Error::last_os_error().into());
    }
    if flags & FILE_PERSISTENT_ACLS == 0 {
        return Err(refusal());
    }
    Ok(())
}
fn open(
    path: &Path,
    principal: &Principal,
    access: u32,
    disposition: u32,
    directory: bool,
    delete_share: bool,
) -> Result<File, SecretError> {
    let name = wide(path)?;
    let security = principal.creation(directory)?;
    let attributes = security.attributes();
    let share =
        FILE_SHARE_READ | FILE_SHARE_WRITE | if delete_share { FILE_SHARE_DELETE } else { 0 };
    let flags = FILE_FLAG_OPEN_REPARSE_POINT
        | if directory {
            FILE_FLAG_BACKUP_SEMANTICS
        } else {
            FILE_ATTRIBUTE_NORMAL
        };
    let descriptor = if disposition == OPEN_EXISTING {
        ptr::null()
    } else {
        &attributes as *const _
    };
    // SAFETY: path is NUL-terminated; security buffers outlive this synchronous call.
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            access,
            share,
            descriptor,
            disposition,
            flags,
            ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error().into());
    }
    // SAFETY: successful CreateFileW transfers one unique owned handle to File.
    Ok(unsafe { File::from_raw_handle(handle) })
}
fn directory(
    path: &Path,
    principal: &Principal,
    strict: bool,
    parent: bool,
) -> Result<File, SecretError> {
    let file = open(
        path,
        principal,
        READ_CONTROL | FILE_READ_ATTRIBUTES,
        OPEN_EXISTING,
        true,
        false,
    )?;
    identity(&file, true)?;
    acl_capability(&file)?;
    principal.check(&file, strict, parent)?;
    Ok(file)
}
fn create_tree(path: &Path, principal: &Principal) -> Result<File, SecretError> {
    match directory(path, principal, false, true) {
        Ok(file) => return Ok(file),
        Err(SecretError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let parent = path
        .parent()
        .filter(|parent| parent != &path)
        .ok_or_else(refusal)?;
    let parent_handle = create_tree(parent, principal)?;
    let before = identity(&parent_handle, true)?;
    let name = wide(path)?;
    let security = principal.creation(true)?;
    let attributes = security.attributes();
    // SAFETY: path and private descriptor are live for the synchronous creation.
    let created = unsafe { CreateDirectoryW(name.as_ptr(), &attributes) } != 0;
    if !created {
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::AlreadyExists {
            return Err(error.into());
        }
    }
    let current_parent = directory(parent, principal, false, true)?;
    if identity(&current_parent, true)? != before {
        return Err(refusal());
    }
    directory(path, principal, created, !created)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Image {
    identity: Identity,
    size: u64,
    modified: i64,
    changed: i64,
}
fn image(file: &File, principal: &Principal, strict: bool) -> Result<Image, SecretError> {
    let identity = identity(file, false)?;
    principal.check(file, strict, false)?;
    let standard: FILE_STANDARD_INFO = info(file, FileStandardInfo)?;
    let basic: FILE_BASIC_INFO = info(file, FileBasicInfo)?;
    let size = u64::try_from(standard.EndOfFile).map_err(|_| refusal())?;
    // Times detect observed image changes only; identity is the actual live file ID.
    Ok(Image {
        identity,
        size,
        modified: basic.LastWriteTime,
        changed: basic.ChangeTime,
    })
}

pub(super) struct CheckedLock {
    lock: FileLock,
    principal: Principal,
    path: PathBuf,
    identity: Identity,
}
impl CheckedLock {
    fn acquire(path: PathBuf, principal: &Principal) -> Result<Self, SecretError> {
        let file = open(
            &path,
            principal,
            FILE_GENERIC_READ | FILE_GENERIC_WRITE,
            OPEN_ALWAYS,
            false,
            false,
        )?;
        let identity = image(&file, principal, true)?.identity;
        let lock = FileLock::acquire(file)?;
        let result = Self {
            lock,
            principal: principal.clone(),
            path,
            identity,
        };
        result.check()?;
        Ok(result)
    }
    pub(super) fn check(&self) -> Result<(), SecretError> {
        self.principal.check_current()?;
        if image(self.lock.file(), &self.principal, true)?.identity != self.identity {
            return Err(refusal());
        }
        let path = open(
            &self.path,
            &self.principal,
            READ_CONTROL | FILE_READ_ATTRIBUTES,
            OPEN_EXISTING,
            false,
            false,
        )?;
        if image(&path, &self.principal, true)?.identity != self.identity {
            return Err(refusal());
        }
        Ok(())
    }
}

pub(super) struct Directories {
    principal: Principal,
    root: File,
    root_id: Identity,
    accounts: File,
    accounts_id: Identity,
}
impl Directories {
    pub(super) fn prepare(requested: &Path) -> Result<(PathBuf, PathBuf, Self), SecretError> {
        let principal = Principal::current()?;
        let absolute = supported_path(requested)?;
        let root = create_tree(&absolute, &principal)?;
        principal.check(&root, false, false)?;
        let root_id = identity(&root, true)?;
        let path = absolute.canonicalize()?;
        let accounts_path = path.join("key-metadata-locks");
        let accounts = create_tree(&accounts_path, &principal)?;
        principal.check(&accounts, false, false)?;
        let accounts_id = identity(&accounts, true)?;
        let proof = Self {
            principal,
            root,
            root_id,
            accounts,
            accounts_id,
        };
        proof.check(&path, &accounts_path)?;
        Ok((path, accounts_path, proof))
    }
    pub(super) fn check(&self, root: &Path, accounts: &Path) -> Result<(), SecretError> {
        self.principal.check_current()?;
        for (path, file, expected) in [
            (root, &self.root, self.root_id),
            (accounts, &self.accounts, self.accounts_id),
        ] {
            self.principal.check(file, false, false)?;
            if identity(file, true)? != expected
                || identity(&directory(path, &self.principal, false, false)?, true)? != expected
            {
                return Err(refusal());
            }
        }
        Ok(())
    }
    pub(super) fn lock(&self, path: PathBuf) -> Result<CheckedLock, SecretError> {
        CheckedLock::acquire(path, &self.principal)
    }
    fn metadata_file(&self, path: &Path) -> Result<File, SecretError> {
        open(
            path,
            &self.principal,
            FILE_GENERIC_READ,
            OPEN_EXISTING,
            false,
            true,
        )
    }
    pub(super) fn image_at(&self, path: &Path) -> Result<Option<Image>, SecretError> {
        match self.metadata_file(path) {
            Ok(file) => Ok(Some(image(&file, &self.principal, false)?)),
            Err(SecretError::Io(error)) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }
    pub(super) fn read(&self, path: &Path) -> Result<(Vec<u8>, Option<Image>), SecretError> {
        let mut file = match self.metadata_file(path) {
            Ok(file) => file,
            Err(SecretError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
                if self.image_at(path)?.is_some() {
                    return Err(refusal());
                }
                return Ok((Vec::new(), None));
            }
            Err(error) => return Err(error),
        };
        let before = image(&file, &self.principal, false)?;
        let mut bytes = Vec::new();
        (&mut file)
            .take(before.size.checked_add(1).ok_or_else(refusal)?)
            .read_to_end(&mut bytes)?;
        if u64::try_from(bytes.len()).ok() != Some(before.size)
            || image(&file, &self.principal, false)? != before
            || self.image_at(path)? != Some(before)
        {
            return Err(refusal());
        }
        Ok((bytes, Some(before)))
    }
    pub(super) fn stage(&self, root: &Path) -> Result<tempfile::NamedTempFile, SecretError> {
        let mut builder = tempfile::Builder::new();
        builder.prefix(".keys-").suffix(".tmp");
        let staged = builder.make_in(root, |path| {
            open(
                path,
                &self.principal,
                FILE_GENERIC_READ | FILE_GENERIC_WRITE,
                CREATE_NEW,
                false,
                true,
            )
            .map_err(|error| match error {
                SecretError::Io(error) => error,
                _ => io::Error::other("private key metadata staging could not be created"),
            })
        })?;
        // RAII owns the created name before any verification/write can fail.
        image(staged.as_file(), &self.principal, true)?;
        Ok(staged)
    }
    pub(super) fn rename(
        &self,
        staged: tempfile::NamedTempFile,
        target: &Path,
    ) -> Result<File, SecretError> {
        let expected = image(staged.as_file(), &self.principal, true)?.identity;
        let stage_path = self.metadata_file(staged.path())?;
        if image(&stage_path, &self.principal, true)?.identity != expected {
            return Err(refusal());
        }
        drop(stage_path);
        let from = wide(staged.path())?;
        let to = wide(target)?;
        let (file, mut path) = staged.into_parts();
        // SAFETY: both paths are live NUL-terminated siblings. No cross-volume copy
        // flag/fallback. The open stage allows the delete sharing needed for rename.
        if unsafe {
            MoveFileExW(
                from.as_ptr(),
                to.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        } == 0
        {
            return Err(io::Error::last_os_error().into());
        }
        // The old name is now vacant; never clean up a later object using that name.
        path.disable_cleanup(true);
        Ok(file)
    }
    pub(super) fn finish(
        &self,
        published: &File,
        _root: &Path,
        target: &Path,
    ) -> Result<(), SecretError> {
        // File flush + write-through namespace replacement is the chosen Windows
        // contract, not an invented portable directory fsync/power-loss guarantee.
        published.sync_all()?;
        let expected = image(published, &self.principal, true)?.identity;
        let actual = self.metadata_file(target)?;
        if image(&actual, &self.principal, true)?.identity != expected {
            return Err(refusal());
        }
        Ok(())
    }
}
