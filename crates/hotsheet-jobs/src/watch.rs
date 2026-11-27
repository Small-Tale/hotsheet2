//! One platform watcher start per waiter (HS2-N1QDZW).
//!
//! On macOS a file watcher is one event stream for all of its paths, and starting or
//! stopping a stream is a call into the system file-event service. That call took 60 ms
//! at the median and up to 1.4 s while many waiters started at once. `notify` 6 restarts
//! the stream on every `watch` call, so a waiter over four paths paid four starts and
//! three stops. [`PathWatcher::start`] registers every path first and starts once.
//!
//! Linux and Windows watch each path separately and have no stream to restart, so they
//! keep `notify` 6 and its per-path `watch` call unchanged. The event types below are
//! therefore the ones of the library in use on the platform; both expose the same
//! names, and the macOS backends of the two versions filter and translate events with
//! the same code.
//!
//! HS2-236XY3: [`PathWatcher::start_excluding`] leaves ignored trees out at the source on
//! macOS (FSEvents exclusion paths) and Linux (one inotify watch per directory, none under
//! an excluded one; see `tree_plan.rs` and `inotify_tree.rs`). Windows `ReadDirectoryChangesW`
//! watches a whole tree through one directory handle and has no way to leave a subtree out,
//! so there it is a plain watcher and consumers filter excluded paths themselves.

use std::path::Path;

#[cfg(not(target_os = "macos"))]
use notify as backend;
#[cfg(target_os = "macos")]
use notify_batched as backend;

use backend::Watcher;
pub use backend::{Error, Event, EventKind, RecursiveMode};
#[cfg(any(target_os = "linux", test))]
pub(crate) use backend::{ErrorKind, event};

/// What a watcher hands its handler: an event, or the reason the platform lost some.
pub type Delivery = Result<Event, Error>;

/// HS2-2HZN2C: the most directories one watcher can leave out at the source
/// ([`PathWatcher::start_excluding`]); the limit of macOS `FSEventStreamSetExclusionPaths`.
#[cfg(not(target_os = "linux"))]
pub const MAX_EXCLUDED_PATHS: usize = 8;
/// HS2-236XY3: on Linux an exclusion is a subtree the per-directory watcher never walks,
/// which costs one path comparison per directory, so far more are accepted than macOS takes.
#[cfg(target_os = "linux")]
pub const MAX_EXCLUDED_PATHS: usize = 64;

/// A platform file watcher over a fixed set of paths, started once.
pub struct PathWatcher {
    watcher: Inner,
    starts: usize,
}

#[cfg(target_os = "macos")]
type SharedHandler = std::sync::Arc<std::sync::Mutex<dyn FnMut(Delivery) + Send>>;

enum Inner {
    Notify(backend::RecommendedWatcher),
    /// HS2-2HZN2C: a macOS stream with exclusion paths, which `notify` cannot set.
    #[cfg(target_os = "macos")]
    Excluding {
        stream: crate::fsevent_stream::ExcludingStream,
        paths: Vec<(std::path::PathBuf, RecursiveMode)>,
        excluded: Vec<std::path::PathBuf>,
        handler: SharedHandler,
    },
    /// HS2-236XY3: per-directory inotify watches that leave excluded subtrees out.
    #[cfg(target_os = "linux")]
    Tree(crate::inotify_tree::TreeWatcher),
}

impl std::fmt::Debug for PathWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PathWatcher")
            .field("starts", &self.starts)
            .finish_non_exhaustive()
    }
}

impl PathWatcher {
    /// Watch every path in `paths` and deliver their events to `handler`. Each path
    /// must exist. Events are delivered from the moment this returns; nothing is
    /// delivered for a path while a later one is still being registered.
    ///
    /// HS2-P0EHD4: access events (an open, a read, a close without a write) are never
    /// delivered. notify 8's inotify backend reports every open of a watched file, so a
    /// consumer's own reads would otherwise wake it again; writes still arrive as create,
    /// modify and remove.
    pub fn start<F>(paths: &[(&Path, RecursiveMode)], mut handler: F) -> Result<Self, Error>
    where
        F: FnMut(Delivery) + Send + 'static,
    {
        let mut watcher = backend::recommended_watcher(move |delivery: Delivery| {
            if !is_access(&delivery) {
                handler(delivery);
            }
        })?;
        register(&mut watcher, paths)?;
        Ok(Self {
            watcher: Inner::Notify(watcher),
            starts: 1,
        })
    }

    /// HS2-2HZN2C: [`PathWatcher::start`], except that nothing inside `excluded` (at most
    /// [`MAX_EXCLUDED_PATHS`] directories) is reported. On macOS the file-event service
    /// drops those events itself, so a busy ignored build tree costs neither `fseventsd`'s
    /// routing to this process nor its callback. HS2-236XY3: on Linux each directory outside
    /// the excluded trees gets its own inotify watch, so those trees take no kernel watches
    /// and report nothing; symlinked directories below a root are not followed. An excluded
    /// directory's own creation and removal are still reported by its parent on both.
    /// Elsewhere (Windows) the platform has no such filter and this is
    /// [`PathWatcher::start`]: consumers still filter excluded paths themselves.
    pub fn start_excluding<F>(
        paths: &[(&Path, RecursiveMode)],
        excluded: &[&Path],
        handler: F,
    ) -> Result<Self, Error>
    where
        F: FnMut(Delivery) + Send + 'static,
    {
        if excluded.len() > MAX_EXCLUDED_PATHS {
            return Err(Error::generic(&format!(
                "more than {MAX_EXCLUDED_PATHS} excluded paths"
            )));
        }
        #[cfg(target_os = "linux")]
        if !excluded.is_empty() {
            let tree = crate::inotify_tree::TreeWatcher::start(paths, excluded, Box::new(handler))?;
            return Ok(Self {
                watcher: Inner::Tree(tree),
                starts: 1,
            });
        }
        #[cfg(target_os = "macos")]
        if !excluded.is_empty() {
            let handler: SharedHandler = std::sync::Arc::new(std::sync::Mutex::new(handler));
            let paths: Vec<_> = paths
                .iter()
                .map(|(path, mode)| (path.to_path_buf(), *mode))
                .collect();
            let excluded: Vec<_> = excluded.iter().map(|path| path.to_path_buf()).collect();
            let stream = start_excluding_stream(&paths, &excluded, &handler)?;
            return Ok(Self {
                watcher: Inner::Excluding {
                    stream,
                    paths,
                    excluded,
                    handler,
                },
                starts: 1,
            });
        }
        Self::start(paths, handler)
    }

    /// Watch one more path. On macOS this restarts the event stream once, so pass
    /// every path known at the start to [`PathWatcher::start`] instead.
    pub fn add(&mut self, path: &Path, mode: RecursiveMode) -> Result<(), Error> {
        self.change(&[], &[(path, mode)])
    }

    /// HS2-654VBY: stop watching `removed` and start watching `added` in one change. On
    /// macOS this is one event-stream restart, where a separate unwatch and watch were two.
    /// A path in `removed` that is not watched is ignored, as an unwatch of it would be.
    pub fn change(
        &mut self,
        removed: &[&Path],
        added: &[(&Path, RecursiveMode)],
    ) -> Result<(), Error> {
        if removed.is_empty() && added.is_empty() {
            return Ok(());
        }
        match &mut self.watcher {
            Inner::Notify(watcher) => reregister(watcher, removed, added)?,
            #[cfg(target_os = "macos")]
            Inner::Excluding {
                stream,
                paths,
                excluded,
                handler,
            } => {
                let mut wanted = paths.clone();
                wanted.retain(|(path, _)| !removed.contains(&path.as_path()));
                wanted.extend(added.iter().map(|(path, mode)| (path.to_path_buf(), *mode)));
                // A stream's paths are fixed: stop it, then start one over the new set.
                drop(std::mem::replace(
                    stream,
                    crate::fsevent_stream::ExcludingStream::stopped(),
                ));
                *stream = start_excluding_stream(&wanted, excluded, handler)?;
                *paths = wanted;
            }
            #[cfg(target_os = "linux")]
            Inner::Tree(tree) => tree.change(removed, added)?,
        }
        self.starts += 1;
        Ok(())
    }

    /// How many times this watcher registered paths with the platform: one for
    /// [`PathWatcher::start`] and one for each [`PathWatcher::add`] or
    /// [`PathWatcher::change`]. On macOS each is one event-stream start.
    pub fn starts(&self) -> usize {
        self.starts
    }
}

#[cfg(target_os = "macos")]
fn start_excluding_stream(
    paths: &[(std::path::PathBuf, RecursiveMode)],
    excluded: &[std::path::PathBuf],
    handler: &SharedHandler,
) -> Result<crate::fsevent_stream::ExcludingStream, Error> {
    let refs: Vec<(&Path, RecursiveMode)> = paths
        .iter()
        .map(|(path, mode)| (path.as_path(), *mode))
        .collect();
    let excluded: Vec<&Path> = excluded.iter().map(std::path::PathBuf::as_path).collect();
    let handler = handler.clone();
    crate::fsevent_stream::ExcludingStream::start(
        &refs,
        &excluded,
        Box::new(move |delivery| {
            if !is_access(&delivery) {
                (handler
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner))(delivery);
            }
        }),
    )
}

/// HS2-P0EHD4: whether a delivery only reports that a file was accessed, which no
/// consumer treats as a change.
pub fn is_access(delivery: &Delivery) -> bool {
    matches!(delivery, Ok(event) if matches!(event.kind, EventKind::Access(_)))
}

/// HS2-AZFCB6: a watch on one directory's path that reports when the directory there
/// changes (removed, created, replaced, renamed away or back, its parent moved) without
/// watching anything beneath the parent.
///
/// A consumer that must notice its directory vanish and return used to watch the parent
/// with a [`PathWatcher`]. On macOS that is an FSEvents stream, which is always recursive,
/// so every event beneath the parent reached the process; on a developer machine, a whole
/// projects folder. On macOS this watch is a kqueue vnode watch on the directory and on its
/// parent's entries (see `root_watch.rs`); the handler runs only when the directory object
/// at the path changed, so churn beside it delivers nothing. Elsewhere the native watchers
/// watch a parent without its subtree already, and [`RootWatch::start`] refuses so callers
/// keep doing that.
pub struct RootWatch {
    #[cfg(target_os = "macos")]
    _watch: crate::root_watch::KqueueRootWatch,
}

impl std::fmt::Debug for RootWatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RootWatch").finish_non_exhaustive()
    }
}

impl RootWatch {
    /// Start watching `root`'s path. The path need not exist yet. Returns once the watch
    /// is registered: every change after that runs `handler` (from the watch's own thread),
    /// and several changes before it runs may share one call.
    pub fn start<F>(root: &Path, handler: F) -> Result<Self, Error>
    where
        F: FnMut() + Send + 'static,
    {
        #[cfg(target_os = "macos")]
        {
            let watch = crate::root_watch::KqueueRootWatch::start(root, Box::new(handler))
                .map_err(Error::io)?;
            Ok(Self { _watch: watch })
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (root, handler);
            Err(Error::generic(
                "a root watch is only needed on macOS; watch the parent instead",
            ))
        }
    }

    /// Whether [`RootWatch::start`] works on this platform.
    pub fn supported() -> bool {
        cfg!(target_os = "macos")
    }
}

/// HS2-F6F42W: an event that tells a consumer to read again because some changes may not
/// have been reported, as a platform's own rescan notice does. [`BackgroundWatcher`] sends
/// one when its stream goes live and while the platform refuses to start one.
pub fn rescan_event() -> Event {
    Event::new(EventKind::Any).set_flag(backend::event::Flag::Rescan)
}

/// The first wait before a refused stream start is tried again (HS2-F6F42W).
pub const START_RETRY_FIRST: std::time::Duration = std::time::Duration::from_millis(100);
/// The longest wait between attempts while the platform refuses to start a stream.
pub const START_RETRY_MAX: std::time::Duration = std::time::Duration::from_secs(5);

/// How often an owner reads on its own while its stream is not live yet (HS2-F6F42W).
pub const PENDING_RECHECK: std::time::Duration = std::time::Duration::from_secs(1);

/// What a [`BackgroundWatcher`] reports to its owner, besides the events themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchStatus {
    /// The stream is live over every path asked for so far. Changes made before this were
    /// not reported: read once now.
    Live,
    /// The platform refused to start (or extend) the stream; it is tried again after a
    /// bounded backoff. Read now, since no event will say when something changed.
    Refused,
}

enum Command {
    Change {
        removed: Vec<std::path::PathBuf>,
        added: Vec<(std::path::PathBuf, RecursiveMode)>,
    },
}

/// HS2-F6F42W: a [`PathWatcher`] whose owner never waits on the platform. On a loaded Mac
/// the file-event service delivered events within milliseconds but took up to 7 s to start
/// a stream, up to 19 s to restart one (adding or moving a path restarts it) and up to 46 s
/// to stop one. Here the stream is started, changed and stopped on its own thread, so:
/// - [`BackgroundWatcher::start`], [`BackgroundWatcher::add`], [`BackgroundWatcher::change`]
///   and dropping the watcher return at once; the owner reads what is already there itself;
/// - once a start or change has taken effect, `status` reports [`WatchStatus::Live`], and
///   the owner reads again to cover the changes the stream could not report yet;
/// - a refused start or change is retried with a bounded backoff ([`START_RETRY_FIRST`]
///   doubling to [`START_RETRY_MAX`]), reporting [`WatchStatus::Refused`] at each attempt,
///   until the watcher is dropped. Nothing gives up silently.
pub struct BackgroundWatcher {
    commands: Option<std::sync::mpsc::Sender<Command>>,
    starts: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    live: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl std::fmt::Debug for BackgroundWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackgroundWatcher")
            .field("starts", &self.starts())
            .field("live", &self.is_live())
            .finish_non_exhaustive()
    }
}

/// How a background watcher starts its stream: the platform, or a test's stand-in.
pub type StartStream = Box<
    dyn FnMut(
            &[(&Path, RecursiveMode)],
            Box<dyn FnMut(Delivery) + Send>,
        ) -> Result<PathWatcher, Error>
        + Send,
>;

impl BackgroundWatcher {
    /// Watch `paths` from a background thread, delivering their events to `handler` and
    /// the stream's state to `status`. Returns at once.
    pub fn start<F, S>(paths: &[(&Path, RecursiveMode)], handler: F, status: S) -> Self
    where
        F: FnMut(Delivery) + Send + 'static,
        S: FnMut(WatchStatus) + Send + 'static,
    {
        Self::start_with(paths, handler, status, Box::new(PathWatcher::start))
    }

    /// [`BackgroundWatcher::start`] with the stream started by `start_stream`.
    pub fn start_with<F, S>(
        paths: &[(&Path, RecursiveMode)],
        handler: F,
        mut status: S,
        mut start_stream: StartStream,
    ) -> Self
    where
        F: FnMut(Delivery) + Send + 'static,
        S: FnMut(WatchStatus) + Send + 'static,
    {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        use std::sync::mpsc::RecvTimeoutError;
        use std::sync::{Arc, Mutex, PoisonError};
        let mut wanted: Vec<(std::path::PathBuf, RecursiveMode)> = paths
            .iter()
            .map(|(path, mode)| (path.to_path_buf(), *mode))
            .collect();
        let (commands, received) = std::sync::mpsc::channel::<Command>();
        let starts = Arc::new(AtomicUsize::new(0));
        let live = Arc::new(AtomicBool::new(false));
        let (thread_starts, thread_live) = (starts.clone(), live.clone());
        // One handler for every stream this thread starts, including restarts after a refusal.
        let handler = Arc::new(Mutex::new(handler));
        let forward = move || -> Box<dyn FnMut(Delivery) + Send> {
            let handler = handler.clone();
            Box::new(move |delivery| {
                (handler.lock().unwrap_or_else(PoisonError::into_inner))(delivery);
            })
        };
        std::thread::Builder::new()
            .name("hotsheet-path-watcher".into())
            .spawn(move || {
                let mut watcher: Option<PathWatcher> = None;
                let mut pending: Vec<Command> = Vec::new();
                let mut retry = START_RETRY_FIRST;
                'run: loop {
                    // Apply what was asked for: a first start, or the changes queued since.
                    let applied = match watcher.as_mut() {
                        None => {
                            for Command::Change { removed, added } in pending.drain(..) {
                                wanted.retain(|(path, _)| !removed.contains(path));
                                wanted.extend(added);
                            }
                            let refs: Vec<(&Path, RecursiveMode)> = wanted
                                .iter()
                                .map(|(path, mode)| (path.as_path(), *mode))
                                .collect();
                            if refs.is_empty() {
                                true
                            } else {
                                match start_stream(&refs, forward()) {
                                    Ok(started) => {
                                        thread_starts.fetch_add(started.starts(), Ordering::SeqCst);
                                        watcher = Some(started);
                                        true
                                    }
                                    Err(_) => false,
                                }
                            }
                        }
                        Some(current) => {
                            let mut ok = true;
                            for command in pending.drain(..) {
                                let Command::Change { removed, added } = command;
                                let removed_refs: Vec<&Path> =
                                    removed.iter().map(std::path::PathBuf::as_path).collect();
                                let added_refs: Vec<(&Path, RecursiveMode)> = added
                                    .iter()
                                    .map(|(path, mode)| (path.as_path(), *mode))
                                    .collect();
                                wanted.retain(|(path, _)| !removed.contains(path));
                                wanted.extend(added.iter().cloned());
                                if current.change(&removed_refs, &added_refs).is_ok() {
                                    thread_starts.fetch_add(1, Ordering::SeqCst);
                                } else {
                                    ok = false;
                                }
                            }
                            if !ok {
                                // Start over with every path asked for, after the backoff.
                                drop(watcher.take());
                            }
                            ok
                        }
                    };
                    thread_live.store(applied && watcher.is_some(), Ordering::SeqCst);
                    if applied {
                        retry = START_RETRY_FIRST;
                        if watcher.is_some() {
                            status(WatchStatus::Live);
                        }
                        // Wait for the next change asked for, or the drop.
                        match received.recv() {
                            Ok(command) => pending.push(command),
                            Err(_) => break,
                        }
                    } else {
                        status(WatchStatus::Refused);
                        match received.recv_timeout(retry) {
                            Ok(command) => pending.push(command),
                            Err(RecvTimeoutError::Timeout) => {}
                            Err(RecvTimeoutError::Disconnected) => break,
                        }
                        retry = (retry * 2).min(START_RETRY_MAX);
                    }
                    // Every change queued meanwhile is applied in one go. A dropped owner ends
                    // the thread here, before another start attempt (HS2-MGX6CW).
                    loop {
                        match received.try_recv() {
                            Ok(command) => pending.push(command),
                            Err(std::sync::mpsc::TryRecvError::Empty) => break,
                            Err(std::sync::mpsc::TryRecvError::Disconnected) => break 'run,
                        }
                    }
                }
                // Stopping a stream can take tens of seconds; it happens here, never on the
                // owner's thread.
                drop(watcher);
            })
            .ok();
        Self {
            commands: Some(commands),
            starts,
            live,
        }
    }

    /// Watch one more path. Returns at once; [`WatchStatus::Live`] follows once it is in.
    pub fn add(&self, path: &Path, mode: RecursiveMode) {
        self.change(&[], &[(path, mode)]);
    }

    /// Stop watching `removed` and start watching `added`, in one change. Returns at once.
    pub fn change(&self, removed: &[&Path], added: &[(&Path, RecursiveMode)]) {
        if removed.is_empty() && added.is_empty() {
            return;
        }
        if let Some(commands) = &self.commands {
            self.live.store(false, std::sync::atomic::Ordering::SeqCst);
            let _ = commands.send(Command::Change {
                removed: removed.iter().map(|path| path.to_path_buf()).collect(),
                added: added
                    .iter()
                    .map(|(path, mode)| (path.to_path_buf(), *mode))
                    .collect(),
            });
        }
    }

    /// How many times this watcher registered paths with the platform so far.
    pub fn starts(&self) -> usize {
        self.starts.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Whether the stream is live over every path asked for so far.
    pub fn is_live(&self) -> bool {
        self.live.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// How long the owner may wait before reading again on its own: [`PENDING_RECHECK`]
    /// while the stream is not live yet (its start or a change can take many seconds, and
    /// nothing reports what changes meanwhile), and `None` once events can be trusted.
    /// The re-check reads local files only, as `ChangeWaiter`'s safety re-check does.
    pub fn recheck(&self) -> Option<std::time::Duration> {
        (!self.is_live()).then_some(PENDING_RECHECK)
    }
}

impl Drop for BackgroundWatcher {
    fn drop(&mut self) {
        // Closing the channel ends the thread, which stops the stream itself.
        drop(self.commands.take());
    }
}

#[cfg(target_os = "macos")]
fn register(
    watcher: &mut backend::RecommendedWatcher,
    paths: &[(&Path, RecursiveMode)],
) -> Result<(), Error> {
    if paths.is_empty() {
        return Ok(());
    }
    // Every path is added to the stopped stream; the commit starts it once.
    let mut pending = watcher.paths_mut();
    for (path, mode) in paths {
        pending.add(path, *mode)?;
    }
    pending.commit()
}

#[cfg(target_os = "macos")]
fn reregister(
    watcher: &mut backend::RecommendedWatcher,
    removed: &[&Path],
    added: &[(&Path, RecursiveMode)],
) -> Result<(), Error> {
    let mut pending = watcher.paths_mut();
    for path in removed {
        // A path that was never watched (or is already gone) has nothing to stop.
        let _ = pending.remove(path);
    }
    for (path, mode) in added {
        pending.add(path, *mode)?;
    }
    pending.commit()
}

#[cfg(not(target_os = "macos"))]
fn reregister(
    watcher: &mut backend::RecommendedWatcher,
    removed: &[&Path],
    added: &[(&Path, RecursiveMode)],
) -> Result<(), Error> {
    for path in removed {
        let _ = watcher.unwatch(path);
    }
    register(watcher, added)
}

#[cfg(not(target_os = "macos"))]
fn register(
    watcher: &mut backend::RecommendedWatcher,
    paths: &[(&Path, RecursiveMode)],
) -> Result<(), Error> {
    for (path, mode) in paths {
        watcher.watch(path, *mode)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::mpsc::{self, Receiver};
    use std::time::{Duration, Instant};

    use super::*;

    /// Paths as events report them: the temporary directory is a symlink on macOS.
    fn real(path: &Path) -> PathBuf {
        hotsheet_sync::paths::canonicalize(path).unwrap()
    }

    fn watching(paths: &[(&Path, RecursiveMode)]) -> (PathWatcher, Receiver<Delivery>) {
        let (sender, receiver) = mpsc::channel();
        let watcher = PathWatcher::start(paths, move |delivery| {
            let _ = sender.send(delivery);
        })
        .unwrap();
        (watcher, receiver)
    }

    /// Wait for a content event on `path`; false when none arrives in ten seconds.
    fn sees(receiver: &Receiver<Delivery>, path: &Path) -> bool {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match receiver.recv_timeout(remaining) {
                Ok(Ok(event)) => {
                    let content = matches!(
                        event.kind,
                        EventKind::Any
                            | EventKind::Create(_)
                            | EventKind::Modify(_)
                            | EventKind::Remove(_)
                    );
                    if content && event.paths.iter().any(|seen| seen == path) {
                        return true;
                    }
                }
                Ok(Err(_)) => {}
                Err(_) => return false,
            }
        }
    }

    /// Whether any event names `path` within `window`.
    fn mentions(receiver: &Receiver<Delivery>, path: &Path, window: Duration) -> bool {
        let deadline = Instant::now() + window;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match receiver.recv_timeout(remaining) {
                Ok(Ok(event)) if event.paths.iter().any(|seen| seen == path) => return true,
                Ok(_) => {}
                Err(_) => return false,
            }
        }
    }

    /// HS2-P0EHD4: access events are never delivered: reading a watched file (which
    /// notify 8's inotify backend reports as an open and a close) wakes nobody, while a
    /// write still arrives. Ported from main's batched-watch test with the classification.
    #[test]
    fn hs2_p0ehd4_reads_of_a_watched_file_are_not_delivered() {
        use backend::event::{AccessKind, AccessMode, CreateKind};
        let access = |kind| Ok(Event::new(EventKind::Access(kind)));
        assert!(is_access(&access(AccessKind::Open(AccessMode::Any))));
        assert!(is_access(&access(AccessKind::Close(AccessMode::Read))));
        assert!(!is_access(&Ok(Event::new(EventKind::Create(
            CreateKind::File
        )))));
        assert!(!is_access(&Ok(rescan_event())));
        let root = tempfile::tempdir().unwrap();
        let dir = real(root.path());
        let file = dir.join("watched");
        fs::write(&file, "x").unwrap();
        let (_watcher, receiver) = watching(&[(&dir, RecursiveMode::Recursive)]);
        fs::write(&file, "y").unwrap();
        assert!(sees(&receiver, &file), "a write is delivered");
        // Let the write's own events arrive, then read many times.
        std::thread::sleep(Duration::from_millis(300));
        while receiver.try_recv().is_ok() {}
        for _ in 0..20 {
            assert_eq!(fs::read_to_string(&file).unwrap(), "y");
        }
        let deadline = Instant::now() + Duration::from_millis(700);
        while let Some(left) = deadline.checked_duration_since(Instant::now()) {
            if let Ok(delivery) = receiver.recv_timeout(left) {
                assert!(!is_access(&delivery), "an access event was delivered");
            }
        }
    }

    #[test]
    fn hs2_n1qdzw_several_paths_take_one_platform_start() {
        let root = tempfile::tempdir().unwrap();
        let root = real(root.path());
        let directories: Vec<PathBuf> = ["a", "b", "c", "d"]
            .iter()
            .map(|name| {
                let directory = root.join(name);
                fs::create_dir_all(&directory).unwrap();
                directory
            })
            .collect();
        let paths: Vec<(&Path, RecursiveMode)> = directories
            .iter()
            .map(|directory| (directory.as_path(), RecursiveMode::Recursive))
            .collect();
        let (mut watcher, receiver) = watching(&paths);
        assert_eq!(watcher.starts(), 1, "four paths, one start");

        // Every path wakes, in an order that interleaves and repeats them.
        for (index, name) in [0, 2, 1, 3, 0, 0, 3].into_iter().zip("abcdefg".chars()) {
            let file = directories[index].join(name.to_string());
            fs::write(&file, b"x").unwrap();
            assert!(sees(&receiver, &file), "no event for {}", file.display());
        }

        // A path added later is a second start, and the earlier paths keep reporting.
        let late = root.join("late");
        fs::create_dir_all(&late).unwrap();
        watcher.add(&late, RecursiveMode::Recursive).unwrap();
        assert_eq!(watcher.starts(), 2);
        for file in [late.join("x"), directories[1].join("y")] {
            fs::write(&file, b"x").unwrap();
            assert!(sees(&receiver, &file), "no event for {}", file.display());
        }
    }

    /// HS2-654VBY: moving a watch from one path to another is one more start, after
    /// which only the new path reports; moving it back and an empty change work too.
    #[test]
    fn hs2_654vby_a_change_moves_the_watch_in_one_start() {
        let root = tempfile::tempdir().unwrap();
        let root = real(root.path());
        let (first, second) = (root.join("first"), root.join("second"));
        fs::create_dir_all(&first).unwrap();
        fs::create_dir_all(&second).unwrap();
        let (mut watcher, receiver) = watching(&[(&first, RecursiveMode::NonRecursive)]);
        assert_eq!(watcher.starts(), 1);
        let file = first.join("one");
        fs::write(&file, b"x").unwrap();
        assert!(sees(&receiver, &file));

        watcher
            .change(&[&first], &[(&second, RecursiveMode::Recursive)])
            .unwrap();
        assert_eq!(watcher.starts(), 2, "an unwatch and a watch, one start");
        let left = first.join("left");
        let moved = second.join("moved");
        fs::write(&left, b"x").unwrap();
        fs::write(&moved, b"x").unwrap();
        assert!(sees(&receiver, &moved));
        assert!(!mentions(&receiver, &left, Duration::from_millis(500)));

        // Nothing to change is no start; removing an unwatched path is not an error.
        watcher.change(&[], &[]).unwrap();
        assert_eq!(watcher.starts(), 2);
        watcher
            .change(
                &[&second, &root.join("never")],
                &[(&first, RecursiveMode::NonRecursive)],
            )
            .unwrap();
        assert_eq!(watcher.starts(), 3);
        let back = first.join("back");
        fs::write(&back, b"x").unwrap();
        assert!(sees(&receiver, &back));
    }

    #[test]
    fn hs2_n1qdzw_each_path_keeps_its_own_depth() {
        let root = tempfile::tempdir().unwrap();
        let root = real(root.path());
        let (shallow, deep) = (root.join("shallow"), root.join("deep"));
        fs::create_dir_all(shallow.join("below")).unwrap();
        fs::create_dir_all(deep.join("below")).unwrap();
        let (_watcher, receiver) = watching(&[
            (&shallow, RecursiveMode::NonRecursive),
            (&deep, RecursiveMode::Recursive),
        ]);

        // A file two levels under the shallow path is not reported; the write after it
        // under the deep path is, which bounds the wait for the absent event.
        let hidden = shallow.join("below/hidden");
        let nested = deep.join("below/nested");
        fs::write(&hidden, b"x").unwrap();
        fs::write(&nested, b"x").unwrap();
        assert!(sees(&receiver, &nested));
        assert!(!mentions(&receiver, &hidden, Duration::from_millis(500)));

        let direct = shallow.join("direct");
        fs::write(&direct, b"x").unwrap();
        assert!(sees(&receiver, &direct));
    }

    #[test]
    fn hs2_n1qdzw_a_directory_created_after_the_start_reports_its_files() {
        let root = tempfile::tempdir().unwrap();
        let root = real(root.path());
        let (_watcher, receiver) = watching(&[(&root, RecursiveMode::Recursive)]);
        let created = root.join("later/deeper");
        fs::create_dir_all(&created).unwrap();
        // Linux adds the watch of a new directory when it sees the directory appear.
        assert!(sees(&receiver, &root.join("later")));
        let file = created.join("file");
        for attempt in 0.. {
            fs::write(&file, attempt.to_string()).unwrap();
            if mentions(&receiver, &file, Duration::from_millis(500)) {
                break;
            }
            assert!(attempt < 20, "no event under a directory created later");
        }
    }

    #[test]
    fn hs2_n1qdzw_a_missing_path_is_refused_and_no_path_is_an_idle_watcher() {
        let root = tempfile::tempdir().unwrap();
        let root = real(root.path());
        let missing = root.join("missing");
        let refused = PathWatcher::start(
            &[
                (&root, RecursiveMode::Recursive),
                (&missing, RecursiveMode::Recursive),
            ],
            |_| {},
        );
        assert!(refused.is_err(), "a missing path must not start a watcher");

        let (mut watcher, receiver) = watching(&[]);
        assert_eq!(watcher.starts(), 1);
        fs::write(root.join("unwatched"), b"x").unwrap();
        assert!(receiver.recv_timeout(Duration::from_millis(300)).is_err());
        // Empty, then refilled: the first real path starts it.
        watcher.add(&root, RecursiveMode::Recursive).unwrap();
        let file = root.join("watched");
        fs::write(&file, b"x").unwrap();
        assert!(sees(&receiver, &file));
    }

    #[test]
    fn hs2_n1qdzw_a_dropped_watcher_goes_quiet_and_a_new_one_reports_again() {
        let root = tempfile::tempdir().unwrap();
        let root = real(root.path());
        for round in 0..3 {
            let (watcher, receiver) = watching(&[(&root, RecursiveMode::Recursive)]);
            let file = root.join(format!("round-{round}"));
            fs::write(&file, b"x").unwrap();
            assert!(sees(&receiver, &file), "round {round}");
            drop(watcher);
            // The handler went with the watcher, so the channel has closed.
            let deadline = Instant::now() + Duration::from_secs(10);
            while receiver.recv_timeout(Duration::from_millis(100)).is_ok() {
                assert!(
                    Instant::now() < deadline,
                    "a dropped watcher kept reporting"
                );
            }
            fs::write(root.join(format!("after-{round}")), b"x").unwrap();
            assert!(matches!(
                receiver.recv_timeout(Duration::from_millis(100)),
                Err(mpsc::RecvTimeoutError::Disconnected)
            ));
        }
    }

    /// How long these tests let the platform take to start or restart one stream: its own
    /// service, measured at up to 7 s to start and 19 s to restart on a loaded Mac, never a
    /// wake of ours (HS2-F6F42W).
    const PLATFORM: Duration = Duration::from_secs(120);

    /// HS2-F6F42W: a stream start the platform holds up holds up neither the owner nor its
    /// drop; `Live` follows once the start returns.
    #[test]
    fn hs2_f6f42w_a_blocked_start_holds_up_neither_the_owner_nor_its_drop() {
        let root = tempfile::tempdir().unwrap();
        let (entered, entering) = mpsc::channel();
        let (release, released) = mpsc::channel::<()>();
        let released = std::sync::Mutex::new(released);
        let (status_tx, status) = mpsc::channel();
        let started = Instant::now();
        let watcher = BackgroundWatcher::start_with(
            &[(root.path(), RecursiveMode::Recursive)],
            |_| {},
            move |state| {
                let _ = status_tx.send(state);
            },
            Box::new(move |paths, handler| {
                let _ = entered.send(());
                let _ = released.lock().unwrap().recv();
                PathWatcher::start(paths, handler)
            }),
        );
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "start returned at once"
        );
        entering.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(!watcher.is_live());
        assert_eq!(watcher.starts(), 0);
        release.send(()).unwrap();
        assert_eq!(status.recv_timeout(PLATFORM).unwrap(), WatchStatus::Live);
        assert!(watcher.is_live());
        assert_eq!(watcher.starts(), 1);
        // A second watcher whose start never returns is dropped at once.
        let (_never, held) = mpsc::channel::<()>();
        let held = std::sync::Mutex::new(held);
        let stuck = BackgroundWatcher::start_with(
            &[(root.path(), RecursiveMode::Recursive)],
            |_| {},
            |_| {},
            Box::new(move |paths, handler| {
                let _ = held.lock().unwrap().recv();
                PathWatcher::start(paths, handler)
            }),
        );
        let dropping = Instant::now();
        drop(stuck);
        assert!(
            dropping.elapsed() < Duration::from_secs(2),
            "the drop returned at once"
        );
    }

    /// HS2-F6F42W: a refused start is reported and retried with a backoff that never
    /// shortens; a later success is live and delivers events; a drop ends the retries.
    #[test]
    fn hs2_f6f42w_a_refused_start_is_retried_with_backoff_until_it_starts() {
        let root = tempfile::tempdir().unwrap();
        let root = real(root.path());
        let attempts = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = attempts.clone();
        let (status_tx, status) = mpsc::channel();
        let (sender, receiver) = mpsc::channel();
        let watcher = BackgroundWatcher::start_with(
            &[(root.as_path(), RecursiveMode::Recursive)],
            move |delivery| {
                let _ = sender.send(delivery);
            },
            move |state| {
                let _ = status_tx.send((state, Instant::now()));
            },
            Box::new(move |paths, handler| {
                if counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst) < 3 {
                    return Err(Error::generic("refused"));
                }
                PathWatcher::start(paths, handler)
            }),
        );
        let reports: Vec<(WatchStatus, Instant)> = (0..4)
            .map(|_| status.recv_timeout(PLATFORM).unwrap())
            .collect();
        let states: Vec<WatchStatus> = reports.iter().map(|(state, _)| *state).collect();
        assert_eq!(
            states,
            [
                WatchStatus::Refused,
                WatchStatus::Refused,
                WatchStatus::Refused,
                WatchStatus::Live
            ]
        );
        for (pair, floor) in reports.windows(2).zip([100_u64, 200]) {
            assert!(pair[1].1 - pair[0].1 >= Duration::from_millis(floor));
        }
        assert!(watcher.is_live());
        let file = root.join("after");
        fs::write(&file, b"x").unwrap();
        assert!(sees(&receiver, &file), "the retried stream reports");
        // Never started: refusals keep coming, then stop with the drop.
        let refused = BackgroundWatcher::start_with(
            &[(root.as_path(), RecursiveMode::Recursive)],
            |_| {},
            |_| {},
            Box::new(|_, _| Err(Error::generic("refused"))),
        );
        drop(refused);
    }

    /// HS2-F6F42W: a path added to a live watcher is live without the owner waiting, and
    /// both old and new paths report; an empty change is nothing.
    #[test]
    fn hs2_f6f42w_an_added_path_goes_live_in_the_background() {
        let root = tempfile::tempdir().unwrap();
        let root = real(root.path());
        let (first, later) = (root.join("first"), root.join("later"));
        fs::create_dir_all(&first).unwrap();
        fs::create_dir_all(&later).unwrap();
        let (status_tx, status) = mpsc::channel();
        let (sender, receiver) = mpsc::channel();
        let watcher = BackgroundWatcher::start(
            &[(first.as_path(), RecursiveMode::Recursive)],
            move |delivery| {
                let _ = sender.send(delivery);
            },
            move |state| {
                let _ = status_tx.send(state);
            },
        );
        assert_eq!(status.recv_timeout(PLATFORM).unwrap(), WatchStatus::Live);
        watcher.change(&[], &[]);
        let asked = Instant::now();
        watcher.add(&later, RecursiveMode::Recursive);
        assert!(
            asked.elapsed() < Duration::from_secs(1),
            "add returned at once"
        );
        assert_eq!(status.recv_timeout(PLATFORM).unwrap(), WatchStatus::Live);
        assert_eq!(watcher.starts(), 2);
        for directory in [&later, &first] {
            let file = directory.join("file");
            fs::write(&file, b"x").unwrap();
            assert!(sees(&receiver, &file), "no event for {}", file.display());
        }
        assert!(rescan_event().need_rescan());
    }

    /// Watch `root` recursively, leaving out `excluded`.
    fn watching_excluding(root: &Path, excluded: &[&Path]) -> (PathWatcher, Receiver<Delivery>) {
        let (sender, receiver) = mpsc::channel();
        let watcher = PathWatcher::start_excluding(
            &[(root, RecursiveMode::Recursive)],
            excluded,
            move |delivery| {
                let _ = sender.send(delivery);
            },
        )
        .unwrap();
        (watcher, receiver)
    }

    /// Write `count` files under each of `dirs` (creating them), then `marker`; wait for the
    /// marker's event and return every path reported up to it. FSEvents reports in order, so
    /// an excluded write made before the marker would have arrived by then.
    fn churn_then_marker(
        receiver: &Receiver<Delivery>,
        dirs: &[PathBuf],
        count: usize,
        marker: &Path,
    ) -> Vec<PathBuf> {
        for dir in dirs {
            fs::create_dir_all(dir).unwrap();
            for index in 0..count {
                fs::write(dir.join(format!("out-{index}")), b"x").unwrap();
            }
        }
        fs::write(marker, b"x").unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut seen = Vec::new();
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match receiver.recv_timeout(remaining) {
                Ok(Ok(event)) => {
                    let done = event.paths.iter().any(|path| path == marker);
                    seen.extend(event.paths);
                    if done {
                        return seen;
                    }
                }
                Ok(Err(_)) => {}
                Err(_) => panic!("no event for {}", marker.display()),
            }
        }
    }

    /// HS2-2HZN2C: excluded build trees (as given or through the macOS `/var` symlink) are
    /// dropped by the platform, including directories created inside them after the start;
    /// a write beside them is still seen, and so is the excluded tree's own parent.
    #[test]
    fn hs2_2hzn2c_excluded_trees_report_nothing_and_their_siblings_still_do() {
        let temp = tempfile::tempdir().unwrap();
        // The spelling as given: on macOS `/var/...`, a symlink to `/private/var/...`.
        let given = temp.path().to_path_buf();
        let root = real(&given);
        let target = given.join("target");
        let worktrees = given.join(".claude/worktrees");
        fs::create_dir_all(&target).unwrap();
        fs::create_dir_all(&worktrees).unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        let (_watcher, receiver) = watching_excluding(&given, &[&target, &worktrees]);
        let seen = churn_then_marker(
            &receiver,
            &[
                root.join("target/debug/deps"),
                root.join(".claude/worktrees/agent/target/debug"),
            ],
            200,
            &root.join("src/tracked.rs"),
        );
        // HS2-236XY3: Linux leaves excluded trees out too.
        if cfg!(any(target_os = "macos", target_os = "linux")) {
            // An excluded directory's own entry belongs to its parent; nothing inside it is
            // reported.
            let inside = |path: &PathBuf, dir: PathBuf| path.starts_with(&dir) && *path != dir;
            let leaked: Vec<_> = seen
                .iter()
                .filter(|path| {
                    inside(path, root.join("target"))
                        || inside(path, root.join(".claude/worktrees"))
                })
                .collect();
            assert!(leaked.is_empty(), "excluded events delivered: {leaked:?}");
        }
        // The excluded directories' parent is still watched.
        let beside = root.join(".claude/settings.json");
        fs::write(&beside, b"{}").unwrap();
        assert!(sees(&receiver, &beside));
    }

    /// HS2-2HZN2C: the exclusions are paths, so they keep applying when the watched root is
    /// removed and created again; tracked writes in the new root are still reported.
    /// macOS only: an FSEvents stream follows the path, while Linux inotify watches the removed
    /// root's inode and reports nothing in its successor (HS2-236XY3 keeps that).
    #[cfg(target_os = "macos")]
    #[test]
    fn hs2_2hzn2c_exclusions_survive_the_root_being_recreated() {
        let temp = tempfile::tempdir().unwrap();
        let root = real(temp.path()).join("checkout");
        fs::create_dir_all(root.join("target")).unwrap();
        let (_watcher, receiver) = watching_excluding(&root, &[&root.join("target")]);
        let first = root.join("first");
        fs::write(&first, b"x").unwrap();
        assert!(sees(&receiver, &first));

        fs::remove_dir_all(&root).unwrap();
        fs::create_dir_all(&root).unwrap();
        let seen = churn_then_marker(&receiver, &[root.join("target")], 100, &root.join("again"));
        // The excluded directory's own removal and creation belong to its parent and are
        // reported; nothing inside it is.
        let target = root.join("target");
        let leaked: Vec<_> = seen
            .iter()
            .filter(|path| path.starts_with(&target) && **path != target)
            .collect();
        assert!(
            leaked.is_empty(),
            "recreated excluded tree reported {leaked:?}"
        );
    }

    /// HS2-2HZN2C: changing the watched paths keeps the exclusions; an empty exclusion list
    /// is a plain watcher; more than the platform's limit is refused.
    #[test]
    fn hs2_2hzn2c_a_change_keeps_exclusions_and_the_limit_is_enforced() {
        let temp = tempfile::tempdir().unwrap();
        let root = real(temp.path());
        let (one, two) = (root.join("one"), root.join("two"));
        fs::create_dir_all(one.join("target")).unwrap();
        fs::create_dir_all(two.join("target")).unwrap();
        let excluded = [one.join("target"), two.join("target")];
        let excluded_refs: Vec<&Path> = excluded.iter().map(PathBuf::as_path).collect();
        let (mut watcher, receiver) = watching_excluding(&one, &excluded_refs);
        watcher
            .change(&[&one], &[(&two, RecursiveMode::Recursive)])
            .unwrap();
        assert_eq!(watcher.starts(), 2);
        let seen = churn_then_marker(&receiver, &[two.join("target")], 50, &two.join("marker"));
        if cfg!(any(target_os = "macos", target_os = "linux")) {
            // The excluded directory's own entry belongs to its parent (FSEvents may still
            // report its creation just before the start); nothing inside it is reported.
            let target = two.join("target");
            let leaked: Vec<_> = seen
                .iter()
                .filter(|path| path.starts_with(&target) && **path != target)
                .collect();
            assert!(leaked.is_empty(), "excluded events delivered: {leaked:?}");
        }
        let left = one.join("left");
        fs::write(&left, b"x").unwrap();
        let after = two.join("after");
        fs::write(&after, b"x").unwrap();
        assert!(sees(&receiver, &after));
        assert!(!mentions(&receiver, &left, Duration::from_millis(300)));

        let (_plain, plain_events) = watching_excluding(&one, &[]);
        let file = one.join("plain");
        fs::write(&file, b"x").unwrap();
        assert!(sees(&plain_events, &file));

        let nine: Vec<PathBuf> = (0..=MAX_EXCLUDED_PATHS)
            .map(|index| root.join(format!("x{index}")))
            .collect();
        let nine: Vec<&Path> = nine.iter().map(PathBuf::as_path).collect();
        let watched = [(root.as_path(), RecursiveMode::Recursive)];
        assert!(PathWatcher::start_excluding(&watched, &nine, |_| {}).is_err());
    }

    /// HS2-236XY3: on Linux the excluding watcher keeps one inotify watch per tracked
    /// directory and follows the tree: a directory created after the start (with entries
    /// made before its watch), renamed within the tree, removed; nothing inside an excluded
    /// tree, created before or after the start; and no symlinked directory.
    #[cfg(target_os = "linux")]
    #[test]
    fn hs2_236xy3_linux_tree_watcher_follows_the_tree_and_skips_excluded_ones() {
        let temp = tempfile::tempdir().unwrap();
        let root = real(temp.path()).join("checkout");
        let outside = real(temp.path()).join("outside");
        for dir in ["src/a/b", "target/debug/deps", "node_modules/pkg"] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, root.join("src/link")).unwrap();
        let excluded = [root.join("target"), root.join("node_modules")];
        let excluded: Vec<&Path> = excluded.iter().map(PathBuf::as_path).collect();
        let (watcher, receiver) = watching_excluding(&root, &excluded);
        let Inner::Tree(tree) = &watcher.watcher else {
            panic!("Linux exclusions use the tree watcher");
        };
        let tracked = |extra: &[&str]| {
            let mut dirs: Vec<PathBuf> = ["", "src", "src/a", "src/a/b"]
                .iter()
                .chain(extra)
                .map(|dir| {
                    if dir.is_empty() {
                        root.clone()
                    } else {
                        root.join(dir)
                    }
                })
                .collect();
            dirs.sort();
            dirs
        };
        let settles = |expected: Vec<PathBuf>| {
            let deadline = Instant::now() + Duration::from_secs(10);
            while tree.watched() != expected {
                assert!(Instant::now() < deadline, "watched {:?}", tree.watched());
                std::thread::sleep(Duration::from_millis(20));
            }
        };
        assert_eq!(tree.watched(), tracked(&[]));

        // Excluded churn, an excluded tree created later and the symlink's target report
        // nothing before a tracked marker.
        fs::create_dir_all(root.join("target/later/x")).unwrap();
        fs::write(outside.join("unseen"), b"x").unwrap();
        let seen = churn_then_marker(
            &receiver,
            &[
                root.join("target/debug/deps"),
                root.join("node_modules/pkg"),
            ],
            50,
            &root.join("src/marker"),
        );
        let leaked: Vec<_> = seen
            .iter()
            .filter(|path| {
                excluded
                    .iter()
                    .any(|dir| path.starts_with(dir) && path.as_path() != *dir)
                    || path.starts_with(&outside)
            })
            .collect();
        assert!(leaked.is_empty(), "{leaked:?}");
        assert_eq!(tree.watched(), tracked(&[]));

        // Created after the start, nested at once: every level is watched and reports.
        fs::create_dir_all(root.join("src/new/deep")).unwrap();
        settles(tracked(&["src/new", "src/new/deep"]));
        let file = root.join("src/new/deep/file");
        fs::write(&file, b"x").unwrap();
        assert!(sees(&receiver, &file));

        // Renamed within the tree: reported under the new path only.
        fs::rename(root.join("src/new"), root.join("src/a/moved")).unwrap();
        settles(tracked(&["src/a/moved", "src/a/moved/deep"]));
        let moved = root.join("src/a/moved/deep/again");
        fs::write(&moved, b"x").unwrap();
        assert!(sees(&receiver, &moved));

        // Removed, then recreated empty and refilled.
        fs::remove_dir_all(root.join("src/a/moved")).unwrap();
        settles(tracked(&[]));
        fs::create_dir(root.join("src/a/moved")).unwrap();
        settles(tracked(&["src/a/moved"]));
        let refilled = root.join("src/a/moved/refilled");
        fs::write(&refilled, b"x").unwrap();
        assert!(sees(&receiver, &refilled));

        // An excluded tree renamed into a tracked directory is watched from then on.
        fs::rename(root.join("target/later"), root.join("src/later")).unwrap();
        settles(tracked(&["src/a/moved", "src/later", "src/later/x"]));
        let inside = root.join("src/later/x/file");
        fs::write(&inside, b"x").unwrap();
        assert!(sees(&receiver, &inside));
    }
}
