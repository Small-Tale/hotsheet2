//! An AI completion's commits must be on the remote (HS2-R8ZH4K).
//!
//! The ai-playbook workflow (HS2-8HCBA1) says completed work always lives on the remote:
//! on the shipping branch, or on `merge-pending/<SLUG>` until it is integrated. This module
//! is the one engine every completing surface (CLI, MCP, HTTP) calls for an `ai` actor,
//! beside the confidence rule in [`crate::actor`], so prose cannot drift from enforcement.
//!
//! The ticket's commits are those named explicitly (`--commit`, MCP/HTTP `commits`), or
//! else every commit in the code checkout whose subject leads with the ticket slug. After
//! a best-effort `git fetch`, each commit must be reachable from a remote-tracking ref or
//! be patch-equivalent to a remote commit naming the slug (a rebase or cherry-pick). An
//! explicit `no_commit` opt-out covers tickets with no code; callers record it in history.
//! When the fetch fails and a commit cannot be proven remote, the completion is refused
//! with its own code rather than passing silently.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use crate::actor::{MutationActor, RuleViolation};

/// A commit naming the ticket is not on any remote branch.
pub const COMMIT_NOT_ON_REMOTE: &str = "commit_not_on_remote";
/// The remote could not be fetched and a commit is not on the last fetched refs.
pub const COMMIT_REMOTE_UNVERIFIED: &str = "commit_remote_unverified";
/// No commit names the ticket and no opt-out was given.
pub const COMMITS_NOT_FOUND: &str = "commits_not_found";
/// An explicitly named commit does not exist in the checkout, or the request is invalid.
pub const COMMIT_INVALID: &str = "commit_invalid";

/// The caller's statement about the ticket's commits.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CommitClaim {
    /// Exact commits to verify instead of searching by slug.
    pub commits: Vec<String>,
    /// The ticket has no code commits (docs-in-store, investigation, decision).
    pub no_commit: bool,
}

/// What a passing check established, for the caller to report or record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The rule does not apply: not an AI completion.
    NotApplicable,
    /// Every commit is on the remote (directly or patch-equivalent).
    Verified { commits: Vec<String> },
    /// The actor declared that the ticket has no commits; callers record this in history.
    OptedOut,
    /// No code checkout is known for this completion, so nothing could be checked.
    NoCheckout,
}

impl Outcome {
    /// The history line a caller appends for an opt-out, if any.
    #[must_use]
    pub fn history_note(&self) -> Option<&'static str> {
        matches!(self, Self::OptedOut).then_some(
            "Completed with the no-commit opt-out: the AI actor declared this ticket has no \
             code commits, so the remote-commit check (HS2-R8ZH4K) did not run.",
        )
    }

    /// A warning a caller shows when the check could not run.
    #[must_use]
    pub fn warning(&self) -> Option<&'static str> {
        matches!(self, Self::NoCheckout).then_some(
            "remote-commit check skipped: no code checkout is known for this completion \
             (HS2-R8ZH4K); pass a checkout or run from the code checkout",
        )
    }
}

/// Append the opt-out's history note after the completing write, when there is one.
/// Returns the updated ticket, or `None` when the outcome records nothing.
pub fn record(
    store: &crate::FsStore,
    ticket: &hotsheet_model::Ulid,
    now: hotsheet_model::Timestamp,
    actor: Option<&MutationActor>,
    outcome: &Outcome,
) -> Result<Option<hotsheet_model::Ticket>, crate::StoreError> {
    let Some(text) = outcome.history_note() else {
        return Ok(None);
    };
    crate::ops::add_note_with_metadata(
        store,
        ticket,
        hotsheet_model::Ulid::new(),
        now,
        hotsheet_model::NoteKind::Activity,
        crate::ops::NoteMetadataInput {
            kind_explicit: true,
            summary: Some("Completed without code commits".into()),
            actor: actor.map(MutationActor::note_actor),
            ..crate::ops::NoteMetadataInput::default()
        },
        text.to_owned(),
    )
    .map(Some)
}

fn violation(code: &'static str, message: String) -> RuleViolation {
    RuleViolation { code, message }
}

fn git(repo: &Path, args: &[&str]) -> Option<String> {
    let output = crate::git::command()
        .arg("-C")
        .arg(repo)
        .env("GIT_TERMINAL_PROMPT", "0")
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// The top level of the Git checkout containing `dir`, if any.
fn toplevel(dir: &Path) -> Option<PathBuf> {
    git(dir, &["rev-parse", "--show-toplevel"])
        .map(|line| PathBuf::from(line.trim()))
        .map(|path| hotsheet_sync::paths::canonicalize(&path).unwrap_or(path))
}

/// The first candidate that is a Git checkout other than the ticket store's own
/// repository. Store commits name slugs too, so they never count as code commits.
#[must_use]
pub fn code_checkout(candidates: &[PathBuf], store_root: &Path) -> Option<PathBuf> {
    let store = toplevel(store_root);
    candidates
        .iter()
        .filter_map(|candidate| toplevel(candidate))
        .find(|top| Some(top) != store.as_ref())
}

/// Checkout roots that could hold `store_root`'s code commits, for [`code_checkout`]:
/// `preferred` first (the request's own checkout), then every registered checkout linked
/// to the store through a Git source or a store link. An unreadable registry yields only
/// `preferred`, which then reports [`Outcome::NoCheckout`] rather than failing the write.
#[must_use]
pub fn checkout_candidates(
    registry: &crate::checkouts::CheckoutRegistry,
    store_root: &Path,
    preferred: Option<&Path>,
) -> Vec<PathBuf> {
    let canonical =
        |path: &Path| hotsheet_sync::paths::canonicalize(path).unwrap_or_else(|_| path.to_owned());
    let store = canonical(store_root);
    let mut candidates: Vec<PathBuf> = preferred.map(Path::to_path_buf).into_iter().collect();
    for checkout in registry.list().unwrap_or_default() {
        let linked = checkout
            .sources
            .iter()
            .filter(|source| source.provider == "git")
            .map(|source| source.locator.as_str())
            .chain(checkout.stores.iter().map(String::as_str))
            .any(|path| canonical(Path::new(path)) == store);
        let root = PathBuf::from(&checkout.root);
        if linked && !candidates.contains(&root) {
            candidates.push(root);
        }
    }
    candidates
}

/// Whether `subject`'s leading slug tokens include `slug` (`HS2-A HS2-B fix …` names both).
fn names_slug(subject: &str, slug: &str) -> bool {
    subject
        .split(|c: char| c.is_whitespace() || c == ',' || c == ':')
        .filter(|token| !token.is_empty())
        .take_while(|token| {
            token.contains('-')
                && token
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '-')
        })
        .any(|token| token.eq_ignore_ascii_case(slug))
}

/// Commits (full SHAs) whose subject leads with `slug`, searched in `scope` (`--all` or
/// `--remotes`).
fn slug_commits(repo: &Path, slug: &str, scope: &str) -> Vec<String> {
    let grep = format!("--grep={slug}");
    git(
        repo,
        &[
            "log",
            scope,
            "--fixed-strings",
            "--regexp-ignore-case",
            &grep,
            "--format=%H%x09%s",
        ],
    )
    .unwrap_or_default()
    .lines()
    .filter_map(|line| line.split_once('\t'))
    .filter(|(_, subject)| names_slug(subject, slug))
    .map(|(sha, _)| sha.to_owned())
    .collect()
}

fn on_remote(repo: &Path, sha: &str) -> bool {
    git(
        repo,
        &[
            "for-each-ref",
            "--count=1",
            "--contains",
            sha,
            "refs/remotes",
        ],
    )
    .is_some_and(|refs| !refs.trim().is_empty())
}

fn patch_id(repo: &Path, sha: &str) -> Option<String> {
    let diff = git(repo, &["show", "--format=", "--no-color", sha])?;
    if diff.trim().is_empty() {
        return None;
    }
    let mut child = crate::git::command()
        .arg("-C")
        .arg(repo)
        .args(["patch-id", "--stable"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .ok()?;
    child.stdin.take()?.write_all(diff.as_bytes()).ok()?;
    let output = child.wait_with_output().ok()?;
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .next()
        .map(str::to_owned)
}

fn local_branches(repo: &Path, sha: &str) -> Vec<String> {
    git(
        repo,
        &[
            "for-each-ref",
            "--format=%(refname:short)",
            "--contains",
            sha,
            "refs/heads",
        ],
    )
    .unwrap_or_default()
    .lines()
    .map(str::to_owned)
    .collect()
}

/// Check an AI completion of `slug` against the code checkout `repo`. `fetch` refreshes
/// every remote first (tests may disable it). Nothing here writes to the ticket.
pub fn check(
    actor: Option<&MutationActor>,
    completes: bool,
    slug: &str,
    repo: Option<&Path>,
    claim: &CommitClaim,
    fetch: bool,
) -> Result<Outcome, RuleViolation> {
    if !completes || !actor.is_some_and(MutationActor::is_ai) {
        return Ok(Outcome::NotApplicable);
    }
    if claim.no_commit && !claim.commits.is_empty() {
        return Err(violation(
            COMMIT_INVALID,
            format!(
                "{COMMIT_INVALID}: completing {slug} named commits and the no-commit opt-out \
                 together. Nothing was changed. Retry with only one of them."
            ),
        ));
    }
    if claim.no_commit {
        return Ok(Outcome::OptedOut);
    }
    let Some(repo) = repo else {
        return Ok(Outcome::NoCheckout);
    };
    let mut commits = Vec::new();
    for named in &claim.commits {
        match git(
            repo,
            &["rev-parse", "--verify", &format!("{named}^{{commit}}")],
        ) {
            Some(sha) => commits.push(sha.trim().to_owned()),
            None => {
                return Err(violation(
                    COMMIT_INVALID,
                    format!(
                        "{COMMIT_INVALID}: commit {named} named for {slug} is not in the code \
                         checkout {}. Nothing was changed. Fetch it or name the commit that \
                         exists.",
                        repo.display()
                    ),
                ));
            }
        }
    }
    let fetch_error = fetch && git(repo, &["fetch", "--all", "--quiet"]).is_none();
    if claim.commits.is_empty() {
        commits = slug_commits(repo, slug, "--all");
    }
    if commits.is_empty() {
        return Err(violation(
            COMMITS_NOT_FOUND,
            format!(
                "{COMMITS_NOT_FOUND}: no commit in the code checkout {} has a subject that \
                 starts with {slug}. Nothing was changed. Commit with a slug-first subject and \
                 push it (shipping branch, or `merge-pending/{slug}`), name the commits \
                 (CLI `--commit <sha>`, MCP/HTTP `commits`), or, for a ticket with no code, \
                 retry with the opt-out (CLI `--no-commit`, MCP/HTTP `no_commit: true`).",
                repo.display()
            ),
        ));
    }
    let remote_patches: Vec<String> = slug_commits(repo, slug, "--remotes")
        .iter()
        .filter_map(|sha| patch_id(repo, sha))
        .collect();
    let missing: Vec<&String> = commits
        .iter()
        .filter(|sha| !on_remote(repo, sha))
        .filter(|sha| patch_id(repo, sha).is_none_or(|id| !remote_patches.contains(&id)))
        .collect();
    if missing.is_empty() {
        return Ok(Outcome::Verified { commits });
    }
    let detail = missing
        .iter()
        .map(|sha| {
            let branches = local_branches(repo, sha);
            let push = branches.first().map_or_else(
                || format!("git push origin {sha}:refs/heads/merge-pending/{slug}"),
                |branch| format!("git push origin {branch}:merge-pending/{slug}"),
            );
            format!(
                "{} (local branches: {}; push with `{push}`)",
                &sha[..sha.len().min(12)],
                if branches.is_empty() {
                    "none".to_owned()
                } else {
                    branches.join(", ")
                }
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    if fetch_error {
        return Err(violation(
            COMMIT_REMOTE_UNVERIFIED,
            format!(
                "{COMMIT_REMOTE_UNVERIFIED}: the remote could not be fetched, and these \
                 commits for {slug} are not on the last fetched remote branches: {detail}. \
                 Nothing was changed. Retry when the remote is reachable."
            ),
        ));
    }
    Err(violation(
        COMMIT_NOT_ON_REMOTE,
        format!(
            "{COMMIT_NOT_ON_REMOTE}: completed work must be on the remote (HS2-8HCBA1), but \
             these commits for {slug} are not on any remote branch: {detail}. Nothing was \
             changed. Push them to the shipping branch or `merge-pending/{slug}`, then retry."
        ),
    ))
}

#[cfg(test)]
#[path = "completion_remote_tests.rs"]
mod tests;
