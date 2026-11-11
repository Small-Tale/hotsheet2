//! The machine-local record of one store's live-state inputs (HS2-MHW3SX,
//! `docs/32-notifier-plugins.md` §32.8.3). The claim tracker and the progress fingerprints
//! otherwise live only in server memory, so a restart would zero "tickets
//! updated" and treat the first write to every ticket as a baseline. The record sits beside
//! the peak book at `metrics/local/live-state.json`, which the store gitignores, and is
//! recovered best effort: an unreadable or missing record starts empty, and the server then
//! falls back to what the tickets themselves record.

use std::collections::BTreeMap;
use std::path::PathBuf;

use hotsheet_ticketing::FsStore;
use serde::{Deserialize, Serialize};

use crate::project_activity::ActivityRecord;

/// The record format. A record of another version is ignored rather than misread.
pub const RECORD_VERSION: u32 = 1;

/// Fingerprints kept per store; the most recently seen win.
pub const FINGERPRINT_LIMIT: usize = 5000;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LiveRecord {
    pub version: u32,
    /// When it was written (Unix ms). Not compared when deciding whether to rewrite.
    #[serde(default)]
    pub saved_at_ms: i64,
    /// The epoch of the server that wrote it; a later server starts above it.
    #[serde(default)]
    pub epoch_ms: i64,
    /// The last pushed revision; a later server continues above it.
    #[serde(default)]
    pub revision: u64,
    #[serde(default)]
    pub activity: Option<ActivityRecord>,
    /// Progress fingerprint per ticket id.
    #[serde(default)]
    pub fingerprints: BTreeMap<String, FingerprintRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FingerprintRecord {
    pub hash: u64,
    /// When the server last saw the ticket (Unix ms), which decides what the cap drops.
    pub seen_ms: i64,
}

impl LiveRecord {
    pub fn path(store: &FsStore) -> PathBuf {
        store
            .root()
            .join("metrics")
            .join("local")
            .join("live-state.json")
    }

    /// The store's record, or `None` when it is missing, unreadable, or another version.
    pub fn load(store: &FsStore) -> Option<LiveRecord> {
        let bytes = std::fs::read(Self::path(store)).ok()?;
        let record: LiveRecord = match serde_json::from_slice(&bytes) {
            Ok(record) => record,
            Err(error) => {
                tracing::warn!(store = %store.root().display(), %error, "ignoring an unreadable live-state record");
                return None;
            }
        };
        (record.version == RECORD_VERSION).then_some(record)
    }

    /// Keep the newest [`FINGERPRINT_LIMIT`] fingerprints.
    pub fn cap_fingerprints(&mut self) {
        if self.fingerprints.len() <= FINGERPRINT_LIMIT {
            return;
        }
        let mut seen: Vec<(i64, String)> = self
            .fingerprints
            .iter()
            .map(|(ticket, fp)| (fp.seen_ms, ticket.clone()))
            .collect();
        seen.sort_unstable_by(|a, b| b.cmp(a));
        for (_, ticket) in seen.into_iter().skip(FINGERPRINT_LIMIT) {
            self.fingerprints.remove(&ticket);
        }
    }

    /// Write atomically with `saved_at_ms`. Failures are logged: the inputs stay in memory
    /// and the next change tries again.
    pub fn save(&self, store: &FsStore, saved_at_ms: i64) -> bool {
        let path = Self::path(store);
        let mut record = self.clone();
        record.saved_at_ms = saved_at_ms;
        let write = || -> std::io::Result<()> {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            hotsheet_ticketing::metrics::ensure_gitignored(store, "metrics/local/")?;
            let temp = path.with_extension("json.tmp");
            std::fs::write(
                &temp,
                serde_json::to_vec_pretty(&record).map_err(std::io::Error::other)?,
            )?;
            std::fs::rename(&temp, &path)
        };
        match write() {
            Ok(()) => true,
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "could not save the live-state record");
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(dir: &tempfile::TempDir) -> FsStore {
        FsStore::init(
            dir.path().join("demo.hs2"),
            &hotsheet_ticketing::StoreMetadata::new("HS2"),
        )
        .unwrap()
    }

    #[test]
    fn hs2_mhw3sx_record_round_trips_gitignored_and_rejects_bad_files() {
        let dir = tempfile::tempdir().unwrap();
        let store = store(&dir);
        assert_eq!(LiveRecord::load(&store), None, "no record yet");
        let mut record = LiveRecord {
            version: RECORD_VERSION,
            epoch_ms: 10,
            revision: 7,
            activity: Some(ActivityRecord::default()),
            ..LiveRecord::default()
        };
        record.fingerprints.insert(
            "t1".into(),
            FingerprintRecord {
                hash: u64::MAX,
                seen_ms: 5,
            },
        );
        assert!(record.save(&store, 99));
        let loaded = LiveRecord::load(&store).unwrap();
        assert_eq!(loaded.saved_at_ms, 99);
        let mut compared = loaded.clone();
        compared.saved_at_ms = 0;
        assert_eq!(compared, record);
        let ignore = std::fs::read_to_string(store.root().join(".gitignore")).unwrap();
        assert!(ignore.lines().any(|l| l.trim() == "metrics/local/"));
        assert!(!LiveRecord::path(&store).with_extension("json.tmp").exists());
        // Another version, or garbage, is no record.
        std::fs::write(LiveRecord::path(&store), r#"{"version":2}"#).unwrap();
        assert_eq!(LiveRecord::load(&store), None);
        std::fs::write(LiveRecord::path(&store), "{not json").unwrap();
        assert_eq!(LiveRecord::load(&store), None);
        // Older fields missing still load.
        std::fs::write(LiveRecord::path(&store), r#"{"version":1}"#).unwrap();
        assert_eq!(LiveRecord::load(&store).unwrap().revision, 0);
        // A record from before HS2-5A83MR still carries the queue runway's ETA samples; it
        // loads and the samples are dropped.
        std::fs::write(
            LiveRecord::path(&store),
            r#"{"version":1,"revision":4,"eta_samples":[{"ticket":"t1","claimed_at_ms":1,"planned_ms":60000}]}"#,
        )
        .unwrap();
        assert_eq!(LiveRecord::load(&store).unwrap().revision, 4);
    }

    #[test]
    fn hs2_mhw3sx_fingerprint_cap_keeps_the_most_recently_seen() {
        let mut record = LiveRecord::default();
        for n in 0..(FINGERPRINT_LIMIT as i64 + 10) {
            record.fingerprints.insert(
                format!("t{n:05}"),
                FingerprintRecord {
                    hash: n as u64,
                    seen_ms: n,
                },
            );
        }
        record.cap_fingerprints();
        assert_eq!(record.fingerprints.len(), FINGERPRINT_LIMIT);
        assert!(
            !record.fingerprints.contains_key("t00009"),
            "oldest dropped"
        );
        assert!(record.fingerprints.contains_key("t00010"));
        record.cap_fingerprints();
        assert_eq!(record.fingerprints.len(), FINGERPRINT_LIMIT, "idempotent");
    }
}
