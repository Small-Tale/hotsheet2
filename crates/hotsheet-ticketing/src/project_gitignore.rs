//! The one writer of a code project's `.gitignore` (HS2-61ZMVA).
//!
//! Hot Sheet keeps checkout-local files under `<project>/.hotsheet2/`. Two of them are
//! committed: the shared settings file and the project group registry (HS2-9KZSD4).
//! Everything else there (the local settings file, the standalone store link, the group
//! registry lock, and any later machine-local file) must stay out of Git.
//!
//! Rather than appending one line per file, every caller that writes into
//! `.hotsheet2/` calls [`ensure_project_block`]. It keeps one delimited, Hot Sheet-owned
//! block in the project `.gitignore`:
//!
//! ```text
//! # >>> Hot Sheet 2 (managed; edit outside this block) >>>
//! /.hotsheet2/*
//! !/.hotsheet2/settings.json
//! !/.hotsheet2/groups/
//! # <<< Hot Sheet 2 <<<
//! ```
//!
//! - Lines outside the block are never changed, except the exact single-file lines older
//!   releases appended ([`LEGACY_LINES`]), which the block makes redundant.
//! - A missing block is not added when the project's existing rules already give every
//!   Hot Sheet path the expected outcome, as Git itself reports it.
//! - A block that was edited by hand, duplicated, or left without its end marker is
//!   restored to the canonical rules.
//! - After writing, Git is asked how each Hot Sheet path resolves; a rule elsewhere that
//!   ignores a committed path, or re-includes a machine-local one, is returned as a
//!   [`GitignoreConflict`] for the caller to warn about. A bare `.hotsheet2/` rule, for
//!   example, ignores the directory so that no `!` rule can bring `settings.json` back.
//!
//! The ticket store's own `.gitignore` is a different file, maintained by the store.
//! AI-tool setup keeps wholly Hot Sheet-owned MCP configs out of Git through the
//! checkout-local `.git/info/exclude`, not this file (`docs/04` §4.4, §4.7.0).

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

/// The first line of the managed block.
pub const BLOCK_BEGIN: &str = "# >>> Hot Sheet 2 (managed; edit outside this block) >>>";
/// The last line of the managed block.
pub const BLOCK_END: &str = "# <<< Hot Sheet 2 <<<";
/// The block's rules, in order. The catch-all must come before the re-includes.
pub const BLOCK_RULES: &[&str] = &[
    "/.hotsheet2/*",
    "!/.hotsheet2/settings.json",
    "!/.hotsheet2/groups/",
];
/// Single-file lines that older releases appended. The block covers them, so they are
/// removed when the block is written.
pub const LEGACY_LINES: &[&str] = &[".hotsheet2/settings.local.json", ".hotsheet2/store"];

/// Paths that must be committed, probed through Git.
const COMMITTED_PROBES: &[&str] = &[
    ".hotsheet2/settings.json",
    ".hotsheet2/groups/0000000000.json",
];
/// Paths that must stay out of Git. The last one stands for any future machine-local file.
const LOCAL_PROBES: &[&str] = &[
    ".hotsheet2/settings.local.json",
    ".hotsheet2/store",
    ".hotsheet2/groups.lock",
    ".hotsheet2/machine-local-file",
];

/// What Hot Sheet expects Git to do with one of its paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expectation {
    /// The path is shared project data and must be committable.
    Committed,
    /// The path is machine-local and must be ignored.
    Ignored,
}

/// A Hot Sheet path whose Git ignore outcome differs from what Hot Sheet expects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitignoreConflict {
    /// The project-relative path that was probed.
    pub path: String,
    /// The outcome Hot Sheet expects.
    pub expected: Expectation,
    /// The deciding rule as Git reports it (`<source>:<line>:<pattern>`), when one matched.
    pub rule: Option<String>,
}

impl GitignoreConflict {
    /// The path a person recognizes: the registry directory or the other machine-local
    /// files rather than the synthetic probe names.
    pub fn subject(&self) -> &str {
        match self.path.as_str() {
            path if path.starts_with(".hotsheet2/groups/") => ".hotsheet2/groups/",
            ".hotsheet2/machine-local-file" => "machine-local files in .hotsheet2/",
            path => path,
        }
    }
}

impl fmt::Display for GitignoreConflict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let subject = self.subject();
        match (self.expected, &self.rule) {
            (Expectation::Committed, Some(rule)) => write!(
                f,
                "ignore rule `{rule}` hides {}, which Hot Sheet shares through Git; \
                 remove or narrow that rule",
                subject
            ),
            (Expectation::Committed, None) => {
                write!(
                    f,
                    "{} is ignored, but Hot Sheet shares it through Git",
                    subject
                )
            }
            (Expectation::Ignored, Some(rule)) => write!(
                f,
                "ignore rule `{rule}` re-includes {}, which is machine-local; \
                 remove that rule",
                subject
            ),
            (Expectation::Ignored, None) => write!(
                f,
                "{} is machine-local but not ignored; restore the Hot Sheet block in .gitignore",
                subject
            ),
        }
    }
}

/// The result of [`ensure_project_block`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitignoreReport {
    /// Whether `.gitignore` was written.
    pub changed: bool,
    /// Hot Sheet paths that Git still resolves differently from what Hot Sheet expects.
    /// Empty when the project is not a Git work tree.
    pub conflicts: Vec<GitignoreConflict>,
}

/// The project `.gitignore` path.
pub fn gitignore_path(project_root: &Path) -> PathBuf {
    project_root.join(".gitignore")
}

/// The `.gitignore` text [`ensure_project_block`] would write, or `None` when the file is
/// already correct. Reads the file and, when no block exists yet, asks Git whether the
/// existing rules already cover the Hot Sheet paths. Writes nothing.
pub fn plan_project_block(project_root: &Path) -> io::Result<Option<String>> {
    let current = read_or_empty(&gitignore_path(project_root))?;
    if !has_block(&current) && probes_satisfied(project_root) == Some(true) {
        return Ok(None);
    }
    let proposed = with_block(&current);
    Ok((proposed != current).then_some(proposed))
}

/// Ensure the project `.gitignore` carries the managed Hot Sheet block, then report any
/// rule that still gives a Hot Sheet path the wrong outcome. Idempotent.
pub fn ensure_project_block(project_root: &Path) -> io::Result<GitignoreReport> {
    let mut report = GitignoreReport::default();
    if let Some(text) = plan_project_block(project_root)? {
        // A plain write keeps a symlinked or specially permissioned `.gitignore` intact.
        std::fs::write(gitignore_path(project_root), text)?;
        report.changed = true;
    }
    report.conflicts = conflicts(project_root).unwrap_or_default();
    Ok(report)
}

/// Print each conflict in `report` as a warning on standard error. For callers whose
/// result type has no room for warnings.
pub fn warn_conflicts(report: &GitignoreReport) {
    for conflict in &report.conflicts {
        eprintln!("warning: {conflict}");
    }
}

/// [`ensure_project_block`] for callers that must not fail because `.gitignore` could not
/// be maintained: errors and conflicts become warnings on standard error.
pub fn ensure_project_block_or_warn(project_root: &Path) {
    match ensure_project_block(project_root) {
        Ok(report) => warn_conflicts(&report),
        Err(error) => eprintln!(
            "warning: could not update {}: {error}",
            gitignore_path(project_root).display()
        ),
    }
}

/// Append one exact `entry` line to `<dir>/.gitignore` when no line already equals it.
/// Only for paths outside a code project's `.hotsheet2/` (the legacy store-owned local
/// settings file); project paths use [`ensure_project_block`].
pub fn ensure_entry(dir: &Path, entry: &str) -> io::Result<bool> {
    let path = gitignore_path(dir);
    let existing = read_or_empty(&path)?;
    if existing.lines().any(|line| line.trim() == entry) {
        return Ok(false);
    }
    let eol = eol_of(&existing);
    let mut out = existing;
    if !out.is_empty() && !out.ends_with('\n') {
        out.push_str(eol);
    }
    out.push_str(entry);
    out.push_str(eol);
    std::fs::write(path, out)?;
    Ok(true)
}

fn read_or_empty(path: &Path) -> io::Result<String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error),
    }
}

fn eol_of(text: &str) -> &'static str {
    if text.contains("\r\n") { "\r\n" } else { "\n" }
}

fn has_block(text: &str) -> bool {
    text.lines().any(|line| line.trim() == BLOCK_BEGIN)
}

/// `text` with exactly one canonical block (in place of the first existing one, or
/// appended) and without the redundant legacy lines.
fn with_block(text: &str) -> String {
    let eol = eol_of(text);
    let mut kept: Vec<&str> = Vec::new();
    let mut block_at: Option<usize> = None;
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        let trimmed = line.trim();
        if trimmed == BLOCK_BEGIN {
            block_at.get_or_insert(kept.len());
            // Drop the old body. When the next marker is an end marker, the body runs to
            // it. Otherwise the block is damaged: only the `.hotsheet2` rules directly
            // after the begin marker belong to it, so later user lines survive.
            let closed = lines
                .clone()
                .map(str::trim)
                .find(|next| *next == BLOCK_BEGIN || *next == BLOCK_END)
                == Some(BLOCK_END);
            while let Some(next) = lines.peek().map(|next| next.trim()) {
                if closed {
                    lines.next();
                    if next == BLOCK_END {
                        break;
                    }
                } else if next != BLOCK_BEGIN && next.contains(".hotsheet2") {
                    lines.next();
                } else {
                    break;
                }
            }
            continue;
        }
        if trimmed == BLOCK_END || LEGACY_LINES.contains(&trimmed) {
            continue;
        }
        kept.push(line);
    }

    let block: Vec<&str> = std::iter::once(BLOCK_BEGIN)
        .chain(BLOCK_RULES.iter().copied())
        .chain(std::iter::once(BLOCK_END))
        .collect();
    let mut out: Vec<&str> = Vec::with_capacity(kept.len() + block.len() + 1);
    match block_at {
        Some(at) => {
            out.extend_from_slice(&kept[..at]);
            out.extend_from_slice(&block);
            out.extend_from_slice(&kept[at..]);
        }
        None => {
            while kept.last().is_some_and(|line| line.trim().is_empty()) {
                kept.pop();
            }
            out.extend_from_slice(&kept);
            if !out.is_empty() {
                out.push("");
            }
            out.extend_from_slice(&block);
        }
    }
    let mut joined = out.join(eol);
    joined.push_str(eol);
    joined
}

/// Whether every probe already resolves as expected, or `None` outside a Git work tree.
fn probes_satisfied(project_root: &Path) -> Option<bool> {
    conflicts(project_root).map(|found| found.is_empty())
}

/// Every probe whose outcome differs from its expectation, or `None` when Git cannot
/// answer (no Git, or not a work tree).
fn conflicts(project_root: &Path) -> Option<Vec<GitignoreConflict>> {
    let probes = COMMITTED_PROBES
        .iter()
        .map(|path| (*path, Expectation::Committed))
        .chain(
            LOCAL_PROBES
                .iter()
                .map(|path| (*path, Expectation::Ignored)),
        );
    let outcomes = check_ignore(project_root)?;
    Some(
        probes
            .filter_map(|(path, expected)| {
                let rule = outcomes.iter().find(|(probe, _)| probe == path)?.1.clone();
                let ignored = rule.as_deref().is_some_and(rule_ignores);
                let wrong = match expected {
                    Expectation::Committed => ignored,
                    Expectation::Ignored => !ignored,
                };
                wrong.then(|| GitignoreConflict {
                    path: path.to_owned(),
                    expected,
                    rule,
                })
            })
            .collect(),
    )
}

/// `git check-ignore --no-index -v -n` over every probe: each probe with the deciding
/// rule (`<source>:<line>:<pattern>`), or `None` when no rule matched.
fn check_ignore(project_root: &Path) -> Option<Vec<(String, Option<String>)>> {
    let output = crate::git::command()
        .arg("-C")
        .arg(project_root)
        .args([
            "check-ignore",
            "--no-index",
            "--verbose",
            "--non-matching",
            "--",
        ])
        .args(COMMITTED_PROBES)
        .args(LOCAL_PROBES)
        .output()
        .ok()?;
    // 0: some path ignored; 1: none ignored; anything else is a fatal error.
    if !matches!(output.status.code(), Some(0 | 1)) {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Some(
        stdout
            .lines()
            .filter_map(|line| {
                let (rule, path) = line.split_once('\t')?;
                let rule = (rule != "::").then(|| rule.to_owned());
                Some((path.to_owned(), rule))
            })
            .collect(),
    )
}

/// Whether a `<source>:<line>:<pattern>` match ignores its path (a `!` pattern
/// re-includes it).
fn rule_ignores(rule: &str) -> bool {
    pattern_of(rule).is_some_and(|pattern| !pattern.starts_with('!'))
}

/// The pattern of a `<source>:<line>:<pattern>` match. The source may itself contain
/// colons (a Windows drive), so the line number is the first all-digit field.
fn pattern_of(rule: &str) -> Option<&str> {
    rule.match_indices(':').find_map(|(colon, _)| {
        let (number, pattern) = rule[colon + 1..].split_once(':')?;
        (!number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())).then_some(pattern)
    })
}

#[cfg(test)]
#[path = "project_gitignore_tests.rs"]
mod tests;
