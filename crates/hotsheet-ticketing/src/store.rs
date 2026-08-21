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
use std::process::Command;

use hotsheet_model::{ParseError, Ticket, Ulid, parse_file, to_file_string};
use serde::{Deserialize, Serialize};

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

/// A filesystem-backed store rooted at a directory.
#[derive(Debug, Clone)]
pub struct FsStore {
    root: PathBuf,
}

impl FsStore {
    /// Initialize a new store at `root` (creates `tickets/` and writes metadata).
    /// Idempotent on the directory; overwrites metadata with the given values.
    pub fn init(root: impl Into<PathBuf>, meta: &StoreMetadata) -> Result<Self, StoreError> {
        let root = root.into();
        fs::create_dir_all(root.join("tickets"))?;
        let json = serde_json::to_string_pretty(meta)?;
        fs::write(root.join(STORE_METADATA_FILE), format!("{json}\n"))?;
        Ok(Self { root })
    }

    /// Open an existing store, erroring if `root` is not a Hot Sheet store.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let root = root.into();
        if !root.join(STORE_METADATA_FILE).is_file() {
            return Err(StoreError::NotAStore(root));
        }
        Ok(Self { root })
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
        let path = self.ticket_path(&ticket.id);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, to_file_string(ticket))?;
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

    /// Stage everything, commit with `message`, and best-effort push. No-op when the
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
        // Push is best-effort: offline / no remote / rejected non-fast-forward are fine
        // here (the aggressive fetch/rebase/push engine is HS2-19).
        if git_stdout(&self.root, &["remote"]).is_some_and(|s| !s.trim().is_empty()) {
            let _ = git(&self.root, &["push", "--quiet"]);
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
        parse_file(&text).map_err(|source| StoreError::Parse {
            path: path.to_path_buf(),
            source,
        })
    }

    /// The attachments directory for a ticket: `attachments/<ULID>/`.
    pub fn attachment_dir(&self, id: &Ulid) -> PathBuf {
        self.root.join("attachments").join(id.to_string())
    }

    /// Write an attachment file under `attachments/<ULID>/<filename>` (the filename is
    /// reduced to its basename to prevent path traversal). Returns the written path.
    pub fn write_attachment(
        &self,
        id: &Ulid,
        filename: &str,
        bytes: &[u8],
    ) -> Result<PathBuf, StoreError> {
        let name = Path::new(filename)
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|n| !n.is_empty())
            .unwrap_or("attachment");
        let dir = self.attachment_dir(id);
        fs::create_dir_all(&dir)?;
        let path = dir.join(name);
        fs::write(&path, bytes)?;
        Ok(path)
    }

    /// Read every ticket in the store, sorted by id (≈ creation order).
    pub fn list_tickets(&self) -> Result<Vec<Ticket>, StoreError> {
        let mut out = Vec::new();
        let tickets_dir = self.root.join("tickets");
        if !tickets_dir.is_dir() {
            return Ok(out);
        }
        for shard in fs::read_dir(&tickets_dir)? {
            let shard = shard?;
            if !shard.file_type()?.is_dir() {
                continue;
            }
            for entry in fs::read_dir(shard.path())? {
                let path = entry?.path();
                if path.extension().and_then(|e| e.to_str()) == Some("md") {
                    out.push(self.read_ticket_at(&path)?);
                }
            }
        }
        out.sort_by_key(|t| t.id);
        Ok(out)
    }
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

    #[test]
    fn write_attachment_stores_under_the_ticket_and_strips_paths() {
        let (_dir, store) = temp_store();
        let id = ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV");
        // A traversal-y filename is reduced to its basename.
        let path = store
            .write_attachment(&id, "../../evil/shot.png", b"PNGDATA")
            .unwrap();
        assert!(path.ends_with("attachments/01ARZ3NDEKTSV4RRFFQ69G5FAV/shot.png"));
        assert_eq!(fs::read(&path).unwrap(), b"PNGDATA");
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
}
