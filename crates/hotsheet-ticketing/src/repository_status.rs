//! Git repository status snapshot used by headless/server consumers.

use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RepositoryFileChange {
    Added,
    Copied,
    Deleted,
    Modified,
    Renamed,
    TypeChanged,
    Unmerged,
    Untracked,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepositoryFile {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub staged: Option<RepositoryFileChange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unstaged: Option<RepositoryFileChange>,
    pub untracked: bool,
    pub conflicted: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepositoryStatus {
    pub branch: Option<String>,
    pub head: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub staged: u32,
    pub unstaged: u32,
    pub untracked: u32,
    pub conflicted: u32,
    pub clean: bool,
    #[serde(default)]
    pub files: Vec<RepositoryFile>,
}

#[derive(Debug, Error)]
pub enum RepositoryStatusError {
    #[error("git status failed: {0}")]
    Git(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub fn snapshot(root: &Path) -> Result<RepositoryStatus, RepositoryStatusError> {
    let output = std::process::Command::new("git")
        .args([
            "-C",
            root.to_str().unwrap_or("."),
            "-c",
            "core.quotepath=false",
            "status",
            "--porcelain=v2",
            "--branch",
            "-z",
        ])
        .output()?;
    if !output.status.success() {
        return Err(RepositoryStatusError::Git(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    Ok(parse_porcelain_v2(&String::from_utf8_lossy(&output.stdout)))
}

pub fn parse_porcelain_v2(text: &str) -> RepositoryStatus {
    let mut out = RepositoryStatus::default();
    let mut records = if text.contains('\0') {
        text.split('\0').peekable()
    } else {
        text.split('\n').peekable()
    };
    while let Some(line) = records.next() {
        if let Some(v) = line.strip_prefix("# branch.head ") {
            out.branch = (v != "(detached)").then(|| v.to_owned());
        } else if let Some(v) = line.strip_prefix("# branch.oid ") {
            out.head = (v != "(initial)").then(|| v.to_owned());
        } else if let Some(v) = line.strip_prefix("# branch.upstream ") {
            out.upstream = Some(v.to_owned());
        } else if let Some(v) = line.strip_prefix("# branch.ab ") {
            for part in v.split_whitespace() {
                if let Some(n) = part.strip_prefix('+') {
                    out.ahead = n.parse().unwrap_or(0);
                }
                if let Some(n) = part.strip_prefix('-') {
                    out.behind = n.parse().unwrap_or(0);
                }
            }
        } else if line.starts_with("? ") {
            out.untracked += 1;
            out.files.push(RepositoryFile {
                path: line[2..].to_owned(),
                original_path: None,
                staged: None,
                unstaged: Some(RepositoryFileChange::Untracked),
                untracked: true,
                conflicted: false,
            });
        } else if line.starts_with("u ") {
            out.conflicted += 1;
            if let Some(path) = record_path(line, 11) {
                out.files.push(RepositoryFile {
                    path: path.to_owned(),
                    original_path: None,
                    staged: Some(RepositoryFileChange::Unmerged),
                    unstaged: Some(RepositoryFileChange::Unmerged),
                    untracked: false,
                    conflicted: true,
                });
            }
        } else if line.starts_with("1 ") || line.starts_with("2 ") {
            let renamed = line.starts_with("2 ");
            let field_count = if renamed { 10 } else { 9 };
            let mut fields = line.splitn(field_count, ' ');
            let _record_kind = fields.next();
            let xy = fields.next().unwrap_or("..").as_bytes();
            let path = fields.last().unwrap_or_default();
            let staged = xy.first().and_then(|value| change_kind(*value));
            let unstaged = xy.get(1).and_then(|value| change_kind(*value));
            if staged.is_some() {
                out.staged += 1;
            }
            if unstaged.is_some() {
                out.unstaged += 1;
            }
            let original_path = renamed
                .then(|| {
                    if text.contains('\0') {
                        records.next().unwrap_or_default().to_owned()
                    } else {
                        path.split_once('\t')
                            .map(|(_, original)| original.to_owned())
                            .unwrap_or_default()
                    }
                })
                .filter(|value| !value.is_empty());
            let path = if !text.contains('\0') && renamed {
                path.split_once('\t').map_or(path, |(current, _)| current)
            } else {
                path
            };
            out.files.push(RepositoryFile {
                path: path.to_owned(),
                original_path,
                staged,
                unstaged,
                untracked: false,
                conflicted: false,
            });
        }
    }
    out.clean = out.staged + out.unstaged + out.untracked + out.conflicted == 0;
    out
}

fn record_path(record: &str, fields: usize) -> Option<&str> {
    record
        .splitn(fields, ' ')
        .last()
        .filter(|path| !path.is_empty())
}

fn change_kind(value: u8) -> Option<RepositoryFileChange> {
    match value {
        b'.' | b' ' => None,
        b'A' => Some(RepositoryFileChange::Added),
        b'C' => Some(RepositoryFileChange::Copied),
        b'D' => Some(RepositoryFileChange::Deleted),
        b'M' => Some(RepositoryFileChange::Modified),
        b'R' => Some(RepositoryFileChange::Renamed),
        b'T' => Some(RepositoryFileChange::TypeChanged),
        b'U' => Some(RepositoryFileChange::Unmerged),
        _ => Some(RepositoryFileChange::Modified),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_branch_divergence_and_worktree_counts() {
        let input = "# branch.oid abc123\0# branch.head main\0# branch.upstream origin/main\0# branch.ab +2 -3\01 M. N... 100644 100644 100644 a b staged file.txt\01 .M N... 100644 100644 100644 a b dirty file.txt\0u UU N... 100644 100644 100644 100644 a b c conflict.txt\0? new file.txt\0";
        let status = parse_porcelain_v2(input);
        assert_eq!((status.ahead, status.behind), (2, 3));
        assert_eq!(
            (
                status.staged,
                status.unstaged,
                status.untracked,
                status.conflicted
            ),
            (1, 1, 1, 1)
        );
        assert!(!status.clean);
        assert_eq!(status.files.len(), 4);
        assert_eq!(status.files[0].path, "staged file.txt");
        assert_eq!(status.files[0].staged, Some(RepositoryFileChange::Modified));
        assert_eq!(
            status.files[1].unstaged,
            Some(RepositoryFileChange::Modified)
        );
        assert!(status.files[2].conflicted);
        assert!(status.files[3].untracked);
        assert_eq!(status.files[3].path, "new file.txt");
    }

    #[test]
    fn parses_rename_records_with_the_original_path() {
        let status = parse_porcelain_v2(
            "# branch.head main\02 R. N... 100644 100644 100644 a b R100 new name.txt\0old name.txt\0",
        );
        assert_eq!(status.staged, 1);
        assert_eq!(status.files[0].path, "new name.txt");
        assert_eq!(
            status.files[0].original_path.as_deref(),
            Some("old name.txt")
        );
        assert_eq!(status.files[0].staged, Some(RepositoryFileChange::Renamed));
    }
}
