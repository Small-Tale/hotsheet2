//! **Local overlay storage** (`docs/02` §2.11, HS2-21) — the per-user / per-machine data
//! that must NOT be committed into a shared ticket file (my read state is not your read
//! state), yet must still be **durable on disk** so it survives an index rebuild (the
//! cardinal principle: everything reconstructs from disk; the index is only a cache).
//!
//! Tier B data lives in **gitignored** files under `<store>/local/`, keyed by ticket ULID.
//! Writing the overlay adds `local/` to the store's `.gitignore` so it is never committed,
//! while the shared ticket files stay committed. The first concrete consumer is **read
//! tracking** (`local/reads.json`: ULID → last-read timestamp); feedback drafts and UI/view
//! state slot into the same mechanism next (HS2-AWTHJE).

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use hotsheet_model::{Ticket, Timestamp, Ulid};

/// Per-store local overlay: reads/writes the gitignored `<store>/local/` tree.
pub struct LocalOverlay {
    store_root: PathBuf,
    local_dir: PathBuf,
}

impl LocalOverlay {
    /// Open the overlay for the store rooted at `store_root` (no I/O until first use).
    pub fn new(store_root: impl Into<PathBuf>) -> Self {
        let store_root = store_root.into();
        let local_dir = store_root.join("local");
        Self {
            store_root,
            local_dir,
        }
    }

    /// The gitignored local directory (`<store>/local`).
    pub fn dir(&self) -> &Path {
        &self.local_dir
    }

    // ---- read tracking (per-user unread state) -----------------------------------

    /// Record that the ticket was read at `at` (per-user; local only).
    pub fn mark_read(&self, id: &Ulid, at: &Timestamp) -> io::Result<()> {
        let mut reads = self.load_reads()?;
        reads.insert(id.to_string(), at.as_str().to_string());
        self.save_reads(&reads)
    }

    /// When the ticket was last read on this machine, if ever.
    pub fn last_read(&self, id: &Ulid) -> io::Result<Option<Timestamp>> {
        Ok(self
            .load_reads()?
            .get(&id.to_string())
            .map(|s| Timestamp::from(s.as_str())))
    }

    /// Whether the ticket is unread: never read, or changed since it was last read
    /// (`updated_at` is newer than the recorded read time).
    pub fn is_unread(&self, ticket: &Ticket) -> io::Result<bool> {
        match self.last_read(&ticket.id)? {
            None => Ok(true),
            Some(read_at) => Ok(ticket.updated_at.as_str() > read_at.as_str()),
        }
    }

    /// Mark every given ticket read as of `now` (e.g. a "mark all read").
    pub fn mark_all_read(&self, tickets: &[Ticket], now: &Timestamp) -> io::Result<()> {
        let mut reads = self.load_reads()?;
        for t in tickets {
            reads.insert(t.id.to_string(), now.as_str().to_string());
        }
        self.save_reads(&reads)
    }

    // ---- storage plumbing --------------------------------------------------------

    fn reads_path(&self) -> PathBuf {
        self.local_dir.join("reads.json")
    }

    fn load_reads(&self) -> io::Result<BTreeMap<String, String>> {
        match std::fs::read_to_string(self.reads_path()) {
            Ok(s) => serde_json::from_str(&s).map_err(io::Error::other),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(BTreeMap::new()),
            Err(e) => Err(e),
        }
    }

    fn save_reads(&self, reads: &BTreeMap<String, String>) -> io::Result<()> {
        std::fs::create_dir_all(&self.local_dir)?;
        self.ensure_gitignored()?;
        let json = serde_json::to_string_pretty(reads).map_err(io::Error::other)?;
        std::fs::write(self.reads_path(), json + "\n")
    }

    /// Ensure `local/` is in the store's `.gitignore`, so the overlay is never committed.
    fn ensure_gitignored(&self) -> io::Result<()> {
        let entry = "local/";
        let gi = self.store_root.join(".gitignore");
        let existing = std::fs::read_to_string(&gi).unwrap_or_default();
        if existing.lines().any(|l| l.trim() == entry) {
            return Ok(());
        }
        let mut content = existing;
        if !content.is_empty() && !content.ends_with('\n') {
            content.push('\n');
        }
        content.push_str("# Per-user local overlay — never committed (HS2-21)\n");
        content.push_str(entry);
        content.push('\n');
        std::fs::write(&gi, content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ticket(id: &str, updated: &str) -> Ticket {
        Ticket::new(
            Ulid::from_string(id).unwrap(),
            "HS-1",
            "t",
            "bug",
            Timestamp::from("2026-01-01T00:00:00Z"),
            Timestamp::from(updated),
        )
    }

    #[test]
    fn read_tracking_is_per_ticket_and_time_aware() {
        let dir = tempfile::tempdir().unwrap();
        let ov = LocalOverlay::new(dir.path());
        let t = ticket("01ARZ3NDEKTSV4RRFFQ69G5FAV", "2026-01-02T00:00:00Z");

        // Never read → unread.
        assert!(ov.is_unread(&t).unwrap());
        // Read at/after its updated_at → read.
        ov.mark_read(&t.id, &Timestamp::from("2026-01-02T00:00:00Z"))
            .unwrap();
        assert!(!ov.is_unread(&t).unwrap());
        // A later edit makes it unread again.
        let mut t2 = t.clone();
        t2.updated_at = Timestamp::from("2026-01-03T00:00:00Z");
        assert!(ov.is_unread(&t2).unwrap());
    }

    #[test]
    fn overlay_writes_are_gitignored_and_under_local() {
        let dir = tempfile::tempdir().unwrap();
        let ov = LocalOverlay::new(dir.path());
        let id = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
        ov.mark_read(&id, &Timestamp::from("2026-01-02T00:00:00Z"))
            .unwrap();

        // The data landed under <store>/local/, and local/ is gitignored.
        assert!(dir.path().join("local/reads.json").is_file());
        let gi = std::fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        assert!(
            gi.lines().any(|l| l.trim() == "local/"),
            ".gitignore has local/: {gi}"
        );
    }

    #[test]
    fn reads_persist_across_overlay_instances() {
        let dir = tempfile::tempdir().unwrap();
        let id = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
        LocalOverlay::new(dir.path())
            .mark_read(&id, &Timestamp::from("2026-01-02T00:00:00Z"))
            .unwrap();
        // A fresh overlay reads it back from disk (durable, not in-memory).
        let again = LocalOverlay::new(dir.path()).last_read(&id).unwrap();
        assert_eq!(again.unwrap().as_str(), "2026-01-02T00:00:00Z");
    }
}
