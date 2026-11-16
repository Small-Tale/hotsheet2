//! Git lock files that block a ticket-store sync (HS2-6ACT5Y, `docs/02` §2.12.2).
//!
//! Git serializes writers with `<name>.lock` files: `index.lock` for the index, and
//! `refs/.../<ref>.lock` for each ref it updates. A Git process that crashed or was killed
//! leaves its lock behind, and every later command that needs the same file fails with
//! `Unable to create '<path>.lock': File exists`. That is neither an unreachable remote
//! nor a ticket merge conflict, so sync reports it as [`crate::sync::SyncReport::Blocked`]
//! and names the lock files.
//!
//! Two checks find them:
//!
//! - [`held_locks`] looks for the locks a cycle needs before it starts any Git step (the
//!   index, `HEAD`, the branch, and its remote-tracking ref), so a stale lock never lets a
//!   rebase start and stop half-way;
//! - [`locks_in_output`] reads the lock paths from a failed Git step's output, for a lock
//!   another process took while the cycle ran.
//!
//! The file looks the same whether its owner crashed or is still running, and removing a
//! live process's lock can corrupt the index or a ref, so a lock is never deleted. Sync
//! moves one aside only when [`crate::sync_stale_locks`] proves it stale (HS2-T0V269);
//! otherwise the recovery is manual and spelled out in [`LOCK_RECOVERY_ACTION`].

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::sync::git_stdout;

/// The next step a blocked store's health reports.
pub const LOCK_RECOVERY_ACTION: &str = "No ticket merge conflict was found. Sync moves a lock \
     aside by itself once it is over 10 minutes old and no process has the store's Git \
     directory open; `hotsheet-cli sync --recover-stale-locks` runs that check now. \
     Otherwise make sure no Git process is running in the store (an editor's Git \
     integration, a terminal, or another Hot Sheet), then remove the stale lock file and run \
     `hotsheet-cli sync`.";

/// How many lock files a reason names before it summarizes the rest.
const MAX_NAMED_LOCKS: usize = 3;

/// The lock files the next cycle needs that exist now: the index, `HEAD`, the checked-out
/// branch, and its remote-tracking ref. One local `git rev-parse` call; no network.
#[must_use]
pub fn held_locks(store_path: &Path) -> Vec<PathBuf> {
    let mut names = vec!["index.lock".to_string(), "HEAD.lock".to_string()];
    if let Some(branch) = git_stdout(store_path, &["symbolic-ref", "--quiet", "HEAD"]) {
        names.push(format!("{}.lock", branch.trim()));
    }
    if let Some(tracking) = git_stdout(
        store_path,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            "--symbolic-full-name",
            "@{u}",
        ],
    ) && !tracking.trim().is_empty()
    {
        names.push(format!("{}.lock", tracking.trim()));
    }
    let mut args = vec!["rev-parse".to_string()];
    for name in &names {
        args.push("--git-path".to_string());
        args.push(name.clone());
    }
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let Some(paths) = git_stdout(store_path, &args) else {
        return Vec::new();
    };
    paths
        .lines()
        .map(|line| store_path.join(line.trim()))
        .filter(|path| path.is_file())
        .collect()
}

/// The local lock files a failed Git step's output says it could not create. Lines the
/// remote sent (`remote: ...`, `! [remote rejected] ...`) are the remote's own locks,
/// which a local recovery cannot fix, and are ignored.
#[must_use]
pub fn locks_in_output(stderr: &str) -> Vec<PathBuf> {
    const OPEN: &str = "Unable to create '";
    const CLOSE: &str = "': File exists";
    let mut found: Vec<PathBuf> = Vec::new();
    for line in stderr.lines() {
        let line = line.trim();
        if line.starts_with("remote:") || line.contains("[remote rejected]") {
            continue;
        }
        let Some(start) = line.find(OPEN).map(|at| at + OPEN.len()) else {
            continue;
        };
        let Some(length) = line[start..].find(CLOSE) else {
            continue;
        };
        let path = PathBuf::from(&line[start..start + length]);
        if path.extension().is_some_and(|ext| ext == "lock") && !found.contains(&path) {
            found.push(path);
        }
    }
    found
}

/// The sync health reason for a cycle a Git lock stopped during `step`: which lock files,
/// how old each is, and that no merge was attempted.
#[must_use]
pub fn blocked_reason(store_path: &Path, step: &str, locks: &[PathBuf]) -> String {
    blocked_reason_at(store_path, step, locks, SystemTime::now())
}

fn blocked_reason_at(store_path: &Path, step: &str, locks: &[PathBuf], now: SystemTime) -> String {
    let mut named: Vec<String> = locks
        .iter()
        .take(MAX_NAMED_LOCKS)
        .map(|lock| {
            let shown = display_path(store_path, lock);
            match age(lock, now) {
                Some(age) => format!("{shown} ({age})"),
                None => shown,
            }
        })
        .collect();
    if locks.len() > MAX_NAMED_LOCKS {
        named.push(format!("{} more", locks.len() - MAX_NAMED_LOCKS));
    }
    let noun = if locks.len() == 1 {
        "a Git lock file"
    } else {
        "Git lock files"
    };
    format!(
        "{noun} blocked sync during {step}: {}. Git leaves a lock behind when one of its \
         processes crashes or is killed; the store was not merged.",
        named.join(", ")
    )
}

/// A lock path relative to the store when it is inside it (`.git/index.lock`), else as Git
/// printed it (a linked worktree's Git directory lives elsewhere).
pub(crate) fn display_path(store_path: &Path, lock: &Path) -> String {
    let canonical_store = store_path.canonicalize().ok();
    let canonical_lock = lock
        .parent()
        .and_then(|parent| parent.canonicalize().ok())
        .zip(lock.file_name())
        .map(|(parent, name)| parent.join(name));
    match (canonical_store, canonical_lock) {
        (Some(store), Some(lock)) => lock.strip_prefix(&store).map_or_else(
            |_| lock.display().to_string(),
            |relative| relative.display().to_string(),
        ),
        _ => lock.display().to_string(),
    }
}

/// How long ago the lock file was created, rounded for a person: `created 3 hours ago`.
fn age(lock: &Path, now: SystemTime) -> Option<String> {
    let modified = std::fs::metadata(lock).ok()?.modified().ok()?;
    let seconds = now.duration_since(modified).ok()?.as_secs();
    let (count, unit) = if seconds < 60 {
        return Some("created just now".to_string());
    } else if seconds < 3600 {
        (seconds / 60, "minute")
    } else if seconds < 86_400 {
        (seconds / 3600, "hour")
    } else {
        (seconds / 86_400, "day")
    };
    let plural = if count == 1 { "" } else { "s" };
    Some(format!("created {count} {unit}{plural} ago"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hs2_6act5y_reads_local_lock_paths_from_git_output() {
        let fetch = "error: cannot lock ref 'refs/remotes/origin/main': Unable to create \
                     '/s/.git/refs/remotes/origin/main.lock': File exists.\n\nAnother git \
                     process seems to be running in this repository, or the lock file may be \
                     stale\n";
        assert_eq!(
            locks_in_output(fetch),
            [PathBuf::from("/s/.git/refs/remotes/origin/main.lock")]
        );
        let add = "fatal: Unable to create '/s/.git/index.lock': File exists.\n";
        assert_eq!(locks_in_output(add), [PathBuf::from("/s/.git/index.lock")]);
        // The same lock named twice is reported once.
        let twice = format!("{add}{add}");
        assert_eq!(locks_in_output(&twice).len(), 1);
    }

    #[test]
    fn hs2_6act5y_ignores_the_remotes_own_locks_and_other_failures() {
        let remote = "remote: error: cannot lock ref 'refs/heads/main': Unable to create \
                      '/srv/r.git/refs/heads/main.lock': File exists.\n \
                      ! [remote rejected] main -> main (reference already exists)\n";
        assert!(locks_in_output(remote).is_empty());
        assert!(locks_in_output("fatal: unable to access 'x': Could not resolve host").is_empty());
        assert!(
            locks_in_output("error: cannot lock ref 'refs/heads/main': is at a but expected b")
                .is_empty()
        );
        // Not a lock file: never named as one.
        assert!(
            locks_in_output("fatal: Unable to create '/s/.git/objects/x': File exists.").is_empty()
        );
    }

    #[test]
    fn hs2_6act5y_reason_names_locks_relative_to_the_store_with_their_age() {
        let dir = tempfile::tempdir().unwrap();
        let git = dir.path().join(".git");
        std::fs::create_dir_all(&git).unwrap();
        let lock = git.join("index.lock");
        std::fs::write(&lock, "").unwrap();
        let later = SystemTime::now() + std::time::Duration::from_secs(3 * 3600 + 5);
        let reason = blocked_reason_at(dir.path(), "fetch", std::slice::from_ref(&lock), later);
        assert!(
            reason.starts_with(
                "a Git lock file blocked sync during fetch: .git/index.lock (created 3 hours ago)."
            ),
            "{reason}"
        );
        let many: Vec<PathBuf> = (0..5).map(|n| git.join(format!("l{n}.lock"))).collect();
        let reason = blocked_reason_at(dir.path(), "push", &many, later);
        assert!(
            reason.starts_with("Git lock files blocked sync during push"),
            "{reason}"
        );
        assert!(reason.contains("2 more"), "{reason}");
    }

    #[test]
    fn hs2_6act5y_age_rounds_for_a_person() {
        let dir = tempfile::tempdir().unwrap();
        let lock = dir.path().join("x.lock");
        std::fs::write(&lock, "").unwrap();
        let now = SystemTime::now();
        let at = |seconds| age(&lock, now + std::time::Duration::from_secs(seconds));
        assert_eq!(at(5).as_deref(), Some("created just now"));
        assert_eq!(at(61).as_deref(), Some("created 1 minute ago"));
        assert_eq!(at(2 * 86_400 + 1).as_deref(), Some("created 2 days ago"));
        assert_eq!(age(&dir.path().join("missing.lock"), now), None);
    }
}
