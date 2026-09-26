//! A stat-validated cache of which ticket files are corrupt (HS2-KYSBT2).
//!
//! [`FsStore::list_tickets_resilient`] parses every ticket file, which costs about half a
//! second on a large store. Callers that only need the *corrupt* files (the checkout
//! `corrupt-tickets` listing a client waits on before it paints ticket rows) can instead
//! keep a [`CorruptTicketCache`]: each scan lists the ticket tree and `stat`s every file,
//! and it re-reads and re-parses only files whose fingerprint changed since the previous
//! scan.
//!
//! The cache validates against the filesystem on every scan instead of trusting a file
//! watcher, so it is never staler than the disk. That includes edits made while the
//! server was down, `git pull`s, and events a watcher missed. A fingerprint covers the
//! ticket file (size, modification time, and on Unix the inode and change time) and the
//! ticket's attachment directory, because the resilient read also enumerates legacy
//! attachments there. Like git's racy-index rule, an entry whose file or attachment
//! directory changed within [`RACY_WINDOW`] of the observation is not trusted and is
//! re-read on the next scan, so a same-size rewrite inside one timestamp tick is not
//! missed. Only outcomes that are a function of the file's bytes are cached: a healthy
//! parse or a parse error. Any other read failure (I/O, permissions, a directory named
//! like a ticket) is re-evaluated on every scan.

use crate::store::{CorruptTicket, FsStore, StoreError};
use hotsheet_model::Ulid;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// A file modified this recently before it was observed may change again within the same
/// timestamp tick, so its cached outcome is not trusted on the next scan.
pub const RACY_WINDOW: Duration = Duration::from_secs(2);

/// The `stat` facts that change whenever a file's contents are replaced.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FileStamp {
    len: u64,
    modified: Option<SystemTime>,
    #[cfg(unix)]
    inode: u64,
    #[cfg(unix)]
    changed: (i64, i64),
}

impl FileStamp {
    fn of(metadata: &std::fs::Metadata) -> Self {
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Self {
            len: metadata.len(),
            modified: metadata.modified().ok(),
            #[cfg(unix)]
            inode: metadata.ino(),
            #[cfg(unix)]
            changed: (metadata.ctime(), metadata.ctime_nsec()),
        }
    }

    /// Whether the stamp is too recent (or too odd) to trust at `observed_at`.
    fn is_racy(&self, observed_at: SystemTime, window: Duration) -> bool {
        match self.modified {
            None => true,
            // A modification time in the future is as untrustworthy as a recent one.
            Some(modified) => observed_at
                .duration_since(modified)
                .map_or(true, |age| age < window),
        }
    }
}

/// Everything a resilient read of one ticket file depends on.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Fingerprint {
    ticket: FileStamp,
    /// The ticket's attachment directory, or `None` when it does not exist.
    attachments: Option<FileStamp>,
}

impl Fingerprint {
    /// `None` when the ticket file cannot be `stat`ed; its read then decides the outcome
    /// and nothing is cached.
    fn of(store: &FsStore, path: &Path) -> Option<Self> {
        let ticket = FileStamp::of(&std::fs::metadata(path).ok()?);
        let attachments = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .and_then(|stem| Ulid::from_string(stem).ok())
            .and_then(|id| std::fs::metadata(store.attachment_dir(&id)).ok())
            .map(|metadata| FileStamp::of(&metadata));
        Some(Self {
            ticket,
            attachments,
        })
    }

    fn is_racy(&self, observed_at: SystemTime, window: Duration) -> bool {
        self.ticket.is_racy(observed_at, window)
            || self
                .attachments
                .as_ref()
                .is_some_and(|stamp| stamp.is_racy(observed_at, window))
    }
}

#[derive(Debug, Clone)]
struct CachedOutcome {
    fingerprint: Fingerprint,
    /// Re-read on the next scan even when the fingerprint still matches.
    racy: bool,
    /// `None` for a healthy ticket.
    corrupt: Option<CorruptTicket>,
}

/// Per-store memo of corrupt ticket files, revalidated by `stat` on every scan. See the
/// module documentation for the invalidation rules.
#[derive(Debug)]
pub struct CorruptTicketCache {
    entries: HashMap<PathBuf, CachedOutcome>,
    racy_window: Duration,
    last_reads: usize,
}

impl Default for CorruptTicketCache {
    fn default() -> Self {
        Self::new()
    }
}

impl CorruptTicketCache {
    /// An empty cache: the first scan reads every ticket file.
    pub fn new() -> Self {
        Self::with_racy_window(RACY_WINDOW)
    }

    /// An empty cache with a custom racy window. Tests use [`Duration::ZERO`] to observe
    /// reuse of files they have just written.
    pub fn with_racy_window(racy_window: Duration) -> Self {
        Self {
            entries: HashMap::new(),
            racy_window,
            last_reads: 0,
        }
    }

    /// How many ticket files the most recent [`Self::corrupt_tickets`] read and parsed
    /// (the rest were answered from the cache).
    pub fn last_reads(&self) -> usize {
        self.last_reads
    }

    /// The store's corrupt ticket files, sorted by path: the same list as
    /// [`FsStore::list_tickets_resilient`]'s `corrupt`, re-reading only files whose
    /// fingerprint changed (or was racy) since the previous scan. Files that disappeared
    /// are dropped. An unreadable `tickets/` tree is still an error.
    pub fn corrupt_tickets(&mut self, store: &FsStore) -> Result<Vec<CorruptTicket>, StoreError> {
        let paths = store.ticket_file_paths()?;
        let mut previous = std::mem::take(&mut self.entries);
        let mut corrupt = Vec::new();
        self.last_reads = 0;
        for path in paths {
            // Observe before reading: a write racing the read leaves a newer fingerprint,
            // so the next scan re-reads the file.
            let observed_at = SystemTime::now();
            let fingerprint = Fingerprint::of(store, &path);
            let cached = previous
                .remove(&path)
                .filter(|cached| !cached.racy && fingerprint.as_ref() == Some(&cached.fingerprint));
            let outcome = match cached {
                Some(cached) => cached,
                None => {
                    self.last_reads += 1;
                    let (cacheable, report) = match store.read_ticket_at(&path) {
                        Ok(_) => (true, None),
                        Err(error) => (
                            matches!(error, StoreError::Parse { .. }),
                            Some(CorruptTicket::from_error(path.clone(), &error)),
                        ),
                    };
                    match fingerprint.filter(|_| cacheable) {
                        Some(fingerprint) => CachedOutcome {
                            racy: fingerprint.is_racy(observed_at, self.racy_window),
                            fingerprint,
                            corrupt: report,
                        },
                        None => {
                            corrupt.extend(report);
                            continue;
                        }
                    }
                }
            };
            corrupt.extend(outcome.corrupt.clone());
            self.entries.insert(path, outcome);
        }
        corrupt.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(corrupt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::StoreMetadata;
    use hotsheet_model::{Ticket, derive_slug};
    use std::fs;

    fn temp_store() -> (tempfile::TempDir, FsStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
        (dir, store)
    }

    fn ulid(s: &str) -> Ulid {
        Ulid::from_string(s).unwrap()
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

    /// A ticket file whose notes block is never closed (the HS2-PRVPCQ field failure).
    fn broken(id: &Ulid, slug: &str, padding: usize) -> String {
        format!(
            "---\nid: {id}\nslug: {slug}\ntitle: broken\ncategory: bug\n\
             created_at: 2026-08-19T00:00:00Z\nupdated_at: 2026-08-19T00:00:00Z\nschema: 1\n---\n\n\
             <!-- hotsheet:body:begin -->\nbody{}\n<!-- hotsheet:body:end -->\n\n\
             <!-- hotsheet:notes:begin -->\n## Notes\n\nunterminated note\n",
            "x".repeat(padding)
        )
    }

    fn plant(store: &FsStore, id: &Ulid, content: &str) -> PathBuf {
        let path = store.ticket_path(id);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
        path
    }

    /// Scan and check the answer against the full resilient listing.
    fn scan(cache: &mut CorruptTicketCache, store: &FsStore) -> Vec<CorruptTicket> {
        let corrupt = cache.corrupt_tickets(store).unwrap();
        assert_eq!(corrupt, store.list_tickets_resilient().unwrap().corrupt);
        corrupt
    }

    fn paths(corrupt: &[CorruptTicket]) -> Vec<PathBuf> {
        corrupt.iter().map(|c| c.path.clone()).collect()
    }

    #[test]
    fn a_file_becomes_corrupt_is_repaired_deleted_and_corrupt_again() {
        let (_dir, store) = temp_store();
        let mut cache = CorruptTicketCache::with_racy_window(Duration::ZERO);
        let healthy = sample(ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV"));
        let target = sample(ulid("3ZARZ3NDEKTSV4RRFFQ69G5FAV"));
        store.write_ticket(&healthy).unwrap();
        store.write_ticket(&target).unwrap();
        let target_path = store.ticket_path(&target.id);

        // Healthy store: every file read once, then answered from the cache.
        assert!(scan(&mut cache, &store).is_empty());
        assert_eq!(cache.last_reads(), 2);
        assert!(scan(&mut cache, &store).is_empty());
        assert_eq!(cache.last_reads(), 0, "unchanged files are not re-read");

        // Healthy → corrupt: only the changed file is re-read.
        plant(&store, &target.id, &broken(&target.id, &target.slug, 0));
        let corrupt = scan(&mut cache, &store);
        assert_eq!(paths(&corrupt), std::slice::from_ref(&target_path));
        assert_eq!(corrupt[0].slug.as_deref(), Some(target.slug.as_str()));
        assert_eq!(cache.last_reads(), 1);
        // A cached corrupt outcome is served without re-reading.
        assert_eq!(
            paths(&scan(&mut cache, &store)),
            std::slice::from_ref(&target_path)
        );
        assert_eq!(cache.last_reads(), 0);

        // Corrupt → repaired.
        store.write_ticket(&target).unwrap();
        assert!(scan(&mut cache, &store).is_empty());
        assert_eq!(cache.last_reads(), 1);

        // Corrupt again, then deleted, then corrupt again at the same path.
        plant(&store, &target.id, &broken(&target.id, &target.slug, 3));
        assert_eq!(
            paths(&scan(&mut cache, &store)),
            std::slice::from_ref(&target_path)
        );
        fs::remove_file(&target_path).unwrap();
        assert!(scan(&mut cache, &store).is_empty());
        assert_eq!(cache.last_reads(), 0, "a deleted file is dropped, not read");
        plant(&store, &target.id, &broken(&target.id, &target.slug, 7));
        assert_eq!(paths(&scan(&mut cache, &store)), [target_path]);
        assert_eq!(cache.last_reads(), 1);
    }

    #[test]
    fn interleaved_changes_across_several_files_stay_exact() {
        let (_dir, store) = temp_store();
        let mut cache = CorruptTicketCache::with_racy_window(Duration::ZERO);
        let a = sample(ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV"));
        let b = sample(ulid("3ZARZ3NDEKTSV4RRFFQ69G5FAV"));
        let c = ulid("7ZARZ3NDEKTSV4RRFFQ69G5FAV");
        store.write_ticket(&a).unwrap();
        let b_path = plant(&store, &b.id, &broken(&b.id, &b.slug, 0));
        assert_eq!(
            paths(&scan(&mut cache, &store)),
            std::slice::from_ref(&b_path)
        );

        // A new corrupt file appears while another is repaired and a healthy one breaks.
        let c_path = plant(&store, &c, "not a ticket");
        store.write_ticket(&b).unwrap();
        let a_path = plant(&store, &a.id, &broken(&a.id, &a.slug, 1));
        assert_eq!(paths(&scan(&mut cache, &store)), [a_path, c_path.clone()]);
        assert_eq!(cache.last_reads(), 3);

        // Empty-then-refill: everything goes away, then comes back.
        fs::remove_dir_all(store.root().join("tickets")).unwrap();
        assert!(scan(&mut cache, &store).is_empty());
        let c_path_again = plant(&store, &c, "still not a ticket");
        assert_eq!(c_path_again, c_path);
        assert_eq!(paths(&scan(&mut cache, &store)), [c_path]);
        assert_eq!(cache.last_reads(), 1);
    }

    #[test]
    fn recently_modified_files_are_re_read_until_they_settle() {
        let (_dir, store) = temp_store();
        let mut cache = CorruptTicketCache::new();
        let ticket = sample(ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV"));
        store.write_ticket(&ticket).unwrap();
        assert!(scan(&mut cache, &store).is_empty());
        // Just written, so inside the racy window: not trusted yet.
        assert!(scan(&mut cache, &store).is_empty());
        assert_eq!(cache.last_reads(), 1);

        // A same-size in-place rewrite inside the window is still detected.
        let path = store.ticket_path(&ticket.id);
        let original = fs::read_to_string(&path).unwrap();
        // Same bytes except the opening frontmatter fence, which no longer parses.
        let same_size = original.replacen("---", "+++", 1);
        assert_eq!(same_size.len(), original.len());
        fs::write(&path, same_size).unwrap();
        assert_eq!(paths(&scan(&mut cache, &store)), [path]);
    }

    #[test]
    fn an_old_file_is_trusted_by_the_default_window() {
        let (_dir, store) = temp_store();
        let mut cache = CorruptTicketCache::new();
        let ticket = sample(ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV"));
        store.write_ticket(&ticket).unwrap();
        let file = fs::File::options()
            .write(true)
            .open(store.ticket_path(&ticket.id))
            .unwrap();
        file.set_modified(SystemTime::now() - Duration::from_secs(60))
            .unwrap();
        drop(file);
        assert!(scan(&mut cache, &store).is_empty());
        assert!(scan(&mut cache, &store).is_empty());
        assert_eq!(cache.last_reads(), 0);
    }

    #[test]
    fn attachment_directory_changes_invalidate_the_ticket() {
        let (_dir, store) = temp_store();
        let mut cache = CorruptTicketCache::with_racy_window(Duration::ZERO);
        let ticket = sample(ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV"));
        store.write_ticket(&ticket).unwrap();
        assert!(scan(&mut cache, &store).is_empty());
        let attachments = store.attachment_dir(&ticket.id);
        fs::create_dir_all(&attachments).unwrap();
        fs::write(attachments.join("legacy.txt"), "bytes").unwrap();
        assert!(scan(&mut cache, &store).is_empty());
        assert_eq!(cache.last_reads(), 1);
        assert!(scan(&mut cache, &store).is_empty());
        assert_eq!(cache.last_reads(), 0);
    }

    #[test]
    fn non_parse_failures_are_re_evaluated_every_scan() {
        let (_dir, store) = temp_store();
        let mut cache = CorruptTicketCache::with_racy_window(Duration::ZERO);
        // A directory named like a ticket fails to read, not to parse.
        let id = ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV");
        let path = store.ticket_path(&id);
        fs::create_dir_all(&path).unwrap();
        let corrupt = scan(&mut cache, &store);
        assert_eq!(paths(&corrupt), std::slice::from_ref(&path));
        assert_eq!(corrupt[0].error_code, "invalid_ticket");
        assert_eq!(
            paths(&scan(&mut cache, &store)),
            std::slice::from_ref(&path)
        );
        assert_eq!(cache.last_reads(), 1, "an I/O failure is never cached");
        fs::remove_dir(&path).unwrap();
        assert!(scan(&mut cache, &store).is_empty());
    }
}
