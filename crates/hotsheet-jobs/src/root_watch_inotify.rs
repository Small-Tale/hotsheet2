//! HS2-CRXWR9: the Linux adapter of [`crate::watch::RootWatch`] (decisions in
//! `root_plan.rs`).
//!
//! An inotify watch belongs to a directory object, not its path: it follows the directory
//! when it moves and ends when it is removed. So each registration watches
//!
//! - the anchor (the root's parent, or the nearest existing ancestor while it is missing):
//!   its entries being created, removed or renamed, and itself being removed or moved;
//! - the root while it exists: itself being removed or moved;
//! - every ancestor above the anchor: itself being removed or moved only, so a grandparent
//!   moved away and back is seen without any of its entry churn.
//!
//! A new registration adds its watches while the old ones stay in place (inotify returns
//! the same watch for the same directory), and the one queue keeps every notice raised in
//! between, so nothing is lost between two registrations. A stop request is an `eventfd`
//! polled beside the queue. The inotify watcher of `inotify_tree.rs` is notify's; this one
//! needs notices notify does not pass on (a watched directory's own move), so it talks to
//! the kernel directly, and only [`crate::watch::RootWatch::start`] starts it.

use std::collections::HashMap;
use std::ffi::{CString, OsString};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::root_plan::{Backend, Identity, Plan, ThreadedRootWatch, Wake};

/// A watched directory left: removed, moved, or its file system unmounted.
const SELF_LEFT: u32 = libc::IN_DELETE_SELF | libc::IN_MOVE_SELF | libc::IN_UNMOUNT;
/// An entry of a watched directory was created, removed or renamed.
const ENTRY: u32 = libc::IN_CREATE | libc::IN_DELETE | libc::IN_MOVED_FROM | libc::IN_MOVED_TO;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Role {
    Root,
    Anchor,
    Above,
}

/// The watches of one registration and the objects they were placed on.
pub(crate) struct Registration {
    roles: HashMap<i32, Role>,
    anchor: Option<Identity>,
    root: Option<Identity>,
    /// A watched path changed while it was being registered.
    stale: bool,
    /// Directories that exist but could not be watched (no read permission): treated as
    /// missing, so the registration skipping them counts as current.
    refused: Vec<PathBuf>,
}

/// One inotify notice.
pub(crate) struct Raw {
    wd: i32,
    mask: u32,
    name: Option<OsString>,
}

struct Inotify {
    queue: OwnedFd,
    stop: Arc<OwnedFd>,
}

fn identity(path: &Path) -> Option<Identity> {
    use std::os::unix::fs::MetadataExt;
    let metadata = std::fs::metadata(path).ok()?;
    metadata.is_dir().then(|| (metadata.dev(), metadata.ino()))
}

/// Watch the directory at `path` for `mask`. Returns the watch and the directory object
/// it was placed on, or `None` as the object when the path changed meanwhile.
fn watch(queue: i32, path: &Path, mask: u32) -> Option<(i32, Option<Identity>)> {
    let c_path = CString::new(path.as_os_str().as_bytes()).ok()?;
    let before = identity(path)?;
    // SAFETY: `c_path` is a NUL-terminated string that outlives the call.
    let wd = unsafe { libc::inotify_add_watch(queue, c_path.as_ptr(), mask | libc::IN_ONLYDIR) };
    if wd < 0 {
        return None;
    }
    let after = identity(path);
    Some((wd, (after == Some(before)).then_some(before)))
}

impl Backend for Inotify {
    type Registration = Registration;
    type Raw = Raw;
    const FOLD_CASE: bool = false;

    fn register(&mut self, root: &Path) -> (Plan, Registration) {
        let queue = self.queue.as_raw_fd();
        let mut roles = HashMap::new();
        let mut stale = false;
        let mut anchor = None;
        let mut refused = Vec::new();
        let mut plan = Plan::resolve(
            root,
            |candidate| match watch(queue, candidate, SELF_LEFT | ENTRY) {
                Some((wd, object)) => {
                    roles.insert(wd, Role::Anchor);
                    stale |= object.is_none();
                    anchor = object;
                    true
                }
                None => {
                    if candidate.is_dir() {
                        refused.push(candidate.to_path_buf());
                    }
                    false
                }
            },
            false,
        );
        let mut root_object = None;
        if let Some((wd, object)) = watch(queue, root, SELF_LEFT) {
            roles.insert(wd, Role::Root);
            stale |= object.is_none();
            root_object = object;
            plan.root_present = true;
        } else if root.is_dir() {
            refused.push(root.to_path_buf());
        }
        for dir in &plan.above {
            if let Some((wd, _)) = watch(queue, dir, SELF_LEFT) {
                roles.entry(wd).or_insert(Role::Above);
            }
        }
        (
            plan,
            Registration {
                roles,
                anchor,
                root: root_object,
                stale,
                refused,
            },
        )
    }

    fn current(&self, root: &Path, plan: &Plan, registration: &Registration) -> bool {
        if registration.stale {
            return false;
        }
        let usable = |candidate: &Path| {
            candidate.is_dir() && !registration.refused.iter().any(|dir| dir == candidate)
        };
        let now = Plan::resolve(root, usable, usable(root));
        now == *plan
            && plan.anchor.as_deref().and_then(identity) == registration.anchor
            && identity(root) == registration.root
    }

    fn retire(&mut self, old: Registration, new: &Registration) {
        for wd in old.roles.keys().filter(|wd| !new.roles.contains_key(wd)) {
            // SAFETY: plain system call on the queue this backend owns; a watch the kernel
            // already dropped (its directory removed) only fails with EINVAL.
            unsafe { libc::inotify_rm_watch(self.queue.as_raw_fd(), *wd) };
        }
    }

    fn notices(&mut self, block: bool) -> Option<Vec<Raw>> {
        let mut fds = [
            libc::pollfd {
                fd: self.queue.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: self.stop.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        loop {
            // SAFETY: `fds` is a valid array of its length for the call.
            let ready = unsafe { libc::poll(fds.as_mut_ptr(), 2, if block { -1 } else { 0 }) };
            if ready < 0 {
                if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                    continue;
                }
                return None;
            }
            break;
        }
        if fds[1].revents != 0 {
            return None;
        }
        if fds[0].revents & !libc::POLLIN != 0 {
            return None;
        }
        let mut raws = Vec::new();
        let mut buffer = vec![0u8; 64 * 1024];
        loop {
            // SAFETY: `buffer` is valid for writes of its length.
            let read = unsafe {
                libc::read(
                    self.queue.as_raw_fd(),
                    buffer.as_mut_ptr().cast(),
                    buffer.len(),
                )
            };
            if read < 0 {
                match std::io::Error::last_os_error().kind() {
                    std::io::ErrorKind::WouldBlock => return Some(raws),
                    std::io::ErrorKind::Interrupted => continue,
                    _ => return None,
                }
            }
            if read == 0 {
                return Some(raws);
            }
            parse(&buffer[..read as usize], &mut raws);
        }
    }

    fn classify(&self, raw: &Raw, registration: &Registration) -> Option<Wake> {
        if raw.mask & libc::IN_Q_OVERFLOW != 0 {
            return Some(Wake::Overflow);
        }
        if raw.mask & libc::IN_IGNORED != 0 {
            return None;
        }
        let left = raw.mask & SELF_LEFT != 0;
        match registration.roles.get(&raw.wd)? {
            Role::Root => left.then_some(Wake::RootLeft),
            Role::Anchor if left => Some(Wake::AnchorLeft),
            Role::Anchor => (raw.mask & ENTRY != 0)
                .then(|| raw.name.clone().map(Wake::AnchorEntry))
                .flatten(),
            Role::Above => left.then_some(Wake::AboveLeft),
        }
    }

    fn identity(&self, path: &Path) -> Option<Identity> {
        identity(path)
    }
}

/// Append the notices in one `read` of an inotify queue.
fn parse(bytes: &[u8], raws: &mut Vec<Raw>) {
    let header = std::mem::size_of::<libc::inotify_event>();
    let mut offset = 0;
    while offset + header <= bytes.len() {
        // SAFETY: at least `header` bytes remain at `offset`; the read may be unaligned.
        let event: libc::inotify_event =
            unsafe { std::ptr::read_unaligned(bytes[offset..].as_ptr().cast()) };
        let start = offset + header;
        let end = (start + event.len as usize).min(bytes.len());
        let name = bytes[start..end]
            .split(|byte| *byte == 0)
            .next()
            .filter(|name| !name.is_empty())
            .map(|name| OsString::from_vec(name.to_vec()));
        raws.push(Raw {
            wd: event.wd,
            mask: event.mask,
            name,
        });
        offset = end;
    }
}

/// Start a root watch on `root`.
pub(crate) fn start(
    root: &Path,
    handler: Box<dyn FnMut() + Send>,
) -> std::io::Result<ThreadedRootWatch> {
    // SAFETY: plain system call; the descriptor is owned below.
    let queue = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
    if queue < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `queue` was just created and is owned by nothing else.
    let queue = unsafe { OwnedFd::from_raw_fd(queue) };
    // SAFETY: plain system call; the descriptor is owned below.
    let stop = unsafe { libc::eventfd(0, libc::EFD_NONBLOCK | libc::EFD_CLOEXEC) };
    if stop < 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `stop` was just created and is owned by nothing else.
    let stop = Arc::new(unsafe { OwnedFd::from_raw_fd(stop) });
    let trigger = stop.clone();
    ThreadedRootWatch::start(
        Inotify { queue, stop },
        Box::new(move || {
            let one: u64 = 1;
            // SAFETY: writes the 8 bytes of `one` to the eventfd this watch owns.
            let written = unsafe {
                libc::write(
                    trigger.as_raw_fd(),
                    std::ptr::from_ref(&one).cast(),
                    std::mem::size_of::<u64>(),
                )
            };
            written == std::mem::size_of::<u64>() as isize
        }),
        root,
        handler,
    )
}
