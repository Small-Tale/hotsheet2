//! A process-safe advisory lock on one checkout's Job directory. The lock file stays in
//! place; the OS releases the lock when the handle drops, including when a process dies
//! without running destructors.
//!
//! This mirrors `hotsheet_ticketing::FileLock` instead of importing it: orchestration code
//! inside `hotsheet-ticketing` consumes Jobs, so this crate must not depend on that one
//! (HS2-GCHX8V).

use std::fs::File;
use std::io;

/// Open a lock file read-only, creating it first when missing. A read-only handle is
/// enough for an advisory lock, and closing it reports no write to a directory watcher.
pub(crate) fn open_lock_file(path: &std::path::Path) -> io::Result<File> {
    match File::open(path) {
        Ok(file) => Ok(file),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(path)?;
            File::open(path)
        }
        Err(error) => Err(error),
    }
}

#[derive(Debug)]
pub(crate) struct DirLock {
    file: File,
}

impl DirLock {
    pub(crate) fn acquire(file: File) -> io::Result<Self> {
        lock_file(&file)?;
        Ok(Self { file })
    }

    /// Take the lock only when nobody holds it right now; `Ok(None)` means it is busy.
    pub(crate) fn try_acquire(file: File) -> io::Result<Option<Self>> {
        if try_lock_file(&file)? {
            Ok(Some(Self { file }))
        } else {
            Ok(None)
        }
    }
}

impl Drop for DirLock {
    fn drop(&mut self) {
        unlock_file(&self.file);
    }
}

#[cfg(unix)]
fn lock_file(file: &File) -> io::Result<()> {
    use std::os::fd::AsRawFd;

    loop {
        // SAFETY: flock only borrows this live file descriptor for the duration of the call.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

#[cfg(unix)]
fn try_lock_file(file: &File) -> io::Result<bool> {
    use std::os::fd::AsRawFd;

    // SAFETY: flock only borrows this live file descriptor for the duration of the call.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
        return Ok(true);
    }
    let error = io::Error::last_os_error();
    if error.kind() == io::ErrorKind::WouldBlock {
        Ok(false)
    } else {
        Err(error)
    }
}

#[cfg(unix)]
fn unlock_file(file: &File) {
    use std::os::fd::AsRawFd;

    // SAFETY: the descriptor remains live until DirLock is dropped.
    let _ = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
}

#[cfg(target_os = "windows")]
fn lock_file(file: &File) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{LOCKFILE_EXCLUSIVE_LOCK, LockFileEx};
    use windows_sys::Win32::System::IO::OVERLAPPED;

    // SAFETY: an all-zero OVERLAPPED initializes a synchronous byte-range lock.
    let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
    // SAFETY: the handle is live and the out-parameter is initialized.
    let result = unsafe {
        LockFileEx(
            file.as_raw_handle(),
            LOCKFILE_EXCLUSIVE_LOCK,
            0,
            u32::MAX,
            u32::MAX,
            &mut overlapped,
        )
    };
    if result != 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(target_os = "windows")]
fn try_lock_file(file: &File) -> io::Result<bool> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::ERROR_LOCK_VIOLATION;
    use windows_sys::Win32::Storage::FileSystem::{
        LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY, LockFileEx,
    };
    use windows_sys::Win32::System::IO::OVERLAPPED;

    // SAFETY: an all-zero OVERLAPPED initializes a synchronous byte-range lock.
    let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
    // SAFETY: the live handle and initialized out-parameter cover this synchronous lock.
    let result = unsafe {
        LockFileEx(
            file.as_raw_handle(),
            LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
            0,
            u32::MAX,
            u32::MAX,
            &mut overlapped,
        )
    };
    if result != 0 {
        return Ok(true);
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(ERROR_LOCK_VIOLATION as i32) {
        Ok(false)
    } else {
        Err(error)
    }
}

#[cfg(target_os = "windows")]
fn unlock_file(file: &File) {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::UnlockFileEx;
    use windows_sys::Win32::System::IO::OVERLAPPED;

    // SAFETY: an all-zero OVERLAPPED addresses the byte range lock_file acquired.
    let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
    // SAFETY: this unlocks the same live handle and byte range acquired by lock_file.
    let _ = unsafe { UnlockFileEx(file.as_raw_handle(), 0, u32::MAX, u32::MAX, &mut overlapped) };
}

#[cfg(not(any(unix, target_os = "windows")))]
fn lock_file(_file: &File) -> io::Result<()> {
    Ok(())
}

#[cfg(not(any(unix, target_os = "windows")))]
fn try_lock_file(_file: &File) -> io::Result<bool> {
    Ok(true)
}

#[cfg(not(any(unix, target_os = "windows")))]
fn unlock_file(_file: &File) {}
