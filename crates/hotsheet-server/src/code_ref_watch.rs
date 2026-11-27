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

/// HS2-BN9MMJ: reads the watched state once and records what changed since the last read.
type Reconcile = Arc<dyn Fn() + Send + Sync>;

struct Watch {
    watcher: BackgroundWatcher,
    /// HS2-BN9MMJ: run at each later request for the watch, so a stream that lags behind
    /// (an `fseventsd` backlog delays events by minutes) delays a push or a pool change by
    /// no more than the follower's next request.
    reconcile: Reconcile,
    /// Set once the stream is live and the read that follows going live is done.
    live: Arc<AtomicBool>,
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
                // Until the stream is live the pending re-check reads the refs.
                let reconcile = watch
                    .live
                    .load(Ordering::SeqCst)
                    .then(|| watch.reconcile.clone());
                drop(watches);
                if let Some(reconcile) = reconcile {
                    reconcile();
                }
                return;
            }
        }
        let Some(common) = git_common_dir(root) else {
            return;
        };
        let pool = Some(common.join(POOL_DIRECTORY)).filter(|pool| pool.is_dir());
        let RefWatch {
            watcher,
            reconcile,
            live,
            pending,
        } = watch_refs(checkout_id, &common, event_log, events, start);
        // A concurrent request may have started the same watch; keep the first.
        self.watches
            .lock_or_recover()
            .entry(checkout_id.to_owned())
            .or_insert(Watch {
                watcher,
                reconcile,
                live,
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

    /// Whether the watch of `checkout_id` is live and has read the refs since.
    #[cfg(test)]
    fn live(&self, checkout_id: &str) -> bool {
        self.watches
            .lock_or_recover()
            .get(checkout_id)
            .is_some_and(|watch| watch.live.load(Ordering::SeqCst))
    }
}

/// One checkout's running watch, as [`watch_refs`] starts it.
struct RefWatch {
    watcher: BackgroundWatcher,
    reconcile: Reconcile,
    live: Arc<AtomicBool>,
    /// The pending re-check ends once this is dropped.
    pending: Arc<()>,
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
///
/// HS2-BN9MMJ: a stream event under the remote-tracking refs or the pool reads the state
/// too, instead of recording a change by the event's path alone, so each change is recorded
/// once whether the stream, the pending re-check or a later request's reconcile (the
/// returned [`Reconcile`]) notices it first, and an event whose paths only look like
/// remote-tracking state records nothing while the refs are unchanged.
fn watch_refs(
    checkout_id: &str,
    common: &Path,
    event_log: &Arc<Mutex<EventLog>>,
    events: &broadcast::Sender<ChangeEvent>,
    start: StartStream,
) -> RefWatch {
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
            // Read under the lock: the stream, the re-check and a request may read at once,
            // and an older reading must never replace a newer one.
            let mut last = seen.lock_or_recover();
            let now = (fingerprint(&common), pool_fingerprint(&common));
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
    let on_event = read.clone();
    let handler = move |delivery: Delivery| {
        let Ok(event) = delivery else { return };
        if matches!(event.kind, EventKind::Access(_)) {
            return;
        }
        // A rescan notice (the platform dropped events, as a backlogged service does)
        // reads the state too, whatever path it names.
        if event.need_rescan()
            || event
                .paths
                .iter()
                .any(|path| is_remote_tracking(&root, path) || is_pool(&root, path))
        {
            on_event();
        }
    };
    let live = Arc::new(AtomicBool::new(false));
    let on_status = read.clone();
    let went_live = live.clone();
    let status = move |status: WatchStatus| {
        on_status();
        // HS2-BN9MMJ: live only after the read that covers the start.
        if status == WatchStatus::Live {
            went_live.store(true, Ordering::SeqCst);
        }
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
    let reconcile: Reconcile = Arc::new(read.clone());
    let watch_live = live.clone();
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
    RefWatch {
        watcher,
        reconcile,
        live: watch_live,
        pending,
    }
}

/// What the remote-tracking state of the common directory `common` looks like: each loose
/// ref under `refs/remotes` (not Git's `.lock` files), with its size and modification
/// time, and the remote-tracking lines of `packed-refs`. Read from the local disk only.
///
/// HS2-BN9MMJ: only the `refs/remotes/` lines of `packed-refs` count (with the peeled line
/// that follows one), so packing local branches or tags is no push.
#[derive(Debug, PartialEq, Eq)]
struct Fingerprint {
    loose: Vec<(PathBuf, u64, Option<std::time::SystemTime>)>,
    packed: Vec<String>,
}

fn fingerprint(common: &Path) -> Fingerprint {
    let mut loose = Vec::new();
    let mut directories = vec![common.join("refs").join("remotes")];
    while let Some(directory) = directories.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                directories.push(path);
            } else if path
                .extension()
                .is_some_and(|extension| extension == "lock")
            {
                // HS2-YRFW31: Git's lock file for a ref it is writing (no ref name ends in
                // `.lock`). Counting it recorded one push up to three times as the stream
                // read between Git creating, writing and renaming it.
                continue;
            } else if let Ok(meta) = std::fs::metadata(&path) {
                loose.push((path, meta.len(), meta.modified().ok()));
            }
        }
    }
    loose.sort();
    let packed = std::fs::read_to_string(common.join("packed-refs"))
        .map(|text| remote_tracking_lines(&text))
        .unwrap_or_default();
    Fingerprint { loose, packed }
}

/// HS2-BN9MMJ: the `packed-refs` lines naming a remote-tracking ref, each with the peeled
/// (`^`) line that follows it.
fn remote_tracking_lines(text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut remote = false;
    for line in text.lines() {
        if line.starts_with('^') {
            if remote {
                lines.push(line.to_owned());
            }
            continue;
        }
        remote = line
            .split_once(' ')
            .is_some_and(|(_, name)| name.starts_with("refs/remotes/"));
        if remote {
            lines.push(line.to_owned());
        }
    }
    lines
}

/// HS2-ESVYQH: the coordination pool's state file, by its size and a hash of its
/// contents. HS2-BN9MMJ: not its modification time, which two same-size writes within
/// one filesystem timestamp tick share, so the second write would look unchanged.
fn pool_fingerprint(common: &Path) -> Option<(u64, u64)> {
    use std::hash::{Hash, Hasher};
    let bytes = std::fs::read(common.join(POOL_DIRECTORY).join("state.json")).ok()?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    Some((bytes.len() as u64, hasher.finish()))
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
    use hotsheet_jobs::watch::InProcessStream;

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

    /// How long a test on the in-process stream waits for an outcome (HS2-BN9MMJ): no
    /// `fseventsd` timing is involved, only this watch's own threads on a loaded runner.
    const IN_PROCESS_BOUND: Duration = Duration::from_secs(30);

    fn wait_within(bound: Duration, what: &str, mut done: impl FnMut() -> bool) {
        let deadline = Instant::now() + bound;
        while !done() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn wait_within_bound(what: &str, done: impl FnMut() -> bool) {
        wait_within(IN_PROCESS_BOUND, what, done);
    }

    fn canonical(path: &Path) -> PathBuf {
        hotsheet_sync::paths::canonicalize(path).unwrap()
    }

    /// Follow `clone` as "checkout" on the in-process `stream`, and wait until the watch is
    /// live and has read the refs once: every later change is left to the stream or a
    /// request's reconcile.
    fn follow_live(
        watches: &CodeRefWatches,
        stream: &InProcessStream,
        clone: &Path,
        log: &Arc<Mutex<EventLog>>,
        events: &broadcast::Sender<ChangeEvent>,
    ) {
        watches.follow_with("checkout", clone, log, events, stream.start_stream());
        wait_within_bound("the watch to go live", || watches.live("checkout"));
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
    /// a later write; it is never taken for a push. HS2-BN9MMJ: on the in-process stream,
    /// which reports each write; the pool's lock and temporary files are no change.
    #[test]
    fn hs2_esvyqh_a_pool_write_in_the_followed_checkout_is_recorded() {
        let dir = tempfile::tempdir().unwrap();
        let clone = repository(dir.path());
        let log = Arc::new(Mutex::new(EventLog::default()));
        let (events, _keep) = broadcast::channel(64);
        let watches = CodeRefWatches::default();
        let stream = InProcessStream::default();
        // Followed before the checkout has a pool: the common directory's own watch sees
        // it created, and the next request watches it too.
        follow_live(&watches, &stream, &clone, &log, &events);
        let common = canonical(&clone.join(".git"));
        let pool = common.join(POOL_DIRECTORY);
        std::fs::create_dir_all(&pool).unwrap();
        std::fs::write(pool.join("state.json"), b"{}").unwrap();
        stream.changed(&pool);
        assert_eq!(pool_changes(&log), 1, "the new pool is recorded");
        watches.follow_with("checkout", &clone, &log, &events, stream.start_stream());
        wait_within_bound("the pool to be watched", || {
            stream
                .watched()
                .contains(&(pool.clone(), RecursiveMode::NonRecursive))
        });
        let write = |text: &[u8]| {
            let mut temporary = tempfile::NamedTempFile::new_in(&pool).unwrap();
            std::io::Write::write_all(&mut temporary, text).unwrap();
            let staged = temporary.path().to_path_buf();
            hotsheet_sync::files::persist(temporary, pool.join("state.json")).unwrap();
            staged
        };
        for revision in 1..=3 {
            let staged = write(format!("{{\"revision\":{revision}}}").as_bytes());
            stream.changed(&staged);
            stream.changed(&pool.join("state.lock"));
            assert_eq!(
                pool_changes(&log),
                revision,
                "a temporary file is no change"
            );
            stream.changed(&pool.join("state.json"));
            assert_eq!(pool_changes(&log), revision + 1, "write {revision}");
        }
        assert_eq!(recorded(&log), 0, "a pool write is no push");
    }
    /// HS2-BN9MMJ: two same-size pool writes inside one filesystem timestamp tick are
    /// still two changes; CI's Linux runners recorded only the first.
    #[test]
    fn hs2_bn9mmj_a_same_size_pool_write_in_one_timestamp_tick_is_a_change() {
        let dir = tempfile::tempdir().unwrap();
        let pool = dir.path().join(POOL_DIRECTORY);
        std::fs::create_dir_all(&pool).unwrap();
        let state = pool.join("state.json");
        std::fs::write(&state, b"{\"revision\":1}").unwrap();
        let tick = std::fs::metadata(&state).unwrap().modified().unwrap();
        let first = pool_fingerprint(dir.path());
        std::fs::write(&state, b"{\"revision\":2}").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&state)
            .unwrap()
            .set_modified(tick)
            .unwrap();
        assert_eq!(std::fs::metadata(&state).unwrap().modified().unwrap(), tick);
        assert_ne!(pool_fingerprint(dir.path()), first);
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
    /// remote-tracking ref moving) is recorded, asking again keeps one watch, a local
    /// commit is not recorded, packing the refs is recorded, and a checkout that is not a
    /// repository is not watched. HS2-BN9MMJ: on the in-process stream, so no `fseventsd`
    /// timing is involved; the test reports the paths each Git command changes.
    #[test]
    fn hs2_xa9dza_a_push_from_the_followed_checkout_is_recorded() {
        let dir = tempfile::tempdir().unwrap();
        let clone = repository(dir.path());
        git(&clone, &["push", "-q", "origin", "main"]);
        let log = Arc::new(Mutex::new(EventLog::default()));
        let (events, _keep) = broadcast::channel(64);
        let watches = CodeRefWatches::default();
        let stream = InProcessStream::default();
        follow_live(&watches, &stream, &clone, &log, &events);
        watches.follow_with("checkout", &clone, &log, &events, stream.start_stream());
        watches.follow_with("plain", dir.path(), &log, &events, stream.start_stream());
        assert_eq!(watches.watched(), ["checkout"]);
        assert_eq!(stream.starts(), 1, "asking again keeps one watch");
        let common = canonical(&clone.join(".git"));
        assert_eq!(
            stream.watched(),
            [
                (common.join("refs"), RecursiveMode::Recursive),
                (common.clone(), RecursiveMode::NonRecursive)
            ]
        );
        assert_eq!(recorded(&log), 0, "starting the watch records nothing");

        git(&clone, &["push", "-q", "origin", "main:refs/heads/handoff"]);
        stream.changed(&common.join("refs/remotes/origin/handoff"));
        assert_eq!(recorded(&log), 1);
        let (events, _) = log.lock_or_recover().since(0);
        let pushed = events
            .iter()
            .find(|event| event.kind == CODE_REFS_CHANGE_KIND)
            .unwrap();
        assert_eq!(pushed.id, "checkout");
        assert!(pushed.slug.is_empty() && pushed.message.is_none());
        // The same change reported again is not another push.
        stream.changed(&common.join("refs/remotes/origin/handoff"));
        assert_eq!(recorded(&log), 1);
        // A local commit moves a local branch only.
        git(
            &clone,
            &["commit", "-q", "--allow-empty", "-m", "Synthetic local"],
        );
        stream.changed(&common.join("refs/heads/main"));
        assert_eq!(recorded(&log), 1, "a local commit is no push");
        // Packing the refs is remote-tracking state too.
        git(&clone, &["pack-refs", "--all"]);
        stream.changed(&common.join("packed-refs"));
        assert_eq!(recorded(&log), 2, "packed refs");
    }

    /// HS2-YRFW31: Git writes a remote-tracking ref through a lock file beside it (create
    /// `<ref>.lock`, write it, rename it onto `<ref>`), and inotify reports each step. A read
    /// while the lock exists sees no new ref, so one push is recorded once, however the
    /// stream's reads interleave with Git's steps.
    #[test]
    fn hs2_yrfw31_a_ref_lock_file_is_no_push() {
        let dir = tempfile::tempdir().unwrap();
        let clone = repository(dir.path());
        git(&clone, &["push", "-q", "origin", "main"]);
        let log = Arc::new(Mutex::new(EventLog::default()));
        let (events, _keep) = broadcast::channel(64);
        let watches = CodeRefWatches::default();
        let stream = InProcessStream::default();
        follow_live(&watches, &stream, &clone, &log, &events);
        let origin = canonical(&clone.join(".git")).join("refs/remotes/origin");
        let (lock, handoff) = (origin.join("handoff.lock"), origin.join("handoff"));
        let target = std::fs::read(origin.join("main")).unwrap();

        std::fs::File::create(&lock).unwrap();
        stream.changed(&lock);
        assert_eq!(recorded(&log), 0, "an empty lock file");
        std::fs::write(&lock, &target).unwrap();
        stream.changed(&lock);
        assert_eq!(recorded(&log), 0, "a written lock file");
        std::fs::rename(&lock, &handoff).unwrap();
        stream.changed(&lock);
        stream.changed(&handoff);
        assert_eq!(recorded(&log), 1, "the ref renamed into place");

        // A lock left behind by an interrupted update is no push either.
        std::fs::write(origin.join("main.lock"), &target).unwrap();
        stream.changed(&origin.join("main.lock"));
        assert_eq!(recorded(&log), 1, "a stale lock file");
    }

    /// HS2-BN9MMJ: a live stream that lags behind (held here, as a backlogged `fseventsd`
    /// held events for minutes) does not hold up a push or a pool change: the follower's
    /// next request reads the state and records each once; the late events, once
    /// delivered, record nothing twice; and a rescan notice (events the platform dropped)
    /// reads the state.
    #[test]
    fn hs2_bn9mmj_a_lagging_ref_stream_is_caught_up_by_the_next_request() {
        let dir = tempfile::tempdir().unwrap();
        let clone = repository(dir.path());
        git(&clone, &["push", "-q", "origin", "main"]);
        let log = Arc::new(Mutex::new(EventLog::default()));
        let (events, _keep) = broadcast::channel(64);
        let watches = CodeRefWatches::default();
        let stream = InProcessStream::default();
        follow_live(&watches, &stream, &clone, &log, &events);
        let common = canonical(&clone.join(".git"));
        stream.hold();

        git(&clone, &["push", "-q", "origin", "main:refs/heads/handoff"]);
        stream.changed(&common.join("refs/remotes/origin/handoff"));
        let pool = common.join(POOL_DIRECTORY);
        std::fs::create_dir_all(&pool).unwrap();
        std::fs::write(pool.join("state.json"), b"{}").unwrap();
        stream.changed(&pool);
        assert_eq!((recorded(&log), pool_changes(&log)), (0, 0));
        watches.follow_with("checkout", &clone, &log, &events, stream.start_stream());
        assert_eq!(
            (recorded(&log), pool_changes(&log)),
            (1, 1),
            "the next request reads the refs and the pool"
        );
        stream.release();
        assert_eq!(
            (recorded(&log), pool_changes(&log)),
            (1, 1),
            "the late events record nothing twice"
        );

        git(&clone, &["push", "-q", "origin", "main:refs/heads/later"]);
        stream.report(hotsheet_jobs::watch::rescan_event().add_path(common.clone()));
        assert_eq!(recorded(&log), 2, "a rescan notice reads the refs");
    }

    /// HS2-BN9MMJ: only the remote-tracking lines of `packed-refs` (with their peeled
    /// lines) are remote-tracking state: packing local branches and tags is no push.
    #[test]
    fn hs2_bn9mmj_only_remote_tracking_packed_lines_count() {
        let text = "# pack-refs with: peeled fully-peeled sorted \n\
                    1111111111111111111111111111111111111111 refs/heads/main\n\
                    2222222222222222222222222222222222222222 refs/remotes/origin/main\n\
                    3333333333333333333333333333333333333333 refs/tags/v1\n\
                    ^4444444444444444444444444444444444444444\n\
                    5555555555555555555555555555555555555555 refs/remotes/origin/v2\n\
                    ^6666666666666666666666666666666666666666\n";
        assert_eq!(
            remote_tracking_lines(text),
            [
                "2222222222222222222222222222222222222222 refs/remotes/origin/main",
                "5555555555555555555555555555555555555555 refs/remotes/origin/v2",
                "^6666666666666666666666666666666666666666",
            ]
        );
        assert!(remote_tracking_lines("").is_empty());
    }

    /// HS2-BN9MMJ: this module's real-stream smoke test. Once the watch is live (and has
    /// read the refs), a push is recorded only if the platform's own stream reports it.
    /// The bound fails only a delivery that never comes; a backlogged `fseventsd`
    /// delivered minutes late. HS2-YRFW31: the wait ends at the first record and only
    /// then asserts there is one, so a push recorded twice fails at once with its count
    /// instead of never matching and running out the bound.
    #[test]
    fn hs2_bn9mmj_smoke_a_real_stream_records_a_push() {
        let dir = tempfile::tempdir().unwrap();
        let clone = repository(dir.path());
        git(&clone, &["push", "-q", "origin", "main"]);
        let log = Arc::new(Mutex::new(EventLog::default()));
        let (events, _keep) = broadcast::channel(64);
        let watches = CodeRefWatches::default();
        watches.follow("checkout", &clone, &log, &events);
        wait_within(
            hotsheet_jobs::watch::REAL_STREAM_TEST_BOUND,
            "the real stream to go live",
            || watches.live("checkout"),
        );
        git(&clone, &["push", "-q", "origin", "main:refs/heads/handoff"]);
        wait_within(
            hotsheet_jobs::watch::REAL_STREAM_TEST_BOUND,
            "the real stream to report the push",
            || recorded(&log) >= 1,
        );
        assert_eq!(recorded(&log), 1, "one push is recorded once");
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
