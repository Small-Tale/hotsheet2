//! Repository-status detail discovery and safe host file actions.

use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

use hotsheet_ticketing::repository_status::{self, RepositoryFile, RepositoryStatus};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::code_review::{self, CodeReviewCommit, CodeReviewRange};

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
    pub initialized: bool,
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

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RepositoryPage<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<usize>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RepositoryFileView {
    Staged,
    Unstaged,
    Untracked,
    Conflicted,
}

#[derive(Debug, Clone, Copy, Deserialize, Default)]
pub struct RepositoryPageQuery {
    #[serde(default)]
    pub cursor: usize,
    #[serde(default = "default_page_limit")]
    pub limit: usize,
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct RepositoryFilePageQuery {
    pub view: RepositoryFileView,
    #[serde(default)]
    pub cursor: usize,
    #[serde(default = "default_page_limit")]
    pub limit: usize,
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

#[derive(Debug, Error)]
pub enum RepositorySetupError {
    #[error("the checkout is not a Git repository")]
    NotRepository,
    #[error("enter a valid Git remote URL")]
    InvalidRemote,
    #[error("origin is already configured as {0}; refusing to replace existing user configuration")]
    OriginAlreadyConfigured(String),
    #[error("could not run Git: {0}")]
    Io(#[from] std::io::Error),
    #[error("git {operation} failed: {detail}")]
    Git {
        operation: &'static str,
        detail: String,
    },
    #[error(transparent)]
    Discovery(#[from] RepositoryBrowserError),
}

pub fn discover(root: &Path) -> Result<RepositoryOverview, RepositoryBrowserError> {
    let mut status = match repository_status::snapshot(root) {
        Ok(status) => status,
        Err(repository_status::RepositoryStatusError::NotRepository) => {
            return Ok(RepositoryOverview {
                initialized: false,
                status: RepositoryStatus::default(),
                root: root.display().to_string(),
                platform: HostPlatform::current(),
                commit_count: 0,
                commits: Vec::new(),
                ranges: Vec::new(),
                difftool: None,
                truncated: false,
            });
        }
        Err(error) => return Err(error.into()),
    };
    let (ranges, difftool) =
        code_review::discover_repository_metadata(root, status.ahead as usize)?;
    status.files.clear();
    Ok(RepositoryOverview {
        initialized: true,
        root: root.display().to_string(),
        platform: HostPlatform::current(),
        commit_count: commit_count(root),
        status,
        commits: Vec::new(),
        ranges,
        difftool,
        truncated: false,
    })
}

/// Initialize only the supplied checkout root and return the same typed overview used by
/// repository-status reads. The operation is idempotent and never stages or commits files.
pub fn initialize(root: &Path) -> Result<RepositoryOverview, RepositorySetupError> {
    let current = discover(root)?;
    if current.initialized {
        return Ok(current);
    }
    checked_git(root, &["init", "--quiet"], "init")?;
    let initialized = discover(root)?;
    if !initialized.initialized {
        return Err(RepositorySetupError::NotRepository);
    }
    Ok(initialized)
}

/// Add a provider-neutral `origin` without replacing user configuration or publishing
/// project contents. Publishing needs an explicit user-owned commit and remains outside
/// this recovery operation.
pub fn configure_origin(
    root: &Path,
    remote: &str,
) -> Result<RepositoryOverview, RepositorySetupError> {
    let remote = remote.trim();
    if remote.is_empty() || remote.starts_with('-') || remote.contains(['\r', '\n']) {
        return Err(RepositorySetupError::InvalidRemote);
    }
    if !discover(root)?.initialized {
        return Err(RepositorySetupError::NotRepository);
    }

    let remotes = checked_git(root, &["remote"], "remote list")?;
    if remotes.lines().any(|name| name.trim() == "origin") {
        let current = checked_git(root, &["remote", "get-url", "origin"], "remote lookup")?;
        let current = current.trim();
        if current == remote {
            return discover(root).map_err(Into::into);
        }
        return Err(RepositorySetupError::OriginAlreadyConfigured(
            current.to_owned(),
        ));
    }

    checked_git(
        root,
        &["remote", "add", "origin", remote],
        "remote add origin",
    )?;
    discover(root).map_err(Into::into)
}

fn checked_git(
    root: &Path,
    args: &[&str],
    operation: &'static str,
) -> Result<String, RepositorySetupError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        return Err(RepositorySetupError::Git {
            operation,
            detail: if stderr.is_empty() {
                if stdout.is_empty() {
                    format!("Git exited with {}", output.status)
                } else {
                    stdout
                }
            } else {
                stderr
            },
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub fn files_page(
    root: &Path,
    query: RepositoryFilePageQuery,
) -> Result<RepositoryPage<RepositoryFile>, RepositoryBrowserError> {
    let status = repository_status::snapshot(root)?;
    let limit = query.limit.clamp(1, 100);
    let mut matching = status
        .files
        .into_iter()
        .filter(|file| match query.view {
            RepositoryFileView::Staged => file.staged.is_some() && !file.conflicted,
            RepositoryFileView::Unstaged => {
                file.unstaged.is_some() && !file.untracked && !file.conflicted
            }
            RepositoryFileView::Untracked => file.untracked,
            RepositoryFileView::Conflicted => file.conflicted,
        })
        .skip(query.cursor)
        .take(limit + 1)
        .collect::<Vec<_>>();
    let has_more = matching.len() > limit;
    matching.truncate(limit);
    Ok(RepositoryPage {
        next_cursor: has_more.then_some(query.cursor + matching.len()),
        items: matching,
    })
}

pub fn commits_page(
    root: &Path,
    query: RepositoryPageQuery,
) -> Result<code_review::CodeReviewPage, RepositoryBrowserError> {
    code_review::discover_repository_page(root, query.cursor, query.limit).map_err(Into::into)
}

fn default_page_limit() -> usize {
    50
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

/// Launch a path already validated by its owning subsystem (for example, an attachment
/// resolved through durable ticket metadata).
pub fn act_on_host_path(
    path: &Path,
    action: RepositoryFileAction,
) -> Result<(), RepositoryBrowserError> {
    let target = match action {
        RepositoryFileAction::Open => path.to_path_buf(),
        RepositoryFileAction::Reveal if cfg!(target_os = "macos") => {
            return spawn("open", &["-R".into(), path.as_os_str().into()]);
        }
        RepositoryFileAction::Reveal if cfg!(target_os = "windows") => {
            return spawn("explorer", &[format!("/select,{}", path.display()).into()]);
        }
        RepositoryFileAction::Reveal => path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| path.to_path_buf()),
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

    fn git(root: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    #[test]
    fn represents_an_uninitialized_checkout_without_an_api_error() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("project.txt"), "project\n").unwrap();

        let overview = discover(root.path()).unwrap();
        assert!(!overview.initialized);
        assert_eq!(overview.root, root.path().display().to_string());
        assert_eq!(overview.commit_count, 0);
        assert!(!overview.status.clean);
        assert!(overview.ranges.is_empty());
    }

    #[test]
    fn initializes_a_checkout_idempotently_without_committing_its_files() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("project.txt"), "project\n").unwrap();

        let first = initialize(root.path()).unwrap();
        let second = initialize(root.path()).unwrap();
        assert!(first.initialized);
        assert!(second.initialized);
        assert_eq!(first.status.untracked, 1);
        assert_eq!(first.commit_count, 0);
        assert!(
            !Command::new("git")
                .arg("-C")
                .arg(root.path())
                .args(["rev-parse", "--verify", "HEAD"])
                .output()
                .unwrap()
                .status
                .success()
        );
    }

    #[test]
    fn configures_origin_idempotently_without_replacing_it_or_pushing() {
        let root = tempfile::tempdir().unwrap();
        initialize(root.path()).unwrap();

        let first = "git@example.com:team/project.git";
        assert!(configure_origin(root.path(), first).unwrap().initialized);
        assert!(configure_origin(root.path(), first).unwrap().initialized);
        assert_eq!(git(root.path(), &["remote", "get-url", "origin"]), first);
        assert!(matches!(
            configure_origin(root.path(), "git@example.com:other/project.git"),
            Err(RepositorySetupError::OriginAlreadyConfigured(current)) if current == first
        ));
        assert_eq!(git(root.path(), &["remote", "get-url", "origin"]), first);
    }

    #[test]
    fn rejects_unsafe_remote_values_before_changing_git_configuration() {
        let root = tempfile::tempdir().unwrap();
        initialize(root.path()).unwrap();
        for remote in ["", "--upload-pack=bad", "good\nbad"] {
            assert!(matches!(
                configure_origin(root.path(), remote),
                Err(RepositorySetupError::InvalidRemote)
            ));
        }
        assert!(git(root.path(), &["remote"]).is_empty());
    }

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
