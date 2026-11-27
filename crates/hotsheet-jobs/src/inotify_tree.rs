//! HS2-236XY3: the Linux watcher behind [`crate::watch::PathWatcher::start_excluding`].
//!
//! notify's recursive inotify watch adds a kernel watch for every directory under a root,
//! including ignored build trees, whose churn then reaches the process only to be dropped.
//! This watcher uses notify's inotify backend with one non-recursive watch per directory the
//! [`TreePlan`] keeps, so excluded subtrees take no watches and deliver nothing. Each
//! directory gets notify's own mask, so tracked paths report what the recursive watch
//! reported.
//!
//! notify runs the handler on its event-loop thread, and `watch`/`unwatch` wait for that
//! thread, so the handler only queues what an event means ([`notices`]) and a manager thread
//! applies it. Entries found inside a directory that appeared (created before its watch
//! was in place) are reported as one create event. An exhausted watch limit fails the start;
//! later it is delivered as a `MaxFilesWatch` error, so the consumer reads again.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use notify::{Config, INotifyWatcher, Watcher};

use crate::tree_plan::{DiskTree, Grown, Notice, TreePlan, Watched, found_event, notices, refusal};
use crate::watch::{Delivery, Error, RecursiveMode, is_access};

type Handler = Arc<Mutex<Box<dyn FnMut(Delivery) + Send>>>;

struct State {
    watcher: INotifyWatcher,
    plan: TreePlan,
    /// Paths watched on their own (a non-recursive path, or a file).
    singles: Vec<PathBuf>,
}

enum Message {
    Notice(Notice),
    Stop,
}

/// A per-directory inotify watcher over trees, leaving excluded subtrees out.
pub(crate) struct TreeWatcher {
    state: Arc<Mutex<State>>,
    messages: Sender<Message>,
    stopping: Arc<AtomicBool>,
}

impl TreeWatcher {
    pub(crate) fn start(
        paths: &[(&Path, RecursiveMode)],
        excluded: &[&Path],
        handler: Box<dyn FnMut(Delivery) + Send>,
    ) -> Result<Self, Error> {
        let handler: Handler = Arc::new(Mutex::new(handler));
        let (messages, received) = std::sync::mpsc::channel();
        let queue = messages.clone();
        let forward = handler.clone();
        let mut watcher = INotifyWatcher::new(
            move |delivery: Delivery| {
                if is_access(&delivery) {
                    return;
                }
                for notice in notices(&delivery) {
                    let _ = queue.send(Message::Notice(notice));
                }
                deliver(&forward, delivery);
            },
            Config::default(),
        )?;
        let mut roots = Vec::new();
        let mut singles = Vec::new();
        for (path, mode) in paths {
            if is_tree(path, *mode)? {
                roots.push(path.to_path_buf());
            } else {
                watcher.watch(path, RecursiveMode::NonRecursive)?;
                singles.push(path.to_path_buf());
            }
        }
        let excluded: Vec<PathBuf> = excluded.iter().map(|path| path.to_path_buf()).collect();
        let plan = TreePlan::new(roots, &excluded, |root| {
            hotsheet_sync::paths::canonicalize(root).ok()
        });
        let mut state = State {
            watcher,
            plan,
            singles,
        };
        let stopping = Arc::new(AtomicBool::new(false));
        let State { watcher, plan, .. } = &mut state;
        plan.start(&DiskTree, |dir| watch_dir(watcher, dir, &stopping))?;
        let state = Arc::new(Mutex::new(state));
        let (thread_state, thread_stopping) = (state.clone(), stopping.clone());
        std::thread::Builder::new()
            .name("hotsheet-tree-watcher".into())
            .spawn(move || manage(&received, &thread_state, &handler, &thread_stopping))
            .map_err(Error::io)?;
        Ok(Self {
            state,
            messages,
            stopping,
        })
    }

    /// Stop watching `removed` and start watching `added`.
    pub(crate) fn change(
        &mut self,
        removed: &[&Path],
        added: &[(&Path, RecursiveMode)],
    ) -> Result<(), Error> {
        let mut guard = lock(&self.state);
        let State {
            watcher,
            plan,
            singles,
        } = &mut *guard;
        for path in removed {
            if let Some(index) = singles.iter().position(|single| single == path) {
                singles.remove(index);
                let _ = watcher.unwatch(path);
            }
            for dir in plan.remove_root(path) {
                let _ = watcher.unwatch(&dir);
            }
        }
        for (path, mode) in added {
            if is_tree(path, *mode)? {
                plan.add_root(path, &DiskTree, |dir| {
                    watch_dir(watcher, dir, &self.stopping)
                })?;
            } else {
                watcher.watch(path, RecursiveMode::NonRecursive)?;
                singles.push(path.to_path_buf());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
impl TreeWatcher {
    /// The directories watched now.
    pub(crate) fn watched(&self) -> Vec<PathBuf> {
        lock(&self.state).plan.watched().iter().cloned().collect()
    }
}

impl Drop for TreeWatcher {
    fn drop(&mut self) {
        // The manager stops before its next change; the inotify watcher goes with the
        // last reference to the state, and its handler with it.
        self.stopping.store(true, Ordering::SeqCst);
        let _ = self.messages.send(Message::Stop);
    }
}

/// Whether `path` is watched as a tree. Each path must exist, as for notify.
fn is_tree(path: &Path, mode: RecursiveMode) -> Result<bool, Error> {
    let metadata =
        std::fs::metadata(path).map_err(|error| Error::io(error).add_path(path.into()))?;
    Ok(mode == RecursiveMode::Recursive && metadata.is_dir())
}

fn watch_dir(watcher: &mut INotifyWatcher, dir: &Path, stopping: &AtomicBool) -> Watched<Error> {
    if stopping.load(Ordering::SeqCst) {
        return Watched::Stop(Error::generic("the watcher is stopping"));
    }
    match watcher.watch(dir, RecursiveMode::NonRecursive) {
        Ok(()) => Watched::Yes,
        Err(error) => refusal(error),
    }
}

fn lock<T: ?Sized>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn deliver(handler: &Handler, delivery: Delivery) {
    (lock(handler))(delivery);
}

/// Apply notices in order until the watcher is dropped. The state lock is never held while
/// the handler runs, and the handler never takes it, so neither thread waits on the other.
fn manage(
    received: &Receiver<Message>,
    state: &Mutex<State>,
    handler: &Handler,
    stopping: &AtomicBool,
) {
    while let Ok(Message::Notice(notice)) = received.recv() {
        let outcome = {
            let mut guard = lock(state);
            let State { watcher, plan, .. } = &mut *guard;
            match notice {
                Notice::Appeared(path) => {
                    plan.appeared(&path, &DiskTree, |dir| watch_dir(watcher, dir, stopping))
                }
                Notice::Gone(path) => {
                    for dir in plan.gone(&path) {
                        // notify drops most of these itself; a second unwatch is harmless.
                        let _ = watcher.unwatch(&dir);
                    }
                    Ok(Grown::default())
                }
                Notice::Rescan => {
                    for dir in plan.reset() {
                        let _ = watcher.unwatch(&dir);
                    }
                    // The rescan event itself tells the consumer to read everything again.
                    plan.start(&DiskTree, |dir| watch_dir(watcher, dir, stopping))
                        .map(|_| Grown::default())
                }
            }
        };
        if stopping.load(Ordering::SeqCst) {
            break;
        }
        match outcome {
            Ok(grown) if !grown.found.is_empty() => deliver(handler, Ok(found_event(grown.found))),
            Ok(_) => {}
            Err(error) => deliver(handler, Err(error)),
        }
    }
}
