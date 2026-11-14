//! Replaying local commits for the sync cycle without a stall (HS2-QR1K8R, `docs/02`
//! §2.12.3).
//!
//! `git rebase` works in the live working tree: it first checks out the upstream, so
//! every file that only local commits contain is gone until its pick is replayed. Ticket
//! writes put their file in the working tree before they take the Git transaction lock,
//! so a write during that window recreated such a file, Git refused and rescheduled the
//! pick, `git rebase --abort` refused for the same reason, and the waiting autocommit
//! then committed onto the detached `HEAD`.
//!
//! [`replay_onto_upstream`] therefore builds the rebased history from objects alone
//! (`git merge-tree --write-tree` plus `git commit-tree`, through the registered merge
//! driver) and never touches the working tree, the index, or `HEAD`.
//! [`move_branch_to`] then moves the branch with one checked two-tree update that
//! changes only the paths the incoming commits changed and refuses, changing nothing,
//! when a concurrent write touched one of them.
//!
//! [`continue_unfinished`] is the self-heal for a rebase that was left unfinished anyway
//! (an older Hot Sheet, an older Git, or a person): when nothing conflicts it is
//! continued, bounded and logged, instead of being reported forever.

use std::io::Write as _;
use std::path::Path;
use std::process::Stdio;

use crate::sync::{git_ok, git_run, git_stdout};
use crate::sync_health::{IntegrationInProgress, integration_in_progress};

/// Automatic continues of one unfinished rebase (one `orig-head`) before the cycle
/// reports it instead.
pub const MAX_CONTINUE_ATTEMPTS: u32 = 3;

/// Replays of one cycle when a concurrent write keeps touching incoming paths.
pub(crate) const MAX_REPLAY_ATTEMPTS: u32 = 3;

/// `git merge-tree --write-tree --merge-base` needs Git 2.40.
const ISOLATED_REPLAY_MIN_GIT: (u32, u32) = (2, 40);

/// Whether the installed Git can replay commits without a working tree.
pub(crate) fn isolated_replay_supported() -> bool {
    static SUPPORTED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *SUPPORTED.get_or_init(|| {
        crate::git::command()
            .arg("version")
            .output()
            .ok()
            .and_then(|out| parse_git_version(&String::from_utf8_lossy(&out.stdout)))
            .is_some_and(|version| version >= ISOLATED_REPLAY_MIN_GIT)
    })
}

/// `(major, minor)` from `git version 2.54.0 (Apple Git-157)`.
pub(crate) fn parse_git_version(text: &str) -> Option<(u32, u32)> {
    let number = text.split_whitespace().nth(2)?;
    let mut parts = number.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    Some((major, minor))
}

/// Why [`replay_onto_upstream`] produced no commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReplayError {
    /// The merge driver could not settle a commit. Nothing was changed.
    Conflict,
    /// The history has a shape this replay does not handle, or Git failed. Nothing was
    /// changed; the caller falls back to `git rebase`.
    Unsupported(String),
}

/// Replay the local commits the upstream lacks onto the fetched upstream and return the
/// new tip commit, without touching the working tree, the index, or any ref. The commit
/// selection matches `git rebase @{u}`: no merge commits, no commits whose patch the
/// upstream already has, and commits that become empty are dropped.
pub(crate) fn replay_onto_upstream(store_path: &Path) -> Result<String, ReplayError> {
    let unsupported = |what: &str| ReplayError::Unsupported(what.to_string());
    let upstream = rev_parse(store_path, "@{u}").ok_or_else(|| unsupported("no upstream"))?;
    let listing = git_stdout(
        store_path,
        &[
            "rev-list",
            "--reverse",
            "--topo-order",
            "--no-merges",
            "--cherry-pick",
            "--right-only",
            "--parents",
            "@{u}...HEAD",
        ],
    )
    .ok_or_else(|| unsupported("could not list local commits"))?;
    let mut tip_tree = rev_parse(store_path, &format!("{upstream}^{{tree}}"))
        .ok_or_else(|| unsupported("the upstream has no tree"))?;
    let mut tip = upstream;
    let identity = committer_identity(store_path);
    for line in listing.lines() {
        let mut ids = line.split(' ');
        let commit = ids.next().unwrap_or_default();
        let Some(parent) = ids.next() else {
            return Err(unsupported("a local root commit"));
        };
        let merged = crate::git::command()
            .arg("-C")
            .arg(store_path)
            .args(["merge-tree", "--write-tree", "--no-messages"])
            .arg(format!("--merge-base={parent}"))
            .args([tip.as_str(), commit])
            .stdin(Stdio::null())
            .output()
            .map_err(|error| ReplayError::Unsupported(error.to_string()))?;
        match merged.status.code() {
            Some(0) => {}
            Some(1) => return Err(ReplayError::Conflict),
            _ => {
                return Err(ReplayError::Unsupported(
                    String::from_utf8_lossy(&merged.stderr).trim().to_string(),
                ));
            }
        }
        let tree = String::from_utf8_lossy(&merged.stdout)
            .lines()
            .next()
            .unwrap_or_default()
            .trim()
            .to_string();
        if tree.is_empty() {
            return Err(unsupported("merge-tree printed no tree"));
        }
        if tree == tip_tree {
            // The upstream already has this change; `git rebase` drops it too.
            continue;
        }
        tip = commit_replayed(store_path, commit, &tree, &tip, identity)?;
        tip_tree = tree;
    }
    Ok(tip)
}

/// The fallback committer identity [`crate::FsStore::autocommit`] uses when the
/// repository has none.
pub(crate) fn committer_identity(store_path: &Path) -> &'static [&'static str] {
    let configured = git_stdout(store_path, &["config", "user.email"])
        .is_some_and(|email| !email.trim().is_empty());
    if configured {
        &[]
    } else {
        &[
            "-c",
            "user.name=Hot Sheet",
            "-c",
            "user.email=hotsheet@localhost",
        ]
    }
}

/// Write `commit`'s message and author onto `tree` with parent `tip`.
fn commit_replayed(
    store_path: &Path,
    commit: &str,
    tree: &str,
    tip: &str,
    identity: &[&str],
) -> Result<String, ReplayError> {
    let failed = |detail: String| ReplayError::Unsupported(detail);
    let raw = crate::git::command()
        .arg("-C")
        .arg(store_path)
        .args(["cat-file", "commit", commit])
        .output()
        .map_err(|error| failed(error.to_string()))?;
    if !raw.status.success() {
        return Err(failed(format!("could not read commit {commit}")));
    }
    let (author, message) = split_commit(&raw.stdout)
        .ok_or_else(|| failed(format!("commit {commit} has no author")))?;
    let mut child = crate::git::command()
        .arg("-C")
        .arg(store_path)
        .args(identity)
        .args(["commit-tree", tree, "-p", tip])
        .env("GIT_AUTHOR_NAME", &author.name)
        .env("GIT_AUTHOR_EMAIL", &author.email)
        .env("GIT_AUTHOR_DATE", format!("@{}", author.date))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| failed(error.to_string()))?;
    if let Some(mut stdin) = child.stdin.take() {
        // A commit message is far below the pipe buffer, so this cannot block.
        stdin
            .write_all(message)
            .map_err(|error| failed(error.to_string()))?;
    }
    let out = child
        .wait_with_output()
        .map_err(|error| failed(error.to_string()))?;
    let id = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() || id.is_empty() {
        return Err(failed(
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        ));
    }
    Ok(id)
}

#[derive(Debug, PartialEq, Eq)]
struct Author {
    name: String,
    email: String,
    /// Git's internal `<seconds> <zone>` form.
    date: String,
}

/// The author and message of a raw commit object.
fn split_commit(raw: &[u8]) -> Option<(Author, &[u8])> {
    let split = raw.windows(2).position(|pair| pair == b"\n\n")?;
    let (header, message) = (&raw[..split], &raw[split + 2..]);
    let header = String::from_utf8_lossy(header);
    let line = header.lines().find_map(|l| l.strip_prefix("author "))?;
    let open = line.rfind('<')?;
    let close = line.rfind('>')?;
    (open < close).then(|| {
        (
            Author {
                name: line[..open].trim().to_string(),
                email: line[open + 1..close].to_string(),
                date: line[close + 1..].trim().to_string(),
            },
            message,
        )
    })
}

fn rev_parse(store_path: &Path, name: &str) -> Option<String> {
    git_stdout(store_path, &["rev-parse", "--verify", "--quiet", name])
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
}

#[cfg(test)]
thread_local! {
    /// Runs on the syncing thread after a replay and before the branch moves, so a test
    /// can land a concurrent write at exactly that point.
    pub(crate) static BEFORE_BRANCH_MOVE: std::cell::RefCell<Option<Box<dyn FnMut()>>> =
        const { std::cell::RefCell::new(None) };
}

/// Move the checked-out branch from `expected_head` to `tip` and update only the working
/// tree files that differ between them. Git checks every such path first and refuses,
/// changing nothing, when one has an uncommitted change. Returns whether the branch moved.
pub(crate) fn move_branch_to(store_path: &Path, expected_head: &str, tip: &str) -> bool {
    #[cfg(test)]
    BEFORE_BRANCH_MOVE.with(|hook| {
        if let Some(hook) = hook.borrow_mut().as_mut() {
            hook();
        }
    });
    // Only lock holders move `HEAD`, so this holds unless a foreign Git process ran.
    if rev_parse(store_path, "HEAD").as_deref() != Some(expected_head) {
        return false;
    }
    git_ok(store_path, &["reset", "--quiet", "--keep", tip])
}

/// What [`continue_unfinished`] decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Continued {
    /// The rebase finished; `backup_ref` keeps the commit `HEAD` was at before.
    Finished { backup_ref: String },
    /// The integration needs a decision; the cycle stops without touching it.
    Stalled { reason: String },
}

fn stalled(reason: impl Into<String>) -> Continued {
    Continued::Stalled {
        reason: reason.into(),
    }
}

/// Paths with unmerged index entries.
pub(crate) fn unmerged_paths(store_path: &Path) -> Vec<String> {
    git_stdout(store_path, &["diff", "--name-only", "--diff-filter=U"])
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

/// Continue an unfinished rebase that needs no judgment (HS2-QR1K8R). The caller holds
/// the history-sync and Git transaction locks.
///
/// It is continued only when it is a `rebase-merge` replay with no unmerged path and no
/// commit Git stopped at (`stopped-sha`): the shape a refused, rescheduled pick leaves.
/// A real conflict, a merge, and a replay a person paused all stay untouched, so sync
/// never stacks a replay on top of one. Stray working-tree changes (the write that made
/// Git refuse the pick) are committed first so the pick can apply through the merge
/// driver. A backup ref records `HEAD` before anything changes. Attempts are counted per
/// replay in the sync record and capped at [`MAX_CONTINUE_ATTEMPTS`].
pub(crate) fn continue_unfinished(store_path: &Path, kind: IntegrationInProgress) -> Continued {
    let default_reason = crate::sync::STALLED_INTEGRATION_REASON;
    if kind != IntegrationInProgress::Rebase {
        return stalled(default_reason);
    }
    let Some(git_dir) = git_stdout(store_path, &["rev-parse", "--absolute-git-dir"]) else {
        return stalled(default_reason);
    };
    let state = Path::new(git_dir.trim_end_matches(['\n', '\r'])).join("rebase-merge");
    let Ok(orig_head) = std::fs::read_to_string(state.join("orig-head")) else {
        // `rebase-apply`, or a state directory Git has not finished writing.
        return stalled(default_reason);
    };
    let orig_head = orig_head.trim().to_string();
    let paths = unmerged_paths(store_path);
    if !paths.is_empty() {
        return stalled(format!(
            "a rebase in the store stopped at a conflict in {} file{} that could not be merged \
             automatically; sync will not start another until it is finished or aborted",
            paths.len(),
            if paths.len() == 1 { "" } else { "s" }
        ));
    }
    // HS2-Z2AT6H: a pick that stopped while Hot Sheet continued its own replay, and whose
    // conflict was then resolved and staged, needs no further judgment unless markers remain.
    // A rebase a person stopped and resolved without a recorded Hot Sheet attempt stays
    // theirs to finish (HS2-QR1K8R).
    let stopped = state.join("stopped-sha").exists();
    if stopped {
        let ours = crate::sync_health::read_record(store_path)
            .and_then(|record| record.continue_attempts)
            .is_some_and(|(head, _)| head == orig_head);
        if !ours {
            return stalled(default_reason);
        }
        let marked = staged_conflict_markers(store_path);
        if !marked.is_empty() {
            return stalled(format!(
                "a rebase in the store stopped with conflict markers still staged in {} file{}; \
                 sync will not start another until it is finished or aborted",
                marked.len(),
                if marked.len() == 1 { "" } else { "s" }
            ));
        }
    }
    let attempt = crate::sync_health::note_continue_attempt(store_path, &orig_head);
    if attempt > MAX_CONTINUE_ATTEMPTS {
        return stalled(format!(
            "the unfinished rebase could not be continued automatically after \
             {MAX_CONTINUE_ATTEMPTS} attempts; sync will not start another until it is \
             finished or aborted"
        ));
    }
    let backup_ref = match crate::sync::record_backup_ref(store_path) {
        Ok((_head, backup_ref)) => backup_ref,
        Err(error) => {
            return stalled(format!(
                "the unfinished rebase was not continued: no backup ref could be recorded \
                 ({error})"
            ));
        }
    };
    if stopped {
        // Stage writes made during the stop; `--continue` commits them with the stopped
        // pick under its own message, where a separate commit would replace that message.
        let _ = git_ok(store_path, &["add", "-A"]);
    } else {
        crate::sync::commit_pending(store_path);
    }
    let run = git_run(
        store_path,
        &["-c", "core.editor=true", "rebase", "--continue"],
    );
    if run.ok && integration_in_progress(store_path).is_none() {
        eprintln!(
            "hotsheet sync: continued an unfinished rebase in {} (attempt {attempt} of \
             {MAX_CONTINUE_ATTEMPTS}); backup ref {backup_ref}",
            store_path.display()
        );
        return Continued::Finished { backup_ref };
    }
    let detail = run
        .stderr
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("error:") || line.starts_with("fatal:"))
        .unwrap_or("git rebase --continue failed");
    eprintln!(
        "hotsheet sync: could not continue the unfinished rebase in {} (attempt {attempt} of \
         {MAX_CONTINUE_ATTEMPTS}): {detail}; backup ref {backup_ref}",
        store_path.display()
    );
    stalled(format!(
        "the unfinished rebase could not be continued automatically ({detail}); a backup ref \
         {backup_ref} keeps its commits"
    ))
}

/// Staged paths whose index content still holds a Git conflict marker line (HS2-Z2AT6H).
fn staged_conflict_markers(store_path: &Path) -> Vec<String> {
    let Some(names) = git_stdout(store_path, &["diff", "--cached", "--name-only", "-z"]) else {
        return Vec::new();
    };
    names
        .split('\0')
        .filter(|name| !name.is_empty())
        .filter(|name| {
            git_stdout(store_path, &["show", &format!(":{name}")]).is_some_and(|body| {
                body.lines()
                    .any(|line| line.starts_with("<<<<<<< ") || line.starts_with(">>>>>>> "))
            })
        })
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hs2_qr1k8r_git_versions_parse_with_vendor_suffixes() {
        assert_eq!(
            parse_git_version("git version 2.54.0 (Apple Git-157)\n"),
            Some((2, 54))
        );
        assert_eq!(
            parse_git_version("git version 2.39.5.windows.1"),
            Some((2, 39))
        );
        assert_eq!(parse_git_version("git version 2.40"), Some((2, 40)));
        assert_eq!(parse_git_version("git version"), None);
        assert_eq!(parse_git_version("git version next"), None);
        assert!((2, 39) < ISOLATED_REPLAY_MIN_GIT);
        assert!((2, 40) >= ISOLATED_REPLAY_MIN_GIT);
        assert!((3, 0) >= ISOLATED_REPLAY_MIN_GIT);
    }

    #[test]
    fn hs2_qr1k8r_raw_commits_split_into_author_and_exact_message() {
        let raw = b"tree 1\nparent 2\nauthor A. Writer <a@test.invalid> 1700000000 -0800\n\
                    committer C <c@test.invalid> 1700000001 +0000\n\nSubject\n\nBody > text\n";
        let (author, message) = split_commit(raw).unwrap();
        assert_eq!(
            author,
            Author {
                name: "A. Writer".to_string(),
                email: "a@test.invalid".to_string(),
                date: "1700000000 -0800".to_string(),
            }
        );
        assert_eq!(message, b"Subject\n\nBody > text\n");
        assert!(split_commit(b"tree 1\ncommitter C <c@test.invalid> 1 +0000\n\nx\n").is_none());
        assert!(split_commit(b"tree 1\nauthor broken\n\nx\n").is_none());
        assert!(split_commit(b"no header end").is_none());
    }
}
