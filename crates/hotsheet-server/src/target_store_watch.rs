//! HS2-G5FYP6: ticket changes in the linked Git stores of a followed checkout that this
//! server does not host.
//!
//! A run follower learns of target ticket changes from this server's target feed
//! (`/checkouts/{checkout}/target-events`). A store the server hosts reports its changes
//! through its own file watcher. A linked store it does not host has none, so a claim a
//! headless `hotsheet-cli` writes there would reach the follower only on its next
//! reconnect. Hosting each such store for the feed costs an index, a native stream and a
//! polling bridge per store, which made loaded runs miss their deadlines (HS2-BDNJ5S).
//! Instead the feed keeps one file watcher on the store's `tickets` directory while
//! followers ask, and records each changed ticket file as a [`TARGET_STORE_CHANGE_KIND`]
//! event in the ordinary change log, so cursors and replay work as for hosted stores.
//! Only the feed reads that kind; it carries the ticket's id and no content.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hotsheet_jobs::watch::{
    BackgroundWatcher, Delivery, EventKind, PENDING_RECHECK, PathWatcher, RecursiveMode,
    StartStream, WatchStatus,
};
use hotsheet_model::Ulid;
use tokio::sync::broadcast;

use crate::events::{ChangeEvent, EventLog, emit_change};
use crate::store_watch_scope::TicketsWatchScope;
use hotsheet_sync::LockExt;

/// The change kind the target feed reads for a store this server does not host.
pub(crate) const TARGET_STORE_CHANGE_KIND: &str = "target_store_changed";

/// A watch nobody's feed asked for in this long is dropped at the next request.
const IDLE: Duration = Duration::from_secs(10 * 60);

/// One linked Git store a feed request names: its connection id and root.
pub(crate) struct UnhostedStore {
    pub connection_id: String,
    pub root: PathBuf,
}

struct Watch {
    _watcher: BackgroundWatcher,
    /// HS2-R49D43: the pending re-check ends once this is dropped.
    _pending: Arc<()>,
    asked: Instant,
}

/// The `tickets` watches of unhosted followed stores, by connection id.
#[derive(Default)]
pub(crate) struct TargetStoreWatches {
    watches: Mutex<HashMap<String, Watch>>,
}

impl TargetStoreWatches {
    /// Keep a watch on each of `stores` (and only while feeds keep asking). HS2-F6F42W: a
    /// store first named here is watched from a background thread; every ticket written
    /// since this call is recorded once the stream is live, so a change after the caller's
    /// cursor is never lost. Watches unasked for [`IDLE`] are dropped.
    pub(crate) fn follow(
        &self,
        stores: Vec<UnhostedStore>,
        event_log: &Arc<Mutex<EventLog>>,
        events: &broadcast::Sender<ChangeEvent>,
    ) {
        let now = Instant::now();
        let missing: Vec<UnhostedStore> = {
            let mut watches = self.watches.lock_or_recover();
            watches.retain(|_, watch| now.duration_since(watch.asked) < IDLE);
            stores
                .into_iter()
                .filter(|store| match watches.get_mut(&store.connection_id) {
                    Some(watch) => {
                        watch.asked = now;
                        false
                    }
                    None => true,
                })
                .collect()
        };
        // A native stream can take seconds to start under load; other feeds keep going.
        for store in missing {
            let Some((watcher, pending)) = watch_tickets(&store, event_log, events) else {
                continue;
            };
            // A concurrent request may have started the same watch; keep the first.
            self.watches
                .lock_or_recover()
                .entry(store.connection_id)
                .or_insert(Watch {
                    _watcher: watcher,
                    _pending: pending,
                    asked: now,
                });
        }
    }

    /// Stop watching a store, for example once this server hosts it.
    pub(crate) fn forget(&self, connection_id: &str) {
        self.watches.lock_or_recover().remove(connection_id);
    }

    #[cfg(test)]
    pub(crate) fn watched(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.watches.lock_or_recover().keys().cloned().collect();
        ids.sort();
        ids
    }
}

/// Start the watch on `store`'s tickets directory, recording each changed ticket file.
///
/// HS2-F6F42W: the shared background watcher starts the stream on its own thread, so the
/// feed request that asked for it never waits on the platform (seconds on a loaded Mac),
/// and dropping an idle watch never waits for its stream to stop. A ticket written before
/// the stream was live is not reported by it, so going live (and each refused start while
/// it is retried) records every ticket file modified since the watch was asked for.
///
/// HS2-R49D43: a start took over a minute on a loaded machine, and a write made meanwhile
/// waited that long for the feed. Until the stream is live, the store is read again every
/// [`PENDING_RECHECK`], as `ChangeWaiter`'s safety re-check does: local files only, and
/// each modified ticket file recorded once. The re-check ends when the stream goes live or
/// the returned token is dropped.
fn watch_tickets(
    store: &UnhostedStore,
    event_log: &Arc<Mutex<EventLog>>,
    events: &broadcast::Sender<ChangeEvent>,
) -> Option<(BackgroundWatcher, Arc<()>)> {
    watch_tickets_with(store, event_log, events, Box::new(PathWatcher::start))
}

/// [`watch_tickets`] with the stream started by `start`, for a test's stand-in.
fn watch_tickets_with(
    store: &UnhostedStore,
    event_log: &Arc<Mutex<EventLog>>,
    events: &broadcast::Sender<ChangeEvent>,
    start: StartStream,
) -> Option<(BackgroundWatcher, Arc<()>)> {
    // A store without a tickets directory has nothing to follow yet.
    let tickets = hotsheet_sync::paths::canonicalize(store.root.join("tickets")).ok()?;
    // HS2-VN96J5: on Windows a handle on `tickets` would keep the store from being moved.
    let scope = TicketsWatchScope::for_store(tickets.parent()?);
    watch_scoped_tickets(store, &tickets, scope, event_log, events, start)
}

/// [`watch_tickets_with`] watching `scope`'s directory, which tests set to the store root.
fn watch_scoped_tickets(
    store: &UnhostedStore,
    tickets: &Path,
    scope: TicketsWatchScope,
    event_log: &Arc<Mutex<EventLog>>,
    events: &broadcast::Sender<ChangeEvent>,
    start: StartStream,
) -> Option<(BackgroundWatcher, Arc<()>)> {
    let tickets = tickets.to_path_buf();
    let watch_dir = scope.watch_dir().to_path_buf();
    let (log, sender, connection) = (
        event_log.clone(),
        events.clone(),
        store.connection_id.clone(),
    );
    let emit = Arc::new(move |ids: Vec<String>| {
        for id in ids {
            emit_change(
                &log,
                &sender,
                ChangeEvent {
                    live: None,
                    cursor: None,
                    store: connection.clone(),
                    kind: TARGET_STORE_CHANGE_KIND.into(),
                    id,
                    slug: String::new(),
                    message: None,
                    activity: None,
                    assignment: None,
                    turn: None,
                },
            );
        }
    });
    let on_event = emit.clone();
    let handler = move |delivery: Delivery| {
        let Ok(mut event) = delivery else { return };
        if matches!(event.kind, EventKind::Access(_))
            || scope.admit_paths(&mut event.paths).is_none()
        {
            return;
        }
        on_event(changed_tickets(event.paths));
    };
    let recent = Arc::new(Mutex::new(Recent::since(std::time::SystemTime::now())));
    let live = Arc::new(AtomicBool::new(false));
    let read = {
        let (recent, tickets, emit) = (recent.clone(), tickets.clone(), emit.clone());
        move || emit(changed_tickets(recent.lock_or_recover().changed(&tickets)))
    };
    let on_status = read.clone();
    let went_live = live.clone();
    let status = move |status: WatchStatus| {
        if status == WatchStatus::Live {
            went_live.store(true, Ordering::SeqCst);
        }
        on_status();
    };
    let watcher = BackgroundWatcher::start_with(
        &[(&watch_dir, RecursiveMode::Recursive)],
        handler,
        status,
        start,
    );
    let pending = Arc::new(());
    let owner = Arc::downgrade(&pending);
    let spawned = std::thread::Builder::new()
        .name("target-store-recheck".into())
        .spawn(move || {
            loop {
                std::thread::sleep(PENDING_RECHECK);
                if live.load(Ordering::SeqCst) || owner.upgrade().is_none() {
                    return;
                }
                read();
            }
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, "no pending re-check for a linked store; it waits for its watch");
    }
    Some((watcher, pending))
}

/// The ticket files of one store recorded since its watch was asked for, by their
/// modification time, so a re-check records each write once.
struct Recent {
    since: std::time::SystemTime,
    seen: HashMap<PathBuf, std::time::SystemTime>,
}

impl Recent {
    fn since(since: std::time::SystemTime) -> Self {
        Self {
            since,
            seen: HashMap::new(),
        }
    }

    /// The ticket files modified since the watch was asked for that were not recorded
    /// yet at their current modification time.
    fn changed(&mut self, tickets: &Path) -> Vec<PathBuf> {
        modified_since(tickets, self.since)
            .into_iter()
            .filter(|(path, modified)| self.seen.insert(path.clone(), *modified) != Some(*modified))
            .map(|(path, _)| path)
            .collect()
    }
}

/// The ticket files under `tickets` (its shard directories included) modified at or after
/// `since`, give or take the file system's timestamp granularity, with their times.
fn modified_since(
    tickets: &Path,
    since: std::time::SystemTime,
) -> Vec<(PathBuf, std::time::SystemTime)> {
    let floor = since
        .checked_sub(std::time::Duration::from_secs(2))
        .unwrap_or(since);
    let recent = |path: PathBuf| {
        std::fs::metadata(&path)
            .and_then(|meta| meta.modified())
            .ok()
            .filter(|modified| *modified >= floor)
            .map(|modified| (path, modified))
    };
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(tickets) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Ok(inner) = std::fs::read_dir(&path) {
                found.extend(
                    inner
                        .flatten()
                        .map(|entry| entry.path())
                        .filter(|path| path.is_file())
                        .filter_map(recent),
                );
            }
        } else if let Some(file) = recent(path) {
            found.push(file);
        }
    }
    found
}

/// The ticket ids (file-name ULIDs) a set of event paths touches. A directory path stands
/// for the ticket files it holds, because a recursive backend can report only the new
/// shard directory a ticket lands in.
fn changed_tickets(paths: Vec<PathBuf>) -> Vec<String> {
    let id = |path: &Path| {
        (path.extension().and_then(|ext| ext.to_str()) == Some("md"))
            .then(|| Ulid::from_string(path.file_stem()?.to_str()?).ok())
            .flatten()
            .map(|id| id.to_string())
    };
    let mut ids = Vec::new();
    for path in paths {
        if path.is_dir() {
            if let Ok(entries) = std::fs::read_dir(&path) {
                ids.extend(entries.flatten().filter_map(|entry| id(&entry.path())));
            }
        } else if let Some(found) = id(&path) {
            ids.push(found);
        }
    }
    ids.sort();
    ids.dedup();
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A followed store's ticket write is recorded once the watch is up; asking again
    /// keeps the one watch, and forgetting it stops recording.
    #[test]
    fn hs2_g5fyp6_follow_records_ticket_writes_in_an_unhosted_store() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("linked");
        let shard = root.join("tickets").join("AB");
        std::fs::create_dir_all(&shard).unwrap();
        let log = Arc::new(Mutex::new(EventLog::default()));
        let (events, _keep) = broadcast::channel(16);
        let watches = TargetStoreWatches::default();
        let store = || UnhostedStore {
            connection_id: "linked-connection".into(),
            root: root.clone(),
        };
        watches.follow(vec![store()], &log, &events);
        watches.follow(vec![store()], &log, &events);
        assert_eq!(watches.watched(), ["linked-connection"]);
        // No tickets directory yet: nothing to watch.
        watches.follow(
            vec![UnhostedStore {
                connection_id: "empty".into(),
                root: dir.path().join("empty"),
            }],
            &log,
            &events,
        );
        assert_eq!(watches.watched(), ["linked-connection"]);

        let id = Ulid::new().to_string();
        std::fs::write(shard.join(format!("{id}.md")), "---\n---\n").unwrap();
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            let (recorded, _) = log.lock_or_recover().since(0);
            if recorded.iter().any(|event| {
                event.kind == TARGET_STORE_CHANGE_KIND
                    && event.store == "linked-connection"
                    && event.id == id
            }) {
                break;
            }
            assert!(Instant::now() < deadline, "the write was never recorded");
            std::thread::sleep(Duration::from_millis(20));
        }
        watches.forget("linked-connection");
        assert!(watches.watched().is_empty());
    }

    /// HS2-R49D43: while the stream has not started (over a minute on a loaded machine),
    /// a ticket written in the store still reaches the log within the re-check, once per
    /// write, and dropping the watch ends the re-check.
    #[test]
    fn hs2_r49d43_a_write_before_the_stream_is_live_is_recorded_by_the_recheck() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("linked");
        let shard = root.join("tickets").join("AB");
        std::fs::create_dir_all(&shard).unwrap();
        let log = Arc::new(Mutex::new(EventLog::default()));
        let (events, _keep) = broadcast::channel(16);
        // A start that never finishes while the test runs.
        let (hold, held) = std::sync::mpsc::channel::<()>();
        let held = Mutex::new(held);
        let start: StartStream = Box::new(move |_, _| {
            let _ = held.lock_or_recover().recv();
            Err(hotsheet_jobs::watch::Error::generic("stopped"))
        });
        let store = UnhostedStore {
            connection_id: "linked-connection".into(),
            root: root.clone(),
        };
        let (watcher, pending) = watch_tickets_with(&store, &log, &events, start).unwrap();
        let recorded = |id: &str| {
            log.lock_or_recover()
                .since(0)
                .0
                .iter()
                .filter(|event| event.kind == TARGET_STORE_CHANGE_KIND && event.id == id)
                .count()
        };
        let until_recorded = |id: &str, times: usize| {
            let deadline = Instant::now() + Duration::from_secs(20);
            while recorded(id) < times {
                assert!(Instant::now() < deadline, "the write was never recorded");
                std::thread::sleep(Duration::from_millis(20));
            }
        };
        let id = Ulid::new().to_string();
        let file = shard.join(format!("{id}.md"));
        std::fs::write(&file, "---\n---\n").unwrap();
        until_recorded(&id, 1);
        assert!(!watcher.is_live());
        // Re-checks of an unchanged file record nothing more.
        std::thread::sleep(PENDING_RECHECK * 3);
        assert_eq!(recorded(&id), 1);
        // A later write to the same ticket is recorded again.
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(&file, "---\nstatus: started\n---\n").unwrap();
        until_recorded(&id, 2);
        // Dropped, the watch stops reading.
        drop((watcher, pending));
        std::thread::sleep(PENDING_RECHECK * 2);
        let other = Ulid::new().to_string();
        std::fs::write(shard.join(format!("{other}.md")), "---\n---\n").unwrap();
        std::thread::sleep(PENDING_RECHECK * 3);
        assert_eq!(recorded(&other), 0);
        // Only now may the stand-in start end (as refused, which reads once more).
        drop(hold);
    }

    /// HS2-VN96J5: in the store-root scope Windows uses, the watch holds the store root,
    /// records ticket writes and nothing beside `tickets/`, and the store can be moved.
    #[test]
    fn hs2_vn96j5_a_root_scoped_watch_records_only_ticket_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("linked");
        let shard = root.join("tickets").join("AB");
        let git = root.join(".git").join("churn");
        std::fs::create_dir_all(&shard).unwrap();
        std::fs::create_dir_all(&git).unwrap();
        let log = Arc::new(Mutex::new(EventLog::default()));
        let (events, _keep) = broadcast::channel(16);
        let store = UnhostedStore {
            connection_id: "linked-connection".into(),
            root: root.clone(),
        };
        let canonical = hotsheet_sync::paths::canonicalize(&root).unwrap();
        let watched = Arc::new(Mutex::new(Vec::new()));
        let start: StartStream = {
            let watched = watched.clone();
            Box::new(move |paths, handler| {
                watched
                    .lock_or_recover()
                    .extend(paths.iter().map(|(path, _)| path.to_path_buf()));
                PathWatcher::start(paths, handler)
            })
        };
        let (watcher, pending) = watch_scoped_tickets(
            &store,
            &canonical.join("tickets"),
            TicketsWatchScope::new(&canonical, true),
            &log,
            &events,
            start,
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        while !watcher.is_live() {
            assert!(Instant::now() < deadline, "the watch never went live");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(*watched.lock_or_recover(), std::slice::from_ref(&canonical));
        let recorded = |id: &str| {
            log.lock_or_recover()
                .since(0)
                .0
                .iter()
                .any(|event| event.kind == TARGET_STORE_CHANGE_KIND && event.id == id)
        };
        // A ULID-named file outside `tickets/` is not a ticket.
        let outside = Ulid::new().to_string();
        std::fs::write(git.join(format!("{outside}.md")), "synthetic").unwrap();
        let id = Ulid::new().to_string();
        std::fs::write(shard.join(format!("{id}.md")), "---\n---\n").unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        while !recorded(&id) {
            assert!(Instant::now() < deadline, "the write was never recorded");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(!recorded(&outside));
        std::fs::rename(&root, dir.path().join("moved")).expect("a watched store can move");
        drop((watcher, pending));
    }

    #[test]
    fn hs2_g5fyp6_changed_tickets_reads_ulid_files_and_shard_directories() {
        let dir = tempfile::tempdir().unwrap();
        let shard = dir.path().join("01");
        std::fs::create_dir(&shard).unwrap();
        let first = Ulid::new().to_string();
        let second = Ulid::new().to_string();
        std::fs::write(shard.join(format!("{first}.md")), "x").unwrap();
        std::fs::write(shard.join("notes.txt"), "x").unwrap();
        let ids = changed_tickets(vec![
            shard.clone(),
            dir.path().join(format!("{second}.md")),
            dir.path().join(format!("{first}.md")),
            dir.path().join("not-a-ulid.md"),
        ]);
        let mut want = vec![first, second];
        want.sort();
        assert_eq!(ids, want);
    }
}
