//! A concrete filesystem-backed git store: read/write/list ticket files on disk
//! (`docs/02-ticket-storage.md` §2.3). This is the direct implementation the CLI
//! uses today; the injected `FileSystem`/`GitLocal` ports (see [`crate::ports`]) will
//! wrap it once the server needs fakeable I/O (HS2-4).
//!
//! Layout:
//! ```text
//! <root>/
//!   hotsheet-store.json      # metadata (prefix, id strategy, sharding)
//!   tickets/<2-char shard>/<ULID>.md
//! ```

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use hotsheet_model::{Attachment, ParseError, Ticket, Timestamp, Ulid, parse_file, to_file_string};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The store metadata file at a store root.
pub const STORE_METADATA_FILE: &str = "hotsheet-store.json";

/// Store metadata (`hotsheet-store.json`, `docs/02` §2.3). camelCase on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreMetadata {
    pub schema_version: u32,
    /// Display prefix for derived slugs (e.g. `HS`); the dash is added by the slug.
    pub ticket_prefix: String,
    pub id_strategy: String,
    pub shard: String,
}

impl StoreMetadata {
    /// Default metadata for a new store with the given display prefix.
    pub fn new(ticket_prefix: impl Into<String>) -> Self {
        Self {
            schema_version: 1,
            ticket_prefix: ticket_prefix.into(),
            id_strategy: "ulid".to_string(),
            shard: "id-prefix-2".to_string(),
        }
    }
}

/// An error reading or writing the store.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("not a Hot Sheet store (no hotsheet-store.json at {0}); run `hotsheet init` first")]
    NotAStore(PathBuf),
    #[error("invalid hotsheet-store.json: {0}")]
    Metadata(#[from] serde_json::Error),
    #[error("parsing ticket {path}: {source}")]
    Parse { path: PathBuf, source: ParseError },
    #[error("git {0}")]
    Git(String),
}

/// A ticket file that could not be parsed during a resilient enumeration
/// ([`FsStore::list_tickets_resilient`]). Surfaced instead of aborting the whole
/// scan so a single bad file can never hide every healthy ticket (HS2-PRVPCQ).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorruptTicket {
    /// The on-disk path of the unparseable file.
    pub path: PathBuf,
    /// The ticket id recovered from the filename stem (or, failing that, the
    /// frontmatter `id:` line), if it is still a valid ULID.
    pub id: Option<Ulid>,
    /// The slug recovered from a readable frontmatter `slug:` line, if any.
    pub slug: Option<String>,
    /// A human-readable description of why the file could not be read/parsed.
    pub error: String,
}

/// The result of a resilient store enumeration ([`FsStore::list_tickets_resilient`]):
/// every healthy ticket plus a separate report of the files that failed to parse.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoreListing {
    /// The healthy tickets, sorted by id (≈ creation order).
    pub tickets: Vec<Ticket>,
    /// The files that could not be parsed, sorted by path. Empty for a clean store.
    pub corrupt: Vec<CorruptTicket>,
}

/// A filesystem-backed store rooted at a directory.
#[derive(Debug, Clone)]
pub struct FsStore {
    root: PathBuf,
    push_after_commit: bool,
}

impl FsStore {
    /// Initialize a new store at `root` (creates `tickets/` and writes metadata).
    /// Idempotent on the directory; overwrites metadata with the given values.
    pub fn init(root: impl Into<PathBuf>, meta: &StoreMetadata) -> Result<Self, StoreError> {
        let root = root.into();
        fs::create_dir_all(root.join("tickets"))?;
        let json = serde_json::to_string_pretty(meta)?;
        fs::write(root.join(STORE_METADATA_FILE), format!("{json}\n"))?;
        Ok(Self {
            root,
            push_after_commit: true,
        })
    }

    /// Open an existing store, erroring if `root` is not a Hot Sheet store.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let root = root.into();
        if !root.join(STORE_METADATA_FILE).is_file() {
            return Err(StoreError::NotAStore(root));
        }
        Ok(Self {
            root,
            push_after_commit: true,
        })
    }

    /// Let an owning service publish commits itself (for example, the server's
    /// coalescing fetch/rebase/push loop) instead of launching a per-write push.
    #[must_use]
    pub fn with_deferred_push(mut self) -> Self {
        self.push_after_commit = false;
        self
    }

    /// The store root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Read the store metadata.
    pub fn metadata(&self) -> Result<StoreMetadata, StoreError> {
        let text = fs::read_to_string(self.root.join(STORE_METADATA_FILE))?;
        Ok(serde_json::from_str(&text)?)
    }

    /// The on-disk path for a ticket id: `tickets/<2-char shard>/<ULID>.md`.
    pub fn ticket_path(&self, id: &Ulid) -> PathBuf {
        let s = id.to_string();
        self.root
            .join("tickets")
            .join(&s[..2])
            .join(format!("{s}.md"))
    }

    /// Write a ticket file (creating its shard directory), returning the path.
    pub fn write_ticket(&self, ticket: &Ticket) -> Result<PathBuf, StoreError> {
        let mut normalized = ticket.clone();
        if !normalized.status.is_active() {
            normalized.up_next = false;
        }
        let path = self.ticket_path(&ticket.id);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, to_file_string(&normalized))?;
        Ok(path)
    }

    /// Write a ticket, then **auto-commit** the change to the store's git repo and
    /// best-effort push it (HS2-VJD1W4) — so a mutation never leaves the store dirty and
    /// unshared, which matters for the headless `work` loop and multi-worker sync. The
    /// commit is best-effort: the write is what must succeed, so a git failure warns but
    /// doesn't fail the op. The mutating `ops` all go through here; `write_ticket` stays
    /// bare for bulk writers (import) that do their own single commit.
    pub fn write_ticket_committing(&self, ticket: &Ticket) -> Result<PathBuf, StoreError> {
        let path = self.write_ticket(ticket)?;
        let status = serde_json::to_value(ticket.status)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_else(|| "update".into());
        let msg = format!("{}: {status} — {}", ticket.slug, ticket.title);
        if let Err(e) = self.autocommit(&msg) {
            eprintln!("warning: hotsheet autocommit failed: {e}");
        }
        Ok(path)
    }

    /// Stage everything, commit with `message`, and launch a best-effort push. No-op when the
    /// store isn't a git repo or `HOTSHEET_NO_AUTOCOMMIT` is set. Returns whether a
    /// commit was actually made. Falls back to a bot identity when the repo has none
    /// configured, so a fresh/CI checkout still commits.
    pub fn autocommit(&self, message: &str) -> Result<bool, StoreError> {
        if std::env::var_os("HOTSHEET_NO_AUTOCOMMIT").is_some() || !self.root.join(".git").exists()
        {
            return Ok(false);
        }
        git(&self.root, &["add", "-A"])?;
        // Nothing staged → nothing to commit (idempotent re-writes, no-op edits).
        if git_ok(&self.root, &["diff", "--cached", "--quiet"]) {
            return Ok(false);
        }
        let has_ident = git_stdout(&self.root, &["config", "user.email"])
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        let ident: &[&str] = if has_ident {
            &[]
        } else {
            &[
                "-c",
                "user.name=Hot Sheet",
                "-c",
                "user.email=hotsheet@localhost",
            ]
        };
        let mut commit = ident.to_vec();
        commit.extend_from_slice(&["commit", "-q", "-m", message]);
        git(&self.root, &commit)?;
        // Remote publication must not hold a local mutation open for network latency.
        // Server-owned stores defer to their coalescing sync loop; headless callers launch
        // a child and a lightweight reaper. The child survives a short-lived CLI process.
        if self.push_after_commit
            && git_stdout(&self.root, &["remote"]).is_some_and(|s| !s.trim().is_empty())
        {
            if let Ok(mut child) = Command::new("git")
                .arg("-C")
                .arg(&self.root)
                .args(["push", "--quiet"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
            {
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
            }
        }
        Ok(true)
    }

    /// Read one ticket by id.
    pub fn read_ticket(&self, id: &Ulid) -> Result<Ticket, StoreError> {
        self.read_ticket_at(&self.ticket_path(id))
    }

    /// Read and parse a ticket file at an explicit path.
    pub fn read_ticket_at(&self, path: &Path) -> Result<Ticket, StoreError> {
        let text = fs::read_to_string(path)?;
        let mut ticket = parse_file(&text).map_err(|source| StoreError::Parse {
            path: path.to_path_buf(),
            source,
        })?;
        self.add_legacy_attachment_metadata(&mut ticket)?;
        Ok(ticket)
    }

    /// The attachments directory for a ticket: `attachments/<ULID>/`.
    pub fn attachment_dir(&self, id: &Ulid) -> PathBuf {
        self.root.join("attachments").join(id.to_string())
    }

    /// Deterministic identity for a pre-metadata attachment.
    pub fn legacy_attachment_id(ticket_id: &Ulid, filename: &str) -> Ulid {
        let mut hash = Sha256::new();
        hash.update(ticket_id.to_string().as_bytes());
        hash.update([0]);
        hash.update(filename.as_bytes());
        let bytes: [u8; 16] = hash.finalize()[..16].try_into().expect("sha prefix");
        Ulid::from(u128::from_be_bytes(bytes))
    }

    /// Add an attachment payload and its durable metadata.
    pub fn write_attachment(
        &self,
        ticket_id: &Ulid,
        attachment_id: Ulid,
        created_at: Timestamp,
        filename: &str,
        bytes: &[u8],
    ) -> Result<(Ticket, PathBuf), StoreError> {
        let mut ticket = self.read_ticket(ticket_id)?;
        let name = Path::new(filename)
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|n| !n.is_empty())
            .unwrap_or("attachment");
        if let Some(existing) = ticket
            .attachments
            .iter()
            .find(|item| item.id == attachment_id)
        {
            if existing.filename != name || existing.created_at != created_at {
                return Err(StoreError::Io(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    format!("attachment id {attachment_id} has different metadata"),
                )));
            }
        }
        let dir = self
            .attachment_dir(ticket_id)
            .join(attachment_id.to_string());
        fs::create_dir_all(&dir)?;
        let path = dir.join(name);
        fs::write(&path, bytes)?;
        if !ticket
            .attachments
            .iter()
            .any(|item| item.id == attachment_id)
        {
            ticket.attachments.push(Attachment {
                id: attachment_id,
                filename: name.to_string(),
                created_at: created_at.clone(),
            });
            ticket.attachments.sort_by(|a, b| {
                a.created_at
                    .chronological_cmp(&b.created_at)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then(a.id.cmp(&b.id))
            });
        }
        ticket.updated_at = created_at;
        self.write_ticket_committing(&ticket)?;
        Ok((ticket, path))
    }

    pub fn rename_attachment(
        &self,
        ticket_id: &Ulid,
        attachment_id: &Ulid,
        now: Timestamp,
        filename: &str,
    ) -> Result<Ticket, StoreError> {
        let mut ticket = self.read_ticket(ticket_id)?;
        let attachment = ticket
            .attachments
            .iter_mut()
            .find(|item| &item.id == attachment_id)
            .ok_or_else(|| {
                StoreError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("attachment {attachment_id}"),
                ))
            })?;
        let name = Path::new(filename)
            .file_name()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty())
            .unwrap_or("attachment");
        let dir = self
            .attachment_dir(ticket_id)
            .join(attachment_id.to_string());
        fs::create_dir_all(&dir)?;
        let nested_source = dir.join(&attachment.filename);
        let source = if nested_source.is_file() {
            nested_source
        } else {
            self.attachment_dir(ticket_id).join(&attachment.filename)
        };
        fs::rename(source, dir.join(name))?;
        attachment.filename = name.into();
        ticket.updated_at = now;
        self.write_ticket_committing(&ticket)?;
        Ok(ticket)
    }

    /// Read an attachment payload using its durable ticket-scoped identity.
    pub fn read_attachment(
        &self,
        ticket_id: &Ulid,
        attachment_id: &Ulid,
    ) -> Result<(Attachment, Vec<u8>), StoreError> {
        let ticket = self.read_ticket(ticket_id)?;
        let attachment = ticket
            .attachments
            .iter()
            .find(|item| &item.id == attachment_id)
            .cloned()
            .ok_or_else(|| {
                StoreError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("attachment {attachment_id}"),
                ))
            })?;
        let nested = self
            .attachment_dir(ticket_id)
            .join(attachment_id.to_string())
            .join(&attachment.filename);
        let legacy = self.attachment_dir(ticket_id).join(&attachment.filename);
        let bytes = fs::read(if nested.is_file() { nested } else { legacy })?;
        Ok((attachment, bytes))
    }

    /// Remove attachment metadata and its payload, committing the ticket update.
    pub fn remove_attachment(
        &self,
        ticket_id: &Ulid,
        attachment_id: &Ulid,
        now: Timestamp,
    ) -> Result<Ticket, StoreError> {
        let mut ticket = self.read_ticket(ticket_id)?;
        let index = ticket
            .attachments
            .iter()
            .position(|item| &item.id == attachment_id)
            .ok_or_else(|| {
                StoreError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("attachment {attachment_id}"),
                ))
            })?;
        let attachment = ticket.attachments.remove(index);
        let nested = self
            .attachment_dir(ticket_id)
            .join(attachment_id.to_string());
        if nested.exists() {
            fs::remove_dir_all(nested)?;
        } else {
            let legacy = self.attachment_dir(ticket_id).join(attachment.filename);
            if legacy.exists() {
                fs::remove_file(legacy)?;
            }
        }
        ticket.updated_at = now;
        self.write_ticket_committing(&ticket)?;
        Ok(ticket)
    }

    fn add_legacy_attachment_metadata(&self, ticket: &mut Ticket) -> Result<(), StoreError> {
        let dir = self.attachment_dir(&ticket.id);
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        for entry in entries {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let filename = entry.file_name().to_string_lossy().to_string();
            let id = Self::legacy_attachment_id(&ticket.id, &filename);
            if !ticket.attachments.iter().any(|item| item.id == id) {
                ticket.attachments.push(Attachment {
                    id,
                    filename,
                    created_at: ticket.created_at.clone(),
                });
            }
        }
        ticket.attachments.sort_by_key(|item| item.id);
        Ok(())
    }

    /// Every `tickets/<shard>/<ULID>.md` file path in the store, unsorted. A
    /// directory-level I/O error (e.g. an unreadable shard) is fatal; per-file
    /// parse errors are the concern of the caller, not this walk.
    fn ticket_file_paths(&self) -> Result<Vec<PathBuf>, StoreError> {
        let mut paths = Vec::new();
        let tickets_dir = self.root.join("tickets");
        if !tickets_dir.is_dir() {
            return Ok(paths);
        }
        for shard in fs::read_dir(&tickets_dir)? {
            let shard = shard?;
            if !shard.file_type()?.is_dir() {
                continue;
            }
            for entry in fs::read_dir(shard.path())? {
                let path = entry?.path();
                if path.extension().and_then(|e| e.to_str()) == Some("md") {
                    paths.push(path);
                }
            }
        }
        Ok(paths)
    }

    /// Read every ticket in the store, sorted by id (≈ creation order). **Strict:**
    /// the first unparseable file aborts the whole enumeration with its
    /// [`StoreError::Parse`]. Callers that must survive a corrupt file (project open,
    /// the server ticket list) use [`FsStore::list_tickets_resilient`] instead.
    pub fn list_tickets(&self) -> Result<Vec<Ticket>, StoreError> {
        let mut out = Vec::new();
        for path in self.ticket_file_paths()? {
            out.push(self.read_ticket_at(&path)?);
        }
        out.sort_by_key(|t| t.id);
        Ok(out)
    }

    /// Enumerate the store **resiliently**: return every healthy ticket AND a separate
    /// report of the files that failed to parse, rather than letting one bad file abort
    /// the whole scan (HS2-PRVPCQ). A single unparseable `.md` — for example a ticket
    /// whose notes block is missing its `<!-- hotsheet:notes:end -->` marker — must never
    /// prevent the rest of a project from loading. Directory-level I/O errors (an
    /// unreadable `tickets/` tree) are still fatal; a per-file read or parse failure is
    /// captured as a [`CorruptTicket`] with its recoverable id/slug and error message.
    pub fn list_tickets_resilient(&self) -> Result<StoreListing, StoreError> {
        let mut tickets = Vec::new();
        let mut corrupt = Vec::new();
        for path in self.ticket_file_paths()? {
            match self.read_ticket_at(&path) {
                Ok(ticket) => tickets.push(ticket),
                Err(error) => {
                    let (id, slug) = recover_identity(&path);
                    corrupt.push(CorruptTicket {
                        path,
                        id,
                        slug,
                        error: error.to_string(),
                    });
                }
            }
        }
        tickets.sort_by_key(|t| t.id);
        corrupt.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(StoreListing { tickets, corrupt })
    }

    // ---- git-diff fast path (docs/03 §3.4, HS2-90) --------------------------------

    /// The current `HEAD` commit id, or `None` if the store isn't a git repo yet or has
    /// no commits. Used to detect a HEAD move (commit/pull/checkout) for the incremental
    /// reindex fast path.
    pub fn head_commit(&self) -> Option<String> {
        git_stdout(&self.root, &["rev-parse", "HEAD"]).map(|s| s.trim().to_string())
    }

    /// Whether the working tree + index are clean (no uncommitted changes). The fast path
    /// only fires on a *pure* HEAD move; any local edits fall back to the full hash-walk.
    pub fn is_working_tree_clean(&self) -> bool {
        git_stdout(&self.root, &["status", "--porcelain"])
            .map(|s| s.trim().is_empty())
            .unwrap_or(false)
    }

    /// The ticket ULIDs whose files changed between two commits (`git diff --name-only
    /// old new -- tickets/`), parsed from the `tickets/NN/<ULID>.md` paths. Renames are
    /// broken into a delete + add (`--no-renames`) so both endpoints reconcile. A path
    /// that no longer parses as a ULID is skipped.
    pub fn changed_ticket_ids_between(
        &self,
        old: &str,
        new: &str,
    ) -> Result<Vec<Ulid>, StoreError> {
        let out = git_stdout(
            &self.root,
            &[
                "diff",
                "--name-only",
                "--no-renames",
                old,
                new,
                "--",
                "tickets",
            ],
        )
        .ok_or_else(|| StoreError::Git(format!("`git diff {old} {new}` failed")))?;

        let mut ids = Vec::new();
        for line in out.lines() {
            if let Some(id) = Path::new(line.trim())
                .file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| Ulid::from_string(s).ok())
            {
                ids.push(id);
            }
        }
        ids.sort();
        ids.dedup();
        Ok(ids)
    }
}

// ---- resilient-enumeration helpers -------------------------------------------------

/// Best-effort recovery of a failed file's identity for a [`CorruptTicket`]. The id
/// comes from the filename stem (the canonical ULID); if the stem isn't a ULID we fall
/// back to a frontmatter `id:` line. The slug comes from a frontmatter `slug:` line.
/// Everything here degrades to `None` rather than erroring — the file is already known
/// bad, so this only enriches the report.
fn recover_identity(path: &Path) -> (Option<Ulid>, Option<String>) {
    let stem_id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .and_then(|s| Ulid::from_string(s).ok());
    let text = fs::read_to_string(path).ok();
    let slug = text.as_deref().and_then(|t| frontmatter_value(t, "slug"));
    let id = stem_id.or_else(|| {
        text.as_deref()
            .and_then(|t| frontmatter_value(t, "id"))
            .and_then(|v| Ulid::from_string(&v).ok())
    });
    (id, slug)
}

/// Pull a top-level scalar `key: value` from a file's leading `---` frontmatter block,
/// tolerating a malformed body below it. Returns the trimmed, unquoted value, or `None`
/// when there is no readable frontmatter or the key is absent/empty. Only exact,
/// unindented top-level keys match (a nested/indented key is ignored).
fn frontmatter_value(text: &str, key: &str) -> Option<String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let normalized = text.replace("\r\n", "\n");
    let rest = normalized.strip_prefix("---\n")?;
    let frontmatter = rest.split_once("\n---").map_or(rest, |(fm, _)| fm);
    for line in frontmatter.lines() {
        let Some(after) = line.strip_prefix(key).and_then(|a| a.strip_prefix(':')) else {
            continue;
        };
        let value = after.trim().trim_matches(['"', '\'']).trim();
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
}

// ---- git helpers (shell-based; the store IS a git repo, docs/02 §2.3) --------------

/// Run `git -C root <args>`, erroring on a non-zero exit.
fn git(root: &Path, args: &[&str]) -> Result<(), StoreError> {
    let status = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(StoreError::Git(format!("`git {}` failed", args.join(" "))))
    }
}

/// True when `git -C root <args>` exits 0 (used for `diff --cached --quiet`).
fn git_ok(root: &Path, args: &[&str]) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Captured stdout of `git -C root <args>`, or `None` if it failed to run.
fn git_stdout(root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hotsheet_model::derive_slug;

    fn ulid(s: &str) -> Ulid {
        Ulid::from_string(s).unwrap()
    }

    fn temp_store() -> (tempfile::TempDir, FsStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
        (dir, store)
    }

    fn sample(id: Ulid) -> Ticket {
        Ticket::new(
            id,
            derive_slug(&id, "HS"),
            "A ticket",
            "bug",
            "2026-08-19T00:00:00Z",
            "2026-08-19T00:00:00Z",
        )
    }

    #[test]
    fn init_open_and_metadata_round_trip() {
        let (dir, store) = temp_store();
        assert_eq!(store.metadata().unwrap(), StoreMetadata::new("HS"));
        // A second open of the same dir succeeds.
        assert!(FsStore::open(dir.path()).is_ok());
    }

    #[test]
    fn write_then_read_round_trips_through_the_file() {
        let (_dir, store) = temp_store();
        let t = sample(ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV"));
        let path = store.write_ticket(&t).unwrap();
        assert!(path.ends_with("tickets/01/01ARZ3NDEKTSV4RRFFQ69G5FAV.md"));
        assert_eq!(store.read_ticket(&t.id).unwrap(), t);
    }

    #[test]
    fn write_normalizes_up_next_off_inactive_statuses() {
        let (_dir, store) = temp_store();
        let mut ticket = sample(ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV"));
        ticket.status = hotsheet_model::Status::Archive;
        ticket.up_next = true;
        store.write_ticket(&ticket).unwrap();
        assert!(!store.read_ticket(&ticket.id).unwrap().up_next);
    }

    #[test]
    fn list_returns_all_tickets_sorted_and_ignores_non_md() {
        let (_dir, store) = temp_store();
        let a = sample(ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV"));
        let b = sample(ulid("7ZARZ3NDEKTSV4RRFFQ69G5FAV"));
        store.write_ticket(&b).unwrap();
        store.write_ticket(&a).unwrap();
        // A stray non-ticket file must be ignored.
        fs::write(store.root().join("tickets/01/README.txt"), "ignore me").unwrap();

        let listed = store.list_tickets().unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].id, a.id, "sorted by id (k-sortable)");
        assert_eq!(listed[1].id, b.id);
    }

    /// A malformed ticket file in the exact field-failure shape: a bounded body followed
    /// by a `notes:begin` marker that is never closed by `notes:end` (parses to a hard
    /// `ParseError::TrailingContent`).
    fn missing_notes_end(id: &Ulid, slug: &str) -> String {
        format!(
            "---\nid: {id}\nslug: {slug}\ntitle: broken\ncategory: bug\n\
             created_at: 2026-08-19T00:00:00Z\nupdated_at: 2026-08-19T00:00:00Z\nschema: 1\n---\n\n\
             <!-- hotsheet:body:begin -->\nbody\n<!-- hotsheet:body:end -->\n\n\
             <!-- hotsheet:notes:begin -->\n## Notes\n\nunterminated note\n"
        )
    }

    /// Write raw bytes straight to a ticket's on-disk path (bypassing the serializer), so
    /// tests can plant a corrupt file the writer would never produce.
    fn plant_raw(store: &FsStore, id: &Ulid, content: &str) -> PathBuf {
        let path = store.ticket_path(id);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn list_tickets_resilient_returns_healthy_and_reports_the_corrupt_file() {
        let (_dir, store) = temp_store();
        let a = sample(ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV"));
        let b = sample(ulid("7ZARZ3NDEKTSV4RRFFQ69G5FAV"));
        store.write_ticket(&a).unwrap();
        store.write_ticket(&b).unwrap();

        // The exact field-failure shape, written straight to disk.
        let bad_id = ulid("3ZARZ3NDEKTSV4RRFFQ69G5FAV");
        let bad_path = plant_raw(&store, &bad_id, &missing_notes_end(&bad_id, "HS-BROKEN"));

        // Strict enumeration still hard-fails on the bad file (documents why the resilient
        // variant exists) — this is the bug that hid the whole store.
        assert!(matches!(
            store.list_tickets(),
            Err(StoreError::Parse { .. })
        ));

        // Resilient enumeration returns every healthy ticket AND reports exactly the one
        // corrupt file, with its path, recovered id/slug, and a non-empty error.
        let listing = store.list_tickets_resilient().unwrap();
        assert_eq!(
            listing.tickets.iter().map(|t| t.id).collect::<Vec<_>>(),
            vec![a.id, b.id],
            "all healthy tickets, sorted by id"
        );
        assert_eq!(listing.corrupt.len(), 1);
        let corrupt = &listing.corrupt[0];
        assert_eq!(corrupt.path, bad_path);
        assert_eq!(corrupt.id, Some(bad_id), "id recovered from the filename");
        assert_eq!(
            corrupt.slug.as_deref(),
            Some("HS-BROKEN"),
            "slug from frontmatter"
        );
        assert!(
            !corrupt.error.is_empty(),
            "a human-readable error is reported"
        );
    }

    #[test]
    fn list_tickets_resilient_survives_multiple_and_varied_bad_files() {
        // Adversarial: several corrupt files of different failure modes, a stray non-.md
        // file next to a corrupt one, an empty file, and truncated frontmatter. Healthy
        // tickets still enumerate; every bad .md is reported exactly once; the non-.md
        // file is ignored, not reported.
        let (_dir, store) = temp_store();
        store
            .write_ticket(&sample(ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV")))
            .unwrap();
        store
            .write_ticket(&sample(ulid("7ZARZ3NDEKTSV4RRFFQ69G5FAV")))
            .unwrap();

        let missing_end_id = ulid("2ZARZ3NDEKTSV4RRFFQ69G5FAV");
        let missing_end = plant_raw(
            &store,
            &missing_end_id,
            &missing_notes_end(&missing_end_id, "HS-ONE"),
        );
        let empty_id = ulid("3ZARZ3NDEKTSV4RRFFQ69G5FAV");
        let empty = plant_raw(&store, &empty_id, "");
        let truncated_id = ulid("4ZARZ3NDEKTSV4RRFFQ69G5FAV");
        let truncated = plant_raw(
            &store,
            &truncated_id,
            &format!("---\nid: {truncated_id}\ntitle: t\nno closing fence\n"),
        );
        // A stray non-ticket file beside a corrupt one must be ignored, not reported.
        fs::write(store.root().join("tickets/3Z/notes.txt"), "just some notes").unwrap();

        let listing = store.list_tickets_resilient().unwrap();
        assert_eq!(listing.tickets.len(), 2, "both healthy tickets enumerate");

        let paths: Vec<PathBuf> = listing.corrupt.iter().map(|c| c.path.clone()).collect();
        assert_eq!(
            paths,
            {
                let mut want = vec![missing_end, empty, truncated];
                want.sort();
                want
            },
            "each bad .md reported exactly once, sorted by path; non-.md ignored"
        );
        for corrupt in &listing.corrupt {
            assert!(!corrupt.error.is_empty(), "{corrupt:?} has an error");
            assert!(
                corrupt.id.is_some(),
                "{corrupt:?} recovers its id from the filename"
            );
        }
    }

    #[test]
    fn list_tickets_resilient_is_clean_for_a_healthy_store() {
        let (_dir, store) = temp_store();
        store
            .write_ticket(&sample(ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV")))
            .unwrap();
        let listing = store.list_tickets_resilient().unwrap();
        assert_eq!(listing.tickets.len(), 1);
        assert!(listing.corrupt.is_empty(), "no corrupt files reported");
    }

    #[test]
    fn write_attachment_stores_under_the_ticket_and_strips_paths() {
        let (_dir, store) = temp_store();
        let id = ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV");
        store
            .write_ticket(&Ticket::new(id, "HS-TEST", "test", "task", "t0", "t0"))
            .unwrap();
        let attachment_id = ulid("01ARZ3NDEKTSV4RRFFQ69G5FB0");
        // A traversal-y filename is reduced to its basename.
        let (ticket, path) = store
            .write_attachment(
                &id,
                attachment_id,
                Timestamp::new("2026-08-26T00:00:00Z"),
                "../../evil/shot.png",
                b"PNGDATA",
            )
            .unwrap();
        assert!(path.ends_with(
            "attachments/01ARZ3NDEKTSV4RRFFQ69G5FAV/01ARZ3NDEKTSV4RRFFQ69G5FB0/shot.png"
        ));
        assert_eq!(fs::read(&path).unwrap(), b"PNGDATA");
        assert_eq!(ticket.attachments[0].id, attachment_id);
        assert_eq!(ticket.attachments[0].filename, "shot.png");
        assert_eq!(
            ticket.attachments[0].created_at.as_str(),
            "2026-08-26T00:00:00Z"
        );
        let renamed = store
            .rename_attachment(
                &id,
                &attachment_id,
                Timestamp::new("2026-08-26T01:00:00Z"),
                "renamed.png",
            )
            .unwrap();
        assert_eq!(renamed.attachments[0].id, attachment_id);
        assert_eq!(renamed.attachments[0].filename, "renamed.png");
        assert_eq!(
            renamed.attachments[0].created_at,
            ticket.attachments[0].created_at
        );
        assert!(path.with_file_name("renamed.png").is_file());
        let (metadata, bytes) = store.read_attachment(&id, &attachment_id).unwrap();
        assert_eq!(metadata.filename, "renamed.png");
        assert_eq!(bytes, b"PNGDATA");
        let removed = store
            .remove_attachment(&id, &attachment_id, Timestamp::new("2026-08-26T02:00:00Z"))
            .unwrap();
        assert!(removed.attachments.is_empty());
        assert!(
            !store
                .attachment_dir(&id)
                .join(attachment_id.to_string())
                .exists()
        );
    }

    #[test]
    fn legacy_attachment_gets_deterministic_metadata_without_using_mtime() {
        let (_dir, store) = temp_store();
        let ticket = sample(ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV"));
        store.write_ticket(&ticket).unwrap();
        let dir = store.attachment_dir(&ticket.id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("legacy.txt"), b"old").unwrap();

        let first = store.read_ticket(&ticket.id).unwrap();
        let second = store.read_ticket(&ticket.id).unwrap();
        assert_eq!(first.attachments, second.attachments);
        assert_eq!(first.attachments[0].filename, "legacy.txt");
        assert_eq!(first.attachments[0].created_at, ticket.created_at);
        assert_eq!(
            first.attachments[0].id,
            FsStore::legacy_attachment_id(&ticket.id, "legacy.txt")
        );
    }

    #[test]
    fn open_on_a_bare_directory_reports_not_a_store() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            FsStore::open(dir.path()),
            Err(StoreError::NotAStore(_))
        ));
    }

    #[test]
    fn autocommit_is_a_noop_without_a_git_repo() {
        let (_dir, store) = temp_store();
        // A store that isn't a git repo (the common test/temp case) never fails and
        // never commits — so ops in a bare dir just work.
        assert!(!store.autocommit("nope").unwrap());
        assert!(
            store
                .write_ticket_committing(&sample(ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV")))
                .is_ok()
        );
    }

    #[test]
    fn committing_write_commits_once_and_leaves_a_clean_tree() {
        let (dir, store) = temp_store();
        // Make it a git repo, as `hotsheet init` does. No user config → autocommit's
        // bot-identity fallback still lets commits land (mirrors a fresh CI checkout).
        git(dir.path(), &["init", "-q"]).unwrap();

        let t = sample(ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV"));
        store.write_ticket_committing(&t).unwrap();

        // Working tree is clean (nothing left uncommitted) after the mutation.
        assert!(
            git_ok(dir.path(), &["diff", "--quiet"])
                && git_ok(dir.path(), &["diff", "--cached", "--quiet"]),
            "the store should be clean after a committing write"
        );
        let count = git_stdout(dir.path(), &["rev-list", "--count", "HEAD"]).unwrap();
        assert_eq!(count.trim(), "1", "one commit for the mutation");

        // Re-writing identical content stages nothing → no empty commit.
        store.write_ticket_committing(&t).unwrap();
        let count = git_stdout(dir.path(), &["rev-list", "--count", "HEAD"]).unwrap();
        assert_eq!(
            count.trim(),
            "1",
            "an unchanged re-write must not add a commit"
        );
    }

    #[cfg(unix)]
    #[test]
    fn committing_write_does_not_wait_for_a_slow_remote_push() {
        use std::os::unix::fs::PermissionsExt;
        use std::time::{Duration, Instant};

        let (dir, store) = temp_store();
        let remote = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q"]).unwrap();
        git(dir.path(), &["config", "user.name", "Hot Sheet Test"]).unwrap();
        git(dir.path(), &["config", "user.email", "test@localhost"]).unwrap();
        git(dir.path(), &["add", "-A"]).unwrap();
        git(dir.path(), &["commit", "-q", "-m", "initial"]).unwrap();
        git(remote.path(), &["init", "--bare", "-q"]).unwrap();
        git(
            dir.path(),
            &["remote", "add", "origin", remote.path().to_str().unwrap()],
        )
        .unwrap();
        git(dir.path(), &["push", "-q", "-u", "origin", "HEAD"]).unwrap();

        let hook = remote.path().join("hooks/pre-receive");
        fs::write(&hook, "#!/bin/sh\nsleep 1\n").unwrap();
        let mut permissions = fs::metadata(&hook).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&hook, permissions).unwrap();

        let started = Instant::now();
        store
            .write_ticket_committing(&sample(ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV")))
            .unwrap();
        assert!(
            started.elapsed() < Duration::from_millis(750),
            "a local mutation waited for remote publication"
        );

        let local_head = git_stdout(dir.path(), &["rev-parse", "HEAD"]).unwrap();
        let deadline = Instant::now() + Duration::from_secs(4);
        loop {
            let remote_head = git_stdout(remote.path(), &["rev-parse", "HEAD"]);
            if remote_head.as_deref().map(str::trim) == Some(local_head.trim()) {
                break;
            }
            assert!(Instant::now() < deadline, "background push never published");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}
