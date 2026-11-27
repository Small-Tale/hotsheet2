//! HS2-BN9MMJ: an in-process stand-in for the platform's file-event service.
//!
//! Under an `fseventsd` backlog a real stream delivers minutes late, so a consumer test that
//! waits a fixed time for a real event fails although nothing was lost. Consumers that take a
//! [`StartStream`] (the background watcher, the followed-store, code-ref and repository
//! watches) can run on this stream instead. It reports exactly the events a test hands it, in
//! order, to every watcher whose registered paths admit them (a recursive path admits its
//! whole subtree, a non-recursive one the path and its direct entries), and it can be held to
//! queue events as a backlogged service does. Each consumer module keeps one real-stream
//! smoke test; the platform adapter's own tests in `watch.rs` stay on the real service.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use crate::watch::{Delivery, Error, Event, EventKind, PathWatcher, RecursiveMode, StartStream};

type SharedHandler = Arc<Mutex<Box<dyn FnMut(Delivery) + Send>>>;

/// An in-process file-event service; clones share it.
#[derive(Clone, Default)]
pub struct InProcessStream(Arc<Mutex<State>>);

impl std::fmt::Debug for InProcessStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InProcessStream").finish_non_exhaustive()
    }
}

#[derive(Default)]
struct State {
    next_id: u64,
    watches: Vec<Watch>,
    starts: usize,
    held: bool,
    queued: VecDeque<Event>,
}

struct Watch {
    id: u64,
    paths: Vec<(PathBuf, RecursiveMode)>,
    handler: SharedHandler,
}

/// One watcher's registration; dropping it stops that watcher's deliveries.
pub(crate) struct Registration {
    stream: InProcessStream,
    id: u64,
}

impl std::fmt::Debug for Registration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Registration")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl Registration {
    /// Stop watching `removed` (an unwatched path is ignored) and watch `added`, which must
    /// exist, as the platform requires.
    pub(crate) fn change(
        &mut self,
        removed: &[&Path],
        added: &[(&Path, RecursiveMode)],
    ) -> Result<(), Error> {
        refuse_missing(added)?;
        let mut state = self.stream.lock();
        state.starts += 1;
        if let Some(watch) = state.watches.iter_mut().find(|watch| watch.id == self.id) {
            watch
                .paths
                .retain(|(path, _)| !removed.contains(&path.as_path()));
            watch
                .paths
                .extend(added.iter().map(|(path, mode)| (path.to_path_buf(), *mode)));
        }
        Ok(())
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        self.stream
            .lock()
            .watches
            .retain(|watch| watch.id != self.id);
    }
}

fn refuse_missing(paths: &[(&Path, RecursiveMode)]) -> Result<(), Error> {
    match paths.iter().find(|(path, _)| !path.exists()) {
        Some((path, _)) => Err(Error::path_not_found().add_path(path.to_path_buf())),
        None => Ok(()),
    }
}

/// Whether a watch on `dir` with `mode` reports a change at `path`.
fn admits(dir: &Path, mode: RecursiveMode, path: &Path) -> bool {
    match mode {
        RecursiveMode::Recursive => path.starts_with(dir),
        RecursiveMode::NonRecursive => path == dir || path.parent() == Some(dir),
    }
}

impl InProcessStream {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Start a watcher over `paths` on this stream, as [`PathWatcher::start`] does on the
    /// platform: every path must exist, and access events are never delivered.
    pub fn start<F>(
        &self,
        paths: &[(&Path, RecursiveMode)],
        handler: F,
    ) -> Result<PathWatcher, Error>
    where
        F: FnMut(Delivery) + Send + 'static,
    {
        refuse_missing(paths)?;
        let mut state = self.lock();
        let id = state.next_id;
        state.next_id += 1;
        state.starts += 1;
        state.watches.push(Watch {
            id,
            paths: paths
                .iter()
                .map(|(path, mode)| (path.to_path_buf(), *mode))
                .collect(),
            handler: Arc::new(Mutex::new(Box::new(handler))),
        });
        drop(state);
        Ok(PathWatcher::in_process(Registration {
            stream: self.clone(),
            id,
        }))
    }

    /// A [`StartStream`] that starts each watcher on this stream.
    pub fn start_stream(&self) -> StartStream {
        let stream = self.clone();
        Box::new(move |paths, handler| stream.start(paths, handler))
    }

    /// Report `event` to every watcher that admits one of its paths, with only the paths it
    /// admits; queued instead while the stream is held. Report paths as the platform spells
    /// them (canonical: `/private/var/...` on macOS).
    pub fn report(&self, event: Event) {
        let mut state = self.lock();
        if state.held {
            state.queued.push_back(event);
            return;
        }
        let deliveries = route(&state, &event);
        drop(state);
        for (handler, delivered) in deliveries {
            (handler.lock().unwrap_or_else(PoisonError::into_inner))(Ok(delivered));
        }
    }

    /// Report a content change of `path`, spelled as the platform reports it.
    pub fn changed(&self, path: &Path) {
        self.report(
            Event::new(EventKind::Modify(crate::watch::event::ModifyKind::Any))
                .add_path(reported(path)),
        );
    }

    /// Queue every later report until [`InProcessStream::release`], as a backlogged service
    /// delays them.
    pub fn hold(&self) {
        self.lock().held = true;
    }

    /// Deliver the queued reports in order, to the watchers registered now, and report
    /// directly again.
    pub fn release(&self) {
        loop {
            let mut state = self.lock();
            let Some(event) = state.queued.pop_front() else {
                state.held = false;
                return;
            };
            let deliveries = route(&state, &event);
            drop(state);
            for (handler, delivered) in deliveries {
                (handler.lock().unwrap_or_else(PoisonError::into_inner))(Ok(delivered));
            }
        }
    }

    /// How many times a watcher registered paths with this stream (a start or a change).
    pub fn starts(&self) -> usize {
        self.lock().starts
    }

    /// Every path the live watchers registered, watcher by watcher in start order.
    pub fn watched(&self) -> Vec<(PathBuf, RecursiveMode)> {
        self.lock()
            .watches
            .iter()
            .flat_map(|watch| watch.paths.iter().cloned())
            .collect()
    }
}

/// Each watcher `event` reaches, with the paths of it that watcher admits.
fn route(state: &State, event: &Event) -> Vec<(SharedHandler, Event)> {
    if matches!(event.kind, EventKind::Access(_)) {
        return Vec::new();
    }
    state
        .watches
        .iter()
        .filter_map(|watch| {
            let paths: Vec<PathBuf> = event
                .paths
                .iter()
                .filter(|path| {
                    watch
                        .paths
                        .iter()
                        .any(|(dir, mode)| admits(dir, *mode, path))
                })
                .cloned()
                .collect();
            if paths.is_empty() {
                return None;
            }
            let mut delivered = event.clone();
            delivered.paths = paths;
            Some((watch.handler.clone(), delivered))
        })
        .collect()
}

/// The spelling the platform reports for `path`: canonical, through its parent when the path
/// itself is gone.
fn reported(path: &Path) -> PathBuf {
    if let Ok(path) = hotsheet_sync::paths::canonicalize(path) {
        return path;
    }
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => hotsheet_sync::paths::canonicalize(parent)
            .map_or_else(|_| path.to_path_buf(), |parent| parent.join(name)),
        _ => path.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;
    use crate::watch::event::{AccessKind, CreateKind};

    fn real(path: &Path) -> PathBuf {
        hotsheet_sync::paths::canonicalize(path).unwrap()
    }

    fn collect(receiver: &mpsc::Receiver<Delivery>) -> Vec<PathBuf> {
        receiver
            .try_iter()
            .flat_map(|delivery| delivery.unwrap().paths)
            .collect()
    }

    /// HS2-BN9MMJ: a recursive path admits its subtree, a non-recursive one its direct
    /// entries; an event is delivered with only the paths a watcher admits; access events
    /// and unwatched paths are not delivered; a missing path is refused.
    #[test]
    fn hs2_bn9mmj_routes_by_registered_path_and_depth() {
        let temp = tempfile::tempdir().unwrap();
        let root = real(temp.path());
        let (deep, shallow) = (root.join("deep"), root.join("shallow"));
        std::fs::create_dir_all(deep.join("a/b")).unwrap();
        std::fs::create_dir_all(shallow.join("below")).unwrap();
        let stream = InProcessStream::default();
        let (sender, receiver) = mpsc::channel();
        let _watcher = stream
            .start(
                &[
                    (&deep, RecursiveMode::Recursive),
                    (&shallow, RecursiveMode::NonRecursive),
                ],
                move |delivery| {
                    let _ = sender.send(delivery);
                },
            )
            .unwrap();
        stream.changed(&deep.join("a/b/file"));
        stream.changed(&shallow.join("direct"));
        stream.changed(&shallow.join("below/hidden"));
        stream.changed(&root.join("outside"));
        stream.report(Event::new(EventKind::Access(AccessKind::Any)).add_path(deep.join("read")));
        stream.report(
            Event::new(EventKind::Create(CreateKind::File))
                .add_path(root.join("elsewhere"))
                .add_path(deep.join("mixed")),
        );
        assert_eq!(
            collect(&receiver),
            [
                deep.join("a/b/file"),
                shallow.join("direct"),
                deep.join("mixed")
            ]
        );
        let missing = root.join("missing");
        assert!(
            stream
                .start(&[(&missing, RecursiveMode::Recursive)], |_| {})
                .is_err()
        );
        assert_eq!(stream.starts(), 1);
    }

    /// HS2-BN9MMJ: a change moves a watcher's paths in one start, a dropped watcher goes
    /// quiet while another keeps reporting, and a held stream delivers its queue in order on
    /// release, to the watchers registered then.
    #[test]
    fn hs2_bn9mmj_changes_drops_and_holds_like_the_platform() {
        let temp = tempfile::tempdir().unwrap();
        let root = real(temp.path());
        let (first, second) = (root.join("first"), root.join("second"));
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        let stream = InProcessStream::default();
        let (sender, receiver) = mpsc::channel();
        let mut moving = stream
            .start(&[(&first, RecursiveMode::Recursive)], move |delivery| {
                let _ = sender.send(delivery);
            })
            .unwrap();
        let (other_sender, other) = mpsc::channel();
        let staying = stream
            .start(&[(&first, RecursiveMode::Recursive)], move |delivery| {
                let _ = other_sender.send(delivery);
            })
            .unwrap();
        moving
            .change(&[&first], &[(&second, RecursiveMode::Recursive)])
            .unwrap();
        assert_eq!(moving.starts(), 2);
        assert_eq!(stream.starts(), 3);
        assert_eq!(
            stream.watched(),
            [
                (second.clone(), RecursiveMode::Recursive),
                (first.clone(), RecursiveMode::Recursive)
            ]
        );
        stream.changed(&first.join("one"));
        stream.changed(&second.join("two"));
        assert_eq!(collect(&receiver), [second.join("two")]);
        assert_eq!(collect(&other), [first.join("one")]);

        stream.hold();
        stream.changed(&second.join("a"));
        stream.changed(&first.join("b"));
        stream.changed(&second.join("c"));
        assert!(collect(&receiver).is_empty() && collect(&other).is_empty());
        drop(staying);
        stream.release();
        assert_eq!(collect(&receiver), [second.join("a"), second.join("c")]);
        assert!(matches!(
            other.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
        stream.changed(&second.join("d"));
        assert_eq!(collect(&receiver), [second.join("d")]);
        drop(moving);
        assert!(stream.watched().is_empty());
        assert!(matches!(
            receiver.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
    }
}
