//! Prune integrated `merge-pending/<SLUG>` remote branches (HS2-ZTG8FD).
//!
//! The ai-playbook workflow (HS2-8HCBA1) keeps completed side-branch work on the remote
//! under `merge-pending/<SLUG>[-<label>]` until its integration ticket lands it on the
//! shipping branch. This headless step finds those branches and deletes one only when
//! every commit it carries is already on the shipping branch, or patch-equivalent there
//! (`git cherry`, since cherry-picking is an allowed integration), **and** its named
//! ticket is closed. It is a dry run unless `apply` is set, and a rerun is a no-op.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use hotsheet_model::Status;
use serde::Serialize;

/// The remote branch namespace this command owns.
pub const PREFIX: &str = "merge-pending/";

#[derive(Clone, Debug)]
pub struct PruneOptions {
    /// Any checkout of the repository whose remote holds the branches.
    pub project: PathBuf,
    pub remote: String,
    /// Shipping ref to compare against; defaults to `<remote>/HEAD`, else `<remote>/main`.
    pub shipping: Option<String>,
    /// Delete eligible branches; otherwise only report.
    pub apply: bool,
    /// Refresh remote-tracking refs first (`git fetch --prune <remote>`).
    pub fetch: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// Eligible; a dry run left it in place.
    WouldDelete,
    Deleted,
    Kept,
    /// Eligible, but `git push --delete` failed; see `error`.
    DeleteFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum KeepReason {
    /// The name is not `merge-pending/<SLUG>[-<label>]`.
    MalformedName,
    /// No ticket with that slug exists in the store.
    UnknownSlug,
    /// Some commits are neither on the shipping branch nor patch-equivalent there.
    UnintegratedCommits,
    /// The named ticket is not completed, verified, archived, or deleted.
    TicketOpen,
    /// The ticket moved to another store, so its completion cannot be read here.
    TicketMoved,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BranchVerdict {
    /// Remote branch name, for example `merge-pending/HS2-FZAE4Y-batch-2`.
    pub branch: String,
    pub slug: Option<String>,
    pub ticket_status: Option<Status>,
    /// Commits `git cherry` reports as missing from the shipping branch.
    pub unintegrated: usize,
    pub action: Action,
    pub reason: Option<KeepReason>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PruneReport {
    pub remote: String,
    pub shipping: String,
    pub apply: bool,
    /// Set when refreshing the remote failed; verdicts then use the last fetched refs.
    pub fetch_error: Option<String>,
    pub branches: Vec<BranchVerdict>,
}

/// The ticket slug a `merge-pending/` branch names: `<PREFIX>-<ID>` with an optional
/// `-<label>` suffix, for example `HS2-FZAE4Y` from `HS2-FZAE4Y-batch-2`.
pub fn parse_slug(name: &str) -> Option<String> {
    let rest = name.strip_prefix(PREFIX)?;
    let mut parts = rest.splitn(3, '-');
    let prefix = parts.next()?;
    let id = parts.next()?;
    let label = parts.next();
    let upper_alnum = |text: &str| {
        !text.is_empty()
            && text
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
    };
    let valid = prefix.starts_with(|c: char| c.is_ascii_uppercase())
        && upper_alnum(prefix)
        && upper_alnum(id)
        && label.is_none_or(|label| !label.is_empty() && !label.contains('/'));
    valid.then(|| format!("{prefix}-{id}"))
}

fn git(project: &Path, args: &[&str]) -> Result<String> {
    let output = hotsheet_ticketing::git::command()
        .arg("-C")
        .arg(project)
        .args(args)
        .output()
        .context("running git")?;
    if !output.status.success() {
        bail!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8(output.stdout)?)
}

fn shipping_ref(options: &PruneOptions) -> Result<String> {
    let shipping = match &options.shipping {
        Some(shipping) => shipping.clone(),
        None => git(
            &options.project,
            &[
                "rev-parse",
                "--abbrev-ref",
                &format!("{}/HEAD", options.remote),
            ],
        )
        .map(|name| name.trim().to_owned())
        .unwrap_or_else(|_| format!("{}/main", options.remote)),
    };
    git(
        &options.project,
        &["rev-parse", "--verify", &format!("{shipping}^{{commit}}")],
    )
    .with_context(|| format!("shipping ref {shipping} does not resolve to a commit"))?;
    Ok(shipping)
}

fn is_closed(status: Status) -> bool {
    matches!(
        status,
        Status::Completed | Status::Verified | Status::Archive | Status::Deleted
    )
}

/// Evaluate, and with `apply` delete, every `<remote>/merge-pending/*` branch.
/// `status_of` resolves a slug to its ticket status, or `None` for an unknown slug.
pub fn prune(
    options: &PruneOptions,
    status_of: &dyn Fn(&str) -> Result<Option<Status>>,
) -> Result<PruneReport> {
    let fetch_error = options
        .fetch
        .then(|| {
            git(
                &options.project,
                &["fetch", "--prune", "--quiet", &options.remote],
            )
            .err()
        })
        .flatten()
        .map(|error| error.to_string());
    let shipping = shipping_ref(options)?;
    let tracking = format!("refs/remotes/{}/", options.remote);
    let refs = git(
        &options.project,
        &[
            "for-each-ref",
            "--format=%(refname)",
            &format!("{tracking}{PREFIX}"),
        ],
    )?;
    let mut branches = Vec::new();
    for full in refs.lines().filter(|line| !line.is_empty()) {
        let branch = full.strip_prefix(&tracking).unwrap_or(full).to_owned();
        let unintegrated = git(&options.project, &["cherry", &shipping, full])?
            .lines()
            .filter(|line| line.starts_with('+'))
            .count();
        let slug = parse_slug(&branch);
        let ticket_status = match &slug {
            Some(slug) => status_of(slug)?,
            None => None,
        };
        let reason = match (&slug, ticket_status) {
            (None, _) => Some(KeepReason::MalformedName),
            (Some(_), None) => Some(KeepReason::UnknownSlug),
            _ if unintegrated > 0 => Some(KeepReason::UnintegratedCommits),
            (Some(_), Some(Status::Moved)) => Some(KeepReason::TicketMoved),
            (Some(_), Some(status)) if !is_closed(status) => Some(KeepReason::TicketOpen),
            _ => None,
        };
        let (action, error) = match (reason, options.apply) {
            (Some(_), _) => (Action::Kept, None),
            (None, false) => (Action::WouldDelete, None),
            (None, true) => match git(
                &options.project,
                &["push", "--quiet", &options.remote, "--delete", &branch],
            ) {
                Ok(_) => (Action::Deleted, None),
                Err(error) => (Action::DeleteFailed, Some(error.to_string())),
            },
        };
        branches.push(BranchVerdict {
            branch,
            slug,
            ticket_status,
            unintegrated,
            action,
            reason,
            error,
        });
    }
    Ok(PruneReport {
        remote: options.remote.clone(),
        shipping,
        apply: options.apply,
        fetch_error,
        branches,
    })
}

#[cfg(test)]
#[path = "merge_pending_tests.rs"]
mod tests;
