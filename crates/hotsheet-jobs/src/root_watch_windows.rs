//! HS2-CRXWR9: the Windows adapter of [`crate::watch::RootWatch`] (decisions in
//! `root_plan.rs`).
//!
//! `ReadDirectoryChangesW` reports a directory's entries through a handle to it, and has no
//! notice for the directory itself moving. So each registration opens two handles, each
//! watching its directory's subdirectory names without the subtree:
//!
//! - the anchor (the root's parent, or the nearest existing ancestor while it is missing),
//!   for the name that leads to the root: the root created, removed or renamed;
//! - the anchor's parent, for the anchor's own name: the anchor renamed or removed.
//!
//! A read that fails (the anchor was deleted while held: its handle reports an error) or
//! returns no records (the buffer overflowed) re-resolves the plan and reports. A directory
//! that cannot be opened (missing, or deleted while another handle still holds it) is
//! skipped for the next ancestor. Handles share delete access, so they never keep a store
//! from being removed or renamed. Each handle has its own event; the watch thread waits on
//! those and a stop event.

use std::collections::HashMap;
use std::ffi::OsString;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use windows_sys::Win32::Foundation::{
    CloseHandle, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, CreateFileW, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OVERLAPPED,
    FILE_LIST_DIRECTORY, FILE_NOTIFY_CHANGE_DIR_NAME, FILE_SHARE_DELETE, FILE_SHARE_READ,
    FILE_SHARE_WRITE, GetFileInformationByHandle, OPEN_EXISTING, ReadDirectoryChangesW,
};
use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
use windows_sys::Win32::System::Threading::{
    CreateEventW, INFINITE, ResetEvent, SetEvent, WaitForMultipleObjects,
};

use crate::root_plan::{
    Backend, Identity, Plan, ThreadedRootWatch, Wake, notify_information_names,
};

/// Bytes of notices one read can hold (the records are 4-byte aligned).
const BUFFER_WORDS: usize = 2048;

/// One open directory with a read pending on it.
struct Dir {
    handle: HANDLE,
    event: HANDLE,
    overlapped: Box<OVERLAPPED>,
    buffer: Box<[u32; BUFFER_WORDS]>,
    pending: bool,
    /// The directory object the handle holds.
    identity: Option<Identity>,
}

impl Dir {
    fn open(path: &Path) -> Option<Self> {
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
        // SAFETY: `wide` is NUL-terminated and outlives the call; no template or security
        // attributes are passed.
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                FILE_LIST_DIRECTORY,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                std::ptr::null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OVERLAPPED,
                std::ptr::null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE || handle.is_null() {
            return None;
        }
        // SAFETY: a manual-reset, unnamed, initially clear event.
        let event = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
        if event.is_null() {
            // SAFETY: `handle` was opened above and is not shared.
            unsafe { CloseHandle(handle) };
            return None;
        }
        // SAFETY: `OVERLAPPED` is plain data; all zero is its documented initial state.
        let mut overlapped: Box<OVERLAPPED> = Box::new(unsafe { std::mem::zeroed() });
        overlapped.hEvent = event;
        let mut dir = Self {
            handle,
            event,
            overlapped,
            buffer: Box::new([0; BUFFER_WORDS]),
            pending: false,
            identity: handle_identity(handle),
        };
        dir.read().then_some(dir)
    }

    /// Start the next read; false when the directory can no longer be read.
    fn read(&mut self) -> bool {
        // SAFETY: `event` is this directory's own event.
        unsafe { ResetEvent(self.event) };
        // SAFETY: the buffer and the overlapped structure are boxed, so they stay at their
        // addresses until the read completes or is cancelled and waited for in `drop`.
        let started = unsafe {
            ReadDirectoryChangesW(
                self.handle,
                self.buffer.as_mut_ptr().cast(),
                (BUFFER_WORDS * 4) as u32,
                0,
                FILE_NOTIFY_CHANGE_DIR_NAME,
                std::ptr::null_mut(),
                &mut *self.overlapped,
                None,
            )
        };
        self.pending = started != 0;
        self.pending
    }

    /// Take the completed read: its notices, then start the next one.
    fn complete(&mut self, key: u64, raws: &mut Vec<Raw>) {
        let mut bytes = 0u32;
        // SAFETY: the read on `handle` with this overlapped structure has completed (its
        // event is set), so this does not wait.
        let done =
            unsafe { GetOverlappedResult(self.handle, &*self.overlapped, &mut bytes, 0) } != 0;
        self.pending = false;
        if !done {
            raws.push(Raw::Failed(key));
            return;
        }
        if bytes == 0 {
            raws.push(Raw::Overflow(key));
        } else {
            // SAFETY: the kernel wrote `bytes` bytes (at most the buffer's length) into the
            // buffer, which is plain `u32` storage viewed as bytes.
            let filled = unsafe {
                std::slice::from_raw_parts(
                    self.buffer.as_ptr().cast::<u8>(),
                    (bytes as usize).min(BUFFER_WORDS * 4),
                )
            };
            raws.extend(
                notify_information_names(filled)
                    .into_iter()
                    .map(|name| Raw::Entry(key, name)),
            );
        }
        if !self.read() {
            raws.push(Raw::Failed(key));
        }
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        if self.pending {
            let mut bytes = 0u32;
            // SAFETY: cancel this directory's read, then wait until the kernel no longer
            // uses the buffer or the overlapped structure.
            unsafe {
                CancelIoEx(self.handle, &*self.overlapped);
                GetOverlappedResult(self.handle, &*self.overlapped, &mut bytes, 1);
            }
        }
        // SAFETY: both handles were opened by `Dir::open` and are closed once.
        unsafe {
            CloseHandle(self.handle);
            CloseHandle(self.event);
        }
    }
}

/// The directory object a handle holds.
fn handle_identity(handle: HANDLE) -> Option<Identity> {
    // SAFETY: plain data, filled by the call below.
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: `handle` is open and `info` is a valid destination.
    if unsafe { GetFileInformationByHandle(handle, &mut info) } == 0 {
        return None;
    }
    Some((
        u64::from(info.dwVolumeSerialNumber),
        (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
    ))
}

/// The directory object at `path`, if a directory is there.
fn identity(path: &Path) -> Option<Identity> {
    use std::os::windows::fs::OpenOptionsExt;
    // Share deletion so this short-lived probe never blocks a move (HS2-R6FZ54).
    let directory = std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
        .ok()?;
    if !directory.metadata().ok()?.is_dir() {
        return None;
    }
    handle_identity(directory.as_raw_handle())
}

/// One notice from a directory read.
pub(crate) enum Raw {
    /// The read failed: the directory was deleted or its handle is no longer usable.
    Failed(u64),
    /// The read returned no records: the buffer overflowed.
    Overflow(u64),
    /// An entry name was created, removed or renamed.
    Entry(u64, OsString),
}

/// The directories of one registration, by key in [`Windows::dirs`].
pub(crate) struct Registration {
    /// Directories that exist but could not be opened (deleted while another handle holds
    /// them): treated as missing, so the registration skipping them counts as current.
    refused: Vec<PathBuf>,
    anchor: Option<u64>,
    guard: Option<u64>,
    anchor_identity: Option<Identity>,
    root: Option<Identity>,
}

struct Windows {
    stop: Arc<OwnedHandle>,
    dirs: HashMap<u64, Dir>,
    next_key: u64,
}

// SAFETY: the handles and buffers are owned by this backend, which moves to the watch thread
// once and is used only there.
unsafe impl Send for Windows {}

impl Windows {
    fn add(&mut self, dir: Dir) -> u64 {
        self.next_key += 1;
        self.dirs.insert(self.next_key, dir);
        self.next_key
    }
}

impl Backend for Windows {
    type Registration = Registration;
    type Raw = Raw;
    const FOLD_CASE: bool = true;

    fn register(&mut self, root: &Path) -> (Plan, Registration) {
        let mut opened = None;
        let mut refused = Vec::new();
        let plan = Plan::resolve(
            root,
            |candidate| match Dir::open(candidate) {
                Some(dir) => {
                    opened = Some(dir);
                    true
                }
                None => {
                    if identity(candidate).is_some() {
                        refused.push(candidate.to_path_buf());
                    }
                    false
                }
            },
            identity(root).is_some(),
        );
        let anchor_identity = opened.as_ref().and_then(|dir| dir.identity);
        let anchor = opened.map(|dir| self.add(dir));
        let guard = plan
            .above
            .first()
            .and_then(|dir| Dir::open(dir))
            .map(|dir| self.add(dir));
        let root_identity = identity(root);
        (
            plan,
            Registration {
                refused,
                anchor,
                guard,
                anchor_identity,
                root: root_identity,
            },
        )
    }

    fn current(&self, root: &Path, plan: &Plan, registration: &Registration) -> bool {
        let usable = |candidate: &Path| {
            identity(candidate).is_some()
                && !registration.refused.iter().any(|dir| dir == candidate)
        };
        let now = Plan::resolve(root, usable, identity(root).is_some());
        now == *plan
            && plan.anchor.as_deref().and_then(identity) == registration.anchor_identity
            && identity(root) == registration.root
    }

    fn retire(&mut self, old: Registration, _new: &Registration) {
        for key in [old.anchor, old.guard].into_iter().flatten() {
            self.dirs.remove(&key);
        }
    }

    fn notices(&mut self, block: bool) -> Option<Vec<Raw>> {
        let mut raws = Vec::new();
        let mut timeout = if block { INFINITE } else { 0 };
        loop {
            let pending: Vec<u64> = self
                .dirs
                .iter()
                .filter(|(_, dir)| dir.pending)
                .map(|(key, _)| *key)
                .collect();
            let mut handles: Vec<HANDLE> = vec![self.stop.as_raw_handle()];
            handles.extend(pending.iter().map(|key| self.dirs[key].event));
            // SAFETY: every handle in `handles` is an open event owned by this backend.
            let waited = unsafe {
                WaitForMultipleObjects(handles.len() as u32, handles.as_ptr(), 0, timeout)
            };
            if waited == WAIT_TIMEOUT {
                return Some(raws);
            }
            let index = waited.wrapping_sub(WAIT_OBJECT_0) as usize;
            if index == 0 || index >= handles.len() {
                // Asked to stop, or the wait failed.
                return None;
            }
            let key = pending[index - 1];
            if let Some(dir) = self.dirs.get_mut(&key) {
                dir.complete(key, &mut raws);
            }
            // Take whatever else is ready without waiting again.
            timeout = 0;
        }
    }

    fn classify(&self, raw: &Raw, registration: &Registration) -> Option<Wake> {
        let (Raw::Failed(key) | Raw::Overflow(key) | Raw::Entry(key, _)) = raw;
        let anchor = if registration.anchor == Some(*key) {
            true
        } else if registration.guard == Some(*key) {
            false
        } else {
            return None;
        };
        Some(match (raw, anchor) {
            (Raw::Overflow(_), _) => Wake::Overflow,
            (Raw::Failed(_), true) => Wake::AnchorLeft,
            (Raw::Failed(_), false) => Wake::AboveLeft,
            (Raw::Entry(_, name), true) => Wake::AnchorEntry(name.clone()),
            (Raw::Entry(_, name), false) => Wake::AboveEntry(name.clone()),
        })
    }

    fn identity(&self, path: &Path) -> Option<Identity> {
        identity(path)
    }
}

/// Start a root watch on `root`.
pub(crate) fn start(
    root: &Path,
    handler: Box<dyn FnMut() + Send>,
) -> std::io::Result<ThreadedRootWatch> {
    // SAFETY: a manual-reset, unnamed, initially clear event, owned below.
    let stop = unsafe { CreateEventW(std::ptr::null(), 1, 0, std::ptr::null()) };
    if stop.is_null() {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `stop` was just created and is owned by nothing else.
    let stop = Arc::new(unsafe { OwnedHandle::from_raw_handle(stop) });
    let trigger = stop.clone();
    ThreadedRootWatch::start(
        Windows {
            stop,
            dirs: HashMap::new(),
            next_key: 0,
        },
        // SAFETY: the stop event stays open while `trigger` holds it.
        Box::new(move || unsafe { SetEvent(trigger.as_raw_handle()) } != 0),
        root,
        handler,
    )
}
