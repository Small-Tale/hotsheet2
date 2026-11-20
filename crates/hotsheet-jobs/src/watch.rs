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

use std::path::Path;

#[cfg(not(target_os = "macos"))]
use notify as backend;
#[cfg(target_os = "macos")]
use notify_batched as backend;

use backend::Watcher;
pub use backend::{Error, Event, EventKind, RecursiveMode};

/// What a watcher hands its handler: an event, or the reason the platform lost some.
pub type Delivery = Result<Event, Error>;

/// A platform file watcher over a fixed set of paths, started once.
pub struct PathWatcher {
    watcher: backend::RecommendedWatcher,
    starts: usize,
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
        Ok(Self { watcher, starts: 1 })
    }

    /// Watch one more path. On macOS this restarts the event stream once, so pass
    /// every path known at the start to [`PathWatcher::start`] instead.
    pub fn add(&mut self, path: &Path, mode: RecursiveMode) -> Result<(), Error> {
        register(&mut self.watcher, &[(path, mode)])?;
        self.starts += 1;
        Ok(())
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
        reregister(&mut self.watcher, removed, added)?;
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

/// HS2-P0EHD4: whether a delivery only reports that a file was accessed, which no
/// consumer treats as a change.
pub fn is_access(delivery: &Delivery) -> bool {
    matches!(delivery, Ok(event) if matches!(event.kind, EventKind::Access(_)))
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
                loop {
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
                    // Every change queued meanwhile is applied in one go.
                    while let Ok(command) = received.try_recv() {
                        pending.push(command);
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
}
