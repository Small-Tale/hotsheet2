//! HS2-CRXWR9: the platform-neutral half of [`crate::watch::RootWatch`] on Linux and Windows.
//!
//! A store watcher must see its store directory vanish and come back (HS2-R6FZ54). Off
//! macOS it used to watch the store's parent with notify. That watch belongs to the
//! parent's inode (inotify) or handle (`ReadDirectoryChangesW`), so it ended with the
//! parent: a parent removed and recreated with a store inside was never re-armed.
//!
//! This module decides what a root watch registers and when it reports; the OS adapters
//! (`root_watch_inotify.rs`, `root_watch_windows.rs`) only register directories and turn
//! raw notices into [`Wake`]s:
//!
//! - [`Plan::resolve`]: the *anchor* is the root's parent, or while it is missing the
//!   nearest ancestor that exists (and the adapter could register). The anchor's entries
//!   are watched for the one name that leads towards the root ([`Plan::next`]); the root
//!   itself while it exists; and the ancestors above the anchor ([`Plan::above`]), so a
//!   grandparent moved or removed is seen too.
//! - [`Plan::moves_root`]: whether a wake means the directory may have left its path,
//!   which is reported even when it is back by the time anyone looks.
//! - [`report`]: report on such a wake, or when the directory object at the path changed.
//! - [`run`]: the watch loop. Every relevant wake registers again while the old
//!   registration stays in place and collects anything raised in between, then retires it,
//!   so a move away and straight back is never lost between two registrations.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// A directory object: device (or volume) and inode (or file index).
pub(crate) type Identity = (u64, u64);

/// How many registrations in a row may collect notices while still current before the
/// watch waits for the next wake anyway (the tree keeps changing beside the root).
pub(crate) const MAX_STALE_RETRIES: u32 = 8;

/// The most registrations one re-registration makes while they keep going stale under it.
/// A stale registration is never kept while the tree settles: the notice that would have
/// corrected it may already have been taken. This only stops a tree that never settles
/// from holding the watch thread.
pub(crate) const MAX_REGISTRATIONS: u32 = 1000;

/// What one registration of a root watch covers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Plan {
    /// The directory whose entries are watched: the root's parent, or while that is
    /// missing the nearest ancestor that exists. `None` when no ancestor could be used.
    pub(crate) anchor: Option<PathBuf>,
    /// The anchor's entry on the way to the root: the root's own name when the anchor is
    /// its parent, otherwise the missing directory below the anchor.
    pub(crate) next: Option<OsString>,
    /// Whether a directory is at the root path (and is watched for leaving it).
    pub(crate) root_present: bool,
    /// The anchor's own ancestors, nearest first.
    pub(crate) above: Vec<PathBuf>,
    /// Whether the anchor is the root's parent.
    pub(crate) anchor_is_parent: bool,
}

/// What woke a root watch, as the adapter read it against one registration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Wake {
    /// The directory at the root path was removed, moved, unmounted or revoked (Linux
    /// watches the root itself; Windows sees it through the anchor's entries).
    #[cfg(any(target_os = "linux", test))]
    RootLeft,
    /// The anchor itself was removed, moved or unmounted, or its watch failed.
    AnchorLeft,
    /// An ancestor above the anchor was removed or moved, or its watch failed.
    AboveLeft,
    /// An entry of the anchor was created, removed or renamed (either name).
    AnchorEntry(OsString),
    /// An entry of the anchor's own parent changed (Windows: it has no self notice, so
    /// the anchor leaving is seen from above; Linux watches the ancestors themselves).
    #[cfg(any(target_os = "windows", test))]
    AboveEntry(OsString),
    /// The platform dropped notices; anything may have happened.
    Overflow,
}

/// Whether two entry names are the same name on this platform.
pub(crate) fn same_name(a: &OsStr, b: &OsStr, fold_case: bool) -> bool {
    if fold_case {
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    } else {
        a == b
    }
}

impl Plan {
    /// Resolve the plan for `root`. `use_anchor` is asked about the root's ancestors,
    /// nearest first, and the first one it accepts (an existing directory the adapter could
    /// register) is the anchor. `root_present` says whether a directory is at `root`.
    pub(crate) fn resolve(
        root: &Path,
        mut use_anchor: impl FnMut(&Path) -> bool,
        root_present: bool,
    ) -> Self {
        let anchor = root
            .parent()
            .into_iter()
            .flat_map(Path::ancestors)
            .find(|candidate| use_anchor(candidate))
            .map(Path::to_path_buf);
        let anchor_is_parent = anchor
            .as_deref()
            .is_some_and(|anchor| Some(anchor) == root.parent());
        let next = anchor.as_deref().and_then(|anchor| {
            root.strip_prefix(anchor)
                .ok()
                .and_then(|rest| rest.components().next())
                .map(|component| component.as_os_str().to_os_string())
        });
        let above = anchor
            .as_deref()
            .map(|anchor| anchor.ancestors().skip(1).map(Path::to_path_buf).collect())
            .unwrap_or_default();
        Self {
            anchor,
            next,
            root_present,
            above,
            anchor_is_parent,
        }
    }

    /// The anchor's own name, which the anchor's parent reports when the anchor leaves.
    #[cfg(any(target_os = "windows", test))]
    fn anchor_name(&self) -> Option<&OsStr> {
        self.anchor.as_deref().and_then(Path::file_name)
    }

    /// Whether `wake` means the directory may have left its path, even if it is back by
    /// now: the root, the anchor or an ancestor left, notices were dropped, or the root's
    /// own entry in its parent changed.
    pub(crate) fn moves_root(&self, wake: &Wake, fold_case: bool) -> bool {
        match wake {
            #[cfg(any(target_os = "linux", test))]
            Wake::RootLeft => true,
            Wake::AnchorLeft | Wake::AboveLeft | Wake::Overflow => true,
            Wake::AnchorEntry(name) => {
                self.anchor_is_parent
                    && self
                        .next
                        .as_deref()
                        .is_some_and(|next| same_name(name, next, fold_case))
            }
            #[cfg(any(target_os = "windows", test))]
            Wake::AboveEntry(name) => self
                .anchor_name()
                .is_some_and(|anchor| same_name(name, anchor, fold_case)),
        }
    }

    /// Whether `wake` needs the plan resolved again: it may move the root, or it names the
    /// missing directory on the way to the root (which may now exist). Churn under other
    /// names in the anchor needs nothing.
    pub(crate) fn needs_replan(&self, wake: &Wake, fold_case: bool) -> bool {
        self.moves_root(wake, fold_case)
            || matches!(wake, Wake::AnchorEntry(name)
                if self.next.as_deref().is_some_and(|next| same_name(name, next, fold_case)))
    }
}

/// Whether to run the handler: something may have moved the root, or the directory object
/// at the path is not the one last reported.
pub(crate) fn report(moved: bool, last: Option<Identity>, now: Option<Identity>) -> bool {
    moved || last != now
}

/// The OS half of a root watch.
pub(crate) trait Backend {
    /// One registration: the OS watches it holds and what they mean.
    type Registration;
    /// One raw notice, read before it is matched against a registration.
    type Raw;
    /// Whether entry names compare without case (Windows).
    const FOLD_CASE: bool;

    /// Register the plan for `root` as it is now, leaving every earlier registration in
    /// place.
    fn register(&mut self, root: &Path) -> (Plan, Self::Registration);
    /// Whether `plan` and `registration` still hold the objects now at their paths. A
    /// change between resolving a path and registering it raises no notice, so a stale
    /// registration is made again. A directory the backend could not register although it
    /// exists (refused) must count as missing here too, or the watch would register forever.
    fn current(&self, root: &Path, plan: &Plan, registration: &Self::Registration) -> bool;
    /// Stop watching what `old` holds and `new` does not.
    fn retire(&mut self, old: Self::Registration, new: &Self::Registration);
    /// Collect notices: block until at least one arrives, or take only those queued.
    /// `None` when the watch is asked to stop or the OS queue failed.
    fn notices(&mut self, block: bool) -> Option<Vec<Self::Raw>>;
    /// What `raw` means for `registration`; `None` when it is not one of its watches.
    fn classify(&self, raw: &Self::Raw, registration: &Self::Registration) -> Option<Wake>;
    /// The directory object at `path`, if one is there.
    fn identity(&self, path: &Path) -> Option<Identity>;
}

/// A live plan and its registration.
pub(crate) type Held<B> = (Plan, <B as Backend>::Registration);

/// Whether any of `raws` moves the root for any of `held`.
fn any_moves<B: Backend>(backend: &B, raws: &[B::Raw], held: &[&Held<B>]) -> bool {
    raws.iter().any(|raw| {
        held.iter().any(|(plan, registration)| {
            backend
                .classify(raw, registration)
                .is_some_and(|wake| plan.moves_root(&wake, B::FOLD_CASE))
        })
    })
}

/// Register `root` again while `held` stays registered, so no change goes unseen in
/// between: notices raised meanwhile, on either registration, are collected before the old
/// one is retired. Repeats while the paths moved under the new registration (HS2-CRXWR9: a
/// stale registration may already have consumed the notice that would correct it, so it is
/// never kept), and a bounded number of times while notices keep arriving. Returns the new
/// registration and whether the directory may have left its path meanwhile; `None` when the
/// watch should stop.
pub(crate) fn reregister<B: Backend>(
    backend: &mut B,
    root: &Path,
    mut held: Option<Held<B>>,
) -> Option<(Held<B>, bool)> {
    let mut moved = false;
    let mut attempts = 0;
    loop {
        let next = backend.register(root);
        let raised = backend.notices(false)?;
        let mut both = vec![&next];
        if let Some(old) = held.as_ref() {
            both.push(old);
        }
        moved |= any_moves(backend, &raised, &both);
        let current = backend.current(root, &next.0, &next.1);
        // Only a registration made while something moved can be stale.
        moved |= !current;
        if let Some((_, old)) = held.take() {
            backend.retire(old, &next.1);
        }
        held = Some(next);
        attempts += 1;
        let settled = current && (raised.is_empty() || attempts >= MAX_STALE_RETRIES);
        if settled || attempts >= MAX_REGISTRATIONS {
            return held.map(|held| (held, moved));
        }
    }
}

/// The state of a running watch between two looks at its path.
pub(crate) struct Watching<B: Backend> {
    held: Held<B>,
    /// Whether a wake since the last look may have moved the root.
    moved: bool,
    /// The directory object at the path when the handler last ran (or at the start).
    last: Option<Identity>,
}

impl<B: Backend> Watching<B> {
    /// Register `root` for the first time. `None` when the watch should stop.
    pub(crate) fn start(backend: &mut B, root: &Path) -> Option<Self> {
        let last = backend.identity(root);
        let (held, moved) = reregister(backend, root, None)?;
        Some(Self { held, moved, last })
    }

    /// Look at the path: whether the handler should run now.
    pub(crate) fn look(&mut self, backend: &B, root: &Path) -> bool {
        let now = backend.identity(root);
        let changed = report(self.moved, self.last, now);
        self.last = now;
        self.moved = false;
        changed
    }

    /// Take the notices of one wake: register again when one of them may concern the root.
    /// `None` when the watch should stop.
    pub(crate) fn wake(self, backend: &mut B, root: &Path, raws: &[B::Raw]) -> Option<Self> {
        let (plan, registration) = &self.held;
        let replan = raws.iter().any(|raw| {
            backend
                .classify(raw, registration)
                .is_some_and(|wake| plan.needs_replan(&wake, B::FOLD_CASE))
        });
        if !replan {
            // Churn beside the root: nothing to register again.
            return Some(self);
        }
        let woke_moved = any_moves(backend, raws, &[&self.held]);
        let (held, reregistered_moved) = reregister(backend, root, Some(self.held))?;
        Some(Self {
            held,
            moved: self.moved || woke_moved || reregistered_moved,
            last: self.last,
        })
    }
}

/// The watch loop: register, wait, and run `handler` for each change of the directory at
/// `root`. `ready` runs once the first registration is in place.
pub(crate) fn run<B: Backend>(
    backend: &mut B,
    root: &Path,
    handler: &mut dyn FnMut(),
    ready: impl FnOnce(),
) {
    let watching = Watching::start(backend, root);
    ready();
    let Some(mut watching) = watching else {
        return;
    };
    loop {
        if watching.look(backend, root) {
            handler();
        }
        let Some(raws) = backend.notices(true) else {
            return;
        };
        let Some(next) = watching.wake(backend, root, &raws) else {
            return;
        };
        watching = next;
    }
}

/// The entry names in one `ReadDirectoryChangesW` result: a chain of
/// `FILE_NOTIFY_INFORMATION` records (next-entry offset, action, name length in bytes, then
/// the UTF-16 name). Every action counts: a created, removed or renamed entry (either name).
/// A truncated record ends the chain.
#[cfg(any(target_os = "windows", test))]
pub(crate) fn notify_information_names(bytes: &[u8]) -> Vec<OsString> {
    const HEADER: usize = 12;
    let word = |at: usize| -> Option<usize> {
        let field = bytes.get(at..at + 4)?;
        Some(u32::from_le_bytes([field[0], field[1], field[2], field[3]]) as usize)
    };
    let mut names = Vec::new();
    let mut offset = 0;
    loop {
        let (Some(next), Some(length)) = (word(offset), word(offset + 8)) else {
            return names;
        };
        let start = offset + HEADER;
        let Some(name) = bytes.get(start..start + length) else {
            return names;
        };
        let units: Vec<u16> = name
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        names.push(OsString::from(String::from_utf16_lossy(&units)));
        if next == 0 {
            return names;
        }
        offset += next;
    }
}

/// A root watch running [`run`] on its own thread; dropping it stops the thread.
pub(crate) struct ThreadedRootWatch {
    stop: Box<dyn Fn() -> bool + Send>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl std::fmt::Debug for ThreadedRootWatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ThreadedRootWatch").finish_non_exhaustive()
    }
}

impl ThreadedRootWatch {
    /// Run `backend` for `root` on a new thread and return once its first registration is
    /// in place. `stop` asks the backend's [`Backend::notices`] to return `None`, and
    /// reports whether it could.
    pub(crate) fn start<B: Backend + Send + 'static>(
        mut backend: B,
        stop: Box<dyn Fn() -> bool + Send>,
        root: &Path,
        mut handler: Box<dyn FnMut() + Send>,
    ) -> std::io::Result<Self> {
        let root = root.to_path_buf();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("hotsheet-root-watch".into())
            .spawn(move || {
                run(&mut backend, &root, &mut *handler, move || {
                    let _ = ready_tx.send(());
                });
            })?;
        let _ = ready_rx.recv();
        Ok(Self {
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for ThreadedRootWatch {
    fn drop(&mut self) {
        if !(self.stop)() {
            // The thread may still wait on its queue: leave it rather than block here.
            return;
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, VecDeque};
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    fn plan_with(root: &str, existing: &[&str], root_present: bool) -> Plan {
        Plan::resolve(
            Path::new(root),
            |candidate| existing.iter().any(|dir| Path::new(dir) == candidate),
            root_present,
        )
    }

    fn os(name: &str) -> OsString {
        OsString::from(name)
    }

    #[test]
    fn hs2_crxwr9_the_parent_anchors_the_watch_while_it_exists() {
        let plan = plan_with("/a/p/store", &["/", "/a", "/a/p"], true);
        assert_eq!(plan.anchor.as_deref(), Some(Path::new("/a/p")));
        assert!(plan.anchor_is_parent);
        assert_eq!(plan.next, Some(os("store")));
        assert!(plan.root_present);
        assert_eq!(plan.above, [PathBuf::from("/a"), PathBuf::from("/")]);
    }

    #[test]
    fn hs2_crxwr9_a_missing_parent_falls_back_to_the_nearest_existing_ancestor() {
        // Parent removed: the grandparent anchors, watching for the parent's name.
        let plan = plan_with("/a/p/store", &["/", "/a"], false);
        assert_eq!(plan.anchor.as_deref(), Some(Path::new("/a")));
        assert!(!plan.anchor_is_parent);
        assert_eq!(plan.next, Some(os("p")));
        assert_eq!(plan.above, [PathBuf::from("/")]);
        // Grandparent removed too: the next one up, watching for the grandparent's name.
        let plan = plan_with("/a/p/store", &["/"], false);
        assert_eq!(plan.anchor.as_deref(), Some(Path::new("/")));
        assert_eq!(plan.next, Some(os("a")));
        assert!(plan.above.is_empty());
        // Nothing usable at all.
        let plan = plan_with("/a/p/store", &[], false);
        assert_eq!(plan.anchor, None);
        assert_eq!(plan.next, None);
        assert!(plan.above.is_empty());
    }

    #[test]
    fn hs2_crxwr9_an_ancestor_the_adapter_refuses_is_skipped() {
        // The parent exists but cannot be registered (delete pending on Windows, no read
        // permission for inotify): the grandparent anchors instead, and the plan says so.
        let mut asked = Vec::new();
        let plan = Plan::resolve(
            Path::new("/a/p/store"),
            |candidate| {
                asked.push(candidate.to_path_buf());
                candidate == Path::new("/a")
            },
            true,
        );
        assert_eq!(asked, [PathBuf::from("/a/p"), PathBuf::from("/a")]);
        assert_eq!(plan.anchor.as_deref(), Some(Path::new("/a")));
        assert_eq!(plan.next, Some(os("p")));
        assert!(!plan.anchor_is_parent);
        // A relative root without a parent component has no anchor.
        let plan = Plan::resolve(Path::new("store"), |_| true, true);
        assert_eq!(plan.anchor.as_deref(), Some(Path::new("")));
        assert_eq!(plan.next, Some(os("store")));
    }

    #[test]
    fn hs2_crxwr9_which_wakes_move_the_root() {
        let parent = plan_with("/a/p/store", &["/", "/a", "/a/p"], true);
        for wake in [
            Wake::RootLeft,
            Wake::AnchorLeft,
            Wake::AboveLeft,
            Wake::Overflow,
            Wake::AnchorEntry(os("store")),
            Wake::AboveEntry(os("p")),
        ] {
            assert!(parent.moves_root(&wake, false), "{wake:?}");
            assert!(parent.needs_replan(&wake, false), "{wake:?}");
        }
        for wake in [
            Wake::AnchorEntry(os("sibling")),
            Wake::AnchorEntry(os("STORE")),
            Wake::AboveEntry(os("other")),
        ] {
            assert!(!parent.moves_root(&wake, false), "{wake:?}");
            assert!(!parent.needs_replan(&wake, false), "{wake:?}");
        }
        // Windows names fold case.
        assert!(parent.moves_root(&Wake::AnchorEntry(os("STORE")), true));
        assert!(parent.moves_root(&Wake::AboveEntry(os("P")), true));

        // With the parent missing, its name reappearing in the grandparent needs a new
        // plan but is not itself a move: whether the root returned is the identity's call.
        let missing = plan_with("/a/p/store", &["/", "/a"], false);
        assert!(!missing.moves_root(&Wake::AnchorEntry(os("p")), false));
        assert!(missing.needs_replan(&Wake::AnchorEntry(os("p")), false));
        assert!(!missing.needs_replan(&Wake::AnchorEntry(os("store")), false));
        assert!(!missing.needs_replan(&Wake::AnchorEntry(os("q")), false));
    }

    /// One `FILE_NOTIFY_INFORMATION` record, padded to a 4-byte boundary when `last` is
    /// false.
    fn record(action: u32, name: &str, last: bool) -> Vec<u8> {
        let units: Vec<u8> = name.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let size = (12 + units.len()).div_ceil(4) * 4;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&(if last { 0u32 } else { size as u32 }).to_le_bytes());
        bytes.extend_from_slice(&action.to_le_bytes());
        bytes.extend_from_slice(&(units.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&units);
        bytes.resize(size, 0);
        bytes
    }

    #[test]
    fn hs2_crxwr9_windows_notice_names_are_read_from_the_record_chain() {
        let mut bytes = record(4, "store", false);
        bytes.extend(record(5, "store-away", false));
        bytes.extend(record(1, "Ünïcode", true));
        assert_eq!(
            notify_information_names(&bytes),
            [os("store"), os("store-away"), os("Ünïcode")]
        );
        assert!(notify_information_names(&[]).is_empty());
        // A truncated record (its name runs past the end) ends the chain.
        let mut truncated = record(1, "first", false);
        let cut = record(1, "second", true);
        truncated.extend_from_slice(&cut[..cut.len() - 4]);
        assert_eq!(notify_information_names(&truncated), [os("first")]);
        // A header cut short ends it too.
        assert!(notify_information_names(&[0, 0, 0]).is_empty());
    }

    #[test]
    fn hs2_crxwr9_report_on_a_move_or_a_changed_identity() {
        let a = Some((1, 10));
        let b = Some((1, 11));
        let cases = [
            (false, a, a, false),
            (true, a, a, true), // moved away and straight back
            (false, a, None, true),
            (false, None, a, true),
            (false, None, None, false),
            (true, None, None, true),
            (false, a, b, true), // replaced in one step
        ];
        for (moved, last, now, expected) in cases {
            assert_eq!(
                report(moved, last, now),
                expected,
                "{moved} {last:?} {now:?}"
            );
        }
    }

    /// An in-memory directory tree whose changes a test scripts, and a backend over it
    /// that registers like the inotify adapter: by object, so a watch follows a directory
    /// that moves and ends when it is removed.
    /// One scripted change of the in-memory tree.
    type Change = Box<dyn FnOnce(&mut World) + Send>;

    #[derive(Default)]
    struct World {
        /// Path to object id.
        dirs: BTreeMap<PathBuf, u64>,
        next_id: u64,
        /// Watched object id to what it is watched for (for every live registration).
        watches: BTreeMap<u64, usize>,
        queue: VecDeque<FakeRaw>,
        stopped: bool,
        /// Runs once after the next `register`, to change the tree under it.
        after_register: Option<Change>,
        /// Registrations left that each flip the parent's presence right after resolving,
        /// raising no notice (a race no registration catches).
        flips_left: usize,
        /// Directories that exist but cannot be registered.
        refused: Vec<PathBuf>,
        registrations: usize,
    }

    #[derive(Clone, Debug)]
    enum FakeRaw {
        SelfLeft(u64),
        Entry(u64, OsString),
        Overflow,
    }

    impl World {
        fn with(dirs: &[&str]) -> Self {
            let mut world = Self::default();
            for dir in dirs {
                world.mkdir(dir);
            }
            world
        }

        fn mkdir(&mut self, path: &str) {
            let path = PathBuf::from(path);
            self.next_id += 1;
            self.dirs.insert(path.clone(), self.next_id);
            self.entry_changed(&path);
        }

        fn entry_changed(&mut self, path: &Path) {
            if let (Some(parent), Some(name)) = (path.parent(), path.file_name())
                && let Some(id) = self.dirs.get(parent)
                && self.watches.contains_key(id)
            {
                self.queue
                    .push_back(FakeRaw::Entry(*id, name.to_os_string()));
            }
        }

        /// Remove `path` and everything below it.
        fn remove(&mut self, path: &str) {
            let path = PathBuf::from(path);
            let gone: Vec<PathBuf> = self
                .dirs
                .keys()
                .filter(|dir| dir.starts_with(&path))
                .cloned()
                .collect();
            for dir in gone.iter().rev() {
                let id = self.dirs.remove(dir).unwrap();
                if self.watches.remove(&id).is_some() {
                    self.queue.push_back(FakeRaw::SelfLeft(id));
                }
            }
            self.entry_changed(&path);
        }

        /// Move `from` (and its subtree) to `to`.
        fn rename(&mut self, from: &str, to: &str) {
            let (from, to) = (PathBuf::from(from), PathBuf::from(to));
            let moved: Vec<(PathBuf, u64)> = self
                .dirs
                .iter()
                .filter(|(dir, _)| dir.starts_with(&from))
                .map(|(dir, id)| (dir.clone(), *id))
                .collect();
            for (dir, id) in &moved {
                self.dirs.remove(dir);
                let rest = dir.strip_prefix(&from).unwrap();
                self.dirs.insert(to.join(rest), *id);
            }
            let top = self.dirs[&to];
            if self.watches.contains_key(&top) {
                self.queue.push_back(FakeRaw::SelfLeft(top));
            }
            self.entry_changed(&from);
            self.entry_changed(&to);
        }
    }

    #[derive(Debug)]
    struct FakeRegistration {
        refused: Vec<PathBuf>,
        anchor: Option<u64>,
        root: Option<u64>,
        above: Vec<u64>,
        ids: (Option<u64>, Option<u64>),
    }

    struct FakeBackend(Arc<Mutex<World>>);

    impl Backend for FakeBackend {
        type Registration = FakeRegistration;
        type Raw = FakeRaw;
        const FOLD_CASE: bool = false;

        fn register(&mut self, root: &Path) -> (Plan, FakeRegistration) {
            let mut world = self.0.lock().unwrap();
            world.registrations += 1;
            let mut anchor = None;
            let mut refused = Vec::new();
            let plan = Plan::resolve(
                root,
                |candidate| {
                    if world.refused.iter().any(|dir| dir == candidate) {
                        refused.push(candidate.to_path_buf());
                        return false;
                    }
                    anchor = world.dirs.get(candidate).copied();
                    anchor.is_some()
                },
                world.dirs.contains_key(root),
            );
            let root_id = world.dirs.get(root).copied();
            let above: Vec<u64> = plan
                .above
                .iter()
                .filter_map(|dir| world.dirs.get(dir).copied())
                .collect();
            for id in anchor.iter().chain(root_id.iter()).chain(above.iter()) {
                *world.watches.entry(*id).or_default() += 1;
            }
            let registration = FakeRegistration {
                refused,
                anchor,
                root: root_id,
                above,
                ids: (anchor, root_id),
            };
            if let Some(change) = world.after_register.take() {
                change(&mut world);
            }
            if world.flips_left > 0 {
                world.flips_left -= 1;
                let parent = PathBuf::from("/a/p");
                if world.dirs.remove(&parent).is_none() {
                    world.next_id += 1;
                    let id = world.next_id;
                    world.dirs.insert(parent, id);
                }
            }
            (plan, registration)
        }

        fn current(&self, root: &Path, plan: &Plan, registration: &FakeRegistration) -> bool {
            let world = self.0.lock().unwrap();
            let usable = |candidate: &Path| {
                world.dirs.contains_key(candidate)
                    && !registration.refused.iter().any(|dir| dir == candidate)
            };
            let now = Plan::resolve(root, usable, world.dirs.contains_key(root));
            now == *plan
                && registration.ids
                    == (
                        plan.anchor
                            .as_ref()
                            .and_then(|dir| world.dirs.get(dir).copied()),
                        world.dirs.get(root).copied(),
                    )
        }

        fn retire(&mut self, old: FakeRegistration, _new: &FakeRegistration) {
            let mut world = self.0.lock().unwrap();
            for id in old
                .anchor
                .iter()
                .chain(old.root.iter())
                .chain(old.above.iter())
            {
                if let Some(count) = world.watches.get_mut(id) {
                    *count -= 1;
                    if *count == 0 {
                        world.watches.remove(id);
                    }
                }
            }
        }

        fn notices(&mut self, block: bool) -> Option<Vec<FakeRaw>> {
            let mut world = self.0.lock().unwrap();
            if world.stopped {
                return None;
            }
            if block && world.queue.is_empty() {
                // The test drives every wake; an empty blocking read ends the script.
                return None;
            }
            Some(world.queue.drain(..).collect())
        }

        fn classify(&self, raw: &FakeRaw, registration: &FakeRegistration) -> Option<Wake> {
            match raw {
                FakeRaw::Overflow => Some(Wake::Overflow),
                FakeRaw::SelfLeft(id) if registration.root == Some(*id) => Some(Wake::RootLeft),
                FakeRaw::SelfLeft(id) if registration.anchor == Some(*id) => Some(Wake::AnchorLeft),
                FakeRaw::SelfLeft(id) if registration.above.contains(id) => Some(Wake::AboveLeft),
                FakeRaw::Entry(id, name) if registration.anchor == Some(*id) => {
                    Some(Wake::AnchorEntry(name.clone()))
                }
                _ => None,
            }
        }

        fn identity(&self, path: &Path) -> Option<Identity> {
            self.0.lock().unwrap().dirs.get(path).map(|id| (0, *id))
        }
    }

    const ROOT: &str = "/a/p/store";

    fn world() -> Arc<Mutex<World>> {
        Arc::new(Mutex::new(World::with(&[
            "/",
            "/a",
            "/a/p",
            ROOT,
            "/a/p/sibling",
        ])))
    }

    /// Run the watch over the scripted `steps` (each applied, then its notices taken as
    /// one wake), returning how many times the handler would run after each step.
    fn script(world: &Arc<Mutex<World>>, steps: Vec<Change>) -> Vec<usize> {
        let mut backend = FakeBackend(world.clone());
        let root = Path::new(ROOT);
        let mut watching = Watching::start(&mut backend, root).unwrap();
        assert!(
            !watching.look(&backend, root),
            "nothing changed at the start"
        );
        let mut reports = Vec::new();
        for change in steps {
            change(&mut world.lock().unwrap());
            let raws = backend.notices(false).unwrap();
            watching = watching.wake(&mut backend, root, &raws).unwrap();
            reports.push(usize::from(watching.look(&backend, root)));
        }
        reports
    }

    fn steps(list: Vec<fn(&mut World)>) -> Vec<Change> {
        list.into_iter()
            .map(|change| Box::new(change) as Change)
            .collect()
    }

    #[test]
    fn hs2_crxwr9_every_transition_of_the_root_and_its_ancestors_is_reported() {
        let world = world();
        let reports = script(
            &world,
            steps(vec![
                // Churn beside the root: nothing.
                |w| w.mkdir("/a/p/other"),
                |w| w.remove("/a/p/other"),
                // Root moved away, then back.
                |w| w.rename(ROOT, "/a/p/away"),
                |w| w.rename("/a/p/away", ROOT),
                // Root removed, recreated.
                |w| w.remove(ROOT),
                |w| w.mkdir(ROOT),
                // Parent removed (the parent's own watch ends), then recreated, then a store
                // created inside the new parent.
                |w| w.remove("/a/p"),
                |w| w.mkdir("/a/p"),
                |w| w.mkdir(ROOT),
                // Parent moved away and back.
                |w| w.rename("/a/p", "/a/q"),
                |w| w.rename("/a/q", "/a/p"),
                // Grandparent removed, recreated with the whole chain at once.
                |w| w.remove("/a"),
                |w| {
                    w.mkdir("/a");
                    w.mkdir("/a/p");
                    w.mkdir(ROOT);
                },
                // Grandparent moved away and back.
                |w| w.rename("/a", "/b"),
                |w| w.rename("/b", "/a"),
                // Overflow: re-checked and reported conservatively.
                |w| w.queue.push_back(FakeRaw::Overflow),
                // Churn again after all that: still nothing.
                |w| w.mkdir("/a/p/late-sibling"),
            ]),
        );
        assert_eq!(
            reports,
            [0, 0, 1, 1, 1, 1, 1, 0, 1, 1, 1, 1, 1, 1, 1, 1, 0],
            "reports per step"
        );
        // Nothing is left watched but the final plan: the anchor, the root and above.
        let world = world.lock().unwrap();
        assert_eq!(world.watches.len(), 4, "{:?}", world.watches);
        assert!(world.watches.values().all(|count| *count == 1));
    }

    #[test]
    fn hs2_crxwr9_rapid_flips_between_two_looks_are_each_reported() {
        let world = world();
        let flips: Vec<fn(&mut World)> = vec![
            |w| {
                w.rename(ROOT, "/a/p/away");
                w.rename("/a/p/away", ROOT);
            },
            |w| {
                w.rename("/a/p", "/a/q");
                w.rename("/a/q", "/a/p");
            },
            |w| {
                w.remove("/a/p");
                w.mkdir("/a/p");
                w.mkdir(ROOT);
            },
            |w| {
                w.rename("/a", "/b");
                w.rename("/b", "/a");
            },
        ];
        let mut all = Vec::new();
        for _ in 0..5 {
            all.extend(flips.iter().copied());
        }
        let reports = script(&world, steps(all));
        assert!(reports.iter().all(|count| *count == 1), "{reports:?}");
    }

    #[test]
    fn hs2_crxwr9_a_change_under_a_new_registration_registers_again() {
        // The parent is recreated (with the store) right after a registration resolved it
        // missing: no notice reaches the grandparent watch for it, so only the stale check
        // catches it, and the next registration anchors on the parent again.
        let world = world();
        world.lock().unwrap().remove("/a/p");
        world.lock().unwrap().queue.clear();
        let mut backend = FakeBackend(world.clone());
        let root = Path::new(ROOT);
        world.lock().unwrap().after_register = Some(Box::new(|w: &mut World| {
            w.dirs.insert(PathBuf::from("/a/p"), 100);
            w.dirs.insert(PathBuf::from(ROOT), 101);
        }));
        let ((plan, _), moved) = reregister(&mut backend, root, None).unwrap();
        assert!(moved);
        assert_eq!(plan.anchor.as_deref(), Some(Path::new("/a/p")));
        assert!(plan.root_present);
        assert_eq!(world.lock().unwrap().registrations, 2);
    }

    #[test]
    fn hs2_crxwr9_a_tree_that_keeps_changing_stops_registering_after_a_bound() {
        let world = world();
        let mut backend = FakeBackend(world.clone());
        // Every registration finds a new notice queued: the loop gives up after the bound
        // and reports the root as possibly moved.
        struct Churning(FakeBackend);
        impl Backend for Churning {
            type Registration = FakeRegistration;
            type Raw = FakeRaw;
            const FOLD_CASE: bool = false;
            fn register(&mut self, root: &Path) -> (Plan, FakeRegistration) {
                let held = self.0.register(root);
                self.0.0.lock().unwrap().queue.push_back(FakeRaw::Overflow);
                held
            }
            fn current(&self, root: &Path, plan: &Plan, registration: &FakeRegistration) -> bool {
                self.0.current(root, plan, registration)
            }
            fn retire(&mut self, old: FakeRegistration, new: &FakeRegistration) {
                self.0.retire(old, new);
            }
            fn notices(&mut self, block: bool) -> Option<Vec<FakeRaw>> {
                self.0.notices(block)
            }
            fn classify(&self, raw: &FakeRaw, registration: &FakeRegistration) -> Option<Wake> {
                self.0.classify(raw, registration)
            }
            fn identity(&self, path: &Path) -> Option<Identity> {
                self.0.identity(path)
            }
        }
        let _ = &mut backend;
        let mut churning = Churning(backend);
        let (_, moved) = reregister(&mut churning, Path::new(ROOT), None).unwrap();
        assert!(moved);
        assert_eq!(
            world.lock().unwrap().registrations,
            MAX_STALE_RETRIES as usize
        );
        // Every registration but the last was retired.
        assert!(
            world
                .lock()
                .unwrap()
                .watches
                .values()
                .all(|count| *count == 1)
        );
    }

    /// HS2-CRXWR9 (found on Linux): a registration that keeps going stale is never kept,
    /// however many times in a row; it ends on the tree as it settled. Keeping the ninth
    /// one lost the parent's recreation when its notice was taken by that registration.
    #[test]
    fn hs2_crxwr9_a_stale_registration_is_never_kept_while_the_tree_settles() {
        for flips in [1, 7, 8, 9, 30] {
            let world = world();
            world.lock().unwrap().flips_left = flips;
            let mut backend = FakeBackend(world.clone());
            let root = Path::new(ROOT);
            let ((plan, registration), moved) = reregister(&mut backend, root, None).unwrap();
            assert!(moved, "flips={flips}");
            assert!(
                backend.current(root, &plan, &registration),
                "flips={flips}: kept a stale registration"
            );
            assert_eq!(world.lock().unwrap().registrations, flips + 1);
            let parent_present = world.lock().unwrap().dirs.contains_key(Path::new("/a/p"));
            assert_eq!(plan.anchor_is_parent, parent_present, "flips={flips}");
        }
    }

    /// HS2-CRXWR9: a directory that exists but cannot be registered is skipped for the next
    /// ancestor, and that registration counts as current, so the watch does not register
    /// again and again; once it can be registered (its removal reported above) the parent
    /// anchors again.
    #[test]
    fn hs2_crxwr9_a_refused_parent_is_skipped_without_registering_forever() {
        let world = world();
        world.lock().unwrap().refused.push(PathBuf::from("/a/p"));
        let mut backend = FakeBackend(world.clone());
        let root = Path::new(ROOT);
        let watching = Watching::start(&mut backend, root).unwrap();
        assert_eq!(world.lock().unwrap().registrations, 1);
        assert_eq!(watching.held.0.anchor.as_deref(), Some(Path::new("/a")));
        assert_eq!(watching.held.0.next, Some(os("p")));
        // The refused parent is replaced by one that can be registered.
        {
            let mut world = world.lock().unwrap();
            world.refused.clear();
            world.remove("/a/p");
            world.mkdir("/a/p");
            world.mkdir(ROOT);
        }
        let raws = backend.notices(false).unwrap();
        let mut watching = watching.wake(&mut backend, root, &raws).unwrap();
        assert!(watching.look(&backend, root));
        assert!(watching.held.0.anchor_is_parent);
        assert!(watching.held.0.root_present);
    }

    #[test]
    fn hs2_crxwr9_a_stop_ends_the_loop_and_the_thread() {
        let world = world();
        let (calls_tx, calls) = mpsc::channel();
        let stopping = world.clone();
        // A backend whose blocking read waits for the stop flag.
        struct Waiting(FakeBackend);
        impl Backend for Waiting {
            type Registration = FakeRegistration;
            type Raw = FakeRaw;
            const FOLD_CASE: bool = false;
            fn register(&mut self, root: &Path) -> (Plan, FakeRegistration) {
                self.0.register(root)
            }
            fn current(&self, root: &Path, plan: &Plan, registration: &FakeRegistration) -> bool {
                self.0.current(root, plan, registration)
            }
            fn retire(&mut self, old: FakeRegistration, new: &FakeRegistration) {
                self.0.retire(old, new);
            }
            fn notices(&mut self, block: bool) -> Option<Vec<FakeRaw>> {
                loop {
                    {
                        let mut world = self.0.0.lock().unwrap();
                        if world.stopped {
                            return None;
                        }
                        if !block || !world.queue.is_empty() {
                            return Some(world.queue.drain(..).collect());
                        }
                    }
                    std::thread::yield_now();
                }
            }
            fn classify(&self, raw: &FakeRaw, registration: &FakeRegistration) -> Option<Wake> {
                self.0.classify(raw, registration)
            }
            fn identity(&self, path: &Path) -> Option<Identity> {
                self.0.identity(path)
            }
        }
        let watch = ThreadedRootWatch::start(
            Waiting(FakeBackend(world.clone())),
            Box::new(move || {
                stopping.lock().unwrap().stopped = true;
                true
            }),
            Path::new(ROOT),
            Box::new(move || {
                let _ = calls_tx.send(());
            }),
        )
        .unwrap();
        world.lock().unwrap().rename(ROOT, "/a/p/away");
        assert!(calls.recv_timeout(Duration::from_secs(10)).is_ok());
        drop(watch);
        // The thread has returned: once the calls it made before are taken, its handler
        // (and sender) are gone.
        loop {
            match calls.recv_timeout(Duration::from_secs(10)) {
                Ok(()) => {}
                Err(error) => {
                    assert_eq!(error, mpsc::RecvTimeoutError::Disconnected);
                    break;
                }
            }
        }
    }
}
