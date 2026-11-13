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
    pub fn start<F>(paths: &[(&Path, RecursiveMode)], handler: F) -> Result<Self, Error>
    where
        F: FnMut(Delivery) + Send + 'static,
    {
        let mut watcher = backend::recommended_watcher(handler)?;
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

    /// How many times this watcher registered paths with the platform: one for
    /// [`PathWatcher::start`] and one for each [`PathWatcher::add`]. On macOS each is
    /// one event-stream start.
    pub fn starts(&self) -> usize {
        self.starts
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
        path.canonicalize().unwrap()
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
}
