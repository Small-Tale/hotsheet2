//! HS2-AZFCB6: notice a directory's path being removed, recreated, replaced or moved,
//! without watching anything beneath its parent.
//!
//! A store watcher has to see its store directory vanish and come back (HS2-R6FZ54). It
//! used to register the parent with its native watcher. On macOS that watcher is one
//! FSEvents stream, and an FSEvents stream is always recursive: non-recursive paths are
//! filtered only in the callback, after `fseventsd` has routed every event beneath the
//! parent to the process. With stores beside their projects, that was every write under a
//! whole projects folder.
//!
//! FSEvents' own answer, `kFSEventStreamCreateFlagWatchRoot`, was measured first and
//! dropped root-changed notices: about one in six transitions when the path moved away
//! and back, sometimes for several transitions in a row. So this watch uses kqueue vnode
//! filters instead, which the kernel raises on the directory objects themselves:
//!
//! - the watched directory: deleted, renamed (also when it returns before anyone looks)
//!   or revoked;
//! - its parent's entries (`NOTE_WRITE` on a directory fires when an entry is added,
//!   removed or renamed, never for changes deeper down), and the parent being deleted or
//!   renamed. While the parent is missing, the nearest existing ancestor's entries.
//!
//! After every wake the path is resolved again and the registrations are renewed while the
//! old ones stay in place, collecting any event raised in between, so a move away and
//! straight back is never lost between two registrations. The handler runs only when the
//! object at the path changed (present or not, device and inode) or the directory or its
//! parent was renamed or removed, so entry churn in the parent costs a re-registration and
//! a `stat`, and delivers nothing.

use std::ffi::CString;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

/// The `EVFILT_USER` identifier that asks the watch thread to stop.
const STOP: usize = 0;

/// A running watch; dropping it stops its thread.
pub(crate) struct KqueueRootWatch {
    kq: libc::c_int,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl std::fmt::Debug for KqueueRootWatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KqueueRootWatch").finish_non_exhaustive()
    }
}

/// How many registrations in a row may find the paths changed under them before the
/// watch waits for an event anyway.
const MAX_STALE_RETRIES: u32 = 8;

/// HS2-AZFCB6 measurement: how often any root watch in this test process woke.
#[cfg(test)]
static WAKES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// The object at `path` (device and inode), if one is there.
fn identity(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    let metadata = std::fs::metadata(path).ok()?;
    Some((metadata.dev(), metadata.ino()))
}

/// The nearest existing directory at or above `path`.
fn nearest_existing(path: &Path) -> Option<PathBuf> {
    path.ancestors()
        .find(|dir| dir.is_dir())
        .map(Path::to_path_buf)
}

/// An event-only descriptor for `path` (it does not keep a volume from unmounting).
fn open_evtonly(path: &Path) -> Option<libc::c_int> {
    let path = CString::new(path.as_os_str().as_bytes()).ok()?;
    // SAFETY: `path` is a NUL-terminated string that outlives the call.
    let fd = unsafe { libc::open(path.as_ptr(), libc::O_EVTONLY | libc::O_CLOEXEC) };
    (fd >= 0).then_some(fd)
}

fn change(ident: usize, filter: i16, flags: u16, fflags: u32) -> libc::kevent {
    libc::kevent {
        ident,
        filter,
        flags,
        fflags,
        data: 0,
        udata: std::ptr::null_mut(),
    }
}

/// Apply `changes` to `kq` without collecting events.
fn register(kq: libc::c_int, changes: &[libc::kevent]) -> bool {
    // SAFETY: `changes` is a valid slice for its length and no event list is passed.
    let result = unsafe {
        libc::kevent(
            kq,
            changes.as_ptr(),
            changes.len() as libc::c_int,
            std::ptr::null_mut(),
            0,
            std::ptr::null(),
        )
    };
    result >= 0
}

/// One directory registered with the queue: its descriptor and the object it holds.
struct Held {
    fd: libc::c_int,
    identity: (u64, u64),
}

impl Held {
    /// Register `path` for `fflags`; `None` when it cannot be opened or registered.
    fn open(kq: libc::c_int, path: &Path, fflags: u32) -> Option<Self> {
        let fd = open_evtonly(path)?;
        // SAFETY: `stat` is plain data, filled by `fstat` on the descriptor opened above.
        let mut stat: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: `fd` is open and `stat` is a valid destination.
        let held = unsafe { libc::fstat(fd, &mut stat) } == 0
            && register(
                kq,
                &[change(
                    fd as usize,
                    libc::EVFILT_VNODE,
                    libc::EV_ADD | libc::EV_CLEAR,
                    fflags,
                )],
            );
        if !held {
            // SAFETY: `fd` was opened above and is not registered or shared.
            unsafe { libc::close(fd) };
            return None;
        }
        Some(Self {
            fd,
            identity: (stat.st_dev as u64, stat.st_ino),
        })
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        // SAFETY: the descriptor was opened by `Held::open` and is closed once; closing it
        // removes its filter from the queue.
        unsafe { libc::close(self.fd) };
    }
}

/// What one registration watches: the parent's entries (or, while the parent is missing,
/// the nearest existing ancestor's) and the directory itself when it exists.
struct Registration {
    anchor: Option<(PathBuf, Held)>,
    root: Option<Held>,
}

impl Registration {
    fn open(kq: libc::c_int, root: &Path) -> Self {
        let anchor = root.parent().and_then(nearest_existing).and_then(|path| {
            let fflags =
                libc::NOTE_WRITE | libc::NOTE_DELETE | libc::NOTE_RENAME | libc::NOTE_REVOKE;
            Held::open(kq, &path, fflags).map(|held| (path, held))
        });
        let root = Held::open(
            kq,
            root,
            libc::NOTE_DELETE | libc::NOTE_RENAME | libc::NOTE_REVOKE,
        );
        Self { anchor, root }
    }

    /// Whether the registered objects are still the ones at their paths. A change that
    /// lands between resolving a path and registering it raises no event, so a stale
    /// registration is made again at once.
    fn current(&self, root: &Path) -> bool {
        let anchor = root.parent().and_then(nearest_existing);
        let anchor_current = match (&self.anchor, anchor) {
            (Some((path, held)), Some(now)) => {
                *path == now && identity(path) == Some(held.identity)
            }
            (None, None) => true,
            _ => false,
        };
        let root_current = match &self.root {
            Some(held) => identity(root) == Some(held.identity),
            None => !root.exists(),
        };
        anchor_current && root_current
    }

    /// Whether `event` means the directory left its path, even if it is back by now: the
    /// directory itself or the ancestor holding its path was renamed, removed or revoked.
    /// New entries in the parent (`NOTE_WRITE`) are not: the path is compared instead.
    fn moved_the_root(&self, event: &libc::kevent) -> bool {
        if event.filter != libc::EVFILT_VNODE {
            return false;
        }
        let is = |held: &Held| event.ident == held.fd as usize;
        let moved = event.fflags & (libc::NOTE_DELETE | libc::NOTE_RENAME | libc::NOTE_REVOKE);
        self.root.as_ref().is_some_and(is)
            || (moved != 0 && self.anchor.as_ref().is_some_and(|(_, held)| is(held)))
    }
}

/// Collect pending events: blocking until at least one arrives, or only those already
/// queued. `None` when the queue failed or the watch is asked to stop.
fn next_events(kq: libc::c_int, block: bool) -> Option<Vec<libc::kevent>> {
    let zero = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    loop {
        let mut events = [change(0, 0, 0, 0); 8];
        // SAFETY: `events` is a valid buffer of its length; a null timeout blocks until an
        // event or the stop trigger arrives, a zero one returns at once.
        let count = unsafe {
            libc::kevent(
                kq,
                std::ptr::null(),
                0,
                events.as_mut_ptr(),
                events.len() as libc::c_int,
                if block { std::ptr::null() } else { &zero },
            )
        };
        if count < 0 {
            if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return None;
        }
        let events = events[..count as usize].to_vec();
        if events.iter().any(|event| event.filter == libc::EVFILT_USER) {
            return None;
        }
        return Some(events);
    }
}

/// Register `root` again while `held` stays registered, so no change goes unseen in
/// between: events raised meanwhile, on either registration, are collected before the old
/// one is closed. Repeats (a bounded number of times) while events keep arriving or the
/// paths moved under the new registration. Returns the new registration and whether the
/// directory left its path meanwhile; `None` when the watch should stop.
fn reregister(
    kq: libc::c_int,
    root: &Path,
    mut held: Option<Registration>,
) -> Option<(Registration, bool)> {
    let mut moved = false;
    let mut attempts = 0;
    loop {
        let next = Registration::open(kq, root);
        let raised = next_events(kq, false)?;
        moved |= raised.iter().any(|event| {
            next.moved_the_root(event)
                || held.as_ref().is_some_and(|held| held.moved_the_root(event))
        });
        let current = next.current(root);
        // Only a registration made while something moved can be stale.
        moved |= !current;
        held = Some(next);
        attempts += 1;
        if (raised.is_empty() && current) || attempts >= MAX_STALE_RETRIES {
            return held.map(|registration| (registration, moved));
        }
    }
}

/// The watch thread: register, wait, and report each change of the directory at `root`.
fn run(
    kq: libc::c_int,
    root: &Path,
    handler: &mut (dyn FnMut() + Send),
    ready: std::sync::mpsc::Sender<()>,
) {
    let mut last = identity(root);
    let Some((mut registration, mut moved)) = reregister(kq, root, None) else {
        return;
    };
    let _ = ready.send(());
    loop {
        let now = identity(root);
        if moved || now != last {
            last = now;
            handler();
        }
        let Some(events) = next_events(kq, true) else {
            return;
        };
        #[cfg(test)]
        WAKES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let woke_moved = events
            .iter()
            .any(|event| registration.moved_the_root(event));
        let Some((next, reregistered_moved)) = reregister(kq, root, Some(registration)) else {
            return;
        };
        registration = next;
        moved = woke_moved || reregistered_moved;
    }
}

impl KqueueRootWatch {
    /// Watch `root`'s path and run `handler` whenever the directory there changed. The
    /// path need not exist; a directory created there later is reported. Returns once the
    /// first registration is in place.
    pub(crate) fn start(
        root: &Path,
        mut handler: Box<dyn FnMut() + Send>,
    ) -> std::io::Result<Self> {
        // SAFETY: plain system call; the descriptor is owned by the returned watch.
        let kq = unsafe { libc::kqueue() };
        if kq < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let stop = change(STOP, libc::EVFILT_USER, libc::EV_ADD | libc::EV_CLEAR, 0);
        if !register(kq, &[stop]) {
            let error = std::io::Error::last_os_error();
            // SAFETY: `kq` was created above and is not shared yet.
            unsafe { libc::close(kq) };
            return Err(error);
        }
        let root = root.to_path_buf();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("hotsheet-root-watch".into())
            .spawn(move || run(kq, &root, &mut *handler, ready_tx));
        let thread = match spawned {
            Ok(thread) => thread,
            Err(error) => {
                // SAFETY: the thread never started, so nothing else uses `kq`.
                unsafe { libc::close(kq) };
                return Err(error);
            }
        };
        let _ = ready_rx.recv();
        Ok(Self {
            kq,
            thread: Some(thread),
        })
    }
}

impl Drop for KqueueRootWatch {
    fn drop(&mut self) {
        let trigger = change(STOP, libc::EVFILT_USER, 0, libc::NOTE_TRIGGER);
        if !register(self.kq, &[trigger]) {
            // The thread may still wait on the queue: leave both rather than close a
            // descriptor it could be using.
            return;
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        // SAFETY: the thread has returned, so nothing else uses the queue.
        unsafe { libc::close(self.kq) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc::{self, Receiver};
    use std::time::Duration;

    struct Watched {
        _watch: KqueueRootWatch,
        calls: Receiver<()>,
        count: Arc<AtomicUsize>,
    }

    fn watch(root: &Path) -> Watched {
        let (sender, calls) = mpsc::channel();
        let count = Arc::new(AtomicUsize::new(0));
        let counted = count.clone();
        let watch = KqueueRootWatch::start(
            root,
            Box::new(move || {
                counted.fetch_add(1, Ordering::SeqCst);
                let _ = sender.send(());
            }),
        )
        .unwrap();
        Watched {
            _watch: watch,
            calls,
            count,
        }
    }

    impl Watched {
        /// Wait for the handler to run after `what`, then drop calls already queued.
        fn reports(&self, what: &str) {
            assert!(
                self.calls.recv_timeout(Duration::from_secs(10)).is_ok(),
                "no notice after {what}"
            );
            while self.calls.try_recv().is_ok() {}
        }
    }

    fn layout() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        let parent = hotsheet_sync::paths::canonicalize(temp.path())
            .unwrap()
            .join("projects");
        let store = parent.join("store.hs2");
        fs::create_dir_all(store.join("tickets")).unwrap();
        (temp, parent, store)
    }

    /// HS2-AZFCB6: thousands of writes in a sibling directory, entries created and removed
    /// directly in the parent, and writes inside the store never run the handler; the
    /// store's own move after them does, at most twice (its rename and the parent entry).
    #[test]
    fn hs2_azfcb6_churn_beside_or_inside_the_root_delivers_nothing() {
        let (_temp, parent, store) = layout();
        let sibling = parent.join("other-project/target/debug");
        fs::create_dir_all(&sibling).unwrap();
        let watched = watch(&store);
        for index in 0..3000 {
            fs::write(sibling.join(format!("out-{index}")), b"x").unwrap();
        }
        for index in 0..200 {
            let entry = parent.join(format!("entry-{index}"));
            fs::create_dir(&entry).unwrap();
            fs::remove_dir(&entry).unwrap();
        }
        for index in 0..200 {
            fs::write(store.join("tickets").join(format!("t-{index}.md")), b"x").unwrap();
        }
        fs::rename(&store, parent.join("away")).unwrap();
        watched.reports("the move after the churn");
        let count = watched.count.load(Ordering::SeqCst);
        assert!((1..=2).contains(&count), "{count} handler runs");
    }

    /// HS2-AZFCB6 (HS2-R6FZ54 transitions): every change of the directory at the path is
    /// reported, including a move away and straight back that FSEvents' root watch missed.
    #[test]
    fn hs2_azfcb6_every_transition_of_the_root_is_reported() {
        let (temp, parent, store) = layout();
        let watched = watch(&store);
        let away = parent.join("store-away");

        fs::rename(&store, &away).unwrap();
        watched.reports("moved away");
        fs::rename(&away, &store).unwrap();
        watched.reports("moved back");

        fs::remove_dir_all(&store).unwrap();
        watched.reports("removed");
        fs::create_dir_all(store.join("tickets")).unwrap();
        watched.reports("recreated");

        // Replaced in one step: another directory renamed over the path.
        let replacement = parent.join("replacement");
        fs::create_dir_all(replacement.join("tickets")).unwrap();
        fs::remove_dir_all(&store).unwrap();
        fs::rename(&replacement, &store).unwrap();
        watched.reports("replaced");

        // Renamed (same directory, new name), then a new one created at the path.
        fs::rename(&store, parent.join("store-renamed")).unwrap();
        watched.reports("renamed");
        fs::create_dir(&store).unwrap();
        watched.reports("created after the rename");

        // The parent moved away and back, then removed and recreated.
        let moved_parent = temp.path().join("projects-moved");
        fs::rename(&parent, &moved_parent).unwrap();
        watched.reports("parent moved away");
        fs::rename(&moved_parent, &parent).unwrap();
        watched.reports("parent moved back");
        fs::remove_dir_all(&parent).unwrap();
        watched.reports("parent removed");
        fs::create_dir_all(&parent).unwrap();
        fs::create_dir(&store).unwrap();
        watched.reports("parent and root recreated");

        // Moves away and straight back, twenty times: each one is reported.
        for round in 0..20 {
            fs::rename(&store, &away).unwrap();
            fs::rename(&away, &store).unwrap();
            watched.reports(&format!("flip {round}"));
            fs::rename(&parent, &moved_parent).unwrap();
            fs::rename(&moved_parent, &parent).unwrap();
            watched.reports(&format!("parent flip {round}"));
        }
    }

    /// HS2-AZFCB6: a watch can start before its directory or parent exists, and dropping it
    /// stops it.
    #[test]
    fn hs2_azfcb6_a_missing_root_is_reported_once_created_and_drop_stops() {
        let temp = tempfile::tempdir().unwrap();
        let base = hotsheet_sync::paths::canonicalize(temp.path()).unwrap();
        let store = base.join("later/store.hs2");
        let watched = watch(&store);
        fs::create_dir(base.join("later")).unwrap();
        fs::create_dir(&store).unwrap();
        watched.reports("created with its parent");
        let Watched { _watch, calls, .. } = watched;
        drop(_watch);
        fs::remove_dir(&store).unwrap();
        assert!(calls.recv_timeout(Duration::from_millis(300)).is_err());
    }

    /// This process's CPU time so far (user and system).
    fn process_cpu() -> Duration {
        // SAFETY: `usage` is plain data that `getrusage` fills for this process.
        let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
        // SAFETY: a valid destination for the calling process's usage.
        unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
        let time = |value: libc::timeval| {
            Duration::from_secs(value.tv_sec as u64) + Duration::from_micros(value.tv_usec as u64)
        };
        time(usage.ru_utime) + time(usage.ru_stime)
    }

    /// `fseventsd`'s CPU time so far, from `ps` (system-wide service: noisy).
    fn fseventsd_cpu() -> Option<Duration> {
        let output = std::process::Command::new("ps")
            .args(["-A", "-o", "time=,comm="])
            .output()
            .ok()?;
        let listing = String::from_utf8_lossy(&output.stdout).into_owned();
        let line = listing
            .lines()
            .find(|line| line.trim_end().ends_with("/fseventsd"))?;
        let time = line.split_whitespace().next()?;
        // `[[dd-]hh:]mm:ss.ss`
        let seconds = time
            .rsplit(':')
            .zip([1.0, 60.0, 3600.0])
            .map(|(part, scale)| part.parse::<f64>().unwrap_or(0.0) * scale)
            .sum::<f64>();
        Some(Duration::from_secs_f64(seconds))
    }

    /// Wait for an event naming `marker` from a stream's handler.
    fn wait_for_marker(events: &Receiver<crate::watch::Delivery>, marker: &Path) {
        let deadline = std::time::Instant::now() + Duration::from_secs(120);
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            match events.recv_timeout(remaining) {
                Ok(Ok(event)) if event.paths.iter().any(|path| path == marker) => return,
                Ok(_) => {}
                Err(_) => panic!("no event for {}", marker.display()),
            }
        }
    }

    /// HS2-AZFCB6 measurement: the FSEvents records routed to the process (before any
    /// callback filtering) and the process CPU while a sibling project of the store takes
    /// `HS2_AZFCB6_FILES` writes, with the store's stream registered as before (the parent
    /// without its subtree, the root, `tickets/`) against the root without the parent plus
    /// this root watch. Run explicitly, unsandboxed:
    /// `HS2_AZFCB6_FILES=20000 cargo nextest run -p hotsheet-jobs --lib --run-ignored only
    /// -E 'test(/hs2_azfcb6_measure/)' --no-capture`.
    #[test]
    #[ignore = "measurement harness for HS2-AZFCB6; prints before/after numbers"]
    fn hs2_azfcb6_measure_records_routed_with_and_without_the_parent_stream() {
        use crate::fsevent_stream::{ExcludingStream, RECORDS_RECEIVED};
        use crate::watch::RecursiveMode;
        let files: usize = std::env::var("HS2_AZFCB6_FILES")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(20_000);
        for round in 0..3 {
            for parent_stream in [true, false] {
                let (_temp, parent, store) = layout();
                let tickets = store.join("tickets");
                let sibling = parent.join("other-project/target/debug");
                fs::create_dir_all(&sibling).unwrap();
                let mut paths = vec![
                    (store.as_path(), RecursiveMode::NonRecursive),
                    (tickets.as_path(), RecursiveMode::Recursive),
                ];
                if parent_stream {
                    paths.insert(0, (parent.as_path(), RecursiveMode::NonRecursive));
                }
                let (sender, events) = mpsc::channel();
                let _stream = ExcludingStream::start(
                    &paths,
                    &[],
                    Box::new(move |delivery| {
                        let _ = sender.send(delivery);
                    }),
                )
                .unwrap();
                let root_watch = watch(&store);
                if parent_stream {
                    drop(root_watch);
                }
                // Settle: start-up records are not counted.
                let first = tickets.join("first.md");
                fs::write(&first, b"x").unwrap();
                wait_for_marker(&events, &first);
                let records = RECORDS_RECEIVED.load(Ordering::SeqCst);
                let wakes = WAKES.load(Ordering::SeqCst);
                let cpu = process_cpu();
                let service_cpu = fseventsd_cpu();
                for index in 0..files {
                    fs::write(sibling.join(format!("out-{index}.o")), b"x").unwrap();
                }
                let marker = tickets.join("marker.md");
                fs::write(&marker, b"x").unwrap();
                wait_for_marker(&events, &marker);
                eprintln!(
                    "HS2-AZFCB6 round={round} files={files} parent_stream={parent_stream} \
                     fsevents_records={} root_watch_wakes={} process_cpu_ms={} \
                     fseventsd_cpu_ms={:?}",
                    RECORDS_RECEIVED.load(Ordering::SeqCst) - records,
                    WAKES.load(Ordering::SeqCst) - wakes,
                    (process_cpu() - cpu).as_millis(),
                    service_cpu
                        .zip(fseventsd_cpu())
                        .map(|(before, after)| after.saturating_sub(before).as_millis())
                );
            }
        }
    }
}
