//! Process-safe advisory file locks shared by registry, guidance, and Git writes.
//! The lock file stays in place; dropping the handle releases the OS lock, including
//! when a process exits without running Rust destructors.

use std::fs::File;
use std::io;

#[derive(Debug)]
pub struct FileLock {
    file: File,
}

impl FileLock {
    pub fn acquire(file: File) -> io::Result<Self> {
        lock_file(&file)?;
        Ok(Self { file })
    }

    /// Take the lock only when nobody holds it right now; `Ok(None)` means it is busy.
    #[cfg(unix)]
    pub fn try_acquire(file: File) -> io::Result<Option<Self>> {
        use std::os::fd::AsRawFd;

        // SAFETY: flock only borrows this live file descriptor for the duration of the call.
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            return Ok(Some(Self { file }));
        }
        let error = io::Error::last_os_error();
        if error.kind() == io::ErrorKind::WouldBlock {
            Ok(None)
        } else {
            Err(error)
        }
    }

    #[cfg(target_os = "windows")]
    pub fn try_acquire(file: File) -> io::Result<Option<Self>> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::Foundation::ERROR_LOCK_VIOLATION;
        use windows_sys::Win32::Storage::FileSystem::{
            LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY, LockFileEx,
        };
        // SAFETY: an all-zero OVERLAPPED initializes a synchronous byte-range lock.
        let mut overlapped: windows_sys::Win32::System::IO::OVERLAPPED =
            unsafe { std::mem::zeroed() };
        // SAFETY: the live handle and initialized out-parameter cover this synchronous lock.
        if unsafe {
            LockFileEx(
                file.as_raw_handle(),
                LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
                0,
                u32::MAX,
                u32::MAX,
                &mut overlapped,
            )
        } != 0
        {
            return Ok(Some(Self { file }));
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() == Some(ERROR_LOCK_VIOLATION as i32) {
            Ok(None)
        } else {
            Err(error)
        }
    }

    /// The locked file, for reading or writing its contents while the lock is held.
    pub(crate) fn file(&self) -> &File {
        &self.file
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        unlock_file(&self.file);
    }
}

#[cfg(unix)]
fn lock_file(file: &File) -> io::Result<()> {
    use std::os::fd::AsRawFd;

    // SAFETY: flock only borrows this live file descriptor for the duration of the call.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(unix)]
fn unlock_file(file: &File) {
    use std::os::fd::AsRawFd;

    // SAFETY: the descriptor remains live until FileLock is dropped.
    let _ = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
}

#[cfg(target_os = "windows")]
fn lock_file(file: &File) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{LOCKFILE_EXCLUSIVE_LOCK, LockFileEx};
    use windows_sys::Win32::System::IO::OVERLAPPED;

    // SAFETY: the handle is live and OVERLAPPED is initialized for a synchronous byte-range lock.
    let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
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
fn unlock_file(file: &File) {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::UnlockFileEx;
    use windows_sys::Win32::System::IO::OVERLAPPED;

    // SAFETY: this unlocks the same live handle and byte range acquired by lock_file.
    let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
    let _ = unsafe { UnlockFileEx(file.as_raw_handle(), 0, u32::MAX, u32::MAX, &mut overlapped) };
}

#[cfg(not(any(unix, target_os = "windows")))]
fn lock_file(_file: &File) -> io::Result<()> {
    Ok(())
}

#[cfg(not(any(unix, target_os = "windows")))]
fn unlock_file(_file: &File) {}
