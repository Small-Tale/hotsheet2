//! Ticket-associated commit discovery and safe configured-difftool launching.
//!
//! A commit belongs to a ticket when the ticket slug appears as a bounded token in its
//! subject or an explicit `Refs:` trailer. Other body text is deliberately ignored: it
//! commonly cross-references tickets whose code the commit did not implement. Launch
//! requests are checked against a fresh discovery result, then passed to `git difftool`
//! as an argument array (never a shell).
//!
//! Ticket discovery also scans commits that are not yet on the checkout's `HEAD`: local
//! branches and other worktrees' detached `HEAD`s (worktrees share one ref namespace), so
//! work done in another worktree is visible, marked [`UnmergedSource`], before it is merged
//! (HS2-0WZRK5).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

const LOG_LIMIT: usize = 2_000;
const FIELD_SEPARATOR: char = '\u{1f}';
const RECORD_SEPARATOR: char = '\u{1e}';

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CodeReviewCommit {
    pub sha: String,
    pub short_sha: String,
    pub subject: String,
    pub body: String,
    pub committed_at: String,
    /// Git ref decorations pointing at this commit (HEAD, branches, remotes, tags), newest-first
    /// as `git log --decorate` reports them. Empty for commits with no refs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refs: Vec<CommitRef>,
    /// Set when the commit is not reachable from the checkout's `HEAD`: where it lives
    /// instead (HS2-0WZRK5). Absent for merged commits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unmerged: Option<UnmergedSource>,
    #[serde(skip_serializing)]
    parents: Vec<String>,
}

/// The local branch and/or worktree an unmerged ticket commit was found on (HS2-0WZRK5).
#[derive(Debug, Clone, Serialize, PartialEq, Eq, Hash)]
pub struct UnmergedSource {
    /// The local branch whose history holds the commit, e.g. `feature-x`. `None` for a
    /// detached worktree `HEAD`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// The worktree directory that has this branch (or detached commit) checked out, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,
}

/// One git ref pointing at a commit, classified so clients can style each kind (HS2-SFJ5TE).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CommitRef {
    /// Human label, e.g. `HEAD → main`, `origin/main`, or `v1.2.0`.
    pub label: String,
    pub kind: CommitRefKind,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum CommitRefKind {
    /// `HEAD` itself, or the `HEAD → branch` pointer.
    Head,
    /// A local branch.
    Branch,
    /// A remote-tracking branch (contains a `/`).
    Remote,
    /// An annotated or lightweight tag.
    Tag,
}

/// Parse a `%D` decoration string (`HEAD -> main, origin/main, tag: v1.2`) into typed refs.
fn parse_decorations(raw: &str) -> Vec<CommitRef> {
    raw.split(',')
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(|token| {
            if let Some(tag) = token.strip_prefix("tag: ") {
                CommitRef {
                    label: tag.to_owned(),
                    kind: CommitRefKind::Tag,
                }
            } else if let Some((_, branch)) = token.split_once(" -> ") {
                CommitRef {
                    label: format!("HEAD → {branch}"),
                    kind: CommitRefKind::Head,
                }
            } else if token == "HEAD" {
                CommitRef {
                    label: "HEAD".to_owned(),
                    kind: CommitRefKind::Head,
                }
            } else if token.contains('/') {
                CommitRef {
                    label: token.to_owned(),
                    kind: CommitRefKind::Remote,
                }
            } else {
                CommitRef {
                    label: token.to_owned(),
                    kind: CommitRefKind::Branch,
                }
            }
        })
        .collect()
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CodeReviewRange {
    /// Oldest commit included in the range.
    pub from: String,
    /// Newest commit included in the range.
    pub to: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CodeReview {
    pub commits: Vec<CodeReviewCommit>,
    /// Maximal runs of adjacent ticket commits, newest run first.
    pub ranges: Vec<CodeReviewRange>,
    pub difftool: Option<String>,
    pub truncated: bool,
    pub summary: CodeReviewSummary,
    pub files: Vec<CodeReviewFile>,
    /// Explicit source-ticket follow-ups, including broken target diagnostics.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub follow_ups: Vec<FollowUpEvidence>,
    /// Recorded shipping work is on remote main, but this checkout has not advanced to it.
    #[serde(default)]
    pub checkout_behind_main: bool,
    /// The ticket's own first-parent commit runs behind `files`, oldest first, so a file
    /// launch never diffs across another ticket's interleaved commits (HS2-F10Y9P).
    #[serde(skip_serializing)]
    file_runs: Vec<FileRun>,
}

/// One first-parent run of a ticket's own commits, diffed on its own for the changed-files
/// summary and for file launches (HS2-F10Y9P).
#[derive(Debug, Clone, PartialEq, Eq)]
struct FileRun {
    /// The oldest run commit's first parent, or the empty tree for a root commit.
    base: String,
    /// The newest commit of the run.
    newest: String,
    /// Each summary path this run changed, mapped to the run-local pathspec that shows that
    /// change (the earlier name in a rename run, both names for a rename inside the run).
    touched: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FollowUpEvidence {
    pub target: hotsheet_model::ProjectTicketRef,
    /// `resolved`, `missing`, `unavailable`, or `invalid`; a broken link never blocks review.
    pub state: &'static str,
    pub slug: Option<String>,
    pub title: Option<String>,
    pub native_url: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CodeReviewFileChange {
    Added,
    Copied,
    Deleted,
    Modified,
    Renamed,
    TypeChanged,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CodeReviewFileCategory {
    Docs,
    Tests,
    Source,
    Other,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CodeReviewFile {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_path: Option<String>,
    pub change: CodeReviewFileChange,
    pub category: CodeReviewFileCategory,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub struct CodeReviewSummary {
    pub files: CodeReviewFileCounts,
    pub tests_added: usize,
    pub tests_modified: usize,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub struct CodeReviewFileCounts {
    pub total: usize,
    pub docs: usize,
    pub tests: usize,
    pub source: usize,
    pub other: usize,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct CodeReviewClassification {
    pub docs: Vec<String>,
    pub tests: Vec<String>,
    pub source: Vec<String>,
}

impl Default for CodeReviewClassification {
    fn default() -> Self {
        Self {
            docs: vec![
                "docs/**".into(),
                "*.md".into(),
                "*.mdx".into(),
                "**/*.md".into(),
                "**/*.mdx".into(),
            ],
            tests: vec![
                "tests/**".into(),
                "*.test.*".into(),
                "*.spec.*".into(),
                "*_test.rs".into(),
                "**/*.test.*".into(),
                "**/*.spec.*".into(),
                "**/*_test.rs".into(),
            ],
            source: vec![
                "src/**".into(),
                "crates/**".into(),
                "clients/**".into(),
                "apps/**".into(),
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CodeReviewPage {
    pub items: Vec<CodeReviewCommit>,
    pub next_cursor: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum ReviewTarget {
    Commit {
        commit: String,
    },
    Range {
        from: String,
        to: String,
    },
    Compare {
        from: String,
        to: String,
    },
    /// One or more ticket-changed files. `paths` also accepts a single `path` string.
    TicketFile {
        #[serde(alias = "path", deserialize_with = "one_or_many_paths")]
        paths: Vec<String>,
    },
    /// One or more changed working-tree files in one area. `paths` also accepts `path`.
    WorktreeFile {
        #[serde(alias = "path", deserialize_with = "one_or_many_paths")]
        paths: Vec<String>,
        area: WorktreeArea,
    },
}

/// Accept a single path string or a list; reject an empty list and drop repeated paths.
fn one_or_many_paths<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
    }
    let paths = match OneOrMany::deserialize(deserializer)? {
        OneOrMany::One(path) => vec![path],
        OneOrMany::Many(paths) => paths,
    };
    let mut seen = HashSet::new();
    let paths: Vec<String> = paths
        .into_iter()
        .filter(|path| seen.insert(path.clone()))
        .collect();
    if paths.is_empty() {
        return Err(serde::de::Error::custom("at least one path is required"));
    }
    Ok(paths)
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorktreeArea {
    Staged,
    Unstaged,
}

#[derive(Debug, Error)]
pub enum CodeReviewError {
    #[error("this checkout is not a readable Git repository")]
    NotRepository,
    #[error(
        "no Git diff tool is configured for this checkout; set diff.tool before opening a review"
    )]
    DifftoolNotConfigured,
    #[error("that commit or range is not associated with this ticket")]
    InvalidTarget,
    #[error("could not inspect the Git repository: {0}")]
    Git(String),
    #[error("could not launch the configured Git diff tool: {0}")]
    Launch(String),
}

pub fn discover(root: &Path, ticket_slug: &str) -> Result<CodeReview, CodeReviewError> {
    discover_with_classification(root, ticket_slug, &CodeReviewClassification::default())
}

/// Match many ticket slugs against one bounded repository-history scan.
pub fn slugs_with_commits(
    root: &Path,
    ticket_slugs: &[String],
) -> Result<HashSet<String>, CodeReviewError> {
    let (mut commits, _, _) = discover_commits(root)?;
    commits.extend(discover_unmerged_commits(root).0);
    Ok(ticket_slugs
        .iter()
        .filter(|slug| {
            commits
                .iter()
                .any(|commit| commit_mentions_ticket(commit, slug))
        })
        .cloned()
        .collect())
}

pub fn discover_with_classification(
    root: &Path,
    ticket_slug: &str,
    classification: &CodeReviewClassification,
) -> Result<CodeReview, CodeReviewError> {
    discover_ticket(root, ticket_slug, classification, None)
}

/// A qualified ticket's explicit integration records supplement slug-based Git discovery
/// (HS2-39D5YS). The old entry point remains useful for repository-only callers.
pub fn discover_for_ticket(
    root: &Path,
    ticket_slug: &str,
    classification: &CodeReviewClassification,
    ticket: &hotsheet_model::ProjectTicketRef,
) -> Result<CodeReview, CodeReviewError> {
    discover_ticket(root, ticket_slug, classification, Some(ticket))
}

#[derive(Deserialize)]
struct IntegrationRecord {
    version: u8,
    ticket: hotsheet_model::ProjectTicketRef,
    operation: String,
    remote: String,
    branch: String,
    source_commits: Vec<String>,
    shipping_commits: Vec<String>,
    remote_tip: String,
    recorded_at: String,
}

fn integration_records(
    root: &Path,
    ticket: &hotsheet_model::ProjectTicketRef,
) -> Result<Vec<IntegrationRecord>, CodeReviewError> {
    let common = git_output(
        root,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    let digest = format!("{:x}", Sha256::digest(ticket.qualified().as_bytes()));
    let directory = Path::new(common.trim())
        .join("hotsheet-integrations")
        .join(digest);
    if !directory.exists() {
        return Ok(Vec::new());
    }
    let mut records = Vec::new();
    for entry in fs::read_dir(directory).map_err(|error| CodeReviewError::Git(error.to_string()))? {
        let entry = entry.map_err(|error| CodeReviewError::Git(error.to_string()))?;
        if entry
            .path()
            .extension()
            .is_none_or(|extension| extension != "json")
        {
            continue;
        }
        let bytes =
            fs::read(entry.path()).map_err(|error| CodeReviewError::Git(error.to_string()))?;
        let record: IntegrationRecord = serde_json::from_slice(&bytes).map_err(|error| {
            CodeReviewError::Git(format!("invalid integration record: {error}"))
        })?;
        let valid_sha = |value: &str| {
            value.len() == 40
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        };
        if record.version != 1
            || record.ticket != *ticket
            || entry.file_name().to_string_lossy() != format!("{}.json", record.operation)
            || record.remote.is_empty()
            || record.branch.is_empty()
            || record.recorded_at.is_empty()
            || !valid_sha(&record.remote_tip)
            || record.source_commits.is_empty()
            || record.shipping_commits.is_empty()
            || !record.source_commits.iter().all(|value| valid_sha(value))
            || !record.shipping_commits.iter().all(|value| valid_sha(value))
        {
            return Err(CodeReviewError::Git(
                "invalid integration record identity or commits".into(),
            ));
        }
        for source in &record.source_commits {
            let retained = git_output(
                root,
                &[
                    "rev-parse",
                    &format!("refs/hotsheet/integration-source/{source}"),
                ],
            )?;
            if retained.trim() != source {
                return Err(CodeReviewError::Git(
                    "integration source is not retained".into(),
                ));
            }
        }
        for shipping in &record.shipping_commits {
            if !git_success(
                root,
                &["merge-base", "--is-ancestor", shipping, &record.remote_tip],
            ) {
                return Err(CodeReviewError::Git(
                    "integration shipping commit is not in the recorded remote tip".into(),
                ));
            }
        }
        records.push(record);
    }
    records.sort_by(|left, right| left.operation.cmp(&right.operation));
    Ok(records)
}

fn discover_ticket(
    root: &Path,
    ticket_slug: &str,
    classification: &CodeReviewClassification,
    ticket: Option<&hotsheet_model::ProjectTicketRef>,
) -> Result<CodeReview, CodeReviewError> {
    let (all, difftool, truncated) = discover_commits(root)?;
    let (unmerged, unmerged_truncated) = discover_unmerged_commits(root);
    let records = ticket
        .map(|ticket| integration_records(root, ticket))
        .transpose()?
        .unwrap_or_default();
    let checkout_behind_main = records.iter().any(|record| {
        record.branch == "main"
            && git_success(
                root,
                &["merge-base", "--is-ancestor", "HEAD", &record.remote_tip],
            )
            && !git_success(
                root,
                &["merge-base", "--is-ancestor", &record.remote_tip, "HEAD"],
            )
    });
    let reconciled: HashSet<&str> = records
        .iter()
        .flat_map(|record| record.source_commits.iter().chain(&record.shipping_commits))
        .map(String::as_str)
        .collect();
    let matching = |commits: &[CodeReviewCommit]| {
        commits
            .iter()
            .filter(|commit| commit_mentions_ticket(commit, ticket_slug))
            .cloned()
            .collect::<Vec<_>>()
    };
    let mut merged = matching(&all);
    let pending = matching(&unmerged)
        .into_iter()
        .filter(|commit| !reconciled.contains(commit.sha.as_str()))
        .collect::<Vec<_>>();
    let mut ranges = unmerged_ranges(&pending);
    ranges.extend(merged_ranges(root, &merged));
    let mut seen: HashSet<String> = merged.iter().map(|commit| commit.sha.clone()).collect();
    for shipping in records.iter().flat_map(|record| &record.shipping_commits) {
        if !seen.insert(shipping.clone()) {
            continue;
        }
        let output = git_output(root, &["log", "-n", "1", &log_format(), shipping])?;
        let commit = parse_log(&output).into_iter().next().ok_or_else(|| {
            CodeReviewError::Git(format!("integration commit {shipping} is unavailable"))
        })?;
        ranges.push(CodeReviewRange {
            from: shipping.clone(),
            to: shipping.clone(),
            count: 1,
        });
        merged.push(commit);
    }
    merged.sort_by(|left, right| right.committed_at.cmp(&left.committed_at));
    // Unmerged work is listed first: it is the newest, not-yet-integrated part of the ticket.
    let commits = pending.into_iter().chain(merged).collect::<Vec<_>>();
    let truncated = truncated || unmerged_truncated;
    let (files, file_runs) = discover_ticket_files(root, &commits, classification);
    Ok(CodeReview {
        file_runs,
        checkout_behind_main,
        follow_ups: Vec::new(),
        summary: summarize_changes(&files),
        files,
        commits,
        ranges,
        difftool,
        truncated,
    })
}

/// Discover recent repository history. When the current branch is ahead of its
/// upstream, expose that consecutive local run as the same validated range contract
/// used by ticket code review.
pub fn discover_repository(root: &Path, ahead: usize) -> Result<CodeReview, CodeReviewError> {
    let (commits, difftool, truncated) = discover_commits(root)?;
    let ranges = repository_range(&commits, ahead).into_iter().collect();
    Ok(CodeReview {
        file_runs: Vec::new(),
        checkout_behind_main: false,
        follow_ups: Vec::new(),
        summary: CodeReviewSummary::default(),
        files: Vec::new(),
        commits,
        ranges,
        difftool,
        truncated,
    })
}

/// The ticket's changed files, aggregated over its own first-parent commit runs, plus those
/// runs for file launches (HS2-F10Y9P). One span from the oldest run's parent to the newest
/// commit would also count every interleaved commit of other tickets, so each run is diffed
/// on its own and the per-run changes are composed by path ([`compose_run_changes`]). A
/// ticket with a single run keeps that run's plain diff unchanged.
fn discover_ticket_files(
    root: &Path,
    commits: &[CodeReviewCommit],
    classification: &CodeReviewClassification,
) -> (Vec<CodeReviewFile>, Vec<FileRun>) {
    let chains = order_runs(root, first_parent_runs(&file_commits(commits)));
    let mut runs = Vec::new();
    let mut changes = Vec::new();
    for (oldest, newest) in chains {
        let base = oldest.parents.first().cloned().or_else(|| {
            git_output(root, &["hash-object", "-t", "tree", "--stdin"])
                .ok()
                .map(|value| value.trim().to_owned())
        });
        let Some(base) = base else {
            return (Vec::new(), Vec::new());
        };
        let Ok(output) = git_output(
            root,
            &[
                "diff",
                "--name-status",
                "-M",
                "-C",
                "-z",
                &base,
                &newest.sha,
            ],
        ) else {
            return (Vec::new(), Vec::new());
        };
        changes.push(parse_changed_files(&output, classification));
        runs.push(FileRun {
            base,
            newest: newest.sha.clone(),
            touched: BTreeMap::new(),
        });
    }
    let (files, touched) = compose_run_changes(changes, classification);
    for (run, touched) in runs.iter_mut().zip(touched) {
        run.touched = touched;
    }
    (files, runs)
}

/// The commits whose trees the changed-files summary may compare: the ticket's merged
/// commits when it has any, otherwise the unmerged commits of the newest commit's source
/// branch (HS2-0WZRK5), so a file diff never mixes trees from unrelated branches.
fn file_commits(commits: &[CodeReviewCommit]) -> Vec<&CodeReviewCommit> {
    let Some(first) = commits.first() else {
        return Vec::new();
    };
    let source = if commits.iter().any(|commit| commit.unmerged.is_none()) {
        None
    } else {
        first.unmerged.clone()
    };
    let mut seen = HashSet::new();
    commits
        .iter()
        .filter(|commit| commit.unmerged == source && seen.insert(commit.sha.as_str()))
        .collect()
}

/// Split commits into maximal first-parent runs as `(oldest, newest)` pairs (HS2-F10Y9P).
/// A run continues only while exactly one selected commit has the previous one as its
/// first parent, so a run never spans a commit outside the selection. On linear history
/// these are the same runs as date-ordered log adjacency would give; across merges they are
/// stricter, since adjacency in a date-ordered log does not imply a parent link. Merged
/// commit ranges ([`merged_ranges`]) use the same runs (HS2-0WPFZV).
fn first_parent_runs<'a>(
    selected: &[&'a CodeReviewCommit],
) -> Vec<(&'a CodeReviewCommit, &'a CodeReviewCommit)> {
    let by_sha: HashSet<&str> = selected.iter().map(|commit| commit.sha.as_str()).collect();
    let mut children: HashMap<&str, Vec<&'a CodeReviewCommit>> = HashMap::new();
    for commit in selected {
        if let Some(parent) = commit.parents.first().map(String::as_str)
            && by_sha.contains(parent)
        {
            children.entry(parent).or_default().push(commit);
        }
    }
    let only_child = |sha: &str| match children.get(sha).map(Vec::as_slice) {
        Some([child]) => Some(*child),
        _ => None,
    };
    let mut runs = Vec::new();
    for commit in selected {
        let continues = commit
            .parents
            .first()
            .is_some_and(|parent| only_child(parent).is_some_and(|child| child.sha == commit.sha));
        if continues {
            continue;
        }
        let mut newest = *commit;
        while let Some(child) = only_child(&newest.sha) {
            newest = child;
        }
        runs.push((*commit, newest));
    }
    runs
}

/// Order runs oldest first by topology, not by `committed_at`, which rebases and clock skew
/// can misorder (HS2-F10Y9P). One bounded `git rev-list --topo-order` over the run tips
/// ranks them; a tip outside that window (or a failed call) counts as older than every
/// ranked tip and falls back to commit time.
fn order_runs<'a>(
    root: &Path,
    mut runs: Vec<(&'a CodeReviewCommit, &'a CodeReviewCommit)>,
) -> Vec<(&'a CodeReviewCommit, &'a CodeReviewCommit)> {
    if runs.len() < 2 {
        return runs;
    }
    let limit = LOG_LIMIT.to_string();
    let mut args = vec!["rev-list", "--topo-order", "-n", &limit];
    args.extend(runs.iter().map(|(_, newest)| newest.sha.as_str()));
    let ranks: HashMap<String, usize> = git_output(root, &args)
        .map(|output| {
            output
                .lines()
                .enumerate()
                .map(|(rank, sha)| (sha.trim().to_owned(), rank))
                .collect()
        })
        .unwrap_or_default();
    runs.sort_by_cached_key(|(_, newest)| match ranks.get(&newest.sha) {
        // `--topo-order` lists descendants first, so a higher rank is older.
        Some(rank) => (1, usize::MAX - rank, String::new(), String::new()),
        None => (0, 0, newest.committed_at.clone(), newest.sha.clone()),
    });
    runs
}

/// One summary file's lineage while runs are composed: its net change against the state
/// before the ticket's first run, and the runs (with run-local pathspecs) that touched it.
struct Lineage {
    file: CodeReviewFile,
    touches: Vec<(usize, Vec<String>)>,
}

/// Compose per-run changes, oldest run first, into the net change of each path against the
/// state before the ticket's first run (HS2-F10Y9P). Returns the files sorted by path and,
/// per run, the summary paths it touched with their run-local pathspecs. A single run is
/// returned unchanged. Rules for a path one run changes and a later run changes again:
///
/// - Added, then modified or type-changed: still Added. Added, then deleted: dropped,
///   because the path neither existed before nor exists after.
/// - Modified, then deleted: Deleted. Modified, then type-changed: TypeChanged.
/// - Deleted, then added again: Modified, because the path existed before and after.
/// - Renamed or copied, then modified: keeps the rename or copy and its origin.
/// - Renamed, then deleted: Deleted at the origin. Copied, then deleted: dropped.
/// - A rename of a path an earlier run added stays Added at the new path; a rename of an
///   earlier rename keeps the first origin, so the chain `a → b`, `b → c` reports `a → c`.
/// - A rename or copy onto a path an earlier run deleted is Modified there; a renamed
///   origin that existed before the ticket is then Deleted.
/// - Adding a path back that an earlier run renamed away turns that rename into a copy,
///   and the re-added path is Modified.
fn compose_run_changes(
    runs: Vec<Vec<CodeReviewFile>>,
    classification: &CodeReviewClassification,
) -> (Vec<CodeReviewFile>, Vec<BTreeMap<String, Vec<String>>>) {
    use CodeReviewFileChange::{Added, Copied, Deleted, Modified, Renamed, TypeChanged};
    let count = runs.len();
    if count == 1 {
        let files = runs.into_iter().next().unwrap_or_default();
        let touched = files
            .iter()
            .map(|file| (file.path.clone(), vec![file.path.clone()]))
            .collect();
        return (files, vec![touched]);
    }
    let deleted = |path: String, touches| Lineage {
        file: CodeReviewFile {
            category: classify(&path, classification),
            path,
            original_path: None,
            change: Deleted,
        },
        touches,
    };
    let mut current: HashMap<String, Lineage> = HashMap::new();
    for (run, files) in runs.into_iter().enumerate() {
        for file in files {
            let path = file.path.clone();
            if let (Renamed | Copied, Some(original)) = (file.change, file.original_path.clone()) {
                let prior = (file.change == Renamed)
                    .then(|| current.remove(&original))
                    .flatten();
                let source = prior
                    .as_ref()
                    .map(|lineage| &lineage.file)
                    .or_else(|| current.get(&original).map(|lineage| &lineage.file));
                let (change, origin) = match source {
                    Some(earlier) if earlier.change == Added => (Added, None),
                    Some(earlier) if matches!(earlier.change, Renamed | Copied) => (
                        file.change,
                        earlier.original_path.clone().or(Some(original.clone())),
                    ),
                    _ => (file.change, Some(original.clone())),
                };
                let mut touches = prior.map(|lineage| lineage.touches).unwrap_or_default();
                touches.push((run, vec![original, path.clone()]));
                let mut net = CodeReviewFile {
                    original_path: origin,
                    change,
                    ..file
                };
                if let Some(destination) = current.remove(&path) {
                    let own = touches.clone();
                    touches = destination.touches.into_iter().chain(touches).collect();
                    if destination.file.change == Deleted {
                        // The destination existed before the ticket and holds new content.
                        if net.change == Renamed
                            && let Some(origin) = net.original_path.clone()
                        {
                            current.insert(origin.clone(), deleted(origin, own));
                        }
                        net.change = Modified;
                        net.original_path = None;
                    }
                }
                current.insert(path, Lineage { file: net, touches });
                continue;
            }
            let spec = vec![path.clone()];
            let Some(mut prior) = current.remove(&path) else {
                let mut change = file.change;
                if change == Added
                    && let Some(moved) = current.values_mut().find(|lineage| {
                        lineage.file.change == Renamed
                            && lineage.file.original_path.as_deref() == Some(path.as_str())
                    })
                {
                    moved.file.change = Copied;
                    change = Modified;
                }
                current.insert(
                    path,
                    Lineage {
                        file: CodeReviewFile {
                            original_path: None,
                            change,
                            ..file
                        },
                        touches: vec![(run, spec)],
                    },
                );
                continue;
            };
            prior.touches.push((run, spec));
            let net = match (prior.file.change, file.change) {
                (Added | Copied, Deleted) => None,
                (Added, _) => Some(Added),
                (Deleted, Added) => Some(Modified),
                (Renamed, Deleted) => {
                    if let Some(origin) = prior.file.original_path.take() {
                        let mut touches = std::mem::take(&mut prior.touches);
                        if let Some(existing) = current.remove(&origin) {
                            touches = existing.touches.into_iter().chain(touches).collect();
                        }
                        current.insert(origin.clone(), deleted(origin, touches));
                    }
                    None
                }
                (Renamed | Copied, _) => Some(prior.file.change),
                (Modified | TypeChanged, Deleted) => Some(Deleted),
                (TypeChanged, _) | (_, TypeChanged) => Some(TypeChanged),
                (_, Added | Modified) => Some(Modified),
                (_, later) => Some(later),
            };
            if let Some(net) = net {
                prior.file.change = net;
                if !matches!(net, Renamed | Copied) {
                    prior.file.original_path = None;
                }
                current.insert(path, prior);
            }
        }
    }
    let mut touched = vec![BTreeMap::<String, Vec<String>>::new(); count];
    let mut files = Vec::with_capacity(current.len());
    for (path, lineage) in current {
        for (run, spec) in lineage.touches {
            let entry = touched[run].entry(path.clone()).or_default();
            for name in spec {
                if !entry.contains(&name) {
                    entry.push(name);
                }
            }
        }
        files.push(lineage.file);
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    (files, touched)
}

fn classify(path: &str, classification: &CodeReviewClassification) -> CodeReviewFileCategory {
    if matches_any(&classification.docs, path) {
        CodeReviewFileCategory::Docs
    } else if matches_any(&classification.tests, path) {
        CodeReviewFileCategory::Tests
    } else if matches_any(&classification.source, path) {
        CodeReviewFileCategory::Source
    } else {
        CodeReviewFileCategory::Other
    }
}

fn parse_changed_files(
    output: &str,
    classification: &CodeReviewClassification,
) -> Vec<CodeReviewFile> {
    let mut fields = output.split('\0').filter(|field| !field.is_empty());
    let mut files = Vec::new();
    while let Some(status) = fields.next() {
        let code = status.chars().next().unwrap_or('M');
        let (original_path, path) = if matches!(code, 'R' | 'C') {
            let (Some(original), Some(path)) = (fields.next(), fields.next()) else {
                break;
            };
            (Some(original.replace('\\', "/")), path)
        } else {
            let Some(path) = fields.next() else { break };
            (None, path)
        };
        let path = path.replace('\\', "/");
        let category = classify(&path, classification);
        let change = match code {
            'A' => CodeReviewFileChange::Added,
            'C' => CodeReviewFileChange::Copied,
            'D' => CodeReviewFileChange::Deleted,
            'R' => CodeReviewFileChange::Renamed,
            'T' => CodeReviewFileChange::TypeChanged,
            _ => CodeReviewFileChange::Modified,
        };
        files.push(CodeReviewFile {
            path,
            original_path,
            change,
            category,
        });
    }
    files
}

fn summarize_changes(files: &[CodeReviewFile]) -> CodeReviewSummary {
    let mut summary = CodeReviewSummary::default();
    for file in files {
        summary.files.total += 1;
        match file.category {
            CodeReviewFileCategory::Docs => summary.files.docs += 1,
            CodeReviewFileCategory::Tests => {
                summary.files.tests += 1;
                if file.change == CodeReviewFileChange::Added {
                    summary.tests_added += 1
                } else {
                    summary.tests_modified += 1
                }
            }
            CodeReviewFileCategory::Source => summary.files.source += 1,
            CodeReviewFileCategory::Other => summary.files.other += 1,
        }
    }
    summary
}

fn matches_any(patterns: &[String], path: &str) -> bool {
    patterns.iter().any(|pattern| glob_matches(pattern, path))
}

fn glob_matches(pattern: &str, path: &str) -> bool {
    let pattern = pattern.as_bytes();
    let path = path.as_bytes();
    let (mut p, mut s, mut star, mut retry) = (0, 0, None, 0);
    while s < path.len() {
        if p < pattern.len() && pattern[p] == b'*' {
            while p < pattern.len() && pattern[p] == b'*' {
                p += 1
            }
            star = Some(p);
            retry = s;
            continue;
        }
        if p < pattern.len() && (pattern[p] == b'?' || pattern[p] == path[s]) {
            p += 1;
            s += 1;
            continue;
        }
        if let Some(after) = star {
            retry += 1;
            s = retry;
            p = after;
            continue;
        }
        return false;
    }
    while p < pattern.len() && pattern[p] == b'*' {
        p += 1
    }
    p == pattern.len()
}

pub fn discover_repository_metadata(
    root: &Path,
    ahead: usize,
) -> Result<(Vec<CodeReviewRange>, Option<String>), CodeReviewError> {
    if !git_success(root, &["rev-parse", "--git-dir"]) {
        return Err(CodeReviewError::NotRepository);
    }
    let difftool = git_output(root, &["config", "--get", "diff.tool"])
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    let ranges = if ahead > 1 && git_success(root, &["rev-parse", "--verify", "HEAD"]) {
        let to = git_output(root, &["rev-parse", "HEAD"])?.trim().to_owned();
        let from = git_output(root, &["rev-parse", &format!("HEAD~{}", ahead - 1)])?
            .trim()
            .to_owned();
        vec![CodeReviewRange {
            from,
            to,
            count: ahead,
        }]
    } else {
        Vec::new()
    };
    Ok((ranges, difftool))
}

pub fn discover_repository_page(
    root: &Path,
    cursor: usize,
    limit: usize,
) -> Result<CodeReviewPage, CodeReviewError> {
    if !git_success(root, &["rev-parse", "--git-dir"]) {
        return Err(CodeReviewError::NotRepository);
    }
    let requested = limit.clamp(1, 100);
    let output = match git_output(
        root,
        &[
            "log",
            &format!("--skip={cursor}"),
            "-n",
            &(requested + 1).to_string(),
            &log_format(),
            "HEAD",
        ],
    ) {
        Ok(output) => output,
        Err(_) if !git_success(root, &["rev-parse", "--verify", "HEAD"]) => String::new(),
        Err(error) => return Err(error),
    };
    let mut items = parse_log(&output);
    let has_more = items.len() > requested;
    items.truncate(requested);
    Ok(CodeReviewPage {
        next_cursor: has_more.then_some(cursor + items.len()),
        items,
    })
}

fn repository_range(commits: &[CodeReviewCommit], ahead: usize) -> Option<CodeReviewRange> {
    let count = ahead.min(commits.len());
    (count > 1).then(|| CodeReviewRange {
        from: commits[count - 1].sha.clone(),
        to: commits[0].sha.clone(),
        count,
    })
}

fn discover_commits(
    root: &Path,
) -> Result<(Vec<CodeReviewCommit>, Option<String>, bool), CodeReviewError> {
    if !git_success(root, &["rev-parse", "--git-dir"]) {
        return Err(CodeReviewError::NotRepository);
    }
    let difftool = git_output(root, &["config", "--get", "diff.tool"])
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty());
    let output = match git_output(
        root,
        &[
            "log",
            "-n",
            &LOG_LIMIT.to_string(),
            "--decorate=short",
            &log_format(),
            "HEAD",
        ],
    ) {
        Ok(output) => output,
        Err(_) if !git_success(root, &["rev-parse", "--verify", "HEAD"]) => String::new(),
        Err(error) => return Err(error),
    };
    let all = parse_log(&output);
    let truncated = all.len() == LOG_LIMIT;
    Ok((all, difftool, truncated))
}

/// Commits reachable from a local branch or another worktree's detached `HEAD` but not
/// from this checkout's `HEAD`, each tagged with where it was found (HS2-0WZRK5). Best
/// effort: an unreadable repository, an unborn `HEAD`, or a failing scan yields none.
/// Returns whether the scan hit the log limit.
fn discover_unmerged_commits(root: &Path) -> (Vec<CodeReviewCommit>, bool) {
    if !git_success(root, &["rev-parse", "--verify", "HEAD"]) {
        return (Vec::new(), false);
    }
    let worktrees = git_output(root, &["worktree", "list", "--porcelain"])
        .map(|output| parse_worktrees(&output))
        .unwrap_or_default();
    let limit = LOG_LIMIT.to_string();
    let format = format!(
        "{}{FIELD_SEPARATOR}%S{RECORD_SEPARATOR}",
        log_format_fields()
    );
    let mut args = vec![
        "log",
        "-n",
        &limit,
        "--decorate=short",
        "--source",
        &format,
        "--branches",
    ];
    // Detached worktree HEADs are not branches; name them as extra tips.
    args.extend(
        worktrees
            .keys()
            .filter(|tip| !tip.starts_with("refs/"))
            .map(String::as_str),
    );
    args.extend(["--not", "HEAD"]);
    let Ok(output) = git_output(root, &args) else {
        return (Vec::new(), false);
    };
    let mut commits = parse_log(&output);
    for commit in &mut commits {
        let source = commit.unmerged.take().and_then(|source| source.branch);
        commit.unmerged = Some(unmerged_source(source.as_deref(), &worktrees));
    }
    let truncated = commits.len() == LOG_LIMIT;
    (commits, truncated)
}

/// Map each worktree's checked-out tip (`refs/heads/<branch>`, or a detached commit sha)
/// to its directory, from `git worktree list --porcelain`.
fn parse_worktrees(output: &str) -> HashMap<String, String> {
    let mut tips = HashMap::new();
    for block in output.split("\n\n") {
        let mut path = None;
        let mut head = None;
        let mut branch = None;
        for line in block.lines() {
            if let Some(value) = line.strip_prefix("worktree ") {
                path = Some(value.to_owned());
            } else if let Some(value) = line.strip_prefix("HEAD ") {
                head = Some(value.to_owned());
            } else if let Some(value) = line.strip_prefix("branch ") {
                branch = Some(value.to_owned());
            }
        }
        if let (Some(path), Some(tip)) = (path, branch.or(head)) {
            tips.insert(tip, path);
        }
    }
    tips
}

/// Describe a `git log --source` source (`refs/heads/<branch>` or a commit sha).
fn unmerged_source(source: Option<&str>, worktrees: &HashMap<String, String>) -> UnmergedSource {
    let source = source.unwrap_or_default();
    // `--branches` reports short branch names; a detached tip is the 40-hex sha we passed.
    let detached = source.len() == 40 && source.bytes().all(|byte| byte.is_ascii_hexdigit());
    if detached {
        return UnmergedSource {
            branch: None,
            worktree: worktrees.get(source).cloned(),
        };
    }
    let branch = source.strip_prefix("refs/heads/").unwrap_or(source);
    UnmergedSource {
        branch: (!branch.is_empty()).then(|| branch.to_owned()),
        worktree: worktrees.get(&format!("refs/heads/{branch}")).cloned(),
    }
}

/// Reviewable runs among unmerged ticket commits: adjacent commits from the same source
/// whose first parent is the next (older) commit in the run, newest run first.
fn unmerged_ranges(matched: &[CodeReviewCommit]) -> Vec<CodeReviewRange> {
    let mut ranges = Vec::new();
    let mut run: Vec<&CodeReviewCommit> = Vec::new();
    let flush = |run: &mut Vec<&CodeReviewCommit>, ranges: &mut Vec<CodeReviewRange>| {
        if let (Some(newest), Some(oldest)) = (run.first(), run.last()) {
            ranges.push(CodeReviewRange {
                from: oldest.sha.clone(),
                to: newest.sha.clone(),
                count: run.len(),
            });
        }
        run.clear();
    };
    let mut sources: Vec<&Option<UnmergedSource>> = Vec::new();
    for commit in matched {
        if !sources.contains(&&commit.unmerged) {
            sources.push(&commit.unmerged);
        }
    }
    for source in sources {
        for commit in matched.iter().filter(|commit| &commit.unmerged == source) {
            let linked = run
                .last()
                .is_some_and(|previous| previous.parents.first() == Some(&commit.sha));
            if !linked {
                flush(&mut run, &mut ranges);
            }
            run.push(commit);
        }
        flush(&mut run, &mut ranges);
    }
    ranges
}

fn log_format() -> String {
    format!("{}{RECORD_SEPARATOR}", log_format_fields())
}

fn log_format_fields() -> String {
    format!(
        "--format=%H{FIELD_SEPARATOR}%h{FIELD_SEPARATOR}%P{FIELD_SEPARATOR}%cI{FIELD_SEPARATOR}%s{FIELD_SEPARATOR}%b{FIELD_SEPARATOR}%D"
    )
}

pub fn launch(
    root: &Path,
    review: &CodeReview,
    target: &ReviewTarget,
) -> Result<(), CodeReviewError> {
    if review.difftool.is_none() {
        return Err(CodeReviewError::DifftoolNotConfigured);
    }
    if let ReviewTarget::TicketFile { paths } = target {
        for (old, new, pathspec) in ticket_file_launches(review, paths)? {
            spawn_difftool_files(root, (old, new), &pathspec)?;
        }
        return Ok(());
    }
    let (old, new) = launch_revisions(root, review, target)?;
    spawn_difftool(root, (old, new))
}

/// One ticket-file difftool launch: base revision, tip revision, and pathspec.
type FileLaunch<'a> = (&'a str, &'a str, Vec<String>);

/// The difftool launches for ticket files: one per first-parent run that touched any of
/// the paths, oldest run first, each over that run's own base, tip, and run-local
/// pathspec, so a file launch never diffs across another ticket's interleaved commits
/// (HS2-F10Y9P). A single-run ticket gets one launch over exactly the requested paths.
/// Every path must be discovered ticket evidence.
fn ticket_file_launches<'a>(
    review: &'a CodeReview,
    paths: &[String],
) -> Result<Vec<FileLaunch<'a>>, CodeReviewError> {
    if paths.is_empty()
        || !paths
            .iter()
            .all(|path| review.files.iter().any(|file| file.path == *path))
    {
        return Err(CodeReviewError::InvalidTarget);
    }
    let launches = review
        .file_runs
        .iter()
        .filter_map(|run| {
            let mut pathspec: Vec<String> = Vec::new();
            for name in paths
                .iter()
                .filter_map(|path| run.touched.get(path))
                .flatten()
            {
                if !pathspec.contains(name) {
                    pathspec.push(name.clone());
                }
            }
            (!pathspec.is_empty()).then_some((run.base.as_str(), run.newest.as_str(), pathspec))
        })
        .collect::<Vec<_>>();
    if launches.is_empty() {
        return Err(CodeReviewError::InvalidTarget);
    }
    Ok(launches)
}

pub fn launch_repository(
    root: &Path,
    ahead: usize,
    target: &ReviewTarget,
) -> Result<(), CodeReviewError> {
    let (ranges, difftool) = discover_repository_metadata(root, ahead)?;
    if difftool.is_none() {
        return Err(CodeReviewError::DifftoolNotConfigured);
    }
    let revisions = match target {
        ReviewTarget::Commit { commit } => {
            validate_reachable_commit(root, commit)?;
            (commit_parent_or_empty_tree(root, commit)?, commit.clone())
        }
        ReviewTarget::Range { from, to } => {
            let range = ranges
                .iter()
                .find(|range| range.from == *from && range.to == *to)
                .ok_or(CodeReviewError::InvalidTarget)?;
            (
                commit_parent_or_empty_tree(root, &range.from)?,
                range.to.clone(),
            )
        }
        ReviewTarget::Compare { from, to } => {
            if from == to {
                return Err(CodeReviewError::InvalidTarget);
            }
            validate_reachable_commit(root, from)?;
            validate_reachable_commit(root, to)?;
            (from.clone(), to.clone())
        }
        ReviewTarget::WorktreeFile { paths, area } => {
            return launch_worktree_files(root, paths, *area);
        }
        ReviewTarget::TicketFile { .. } => return Err(CodeReviewError::InvalidTarget),
    };
    spawn_difftool(root, revisions)
}

fn launch_worktree_files(
    root: &Path,
    paths: &[String],
    area: WorktreeArea,
) -> Result<(), CodeReviewError> {
    let args = match area {
        WorktreeArea::Staged => ["diff", "--name-only", "-z", "--cached"].as_slice(),
        WorktreeArea::Unstaged => ["diff", "--name-only", "-z"].as_slice(),
    };
    let output = git_output(root, args)?;
    let changed: HashSet<&str> = output.split('\0').collect();
    if paths.is_empty() || !paths.iter().all(|path| changed.contains(path.as_str())) {
        return Err(CodeReviewError::InvalidTarget);
    }
    let tool = configured_difftool(root)?;
    let mut command = hotsheet_ticketing::git::command();
    command.arg("-C").arg(root).args(difftool_args(&tool, &[]));
    if area == WorktreeArea::Staged {
        command.arg("--cached");
    }
    spawn_detached(command.arg("--").args(paths))
}

/// `git difftool` arguments before the pathspec. Several files share one
/// `git difftool --no-prompt -- <paths…>` invocation so the configured tool receives
/// each file as an ordinary per-file diff against the real working tree, never a
/// `--dir-diff` temporary copy (HS2-J7HQ5E, HS2-TRJ9P7).
/// `git difftool` arguments that name the configured tool explicitly (`--tool=<name>`).
/// Without it, a helper that can no longer read the repository's `diff.tool` (the repository
/// moved or was deleted mid-launch) falls back to git's platform default, which on macOS is
/// `opendiff`/FileMerge, opening a tool the user never configured (HS2-YA1N0S).
fn difftool_args(tool: &str, revisions: &[&str]) -> Vec<String> {
    let mut args = vec![
        "difftool".to_owned(),
        "--no-prompt".to_owned(),
        format!("--tool={tool}"),
    ];
    args.extend(revisions.iter().map(|revision| (*revision).to_owned()));
    args
}

/// The repository's configured `diff.tool`, required before any launch.
fn configured_difftool(root: &Path) -> Result<String, CodeReviewError> {
    git_output(root, &["config", "--get", "diff.tool"])
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .ok_or(CodeReviewError::DifftoolNotConfigured)
}

fn spawn_detached(command: &mut Command) -> Result<(), CodeReviewError> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    hotsheet_process_journal::LaunchContext {
        journal: hotsheet_process_journal::ProcessJournal::at(hotsheet_plugins::hotsheet_home()),
        supervisor: "hotsheet-server",
        purpose: hotsheet_process_journal::Purpose::CodeReviewDifftool,
        project_id: None,
        session_id: None,
        binary_revision: option_env!("HOT_SHEET_BUILD_REVISION").map(str::to_owned),
    }
    .detached(command)
    .map_err(|error| CodeReviewError::Launch(error.to_string()))
}

fn validate_reachable_commit(root: &Path, commit: &str) -> Result<(), CodeReviewError> {
    if commit.len() != 40 || !commit.bytes().all(|value| value.is_ascii_hexdigit()) {
        return Err(CodeReviewError::InvalidTarget);
    }
    if !git_success(root, &["merge-base", "--is-ancestor", commit, "HEAD"]) {
        return Err(CodeReviewError::InvalidTarget);
    }
    Ok(())
}

fn commit_parent_or_empty_tree(root: &Path, commit: &str) -> Result<String, CodeReviewError> {
    git_output(root, &["rev-parse", &format!("{commit}^")])
        .map(|value| value.trim().to_owned())
        .or_else(|_| {
            git_output(root, &["hash-object", "-t", "tree", "--stdin"])
                .map(|value| value.trim().to_owned())
        })
}

fn spawn_difftool(root: &Path, (old, new): (String, String)) -> Result<(), CodeReviewError> {
    let tool = configured_difftool(root)?;
    let mut command = hotsheet_ticketing::git::command();
    command
        .arg("-C")
        .arg(root)
        .args(difftool_args(&tool, &[&old, &new]));
    spawn_detached(&mut command)
}

fn spawn_difftool_files(
    root: &Path,
    (old, new): (&str, &str),
    paths: &[String],
) -> Result<(), CodeReviewError> {
    let tool = configured_difftool(root)?;
    let mut command = hotsheet_ticketing::git::command();
    command
        .arg("-C")
        .arg(root)
        .args(difftool_args(&tool, &[old, new]))
        .arg("--")
        .args(paths);
    spawn_detached(&mut command)
}

fn launch_revisions(
    root: &Path,
    review: &CodeReview,
    target: &ReviewTarget,
) -> Result<(String, String), CodeReviewError> {
    match target {
        ReviewTarget::Commit { commit } => {
            let found = review
                .commits
                .iter()
                .find(|candidate| candidate.sha == *commit)
                .ok_or(CodeReviewError::InvalidTarget)?;
            let old = match found.parents.first() {
                Some(parent) => parent.clone(),
                None => git_output(root, &["hash-object", "-t", "tree", "--stdin"])?
                    .trim()
                    .to_owned(),
            };
            Ok((old, found.sha.clone()))
        }
        ReviewTarget::Range { from, to } => {
            let range = review
                .ranges
                .iter()
                .find(|candidate| {
                    candidate.from == *from && candidate.to == *to && candidate.count > 1
                })
                .ok_or(CodeReviewError::InvalidTarget)?;
            let oldest = review
                .commits
                .iter()
                .find(|candidate| candidate.sha == range.from)
                .ok_or(CodeReviewError::InvalidTarget)?;
            let old = match oldest.parents.first() {
                Some(parent) => parent.clone(),
                None => git_output(root, &["hash-object", "-t", "tree", "--stdin"])?
                    .trim()
                    .to_owned(),
            };
            Ok((old, range.to.clone()))
        }
        ReviewTarget::Compare { from, to } => {
            if from == to {
                return Err(CodeReviewError::InvalidTarget);
            }
            let from = review
                .commits
                .iter()
                .find(|candidate| candidate.sha == *from)
                .ok_or(CodeReviewError::InvalidTarget)?;
            let to = review
                .commits
                .iter()
                .find(|candidate| candidate.sha == *to)
                .ok_or(CodeReviewError::InvalidTarget)?;
            Ok((from.sha.clone(), to.sha.clone()))
        }
        // Ticket files launch per run through `ticket_file_launches` (HS2-F10Y9P).
        ReviewTarget::TicketFile { .. } | ReviewTarget::WorktreeFile { .. } => {
            Err(CodeReviewError::InvalidTarget)
        }
    }
}

fn git_output(root: &Path, args: &[&str]) -> Result<String, CodeReviewError> {
    let output = hotsheet_ticketing::git::command()
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|error| CodeReviewError::Git(error.to_string()))?;
    if !output.status.success() {
        return Err(CodeReviewError::Git(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn git_success(root: &Path, args: &[&str]) -> bool {
    hotsheet_ticketing::git::command()
        .arg("-C")
        .arg(root)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn parse_log(output: &str) -> Vec<CodeReviewCommit> {
    output
        .split(RECORD_SEPARATOR)
        .filter_map(|record| {
            let record = record.trim_start_matches(['\r', '\n']);
            if record.is_empty() {
                return None;
            }
            let mut fields = record.splitn(8, FIELD_SEPARATOR);
            Some(CodeReviewCommit {
                sha: fields.next()?.to_owned(),
                short_sha: fields.next()?.to_owned(),
                parents: fields
                    .next()?
                    .split_whitespace()
                    .map(str::to_owned)
                    .collect(),
                committed_at: fields.next()?.to_owned(),
                subject: fields.next()?.to_owned(),
                body: fields.next()?.trim_end_matches(['\r', '\n']).to_owned(),
                refs: parse_decorations(fields.next().unwrap_or("").trim_matches(['\r', '\n'])),
                // A `--source` scan appends the reaching ref; carried here until
                // `discover_unmerged_commits` resolves it.
                unmerged: fields
                    .next()
                    .map(|source| source.trim_matches(['\r', '\n']))
                    .filter(|source| !source.is_empty())
                    .map(|source| UnmergedSource {
                        branch: Some(source.to_owned()),
                        worktree: None,
                    }),
            })
        })
        .collect()
}

fn subject_mentions_ticket(subject: &str, ticket_slug: &str) -> bool {
    let subject = subject.to_ascii_lowercase();
    let needle = ticket_slug.to_ascii_lowercase();
    subject.match_indices(&needle).any(|(start, _)| {
        let before = subject[..start].chars().next_back();
        let after = subject[start + needle.len()..].chars().next();
        before.is_none_or(|value| !is_ticket_char(value))
            && after.is_none_or(|value| !is_ticket_char(value))
    })
}

fn commit_mentions_ticket(commit: &CodeReviewCommit, ticket_slug: &str) -> bool {
    subject_mentions_ticket(&commit.subject, ticket_slug)
        || commit
            .body
            .lines()
            .any(|line| refs_line_mentions_ticket(line, ticket_slug))
}

fn refs_line_mentions_ticket(line: &str, ticket_slug: &str) -> bool {
    let line = line.trim();
    let Some(key) = line.get(..4) else {
        return false;
    };
    if !key.eq_ignore_ascii_case("refs") {
        return false;
    }
    let suffix = &line[4..];
    if !suffix.starts_with(':') && !suffix.starts_with(char::is_whitespace) {
        return false;
    }
    let references = suffix.trim_start_matches(|value: char| value == ':' || value.is_whitespace());
    !references.is_empty() && subject_mentions_ticket(references, ticket_slug)
}

fn is_ticket_char(value: char) -> bool {
    value.is_ascii_alphanumeric() || value == '_' || value == '-'
}

/// Reviewable ranges among the ticket's merged commits, newest run first (HS2-0WPFZV).
/// Each range is one maximal first-parent run ([`first_parent_runs`]) ordered by topology
/// ([`order_runs`]), so a `from^..to` launch never spans another ticket's commits even when
/// a date-ordered log lists two ticket commits next to each other across a merge.
fn merged_ranges(root: &Path, merged: &[CodeReviewCommit]) -> Vec<CodeReviewRange> {
    let mut seen = HashSet::new();
    let selected = merged
        .iter()
        .filter(|commit| seen.insert(commit.sha.as_str()))
        .collect::<Vec<_>>();
    let by_sha: HashMap<&str, &CodeReviewCommit> = selected
        .iter()
        .map(|commit| (commit.sha.as_str(), *commit))
        .collect();
    let mut runs = order_runs(root, first_parent_runs(&selected));
    runs.reverse();
    runs.into_iter()
        .map(|(oldest, newest)| {
            let mut count = 1;
            let mut current = newest;
            while current.sha != oldest.sha {
                let Some(parent) = current
                    .parents
                    .first()
                    .and_then(|parent| by_sha.get(parent.as_str()))
                else {
                    break;
                };
                current = parent;
                count += 1;
            }
            CodeReviewRange {
                from: oldest.sha.clone(),
                to: newest.sha.clone(),
                count,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hs2_39d5ys_qualified_mapping_keeps_new_branch_work_pending() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        let git = |args: &[&str]| {
            let output = hotsheet_ticketing::git::command_in(root)
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8_lossy(&output.stdout).trim().to_owned()
        };
        git(&["init", "-b", "main"]);
        git(&["config", "user.name", "Synthetic Test"]);
        git(&["config", "user.email", "test@invalid.example"]);
        fs::write(root.join("seed"), "seed\n").unwrap();
        git(&["add", "seed"]);
        git(&["commit", "-m", "Seed"]);
        git(&["switch", "-c", "worker"]);
        fs::write(root.join("work"), "original\n").unwrap();
        git(&["add", "work"]);
        git(&["commit", "-m", "HS2-39D5YS Worker source"]);
        let source = git(&["rev-parse", "HEAD"]);
        fs::write(root.join("later"), "later\n").unwrap();
        git(&["add", "later"]);
        git(&["commit", "-m", "HS2-39D5YS Follow-on work"]);
        let later = git(&["rev-parse", "HEAD"]);
        git(&["switch", "main"]);
        fs::write(root.join("work"), "corrected\n").unwrap();
        git(&["add", "work"]);
        git(&["commit", "-m", "Reviewed correction"]);
        let shipping = git(&["rev-parse", "HEAD"]);
        git(&[
            "update-ref",
            &format!("refs/hotsheet/integration-source/{source}"),
            &source,
        ]);
        let ticket = hotsheet_model::ProjectTicketRef {
            project_id: "project-a".into(),
            connection_id: "connection-a".into(),
            native_id: "ticket-123".into(),
        };
        let digest = format!("{:x}", Sha256::digest(ticket.qualified().as_bytes()));
        let directory = root.join(".git/hotsheet-integrations").join(digest);
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join("handoff.json"),
            serde_json::json!({
                "version": 1,
                "ticket": ticket,
                "operation": "handoff",
                "remote": "origin",
                "branch": "main",
                "source_commits": [source],
                "shipping_commits": [shipping],
                "remote_tip": shipping,
                "recorded_at": "2026-10-29T00:00:00Z"
            })
            .to_string(),
        )
        .unwrap();
        let review = discover_for_ticket(
            root,
            "HS2-39D5YS",
            &CodeReviewClassification::default(),
            &ticket,
        )
        .unwrap();
        assert!(
            review
                .commits
                .iter()
                .any(|commit| commit.sha == shipping && commit.unmerged.is_none())
        );
        assert!(
            review
                .commits
                .iter()
                .any(|commit| commit.sha == later && commit.unmerged.is_some())
        );
        assert!(!review.commits.iter().any(|commit| commit.sha == source));
        let other = hotsheet_model::ProjectTicketRef {
            project_id: "project-b".into(),
            ..ticket.clone()
        };
        let other_review = discover_for_ticket(
            root,
            "HS2-39D5YS",
            &CodeReviewClassification::default(),
            &other,
        )
        .unwrap();
        assert!(
            other_review
                .commits
                .iter()
                .any(|commit| commit.sha == source && commit.unmerged.is_some())
        );
        assert!(
            !other_review
                .commits
                .iter()
                .any(|commit| commit.sha == shipping)
        );
        let base = git(&["rev-parse", "main^"]);
        git(&["switch", "-c", "stale", &base]);
        let stale = discover_for_ticket(
            root,
            "HS2-39D5YS",
            &CodeReviewClassification::default(),
            &ticket,
        )
        .unwrap();
        assert!(stale.checkout_behind_main);
        assert!(
            stale
                .commits
                .iter()
                .any(|commit| commit.sha == shipping && commit.unmerged.is_none())
        );
    }

    fn commit(sha: &str, subject: &str, parent: &str) -> CodeReviewCommit {
        CodeReviewCommit {
            sha: sha.into(),
            short_sha: sha.chars().take(7).collect(),
            subject: subject.into(),
            body: String::new(),
            committed_at: "2026-09-02T00:00:00Z".into(),
            refs: Vec::new(),
            unmerged: None,
            parents: (!parent.is_empty())
                .then(|| parent.into())
                .into_iter()
                .collect(),
        }
    }

    fn on(mut commit: CodeReviewCommit, branch: Option<&str>) -> CodeReviewCommit {
        commit.unmerged = Some(UnmergedSource {
            branch: branch.map(str::to_owned),
            worktree: None,
        });
        commit
    }

    #[test]
    fn worktree_porcelain_maps_branch_and_detached_tips_to_directories() {
        let sha = "a".repeat(40);
        let porcelain = format!(
            "worktree /repo\nHEAD {b}\nbranch refs/heads/main\n\n\
             worktree /wt/feature\nHEAD {c}\nbranch refs/heads/feature\n\n\
             worktree /wt/detached\nHEAD {sha}\ndetached\n\n\
             worktree /wt/bare\nbare\n",
            b = "b".repeat(40),
            c = "c".repeat(40),
        );
        let tips = parse_worktrees(&porcelain);
        assert_eq!(tips.get("refs/heads/feature").unwrap(), "/wt/feature");
        assert_eq!(tips.get(&sha).unwrap(), "/wt/detached");
        assert_eq!(tips.len(), 3);

        let feature = unmerged_source(Some("feature"), &tips);
        assert_eq!(feature.branch.as_deref(), Some("feature"));
        assert_eq!(feature.worktree.as_deref(), Some("/wt/feature"));
        let full = unmerged_source(Some("refs/heads/feature"), &tips);
        assert_eq!(full, feature);
        let detached = unmerged_source(Some(&sha), &tips);
        assert_eq!(
            (detached.branch, detached.worktree.as_deref()),
            (None, Some("/wt/detached"))
        );
        let plain = unmerged_source(Some("topic"), &tips);
        assert_eq!(
            (plain.branch.as_deref(), plain.worktree),
            (Some("topic"), None)
        );
        assert_eq!(unmerged_source(None, &tips).branch, None);
    }

    #[test]
    fn unmerged_ranges_follow_parent_links_within_one_source_even_when_interleaved() {
        // Date-ordered log interleaving two branches: a3 → a2 → a1 on `a`, b2 → b1 on `b`,
        // and a gap on `a` (a1's parent a0 is not a ticket commit, a-1 skips it).
        let matched = vec![
            on(commit("a3", "x", "a2"), Some("a")),
            on(commit("b2", "x", "b1"), Some("b")),
            on(commit("a2", "x", "a1"), Some("a")),
            on(commit("b1", "x", "base"), Some("b")),
            on(commit("a1", "x", "a0"), Some("a")),
            on(commit("am", "x", "zz"), Some("a")),
        ];
        let ranges = unmerged_ranges(&matched);
        let spans = ranges
            .iter()
            .map(|range| (range.from.as_str(), range.to.as_str(), range.count))
            .collect::<Vec<_>>();
        assert_eq!(
            spans,
            vec![("a1", "a3", 3), ("am", "am", 1), ("b1", "b2", 2)]
        );
        assert!(unmerged_ranges(&[]).is_empty());
    }

    /// `(oldest, newest)` of each first-parent file run, in discovery order.
    fn run_spans(commits: &[CodeReviewCommit]) -> Vec<(String, String)> {
        first_parent_runs(&file_commits(commits))
            .into_iter()
            .map(|(oldest, newest)| (oldest.sha.clone(), newest.sha.clone()))
            .collect()
    }

    fn spans(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(oldest, newest)| ((*oldest).to_owned(), (*newest).to_owned()))
            .collect()
    }

    #[test]
    fn file_runs_prefer_merged_commits_then_the_newest_unmerged_source() {
        let mixed = vec![
            on(commit("u1", "x", "m2"), Some("feature")),
            commit("m2", "x", "m1"),
            commit("m1", "x", "m0"),
        ];
        assert_eq!(run_spans(&mixed), spans(&[("m1", "m2")]));

        let only_unmerged = vec![
            on(commit("f2", "x", "f1"), Some("feature")),
            on(commit("o1", "x", "base"), Some("other")),
            on(commit("f1", "x", "base"), Some("feature")),
        ];
        assert_eq!(run_spans(&only_unmerged), spans(&[("f1", "f2")]));
        let single = vec![on(commit("s1", "x", "base"), None)];
        assert_eq!(run_spans(&single), spans(&[("s1", "s1")]));
        assert!(run_spans(&[]).is_empty());
    }

    #[test]
    fn hs2_f10y9p_file_runs_split_at_gaps_and_forks_but_not_at_log_adjacency() {
        // A3 → A2 is one run; A1's child B1 belongs to another ticket, so A1 is its own run.
        let gap = vec![
            commit("a3", "x", "a2"),
            commit("a2", "x", "b1"),
            commit("a1", "x", "base"),
        ];
        assert_eq!(run_spans(&gap), spans(&[("a2", "a3"), ("a1", "a1")]));
        // Date order interleaves a side-branch commit between parent-linked commits: the
        // run still follows first parents.
        let interleaved = vec![
            commit("m3", "x", "m2"),
            commit("s1", "x", "base"),
            commit("m2", "x", "m1"),
            commit("m1", "x", "base"),
        ];
        assert_eq!(
            run_spans(&interleaved),
            spans(&[("s1", "s1"), ("m1", "m3")])
        );
        // Two ticket commits on top of one ticket commit: the fork ends the run there.
        let fork = vec![
            commit("l2", "x", "f1"),
            commit("r2", "x", "f1"),
            commit("f1", "x", "base"),
        ];
        assert_eq!(
            run_spans(&fork),
            spans(&[("l2", "l2"), ("r2", "r2"), ("f1", "f1")])
        );
        // Repeated commits are deduplicated before runs are built.
        let repeated = vec![
            commit("d2", "x", "d1"),
            commit("d2", "x", "d1"),
            commit("d1", "x", ""),
        ];
        assert_eq!(run_spans(&repeated), spans(&[("d1", "d2")]));
    }

    #[test]
    fn source_field_parses_only_from_source_scans() {
        let plain = format!(
            "{sha}\x1fabc\x1f\x1f2026-01-01T00:00:00Z\x1fs\x1f\x1f\x1e",
            sha = "1".repeat(40)
        );
        assert_eq!(parse_log(&plain)[0].unmerged, None);
        let sourced = format!(
            "{sha}\x1fabc\x1f\x1f2026-01-01T00:00:00Z\x1fs\x1f\x1ffeature\x1ffeature\n\x1e",
            sha = "1".repeat(40)
        );
        let parsed = parse_log(&sourced);
        assert_eq!(
            parsed[0].unmerged.as_ref().unwrap().branch.as_deref(),
            Some("feature")
        );
        assert_eq!(parsed[0].refs.len(), 1);
    }

    #[test]
    fn subject_matching_is_case_insensitive_and_bounded() {
        assert!(subject_mentions_ticket(
            "HS2-PG1HKJ: add review",
            "hs2-pg1hkj"
        ));
        assert!(subject_mentions_ticket("fix (HS2-PG1HKJ)", "HS2-PG1HKJ"));
        assert!(!subject_mentions_ticket(
            "HS2-PG1HKJX: different ticket",
            "HS2-PG1HKJ"
        ));
        assert!(!subject_mentions_ticket(
            "XHS2-PG1HKJ: different ticket",
            "HS2-PG1HKJ"
        ));
    }

    #[test]
    fn explicit_refs_lines_with_multiple_tickets_are_ownership_but_ordinary_body_mentions_are_not()
    {
        let mut referenced = commit("aaaa", "workflow documentation", "root");
        referenced.body = "Why this changed.\n\nRefs: HS2-PG1HKJ HS2-OTHER1".into();
        assert!(commit_mentions_ticket(&referenced, "HS2-PG1HKJ"));
        assert!(commit_mentions_ticket(&referenced, "HS2-OTHER1"));
        assert!(!commit_mentions_ticket(&referenced, "HS2-PG1HK"));
        referenced.body = "Why this changed.\n\nRefs HS2-PG1HKJ, HS2-OTHER1".into();
        assert!(commit_mentions_ticket(&referenced, "HS2-PG1HKJ"));
        assert!(commit_mentions_ticket(&referenced, "HS2-OTHER1"));
        referenced.body = "Follow-up for HS2-PG1HKJ".into();
        assert!(!commit_mentions_ticket(&referenced, "HS2-PG1HKJ"));
        referenced.body = "Refresh HS2-PG1HKJ".into();
        assert!(!commit_mentions_ticket(&referenced, "HS2-PG1HKJ"));
    }

    #[test]
    fn adjacent_matches_form_reviewable_ranges_without_crossing_unrelated_commits() {
        let all = [
            commit("eeeeeeee", "HS2-X: later polish", "dddddddd"),
            commit("dddddddd", "HS2-X: later implementation", "cccccccc"),
            commit("cccccccc", "unrelated", "bbbbbbbb"),
            commit("bbbbbbbb", "HS2-X: initial polish", "aaaaaaaa"),
            commit("aaaaaaaa", "HS2-X: initial implementation", "rootroot"),
        ];
        let matched = all
            .iter()
            .filter(|entry| subject_mentions_ticket(&entry.subject, "HS2-X"))
            .cloned()
            .collect::<Vec<_>>();
        // Outside a repository `order_runs` falls back to commit time, then SHA.
        let outside = tempfile::tempdir().unwrap();
        assert_eq!(
            merged_ranges(outside.path(), &matched),
            vec![
                CodeReviewRange {
                    from: "dddddddd".into(),
                    to: "eeeeeeee".into(),
                    count: 2
                },
                CodeReviewRange {
                    from: "aaaaaaaa".into(),
                    to: "bbbbbbbb".into(),
                    count: 2
                },
            ]
        );
    }

    #[test]
    fn arbitrary_commits_and_ranges_are_rejected_before_launch() {
        let review = CodeReview {
            file_runs: Vec::new(),
            checkout_behind_main: false,
            follow_ups: Vec::new(),
            summary: CodeReviewSummary::default(),
            files: Vec::new(),
            commits: vec![commit("bbbbbbbb", "HS2-X", "aaaaaaaa")],
            ranges: vec![],
            difftool: Some("configured".into()),
            truncated: false,
        };
        assert!(matches!(
            launch_revisions(
                Path::new("."),
                &review,
                &ReviewTarget::Commit {
                    commit: "--no-index".into()
                }
            ),
            Err(CodeReviewError::InvalidTarget)
        ));
        assert!(matches!(
            launch_revisions(
                Path::new("."),
                &review,
                &ReviewTarget::Range {
                    from: "aaaaaaaa".into(),
                    to: "bbbbbbbb".into()
                }
            ),
            Err(CodeReviewError::InvalidTarget)
        ));
        assert!(matches!(
            launch_revisions(
                Path::new("."),
                &review,
                &ReviewTarget::Compare {
                    from: "bbbbbbbb".into(),
                    to: "bbbbbbbb".into()
                }
            ),
            Err(CodeReviewError::InvalidTarget)
        ));
        assert!(matches!(
            launch_revisions(
                Path::new("."),
                &review,
                &ReviewTarget::Compare {
                    from: "bbbbbbbb".into(),
                    to: "--no-index".into()
                }
            ),
            Err(CodeReviewError::InvalidTarget)
        ));
    }

    #[test]
    fn parses_multiline_commit_bodies_without_splitting_commits() {
        let output = format!(
            "aaa{FIELD_SEPARATOR}aaa{FIELD_SEPARATOR}parent{FIELD_SEPARATOR}2026-09-02T00:00:00Z{FIELD_SEPARATOR}Subject{FIELD_SEPARATOR}First line\n\n**Markdown** line{RECORD_SEPARATOR}\nbbb{FIELD_SEPARATOR}bbb{FIELD_SEPARATOR}{FIELD_SEPARATOR}2026-09-01T00:00:00Z{FIELD_SEPARATOR}Root{FIELD_SEPARATOR}{RECORD_SEPARATOR}\n"
        );
        let commits = parse_log(&output);
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].body, "First line\n\n**Markdown** line");
        assert_eq!(commits[1].subject, "Root");
    }

    #[test]
    fn classifies_git_ref_decorations_by_kind() {
        let refs =
            parse_decorations("HEAD -> main, origin/main, origin/HEAD, tag: v1.2.0, release");
        assert_eq!(
            refs,
            vec![
                CommitRef {
                    label: "HEAD → main".into(),
                    kind: CommitRefKind::Head
                },
                CommitRef {
                    label: "origin/main".into(),
                    kind: CommitRefKind::Remote
                },
                CommitRef {
                    label: "origin/HEAD".into(),
                    kind: CommitRefKind::Remote
                },
                CommitRef {
                    label: "v1.2.0".into(),
                    kind: CommitRefKind::Tag
                },
                CommitRef {
                    label: "release".into(),
                    kind: CommitRefKind::Branch
                },
            ]
        );
        assert!(parse_decorations("").is_empty());
        assert_eq!(
            parse_decorations("HEAD"),
            vec![CommitRef {
                label: "HEAD".into(),
                kind: CommitRefKind::Head
            }]
        );
    }

    #[test]
    fn parses_the_decoration_field_into_commit_refs() {
        let output = format!(
            "aaa{FIELD_SEPARATOR}aaa{FIELD_SEPARATOR}parent{FIELD_SEPARATOR}2026-09-02T00:00:00Z{FIELD_SEPARATOR}Subject{FIELD_SEPARATOR}Body{FIELD_SEPARATOR}HEAD -> main, tag: v2{RECORD_SEPARATOR}\nbbb{FIELD_SEPARATOR}bbb{FIELD_SEPARATOR}{FIELD_SEPARATOR}2026-09-01T00:00:00Z{FIELD_SEPARATOR}Root{FIELD_SEPARATOR}{FIELD_SEPARATOR}{RECORD_SEPARATOR}\n"
        );
        let commits = parse_log(&output);
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].body, "Body");
        assert_eq!(
            commits[0].refs,
            vec![
                CommitRef {
                    label: "HEAD → main".into(),
                    kind: CommitRefKind::Head
                },
                CommitRef {
                    label: "v2".into(),
                    kind: CommitRefKind::Tag
                },
            ]
        );
        assert!(commits[1].refs.is_empty());
    }

    #[test]
    fn compare_uses_the_two_exact_discovered_commits() {
        let review = CodeReview {
            file_runs: Vec::new(),
            checkout_behind_main: false,
            follow_ups: Vec::new(),
            summary: CodeReviewSummary::default(),
            files: Vec::new(),
            commits: vec![
                commit("bbbbbbbb", "new", "aaaaaaaa"),
                commit("aaaaaaaa", "old", "rootroot"),
            ],
            ranges: vec![],
            difftool: Some("configured".into()),
            truncated: false,
        };
        assert_eq!(
            launch_revisions(
                Path::new("."),
                &review,
                &ReviewTarget::Compare {
                    from: "aaaaaaaa".into(),
                    to: "bbbbbbbb".into(),
                },
            )
            .unwrap(),
            ("aaaaaaaa".into(), "bbbbbbbb".into())
        );
    }

    #[test]
    fn repository_review_exposes_only_the_consecutive_unpushed_range() {
        let commits = vec![
            commit("cccccccc", "newest", "bbbbbbbb"),
            commit("bbbbbbbb", "middle", "aaaaaaaa"),
            commit("aaaaaaaa", "upstream", "rootroot"),
        ];
        assert_eq!(
            repository_range(&commits, 2),
            Some(CodeReviewRange {
                from: "bbbbbbbb".into(),
                to: "cccccccc".into(),
                count: 2,
            })
        );
    }

    #[test]
    fn configurable_file_patterns_cover_nested_paths() {
        assert!(glob_matches("docs/**", "docs/guide/setup.md"));
        assert!(glob_matches("**/*.test.*", "clients/web/src/api.test.ts"));
        assert!(glob_matches(
            "**/*_test.rs",
            "crates/core/src/store_test.rs"
        ));
        assert!(!glob_matches("src/**", "tests/src/example.rs"));
    }

    #[test]
    fn parses_changed_files_with_categories_and_rename_origins() {
        let files = parse_changed_files(
            "A\0tests/new.spec.ts\0R100\0docs/old.md\0docs/new.md\0M\0clients/web/src/main.tsx\0",
            &CodeReviewClassification::default(),
        );
        assert_eq!(files.len(), 3);
        assert_eq!(files[0].category, CodeReviewFileCategory::Tests);
        assert_eq!(files[0].change, CodeReviewFileChange::Added);
        assert_eq!(files[1].category, CodeReviewFileCategory::Docs);
        assert_eq!(files[1].change, CodeReviewFileChange::Renamed);
        assert_eq!(files[1].original_path.as_deref(), Some("docs/old.md"));
        assert_eq!(files[2].category, CodeReviewFileCategory::Source);
        let summary = summarize_changes(&files);
        assert_eq!(summary.files.total, 3);
        assert_eq!(summary.tests_added, 1);
    }

    #[test]
    fn ticket_file_targets_must_match_discovered_evidence() {
        let review = CodeReview {
            file_runs: vec![FileRun {
                base: "aaaaaaaa".into(),
                newest: "bbbbbbbb".into(),
                touched: BTreeMap::from([("src/known.rs".into(), vec!["src/known.rs".into()])]),
            }],
            checkout_behind_main: false,
            follow_ups: Vec::new(),
            summary: CodeReviewSummary::default(),
            files: vec![CodeReviewFile {
                path: "src/known.rs".into(),
                original_path: None,
                change: CodeReviewFileChange::Modified,
                category: CodeReviewFileCategory::Source,
            }],
            commits: vec![commit("bbbbbbbb", "new", "aaaaaaaa")],
            ranges: vec![],
            difftool: Some("configured".into()),
            truncated: false,
        };
        assert_eq!(
            ticket_file_launches(&review, &["src/known.rs".into()]).unwrap(),
            vec![("aaaaaaaa", "bbbbbbbb", vec!["src/known.rs".to_owned()])]
        );
        assert!(matches!(
            ticket_file_launches(&review, &["--no-index".into()]),
            Err(CodeReviewError::InvalidTarget)
        ));
        // Every path in a multi-file target must be ticket evidence.
        assert!(matches!(
            ticket_file_launches(&review, &["src/known.rs".into(), "--no-index".into()]),
            Err(CodeReviewError::InvalidTarget)
        ));
        assert!(matches!(
            ticket_file_launches(&review, &[]),
            Err(CodeReviewError::InvalidTarget)
        ));
        // Ticket files never resolve to one span through the commit/range path.
        assert!(matches!(
            launch_revisions(
                Path::new("."),
                &review,
                &ReviewTarget::TicketFile {
                    paths: vec!["src/known.rs".into()]
                },
            ),
            Err(CodeReviewError::InvalidTarget)
        ));
    }

    #[test]
    fn file_targets_accept_one_path_or_a_deduplicated_list() {
        let parse = |json: &str| serde_json::from_str::<ReviewTarget>(json);
        assert_eq!(
            parse(r#"{"mode":"ticket_file","path":"a.rs"}"#).unwrap(),
            ReviewTarget::TicketFile {
                paths: vec!["a.rs".into()]
            }
        );
        assert_eq!(
            parse(r#"{"mode":"worktree_file","paths":["a.rs","b.rs","a.rs"],"area":"staged"}"#)
                .unwrap(),
            ReviewTarget::WorktreeFile {
                paths: vec!["a.rs".into(), "b.rs".into()],
                area: WorktreeArea::Staged,
            }
        );
        assert!(parse(r#"{"mode":"ticket_file","paths":[]}"#).is_err());
        assert!(parse(r#"{"mode":"ticket_file"}"#).is_err());
    }

    #[test]
    fn several_files_share_one_plain_difftool_invocation() {
        // HS2-TRJ9P7: no `--dir-diff`; the pathspec alone selects the files.
        assert_eq!(
            difftool_args("ks", &[]),
            ["difftool", "--no-prompt", "--tool=ks"]
        );
        assert_eq!(
            difftool_args("ks", &["old", "new"]),
            ["difftool", "--no-prompt", "--tool=ks", "old", "new"]
        );
    }

    fn changed(code: char, path: &str, original: Option<&str>) -> CodeReviewFile {
        let change = match code {
            'A' => CodeReviewFileChange::Added,
            'C' => CodeReviewFileChange::Copied,
            'D' => CodeReviewFileChange::Deleted,
            'R' => CodeReviewFileChange::Renamed,
            'T' => CodeReviewFileChange::TypeChanged,
            _ => CodeReviewFileChange::Modified,
        };
        CodeReviewFile {
            path: path.into(),
            original_path: original.map(str::to_owned),
            change,
            category: classify(path, &CodeReviewClassification::default()),
        }
    }

    /// `path → (change letter, origin)` of composed files, for compact assertions.
    fn net(files: &[CodeReviewFile]) -> Vec<(String, char, Option<String>)> {
        files
            .iter()
            .map(|file| {
                let code = match file.change {
                    CodeReviewFileChange::Added => 'A',
                    CodeReviewFileChange::Copied => 'C',
                    CodeReviewFileChange::Deleted => 'D',
                    CodeReviewFileChange::Renamed => 'R',
                    CodeReviewFileChange::TypeChanged => 'T',
                    CodeReviewFileChange::Modified => 'M',
                };
                (file.path.clone(), code, file.original_path.clone())
            })
            .collect()
    }

    fn entry(path: &str, code: char, origin: Option<&str>) -> (String, char, Option<String>) {
        (path.into(), code, origin.map(str::to_owned))
    }

    #[test]
    fn hs2_f10y9p_run_changes_compose_by_path() {
        let classification = CodeReviewClassification::default();
        let (files, touched) = compose_run_changes(
            vec![
                vec![
                    changed('A', "added_kept.rs", None),
                    changed('A', "added_dropped.rs", None),
                    changed('M', "modified_deleted.rs", None),
                    changed('D', "deleted_readded.rs", None),
                    changed('R', "b.rs", Some("a.rs")),
                    changed('R', "gone_new.rs", Some("gone_old.rs")),
                    changed('A', "added_renamed.rs", None),
                    changed('M', "typed.rs", None),
                    changed('C', "copy.rs", Some("copy_source.rs")),
                    changed('D', "target.rs", None),
                    changed('R', "moved_away.rs", Some("returns.rs")),
                ],
                vec![
                    changed('M', "added_kept.rs", None),
                    changed('D', "added_dropped.rs", None),
                    changed('D', "modified_deleted.rs", None),
                    changed('A', "deleted_readded.rs", None),
                    changed('R', "c.rs", Some("b.rs")),
                    changed('D', "gone_new.rs", None),
                    changed('R', "added_final.rs", Some("added_renamed.rs")),
                    changed('T', "typed.rs", None),
                    changed('D', "copy.rs", None),
                    changed('R', "target.rs", Some("source.rs")),
                    changed('A', "returns.rs", None),
                ],
            ],
            &classification,
        );
        assert_eq!(
            net(&files),
            vec![
                entry("added_final.rs", 'A', None),
                entry("added_kept.rs", 'A', None),
                entry("c.rs", 'R', Some("a.rs")),
                entry("deleted_readded.rs", 'M', None),
                entry("gone_old.rs", 'D', None),
                entry("modified_deleted.rs", 'D', None),
                entry("moved_away.rs", 'C', Some("returns.rs")),
                entry("returns.rs", 'M', None),
                entry("source.rs", 'D', None),
                entry("target.rs", 'M', None),
                entry("typed.rs", 'T', None),
            ]
        );
        // A rename chain keeps the run-local names: the first run shows `a.rs → b.rs`, the
        // second `b.rs → c.rs`; a later-run-only path is not touched by the first run.
        assert_eq!(touched.len(), 2);
        assert_eq!(touched[0]["c.rs"], ["a.rs", "b.rs"]);
        assert_eq!(touched[1]["c.rs"], ["b.rs", "c.rs"]);
        assert_eq!(touched[0]["added_kept.rs"], ["added_kept.rs"]);
        assert_eq!(
            touched[1]["added_final.rs"],
            ["added_renamed.rs", "added_final.rs"]
        );
        assert!(!touched[0].contains_key("source.rs"));
        assert!(
            !touched
                .iter()
                .any(|run| run.contains_key("added_dropped.rs"))
        );

        // One run is returned exactly as parsed, including git's own order.
        let single = vec![changed('M', "z.rs", None), changed('A', "a.rs", None)];
        let (files, touched) = compose_run_changes(vec![single.clone()], &classification);
        assert_eq!(files, single);
        assert_eq!(touched[0]["z.rs"], ["z.rs"]);
        assert_eq!(
            compose_run_changes(Vec::new(), &classification),
            (vec![], vec![])
        );
    }

    /// A synthetic repository with a helper that runs git and returns trimmed stdout.
    fn synthetic_repository() -> (tempfile::TempDir, impl Fn(&[&str]) -> String) {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().to_path_buf();
        let git = move |args: &[&str]| {
            let output = hotsheet_ticketing::git::command_in(&root)
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8_lossy(&output.stdout).trim().to_owned()
        };
        git(&["init", "-b", "main"]);
        git(&["config", "user.name", "Synthetic Test"]);
        git(&["config", "user.email", "test@invalid.example"]);
        git(&["config", "diff.tool", "configured"]);
        (temporary, git)
    }

    /// Distinct multi-line content so git's rename detection pairs moved files.
    fn body(name: &str) -> String {
        (0..12)
            .map(|line| format!("{name} line {line}\n"))
            .collect()
    }

    fn write(root: &Path, path: &str, contents: &str) {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    /// Seed, then ticket A, ticket B, and ticket A again on the current branch. Returns the
    /// two A commits and the B commit.
    fn commit_a_b_a(root: &Path, git: &impl Fn(&[&str]) -> String) -> (String, String, String) {
        write(root, "shared.txt", "seed\n");
        write(root, "doomed.txt", &body("doomed"));
        write(root, "notes/old_name.md", &body("notes"));
        git(&["add", "-A"]);
        git(&["commit", "-m", "Seed"]);
        write(root, "shared.txt", "seed\na1\n");
        write(root, "src/a_only.rs", &body("a_only"));
        write(root, "temp.rs", &body("temp"));
        write(root, "doomed.txt", &format!("{}changed\n", body("doomed")));
        write(root, "notes/old_name.md", &format!("{}a1\n", body("notes")));
        git(&["add", "-A"]);
        git(&["commit", "-m", "HS2-AAAAAA First run"]);
        let first = git(&["rev-parse", "HEAD"]);
        write(root, "shared.txt", "seed\na1\nb\n");
        write(root, "src/b_only.rs", &body("b_only"));
        write(root, "tests/b_only.test.ts", &body("b_test"));
        git(&["add", "-A"]);
        git(&["commit", "-m", "HS2-BBBBBB Interleaved other ticket"]);
        let other = git(&["rev-parse", "HEAD"]);
        write(root, "shared.txt", "seed\na1\nb\na2\n");
        git(&["rm", "-q", "temp.rs", "doomed.txt"]);
        fs::rename(root.join("src/a_only.rs"), root.join("src/a_renamed.rs")).unwrap();
        fs::create_dir_all(root.join("docs")).unwrap();
        fs::rename(
            root.join("notes/old_name.md"),
            root.join("docs/new_name.md"),
        )
        .unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-m", "HS2-AAAAAA Second run"]);
        let second = git(&["rev-parse", "HEAD"]);
        (first, other, second)
    }

    /// Commit everything staged with an exact committer and author time, so the date-ordered
    /// log is deterministic.
    fn commit_at(root: &Path, message: &str, seconds: u32) -> String {
        let date = format!("2026-09-02T00:00:{seconds:02}Z");
        let run = |args: &[&str]| {
            let output = hotsheet_ticketing::git::command_in(root)
                .env("GIT_COMMITTER_DATE", &date)
                .env("GIT_AUTHOR_DATE", &date)
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8_lossy(&output.stdout).trim().to_owned()
        };
        run(&["commit", "-q", "--allow-empty", "-m", message]);
        run(&["rev-parse", "HEAD"])
    }

    #[test]
    fn hs2_0wpfzv_merged_ranges_follow_first_parents_not_date_adjacency() {
        let (temporary, git) = synthetic_repository();
        let root = temporary.path();
        write(root, "seed.txt", "seed\n");
        git(&["add", "-A"]);
        commit_at(root, "Seed", 0);
        // A side branch carries another ticket's commit under two ticket commits.
        git(&["checkout", "-q", "-b", "side"]);
        write(root, "b.txt", "b\n");
        git(&["add", "-A"]);
        let other = commit_at(root, "HS2-BBBBBB Other ticket", 1);
        write(root, "side.txt", "side\n");
        git(&["add", "-A"]);
        let side_first = commit_at(root, "HS2-AAAAAA Side work", 2);
        write(root, "side.txt", "side\nmore\n");
        git(&["add", "-A"]);
        let side_second = commit_at(root, "HS2-AAAAAA Side polish", 3);
        git(&["checkout", "-q", "main"]);
        write(root, "main.txt", "main\n");
        git(&["add", "-A"]);
        let main_only = commit_at(root, "HS2-AAAAAA Main work", 4);
        git(&["merge", "-q", "--no-ff", "--no-commit", "side"]);
        commit_at(root, "Merge side", 5);

        // The defect: by date, the main commit sits next to the side run, so log adjacency
        // joined them into one range whose `from^..to` diff spans HS2-BBBBBB.
        let dated = git(&["log", "--format=%H", "HEAD"]);
        let dated = dated.lines().collect::<Vec<_>>();
        assert_eq!(
            dated[1..5],
            [
                main_only.as_str(),
                side_second.as_str(),
                side_first.as_str(),
                other.as_str()
            ]
        );

        let review = discover(root, "HS2-AAAAAA").unwrap();
        // Newest run first: topology ranks the merge's first parent ahead of the side run.
        assert_eq!(
            review.ranges,
            vec![
                CodeReviewRange {
                    from: main_only.clone(),
                    to: main_only.clone(),
                    count: 1,
                },
                CodeReviewRange {
                    from: side_first.clone(),
                    to: side_second.clone(),
                    count: 2,
                },
            ]
        );
    }

    #[test]
    fn hs2_f10y9p_interleaved_ticket_files_exclude_the_other_tickets_paths() {
        let (temporary, git) = synthetic_repository();
        let root = temporary.path();
        let (first, other, second) = commit_a_b_a(root, &git);
        // The defect: one oldest-parent-to-newest span includes ticket B's files.
        let span = git(&["diff", "--name-only", &format!("{first}^"), &second]);
        assert!(span.lines().any(|path| path == "src/b_only.rs"));

        let review = discover(root, "HS2-AAAAAA").unwrap();
        assert_eq!(review.ranges.len(), 2);
        assert_eq!(
            net(&review.files),
            vec![
                entry("docs/new_name.md", 'R', Some("notes/old_name.md")),
                entry("doomed.txt", 'D', None),
                entry("shared.txt", 'M', None),
                entry("src/a_renamed.rs", 'A', None),
            ]
        );
        assert_eq!(review.summary.files.total, 4);
        assert_eq!(review.summary.files.tests, 0);
        assert_eq!(review.summary.files.source, 1);
        assert_eq!(review.summary.files.docs, 1);
        assert_eq!(review.summary.files.other, 2);

        // Each launch covers one run, oldest first, and never spans B's commit.
        let seed = git(&["rev-parse", &format!("{first}^")]);
        let shared = vec!["shared.txt".to_owned()];
        assert_eq!(
            ticket_file_launches(&review, &shared).unwrap(),
            vec![
                (seed.as_str(), first.as_str(), shared.clone()),
                (other.as_str(), second.as_str(), shared.clone()),
            ]
        );
        // A rename across runs launches each run under that run's names.
        let renamed = ticket_file_launches(&review, &["docs/new_name.md".into()]).unwrap();
        assert_eq!(
            renamed
                .iter()
                .map(|(_, newest, paths)| (*newest, paths.clone()))
                .collect::<Vec<_>>(),
            vec![
                (first.as_str(), vec!["notes/old_name.md".to_owned()]),
                (
                    second.as_str(),
                    vec![
                        "notes/old_name.md".to_owned(),
                        "docs/new_name.md".to_owned()
                    ]
                ),
            ]
        );
        // A modified-then-deleted path opens both runs; B's and dropped paths are not evidence.
        let deleted = ticket_file_launches(&review, &["doomed.txt".into()]).unwrap();
        assert_eq!(deleted.len(), 2);
        assert!(matches!(
            ticket_file_launches(&review, &["src/b_only.rs".into()]),
            Err(CodeReviewError::InvalidTarget)
        ));
        assert!(matches!(
            ticket_file_launches(&review, &["temp.rs".into()]),
            Err(CodeReviewError::InvalidTarget)
        ));
        // Commit and range review are unchanged: B's commit stays outside the ticket.
        assert!(!review.commits.iter().any(|commit| commit.sha == other));
        assert!(matches!(
            launch_revisions(
                root,
                &review,
                &ReviewTarget::Commit {
                    commit: other.clone()
                }
            ),
            Err(CodeReviewError::InvalidTarget)
        ));
        // Ticket B's own summary still sees only its own files.
        let b_review = discover(root, "HS2-BBBBBB").unwrap();
        assert_eq!(
            net(&b_review.files),
            vec![
                entry("shared.txt", 'M', None),
                entry("src/b_only.rs", 'A', None),
                entry("tests/b_only.test.ts", 'A', None),
            ]
        );
        assert_eq!(b_review.summary.tests_added, 1);
    }

    #[test]
    fn hs2_f10y9p_a_single_run_keeps_the_plain_span_diff_and_launch() {
        let (temporary, git) = synthetic_repository();
        let root = temporary.path();
        write(root, "seed.txt", "seed\n");
        git(&["add", "-A"]);
        git(&["commit", "-m", "Seed"]);
        let base = git(&["rev-parse", "HEAD"]);
        write(root, "src/one.rs", &body("one"));
        git(&["add", "-A"]);
        git(&["commit", "-m", "HS2-SINGLE First"]);
        write(root, "seed.txt", "seed\nchanged\n");
        write(root, "docs/two.md", &body("two"));
        git(&["add", "-A"]);
        git(&["commit", "-m", "HS2-SINGLE Second"]);
        let newest = git(&["rev-parse", "HEAD"]);
        let review = discover(root, "HS2-SINGLE").unwrap();
        let plain = git_output(
            root,
            &["diff", "--name-status", "-M", "-C", "-z", &base, &newest],
        )
        .unwrap();
        assert_eq!(
            review.files,
            parse_changed_files(&plain, &CodeReviewClassification::default())
        );
        let requested = vec!["src/one.rs".to_owned(), "seed.txt".to_owned()];
        assert_eq!(
            ticket_file_launches(&review, &requested).unwrap(),
            vec![(base.as_str(), newest.as_str(), requested.clone())]
        );
    }

    #[test]
    fn hs2_f10y9p_unmerged_only_runs_also_skip_interleaved_commits() {
        let (temporary, git) = synthetic_repository();
        let root = temporary.path();
        write(root, "base.txt", "base\n");
        git(&["add", "-A"]);
        git(&["commit", "-m", "Base"]);
        git(&["switch", "-q", "-c", "worker"]);
        let (first, other, second) = commit_a_b_a(root, &git);
        git(&["switch", "-q", "main"]);
        let review = discover(root, "HS2-AAAAAA").unwrap();
        assert!(
            review
                .commits
                .iter()
                .all(|commit| commit.unmerged.is_some())
        );
        let paths = review
            .files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            paths,
            [
                "docs/new_name.md",
                "doomed.txt",
                "shared.txt",
                "src/a_renamed.rs"
            ]
        );
        let seed = git(&["rev-parse", &format!("{first}^")]);
        let launches = ticket_file_launches(&review, &["shared.txt".into()]).unwrap();
        assert_eq!(
            launches
                .iter()
                .map(|(base, newest, _)| (*base, *newest))
                .collect::<Vec<_>>(),
            vec![
                (seed.as_str(), first.as_str()),
                (other.as_str(), second.as_str())
            ]
        );
    }
}
