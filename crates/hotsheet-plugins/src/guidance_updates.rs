//! Read-only freshness checks for a project's exact pinned guidance sources (HS2-RDBVTG).
//! Checking never downloads rule bodies or changes the project's cached snapshot.

use crate::guidance_sources::{self, GuidanceSource, GuidanceSourceError};
use serde::Serialize;
use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

const CHECK_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateState {
    Current,
    RevisionChanged,
    Pinned,
    TagMoved,
    CheckFailed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct SourceUpdate {
    pub id: String,
    pub requested_ref: String,
    pub pinned_commit: String,
    pub observed_commit: Option<String>,
    pub state: UpdateState,
    /// A bounded, credential-free explanation when a check cannot establish freshness.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
}

impl SourceUpdate {
    fn new(source: &GuidanceSource, state: UpdateState, observed_commit: Option<String>) -> Self {
        Self {
            id: source.id.clone(),
            requested_ref: source.requested_ref.clone(),
            pinned_commit: source.commit.clone(),
            observed_commit,
            state,
            reason: None,
        }
    }

    fn failed(source: &GuidanceSource, reason: &'static str) -> Self {
        Self {
            reason: Some(reason),
            ..Self::new(source, UpdateState::CheckFailed, None)
        }
    }
}

/// Compare the registered sources without writing their pins or downloaded bodies.
/// A source's failure is reported beside healthy sources rather than failing the whole check.
pub fn check_project(project: &Path) -> Result<Vec<SourceUpdate>, GuidanceSourceError> {
    let config = guidance_sources::load(project)?;
    Ok(config.sources.iter().map(check_source).collect())
}

/// A changed branch ref is a review candidate, not proof of chronological advancement:
/// force pushes can move a branch backwards or sideways.
pub fn check_source(source: &GuidanceSource) -> SourceUpdate {
    let requested = source.requested_ref.as_str();
    if (7..=40).contains(&requested.len())
        && requested.bytes().all(|byte| byte.is_ascii_hexdigit())
        && source
            .commit
            .get(..requested.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(requested))
    {
        return SourceUpdate::new(source, UpdateState::Pinned, None);
    }
    let branch = requested.strip_prefix("refs/heads/").unwrap_or(requested);
    let tag = requested.strip_prefix("refs/tags/").unwrap_or(requested);
    let head = requested == "HEAD";
    let branch_ref = format!("refs/heads/{branch}");
    let tag_ref = format!("refs/tags/{tag}");
    let peeled_tag_ref = format!("{tag_ref}^{{}}");
    let patterns = if head {
        vec!["HEAD".to_owned()]
    } else if requested.starts_with("refs/heads/") {
        vec![branch_ref.clone()]
    } else if requested.starts_with("refs/tags/") {
        vec![tag_ref.clone(), peeled_tag_ref.clone()]
    } else {
        vec![branch_ref.clone(), tag_ref.clone(), peeled_tag_ref.clone()]
    };

    let mut command = hotsheet_ticketing::git::command();
    command
        .args(["ls-remote", "--quiet", "--"])
        .arg(&source.repository)
        .args(&patterns)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let Ok(mut child) = command.spawn() else {
        return SourceUpdate::failed(source, "Git could not start the source check");
    };
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() < CHECK_TIMEOUT => {
                std::thread::sleep(Duration::from_millis(50))
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return SourceUpdate::failed(source, "Source check timed out");
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return SourceUpdate::failed(source, "Git could not finish the source check");
            }
        }
    }
    let Ok(output) = child.wait_with_output() else {
        return SourceUpdate::failed(source, "Git could not read the source check");
    };
    if !output.status.success() {
        return SourceUpdate::failed(source, "Source ref could not be checked");
    }
    let Ok(text) = String::from_utf8(output.stdout) else {
        return SourceUpdate::failed(source, "Source ref response was invalid");
    };
    let mut branch_commit = None;
    let mut tag_commit = None;
    let mut head_commit = None;
    for line in text.lines() {
        let Some((commit, reference)) = line.split_once('\t') else {
            return SourceUpdate::failed(source, "Source ref response was invalid");
        };
        if commit.len() != 40 || !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return SourceUpdate::failed(source, "Source ref response was invalid");
        }
        if reference == branch_ref {
            branch_commit = Some(commit.to_owned());
        } else if reference == peeled_tag_ref || (reference == tag_ref && tag_commit.is_none()) {
            tag_commit = Some(commit.to_owned());
        } else if reference == "HEAD" && head {
            head_commit = Some(commit.to_owned());
        }
    }
    if branch_commit.is_some() && tag_commit.is_some() {
        return SourceUpdate::failed(source, "Source ref matches both a branch and tag");
    }
    if let Some(observed) = branch_commit.or(head_commit) {
        let state = if observed == source.commit {
            UpdateState::Current
        } else {
            UpdateState::RevisionChanged
        };
        return SourceUpdate::new(source, state, Some(observed));
    }
    if let Some(observed) = tag_commit {
        let state = if observed == source.commit {
            UpdateState::Pinned
        } else {
            UpdateState::TagMoved
        };
        return SourceUpdate::new(source, state, Some(observed));
    }
    SourceUpdate::failed(source, "Source ref was not found")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::guidance_sources::GuidanceConfig;
    use std::fs;
    use std::path::Path;

    fn git(repo: &Path, args: &[&str]) -> String {
        let output = hotsheet_ticketing::git::command_in(repo)
            .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    fn commit(repo: &Path, content: &str) -> String {
        fs::write(repo.join("rule.md"), content).unwrap();
        git(repo, &["add", "rule.md"]);
        git(repo, &["commit", "-qm", content]);
        git(repo, &["rev-parse", "HEAD"])
    }

    fn source(repo: &Path, requested_ref: &str, pinned_commit: &str) -> GuidanceSource {
        GuidanceSource {
            id: "fixture".into(),
            repository: repo.to_str().unwrap().into(),
            requested_ref: requested_ref.into(),
            commit: pinned_commit.into(),
            rules: vec![],
            skills: vec![],
            selected_rules: vec![],
            selected_skills: vec![],
        }
    }

    #[test]
    fn branch_check_survives_advance_rewind_missing_and_empty_then_refilled_config() {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("source");
        let project = root.path().join("project");
        fs::create_dir_all(&repo).unwrap();
        fs::create_dir(&project).unwrap();
        git(&repo, &["init", "-q"]);
        let first = commit(&repo, "first");
        git(&repo, &["branch", "-M", "main"]);
        let mut pinned = source(&repo, "main", &first);
        assert_eq!(check_source(&pinned).state, UpdateState::Current);
        let second = commit(&repo, "second");
        let changed = check_source(&pinned);
        assert_eq!(changed.state, UpdateState::RevisionChanged);
        assert_eq!(changed.observed_commit.as_deref(), Some(second.as_str()));
        assert_eq!(pinned.commit, first, "a check must not advance the pin");
        git(&repo, &["reset", "--hard", &first]);
        assert_eq!(check_source(&pinned).state, UpdateState::Current);
        git(&repo, &["checkout", "--detach", "-q"]);
        git(&repo, &["branch", "-D", "main"]);
        assert_eq!(check_source(&pinned).state, UpdateState::CheckFailed);
        git(&repo, &["branch", "main", &second]);
        assert_eq!(check_source(&pinned).state, UpdateState::RevisionChanged);

        guidance_sources::save(&project, &GuidanceConfig::default()).unwrap();
        assert!(check_project(&project).unwrap().is_empty());
        let config = GuidanceConfig {
            sources: vec![pinned.clone()],
            ..GuidanceConfig::default()
        };
        guidance_sources::save(&project, &config).unwrap();
        assert_eq!(
            check_project(&project).unwrap()[0].state,
            UpdateState::RevisionChanged
        );
        pinned.repository = root.path().join("missing").to_str().unwrap().into();
        let failed = check_source(&pinned);
        assert_eq!(failed.state, UpdateState::CheckFailed);
        assert_eq!(failed.reason, Some("Source ref could not be checked"));
    }

    #[test]
    fn immutable_and_moved_refs_never_claim_a_newer_branch_revision() {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path();
        git(repo, &["init", "-q"]);
        let first = commit(repo, "first");
        let second = commit(repo, "second");
        assert_eq!(
            check_source(&source(repo, &first, &first)).state,
            UpdateState::Pinned
        );
        assert_eq!(
            check_source(&source(repo, &first[..7], &first)).state,
            UpdateState::Pinned
        );
        git(repo, &["branch", "deadbee", &second]);
        assert_eq!(
            check_source(&source(repo, "deadbee", &first)).state,
            UpdateState::RevisionChanged
        );
        git(repo, &["tag", "v1", &first]);
        assert_eq!(
            check_source(&source(repo, "v1", &first)).state,
            UpdateState::Pinned
        );
        git(repo, &["tag", "-f", "v1", &second]);
        let moved = check_source(&source(repo, "v1", &first));
        assert_eq!(moved.state, UpdateState::TagMoved);
        assert_eq!(moved.observed_commit.as_deref(), Some(second.as_str()));
        git(repo, &["branch", "v1", &second]);
        assert_eq!(
            check_source(&source(repo, "v1", &first)).reason,
            Some("Source ref matches both a branch and tag")
        );
        assert_eq!(
            check_source(&source(repo, "refs/tags/v1", &first)).state,
            UpdateState::TagMoved
        );
        assert_eq!(
            check_source(&source(repo, "refs/heads/v1", &first)).state,
            UpdateState::RevisionChanged
        );
        git(repo, &["tag", "-a", "v2", "-m", "annotated", &first]);
        assert_eq!(
            check_source(&source(repo, "v2", &first)).state,
            UpdateState::Pinned
        );
        git(repo, &["tag", "-fa", "v2", "-m", "moved", &second]);
        assert_eq!(
            check_source(&source(repo, "v2", &first)).state,
            UpdateState::TagMoved
        );
    }
}
