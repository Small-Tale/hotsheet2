//! Ticket-associated commit discovery and safe configured-difftool launching.
//!
//! A commit belongs to a ticket only when the ticket slug appears as a bounded token in
//! its subject. Bodies are deliberately ignored: they commonly cross-reference tickets
//! whose code the commit did not implement. Launch requests are checked against a fresh
//! discovery result, then passed to `git difftool` as an argument array (never a shell).

use std::path::Path;
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};
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
    #[serde(skip_serializing)]
    parents: Vec<String>,
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
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CodeReviewPage {
    pub items: Vec<CodeReviewCommit>,
    pub next_cursor: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum ReviewTarget {
    Commit { commit: String },
    Range { from: String, to: String },
    Compare { from: String, to: String },
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
    let (all, difftool, truncated) = discover_commits(root)?;
    let commits = all
        .iter()
        .filter(|commit| subject_mentions_ticket(&commit.subject, ticket_slug))
        .cloned()
        .collect::<Vec<_>>();
    let ranges = contiguous_ranges(&all, &commits);
    Ok(CodeReview {
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
        commits,
        ranges,
        difftool,
        truncated,
    })
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
        &["log", "-n", &LOG_LIMIT.to_string(), &log_format(), "HEAD"],
    ) {
        Ok(output) => output,
        Err(_) if !git_success(root, &["rev-parse", "--verify", "HEAD"]) => String::new(),
        Err(error) => return Err(error),
    };
    let all = parse_log(&output);
    let truncated = all.len() == LOG_LIMIT;
    Ok((all, difftool, truncated))
}

fn log_format() -> String {
    format!(
        "--format=%H{FIELD_SEPARATOR}%h{FIELD_SEPARATOR}%P{FIELD_SEPARATOR}%cI{FIELD_SEPARATOR}%s{FIELD_SEPARATOR}%b{RECORD_SEPARATOR}"
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
    let (old, new) = launch_revisions(root, review, target)?;
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["difftool", "--no-prompt", &old, &new])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| CodeReviewError::Launch(error.to_string()))
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
    };
    spawn_difftool(root, revisions)
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
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["difftool", "--no-prompt", &old, &new])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| CodeReviewError::Launch(error.to_string()))
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
    }
}

fn git_output(root: &Path, args: &[&str]) -> Result<String, CodeReviewError> {
    let output = Command::new("git")
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
    Command::new("git")
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
            let mut fields = record.splitn(6, FIELD_SEPARATOR);
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

fn is_ticket_char(value: char) -> bool {
    value.is_ascii_alphanumeric() || value == '_' || value == '-'
}

fn contiguous_ranges(
    all: &[CodeReviewCommit],
    matched: &[CodeReviewCommit],
) -> Vec<CodeReviewRange> {
    let matched = matched
        .iter()
        .map(|commit| commit.sha.as_str())
        .collect::<std::collections::HashSet<_>>();
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
    for commit in all {
        if matched.contains(commit.sha.as_str()) {
            run.push(commit);
        } else if !run.is_empty() {
            flush(&mut run, &mut ranges);
        }
    }
    if !run.is_empty() {
        flush(&mut run, &mut ranges);
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(sha: &str, subject: &str, parent: &str) -> CodeReviewCommit {
        CodeReviewCommit {
            sha: sha.into(),
            short_sha: sha.chars().take(7).collect(),
            subject: subject.into(),
            body: String::new(),
            committed_at: "2026-09-02T00:00:00Z".into(),
            parents: (!parent.is_empty())
                .then(|| parent.into())
                .into_iter()
                .collect(),
        }
    }

    #[test]
    fn matching_is_case_insensitive_bounded_and_subject_only() {
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
    fn adjacent_matches_form_reviewable_ranges_without_crossing_unrelated_commits() {
        let all = vec![
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
        assert_eq!(
            contiguous_ranges(&all, &matched),
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
    fn compare_uses_the_two_exact_discovered_commits() {
        let review = CodeReview {
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
}
