//! Git publication of the project group registry (HS2-9EGWC4): commit every registry write
//! in the code checkout, push it to the branch's upstream, and see registry changes made in
//! other clones without moving the user's branch.
//!
//! The registry lives in the user's code checkout, so every step is careful not to disturb
//! other work there:
//!
//! - **Commit** only the changed `.hotsheet2/groups/*.json` files, with `git commit --only`,
//!   so other staged and unstaged changes stay exactly as they were. Repository hooks run;
//!   Hot Sheet never passes `--no-verify`.
//! - **Skip** (and retry on a later write or sync pass, never failing the registry write)
//!   when the checkout is not a Git work tree, the registry is ignored, autocommit is
//!   disabled (`HOTSHEET_NO_AUTOCOMMIT`), `HEAD` is detached, or Git is mid merge, rebase,
//!   cherry-pick, or revert.
//! - **Push** the current branch to its upstream only as a fast-forward that publishes
//!   registry commits alone: never when the branch is behind, when an unpublished commit is
//!   a merge, or when any unpublished commit touches another file (that is the user's work
//!   to publish). Never force. A refusal or a failing hook leaves the commit local and
//!   records a visible status.
//! - **Pull** by fetching the upstream branch and recording, in a machine-local overlay,
//!   the entries that upstream added or renamed and this checkout has not changed. The
//!   registry reads the overlay while the local file is unchanged; the files themselves
//!   arrive with the user's next pull.
//!
//! The server commits in the background, coalescing a burst of writes into one commit
//! ([`enable_background`]); the CLI and MCP commit as each write returns. The server's
//! sync loop retries the commit and push and refreshes the overlay ([`sync_checkout`]).
//! [`on_status_change`] tells a process when a recorded state changes, so the server can
//! announce it to open clients (HS2-1CWD0H).
//!
//! The milestone registry (HS2-9PYNCN, docs/38 §38.2) shares this machinery unchanged: each
//! step takes a [`RegistryKind`] naming the registry's directory, status file, and wording.
//! Group and milestone commits both count as registry-only commits for the push check, so
//! publishing one never waits behind the other. Only the group registry keeps a remote overlay.
//!
//! Concurrency (HS2-NM6E27): every commit step, from the background committer, the sync
//! loop, or a CLI or MCP write, holds the per-repository Git transaction lock the ticket
//! store's autocommit and sync also take. Hot Sheet's own reads (repository status, the
//! checkout watcher, code review) run without Git's optional locks
//! ([`crate::git::command`]), so a `git status` never holds `index.lock` against a commit.
//! A lock another program holds (the user's Git, an editor) is retried with bounded backoff
//! while it is young; a stale lock, a lock still held when the retries run out, and every
//! hook refusal are reported at once.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant, SystemTime};

use hotsheet_model::is_group_id;
use serde::{Deserialize, Serialize};

use super::registry::GROUP_REGISTRY_DIR;

/// Machine-local status of the last publication attempt, beside the registry lock.
/// `.hotsheet2/*` stays ignored, so it is never committed.
pub const GROUP_SYNC_STATUS_FILE: &str = ".hotsheet2/groups-sync.json";
/// Machine-local overlay of registry entries the upstream branch changed.
pub const GROUP_REMOTE_OVERLAY_FILE: &str = ".hotsheet2/groups-remote.json";
/// The commit message when a sync pass commits changes no write announced.
pub const PENDING_COMMIT_MESSAGE: &str = "Update group registry";

/// The milestone registry's directory (HS2-9PYNCN, docs/38 §38.2).
pub const MILESTONE_REGISTRY_DIR: &str = ".hotsheet2/milestones";
/// Machine-local status of the milestone registry's last publication attempt.
pub const MILESTONE_SYNC_STATUS_FILE: &str = ".hotsheet2/milestones-sync.json";
/// The commit message when a sync pass commits milestone changes no write announced.
pub const MILESTONE_PENDING_COMMIT_MESSAGE: &str = "Update milestone registry";

/// Which project registry a publication step works on. Both live in the code checkout,
/// one committed JSON file per entry, and publish by the same rules (docs/35 §35.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RegistryKind {
    /// Ticket groups, `.hotsheet2/groups/` (HS2-9KZSD4).
    Groups,
    /// Milestones, `.hotsheet2/milestones/` (HS2-9PYNCN).
    Milestones,
}

impl RegistryKind {
    /// Every registry; a push publishes commits that touch only these files.
    pub const ALL: [Self; 2] = [Self::Groups, Self::Milestones];

    /// The registry directory, relative to the checkout root.
    pub fn dir(self) -> &'static str {
        match self {
            Self::Groups => GROUP_REGISTRY_DIR,
            Self::Milestones => MILESTONE_REGISTRY_DIR,
        }
    }

    /// The machine-local publication status file.
    pub fn status_file(self) -> &'static str {
        match self {
            Self::Groups => GROUP_SYNC_STATUS_FILE,
            Self::Milestones => MILESTONE_SYNC_STATUS_FILE,
        }
    }

    /// The commit message when a sync pass commits changes no write announced.
    pub fn pending_message(self) -> &'static str {
        match self {
            Self::Groups => PENDING_COMMIT_MESSAGE,
            Self::Milestones => MILESTONE_PENDING_COMMIT_MESSAGE,
        }
    }

    /// The registry's name in status lines.
    fn noun(self) -> &'static str {
        match self {
            Self::Groups => "group registry",
            Self::Milestones => "milestone registry",
        }
    }

    /// The registry files, as a pathspec relative to the checkout root.
    fn pathspec(self) -> String {
        format!(":(glob){}/*.json", self.dir())
    }

    /// A probe path for `git check-ignore`; ids are never all zeros in practice, and the
    /// check does not need the file to exist.
    fn ignore_probe(self) -> String {
        format!("{}/0000000000.json", self.dir())
    }

    /// Whether a sync pass pulls the upstream's entries into a remote overlay. Only the
    /// group registry has one; milestone changes arrive with the user's pull, which the
    /// checkout watcher announces (docs/38 §38.6).
    fn has_overlay(self) -> bool {
        matches!(self, Self::Groups)
    }
}

/// How the registry's publication stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegistrySyncState {
    /// Every registry change is committed and published (or there is nothing to publish).
    Synced,
    /// Committed, but the branch has no upstream to publish to.
    LocalOnly,
    /// Git is busy (merge, rebase, detached `HEAD`, ...); retried on the next pass.
    Waiting,
    /// Committed, but publishing needs the user: the branch is behind or diverged, or other
    /// unpublished commits would go with it.
    Blocked,
    /// A commit or push failed, for example a hook refused it or the remote was offline.
    Failed,
    /// Not committed at all: no Git work tree, the registry is ignored, or autocommit is
    /// disabled.
    Untracked,
}

impl RegistrySyncState {
    /// The serialized name (`local_only`, ...).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Synced => "synced",
            Self::LocalOnly => "local_only",
            Self::Waiting => "waiting",
            Self::Blocked => "blocked",
            Self::Failed => "failed",
            Self::Untracked => "untracked",
        }
    }

    /// Whether a person should look at it.
    pub fn needs_attention(self) -> bool {
        matches!(self, Self::Blocked | Self::Failed)
    }
}

/// The result of one publication attempt, as recorded in [`GROUP_SYNC_STATUS_FILE`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistrySyncStatus {
    pub state: RegistrySyncState,
    /// One line for people: what happened or what is needed.
    pub detail: String,
    /// Whether this attempt made a commit.
    #[serde(default)]
    pub committed: bool,
    /// When it was recorded (RFC 3339, UTC).
    #[serde(default)]
    pub at: String,
}

impl RegistrySyncStatus {
    fn new(state: RegistrySyncState, detail: impl Into<String>) -> Self {
        Self {
            state,
            detail: detail.into(),
            committed: false,
            at: String::new(),
        }
    }
}

/// The last recorded status of the group registry under `checkout_root`, if any.
pub fn read_status(checkout_root: &Path) -> Option<RegistrySyncStatus> {
    read_status_of(RegistryKind::Groups, checkout_root)
}

/// The last recorded status of the `kind` registry under `checkout_root`, if any.
pub fn read_status_of(kind: RegistryKind, checkout_root: &Path) -> Option<RegistrySyncStatus> {
    let bytes = fs::read(checkout_root.join(kind.status_file())).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn record_status(kind: RegistryKind, checkout_root: &Path, status: &mut RegistrySyncStatus) {
    status.at = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default();
    // The background committer and a sync-loop pass can record at once; compare and write
    // under one lock so each state change is announced exactly once (HS2-1CWD0H).
    let _recording = RECORDING.lock().unwrap_or_else(PoisonError::into_inner);
    let before = read_status_of(kind, checkout_root).map(|status| status.state);
    let path = checkout_root.join(kind.status_file());
    let after = if status.state == RegistrySyncState::Untracked {
        // A registry Hot Sheet does not publish has no publication status to show.
        let _ = fs::remove_file(&path);
        None
    } else if !path.parent().is_some_and(Path::is_dir) {
        // Never create `.hotsheet2/` just to record a status.
        return;
    } else {
        let Ok(mut text) = serde_json::to_string_pretty(status) else {
            return;
        };
        text.push('\n');
        if crate::store::write_file_atomically(&path, text.as_bytes()).is_err() {
            return;
        }
        Some(status.state)
    };
    // The listeners announce `group_registry_sync`; a milestone status has no event of its
    // own yet (HS2-9PYNCN follow-up) and is read with the milestone list.
    if before != after && kind == RegistryKind::Groups {
        notify_status_change(checkout_root, after);
    }
}

// ---- status-change listeners (HS2-1CWD0H) ---------------------------------------------

/// Serializes [`record_status`]'s read, compare, and write.
static RECORDING: Mutex<()> = Mutex::new(());

/// Called with the checkout root and the registry's new visible state (`None` once there
/// is no status to show) whenever a recorded publication state changes.
type StatusListenerFn = dyn Fn(&Path, Option<RegistrySyncState>) + Send + Sync;

static LISTENERS: Mutex<Vec<(u64, Arc<StatusListenerFn>)>> = Mutex::new(Vec::new());
static NEXT_LISTENER: AtomicU64 = AtomicU64::new(0);

/// A registered status-change listener; dropping it unregisters the listener.
#[must_use = "dropping the guard unregisters the listener"]
pub struct StatusListener(u64);

impl Drop for StatusListener {
    fn drop(&mut self) {
        LISTENERS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|(id, _)| *id != self.0);
    }
}

/// Call `listener` whenever a registry's recorded publication state changes in this
/// process: after a write's commit (in the background committer or inline) or a sync pass.
/// A repeat of the same state never calls it. It runs on the thread that recorded the
/// status, so it must be quick and must not wait on registry work.
pub fn on_status_change(
    listener: impl Fn(&Path, Option<RegistrySyncState>) + Send + Sync + 'static,
) -> StatusListener {
    let id = NEXT_LISTENER.fetch_add(1, Ordering::Relaxed);
    LISTENERS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push((id, Arc::new(listener)));
    StatusListener(id)
}

fn notify_status_change(checkout_root: &Path, state: Option<RegistrySyncState>) {
    let listeners: Vec<Arc<StatusListenerFn>> = LISTENERS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .map(|(_, listener)| Arc::clone(listener))
        .collect();
    for listener in listeners {
        listener(checkout_root, state);
    }
}

// ---- registry writes ------------------------------------------------------------------

/// Called by the registry after it wrote an entry: commit (and push) now, or queue the
/// commit for the background committer when the process enabled one.
pub(crate) fn registry_written(kind: RegistryKind, checkout_root: &Path, message: &str) {
    if let Some(background) = BACKGROUND.get() {
        background.submit(kind, checkout_root, message);
    } else {
        publish_registry(kind, checkout_root, Some(message));
    }
}

/// Commit the group registry's pending changes with `message` (or
/// [`PENDING_COMMIT_MESSAGE`]) and push them when that is safe. Records and returns the
/// status. Never fails: every problem is a status.
pub fn publish(checkout_root: &Path, message: Option<&str>) -> RegistrySyncStatus {
    publish_registry(RegistryKind::Groups, checkout_root, message)
}

/// [`publish`] for any registry.
pub fn publish_registry(
    kind: RegistryKind,
    checkout_root: &Path,
    message: Option<&str>,
) -> RegistrySyncStatus {
    let mut status = match Checkout::open(kind, checkout_root) {
        Err(status) => status,
        Ok(checkout) => {
            let committed = checkout.commit(message.unwrap_or(kind.pending_message()));
            match committed {
                Err(status) => status,
                Ok(committed) => {
                    let mut status = checkout.push();
                    status.committed = committed;
                    status
                }
            }
        }
    };
    record_status(kind, checkout_root, &mut status);
    status
}

/// One sync-loop pass for a checkout's group registry: commit anything pending, fetch the
/// upstream branch, refresh the remote overlay, and push. Never fails: every problem is a
/// status.
pub fn sync_checkout(checkout_root: &Path) -> RegistrySyncStatus {
    sync_registry(RegistryKind::Groups, checkout_root)
}

/// [`sync_checkout`] for any registry. A registry without a remote overlay (milestones)
/// commits and pushes only, against the remote-tracking ref the group pass fetched.
pub fn sync_registry(kind: RegistryKind, checkout_root: &Path) -> RegistrySyncStatus {
    let mut status = match Checkout::open(kind, checkout_root) {
        Err(status) => status,
        Ok(checkout) => {
            let committed = checkout.commit(kind.pending_message());
            // Pull even when the commit failed (a refusing hook must not hide other
            // clones' groups). Offline is not fatal: the overlay keeps its last view and
            // the push reports the failure.
            if kind.has_overlay() {
                if let Some(upstream) = checkout.upstream() {
                    if checkout.fetch(&upstream) {
                        checkout.refresh_overlay(&upstream);
                    }
                } else {
                    remove_overlay(checkout_root);
                }
            }
            match committed {
                Err(status) => status,
                Ok(committed) => {
                    let mut status = checkout.push();
                    status.committed = committed;
                    status
                }
            }
        }
    };
    record_status(kind, checkout_root, &mut status);
    status
}

// ---- background committer -------------------------------------------------------------

static BACKGROUND: OnceLock<&'static Background> = OnceLock::new();

/// Quiet period after the last write before a burst is committed.
const BURST_QUIET: Duration = Duration::from_millis(400);
/// Longest a burst waits from its first write.
const BURST_MAX: Duration = Duration::from_secs(2);

/// Commit registry writes on a background thread, one commit per burst of writes to a
/// checkout. Long-running processes (the server) call this once at startup; it is
/// idempotent.
pub fn enable_background() {
    BACKGROUND.get_or_init(|| {
        let background: &'static Background = Box::leak(Box::new(Background {
            state: Mutex::new(BackgroundState::default()),
            changed: Condvar::new(),
        }));
        std::thread::Builder::new()
            .name("hotsheet-group-registry-sync".into())
            .spawn(move || background.work())
            .expect("spawn the group registry committer");
        background
    });
}

/// Block until every queued registry commit has run. A no-op without
/// [`enable_background`].
pub fn flush_background() {
    if let Some(background) = BACKGROUND.get() {
        background.flush();
    }
}

struct Background {
    state: Mutex<BackgroundState>,
    changed: Condvar,
}

#[derive(Default)]
struct BackgroundState {
    pending: HashMap<(RegistryKind, PathBuf), Burst>,
    busy: bool,
    flushing: usize,
}

struct Burst {
    messages: Vec<String>,
    first: Instant,
    last: Instant,
}

impl Burst {
    fn due(&self) -> Instant {
        (self.last + BURST_QUIET).min(self.first + BURST_MAX)
    }
}

/// One commit message for a burst: the first write's, counting the rest, like the ticket
/// store's coalesced autocommits.
fn coalesced_message(kind: RegistryKind, messages: &[String]) -> String {
    match messages {
        [] => kind.pending_message().to_string(),
        [one] => one.clone(),
        [first, rest @ ..] => format!("{first} (+{} more)", rest.len()),
    }
}

impl Background {
    fn lock(&self) -> std::sync::MutexGuard<'_, BackgroundState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn submit(&self, kind: RegistryKind, root: &Path, message: &str) {
        let now = Instant::now();
        let mut state = self.lock();
        let burst = state
            .pending
            .entry((kind, root.to_path_buf()))
            .or_insert(Burst {
                messages: Vec::new(),
                first: now,
                last: now,
            });
        burst.messages.push(message.to_string());
        burst.last = now;
        drop(state);
        self.changed.notify_all();
    }

    /// The next burst to commit: the earliest due one once it is due, or any burst at once
    /// while someone is flushing.
    fn take_due(&self) -> ((RegistryKind, PathBuf), Vec<String>) {
        let mut state = self.lock();
        loop {
            let next = state
                .pending
                .iter()
                .map(|(root, burst)| (burst.due(), root.clone()))
                .min();
            match next {
                Some((due, root)) if state.flushing > 0 || due <= Instant::now() => {
                    let burst = state.pending.remove(&root).expect("present");
                    state.busy = true;
                    return (root, burst.messages);
                }
                Some((due, _)) => {
                    let wait = due.saturating_duration_since(Instant::now());
                    state = self
                        .changed
                        .wait_timeout(state, wait)
                        .unwrap_or_else(PoisonError::into_inner)
                        .0;
                }
                None => {
                    state = self
                        .changed
                        .wait(state)
                        .unwrap_or_else(PoisonError::into_inner);
                }
            }
        }
    }

    fn work(&self) {
        loop {
            let ((kind, root), messages) = self.take_due();
            publish_registry(kind, &root, Some(&coalesced_message(kind, &messages)));
            self.lock().busy = false;
            self.changed.notify_all();
        }
    }

    fn flush(&self) {
        let mut state = self.lock();
        state.flushing += 1;
        self.changed.notify_all();
        while state.busy || !state.pending.is_empty() {
            state = self
                .changed
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
        state.flushing -= 1;
    }
}

// ---- Git lock contention (HS2-NM6E27) --------------------------------------------------

/// A failed commit attempt: its status, and the Git lock files it could not create.
#[derive(Debug)]
struct CommitFailure {
    status: RegistrySyncStatus,
    locks: Vec<PathBuf>,
}

impl From<RegistrySyncStatus> for CommitFailure {
    fn from(status: RegistrySyncStatus) -> Self {
        Self {
            status,
            locks: Vec::new(),
        }
    }
}

/// How many times a commit that met another program's Git lock is retried.
const LOCK_RETRIES: usize = 12;
/// The first wait before a retry; each later wait doubles, up to [`LOCK_RETRY_MAX_DELAY`].
const LOCK_RETRY_DELAY: Duration = Duration::from_millis(25);
const LOCK_RETRY_MAX_DELAY: Duration = Duration::from_millis(400);
/// A lock file older than this belongs to a crashed or killed Git process, not to one that
/// is running a short command; it is reported at once and never removed.
const STALE_LOCK_AGE: Duration = Duration::from_secs(60);

/// How many times a push that lost a ref race to a concurrent push is retried.
const PUSH_RACE_RETRIES: usize = 6;

/// Whether a failed push lost a ref update race to a concurrent push: the remote (or the
/// local remote-tracking ref) moved between the push reading it and locking it.
fn lost_ref_race(printed: &str) -> bool {
    printed.contains("cannot lock ref")
        || printed.contains("incorrect old value provided")
        || printed.contains("failed to update ref")
}

/// Run `attempt` until it succeeds or fails for a reason other than a live Git lock.
///
/// A failure that names no lock file (a hook refusal, a missing identity, ...) returns at
/// once. A failure on lock files is retried with bounded, doubling backoff while every
/// lock is gone or younger than [`STALE_LOCK_AGE`]; a stale lock returns at once with its
/// age, and a lock still held after [`LOCK_RETRIES`] returns the last failure. The next
/// write or sync pass tries again either way.
fn retry_live_lock(
    mut attempt: impl FnMut() -> Result<bool, CommitFailure>,
    mut wait: impl FnMut(Duration),
    mut now: impl FnMut() -> SystemTime,
) -> Result<bool, RegistrySyncStatus> {
    let mut delay = LOCK_RETRY_DELAY;
    let mut retries = LOCK_RETRIES;
    loop {
        let failure = match attempt() {
            Ok(committed) => return Ok(committed),
            Err(failure) => failure,
        };
        if failure.locks.is_empty() {
            return Err(failure.status);
        }
        let at = now();
        let stale = failure.locks.iter().find_map(|lock| {
            let modified = fs::metadata(lock).and_then(|meta| meta.modified()).ok()?;
            let age = at.duration_since(modified).unwrap_or_default();
            (age >= STALE_LOCK_AGE).then_some((lock, age))
        });
        if let Some((lock, age)) = stale {
            let mut status = failure.status;
            let minutes = age.as_secs() / 60;
            status.detail = format!(
                "{} ({} is {minutes} minute{} old: a Git process that crashed or was killed \
                 left it behind; remove it once no Git command is running)",
                status.detail,
                lock.display(),
                if minutes == 1 { "" } else { "s" },
            );
            return Err(status);
        }
        if retries == 0 {
            return Err(failure.status);
        }
        retries -= 1;
        wait(delay);
        delay = (delay * 2).min(LOCK_RETRY_MAX_DELAY);
    }
}

// ---- Git -------------------------------------------------------------------------------

/// The upstream of the checked-out branch.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Upstream {
    /// The local branch, `refs/heads/<name>`.
    branch: String,
    /// The remote's name (`origin`).
    remote: String,
    /// The branch on the remote (`refs/heads/main`).
    remote_ref: String,
    /// The local remote-tracking ref (`refs/remotes/origin/main`).
    tracking: String,
}

/// A checkout whose registry can be committed now.
struct Checkout {
    kind: RegistryKind,
    root: PathBuf,
    /// Every registry directory relative to the repository's top level, with a trailing
    /// slash, as `git log --name-only` prints paths. A push publishes commits that touch
    /// only files directly inside these.
    registry_prefixes: Vec<String>,
}

fn git(root: &Path) -> std::process::Command {
    let mut command = crate::git::command();
    command
        .arg("-C")
        .arg(root)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C")
        .stdin(Stdio::null());
    command
}

fn run(root: &Path, args: &[&str]) -> Option<Output> {
    git(root).args(args).output().ok()
}

fn stdout(root: &Path, args: &[&str]) -> Option<String> {
    run(root, args)
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
}

fn succeeds(root: &Path, args: &[&str]) -> bool {
    run(root, args).is_some_and(|output| output.status.success())
}

/// The last few lines Git (or a hook) printed, for a one-line status.
fn tail(output: &Output) -> String {
    let text = String::from_utf8_lossy(&output.stderr);
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let mut joined = lines[lines.len().saturating_sub(3)..].join(" / ");
    if joined.len() > 400 {
        let mut cut = 400;
        while !joined.is_char_boundary(cut) {
            cut -= 1;
        }
        joined.truncate(cut);
        joined.push('…');
    }
    joined
}

impl Checkout {
    /// The checkout, when its registry can be committed right now; otherwise the status
    /// that explains why not.
    fn open(kind: RegistryKind, root: &Path) -> Result<Self, RegistrySyncStatus> {
        use RegistrySyncState::{Untracked, Waiting};
        if std::env::var_os("HOTSHEET_NO_AUTOCOMMIT").is_some() {
            return Err(RegistrySyncStatus::new(
                Untracked,
                "autocommit is disabled (HOTSHEET_NO_AUTOCOMMIT)",
            ));
        }
        let lines = stdout(
            root,
            &[
                "rev-parse",
                "--is-inside-work-tree",
                "--absolute-git-dir",
                "--show-prefix",
            ],
        );
        let Some(lines) = lines else {
            return Err(RegistrySyncStatus::new(
                Untracked,
                "the checkout is not a Git work tree",
            ));
        };
        let mut lines = lines.lines();
        let (Some("true"), Some(git_dir)) = (lines.next(), lines.next()) else {
            return Err(RegistrySyncStatus::new(
                Untracked,
                "the checkout is not a Git work tree",
            ));
        };
        let prefix = lines.next().unwrap_or_default();
        if succeeds(
            root,
            &[
                "check-ignore",
                "-q",
                "--no-index",
                "--",
                &kind.ignore_probe(),
            ],
        ) {
            return Err(RegistrySyncStatus::new(
                Untracked,
                format!("Git ignores {}/ in this checkout", kind.dir()),
            ));
        }
        let git_dir = Path::new(git_dir);
        for (marker, what) in [
            ("MERGE_HEAD", "a merge"),
            ("rebase-merge", "a rebase"),
            ("rebase-apply", "a rebase"),
            ("CHERRY_PICK_HEAD", "a cherry-pick"),
            ("REVERT_HEAD", "a revert"),
        ] {
            if git_dir.join(marker).exists() {
                return Err(RegistrySyncStatus::new(
                    Waiting,
                    format!("waiting for {what} to finish"),
                ));
            }
        }
        if !succeeds(root, &["symbolic-ref", "-q", "HEAD"]) {
            return Err(RegistrySyncStatus::new(
                Waiting,
                "waiting for a branch to be checked out (HEAD is detached)",
            ));
        }
        Ok(Self {
            kind,
            root: root.to_path_buf(),
            registry_prefixes: RegistryKind::ALL
                .iter()
                .map(|registry| format!("{prefix}{}/", registry.dir()))
                .collect(),
        })
    }

    /// The registry files that differ from `HEAD` in the index or the working tree.
    fn changed_paths(&self) -> Vec<String> {
        let pathspec = self.kind.pathspec();
        let mut paths = Vec::new();
        let listings = [
            // Modified, deleted, and untracked (not ignored) in the working tree.
            vec![
                "ls-files",
                "-z",
                "--modified",
                "--deleted",
                "--others",
                "--exclude-standard",
                "--",
                &pathspec,
            ],
            // Staged but not committed.
            vec![
                "diff",
                "--cached",
                "--name-only",
                "--no-renames",
                "-z",
                "--relative",
                "--",
                &pathspec,
            ],
        ];
        for args in listings {
            for path in stdout(&self.root, &args).unwrap_or_default().split('\0') {
                if !path.is_empty() && !paths.iter().any(|known| known == path) {
                    paths.push(path.to_string());
                }
            }
        }
        paths
    }

    /// Commit the pending registry changes, leaving everything else as it was. Returns
    /// whether a commit was made. A Git lock another program holds briefly is waited out
    /// (HS2-NM6E27); a hook refusal or any other failure is returned at once.
    fn commit(&self, message: &str) -> Result<bool, RegistrySyncStatus> {
        retry_live_lock(
            || self.commit_once(message),
            std::thread::sleep,
            SystemTime::now,
        )
    }

    /// One commit attempt, under the per-repository Git transaction lock.
    fn commit_once(&self, message: &str) -> Result<bool, CommitFailure> {
        let failed = |output: &Output| CommitFailure {
            status: RegistrySyncStatus::new(
                RegistrySyncState::Failed,
                format!(
                    "could not commit the {}: {}",
                    self.kind.noun(),
                    tail(output)
                ),
            ),
            locks: crate::sync_locks::locks_in_output(&String::from_utf8_lossy(&output.stderr)),
        };
        let cannot_run = || {
            CommitFailure::from(RegistrySyncStatus::new(
                RegistrySyncState::Failed,
                "could not run git",
            ))
        };
        // The same per-repository Git transaction lock the ticket store uses, so a
        // designated store inside this repository never races this commit.
        let _lock = crate::store::lock_git_transaction(&self.root).map_err(|error| {
            CommitFailure::from(RegistrySyncStatus::new(
                RegistrySyncState::Failed,
                error.to_string(),
            ))
        })?;
        let paths = self.changed_paths();
        if paths.is_empty() {
            return Ok(false);
        }
        let mut add = vec!["add", "-A", "--"];
        add.extend(paths.iter().map(String::as_str));
        let output = run(&self.root, &add).ok_or_else(cannot_run)?;
        if !output.status.success() {
            return Err(failed(&output));
        }
        let mut quiet = vec!["diff", "--cached", "--quiet", "--"];
        quiet.extend(paths.iter().map(String::as_str));
        if succeeds(&self.root, &quiet) {
            return Ok(false);
        }
        let has_identity = stdout(&self.root, &["config", "user.email"])
            .is_some_and(|email| !email.trim().is_empty());
        let mut commit: Vec<&str> = if has_identity {
            Vec::new()
        } else {
            vec![
                "-c",
                "user.name=Hot Sheet",
                "-c",
                "user.email=hotsheet@localhost",
            ]
        };
        commit.extend(["commit", "-q", "--only", "-m", message, "--"]);
        commit.extend(paths.iter().map(String::as_str));
        let output = run(&self.root, &commit).ok_or_else(cannot_run)?;
        if !output.status.success() {
            return Err(failed(&output));
        }
        Ok(true)
    }

    /// The checked-out branch's upstream on a remote, if it has one.
    fn upstream(&self) -> Option<Upstream> {
        let branch = stdout(&self.root, &["symbolic-ref", "-q", "HEAD"])?
            .trim()
            .to_string();
        let format = "%(upstream)%00%(upstream:remotename)%00%(upstream:remoteref)";
        let line = stdout(
            &self.root,
            &["for-each-ref", &format!("--format={format}"), &branch],
        )?;
        let mut fields = line.trim_end_matches('\n').split('\0');
        let (Some(tracking), Some(remote), Some(remote_ref)) =
            (fields.next(), fields.next(), fields.next())
        else {
            return None;
        };
        // `.` is a local branch, not a remote; an empty field means no upstream.
        if tracking.is_empty() || remote.is_empty() || remote == "." || remote_ref.is_empty() {
            return None;
        }
        Some(Upstream {
            branch,
            remote: remote.to_string(),
            remote_ref: remote_ref.to_string(),
            tracking: tracking.to_string(),
        })
    }

    /// Fetch the upstream branch into its remote-tracking ref. Returns whether it worked.
    fn fetch(&self, upstream: &Upstream) -> bool {
        let refspec = format!("+{}:{}", upstream.remote_ref, upstream.tracking);
        succeeds(
            &self.root,
            &["fetch", "--quiet", "--no-tags", &upstream.remote, &refspec],
        )
    }

    /// Whether every path in `listing` (one per line) is a registry entry file.
    fn only_registry_paths(&self, listing: &str) -> bool {
        listing.lines().filter(|line| !line.is_empty()).all(|path| {
            self.registry_prefixes.iter().any(|prefix| {
                path.strip_prefix(prefix.as_str())
                    .and_then(|name| name.strip_suffix(".json"))
                    .is_some_and(|id| !id.contains('/'))
            })
        })
    }

    /// Push the branch's unpublished registry commits when that is a safe fast-forward.
    fn push(&self) -> RegistrySyncStatus {
        use RegistrySyncState::{Blocked, Failed, LocalOnly, Synced, Waiting};
        let noun = self.kind.noun();
        let Some(upstream) = self.upstream() else {
            return RegistrySyncStatus::new(
                LocalOnly,
                "committed locally; the branch has no upstream to push to",
            );
        };
        let range = format!("{}..HEAD", upstream.tracking);
        let Some(ahead) = stdout(&self.root, &["rev-list", &range]) else {
            return RegistrySyncStatus::new(
                Waiting,
                format!(
                    "waiting for {} to be fetched before publishing",
                    upstream.tracking
                ),
            );
        };
        if ahead.trim().is_empty() {
            return RegistrySyncStatus::new(Synced, format!("the {noun} is published"));
        }
        let merges = stdout(&self.root, &["rev-list", "--merges", &range]).unwrap_or_default();
        let touched = stdout(
            &self.root,
            &["log", "--format=", "--name-only", "--no-renames", &range],
        )
        .unwrap_or_default();
        if !merges.trim().is_empty() || !self.only_registry_paths(&touched) {
            return RegistrySyncStatus::new(
                Blocked,
                format!(
                    "{noun} commits are waiting behind other unpublished commits on this \
                     branch; push the branch to publish them"
                ),
            );
        }
        let behind = stdout(
            &self.root,
            &[
                "rev-list",
                "--count",
                &format!("HEAD..{}", upstream.tracking),
            ],
        )
        .and_then(|count| count.trim().parse::<u64>().ok())
        .unwrap_or(0);
        if behind > 0 {
            return RegistrySyncStatus::new(
                Blocked,
                format!(
                    "the branch is behind its upstream; pull (with --rebase) to publish the {noun}"
                ),
            );
        }
        let refspec = format!("{}:{}", upstream.branch, upstream.remote_ref);
        let args = ["push", "--quiet", "--porcelain", &upstream.remote, &refspec];
        // Another publish or sync pass in this checkout may push the same commits at the
        // same moment; the slower push then loses the remote's ref lock. A retry reads the
        // remote again and either pushes or finds the commits published (HS2-NM6E27).
        let mut delay = LOCK_RETRY_DELAY;
        let mut output = None;
        for retry in 0..=PUSH_RACE_RETRIES {
            if retry > 0 {
                std::thread::sleep(delay);
                delay = (delay * 2).min(LOCK_RETRY_MAX_DELAY);
            }
            let Some(attempt) = run(&self.root, &args) else {
                return RegistrySyncStatus::new(Failed, "could not run git push");
            };
            let raced = !attempt.status.success()
                && lost_ref_race(&format!(
                    "{}{}",
                    String::from_utf8_lossy(&attempt.stdout),
                    String::from_utf8_lossy(&attempt.stderr)
                ));
            output = Some(attempt);
            if !raced {
                break;
            }
        }
        let Some(output) = output else {
            return RegistrySyncStatus::new(Failed, "could not run git push");
        };
        if output.status.success() {
            let count = ahead.lines().count();
            return RegistrySyncStatus::new(
                Synced,
                format!(
                    "published {count} {noun} commit{}",
                    if count == 1 { "" } else { "s" }
                ),
            );
        }
        let printed = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        if printed.contains("[rejected]") || printed.contains("non-fast-forward") {
            return RegistrySyncStatus::new(
                Blocked,
                format!("the upstream moved; pull (with --rebase) to publish the {noun}"),
            );
        }
        RegistrySyncStatus::new(Failed, format!("the {noun} push failed: {}", tail(&output)))
    }

    /// The registry entry blobs of `rev`, id to object id.
    fn entries_at(&self, rev: &str) -> BTreeMap<String, String> {
        let listing = stdout(
            &self.root,
            &[
                "ls-tree",
                "-z",
                rev,
                "--",
                &format!("{GROUP_REGISTRY_DIR}/"),
            ],
        )
        .unwrap_or_default();
        listing
            .split('\0')
            .filter_map(|entry| {
                let (meta, path) = entry.split_once('\t')?;
                let mut meta = meta.split(' ');
                let (_, Some("blob"), Some(object)) = (meta.next(), meta.next(), meta.next())
                else {
                    return None;
                };
                let id = Path::new(path)
                    .file_name()?
                    .to_str()?
                    .strip_suffix(".json")
                    .filter(|id| is_group_id(id))?;
                Some((id.to_string(), object.to_string()))
            })
            .collect()
    }

    /// Record the entries the upstream added or renamed that this checkout has not
    /// changed since the two histories split. Each keeps the local file text it was
    /// computed against, so any later local edit wins.
    fn refresh_overlay(&self, upstream: &Upstream) {
        let theirs = self.entries_at(&upstream.tracking);
        let ours = self.entries_at("HEAD");
        let base = stdout(&self.root, &["merge-base", "HEAD", &upstream.tracking])
            .map(|sha| self.entries_at(sha.trim()))
            .unwrap_or_default();
        let mut entries = BTreeMap::new();
        for (id, object) in &theirs {
            let mine = ours.get(id);
            // Unchanged upstream, or changed here since the split (ours wins, as a merge
            // would ask the user).
            if mine == Some(object) || mine != base.get(id) {
                continue;
            }
            let Some(name) = stdout(&self.root, &["cat-file", "blob", object])
                .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
                .and_then(|value| {
                    hotsheet_model::normalize_group_name_lossy(value.get("name")?.as_str()?)
                })
            else {
                continue;
            };
            let local = fs::read_to_string(entry_path(&self.root, id)).ok();
            entries.insert(id.clone(), OverlayEntry { name, local });
        }
        write_overlay(&self.root, &Overlay { entries });
    }
}

// ---- remote overlay ---------------------------------------------------------------------

#[derive(Debug, Default, Serialize, Deserialize)]
struct Overlay {
    entries: BTreeMap<String, OverlayEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OverlayEntry {
    /// The upstream name.
    name: String,
    /// The local file text when the overlay was computed (`None`: no local file).
    local: Option<String>,
}

fn entry_path(root: &Path, id: &str) -> PathBuf {
    root.join(GROUP_REGISTRY_DIR).join(format!("{id}.json"))
}

fn write_overlay(root: &Path, overlay: &Overlay) {
    if overlay.entries.is_empty() {
        remove_overlay(root);
        return;
    }
    let path = root.join(GROUP_REMOTE_OVERLAY_FILE);
    let Some(parent) = path.parent() else {
        return;
    };
    // Best effort, and the next pass recomputes the overlay; until then group names show
    // without upstream changes, so a failure is reported (HS2-68JAY3).
    let written = fs::create_dir_all(parent).and_then(|()| {
        let mut text = serde_json::to_string_pretty(overlay).map_err(std::io::Error::from)?;
        text.push('\n');
        crate::store::write_file_atomically(&path, text.as_bytes())
    });
    if let Err(error) = written {
        eprintln!("warning: could not write {}: {error}", path.display());
    }
}

fn remove_overlay(root: &Path) {
    // Missing already, or unremovable: the next pass tries again.
    let _ = fs::remove_file(root.join(GROUP_REMOTE_OVERLAY_FILE));
}

/// The upstream names that apply now: entries whose local file is still exactly as it was
/// when the overlay was computed.
pub(crate) fn overlay_names(checkout_root: &Path) -> BTreeMap<String, String> {
    let Some(overlay) = fs::read(checkout_root.join(GROUP_REMOTE_OVERLAY_FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Overlay>(&bytes).ok())
    else {
        return BTreeMap::new();
    };
    overlay
        .entries
        .into_iter()
        .filter(|(id, entry)| {
            is_group_id(id) && fs::read_to_string(entry_path(checkout_root, id)).ok() == entry.local
        })
        .map(|(id, entry)| (id, entry.name))
        .collect()
}

#[cfg(test)]
#[path = "git_sync/tests.rs"]
mod tests;
