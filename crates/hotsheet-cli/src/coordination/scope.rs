//! Assignment path scopes and integrator-reviewed shared sections (HS2-YHYNJ6).
//!
//! A scope entry is a repository-relative `path`, or `path#row:<id>` / `path#heading:<slug>`
//! naming one keyed section of a document the integrator has published as shared. A whole
//! path overlaps everything under it, including every section of that file; two entries for
//! the same file are disjoint only when both name different sections. Code handoffs are then
//! checked against the commit's actual changes, so a declaration cannot hide an edit.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Result, ensure};

/// Section kinds a shared document is addressed by.
pub const SECTION_KINDS: &[&str] = &["row", "heading"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Entry<'a> {
    pub path: &'a str,
    pub section: Option<&'a str>,
}

pub(super) fn entry(scope: &str) -> Entry<'_> {
    match scope.split_once('#') {
        Some((path, section)) => Entry {
            path,
            section: Some(section),
        },
        None => Entry {
            path: scope,
            section: None,
        },
    }
}

fn valid_key(key: &str) -> bool {
    key.bytes()
        .next()
        .is_some_and(|b| b.is_ascii_alphanumeric())
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn normalized_path(raw: &str, path: &str) -> Result<String> {
    let path = path.replace('\\', "/");
    ensure!(
        !path.starts_with('/') && !path.contains(':'),
        "scope paths must be repository-relative: {raw}"
    );
    let components: Vec<&str> = path
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect();
    ensure!(
        !components.is_empty() && !components.contains(&".."),
        "scope path must name a file or directory inside the repository: {raw}"
    );
    Ok(components.join("/"))
}

/// One shared document path: a file, never a section.
pub(super) fn normalized_shared(shared: &[String]) -> Result<Vec<String>> {
    let mut normalized = Vec::with_capacity(shared.len());
    for raw in shared {
        ensure!(
            !raw.contains('#'),
            "a shared path names a whole document, not a section: {raw}"
        );
        normalized.push(normalized_path(raw, raw)?);
    }
    normalized.sort();
    normalized.dedup();
    Ok(normalized)
}

/// Repository-relative scope entries, normalized, sorted and deduplicated.
pub(super) fn normalized_scope(scope: &[String]) -> Result<Vec<String>> {
    let mut normalized = Vec::with_capacity(scope.len());
    for raw in scope {
        let Entry { path, section } = entry(raw);
        let path = normalized_path(raw, path)?;
        match section {
            None => normalized.push(path),
            Some(section) => {
                let valid = section
                    .split_once(':')
                    .is_some_and(|(kind, key)| SECTION_KINDS.contains(&kind) && valid_key(key));
                ensure!(
                    valid,
                    "a scope section must be row:<id> or heading:<slug> (lowercase, digits, hyphens): {raw}"
                );
                normalized.push(format!("{path}#{section}"));
            }
        }
    }
    normalized.sort();
    normalized.dedup();
    Ok(normalized)
}

fn contains(outer: &str, inner: &str) -> bool {
    inner == outer
        || inner
            .strip_prefix(outer)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// Whether two normalized entries could change the same content.
pub(super) fn scopes_overlap(left: &str, right: &str) -> bool {
    let (left, right) = (entry(left), entry(right));
    if left.path == right.path {
        return match (left.section, right.section) {
            (Some(a), Some(b)) => a == b,
            _ => true,
        };
    }
    contains(left.path, right.path) || contains(right.path, left.path)
}

/// Lowercase, hyphen-separated heading identity: `### Typed refusal (HS2-X)` is
/// `typed-refusal-hs2-x`.
pub(super) fn heading_slug(text: &str) -> String {
    let mut slug = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.trim_end_matches('-').to_owned()
}

/// A document split into keyed sections. Text before the first heading is `preamble`; a
/// key that occurs twice is ambiguous and can only be changed as a whole-file edit. The
/// outline keeps each heading's level in document order, because moving an unchanged
/// block changes the document without changing any block's content.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Sections {
    pub content: BTreeMap<String, String>,
    pub ambiguous: BTreeSet<String>,
    pub outline: Vec<(usize, String)>,
}

const PREAMBLE: &str = "preamble";

/// Split Markdown into `row:<id>` table rows whose first cell is a lowercase id and
/// `heading:<slug>` blocks (the heading plus its non-row lines). Fenced code is content.
pub(super) fn sections(text: &str) -> Sections {
    let mut sections = Sections::default();
    let add = |sections: &mut Sections, key: String, line: &str| {
        let content = sections.content.entry(key).or_default();
        content.push_str(line);
        content.push('\n');
    };
    let mut current = PREAMBLE.to_owned();
    let mut fenced = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
        } else if !fenced {
            if let Some(heading) = line
                .strip_prefix('#')
                .map(|rest| rest.trim_start_matches('#'))
                .filter(|rest| rest.starts_with(' '))
                .filter(|_| line.chars().take_while(|c| *c == '#').count() <= 6)
            {
                current = format!("heading:{}", heading_slug(heading));
                if sections.content.contains_key(&current) {
                    sections.ambiguous.insert(current.clone());
                }
                let level = line.len() - line.trim_start_matches('#').len();
                sections.outline.push((level, current.clone()));
                add(&mut sections, current.clone(), line);
                continue;
            }
            if let Some(id) = row_id(line) {
                let key = format!("row:{id}");
                if sections.content.contains_key(&key) {
                    sections.ambiguous.insert(key.clone());
                }
                add(&mut sections, key, line);
                continue;
            }
        }
        add(&mut sections, current.clone(), line);
    }
    sections
}

fn row_id(line: &str) -> Option<&str> {
    let rest = line.strip_prefix('|')?;
    let (cell, _) = rest.split_once('|')?;
    let id = cell.trim();
    valid_key(id).then_some(id)
}

fn headings(sections: &Sections) -> BTreeSet<&String> {
    sections.outline.iter().map(|(_, key)| key).collect()
}

/// Each heading present in both outlines, in order, with its level and parent (the nearest
/// preceding heading of a lower level, which may be a heading only one side has).
fn placements<'a>(
    outline: &'a [(usize, String)],
    common: &BTreeSet<&String>,
) -> Vec<(&'a str, usize, Option<&'a str>)> {
    let mut parents: Vec<(usize, &str)> = Vec::new();
    let mut placed = Vec::new();
    for (level, key) in outline {
        while parents.last().is_some_and(|(parent, _)| parent >= level) {
            parents.pop();
        }
        if common.contains(key) {
            placed.push((
                key.as_str(),
                *level,
                parents.last().map(|(_, parent)| *parent),
            ));
        }
        parents.push((*level, key));
    }
    placed
}

/// Drop only whole trailing blank lines (empty, or spaces and tabs only, as CommonMark
/// defines them). Every byte of the last nonblank line stays content, including trailing
/// spaces, tabs and non-breaking spaces.
fn without_trailing_blank_lines(content: &str) -> &str {
    let mut end = content.len();
    for line in content.split_inclusive('\n').rev() {
        if line
            .bytes()
            .all(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r'))
        {
            end -= line.len();
        } else {
            break;
        }
    }
    &content[..end]
}

/// Section keys whose content differs, or `None` when the edit is not confined to
/// unambiguous keyed sections: preamble text, a duplicated key, or a heading kept on both
/// sides whose order, level or parent changed. New headings that reparent nothing stay
/// confined; table rows are keyed without order; trailing blank lines are not content.
pub(super) fn touched_sections(before: &str, after: &str) -> Option<BTreeSet<String>> {
    let (before, after) = (sections(before), sections(after));
    let (left, right) = (headings(&before), headings(&after));
    let common: BTreeSet<&String> = left.intersection(&right).copied().collect();
    if placements(&before.outline, &common) != placements(&after.outline, &common) {
        return None;
    }
    let keys: BTreeSet<&String> = before.content.keys().chain(after.content.keys()).collect();
    let mut touched = BTreeSet::new();
    for key in keys {
        // Trailing blank lines only separate blocks, so appending a section after the last
        // one does not touch it.
        let text = |s: &Sections| {
            s.content
                .get(key)
                .map(|c| without_trailing_blank_lines(c).to_owned())
        };
        if text(&before) != text(&after) {
            if key == PREAMBLE || before.ambiguous.contains(key) || after.ambiguous.contains(key) {
                return None;
            }
            touched.insert(key.clone());
        }
    }
    Some(touched)
}

/// Whether `scope` names sections of exactly `path`, so its contents must be compared.
pub fn sectioned(scope: &[String], path: &str) -> bool {
    scope
        .iter()
        .map(|raw| entry(raw))
        .any(|e| e.path == path && e.section.is_some())
}

/// One path changed by a code handoff, with its content at the assignment base and at the
/// handoff commit (`None` where absent).
pub struct ChangedPath {
    pub path: String,
    pub before: Option<String>,
    pub after: Option<String>,
}

/// Every changed path must be covered by a whole-path entry, or be a document whose changed
/// sections are all declared. Anything else needs a reviewed scope change.
pub fn review_handoff(scope: &[String], changed: &[ChangedPath]) -> Result<()> {
    let undeclared = |path: &str, detail: String| {
        anyhow::anyhow!(
            "handoff changes {path} outside the declared scope ({detail}); block with a note naming the needed scope so the integrator can review it and the dispatcher can reserve a new operation"
        )
    };
    for change in changed {
        let path = change.path.as_str();
        if scope
            .iter()
            .map(|raw| entry(raw))
            .any(|e| e.section.is_none() && contains(e.path, path))
        {
            continue;
        }
        let declared: BTreeSet<String> = scope
            .iter()
            .map(|raw| entry(raw))
            .filter(|e| e.path == path)
            .filter_map(|e| e.section.map(str::to_owned))
            .collect();
        if declared.is_empty() {
            return Err(undeclared(path, "path not declared".into()));
        }
        let (Some(before), Some(after)) = (&change.before, &change.after) else {
            return Err(undeclared(
                path,
                "a sectioned document cannot be added or removed".into(),
            ));
        };
        let touched = touched_sections(before, after).ok_or_else(|| {
            undeclared(
                path,
                "edit outside keyed sections or to a duplicated key".into(),
            )
        })?;
        let extra: Vec<&String> = touched.difference(&declared).collect();
        ensure!(
            extra.is_empty(),
            undeclared(
                path,
                format!(
                    "undeclared sections {}",
                    extra
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            )
        );
    }
    Ok(())
}
