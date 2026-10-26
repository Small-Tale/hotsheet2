//! Read-only planning for reclaiming attachment history in a dedicated Git ticket store.
//!
//! A plan never edits a ticket or repository. Its fingerprint binds the selected payloads
//! to the current Git head so execution can reject stale previews.

use crate::store::{FsStore, StoreError, lock_git_transaction, lock_history_sync};
use hotsheet_model::{AttachmentActorRole, Note, NoteActor, NoteKind, Status, Timestamp, Ulid};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use time::OffsetDateTime;

const MAX_CANDIDATES: usize = 20_000;
const MAX_PATH_COMPONENT: usize = 255;
const MAX_HISTORY_BYTES: u64 = 64 * 1024 * 1024;
const MAX_HISTORY_BLOBS: usize = 20_000;

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
    Orphaned,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct HistoricalBlob {
    /// Git object identity retained in the mirror backup.
    pub oid: String,
    pub bytes: u64,
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
    /// Historical versions of a deleted path. Empty for payloads still on ticket heads.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub historical_blobs: Vec<HistoricalBlob>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub removed_in_commit: Option<String>,
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

/// The original checkout must not resume writing after its remote history was
/// replaced. Keep this guard in Git's private directory so it survives a server
/// restart without making the working tree dirty (HS2-RFJ1GN).
pub fn recovery_marker_path(root: &Path) -> Result<PathBuf, CompactionError> {
    let path = git(
        root,
        &[
            "rev-parse",
            "--git-path",
            "hotsheet-compaction-recovery.json",
        ],
    )?;
    let path = PathBuf::from(path);
    Ok(if path.is_absolute() {
        path
    } else {
        root.join(path)
    })
}

pub fn recovery_required(root: &Path) -> bool {
    match recovery_marker_path(root) {
        Ok(path) => path.exists(),
        // If a Git checkout cannot resolve its private marker, do not let it
        // resume writes or sync on a potentially stale branch (HS2-RFJ1GN).
        Err(_) => root.join(".git").exists(),
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct RecoveryMarker {
    schema_version: u32,
    old_head: String,
    branch: String,
    fingerprint: String,
    backup: PathBuf,
}

/// Clear the write/sync guard only after the local checkout has been restored to
/// the exact remote head (or the rewrite failed before changing origin). The
/// mirror backup is retained for the human to remove later (HS2-RFJ1GN).
pub fn recover_store(root: &Path) -> Result<(), CompactionError> {
    let _history = lock_history_sync(root)?;
    let _git = lock_git_transaction(root)?;
    let path = recovery_marker_path(root)?;
    let marker: RecoveryMarker = serde_json::from_slice(&fs::read(&path)?)
        .map_err(|error| CompactionError::Invalid(format!("invalid recovery marker: {error}")))?;
    if marker.schema_version != 1 {
        return Err(CompactionError::Invalid(
            "recovery marker has an unsupported version".into(),
        ));
    }
    let branch = git(root, &["symbolic-ref", "--quiet", "--short", "HEAD"])?;
    let local = git(root, &["rev-parse", "HEAD"])?;
    let remote = git(
        root,
        &["ls-remote", "origin", &format!("refs/heads/{branch}")],
    )?;
    if branch != marker.branch
        || remote.split_whitespace().next() != Some(local.as_str())
        || !git(root, &["status", "--porcelain", "--untracked-files=normal"])?.is_empty()
    {
        return Err(CompactionError::Invalid(
            "recovery requires a clean checkout on the recorded branch at the exact origin head; preserve local work, then re-clone or reset to origin before retrying".into(),
        ));
    }
    fs::remove_file(path)?;
    Ok(())
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

fn git_output_bounded(root: &Path, args: &[&str]) -> Result<Vec<u8>, CompactionError> {
    let mut child = crate::git::command_in(root)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut output = Vec::new();
    let read = child
        .stdout
        .take()
        .ok_or_else(|| CompactionError::Invalid("cannot read Git history".into()))?
        .take(MAX_HISTORY_BYTES + 1)
        .read_to_end(&mut output);
    if read.is_err() || output.len() as u64 > MAX_HISTORY_BYTES {
        let _ = child.kill();
        let _ = child.wait();
        return Err(CompactionError::Invalid(
            "attachment history exceeds the bounded inventory; narrow the repository first".into(),
        ));
    }
    if !child.wait()?.success() {
        return Err(CompactionError::Invalid(
            "cannot inventory attachment history".into(),
        ));
    }
    Ok(output)
}

fn historical_attachment_path(path: &str) -> bool {
    let parts = path.split('/').collect::<Vec<_>>();
    if !(parts.len() == 3 || parts.len() == 4)
        || parts[0] != "attachments"
        || Ulid::from_string(parts[1]).is_err()
        || (parts.len() == 4 && Ulid::from_string(parts[2]).is_err())
    {
        return false;
    }
    let filename = parts[parts.len() - 1];
    !filename.is_empty()
        && filename.len() <= MAX_PATH_COMPONENT
        && filename != "."
        && filename != ".."
        && !filename.contains('\\')
}

fn deleted_attachment_commits(root: &Path) -> Result<BTreeMap<String, String>, CompactionError> {
    let log = git_output_bounded(
        root,
        &[
            "log",
            "--all",
            "--no-renames",
            "--diff-filter=D",
            "--format=%H%x00",
            "--name-only",
            "-z",
            "--",
            "attachments/",
        ],
    )?;
    let mut commit = None::<String>;
    let mut paths = BTreeMap::new();
    for raw in log.split(|byte| *byte == 0) {
        let raw = raw
            .iter()
            .copied()
            .skip_while(|byte| *byte == b'\n')
            .collect::<Vec<_>>();
        if raw.is_empty() {
            continue;
        }
        let Ok(value) = std::str::from_utf8(&raw) else {
            continue;
        };
        if value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            commit = Some(value.to_owned());
        } else if historical_attachment_path(value) {
            let id = commit.as_ref().ok_or_else(|| {
                CompactionError::Invalid("malformed Git attachment deletion history".into())
            })?;
            paths.entry(value.to_owned()).or_insert_with(|| id.clone());
            if paths.len() > MAX_CANDIDATES {
                return Err(CompactionError::Invalid(
                    "too many historical attachment paths".into(),
                ));
            }
        }
    }
    Ok(paths)
}

fn orphaned_candidates(store: &FsStore) -> Result<Vec<CompactionCandidate>, CompactionError> {
    let root = store.root();
    let removed_in = deleted_attachment_commits(root)?;
    // Raw NUL records preserve newlines and Git's rename marker in attachment names.
    // --no-renames exposes a move as a deletion plus an addition at distinct paths.
    let log = git_output_bounded(
        root,
        &[
            "log",
            "--all",
            "--raw",
            "-z",
            "--format=",
            "--abbrev=40",
            "--no-renames",
            "--",
            "attachments/",
        ],
    )?;
    let mut by_path = BTreeMap::<String, BTreeSet<String>>::new();
    let mut unsafe_paths = BTreeSet::new();
    let mut blob_count = 0usize;
    let mut records = log.split(|byte| *byte == 0);
    while let Some(raw_header) = records.next() {
        let header = raw_header
            .iter()
            .copied()
            .skip_while(|byte| *byte == b'\n')
            .collect::<Vec<_>>();
        if header.is_empty() {
            continue;
        }
        let Some(raw_path) = records.next() else {
            return Err(CompactionError::Invalid(
                "malformed Git attachment history".into(),
            ));
        };
        let Ok(path) = std::str::from_utf8(raw_path) else {
            continue;
        };
        if !historical_attachment_path(path) {
            continue;
        }
        let fields = std::str::from_utf8(&header)
            .map_err(|_| CompactionError::Invalid("malformed Git attachment history".into()))?
            .split_whitespace()
            .collect::<Vec<_>>();
        if fields.len() != 5 || !fields[0].starts_with(':') {
            return Err(CompactionError::Invalid(
                "malformed Git attachment history".into(),
            ));
        }
        if [fields[0].trim_start_matches(':'), fields[1]]
            .iter()
            .any(|mode| *mode != "000000" && *mode != "100644" && *mode != "100755")
        {
            unsafe_paths.insert(path.to_owned());
            if unsafe_paths.len() > MAX_CANDIDATES {
                return Err(CompactionError::Invalid(
                    "too many historical attachment paths".into(),
                ));
            }
            continue;
        }
        let entry = by_path.entry(path.to_owned()).or_default();
        for oid in [fields[2], fields[3]] {
            if oid.len() != 40 || !oid.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err(CompactionError::Invalid(
                    "malformed Git blob identity".into(),
                ));
            }
            if !oid.bytes().all(|byte| byte == b'0') && entry.insert(oid.to_owned()) {
                blob_count += 1;
            }
        }
        if by_path.len() > MAX_CANDIDATES || blob_count > MAX_HISTORY_BLOBS {
            return Err(CompactionError::Invalid(
                "too many historical attachment paths or blobs".into(),
            ));
        }
    }
    let mut current_paths = BTreeSet::new();
    for ticket in store.list_tickets()? {
        for attachment in ticket.attachments {
            let base = format!("attachments/{}/", ticket.id);
            current_paths.insert(format!("{base}{}", attachment.filename));
            current_paths.insert(format!("{base}{}/{}", attachment.id, attachment.filename));
        }
    }
    let mut candidates = Vec::new();
    for (path, oids) in by_path {
        if unsafe_paths.contains(&path)
            || current_paths.contains(&path)
            || fs::symlink_metadata(root.join(&path)).is_ok()
        {
            continue;
        }
        let mut historical_blobs = Vec::new();
        for oid in oids {
            let bytes = git(root, &["cat-file", "-s", &oid])?
                .parse::<u64>()
                .map_err(|_| CompactionError::Invalid("invalid Git blob size".into()))?;
            historical_blobs.push(HistoricalBlob { oid, bytes });
        }
        if historical_blobs.is_empty() {
            continue;
        }
        let Some(removed_in_commit) = removed_in.get(&path) else {
            continue;
        };
        let bytes = historical_blobs.iter().try_fold(0u64, |sum, blob| {
            sum.checked_add(blob.bytes)
                .ok_or_else(|| CompactionError::Invalid("historical blob bytes overflow".into()))
        })?;
        let parts = path.split('/').collect::<Vec<_>>();
        candidates.push(CompactionCandidate {
            ticket_id: parts[1].to_owned(),
            ticket_slug: String::new(),
            attachment_id: if parts.len() == 4 {
                parts[2].to_owned()
            } else {
                String::new()
            },
            filename: parts[parts.len() - 1].to_owned(),
            path,
            bytes,
            sha256: String::new(),
            reason: CandidateReason::Orphaned,
            historical_blobs,
            removed_in_commit: Some(removed_in_commit.clone()),
        });
    }
    Ok(candidates)
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
                    historical_blobs: Vec::new(),
                    removed_in_commit: None,
                });
                if candidates.len() > MAX_CANDIDATES {
                    return Err(CompactionError::Invalid(
                        "too many compaction candidates".into(),
                    ));
                }
            }
        }
    }
    candidates.extend(orphaned_candidates(store)?);
    if candidates.len() > MAX_CANDIDATES {
        return Err(CompactionError::Invalid(
            "too many compaction candidates".into(),
        ));
    }
    let reclaimable_payload_bytes = candidates.iter().try_fold(0u64, |sum, item| {
        sum.checked_add(item.bytes)
            .ok_or_else(|| CompactionError::Invalid("candidate bytes overflow".into()))
    })?;
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
        if candidate.reason == CandidateReason::Orphaned {
            continue;
        }
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
            kind_explicit: false,
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
    let _history = lock_history_sync(store.root())?;
    store.with_ticket_transaction(|| {
        let _git = lock_git_transaction(store.root())?;
        execute_store_locked(store, now, plan, backup, confirmation)
    })
}

fn execute_store_locked(
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
    let marker = recovery_marker_path(root)?;
    let marker_contents = RecoveryMarker {
        schema_version: 1,
        old_head: current.head.clone(),
        branch: current.branch.clone(),
        fingerprint: current.fingerprint.clone(),
        backup: backup.to_path_buf(),
    };
    let mut marker_file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&marker)
        .map_err(|error| {
            if error.kind() == io::ErrorKind::AlreadyExists {
                CompactionError::Invalid(
                    "this checkout already needs compaction recovery; verify the remote and restore it before another rewrite".into(),
                )
            } else {
                CompactionError::Io(error)
            }
        })?;
    serde_json::to_writer_pretty(&mut marker_file, &marker_contents)
        .map_err(|error| CompactionError::Invalid(error.to_string()))?;
    marker_file.write_all(b"\n")?;
    marker_file.sync_all()?;
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
    if current
        .candidates
        .iter()
        .any(|candidate| candidate.reason != CandidateReason::Orphaned)
    {
        git(&work, &["add", "-A", "--", "tickets", "attachments"])?;
    }
    let orphans = current
        .candidates
        .iter()
        .filter(|candidate| candidate.reason == CandidateReason::Orphaned)
        .collect::<Vec<_>>();
    if !orphans.is_empty() {
        let directory = work.join("compaction");
        fs::create_dir_all(&directory)?;
        let manifest_path = format!("compaction/orphaned-{}.json", current.fingerprint);
        let manifest = work.join(&manifest_path);
        let mut contents = serde_json::to_vec_pretty(&orphans)
            .map_err(|error| CompactionError::Invalid(error.to_string()))?;
        contents.push(b'\n');
        fs::write(&manifest, contents)?;
        git(&work, &["add", "--", &manifest_path])?;
    }
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
            "The original local store is deliberately left unchanged; do not push it again. After restoring it to the rewritten origin head, run `hotsheet-cli checkout compact <source> <project> --recover` before further ticket writes.".into(),
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::StoreMetadata;

    #[test]
    fn hs2_rfj1gn_unresolvable_git_marker_fails_closed() {
        let root = tempfile::tempdir().unwrap();
        assert!(!recovery_required(root.path()));
        fs::write(root.path().join(".git"), "gitdir: missing\n").unwrap();
        assert!(recovery_required(root.path()));
    }
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
            historical_blobs: Vec::new(),
            removed_in_commit: None,
        }
    }

    fn write_test_recovery_marker(store: &FsStore) {
        let marker = RecoveryMarker {
            schema_version: 1,
            old_head: git(store.root(), &["rev-parse", "HEAD"]).unwrap(),
            branch: "main".into(),
            fingerprint: String::new(),
            backup: store.root().with_extension("backup.git"),
        };
        fs::write(
            recovery_marker_path(store.root()).unwrap(),
            serde_json::to_vec(&marker).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn hs2_rfj1gn_guard_blocks_purge_queued_commit_and_activation() {
        let (_dir, store, now) = fixture();
        let mut ticket = store.list_tickets().unwrap().remove(0);
        ticket.status = Status::Deleted;
        store.write_ticket_committing(&ticket).unwrap();
        let ticket_path = store.ticket_path(&ticket.id);
        let before = fs::read(&ticket_path).unwrap();
        assert_eq!(
            crate::ops::trash_due_for_purge(&store, &Timestamp::from(now), 0)
                .unwrap()
                .len(),
            1
        );
        let head = git(store.root(), &["rev-parse", "HEAD"]).unwrap();
        write_test_recovery_marker(&store);
        assert!(crate::ops::purge_trash(&store, &Timestamp::from(now), 0).is_err());
        assert_eq!(fs::read(ticket_path).unwrap(), before);
        assert!(store.attachment_dir(&ticket.id).exists());
        let pending = store.root().join("pending.txt");
        fs::write(&pending, "synthetic queued mutation").unwrap();
        assert!(matches!(
            store.autocommit_paths("pending mutation", &[pending]),
            Err(StoreError::CompactionRecoveryRequired)
        ));
        assert!(matches!(
            store.activate_current_format(),
            Err(StoreError::CompactionRecoveryRequired)
        ));
        assert_eq!(git(store.root(), &["rev-parse", "HEAD"]).unwrap(), head);
    }

    #[test]
    fn hs2_rfj1gn_waiting_sync_rechecks_marker_before_any_commit() {
        use std::sync::mpsc;
        use std::time::Duration;

        let (_dir, store, _now) = fixture();
        let head = git(store.root(), &["rev-parse", "HEAD"]).unwrap();
        let held = lock_history_sync(store.root()).unwrap();
        let (started, start) = mpsc::channel();
        let (finished, result) = mpsc::channel();
        let root = store.root().to_path_buf();
        let worker = std::thread::spawn(move || {
            started.send(()).unwrap();
            finished.send(crate::sync::sync_once(&root)).unwrap();
        });
        start.recv().unwrap();
        assert!(result.recv_timeout(Duration::from_millis(200)).is_err());
        fs::write(
            store.root().join("pending.txt"),
            "synthetic pending mutation",
        )
        .unwrap();
        write_test_recovery_marker(&store);
        drop(held);
        assert_eq!(
            result.recv_timeout(Duration::from_secs(5)).unwrap(),
            crate::sync::SyncReport::Offline
        );
        worker.join().unwrap();
        assert_eq!(git(store.root(), &["rev-parse", "HEAD"]).unwrap(), head);
    }

    #[test]
    fn hs2_rfj1gn_recovery_waits_for_active_rewrite_exclusion() {
        use std::sync::mpsc;
        use std::time::Duration;

        let (_dir, store, _now) = fixture();
        let held = lock_history_sync(store.root()).unwrap();
        write_test_recovery_marker(&store);
        let (started, start) = mpsc::channel();
        let (finished, result) = mpsc::channel();
        let root = store.root().to_path_buf();
        let worker = std::thread::spawn(move || {
            started.send(()).unwrap();
            finished.send(recover_store(&root)).unwrap();
        });
        start.recv().unwrap();
        assert!(result.recv_timeout(Duration::from_millis(200)).is_err());
        assert!(recovery_required(store.root()));
        drop(held);
        result
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        worker.join().unwrap();
        assert!(!recovery_required(store.root()));
    }

    #[test]
    fn hs2_jrt1zp_previews_and_rewrites_only_deleted_attachment_history() {
        let (dir, store, now) = fixture();
        let ticket = store.list_tickets().unwrap().remove(0);
        let old = Timestamp::new("2026-01-01T00:00:00Z");
        let orphan_id = Ulid::new();
        store
            .write_attachment(
                &ticket.id,
                orphan_id,
                old.clone(),
                "removed\n==>file.png",
                b"synthetic deleted attachment bytes",
            )
            .unwrap();
        let path = format!(
            "attachments/{}/{}/removed\n==>file.png",
            ticket.id, orphan_id
        );
        let first_oid = git(store.root(), &["rev-parse", &format!("HEAD:{path}")]).unwrap();
        fs::write(store.root().join(&path), b"synthetic replacement bytes").unwrap();
        git_ok(store.root(), &["add", "--", &path]);
        git_ok(
            store.root(),
            &["commit", "-qm", "replace synthetic payload"],
        );
        let second_oid = git(store.root(), &["rev-parse", &format!("HEAD:{path}")]).unwrap();
        store
            .remove_attachment(&ticket.id, &orphan_id, old)
            .unwrap();
        let unrelated = store.root().join("other");
        fs::create_dir_all(&unrelated).unwrap();
        fs::write(unrelated.join("unrelated.bin"), b"unrelated bytes").unwrap();
        git_ok(store.root(), &["add", "other/unrelated.bin"]);
        git_ok(store.root(), &["commit", "-qm", "unrelated"]);
        git_ok(store.root(), &["push", "-q", "origin", "main"]);

        let plan = plan_store(&store, now, 1000, 15).unwrap();
        let orphan = plan
            .candidates
            .iter()
            .find(|item| item.reason == CandidateReason::Orphaned)
            .unwrap();
        assert_eq!(
            plan.candidates.len(),
            1,
            "current attachments stay outside the orphan plan"
        );
        assert_eq!(orphan.path, path);
        assert_eq!(orphan.historical_blobs.len(), 2);
        assert!(
            orphan
                .historical_blobs
                .iter()
                .any(|blob| blob.oid == first_oid && blob.bytes == 34)
        );
        assert!(
            orphan
                .historical_blobs
                .iter()
                .any(|blob| blob.oid == second_oid && blob.bytes == 27)
        );
        assert_eq!(orphan.bytes, 61);
        assert_eq!(orphan.removed_in_commit.as_ref().map(String::len), Some(40));
        assert!(
            !plan
                .candidates
                .iter()
                .any(|item| item.path.starts_with("other/"))
        );
        assert_eq!(plan, plan_store(&store, now, 1000, 15).unwrap());

        let backup = dir.path().join("orphan-backup.git");
        let result = execute_store(
            &store,
            now,
            &plan,
            &backup,
            &format!("rewrite {} {}", plan.branch, plan.fingerprint),
        )
        .unwrap();
        let work = backup.with_extension("rewrite-work");
        let manifest: serde_json::Value = serde_json::from_slice(
            &fs::read(work.join(format!("compaction/orphaned-{}.json", plan.fingerprint))).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest[0]["path"], path);
        assert_eq!(manifest[0]["historical_blobs"].as_array().unwrap().len(), 2);
        assert_eq!(
            manifest[0]["removed_in_commit"],
            orphan.removed_in_commit.as_deref().unwrap()
        );
        let rewritten_objects = git(&work, &["rev-list", "--objects", "--all"]).unwrap();
        assert!(!rewritten_objects.contains(&path));
        assert!(rewritten_objects.contains("other/unrelated.bin"));
        assert_eq!(
            git(&backup, &["rev-parse", "main"]).unwrap(),
            result.old_head
        );
        assert_eq!(
            git(&dir.path().join("remote.git"), &["rev-parse", "main"]).unwrap(),
            result.new_head
        );
    }

    #[test]
    fn hs2_jrt1zp_orphan_only_rewrite_needs_no_current_attachment_directory() {
        let (dir, store, now) = fixture();
        let ticket = store.list_tickets().unwrap().remove(0);
        let old = Timestamp::new("2026-01-01T00:00:00Z");
        for attachment in ticket.attachments {
            store
                .remove_attachment(&ticket.id, &attachment.id, old.clone())
                .unwrap();
        }
        git_ok(store.root(), &["push", "-q", "origin", "main"]);
        let plan = plan_store(&store, now, 1000, 15).unwrap();
        assert_eq!(plan.candidates.len(), 3);
        assert!(
            plan.candidates
                .iter()
                .all(|item| item.reason == CandidateReason::Orphaned)
        );
        let backup = dir.path().join("orphan-only-backup.git");
        let result = execute_store(
            &store,
            now,
            &plan,
            &backup,
            &format!("rewrite {} {}", plan.branch, plan.fingerprint),
        )
        .unwrap();
        assert_eq!(result.candidates_removed, 3);
        assert!(
            backup
                .with_extension("rewrite-work")
                .join(format!("compaction/orphaned-{}.json", plan.fingerprint))
                .exists()
        );
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
        assert!(recovery_required(store.root()));
        assert!(matches!(
            store.with_ticket_transaction(|| Ok::<_, StoreError>(())),
            Err(StoreError::CompactionRecoveryRequired)
        ));
        let reopened = FsStore::open_without_maintenance(store.root()).unwrap();
        assert!(matches!(
            reopened.with_ticket_transaction(|| Ok::<_, StoreError>(())),
            Err(StoreError::CompactionRecoveryRequired)
        ));
        assert_eq!(
            crate::sync::sync_once(store.root()),
            crate::sync::SyncReport::Offline
        );
        assert!(recover_store(store.root()).is_err());
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
        git_ok(store.root(), &["fetch", "-q", "origin"]);
        git_ok(store.root(), &["reset", "--hard", "origin/main"]);
        recover_store(store.root()).unwrap();
        assert!(!recovery_required(store.root()));
        store
            .with_ticket_transaction(|| Ok::<_, StoreError>(()))
            .unwrap();
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
