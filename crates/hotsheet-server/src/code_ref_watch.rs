//! HS2-XA9DZA: pushes made from a followed checkout, as an event.
//!
//! A worker on another site of a project hands off by pushing a branch to the shared
//! code remote and noting it on its ticket (HS2-5P112Z). The note and the release are
//! ticket changes the orchestrator's follower sees; the push is not. When the push lands
//! after the note, nothing would ask the project's home to fetch it again once its timed
//! asks stopped (HS2-RQDH0Q).
//!
//! Nothing watches a remote without polling it, but the clone that pushed records each
//! successful push itself: it moves its remote-tracking ref (`refs/remotes/<remote>/…`,
//! or `packed-refs`) in its git common directory, which its worktrees share. While run
//! followers ask this server's target feed for a checkout, the feed keeps one file watch
//! on that directory (kernel events) and records each remote-tracking change as
//! a [`CODE_REFS_CHANGE_KIND`] event in the ordinary change log, so cursors and replay
//! work as for ticket changes. The event names the checkout only, never a ref or commit.
//! A push from another clone changes nothing here; a person's note then wakes the home.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use hotsheet_jobs::watch::{
    BackgroundWatcher, Delivery, EventKind, PENDING_RECHECK, PathWatcher, RecursiveMode,
    StartStream, WatchStatus,
};
use tokio::sync::broadcast;

use crate::events::{ChangeEvent, EventLog, emit_change};
use hotsheet_sync::LockExt;

/// The change kind the target feed reads for a followed checkout's remote-tracking refs.
pub(crate) const CODE_REFS_CHANGE_KIND: &str = "code_refs_changed";

/// HS2-ESVYQH: the change kind for a followed checkout's coordination pool
/// (`<git-common-dir>/hotsheet-coordination/state.json`), which an integrator's review
/// return changes without any ticket change after it.
pub(crate) const POOL_CHANGE_KIND: &str = "coordination_pool_changed";

/// HS2-ESVYQH: the directory of the coordination pool under the git common directory.
const POOL_DIRECTORY: &str = "hotsheet-coordination";

/// A watch nobody's feed asked for in this long is dropped at the next request.
const IDLE: Duration = Duration::from_secs(10 * 60);

struct Watch {
    watcher: BackgroundWatcher,
    /// HS2-ESVYQH: the coordination pool's directory, once it is watched. A pool
    /// created after the watch started is added at the next request.
    pool: Option<PathBuf>,
    common: PathBuf,
    /// The pending re-check ends once this is dropped.
    _pending: Arc<()>,
    asked: Instant,
}

/// The ref watches of followed checkouts, by checkout id.
#[derive(Default)]
pub(crate) struct CodeRefWatches {
    watches: Mutex<HashMap<String, Watch>>,
}

impl CodeRefWatches {
    /// Keep a watch on the git common directory of checkout `checkout_id` at `root`, only
    /// while feeds keep asking. A checkout that is not a git repository is not watched.
    /// Watches unasked for [`IDLE`] are dropped.
    pub(crate) fn follow(
        &self,
        checkout_id: &str,
        root: &Path,
        event_log: &Arc<Mutex<EventLog>>,
        events: &broadcast::Sender<ChangeEvent>,
    ) {
        self.follow_with(
            checkout_id,
            root,
            event_log,
            events,
            Box::new(PathWatcher::start),
        );
    }

    fn follow_with(
        &self,
        checkout_id: &str,
        root: &Path,
        event_log: &Arc<Mutex<EventLog>>,
        events: &broadcast::Sender<ChangeEvent>,
        start: StartStream,
    ) {
        let now = Instant::now();
        {
            let mut watches = self.watches.lock_or_recover();
            watches.retain(|_, watch| now.duration_since(watch.asked) < IDLE);
            if let Some(watch) = watches.get_mut(checkout_id) {
                watch.asked = now;
                let pool = watch.common.join(POOL_DIRECTORY);
                if watch.pool.is_none() && pool.is_dir() {
                    watch.watcher.add(&pool, RecursiveMode::NonRecursive);
                    watch.pool = Some(pool);
                }
                return;
            }
        }
        let Some(common) = git_common_dir(root) else {
            return;
        };
        let pool = Some(common.join(POOL_DIRECTORY)).filter(|pool| pool.is_dir());
        let (watcher, pending) = watch_refs(checkout_id, &common, event_log, events, start);
        // A concurrent request may have started the same watch; keep the first.
        self.watches
            .lock_or_recover()
            .entry(checkout_id.to_owned())
            .or_insert(Watch {
                watcher,
                pool,
                common,
                _pending: pending,
                asked: now,
            });
    }

    #[cfg(test)]
    pub(crate) fn watched(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.watches.lock_or_recover().keys().cloned().collect();
        ids.sort();
        ids
    }
}

/// The canonical git common directory of the repository at `root`: the `.git` directory
/// of a main checkout, or the one a linked worktree's `.git` file leads to. `None` for a
/// directory that is not a git checkout.
fn git_common_dir(root: &Path) -> Option<PathBuf> {
    let dot_git = root.join(".git");
    let git_dir = if dot_git.is_dir() {
        dot_git
    } else {
        let text = std::fs::read_to_string(&dot_git).ok()?;
        let linked = text.strip_prefix("gitdir:")?.trim();
        root.join(linked)
    };
    let common = match std::fs::read_to_string(git_dir.join("commondir")) {
        Ok(text) => git_dir.join(text.trim()),
        Err(_) => git_dir,
    };
    let common = hotsheet_sync::paths::canonicalize(common).ok()?;
    common.join("refs").is_dir().then_some(common)
}

/// Start the watch: `refs/` recursively and the common directory itself for
/// `packed-refs`. Each remote-tracking change the stream reports is recorded.
///
/// As for HS2-G5FYP6 and HS2-R49D43, a stream can take over a minute to start on a
/// loaded machine, and it does not report what changed before it was live. So the
/// remote-tracking state is fingerprinted when the watch is asked for, and until the
/// stream is live it is read again every [`PENDING_RECHECK`] (local files only, never the
/// remote); going live, or a refused start, reads it once more. A read records a change
/// only when the fingerprint differs from the last one, so nothing is recorded for a
/// watch that merely started.
fn watch_refs(
    checkout_id: &str,
    common: &Path,
    event_log: &Arc<Mutex<EventLog>>,
    events: &broadcast::Sender<ChangeEvent>,
    start: StartStream,
) -> (BackgroundWatcher, Arc<()>) {
    let (log, sender, checkout) = (event_log.clone(), events.clone(), checkout_id.to_owned());
    let emit_kind = Arc::new(move |kind: &'static str| {
        emit_change(
            &log,
            &sender,
            ChangeEvent {
                live: None,
                cursor: None,
                store: checkout.clone(),
                kind: kind.into(),
                id: checkout.clone(),
                slug: String::new(),
                message: None,
                activity: None,
                assignment: None,
                turn: None,
            },
        );
    });
    let seen = Arc::new(Mutex::new((fingerprint(common), pool_fingerprint(common))));
    let read = {
        let (seen, common, emit_kind) = (seen.clone(), common.to_path_buf(), emit_kind.clone());
        move || {
            let now = (fingerprint(&common), pool_fingerprint(&common));
            let mut last = seen.lock_or_recover();
            let (refs_moved, pool_moved) = (last.0 != now.0, last.1 != now.1);
            *last = now;
            drop(last);
            if refs_moved {
                emit_kind(CODE_REFS_CHANGE_KIND);
            }
            if pool_moved {
                emit_kind(POOL_CHANGE_KIND);
            }
        }
    };
    let root = common.to_path_buf();
    let handler = move |delivery: Delivery| {
        let Ok(event) = delivery else { return };
        if matches!(event.kind, EventKind::Access(_)) {
            return;
        }
        if event
            .paths
            .iter()
            .any(|path| is_remote_tracking(&root, path))
        {
            emit_kind(CODE_REFS_CHANGE_KIND);
        }
        if event.paths.iter().any(|path| is_pool(&root, path)) {
            emit_kind(POOL_CHANGE_KIND);
        }
    };
    let live = Arc::new(AtomicBool::new(false));
    let on_status = read.clone();
    let went_live = live.clone();
    let status = move |status: WatchStatus| {
        if status == WatchStatus::Live {
            went_live.store(true, Ordering::SeqCst);
        }
        on_status();
    };
    let refs = common.join("refs");
    // HS2-ESVYQH: the pool's directory when it exists; the common directory's own
    // watch sees it created.
    let pool = common.join(POOL_DIRECTORY);
    let mut paths = vec![
        (refs.as_path(), RecursiveMode::Recursive),
        (common, RecursiveMode::NonRecursive),
    ];
    if pool.is_dir() {
        paths.push((pool.as_path(), RecursiveMode::NonRecursive));
    }
    let watcher = BackgroundWatcher::start_with(&paths, handler, status, start);
    let pending = Arc::new(());
    let owner = Arc::downgrade(&pending);
    let spawned = std::thread::Builder::new()
        .name("code-ref-recheck".into())
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
        tracing::warn!(%error, "no pending re-check for a checkout's refs; it waits for its watch");
    }
    (watcher, pending)
}

/// What the remote-tracking state of the common directory `common` looks like: each
/// file under `refs/remotes` and `packed-refs`, with its size and modification time.
/// Read from the local disk only.
fn fingerprint(common: &Path) -> Vec<(PathBuf, u64, Option<std::time::SystemTime>)> {
    let mut found = Vec::new();
    let mut stamp = |path: PathBuf| {
        if let Ok(meta) = std::fs::metadata(&path) {
            found.push((path, meta.len(), meta.modified().ok()));
        }
    };
    stamp(common.join("packed-refs"));
    let mut directories = vec![common.join("refs").join("remotes")];
    while let Some(directory) = directories.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                directories.push(path);
            } else {
                stamp(path);
            }
        }
    }
    found.sort();
    found
}

/// HS2-ESVYQH: the coordination pool's state file, with its size and modification time.
fn pool_fingerprint(common: &Path) -> Option<(u64, Option<std::time::SystemTime>)> {
    let meta = std::fs::metadata(common.join(POOL_DIRECTORY).join("state.json")).ok()?;
    Some((meta.len(), meta.modified().ok()))
}

/// HS2-ESVYQH: whether a changed path under `common` is the coordination pool: its
/// directory (created), or its state file (a write renames a temporary file onto it).
/// Its lock and temporary files are not.
fn is_pool(common: &Path, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(common) else {
        return false;
    };
    let parts: Vec<&std::ffi::OsStr> = relative
        .components()
        .filter_map(|part| match part {
            Component::Normal(name) => Some(name),
            _ => None,
        })
        .collect();
    match parts.as_slice() {
        [directory] => *directory == POOL_DIRECTORY,
        [directory, file] => *directory == POOL_DIRECTORY && *file == "state.json",
        _ => false,
    }
}

/// Whether a changed path under the common directory `common` is remote-tracking state:
/// a file or directory under `refs/remotes`, or `packed-refs` (which holds packed
/// remote-tracking refs too). A local branch, `HEAD`, the index and objects are not.
fn is_remote_tracking(common: &Path, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(common) else {
        return false;
    };
    let parts: Vec<&std::ffi::OsStr> = relative
        .components()
        .filter_map(|part| match part {
            Component::Normal(name) => Some(name),
            _ => None,
        })
        .collect();
    match parts.as_slice() {
        [file] => file
            .to_str()
            .is_some_and(|name| name.starts_with("packed-refs")),
        [refs, remotes, ..] => *refs == "refs" && *remotes == "remotes",
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        let status = hotsheet_ticketing::git::launch("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .env("GIT_AUTHOR_NAME", "Synthetic")
            .env("GIT_AUTHOR_EMAIL", "synthetic@example.invalid")
            .env("GIT_COMMITTER_NAME", "Synthetic")
            .env("GIT_COMMITTER_EMAIL", "synthetic@example.invalid")
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    }

    fn recorded(log: &Arc<Mutex<EventLog>>) -> usize {
        log.lock_or_recover()
            .since(0)
            .0
            .iter()
            .filter(|event| event.kind == CODE_REFS_CHANGE_KIND && event.store == "checkout")
            .count()
    }

    fn until(log: &Arc<Mutex<EventLog>>, at_least: usize, what: &str) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while recorded(log) < at_least {
            assert!(Instant::now() < deadline, "{what} was never recorded");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// The paths a push, a fetch and local work change, sorted into remote-tracking
    /// state and everything else.
    #[test]
    fn hs2_xa9dza_only_remote_tracking_paths_count() {
        let common = Path::new("/repo/.git");
        for path in [
            "/repo/.git/refs/remotes/origin/hs2-branch",
            "/repo/.git/refs/remotes/origin/hs2-branch.lock",
            "/repo/.git/refs/remotes/origin",
            "/repo/.git/packed-refs",
            "/repo/.git/packed-refs.lock",
        ] {
            assert!(is_remote_tracking(common, Path::new(path)), "{path}");
        }
        for path in [
            "/repo/.git/refs/heads/main",
            "/repo/.git/refs/tags/v1",
            "/repo/.git/refs",
            "/repo/.git/HEAD",
            "/repo/.git/index",
            "/repo/.git/objects/ab/cdef",
            "/repo/.git/logs/refs/remotes/origin/hs2-branch",
            "/elsewhere/refs/remotes/origin/x",
        ] {
            assert!(!is_remote_tracking(common, Path::new(path)), "{path}");
        }
    }

    /// HS2-ESVYQH: only the coordination pool's directory and state file count as the
    /// pool; its lock and temporary files, and anything else, do not.
    #[test]
    fn hs2_esvyqh_only_the_pool_state_counts_as_a_pool_change() {
        let common = Path::new("/repo/.git");
        for path in [
            "/repo/.git/hotsheet-coordination",
            "/repo/.git/hotsheet-coordination/state.json",
        ] {
            assert!(is_pool(common, Path::new(path)), "{path}");
        }
        for path in [
            "/repo/.git/hotsheet-coordination/.tmpA1b2C3",
            "/repo/.git/hotsheet-coordination/state.lock",
            "/repo/.git/hotsheet-coordination/nested/state.json",
            "/repo/.git/refs/remotes/origin/main",
            "/repo/.git/state.json",
            "/elsewhere/hotsheet-coordination/state.json",
        ] {
            assert!(!is_pool(common, Path::new(path)), "{path}");
        }
    }

    fn pool_changes(log: &Arc<Mutex<EventLog>>) -> usize {
        log.lock_or_recover()
            .since(0)
            .0
            .iter()
            .filter(|event| event.kind == POOL_CHANGE_KIND && event.store == "checkout")
            .count()
    }

    /// HS2-ESVYQH: a followed checkout's pool state written (as a review return writes
    /// it, a temporary file renamed onto it) is recorded as a pool change, and again on
    /// a later write; it is never taken for a push.
    #[test]
    fn hs2_esvyqh_a_pool_write_in_the_followed_checkout_is_recorded() {
        let dir = tempfile::tempdir().unwrap();
        let clone = repository(dir.path());
        let log = Arc::new(Mutex::new(EventLog::default()));
        let (events, _keep) = broadcast::channel(64);
        let watches = CodeRefWatches::default();
        // Followed before the checkout has a pool: the next request after the pool is
        // created watches it too.
        watches.follow("checkout", &clone, &log, &events);
        let pool = clone.join(".git").join(POOL_DIRECTORY);
        std::fs::create_dir_all(&pool).unwrap();
        std::fs::write(pool.join("state.json"), b"{}").unwrap();
        watches.follow("checkout", &clone, &log, &events);
        let write = |text: &[u8]| {
            let mut temporary = tempfile::NamedTempFile::new_in(&pool).unwrap();
            std::io::Write::write_all(&mut temporary, text).unwrap();
            hotsheet_sync::files::persist(temporary, pool.join("state.json")).unwrap();
        };
        let deadline = Instant::now() + Duration::from_secs(30);
        // The stream may start after the first write; the re-check covers that.
        write(br#"{"revision":1}"#);
        while pool_changes(&log) < 1 {
            assert!(
                Instant::now() < deadline,
                "the pool write was never recorded"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        // A later write is recorded too, once the added watch applies; each write is
        // a change of its own, so the test writes until one is seen.
        let first = pool_changes(&log);
        let mut revision = 2;
        while pool_changes(&log) <= first {
            assert!(
                Instant::now() < deadline,
                "a later pool write was never recorded"
            );
            write(format!("{{\"revision\":{revision}}}").as_bytes());
            revision += 1;
            std::thread::sleep(Duration::from_millis(200));
        }
        assert_eq!(recorded(&log), 0, "a pool write is no push");
    }

    /// A main checkout and a linked worktree resolve to one common directory; a plain
    /// directory has none and is not watched.
    #[test]
    fn hs2_xa9dza_the_common_directory_of_a_checkout_and_its_worktree() {
        let dir = tempfile::tempdir().unwrap();
        let main = dir.path().join("main");
        std::fs::create_dir_all(&main).unwrap();
        git(&main, &["init", "-q", "-b", "main"]);
        git(&main, &["commit", "-q", "--allow-empty", "-m", "Synthetic"]);
        let linked = dir.path().join("linked");
        git(
            &main,
            &[
                "worktree",
                "add",
                "-q",
                linked.to_str().unwrap(),
                "-b",
                "work",
            ],
        );
        let common = hotsheet_sync::paths::canonicalize(main.join(".git")).unwrap();
        assert_eq!(git_common_dir(&main), Some(common.clone()));
        assert_eq!(git_common_dir(&linked), Some(common));
        assert_eq!(git_common_dir(dir.path()), None);
    }

    fn repository(dir: &Path) -> PathBuf {
        let remote = dir.join("remote.git");
        let clone = dir.join("clone");
        std::fs::create_dir_all(&clone).unwrap();
        git(dir, &["init", "-q", "--bare", remote.to_str().unwrap()]);
        git(&clone, &["init", "-q", "-b", "main"]);
        git(
            &clone,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git(
            &clone,
            &["commit", "-q", "--allow-empty", "-m", "Synthetic base"],
        );
        clone
    }

    /// Followed: starting the watch records nothing, a push from the checkout (its
    /// remote-tracking ref moving) is recorded, asking again keeps one watch, packing
    /// the refs is recorded, and a checkout that is not a repository is not watched.
    #[test]
    fn hs2_xa9dza_a_push_from_the_followed_checkout_is_recorded() {
        let dir = tempfile::tempdir().unwrap();
        let clone = repository(dir.path());
        git(&clone, &["push", "-q", "origin", "main"]);
        let log = Arc::new(Mutex::new(EventLog::default()));
        let (events, _keep) = broadcast::channel(64);
        let watches = CodeRefWatches::default();
        watches.follow("checkout", &clone, &log, &events);
        watches.follow("checkout", &clone, &log, &events);
        watches.follow("plain", dir.path(), &log, &events);
        assert_eq!(watches.watched(), ["checkout"]);

        git(&clone, &["push", "-q", "origin", "main:refs/heads/handoff"]);
        until(&log, 1, "the push");
        let (events, _) = log.lock_or_recover().since(0);
        let pushed = events
            .iter()
            .find(|event| event.kind == CODE_REFS_CHANGE_KIND)
            .unwrap();
        assert_eq!(pushed.id, "checkout");
        assert!(pushed.slug.is_empty() && pushed.message.is_none());
        let after_push = recorded(&log);
        // Packing the refs is remote-tracking state too.
        git(&clone, &["pack-refs", "--all"]);
        until(&log, after_push + 1, "packed refs");
    }

    /// While the stream has not started (over a minute on a loaded machine), a push is
    /// still recorded by the local re-check, once; a local commit is not; and dropping
    /// the watch ends the re-check.
    #[test]
    fn hs2_xa9dza_a_push_before_the_stream_is_live_is_recorded_by_the_recheck() {
        let dir = tempfile::tempdir().unwrap();
        let clone = repository(dir.path());
        let log = Arc::new(Mutex::new(EventLog::default()));
        let (events, _keep) = broadcast::channel(16);
        // A start that never finishes while the test runs.
        let (hold, held) = std::sync::mpsc::channel::<()>();
        let held = Mutex::new(held);
        let start: StartStream = Box::new(move |_, _| {
            let _ = held.lock_or_recover().recv();
            Err(hotsheet_jobs::watch::Error::generic("stopped"))
        });
        let watches = CodeRefWatches::default();
        watches.follow_with("checkout", &clone, &log, &events, start);
        git(
            &clone,
            &["commit", "-q", "--allow-empty", "-m", "Synthetic local"],
        );
        std::thread::sleep(PENDING_RECHECK * 3);
        assert_eq!(recorded(&log), 0, "a local commit is no push");
        git(&clone, &["push", "-q", "origin", "main"]);
        until(&log, 1, "the push");
        std::thread::sleep(PENDING_RECHECK * 3);
        assert_eq!(
            recorded(&log),
            1,
            "an unchanged state is not recorded again"
        );
        // Dropped, the watch stops reading.
        drop(watches);
        std::thread::sleep(PENDING_RECHECK * 2);
        git(&clone, &["push", "-q", "origin", "main:refs/heads/later"]);
        std::thread::sleep(PENDING_RECHECK * 3);
        assert_eq!(recorded(&log), 1);
        // Only now may the stand-in start end (as refused).
        drop(hold);
    }
}
