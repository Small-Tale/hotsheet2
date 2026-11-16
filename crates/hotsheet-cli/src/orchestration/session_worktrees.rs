//! HS2-TRP2MJ: which worktrees of a target checkout hold a hosted session's own
//! unfinished work, for the `uncommitted_changes` part of its safe point.
//!
//! A worker owns the worktrees its unfinished pool assignments name (`worktree`,
//! reported by `coordination worker --worktree`). An assignment recorded before that
//! field existed falls back to inference: a linked worktree whose branch contains the
//! assignment's slug, or whose HEAD is its handoff commit. A worker never owns the main
//! checkout unless an assignment names it, so a person's edits there and the tool
//! configuration a launch writes there do not hold it. An integrator owns the main
//! checkout; a dispatcher and a planner own nothing. Untracked files count inside an
//! owned worktree.
use std::path::{Path, PathBuf};

use hotsheet_ticketing::orchestration::SessionRole;

use crate::coordination::Assignment;

/// One worktree as `git worktree list --porcelain` lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Worktree {
    pub path: PathBuf,
    pub head: Option<String>,
    /// The branch it has checked out, without `refs/heads/`; `None` when detached.
    pub branch: Option<String>,
    /// The checkout's main worktree (the first one listed).
    pub main: bool,
}

/// The worktrees of a listing that have files: a bare entry and a prunable one (its
/// directory is gone) are left out. The first entry is the main worktree.
pub(super) fn parse_worktrees(listing: &str) -> Vec<Worktree> {
    let mut found = Vec::new();
    for (index, block) in listing
        .split("\n\n")
        .filter(|block| !block.trim().is_empty())
        .enumerate()
    {
        let mut worktree = Worktree {
            path: PathBuf::new(),
            head: None,
            branch: None,
            main: index == 0,
        };
        let mut skip = false;
        let mut named = false;
        for line in block.lines() {
            if let Some(path) = line.strip_prefix("worktree ") {
                worktree.path = PathBuf::from(path);
                named = true;
            } else if let Some(head) = line.strip_prefix("HEAD ") {
                worktree.head = Some(head.to_owned());
            } else if let Some(branch) = line.strip_prefix("branch ") {
                worktree.branch = Some(branch.trim_start_matches("refs/heads/").to_owned());
            }
            skip |= line == "bare" || line.starts_with("prunable");
        }
        if named && !skip {
            found.push(worktree);
        }
    }
    found
}

/// The same directory, resolving links (a temporary directory behind `/private`).
fn same_path(left: &Path, right: &Path) -> bool {
    let resolve = |path: &Path| std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
    left == right || resolve(left) == resolve(right)
}

/// Whether an assignment recorded without a worktree is taken to have worked in
/// `worktree`: a linked worktree on a branch naming its slug, or at its handoff commit.
fn inferred(assignment: &Assignment, worktree: &Worktree) -> bool {
    if worktree.main {
        return false;
    }
    let slug = assignment.slug.to_ascii_lowercase();
    let on_branch = !slug.is_empty()
        && worktree
            .branch
            .as_ref()
            .is_some_and(|branch| branch.to_ascii_lowercase().contains(&slug));
    let at_handoff = assignment
        .commit
        .as_ref()
        .is_some_and(|commit| worktree.head.as_ref() == Some(commit));
    on_branch || at_handoff
}

/// The worktrees whose uncommitted work is this session's. `assignments` is every
/// assignment of the target's pool; only `worker`'s unfinished ones count.
pub(super) fn owned_worktrees<'a>(
    worktrees: &'a [Worktree],
    role: SessionRole,
    worker: &str,
    assignments: &[&Assignment],
) -> Vec<&'a Worktree> {
    match role {
        SessionRole::Integrator => worktrees.iter().filter(|worktree| worktree.main).collect(),
        SessionRole::Dispatcher | SessionRole::Planner => Vec::new(),
        SessionRole::Worker => {
            let mine: Vec<&Assignment> = assignments
                .iter()
                .copied()
                .filter(|assignment| assignment.worker == worker && assignment.phase.pending())
                .collect();
            worktrees
                .iter()
                .filter(|worktree| {
                    mine.iter().any(|assignment| match &assignment.worktree {
                        Some(named) => same_path(Path::new(named), &worktree.path),
                        None => inferred(assignment, worktree),
                    })
                })
                .collect()
        }
    }
}
