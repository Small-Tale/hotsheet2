//! Ticket writes saved while a rebase or merge is unfinished in the store (HS2-JGH11Q,
//! `docs/02` §2.12.2).
//!
//! [`crate::FsStore`] never commits onto the detached `HEAD` of an unfinished rebase or
//! into a merge (HS2-QR1K8R): the file is written and the commit is deferred, so the
//! change waits in the working tree. This module finds those deferred writes, reports
//! them through sync health, and carries them across the explicit recovery
//! ([`crate::sync::abort_stalled_integration`]), whose `git rebase --abort` is a hard
//! reset that would otherwise discard a deferred write to a tracked ticket file.
//!
//! A deferred write is an **unstaged** working-tree change or an untracked file inside the
//! store. Staged and unmerged content belongs to the rebase or merge itself (its current
//! pick or its conflict) and is left to the abort. Before anything is changed the writes
//! are recorded as a backup commit under `refs/hotsheet/sync-backup/`, so a crash between
//! the abort and the restore loses nothing.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use crate::sync::{StalledIntegrationError, git_run, git_stdout};

/// Deferred ticket writes recorded before an abort, and where they live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SavedWrites {
    /// The repository top level every path is relative to.
    top: PathBuf,
    /// The backup commit holding the writes (its parent is the `HEAD` they were saved on).
    commit: String,
    /// The ref keeping [`Self::commit`] reachable.
    pub(crate) backup_ref: String,
    /// Changed or created paths that existed in the index (restored from it to set aside).
    tracked: Vec<String>,
    /// Paths Git did not track (removed to set aside).
    untracked: Vec<String>,
}

impl SavedWrites {
    /// Every saved path, repository-relative.
    pub(crate) fn paths(&self) -> impl Iterator<Item = &String> {
        self.tracked.iter().chain(&self.untracked)
    }

    pub(crate) fn len(&self) -> usize {
        self.tracked.len() + self.untracked.len()
    }
}

/// The uncommitted writes waiting in the store: `(top level, tracked, untracked)`, with
/// tracked and untracked paths relative to the top level. Unmerged paths are excluded.
fn find(store_path: &Path) -> Option<(PathBuf, Vec<String>, Vec<String>)> {
    let top = PathBuf::from(
        git_stdout(store_path, &["rev-parse", "--show-toplevel"])?
            .trim_end_matches(['\n', '\r'])
            .to_string(),
    );
    let prefix = git_stdout(store_path, &["rev-parse", "--show-prefix"])?
        .trim_end_matches(['\n', '\r'])
        .to_string();
    let scope = if prefix.is_empty() { "." } else { &prefix };
    let names = |args: &[&str]| -> Vec<String> {
        let mut full = vec!["--literal-pathspecs"];
        full.extend_from_slice(args);
        full.extend_from_slice(&["--", scope]);
        git_stdout(&top, &full)
            .unwrap_or_default()
            .split('\0')
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .collect()
    };
    let unmerged = names(&["diff", "--name-only", "-z", "--diff-filter=U"]);
    let mut tracked = names(&["diff", "--name-only", "-z"]);
    tracked.retain(|path| !unmerged.contains(path));
    let untracked = names(&["ls-files", "-z", "--others", "--exclude-standard"]);
    Some((top, tracked, untracked))
}

/// How many uncommitted changes wait in the store's working tree. Sync health reports
/// this while a rebase or merge is unfinished: those are the writes whose commits were
/// deferred.
#[must_use]
pub fn pending_count(store_path: &Path) -> usize {
    find(store_path).map_or(0, |(_, tracked, untracked)| tracked.len() + untracked.len())
}

fn error(step: &'static str, detail: impl Into<String>) -> StalledIntegrationError {
    StalledIntegrationError::Git {
        step,
        detail: detail.into(),
    }
}

/// Record the deferred writes as a backup commit on a temporary index, without touching
/// the store's index or working tree. `None` when there are none.
pub(crate) fn save(store_path: &Path) -> Result<Option<SavedWrites>, StalledIntegrationError> {
    let Some((top, tracked, untracked)) = find(store_path) else {
        return Err(error(
            "status",
            "the store's working tree could not be read",
        ));
    };
    if tracked.is_empty() && untracked.is_empty() {
        return Ok(None);
    }
    let git_dir = git_stdout(&top, &["rev-parse", "--absolute-git-dir"])
        .map(|dir| PathBuf::from(dir.trim_end_matches(['\n', '\r'])))
        .ok_or_else(|| error("rev-parse", "the store has no Git directory"))?;
    let index = git_dir.join("hotsheet-saved-writes.index");
    let _ = std::fs::remove_file(&index);
    let run = |args: &[&str]| -> Result<String, StalledIntegrationError> {
        let out = crate::git::command()
            .arg("-C")
            .arg(&top)
            .args(crate::sync_replay::committer_identity(&top))
            .arg("--literal-pathspecs")
            .args(args)
            .env("GIT_INDEX_FILE", &index)
            .stdin(Stdio::null())
            .output()
            .map_err(|source| error("save", source.to_string()))?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
        } else {
            Err(error(
                "save",
                String::from_utf8_lossy(&out.stderr).trim().to_string(),
            ))
        }
    };
    let saved = (|| {
        run(&["read-tree", "HEAD"])?;
        let mut add = vec!["add", "-A", "--"];
        add.extend(tracked.iter().chain(&untracked).map(String::as_str));
        run(&add)?;
        let tree = run(&["write-tree"])?;
        run(&[
            "commit-tree",
            &tree,
            "-p",
            "HEAD",
            "-m",
            "Hot Sheet: changes saved while a rebase or merge was unfinished",
        ])
    })();
    let _ = std::fs::remove_file(&index);
    let commit = saved?;
    let stamp = time::OffsetDateTime::now_utc().unix_timestamp_nanos();
    let backup_ref = format!("refs/hotsheet/sync-backup/{stamp}-saved-writes");
    let update = git_run(&top, &["update-ref", &backup_ref, &commit]);
    if !update.ok {
        return Err(error("update-ref", update.stderr.trim()));
    }
    Ok(Some(SavedWrites {
        top,
        commit,
        backup_ref,
        tracked,
        untracked,
    }))
}

/// Return the saved paths to their index state, so the abort neither discards them
/// (`rebase --abort`) nor refuses because of them (`merge --abort`).
pub(crate) fn set_aside(saved: &SavedWrites) -> Result<(), StalledIntegrationError> {
    if !saved.tracked.is_empty() {
        let mut args = vec!["--literal-pathspecs", "checkout", "-q", "--"];
        args.extend(saved.tracked.iter().map(String::as_str));
        let run = git_run(&saved.top, &args);
        if !run.ok {
            return Err(error("set aside", run.stderr.trim()));
        }
    }
    for path in &saved.untracked {
        remove(&saved.top.join(path))?;
    }
    Ok(())
}

/// Write every saved path back exactly as it was saved: its content, or its absence.
pub(crate) fn restore(saved: &SavedWrites) -> Result<(), StalledIntegrationError> {
    let mut args = vec![
        "--literal-pathspecs",
        "ls-tree",
        "-r",
        "-z",
        "--name-only",
        &saved.commit,
        "--",
    ];
    args.extend(saved.paths().map(String::as_str));
    let present: Vec<String> = git_stdout(&saved.top, &args)
        .ok_or_else(|| error("restore", "the saved writes could not be listed"))?
        .split('\0')
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect();
    for path in saved.paths() {
        let target = saved.top.join(path);
        if present.contains(path) {
            let out = crate::git::command()
                .arg("-C")
                .arg(&saved.top)
                .args(["cat-file", "blob", &format!("{}:{path}", saved.commit)])
                .stdin(Stdio::null())
                .output()
                .map_err(|source| error("restore", source.to_string()))?;
            if !out.status.success() {
                return Err(error(
                    "restore",
                    String::from_utf8_lossy(&out.stderr).trim().to_string(),
                ));
            }
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|e| error("restore", e.to_string()))?;
            }
            std::fs::write(&target, out.stdout).map_err(|e| error("restore", e.to_string()))?;
        } else {
            remove(&target)?;
        }
    }
    Ok(())
}

/// Commit the restored writes on the branch, path-scoped. Returns whether a commit was
/// made (a write equal to the branch's content needs none). Never pushes.
pub(crate) fn commit(saved: &SavedWrites) -> Result<bool, StalledIntegrationError> {
    let paths: Vec<&str> = saved.paths().map(String::as_str).collect();
    let mut add = vec!["--literal-pathspecs", "add", "-A", "--"];
    add.extend(&paths);
    let staged = git_run(&saved.top, &add);
    if !staged.ok {
        return Err(error("add", staged.stderr.trim()));
    }
    let mut quiet = vec!["--literal-pathspecs", "diff", "--cached", "--quiet", "--"];
    quiet.extend(&paths);
    if crate::sync::git_ok(&saved.top, &quiet) {
        return Ok(false);
    }
    let mut args = crate::sync_replay::committer_identity(&saved.top).to_vec();
    args.extend_from_slice(&[
        "--literal-pathspecs",
        "commit",
        "-q",
        "-m",
        "Hot Sheet: commit changes saved while a rebase or merge was unfinished",
        "--only",
        "--",
    ]);
    args.extend(&paths);
    let run = git_run(&saved.top, &args);
    if !run.ok {
        return Err(error("commit", run.stderr.trim()));
    }
    Ok(true)
}

fn remove(path: &Path) -> Result<(), StalledIntegrationError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(error("set aside", source.to_string())),
    }
}

/// A saved-writes backup whose writes reached neither the branch nor the working tree
/// (HS2-RDPT34): the recovery set them aside and then stopped (the process died, or the
/// restore failed) before writing them back.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StrandedSavedWrites {
    /// The `refs/hotsheet/sync-backup/<stamp>-saved-writes` ref holding them.
    pub backup_ref: String,
    /// The stranded paths, repository-relative.
    pub paths: Vec<String>,
}

/// Every saved-writes backup with stranded paths. A saved path is stranded when its saved
/// content (or absence) is on neither the branch's `HEAD` nor the working tree, and no
/// branch commit made after the backup (in a later second, Git's commit-time resolution)
/// touched it. A completed recovery puts the writes on the branch, and a later edit of the
/// same ticket supersedes them, so neither is reported.
/// Read-only: a few local `git` calls per saved-writes ref, none when there are none.
#[must_use]
pub fn stranded(store_path: &Path) -> Vec<StrandedSavedWrites> {
    let Some(top) = git_stdout(store_path, &["rev-parse", "--show-toplevel"])
        .map(|top| PathBuf::from(top.trim_end_matches(['\n', '\r'])))
    else {
        return Vec::new();
    };
    git_stdout(
        &top,
        &[
            "for-each-ref",
            "--format=%(refname)",
            "refs/hotsheet/sync-backup/",
        ],
    )
    .unwrap_or_default()
    .lines()
    .filter(|name| name.ends_with("-saved-writes"))
    .filter_map(|backup_ref| {
        let paths = stranded_paths(&top, backup_ref);
        (!paths.is_empty()).then(|| StrandedSavedWrites {
            backup_ref: backup_ref.to_string(),
            paths,
        })
    })
    .collect()
}

/// The object id `rev` names, or `None` when it names nothing (an absent path).
fn object(top: &Path, rev: &str) -> Option<String> {
    git_stdout(top, &["rev-parse", "--verify", "--quiet", rev])
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
}

fn stranded_paths(top: &Path, backup_ref: &str) -> Vec<String> {
    let split = |text: String| -> Vec<String> {
        text.split('\0')
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .collect()
    };
    // The backup commit's parent is the HEAD the writes were saved on, so its diff names
    // exactly the saved paths.
    let saved = split(
        git_stdout(
            top,
            &[
                "diff-tree",
                "--no-commit-id",
                "--name-only",
                "-r",
                "-z",
                &format!("{backup_ref}^"),
                backup_ref,
            ],
        )
        .unwrap_or_default(),
    );
    if saved.is_empty() {
        return saved;
    }
    let Some(saved_at) = git_stdout(top, &["log", "-1", "--format=%ct", backup_ref])
        .and_then(|time| time.trim().parse::<i64>().ok())
    else {
        return Vec::new();
    };
    // Walk every branch commit touching a saved path and keep those dated strictly later.
    // No `--since`: Git stops walking at the first older commit, which a skewed or
    // out-of-order commit date would turn into a missed later edit.
    let mut args = vec![
        "-c",
        "core.quotePath=false",
        "--literal-pathspecs",
        "log",
        "--format=%x01%ct",
        "--name-only",
        "HEAD",
        "--",
    ];
    args.extend(saved.iter().map(String::as_str));
    let mut touched: Vec<String> = Vec::new();
    let mut later = false;
    for line in git_stdout(top, &args).unwrap_or_default().lines() {
        if let Some(time) = line.strip_prefix('\u{1}') {
            // Strictly later: a commit in the backup's own second may predate it.
            later = time.trim().parse::<i64>().is_ok_and(|time| time > saved_at);
        } else if later && !line.is_empty() && !touched.iter().any(|path| path == line) {
            touched.push(line.to_string());
        }
    }
    saved
        .into_iter()
        .filter(|path| !touched.contains(path))
        .filter(|path| {
            let kept = object(top, &format!("{backup_ref}:{path}"));
            kept != object(top, &format!("HEAD:{path}")) && kept != working_object(top, path)
        })
        .collect()
}

/// The object id the working-tree file at `path` would have, or `None` when it is absent.
fn working_object(top: &Path, path: &str) -> Option<String> {
    top.join(path)
        .is_file()
        .then(|| git_stdout(top, &["hash-object", "--", path]))
        .flatten()
        .map(|id| id.trim().to_string())
}

/// What [`restore_stranded`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestoredSavedWrites {
    /// Stranded paths written back.
    pub restored: usize,
    /// Whether they were committed on the branch.
    pub committed: bool,
}

/// Write the stranded paths of `backup_ref` back and commit them on the branch. Paths the
/// branch or the working tree already carry, or that a later commit touched, are left
/// alone, so running it again changes nothing. The caller holds the sync locks and has
/// checked that no rebase or merge is unfinished.
pub(crate) fn restore_stranded(
    store_path: &Path,
    backup_ref: &str,
) -> Result<RestoredSavedWrites, StalledIntegrationError> {
    if !backup_ref.starts_with("refs/hotsheet/sync-backup/")
        || !backup_ref.ends_with("-saved-writes")
    {
        return Err(error(
            "restore saved writes",
            format!("{backup_ref} is not a saved-writes backup"),
        ));
    }
    let top = git_stdout(store_path, &["rev-parse", "--show-toplevel"])
        .map(|top| PathBuf::from(top.trim_end_matches(['\n', '\r'])))
        .ok_or_else(|| error("rev-parse", "the store has no Git working tree"))?;
    let backup_commit = object(&top, &format!("{backup_ref}^{{commit}}")).ok_or_else(|| {
        error(
            "restore saved writes",
            format!("{backup_ref} does not exist"),
        )
    })?;
    let paths = stranded_paths(&top, backup_ref);
    // Never overwrite a working-tree change (edit, new file, or deletion) the branch lacks.
    let dirty: Vec<&str> = paths
        .iter()
        .filter(|path| working_object(&top, path) != object(&top, &format!("HEAD:{path}")))
        .map(String::as_str)
        .collect();
    if !dirty.is_empty() {
        return Err(error(
            "restore saved writes",
            format!(
                "uncommitted changes to {} would be overwritten; commit or discard them first",
                dirty.join(", ")
            ),
        ));
    }
    if paths.is_empty() {
        return Ok(RestoredSavedWrites {
            restored: 0,
            committed: false,
        });
    }
    let saved = SavedWrites {
        top,
        commit: backup_commit,
        backup_ref: backup_ref.to_string(),
        tracked: paths,
        untracked: Vec::new(),
    };
    restore(&saved)?;
    let committed = commit(&saved)?;
    Ok(RestoredSavedWrites {
        restored: saved.len(),
        committed,
    })
}
