//! Git repository status snapshot used by headless/server consumers.

use serde::{Deserialize, Serialize};
use std::path::Path;
use thiserror::Error;

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
            "status",
            "--porcelain=v2",
            "--branch",
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
    for line in text.lines() {
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
        } else if line.starts_with("u ") {
            out.conflicted += 1;
        } else if line.starts_with("1 ") || line.starts_with("2 ") {
            let xy = line.split_whitespace().nth(1).unwrap_or("..").as_bytes();
            if xy.first().is_some_and(|v| *v != b'.') {
                out.staged += 1;
            }
            if xy.get(1).is_some_and(|v| *v != b'.') {
                out.unstaged += 1;
            }
        }
    }
    out.clean = out.staged + out.unstaged + out.untracked + out.conflicted == 0;
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_branch_divergence_and_worktree_counts() {
        let input = "# branch.oid abc123\n# branch.head main\n# branch.upstream origin/main\n# branch.ab +2 -3\n1 M. N... 100644 100644 100644 a b staged\n1 .M N... 100644 100644 100644 a b dirty\nu UU N... 100644 100644 100644 100644 a b c conflict\n? new.txt\n";
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
    }
}
