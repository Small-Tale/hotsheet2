//! HS2-WKRFD5: durable delivery state for human-feedback notifications (HS2-25TP3N).
//!
//! A Git ticket's delivery activity note is the lasting record that a human note reached an AI
//! session, but it can only be written after the send, and that store write can fail. The
//! journal closes that gap with one small file per note under the machine home, written
//! before the send and removed once the activity note commits:
//!
//! - `sending`: written (and synced) before the message leaves. Its write failing means the
//!   message is never sent. Found after a restart, the outcome is uncertain: the send may or
//!   may not have happened, so the note is never resent and the ticket records the uncertainty.
//! - `sent`: the send succeeded but the activity note may be missing. Reconciliation writes
//!   the ordinary delivery note.
//!
//! Every outcome is at most once: a journal file, an in-memory reservation, or an activity
//! note each refuse a further send for that note.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use hotsheet_model::Ulid;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum JournalState {
    Sending,
    Sent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct JournalEntry {
    pub(crate) note_id: String,
    pub(crate) ticket_id: String,
    pub(crate) store_root: PathBuf,
    pub(crate) connection_id: String,
    pub(crate) state: JournalState,
}

/// The activity-note text that records a confirmed delivery (HS2-25TP3N).
pub(crate) fn sent_text(note_id: &str, connection_id: &str) -> String {
    format!("HS2-25TP3N notified note {note_id} through session {connection_id}.")
}

/// The activity-note text that records a delivery whose outcome a restart made unknowable.
pub(crate) fn uncertain_text(note_id: &str, connection_id: &str) -> String {
    format!(
        "HS2-25TP3N may have notified note {note_id} through session {connection_id}; the \
         server stopped before confirming the send, so the note was not resent (HS2-WKRFD5)."
    )
}

/// Whether an activity note's text records a delivery of `note_id`, confirmed or uncertain.
pub(crate) fn records_delivery(text: &str, note_id: &str) -> bool {
    text.starts_with(&format!("HS2-25TP3N notified note {note_id} "))
        || text.starts_with(&format!("HS2-25TP3N may have notified note {note_id} "))
}

impl JournalEntry {
    /// The activity note reconciliation writes for this entry.
    pub(crate) fn activity_text(&self) -> String {
        match self.state {
            JournalState::Sent => sent_text(&self.note_id, &self.connection_id),
            JournalState::Sending => uncertain_text(&self.note_id, &self.connection_id),
        }
    }
}

/// One directory of per-note journal files. Per-note files keep concurrent deliveries (and
/// concurrent server processes sharing a home) from overwriting each other's state.
#[derive(Debug, Clone)]
pub(crate) struct Journal {
    dir: PathBuf,
}

impl Journal {
    pub(crate) fn at(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    fn path(&self, note_id: &str) -> std::io::Result<PathBuf> {
        // Only a ULID names a file, so a note id can never escape the directory.
        Ulid::from_string(note_id)
            .map(|id| self.dir.join(format!("{id}.json")))
            .map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "feedback note id is not a ULID",
                )
            })
    }

    /// Durably replace this note's entry: a synced temporary file renamed into place.
    pub(crate) fn write(&self, entry: &JournalEntry) -> std::io::Result<()> {
        let path = self.path(&entry.note_id)?;
        std::fs::create_dir_all(&self.dir)?;
        let temporary = path.with_extension("json.tmp");
        let body = serde_json::to_vec(entry).map_err(std::io::Error::other)?;
        {
            let mut file = std::fs::File::create(&temporary)?;
            file.write_all(&body)?;
            file.sync_all()?;
        }
        hotsheet_sync::files::replace(&temporary, &path)?;
        if let Ok(dir) = std::fs::File::open(&self.dir) {
            let _ = dir.sync_all();
        }
        Ok(())
    }

    /// This note's entry. An unreadable or corrupt file is an error, which callers treat as
    /// an uncertain delivery rather than permission to send.
    pub(crate) fn read(&self, note_id: &str) -> std::io::Result<Option<JournalEntry>> {
        match std::fs::read(self.path(note_id)?) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(std::io::Error::other),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub(crate) fn remove(&self, note_id: &str) -> std::io::Result<()> {
        match std::fs::remove_file(self.path(note_id)?) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        }
    }

    /// Every readable entry for the store at `store_root`.
    pub(crate) fn for_store(&self, store_root: &Path) -> Vec<JournalEntry> {
        let Ok(dir) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        dir.filter_map(Result::ok)
            .filter(|file| file.path().extension().is_some_and(|ext| ext == "json"))
            .filter_map(|file| std::fs::read(file.path()).ok())
            .filter_map(|bytes| serde_json::from_slice::<JournalEntry>(&bytes).ok())
            .filter(|entry| entry.store_root == store_root)
            .collect()
    }
}

/// What a delivery attempt may do for a note, given its journal entry and ticket record.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Admission {
    /// No trace of an earlier attempt: the note may be sent once.
    Send,
    /// An activity note already records the delivery.
    Recorded,
    /// A journal entry from an earlier attempt exists; reconcile it and never resend.
    Reconcile(JournalEntry),
    /// The journal could not be read; refuse rather than risk a duplicate.
    Unreadable,
}

/// Decide whether a note may be sent (HS2-WKRFD5). The journal is consulted first because a
/// `sent` entry can exist precisely when the activity note write failed.
pub(crate) fn admit(
    journal: &Journal,
    note_id: &str,
    recorded_on_ticket: impl FnOnce() -> bool,
) -> Admission {
    match journal.read(note_id) {
        Ok(Some(entry)) => Admission::Reconcile(entry),
        Ok(None) if recorded_on_ticket() => Admission::Recorded,
        Ok(None) => Admission::Send,
        Err(_) => Admission::Unreadable,
    }
}

/// Write the activity note an entry stands for and drop the entry once it commits; the
/// entry stays (and keeps refusing resends) while the store write keeps failing.
pub(crate) fn reconcile<E>(
    journal: &Journal,
    entry: &JournalEntry,
    already_recorded: bool,
    write_activity: impl FnOnce(String) -> Result<(), E>,
) -> Result<(), E> {
    if !already_recorded {
        write_activity(entry.activity_text())?;
    }
    if let Err(error) = journal.remove(&entry.note_id) {
        tracing::warn!(%error, note_id = entry.note_id, "removing a reconciled feedback journal entry failed");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(note_id: &str, state: JournalState) -> JournalEntry {
        JournalEntry {
            note_id: note_id.into(),
            ticket_id: Ulid::new().to_string(),
            store_root: PathBuf::from("/synthetic/store"),
            connection_id: "session-1".into(),
            state,
        }
    }

    #[test]
    fn hs2_wkrfd5_journal_round_trips_replaces_and_removes_per_note() {
        let dir = tempfile::tempdir().unwrap();
        let journal = Journal::at(dir.path().join("feedback-deliveries"));
        let note = Ulid::new().to_string();
        assert_eq!(journal.read(&note).unwrap(), None, "empty before any write");
        let sending = entry(&note, JournalState::Sending);
        journal.write(&sending).unwrap();
        assert_eq!(journal.read(&note).unwrap(), Some(sending.clone()));
        let sent = JournalEntry {
            state: JournalState::Sent,
            ..sending
        };
        journal.write(&sent).unwrap();
        assert_eq!(journal.read(&note).unwrap(), Some(sent.clone()));
        assert_eq!(
            journal.for_store(Path::new("/synthetic/store")),
            vec![sent.clone()]
        );
        assert!(journal.for_store(Path::new("/other")).is_empty());
        journal.remove(&note).unwrap();
        journal.remove(&note).unwrap();
        assert_eq!(journal.read(&note).unwrap(), None, "removal is idempotent");
        // Empty, then refilled.
        journal.write(&sent).unwrap();
        assert_eq!(journal.read(&note).unwrap(), Some(sent));
    }

    #[test]
    fn hs2_wkrfd5_a_non_ulid_note_id_never_names_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let journal = Journal::at(dir.path());
        let mut escape = entry("x", JournalState::Sending);
        escape.note_id = "../escape".into();
        assert!(journal.write(&escape).is_err());
        assert!(journal.read("../escape").is_err());
        assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none());
    }

    #[test]
    fn hs2_wkrfd5_admission_never_sends_twice_across_write_failure_restart_and_retry() {
        let dir = tempfile::tempdir().unwrap();
        let journal = Journal::at(dir.path());
        let note = Ulid::new().to_string();
        // First attempt: nothing recorded, the note may be sent.
        assert_eq!(admit(&journal, &note, || false), Admission::Send);
        // The send succeeds, then the activity note write fails: the sent entry stays.
        let sent = entry(&note, JournalState::Sent);
        journal.write(&sent).unwrap();
        let failed: Result<(), &str> = reconcile(&journal, &sent, false, |_| Err("store locked"));
        assert_eq!(failed, Err("store locked"));
        // Restart (a fresh process has no reservation) and retry: never resent.
        assert_eq!(
            admit(&journal, &note, || false),
            Admission::Reconcile(sent.clone())
        );
        // Reconciliation writes the ordinary delivery note once the store accepts writes.
        let mut written = Vec::new();
        reconcile::<()>(&journal, &sent, false, |text| {
            written.push(text);
            Ok(())
        })
        .unwrap();
        assert_eq!(written, [sent_text(&note, "session-1")]);
        // A further retry sees the activity note on the ticket.
        assert_eq!(admit(&journal, &note, || true), Admission::Recorded);
    }

    #[test]
    fn hs2_wkrfd5_a_restart_mid_send_is_reported_as_uncertain_and_not_resent() {
        let dir = tempfile::tempdir().unwrap();
        let journal = Journal::at(dir.path());
        let note = Ulid::new().to_string();
        let sending = entry(&note, JournalState::Sending);
        journal.write(&sending).unwrap();
        // The process stopped between the intent and the confirmation.
        assert_eq!(
            admit(&journal, &note, || false),
            Admission::Reconcile(sending.clone())
        );
        let mut written = Vec::new();
        reconcile::<()>(&journal, &sending, false, |text| {
            written.push(text);
            Ok(())
        })
        .unwrap();
        assert_eq!(written, [uncertain_text(&note, "session-1")]);
        assert!(records_delivery(&written[0], &note));
        assert_eq!(journal.read(&note).unwrap(), None);
        // An entry whose note already exists (written, then removal failed) is only removed.
        journal.write(&sending).unwrap();
        reconcile::<()>(&journal, &sending, true, |_| panic!("already recorded")).unwrap();
        assert_eq!(journal.read(&note).unwrap(), None);
    }

    #[test]
    fn hs2_wkrfd5_a_corrupt_entry_refuses_rather_than_resends() {
        let dir = tempfile::tempdir().unwrap();
        let journal = Journal::at(dir.path());
        let note = Ulid::new().to_string();
        std::fs::write(dir.path().join(format!("{note}.json")), b"{not json").unwrap();
        assert_eq!(admit(&journal, &note, || false), Admission::Unreadable);
        assert!(journal.for_store(Path::new("/synthetic/store")).is_empty());
    }

    #[test]
    fn hs2_wkrfd5_both_activity_forms_count_as_recorded_for_their_note_only() {
        let note = Ulid::new().to_string();
        let other = Ulid::new().to_string();
        assert!(records_delivery(&sent_text(&note, "s"), &note));
        assert!(records_delivery(&uncertain_text(&note, "s"), &note));
        assert!(!records_delivery(&sent_text(&other, "s"), &note));
        assert!(!records_delivery("HS2-25TP3N notified note", &note));
    }
}
