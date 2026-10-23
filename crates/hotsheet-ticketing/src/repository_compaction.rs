//! Read-only planning for reclaiming attachment history in a dedicated Git ticket store.
//!
//! A plan never edits a ticket or repository. Its fingerprint binds the selected payloads
//! to the current Git head so execution can reject stale previews.

use crate::store::{FsStore, StoreError};
use hotsheet_model::{AttachmentActorRole, Note, NoteActor, NoteKind, Status, Timestamp, Ulid};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use time::OffsetDateTime;

const MAX_CANDIDATES: usize = 20_000;
const MAX_PATH_COMPONENT: usize = 255;

#[derive(Debug, thiserror::Error)]
pub enum CompactionError {
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Io(#[from] io::Error),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CandidateReason {
    Duplicate,
    Oversized,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompactionCandidate {
    pub ticket_id: String,
    pub ticket_slug: String,
    pub attachment_id: String,
    pub filename: String,
    /// Slash-separated Git path, relative to the dedicated store repository root.
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
    pub reason: CandidateReason,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompactionPlan {
    pub head: String,
    pub branch: String,
    pub min_age_days: u64,
    pub oversized_bytes: u64,
    pub candidates: Vec<CompactionCandidate>,
    pub reclaimable_payload_bytes: u64,
    pub fingerprint: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompactionResult {
    pub old_head: String,
    pub new_head: String,
    pub backup: String,
    pub candidates_removed: usize,
    pub git_bytes_before: u64,
    pub git_bytes_after: u64,
    pub recovery: Vec<String>,
}

fn git(root: &Path, args: &[&str]) -> Result<String, CompactionError> {
    let output = crate::git::command_in(root).args(args).output()?;
    if !output.status.success() {
        return Err(CompactionError::Invalid(format!(
            "git {} failed: {}",
            args.first().copied().unwrap_or(""),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn run_git(args: &[&str], cwd: &Path) -> Result<String, CompactionError> {
    let output = crate::git::command_in(cwd).args(args).output()?;
    if !output.status.success() {
        return Err(CompactionError::Invalid(format!(
            "git {} failed: {}",
            args.first().copied().unwrap_or(""),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn ensure_dedicated_clean_store(root: &Path) -> Result<(String, String), CompactionError> {
    let top = PathBuf::from(git(root, &["rev-parse", "--show-toplevel"])?);
    if top.canonicalize()? != root.canonicalize()? {
        return Err(CompactionError::Invalid(
            "history rewrite requires a dedicated ticket-store Git repository; this store shares another repository".into(),
        ));
    }
    if !git(root, &["status", "--porcelain", "--untracked-files=normal"])?.is_empty() {
        return Err(CompactionError::Invalid(
            "ticket store has uncommitted changes; commit or remove them before planning compaction".into(),
        ));
    }
    let branch = git(root, &["symbolic-ref", "--quiet", "--short", "HEAD"])?;
    let head = git(root, &["rev-parse", "HEAD"])?;
    let remote = git(root, &["remote", "get-url", "origin"])?;
    if branch.is_empty() || head.len() != 40 || remote.is_empty() {
        return Err(CompactionError::Invalid(
            "compaction requires a named branch, a Git commit, and an origin remote".into(),
        ));
    }
    Ok((head, branch))
}

fn file_hash(path: &Path) -> Result<(u64, String), CompactionError> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut bytes = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        digest.update(&buffer[..n]);
        bytes = bytes.saturating_add(n as u64);
    }
    Ok((bytes, format!("{:x}", digest.finalize())))
}

fn payload_path(
    root: &Path,
    ticket_id: &str,
    attachment_id: &str,
    filename: &str,
) -> Result<PathBuf, CompactionError> {
    if filename.is_empty()
        || filename.len() > MAX_PATH_COMPONENT
        || filename == "."
        || filename == ".."
        || filename.contains('/')
        || filename.contains('\\')
    {
        return Err(CompactionError::Invalid(
            "attachment has an unsafe filename".into(),
        ));
    }
    let base = root.join("attachments").join(ticket_id);
    let nested = base.join(attachment_id).join(filename);
    let path = if nested.is_file() {
        nested
    } else {
        base.join(filename)
    };
    if !fs::symlink_metadata(&path)?.file_type().is_file() {
        return Err(CompactionError::Invalid(format!(
            "attachment payload is not a regular file: {}",
            path.display()
        )));
    }
    Ok(path)
}

/// Inventory duplicate and oversized payloads on closed tickets older than `min_age_days`.
/// Only a dedicated, clean Git store with an `origin` can produce an executable plan.
pub fn plan_store(
    store: &FsStore,
    now: OffsetDateTime,
    min_age_days: u64,
    oversized_bytes: u64,
) -> Result<CompactionPlan, CompactionError> {
    if min_age_days == 0 || min_age_days > 36_500 || oversized_bytes == 0 {
        return Err(CompactionError::Invalid(
            "minimum age and oversized threshold must both be positive".into(),
        ));
    }
    let root = store.root();
    let (head, branch) = ensure_dedicated_clean_store(root)?;
    let cutoff = now - time::Duration::days(min_age_days as i64);
    let mut seen = BTreeMap::<String, String>::new();
    let mut candidates = Vec::new();
    let mut tickets = store.list_tickets()?;
    tickets.sort_by_key(|ticket| ticket.id);
    for ticket in tickets {
        if !matches!(
            ticket.status,
            Status::Completed | Status::Verified | Status::Archive
        ) {
            continue;
        }
        let closed_at = ticket
            .verified_at
            .as_ref()
            .or(ticket.completed_at.as_ref())
            .unwrap_or(&ticket.updated_at);
        if !closed_at.instant().is_some_and(|date| date <= cutoff) {
            continue;
        }
        let ticket_id = ticket.id.to_string();
        let mut attachments = ticket.attachments;
        attachments.sort_by_key(|attachment| attachment.id);
        for attachment in attachments {
            let attachment_id = attachment.id.to_string();
            let path = payload_path(root, &ticket_id, &attachment_id, &attachment.filename)?;
            let (bytes, sha256) = file_hash(&path)?;
            let relative = path
                .strip_prefix(root)
                .map_err(|_| CompactionError::Invalid("attachment escaped store root".into()))?;
            let relative = relative.to_string_lossy().replace('\\', "/");
            let reason = if seen.contains_key(&sha256) {
                Some(CandidateReason::Duplicate)
            } else if bytes >= oversized_bytes {
                Some(CandidateReason::Oversized)
            } else {
                None
            };
            seen.insert(sha256.clone(), relative.clone());
            if let Some(reason) = reason {
                candidates.push(CompactionCandidate {
                    ticket_id: ticket_id.clone(),
                    ticket_slug: ticket.slug.clone(),
                    attachment_id,
                    filename: attachment.filename,
                    path: relative,
                    bytes,
                    sha256,
                    reason,
                });
                if candidates.len() > MAX_CANDIDATES {
                    return Err(CompactionError::Invalid(
                        "too many compaction candidates".into(),
                    ));
                }
            }
        }
    }
    let reclaimable_payload_bytes = candidates.iter().map(|item| item.bytes).sum();
    let fingerprint = {
        let payload =
            serde_json::to_vec(&(&head, &branch, min_age_days, oversized_bytes, &candidates))
                .map_err(|error| CompactionError::Invalid(error.to_string()))?;
        format!("{:x}", Sha256::digest(payload))
    };
    Ok(CompactionPlan {
        head,
        branch,
        min_age_days,
        oversized_bytes,
        candidates,
        reclaimable_payload_bytes,
        fingerprint,
    })
}

fn add_tombstone_notes(
    store: &FsStore,
    plan: &CompactionPlan,
    now: OffsetDateTime,
) -> Result<(), CompactionError> {
    let mut by_ticket = BTreeMap::<String, Vec<&CompactionCandidate>>::new();
    for candidate in &plan.candidates {
        by_ticket
            .entry(candidate.ticket_id.clone())
            .or_default()
            .push(candidate);
    }
    for (ticket_id, candidates) in by_ticket {
        let id = Ulid::from_string(&ticket_id)
            .map_err(|error| CompactionError::Invalid(error.to_string()))?;
        let original = store.read_ticket(&id)?;
        let rows = candidates
            .iter()
            .map(|candidate| {
                let attachment = original
                    .attachments
                    .iter()
                    .find(|item| item.id.to_string() == candidate.attachment_id)
                    .ok_or_else(|| {
                        CompactionError::Invalid("selected attachment metadata changed".into())
                    })?;
                let metadata = serde_json::to_string_pretty(attachment)
                    .map_err(|error| CompactionError::Invalid(error.to_string()))?;
                let metadata = metadata
                    .lines()
                    .map(|line| format!("    {line}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                Ok(format!(
                    "- `{}` (`{}`): {} bytes, SHA-256 `{}`, reason: {:?}.\n\n  Original attachment metadata:\n\n{metadata}",
                    candidate.filename,
                    candidate.attachment_id,
                    candidate.bytes,
                    candidate.sha256,
                    candidate.reason
                ))
            })
            .collect::<Result<Vec<_>, CompactionError>>()?
            .join("\n\n");
        for candidate in &candidates {
            let attachment_id = Ulid::from_string(&candidate.attachment_id)
                .map_err(|error| CompactionError::Invalid(error.to_string()))?;
            store.remove_attachment(&id, &attachment_id, Timestamp::from(now))?;
        }
        let mut ticket = store.read_ticket(&id)?;
        let text = format!(
            "## Compacted attachments\n\nThe payloads below were removed from the ticket repository with an opt-in history rewrite. Their original metadata is retained for audit; restore bytes from the backup named in the compaction result.\n\n{rows}"
        );
        ticket.notes.push(Note {
            id: Ulid::new(),
            kind: NoteKind::Regular,
            created_at: Timestamp::from(now),
            edited_at: Timestamp::from(now),
            summary: None,
            confidence: None,
            feedback_for: None,
            feedback_request: false,
            response_to: None,
            ai_feedback: None,
            human_edited: false,
            actor: Some(NoteActor {
                role: AttachmentActorRole::System,
                id: Some("repository-compaction".into()),
            }),
            text,
        });
        ticket.updated_at = Timestamp::from(now);
        store.write_ticket_committing(&ticket)?;
    }
    Ok(())
}

/// git-filter-repo's paths file treats line breaks as separators and `==>` as a
/// rename directive. Encode those paths as exact byte regexes; ordinary paths
/// retain its faster literal matching (HS2-M0VTRA).
fn write_filter_paths(
    mut output: impl Write,
    candidates: &[CompactionCandidate],
) -> io::Result<()> {
    for candidate in candidates {
        let path = candidate.path.as_bytes();
        if path.contains(&b'\n')
            || path.contains(&b'\r')
            || path.windows(3).any(|part| part == b"==>")
        {
            output.write_all(b"regex:\\A")?;
            for byte in path {
                write!(output, "\\x{byte:02x}")?;
            }
            output.write_all(b"\\Z\n")?;
        } else {
            output.write_all(b"literal:")?;
            output.write_all(path)?;
            output.write_all(b"\n")?;
        }
    }
    output.flush()
}

/// Rewrite a disposable clone, keeping a mirror backup and an untouched original checkout.
/// The caller must display the full plan and obtain an exact confirmation string first.
/// Only the selected branch is accepted; all other local and remote refs must be absent.
pub fn execute_store(
    store: &FsStore,
    now: OffsetDateTime,
    plan: &CompactionPlan,
    backup: &Path,
    confirmation: &str,
) -> Result<CompactionResult, CompactionError> {
    let current = plan_store(store, now, plan.min_age_days, plan.oversized_bytes)?;
    if current != *plan || current.candidates.is_empty() {
        return Err(CompactionError::Invalid(
            "compaction preview is stale or has no candidates".into(),
        ));
    }
    let required = format!("rewrite {} {}", current.branch, current.fingerprint);
    if confirmation != required {
        return Err(CompactionError::Invalid(format!(
            "confirmation must exactly match: {required}"
        )));
    }
    let root = store.root();
    if !crate::git::command_in(root)
        .args(["filter-repo", "--version"])
        .current_dir(root)
        .output()
        .is_ok_and(|output| output.status.success())
    {
        return Err(CompactionError::Invalid(
            "git-filter-repo is required for history rewrite. Install it from the official git-filter-repo project, then rerun the preview and execution command".into(),
        ));
    }
    let heads = git(
        root,
        &[
            "for-each-ref",
            "--format=%(refname)",
            "refs/heads",
            "refs/tags",
        ],
    )?;
    if heads
        .lines()
        .any(|item| item != format!("refs/heads/{}", current.branch))
    {
        return Err(CompactionError::Invalid(
            "history rewrite currently requires a single branch and no tags".into(),
        ));
    }
    let remote_head = git(
        root,
        &[
            "ls-remote",
            "origin",
            &format!("refs/heads/{}", current.branch),
        ],
    )?;
    if remote_head.split_whitespace().next() != Some(current.head.as_str()) {
        return Err(CompactionError::Invalid(
            "origin changed or the local branch is not pushed; refresh the preview".into(),
        ));
    }
    let expected_ref = format!("refs/heads/{}", current.branch);
    let remote_refs = git(root, &["ls-remote", "--refs", "origin"])?;
    if remote_refs
        .lines()
        .any(|line| line.split_whitespace().nth(1) != Some(expected_ref.as_str()))
    {
        return Err(CompactionError::Invalid(
            "history rewrite currently requires origin to have a single branch and no other refs"
                .into(),
        ));
    }
    if backup.exists() {
        return Err(CompactionError::Invalid(
            "backup destination already exists".into(),
        ));
    }
    let backup_parent = backup
        .parent()
        .ok_or_else(|| CompactionError::Invalid("backup needs a parent directory".into()))?;
    if !backup_parent.is_dir() {
        return Err(CompactionError::Invalid(
            "backup parent directory does not exist".into(),
        ));
    }
    let work = backup.with_extension("rewrite-work");
    if work.exists() {
        return Err(CompactionError::Invalid(
            "disposable rewrite directory already exists".into(),
        ));
    }
    let parent = backup_parent.canonicalize()?;
    if parent.starts_with(root.canonicalize()?) {
        return Err(CompactionError::Invalid(
            "backup and rewrite directories must be outside the ticket store".into(),
        ));
    }
    let mut paths_file = tempfile::NamedTempFile::new_in(backup_parent)?;
    write_filter_paths(paths_file.as_file_mut(), &current.candidates)?;
    let paths_file_arg = paths_file.path().to_string_lossy().into_owned();
    run_git(
        &[
            "clone",
            "--mirror",
            "--no-hardlinks",
            &root.to_string_lossy(),
            &backup.to_string_lossy(),
        ],
        backup_parent,
    )?;
    run_git(
        &[
            "clone",
            "--no-hardlinks",
            "--branch",
            &current.branch,
            &root.to_string_lossy(),
            &work.to_string_lossy(),
        ],
        backup_parent,
    )?;
    let before = crate::repository_size::git_object_size(&work)
        .ok_or_else(|| CompactionError::Invalid("cannot measure pre-rewrite Git size".into()))?
        .total_bytes;
    let cloned_store = FsStore::open_without_maintenance(&work)?
        .with_deferred_push()
        .with_deferred_autocommit();
    add_tombstone_notes(&cloned_store, &current, now)?;
    git(&work, &["add", "-A", "--", "tickets", "attachments"])?;
    git(
        &work,
        &[
            "-c",
            "user.name=Hot Sheet compaction",
            "-c",
            "user.email=compaction@localhost",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "Record attachment compaction tombstones",
        ],
    )?;
    run_git(
        &[
            "filter-repo",
            "--force",
            "--invert-paths",
            "--paths-from-file",
            &paths_file_arg,
        ],
        &work,
    )?;
    let new_head = git(&work, &["rev-parse", "HEAD"])?;
    let after = crate::repository_size::git_object_size(&work)
        .ok_or_else(|| CompactionError::Invalid("cannot measure rewritten Git size".into()))?
        .total_bytes;
    // git-filter-repo removes origin to prevent accidental pushes. Recreate only the
    // reviewed remote, then lease the exact head observed before the rewrite.
    let remote = git(root, &["remote", "get-url", "origin"])?;
    git(&work, &["remote", "add", "origin", &remote])?;
    git(
        &work,
        &[
            "push",
            &format!(
                "--force-with-lease=refs/heads/{}:{}",
                current.branch, current.head
            ),
            "origin",
            &format!("HEAD:refs/heads/{}", current.branch),
        ],
    )?;
    Ok(CompactionResult {
        old_head: current.head,
        new_head,
        backup: backup.display().to_string(),
        candidates_removed: current.candidates.len(),
        git_bytes_before: before,
        git_bytes_after: after,
        recovery: vec![
            "Keep the mirror backup until every collaborator confirms the replacement branch.".into(),
            "Each collaborator must re-clone the ticket repository or reset their ticket-store checkout to the rewritten origin branch after preserving local work.".into(),
            "The original local store is deliberately left unchanged; do not push it again. Re-clone it before further ticket writes.".into(),
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::StoreMetadata;
    use hotsheet_model::Ticket;

    fn git_ok(root: &Path, args: &[&str]) {
        let output = crate::git::command_in(root)
            .args(args)
            .env("GIT_AUTHOR_NAME", "test")
            .env("GIT_AUTHOR_EMAIL", "test@example.com")
            .env("GIT_COMMITTER_NAME", "test")
            .env("GIT_COMMITTER_EMAIL", "test@example.com")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn fixture() -> (tempfile::TempDir, FsStore, OffsetDateTime) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("tickets");
        fs::create_dir(&root).unwrap();
        git_ok(&root, &["init", "-q", "-b", "main"]);
        git_ok(&root, &["config", "user.name", "test"]);
        git_ok(&root, &["config", "user.email", "test@example.com"]);
        let store = FsStore::init(&root, &StoreMetadata::new("HS"))
            .unwrap()
            .with_deferred_push();
        let now = OffsetDateTime::parse(
            "2026-10-23T00:00:00Z",
            &time::format_description::well_known::Rfc3339,
        )
        .unwrap();
        let old = Timestamp::new("2026-01-01T00:00:00Z");
        let id = Ulid::new();
        let mut ticket = Ticket::new(id, "HS-OLD", "old", "task", old.clone(), old.clone());
        ticket.status = Status::Completed;
        ticket.completed_at = Some(old.clone());
        store.write_ticket_committing(&ticket).unwrap();
        store
            .write_attachment(&id, Ulid::new(), old.clone(), "a.png", b"same bytes")
            .unwrap();
        store
            .write_attachment(&id, Ulid::new(), old.clone(), "b.png", b"same bytes")
            .unwrap();
        store
            .write_attachment_with_metadata(
                &id,
                Ulid::new(),
                old,
                "large.png",
                b"much larger payload",
                hotsheet_model::AttachmentMetadata {
                    purpose: Some(hotsheet_model::AttachmentPurpose::CorrectnessEvidence),
                    batch_id: Some("review-round".into()),
                    batch_label: Some("Review round".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        let remote = dir.path().join("remote.git");
        git_ok(
            dir.path(),
            &["init", "-q", "--bare", &remote.to_string_lossy()],
        );
        git_ok(
            &root,
            &["remote", "add", "origin", &remote.to_string_lossy()],
        );
        git_ok(&root, &["push", "-q", "-u", "origin", "main"]);
        (dir, store, now)
    }

    fn candidate(path: String) -> CompactionCandidate {
        CompactionCandidate {
            ticket_id: String::new(),
            ticket_slug: String::new(),
            attachment_id: String::new(),
            filename: String::new(),
            path,
            bytes: 1,
            sha256: String::new(),
            reason: CandidateReason::Oversized,
        }
    }

    #[test]
    fn hs2_m0vtra_paths_file_handles_a_plan_larger_than_process_argv() {
        let (dir, store, now) = fixture();
        let mut plan = plan_store(&store, now, 30, 15).unwrap();
        let actual = plan.candidates.remove(0);
        let mut candidates = vec![actual];
        candidates.extend((0..MAX_CANDIDATES - 1).map(|index| {
            candidate(format!(
                "attachments/{index:026}/{index:026}/payload-{index:05}-with-a-long-name.bin"
            ))
        }));
        let mut paths_file = tempfile::NamedTempFile::new_in(dir.path()).unwrap();
        write_filter_paths(paths_file.as_file_mut(), &candidates).unwrap();
        let contents = fs::read(paths_file.path()).unwrap();
        assert!(
            contents.len() > 1_000_000,
            "selection exceeds typical argv limits"
        );
        assert_eq!(
            contents.iter().filter(|byte| **byte == b'\n').count(),
            MAX_CANDIDATES
        );
        let probe = dir.path().join("probe");
        run_git(
            &[
                "clone",
                "-q",
                &store.root().to_string_lossy(),
                &probe.to_string_lossy(),
            ],
            dir.path(),
        )
        .unwrap();
        run_git(
            &[
                "filter-repo",
                "--force",
                "--dry-run",
                "--invert-paths",
                "--paths-from-file",
                &paths_file.path().to_string_lossy(),
            ],
            &probe,
        )
        .unwrap();
    }

    #[test]
    fn hs2_m0vtra_rewrites_paths_with_line_breaks_and_rename_markers() {
        let (dir, store, now) = fixture();
        let ticket = store.list_tickets().unwrap().remove(0);
        let old = Timestamp::new("2026-01-01T00:00:00Z");
        for filename in ["line\nbreak.png", "rename==>marker.png"] {
            store
                .write_attachment(
                    &ticket.id,
                    Ulid::new(),
                    old.clone(),
                    filename,
                    b"another oversized payload",
                )
                .unwrap();
        }
        git_ok(store.root(), &["push", "-q", "origin", "main"]);
        let plan = plan_store(&store, now, 30, 15).unwrap();
        assert_eq!(plan.candidates.len(), 4);
        let backup = dir.path().join("backup.git");
        let result = execute_store(
            &store,
            now,
            &plan,
            &backup,
            &format!("rewrite {} {}", plan.branch, plan.fingerprint),
        )
        .unwrap();
        assert_eq!(result.candidates_removed, 4);
        let rewritten =
            FsStore::open_without_maintenance(backup.with_extension("rewrite-work")).unwrap();
        assert_eq!(
            rewritten
                .list_tickets()
                .unwrap()
                .remove(0)
                .attachments
                .len(),
            1
        );
        let names = crate::git::command_in(backup.with_extension("rewrite-work"))
            .args(["log", "--all", "--name-only", "-z", "--pretty=format:"])
            .output()
            .unwrap();
        assert!(names.status.success());
        for path in plan.candidates.iter().map(|candidate| &candidate.path) {
            assert!(
                !names
                    .stdout
                    .windows(path.len())
                    .any(|part| part == path.as_bytes())
            );
        }
    }

    #[test]
    fn plans_only_closed_old_duplicates_and_oversized_payloads() {
        let (_dir, store, now) = fixture();
        let plan = plan_store(&store, now, 30, 15).unwrap();
        assert_eq!(plan.candidates.len(), 2);
        assert!(
            plan.candidates
                .iter()
                .any(|item| item.reason == CandidateReason::Duplicate)
        );
        assert!(
            plan.candidates
                .iter()
                .any(|item| item.reason == CandidateReason::Oversized)
        );
        assert_eq!(plan.reclaimable_payload_bytes, 10 + 19);
        assert_eq!(plan, plan_store(&store, now, 30, 15).unwrap());
        assert!(execute_store(&store, now, &plan, &store.root().join("backup"), "wrong").is_err());
        assert!(plan_store(&store, now, 0, 15).is_err());
    }

    #[test]
    fn embedded_store_and_dirty_store_cannot_plan_history_rewrite() {
        let (dir, store, now) = fixture();
        fs::write(store.root().join("untracked"), "local work").unwrap();
        assert!(plan_store(&store, now, 30, 15).is_err());
        fs::remove_file(store.root().join("untracked")).unwrap();
        let embedded = dir.path().join("embedded");
        fs::create_dir(&embedded).unwrap();
        git_ok(dir.path(), &["init", "-q", "-b", "main"]);
        let embedded_store = FsStore::init(&embedded, &StoreMetadata::new("EM")).unwrap();
        assert!(plan_store(&embedded_store, now, 30, 15).is_err());
    }

    #[test]
    fn rewrites_disposable_clone_with_backup_and_leased_remote_push() {
        let (dir, store, now) = fixture();
        let plan = plan_store(&store, now, 30, 15).unwrap();
        let backup = dir.path().join("backup.git");
        let result = execute_store(
            &store,
            now,
            &plan,
            &backup,
            &format!("rewrite {} {}", plan.branch, plan.fingerprint),
        )
        .unwrap();
        assert_eq!(result.candidates_removed, 2);
        assert_ne!(result.old_head, result.new_head);
        assert_eq!(
            git(store.root(), &["rev-parse", "HEAD"]).unwrap(),
            result.old_head
        );
        assert_eq!(
            git(&backup, &["rev-parse", "HEAD"]).unwrap(),
            result.old_head
        );
        assert_eq!(
            git(
                dir.path().join("remote.git").as_path(),
                &["rev-parse", "main"]
            )
            .unwrap(),
            result.new_head
        );
        let work = backup.with_extension("rewrite-work");
        let rewritten = FsStore::open_without_maintenance(&work).unwrap();
        let ticket = rewritten.list_tickets().unwrap().remove(0);
        assert_eq!(ticket.attachments.len(), 1);
        assert!(
            ticket
                .notes
                .iter()
                .any(|note| note.text.contains("SHA-256"))
        );
        assert!(ticket.notes.iter().any(|note| {
            note.text.contains("correctness_evidence")
                && note.text.contains("review-round")
                && note.text.contains("Original attachment metadata")
        }));
        let objects = git(&work, &["rev-list", "--objects", "--all"]).unwrap();
        for candidate in &plan.candidates {
            assert!(!objects.contains(&candidate.path));
        }
        assert!(objects.contains("a.png"));
    }

    #[test]
    fn stale_or_multibranch_plan_cannot_create_backup_or_rewrite_origin() {
        let (dir, store, now) = fixture();
        let plan = plan_store(&store, now, 30, 15).unwrap();
        let backup = dir.path().join("backup.git");
        let confirmation = format!("rewrite {} {}", plan.branch, plan.fingerprint);
        git_ok(store.root(), &["branch", "other"]);
        let error = execute_store(&store, now, &plan, &backup, &confirmation).unwrap_err();
        assert!(error.to_string().contains("single branch"));
        assert!(!backup.exists());
        git_ok(store.root(), &["branch", "-D", "other"]);
        fs::write(store.root().join("change.txt"), "new").unwrap();
        git_ok(store.root(), &["add", "change.txt"]);
        git_ok(store.root(), &["commit", "-qm", "new"]);
        let error = execute_store(&store, now, &plan, &backup, &confirmation).unwrap_err();
        assert!(error.to_string().contains("stale"));
        assert!(!backup.exists());
    }

    #[test]
    fn changed_remote_head_rejects_rewrite_before_backup() {
        let (dir, store, now) = fixture();
        let plan = plan_store(&store, now, 30, 15).unwrap();
        let other = dir.path().join("other");
        git_ok(
            dir.path(),
            &[
                "clone",
                "-q",
                "-b",
                "main",
                &dir.path().join("remote.git").to_string_lossy(),
                &other.to_string_lossy(),
            ],
        );
        fs::write(other.join("change.txt"), "another collaborator").unwrap();
        git_ok(&other, &["add", "change.txt"]);
        git_ok(&other, &["commit", "-qm", "remote change"]);
        git_ok(&other, &["push", "-q", "origin", "main"]);
        let backup = dir.path().join("backup.git");
        let error = execute_store(
            &store,
            now,
            &plan,
            &backup,
            &format!("rewrite {} {}", plan.branch, plan.fingerprint),
        )
        .unwrap_err();
        assert!(error.to_string().contains("origin changed"));
        assert!(!backup.exists());
        assert_eq!(
            git(store.root(), &["rev-parse", "HEAD"]).unwrap(),
            plan.head
        );
    }

    #[test]
    fn unrelated_remote_ref_rejects_rewrite_before_backup() {
        let (dir, store, now) = fixture();
        let plan = plan_store(&store, now, 30, 15).unwrap();
        let remote = dir.path().join("remote.git");
        git_ok(&remote, &["update-ref", "refs/notes/keep", &plan.head]);
        let backup = dir.path().join("backup.git");
        let error = execute_store(
            &store,
            now,
            &plan,
            &backup,
            &format!("rewrite {} {}", plan.branch, plan.fingerprint),
        )
        .unwrap_err();
        assert!(error.to_string().contains("no other refs"));
        assert!(!backup.exists());
    }
}
