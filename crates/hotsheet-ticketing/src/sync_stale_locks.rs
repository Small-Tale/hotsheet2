//! Automatic recovery of provably stale Git locks in a ticket store (HS2-T0V269,
//! `docs/02` §2.12.2).
//!
//! HS2-6ACT5Y reports a held `index.lock`, `HEAD.lock`, branch lock, or remote-tracking ref
//! lock as `blocked`. A Git process that crashed or was killed leaves such a lock behind
//! for good, so a store would stay blocked until a person removed it. This module moves a
//! lock aside by itself, but only when it can prove that no process still owns it:
//!
//! 1. the lock file is at least [`STALE_LOCK_MIN_AGE`] old;
//! 2. no process has a file open under the store's Git directory or its common Git
//!    directory, apart from Hot Sheet's own coordination locks (`hotsheet-*.lock`), which
//!    the caller holds itself so no other Hot Sheet Git transaction is running;
//! 3. no `git` process has its working directory in the store, in any worktree of its
//!    repository, or in the Git directory.
//!
//! The process scan reads `/proc` on Linux and `lsof` on macOS and other Unix systems, for
//! the current user's processes. It must see this process's own open coordination lock;
//! when it cannot (a sandbox, a missing `lsof`), nothing is proven and nothing moves. On
//! Windows no scan is implemented, so recovery always refuses there and the lock stays a
//! manual recovery.
//!
//! A lock is never deleted. It is renamed into
//! `<common-git-dir>/hotsheet-stale-locks/<UTC timestamp>/`, keeping its path relative to
//! that directory, beside a `recovery.json` that records what was moved and why.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::sync::git_stdout;

/// How old a lock must be before sync may treat it as abandoned. A live Git command holds
/// its lock for milliseconds to seconds; ten minutes is far beyond any of them.
pub const STALE_LOCK_MIN_AGE: Duration = Duration::from_secs(10 * 60);

/// The directory under the common Git directory that keeps moved locks.
pub const STALE_LOCK_DIR: &str = "hotsheet-stale-locks";

/// How many moved locks a reason names before it summarizes the rest.
const MAX_NAMED: usize = 3;

/// One lock sync moved aside.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MovedLock {
    /// Where it was, store-relative when inside the store (`.git/index.lock`).
    pub from: String,
    /// Where it is now, shown the same way.
    pub to: String,
    /// How old the lock was when it was moved.
    pub age_seconds: u64,
}

/// A process that keeps sync from proving a lock stale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holder {
    pub pid: u32,
    pub command: String,
    /// `true` when it is a Git process working in the store; `false` when it has a file
    /// in the Git directory open.
    pub working_in_store: bool,
}

/// Why a held lock was not moved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kept {
    /// Younger than [`STALE_LOCK_MIN_AGE`].
    TooYoung { age_seconds: u64 },
    /// A process has the Git directory open or runs Git in the store.
    Held(Vec<Holder>),
    /// The process check could not run or could not be trusted.
    Unverifiable(String),
    /// The lock changed between the check and the move, or the move failed.
    NotMoved(String),
}

/// What one recovery pass did with the held locks it was given.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recovery {
    pub moved: Vec<MovedLock>,
    /// The lock as shown (store-relative) and why it stayed.
    pub kept: Vec<(String, Kept)>,
}

impl Recovery {
    /// One line per distinct reason a lock was kept, naming its locks, for a health reason
    /// (which is capped in length, so locks kept for the same reason share a line).
    #[must_use]
    pub fn kept_lines(&self) -> Vec<String> {
        let mut groups: Vec<(String, Vec<&str>)> = Vec::new();
        for (shown, why) in &self.kept {
            let text = kept_text(why);
            match groups.iter_mut().find(|(existing, _)| *existing == text) {
                Some((_, locks)) => locks.push(shown),
                None => groups.push((text, vec![shown])),
            }
        }
        groups
            .into_iter()
            .map(|(text, locks)| format!("Not moved automatically: {} {text}.", locks.join(", ")))
            .collect()
    }
}

/// Why a lock was kept, as a sentence tail.
#[must_use]
pub fn kept_text(why: &Kept) -> String {
    match why {
        Kept::TooYoung { .. } => format!(
            "is younger than {} minutes, so its Git process may still be running",
            STALE_LOCK_MIN_AGE.as_secs() / 60
        ),
        Kept::Held(holders) => {
            let named: Vec<String> = holders
                .iter()
                .take(MAX_NAMED)
                .map(|holder| {
                    let what = if holder.working_in_store {
                        "runs Git in the store"
                    } else {
                        "has the store's Git directory open"
                    };
                    format!("process {} ({}) {what}", holder.pid, holder.command)
                })
                .collect();
            let more = holders.len().saturating_sub(MAX_NAMED);
            let more = if more > 0 {
                format!(", and {more} more")
            } else {
                String::new()
            };
            format!("may still be in use: {}{more}", named.join("; "))
        }
        Kept::Unverifiable(why) => format!("was not proven stale: {why}"),
        Kept::NotMoved(why) => format!("could not be moved: {why}"),
    }
}

/// The reason line for locks a pass moved aside.
#[must_use]
pub fn moved_line(moved: &[MovedLock]) -> String {
    let named: Vec<String> = moved
        .iter()
        .take(MAX_NAMED)
        .map(|lock| format!("{} to {}", lock.from, lock.to))
        .collect();
    let more = moved.len().saturating_sub(MAX_NAMED);
    let more = if more > 0 {
        format!(" and {more} more")
    } else {
        String::new()
    };
    let noun = if moved.len() == 1 {
        "a stale Git lock"
    } else {
        "stale Git locks"
    };
    format!(
        "Moved {noun} aside: {}{more}. Each was over {} minutes old and no process had the \
         store's Git directory open.",
        named.join(", "),
        STALE_LOCK_MIN_AGE.as_secs() / 60
    )
}

/// Move aside every lock in `locks` that is provably stale. The caller holds the store's
/// Git transaction lock, so no Hot Sheet Git step runs, and passes each lock once per pass.
pub(crate) fn recover(store_path: &Path, locks: &[PathBuf]) -> Recovery {
    recover_with(store_path, locks, SystemTime::now(), &scan_processes)
}

type Scan = dyn Fn() -> Result<Vec<ProcessFiles>, String>;

fn recover_with(store_path: &Path, locks: &[PathBuf], now: SystemTime, scan: &Scan) -> Recovery {
    let mut recovery = Recovery::default();
    // Age first: it is free, and a young lock never needs the process scan.
    let mut candidates: Vec<(PathBuf, SystemTime, u64)> = Vec::new();
    for lock in locks {
        let shown = crate::sync_locks::display_path(store_path, lock);
        let Ok(modified) = std::fs::metadata(lock).and_then(|meta| meta.modified()) else {
            // Gone since the pre-check: nothing to move.
            continue;
        };
        let age_seconds = now.duration_since(modified).map_or(0, |age| age.as_secs());
        if age_seconds < STALE_LOCK_MIN_AGE.as_secs() {
            recovery.kept.push((shown, Kept::TooYoung { age_seconds }));
        } else {
            candidates.push((lock.clone(), modified, age_seconds));
        }
    }
    if candidates.is_empty() {
        return recovery;
    }
    let proof = Targets::for_store(store_path).and_then(|targets| {
        let processes = scan()?;
        holders(&processes, &targets, std::process::id()).map(|holders| (targets, holders))
    });
    let targets = match proof {
        Ok((_, holders)) if !holders.is_empty() => {
            for (lock, _, _) in &candidates {
                let shown = crate::sync_locks::display_path(store_path, lock);
                recovery.kept.push((shown, Kept::Held(holders.clone())));
            }
            return recovery;
        }
        Ok((targets, _)) => targets,
        Err(why) => {
            for (lock, _, _) in &candidates {
                let shown = crate::sync_locks::display_path(store_path, lock);
                recovery.kept.push((shown, Kept::Unverifiable(why.clone())));
            }
            return recovery;
        }
    };
    let Some(stash) = stash_dir(&targets.common_dir, now) else {
        for (lock, _, _) in &candidates {
            let shown = crate::sync_locks::display_path(store_path, lock);
            recovery.kept.push((
                shown,
                Kept::NotMoved(format!("could not create {STALE_LOCK_DIR}")),
            ));
        }
        return recovery;
    };
    let mut records = Vec::new();
    for (lock, modified, age_seconds) in candidates {
        let shown = crate::sync_locks::display_path(store_path, &lock);
        match move_aside(&lock, modified, &targets.common_dir, &stash) {
            Ok(Some(to)) => {
                records.push(RecoveryRecord {
                    from: lock.display().to_string(),
                    to: to.display().to_string(),
                    age_seconds,
                    created_at: rfc3339(modified),
                });
                recovery.moved.push(MovedLock {
                    from: shown,
                    to: crate::sync_locks::display_path(store_path, &to),
                    age_seconds,
                });
            }
            // Its owner released it meanwhile: nothing to move.
            Ok(None) => {}
            Err(why) => recovery.kept.push((shown, Kept::NotMoved(why))),
        }
    }
    if records.is_empty() {
        let _ = std::fs::remove_dir(&stash);
    } else {
        write_recovery_file(&stash, &records, now);
    }
    recovery
}

/// The paths the process scan compares against, canonical.
struct Targets {
    /// The Git directory and the common Git directory (equal outside a linked worktree).
    git_dirs: Vec<PathBuf>,
    common_dir: PathBuf,
    /// Every working tree of the repository (the store's and any linked worktrees).
    work_dirs: Vec<PathBuf>,
}

impl Targets {
    fn for_store(store_path: &Path) -> Result<Self, String> {
        let unreadable = || "could not read the store's Git directories".to_string();
        let git_dir = git_stdout(store_path, &["rev-parse", "--absolute-git-dir"])
            .map(|out| PathBuf::from(out.trim_end_matches(['\n', '\r'])))
            .and_then(|dir| dir.canonicalize().ok())
            .ok_or_else(unreadable)?;
        let common_dir = git_stdout(store_path, &["rev-parse", "--git-common-dir"])
            .map(|out| {
                let dir = PathBuf::from(out.trim_end_matches(['\n', '\r']));
                if dir.is_absolute() {
                    dir
                } else {
                    store_path.join(dir)
                }
            })
            .and_then(|dir| dir.canonicalize().ok())
            .ok_or_else(unreadable)?;
        let mut work_dirs: Vec<PathBuf> =
            git_stdout(store_path, &["worktree", "list", "--porcelain"])
                .unwrap_or_default()
                .lines()
                .filter_map(|line| line.strip_prefix("worktree "))
                .filter_map(|dir| Path::new(dir).canonicalize().ok())
                .collect();
        if let Ok(store) = store_path.canonicalize()
            && !work_dirs.contains(&store)
        {
            work_dirs.push(store);
        }
        let mut git_dirs = vec![git_dir];
        if !git_dirs.contains(&common_dir) {
            git_dirs.push(common_dir.clone());
        }
        Ok(Self {
            git_dirs,
            common_dir,
            work_dirs,
        })
    }

    fn in_git_dir(&self, path: &Path) -> bool {
        self.git_dirs.iter().any(|dir| path.starts_with(dir))
    }

    /// Hot Sheet's own cross-process coordination locks (`hotsheet-autocommit.lock`,
    /// `hotsheet-history-sync.lock`, ...), directly in a Git directory.
    fn is_coordination_lock(&self, path: &Path) -> bool {
        path.parent()
            .is_some_and(|parent| self.git_dirs.iter().any(|dir| dir == parent))
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("hotsheet-") && name.ends_with(".lock"))
    }
}

/// One process's working directory and open files, as the platform scan reports them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ProcessFiles {
    pub pid: u32,
    pub command: String,
    pub cwd: Option<PathBuf>,
    pub open: Vec<PathBuf>,
}

/// The processes that keep a lock from being proven stale. Fails when the scan did not see
/// `self_pid` holding its own coordination lock, because then it cannot be trusted to have
/// seen anyone else's files either.
fn holders(
    processes: &[ProcessFiles],
    targets: &Targets,
    self_pid: u32,
) -> Result<Vec<Holder>, String> {
    let saw_self = processes.iter().any(|process| {
        process.pid == self_pid
            && process
                .open
                .iter()
                .any(|path| targets.is_coordination_lock(path))
    });
    if !saw_self {
        return Err(
            "the process check could not see this process's own open files, so it \
                    cannot prove that no other process holds the lock"
                .to_string(),
        );
    }
    let mut found = Vec::new();
    for process in processes.iter().filter(|process| process.pid != self_pid) {
        let opens_git_dir = process
            .open
            .iter()
            .any(|path| targets.in_git_dir(path) && !targets.is_coordination_lock(path));
        let runs_git_here = process.cwd.as_deref().is_some_and(|cwd| {
            targets.in_git_dir(cwd)
                || (process.command.starts_with("git")
                    && targets.work_dirs.iter().any(|dir| cwd.starts_with(dir)))
        });
        if opens_git_dir || runs_git_here {
            found.push(Holder {
                pid: process.pid,
                command: process.command.clone(),
                working_in_store: !opens_git_dir,
            });
        }
    }
    Ok(found)
}

/// Every current-user process's working directory and open files.
#[cfg(target_os = "linux")]
fn scan_processes() -> Result<Vec<ProcessFiles>, String> {
    use std::os::unix::fs::MetadataExt as _;
    // SAFETY: geteuid takes no arguments and has no preconditions.
    let uid = unsafe { libc::geteuid() };
    let entries =
        std::fs::read_dir("/proc").map_err(|error| format!("could not read /proc: {error}"))?;
    let mut processes = Vec::new();
    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse().ok())
        else {
            continue;
        };
        let dir = entry.path();
        // Other accounts' processes are not readable; see the module docs.
        if !std::fs::metadata(&dir).is_ok_and(|meta| meta.uid() == uid) {
            continue;
        }
        let open = std::fs::read_dir(dir.join("fd"))
            .map(|fds| {
                fds.flatten()
                    .filter_map(|fd| std::fs::read_link(fd.path()).ok())
                    .collect()
            })
            .unwrap_or_default();
        processes.push(ProcessFiles {
            pid,
            command: std::fs::read_to_string(dir.join("comm"))
                .map(|comm| comm.trim().to_string())
                .unwrap_or_default(),
            cwd: std::fs::read_link(dir.join("cwd")).ok(),
            open,
        });
    }
    Ok(processes)
}

/// Every current-user process's working directory and open files, from `lsof`.
#[cfg(all(unix, not(target_os = "linux")))]
fn scan_processes() -> Result<Vec<ProcessFiles>, String> {
    // SAFETY: geteuid takes no arguments and has no preconditions.
    let uid = unsafe { libc::geteuid() };
    // A GUI app's PATH often lacks /usr/sbin, where macOS keeps lsof.
    let program = ["/usr/sbin/lsof", "/usr/bin/lsof"]
        .into_iter()
        .find(|path| Path::new(path).is_file())
        .unwrap_or("lsof");
    let output = crate::git::launch(program)
        .args(["-n", "-P", "-w", "-F", "pcfn", "-u"])
        .arg(uid.to_string())
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|error| format!("could not run lsof: {error}"))?;
    Ok(parse_lsof(&String::from_utf8_lossy(&output.stdout)))
}

/// No process scan on this platform: recovery is refused (see the module docs).
#[cfg(not(unix))]
fn scan_processes() -> Result<Vec<ProcessFiles>, String> {
    Err(
        "automatic recovery is not available on this platform, because Hot Sheet cannot \
         check which processes hold the store's Git directory"
            .to_string(),
    )
}

/// Parse `lsof -F pcfn` output: `p<pid>`, `c<command>`, then `f<fd>` and `n<name>` per file.
#[cfg(all(unix, any(test, not(target_os = "linux"))))]
fn parse_lsof(output: &str) -> Vec<ProcessFiles> {
    let mut processes: Vec<ProcessFiles> = Vec::new();
    let mut fd = String::new();
    for line in output.lines() {
        let Some(tag) = line.chars().next() else {
            continue;
        };
        let value = &line[tag.len_utf8()..];
        match tag {
            'p' => {
                if let Ok(pid) = value.parse() {
                    processes.push(ProcessFiles {
                        pid,
                        ..ProcessFiles::default()
                    });
                }
            }
            'c' => {
                if let Some(process) = processes.last_mut() {
                    process.command = value.to_string();
                }
            }
            'f' => value.clone_into(&mut fd),
            'n' => {
                if let Some(process) = processes.last_mut() {
                    // lsof appends " (deleted)" and similar notes after a space-free path
                    // only for special files; a path is kept as printed.
                    let path = PathBuf::from(value);
                    if fd == "cwd" {
                        process.cwd = Some(path);
                    } else if path.is_absolute() {
                        process.open.push(path);
                    }
                }
            }
            _ => {}
        }
    }
    processes
}

/// A new, empty `<common-dir>/hotsheet-stale-locks/<timestamp>` for this pass.
fn stash_dir(common_dir: &Path, now: SystemTime) -> Option<PathBuf> {
    let stamp = OffsetDateTime::from(now);
    let base = format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        stamp.year(),
        u8::from(stamp.month()),
        stamp.day(),
        stamp.hour(),
        stamp.minute(),
        stamp.second()
    );
    let root = common_dir.join(STALE_LOCK_DIR);
    std::fs::create_dir_all(&root).ok()?;
    for attempt in 1..100 {
        let name = if attempt == 1 {
            base.clone()
        } else {
            format!("{base}-{attempt}")
        };
        let dir = root.join(name);
        match std::fs::create_dir(&dir) {
            Ok(()) => return Some(dir),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return None,
        }
    }
    None
}

/// Rename `lock` into `stash`, keeping its path relative to the common Git directory.
/// `Ok(None)` when the lock is gone; an error when it changed since it was checked.
fn move_aside(
    lock: &Path,
    checked: SystemTime,
    common_dir: &Path,
    stash: &Path,
) -> Result<Option<PathBuf>, String> {
    let canonical = lock
        .parent()
        .and_then(|parent| parent.canonicalize().ok())
        .zip(lock.file_name())
        .map(|(parent, name)| parent.join(name));
    let relative = canonical
        .as_deref()
        .and_then(|path| path.strip_prefix(common_dir).ok())
        .map(Path::to_path_buf)
        .or_else(|| lock.file_name().map(PathBuf::from))
        .ok_or_else(|| "the lock has no file name".to_string())?;
    let to = stash.join(relative);
    match std::fs::metadata(lock).and_then(|meta| meta.modified()) {
        Ok(modified) if modified == checked => {}
        Ok(_) => return Err("it changed while it was being checked".to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    }
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    match std::fs::rename(lock, &to) {
        Ok(()) => Ok(Some(to)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

#[derive(Serialize)]
struct RecoveryRecord {
    from: String,
    to: String,
    age_seconds: u64,
    created_at: String,
}

/// `recovery.json` beside the moved locks: what moved, from where, and the proof.
fn write_recovery_file(stash: &Path, records: &[RecoveryRecord], now: SystemTime) {
    let body = serde_json::json!({
        "ticket": "HS2-T0V269",
        "moved_at": rfc3339(now),
        "proof": format!(
            "each lock was at least {} minutes old; no process had the Git directory open \
             and no Git process ran in the store",
            STALE_LOCK_MIN_AGE.as_secs() / 60
        ),
        "moved": records,
    });
    if let Ok(text) = serde_json::to_vec_pretty(&body) {
        let _ = std::fs::write(stash.join("recovery.json"), text);
    }
}

fn rfc3339(at: SystemTime) -> String {
    OffsetDateTime::from(at)
        .format(&Rfc3339)
        .unwrap_or_default()
}

#[cfg(test)]
pub(crate) mod tests;
