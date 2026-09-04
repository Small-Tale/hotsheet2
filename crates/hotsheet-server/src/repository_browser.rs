//! Repository-status detail discovery and safe host file actions.

use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

use hotsheet_ticketing::repository_status::{self, RepositoryStatus};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::code_review::{self, CodeReview, CodeReviewCommit, CodeReviewRange};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HostPlatform {
    Macos,
    Windows,
    Linux,
}

impl HostPlatform {
    pub fn current() -> Self {
        match std::env::consts::OS {
            "macos" => Self::Macos,
            "windows" => Self::Windows,
            _ => Self::Linux,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RepositoryOverview {
    #[serde(flatten)]
    pub status: RepositoryStatus,
    pub root: String,
    pub platform: HostPlatform,
    pub commit_count: u64,
    pub commits: Vec<CodeReviewCommit>,
    pub ranges: Vec<CodeReviewRange>,
    pub difftool: Option<String>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RepositoryFileAction {
    Open,
    Reveal,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct RepositoryFileActionRequest {
    pub path: String,
    pub action: RepositoryFileAction,
}

#[derive(Debug, Error)]
pub enum RepositoryBrowserError {
    #[error(transparent)]
    Status(#[from] repository_status::RepositoryStatusError),
    #[error(transparent)]
    Review(#[from] code_review::CodeReviewError),
    #[error("that file is not part of the current repository status")]
    UnknownFile,
    #[error("the repository path is not safe to open")]
    UnsafePath,
    #[error("the selected file no longer exists")]
    MissingFile,
    #[error("could not launch the host application: {0}")]
    Launch(String),
}

pub fn discover(root: &Path) -> Result<RepositoryOverview, RepositoryBrowserError> {
    let status = repository_status::snapshot(root)?;
    let CodeReview {
        commits,
        ranges,
        difftool,
        truncated,
    } = code_review::discover_repository(root, status.ahead as usize)?;
    Ok(RepositoryOverview {
        root: root.display().to_string(),
        platform: HostPlatform::current(),
        commit_count: commit_count(root),
        status,
        commits,
        ranges,
        difftool,
        truncated,
    })
}

pub fn act_on_file(
    root: &Path,
    request: &RepositoryFileActionRequest,
) -> Result<(), RepositoryBrowserError> {
    let candidate = validated_status_path(root, &request.path)?;
    let target = match request.action {
        RepositoryFileAction::Open => {
            if !candidate.exists() {
                return Err(RepositoryBrowserError::MissingFile);
            }
            candidate
        }
        RepositoryFileAction::Reveal if cfg!(target_os = "macos") && candidate.exists() => {
            return spawn("open", &["-R".into(), candidate.into_os_string()]);
        }
        RepositoryFileAction::Reveal if cfg!(target_os = "windows") => {
            let selected = format!("/select,{}", candidate.display());
            return spawn("explorer", &[selected.into()]);
        }
        RepositoryFileAction::Reveal => candidate
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| root.to_path_buf()),
    };

    if cfg!(target_os = "macos") {
        spawn("open", &[target.into_os_string()])
    } else if cfg!(target_os = "windows") {
        spawn(
            "rundll32",
            &[
                "url.dll,FileProtocolHandler".into(),
                target.into_os_string(),
            ],
        )
    } else {
        spawn("xdg-open", &[target.into_os_string()])
    }
}

fn validated_status_path(root: &Path, relative: &str) -> Result<PathBuf, RepositoryBrowserError> {
    let relative_path = Path::new(relative);
    if relative.is_empty()
        || relative_path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(RepositoryBrowserError::UnsafePath);
    }
    let status = repository_status::snapshot(root)?;
    if !status.files.iter().any(|file| file.path == relative) {
        return Err(RepositoryBrowserError::UnknownFile);
    }
    let canonical_root = root
        .canonicalize()
        .map_err(repository_status::RepositoryStatusError::from)?;
    let candidate = canonical_root.join(relative_path);
    let containment_target = nearest_existing_path(&candidate)
        .canonicalize()
        .map_err(repository_status::RepositoryStatusError::from)?;
    if !containment_target.starts_with(&canonical_root) {
        return Err(RepositoryBrowserError::UnsafePath);
    }
    Ok(candidate)
}

fn nearest_existing_path(path: &Path) -> &Path {
    let mut current = path;
    while !current.exists() {
        current = current.parent().unwrap_or(current);
        if current.parent().is_none() {
            break;
        }
    }
    current
}

fn commit_count(root: &Path) -> u64 {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-list", "--count", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(0)
}

fn spawn(program: &str, args: &[std::ffi::OsString]) -> Result<(), RepositoryBrowserError> {
    Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|error| RepositoryBrowserError::Launch(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_paths_that_can_escape_the_checkout() {
        let root = tempfile::tempdir().unwrap();
        assert!(matches!(
            validated_status_path(root.path(), "../outside"),
            Err(RepositoryBrowserError::UnsafePath)
        ));
        assert!(matches!(
            validated_status_path(root.path(), "/absolute"),
            Err(RepositoryBrowserError::UnsafePath)
        ));
    }
}
