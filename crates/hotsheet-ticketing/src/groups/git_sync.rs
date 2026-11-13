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

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};
use std::sync::{Condvar, Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

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

/// The registry files, as a pathspec relative to the checkout root.
const REGISTRY_PATHSPEC: &str = ":(glob).hotsheet2/groups/*.json";
/// A probe path for `git check-ignore`; ids are never all zeros in practice, and the
/// check does not need the file to exist.
const IGNORE_PROBE: &str = ".hotsheet2/groups/0000000000.json";

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

/// The last recorded status of the registry under `checkout_root`, if any.
pub fn read_status(checkout_root: &Path) -> Option<RegistrySyncStatus> {
    let bytes = fs::read(checkout_root.join(GROUP_SYNC_STATUS_FILE)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn record_status(checkout_root: &Path, status: &mut RegistrySyncStatus) {
    status.at = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default();
    let path = checkout_root.join(GROUP_SYNC_STATUS_FILE);
    // A registry Hot Sheet does not publish has no publication status to show.
    if status.state == RegistrySyncState::Untracked {
        let _ = fs::remove_file(&path);
        return;
    }
    // Never create `.hotsheet2/` just to record a status.
    if !path.parent().is_some_and(Path::is_dir) {
        return;
    }
    if let Ok(mut text) = serde_json::to_string_pretty(status) {
        text.push('\n');
        let _ = crate::store::write_file_atomically(&path, text.as_bytes());
    }
}

// ---- registry writes ------------------------------------------------------------------

/// Called by the registry after it wrote an entry: commit (and push) now, or queue the
/// commit for the background committer when the process enabled one.
pub(crate) fn registry_written(checkout_root: &Path, message: &str) {
    if let Some(background) = BACKGROUND.get() {
        background.submit(checkout_root, message);
    } else {
        publish(checkout_root, Some(message));
    }
}

/// Commit the registry's pending changes with `message` (or [`PENDING_COMMIT_MESSAGE`])
/// and push them when that is safe. Records and returns the status. Never fails: every
/// problem is a status.
pub fn publish(checkout_root: &Path, message: Option<&str>) -> RegistrySyncStatus {
    let mut status = match Checkout::open(checkout_root) {
        Err(status) => status,
        Ok(checkout) => {
            let committed = checkout.commit(message.unwrap_or(PENDING_COMMIT_MESSAGE));
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
    record_status(checkout_root, &mut status);
    status
}

/// One sync-loop pass for a checkout: commit anything pending, fetch the upstream branch,
/// refresh the remote overlay, and push. Never fails: every problem is a status.
pub fn sync_checkout(checkout_root: &Path) -> RegistrySyncStatus {
    let mut status = match Checkout::open(checkout_root) {
        Err(status) => status,
        Ok(checkout) => {
            let committed = checkout.commit(PENDING_COMMIT_MESSAGE);
            // Pull even when the commit failed (a refusing hook must not hide other
            // clones' groups). Offline is not fatal: the overlay keeps its last view and
            // the push reports the failure.
            if let Some(upstream) = checkout.upstream() {
                if checkout.fetch(&upstream) {
                    checkout.refresh_overlay(&upstream);
                }
            } else {
                remove_overlay(checkout_root);
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
    record_status(checkout_root, &mut status);
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
    pending: HashMap<PathBuf, Burst>,
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
fn coalesced_message(messages: &[String]) -> String {
    match messages {
        [] => PENDING_COMMIT_MESSAGE.to_string(),
        [one] => one.clone(),
        [first, rest @ ..] => format!("{first} (+{} more)", rest.len()),
    }
}

impl Background {
    fn lock(&self) -> std::sync::MutexGuard<'_, BackgroundState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn submit(&self, root: &Path, message: &str) {
        let now = Instant::now();
        let mut state = self.lock();
        let burst = state.pending.entry(root.to_path_buf()).or_insert(Burst {
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
    fn take_due(&self) -> (PathBuf, Vec<String>) {
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
            let (root, messages) = self.take_due();
            publish(&root, Some(&coalesced_message(&messages)));
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
    root: PathBuf,
    /// The registry directory relative to the repository's top level, with a trailing
    /// slash, as `git log --name-only` prints paths.
    registry_prefix: String,
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
    fn open(root: &Path) -> Result<Self, RegistrySyncStatus> {
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
            &["check-ignore", "-q", "--no-index", "--", IGNORE_PROBE],
        ) {
            return Err(RegistrySyncStatus::new(
                Untracked,
                "Git ignores .hotsheet2/groups/ in this checkout",
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
            root: root.to_path_buf(),
            registry_prefix: format!("{prefix}{GROUP_REGISTRY_DIR}/"),
        })
    }

    /// The registry files that differ from `HEAD` in the index or the working tree.
    fn changed_paths(&self) -> Vec<String> {
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
                REGISTRY_PATHSPEC,
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
                REGISTRY_PATHSPEC,
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
    /// whether a commit was made.
    fn commit(&self, message: &str) -> Result<bool, RegistrySyncStatus> {
        let failed = |output: &Output| {
            RegistrySyncStatus::new(
                RegistrySyncState::Failed,
                format!("could not commit the group registry: {}", tail(output)),
            )
        };
        // The same per-repository Git transaction lock the ticket store uses, so a
        // designated store inside this repository never races this commit.
        let _lock = crate::store::lock_git_transaction(&self.root).map_err(|error| {
            RegistrySyncStatus::new(RegistrySyncState::Failed, error.to_string())
        })?;
        let paths = self.changed_paths();
        if paths.is_empty() {
            return Ok(false);
        }
        let mut add = vec!["add", "-A", "--"];
        add.extend(paths.iter().map(String::as_str));
        let output = run(&self.root, &add).ok_or_else(|| {
            RegistrySyncStatus::new(RegistrySyncState::Failed, "could not run git")
        })?;
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
        let output = run(&self.root, &commit).ok_or_else(|| {
            RegistrySyncStatus::new(RegistrySyncState::Failed, "could not run git")
        })?;
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
            path.strip_prefix(&self.registry_prefix)
                .and_then(|name| name.strip_suffix(".json"))
                .is_some_and(|id| !id.contains('/'))
        })
    }

    /// Push the branch's unpublished registry commits when that is a safe fast-forward.
    fn push(&self) -> RegistrySyncStatus {
        use RegistrySyncState::{Blocked, Failed, LocalOnly, Synced, Waiting};
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
            return RegistrySyncStatus::new(Synced, "the group registry is published");
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
                "group registry commits are waiting behind other unpublished commits on this \
                 branch; push the branch to publish them",
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
                "the branch is behind its upstream; pull (with --rebase) to publish the group \
                 registry",
            );
        }
        let refspec = format!("{}:{}", upstream.branch, upstream.remote_ref);
        let Some(output) = run(
            &self.root,
            &["push", "--quiet", "--porcelain", &upstream.remote, &refspec],
        ) else {
            return RegistrySyncStatus::new(Failed, "could not run git push");
        };
        if output.status.success() {
            let count = ahead.lines().count();
            return RegistrySyncStatus::new(
                Synced,
                format!(
                    "published {count} group registry commit{}",
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
                "the upstream moved; pull (with --rebase) to publish the group registry",
            );
        }
        RegistrySyncStatus::new(
            Failed,
            format!("the group registry push failed: {}", tail(&output)),
        )
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
    if fs::create_dir_all(parent).is_err() {
        return;
    }
    if let Ok(mut text) = serde_json::to_string_pretty(overlay) {
        text.push('\n');
        let _ = crate::store::write_file_atomically(&path, text.as_bytes());
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
