//! Process-shared delivery journal and durable held-summary reservations (HS2-MFD44D).
//! The flush lock may enclose journal locks; a journal lock never encloses transport work.

use std::collections::HashSet;
#[cfg(unix)]
use std::fs::File;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::Path;

use hotsheet_ticketing::FileLock;
use serde::{Deserialize, Serialize};

use crate::dispatch::{DeliveryRecord, Outcome};
use crate::message::EventKind;

pub(crate) const COMPACT_AT: usize = 1000;
pub(crate) const KEEP: usize = 500;
const RESERVATION_KEY: &str = "held_summary_reservation";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReservationEntry {
    held_summary_reservation: Reservation,
}

/// Private journal metadata, deliberately not shaped like a public DeliveryRecord.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reservation {
    version: u8,
    record: DeliveryRecord,
    held_ids: Vec<String>,
}

#[derive(Debug, Clone)]
enum Entry {
    Record(DeliveryRecord),
    Reservation(Reservation),
    // Old readers skipped unknown/malformed lines. Retain them through bounded compaction.
    Other(String),
}

impl Entry {
    fn encode(&self) -> io::Result<String> {
        match self {
            Self::Record(record) => serde_json::to_string(record).map_err(io::Error::other),
            Self::Reservation(reservation) => serde_json::to_string(&ReservationEntry {
                held_summary_reservation: reservation.clone(),
            })
            .map_err(io::Error::other),
            Self::Other(line) => Ok(line.clone()),
        }
    }
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn decode(line: &str) -> io::Result<Entry> {
    let value = serde_json::from_str::<serde_json::Value>(line);
    if value
        .as_ref()
        .is_ok_and(|value| value.get(RESERVATION_KEY).is_some())
        || (value.is_err()
            && line
                .trim_start()
                .starts_with("{\"held_summary_reservation\""))
    {
        let entry: ReservationEntry = serde_json::from_str(line)
            .map_err(|_| invalid("invalid held-summary reservation; refusing to resend"))?;
        let reservation = entry.held_summary_reservation;
        let distinct: HashSet<_> = reservation.held_ids.iter().collect();
        if reservation.version != 1
            || reservation.record.event != EventKind::HeldSummary
            || reservation.record.id.is_empty()
            || reservation.record.channel.is_empty()
            || reservation.record.held
            || reservation.record.outcome != Outcome::Failed
            || reservation.record.attempts != 0
            || reservation.held_ids.is_empty()
            || distinct.len() != reservation.held_ids.len()
            || reservation.held_ids.iter().any(String::is_empty)
        {
            return Err(invalid(
                "invalid held-summary reservation; refusing to resend",
            ));
        }
        return Ok(Entry::Reservation(reservation));
    }
    Ok(serde_json::from_str(line)
        .map(Entry::Record)
        .unwrap_or_else(|_| Entry::Other(line.to_owned())))
}

fn lock_file(path: &Path, suffix: &str) -> io::Result<std::fs::File> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let name = path
        .file_name()
        .ok_or_else(|| invalid("delivery journal has no filename"))?
        .to_string_lossy();
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path.with_file_name(format!(".{name}.{suffix}.lock")))?;
    Ok(file)
}

fn lock(path: &Path, suffix: &str) -> io::Result<FileLock> {
    FileLock::acquire(lock_file(path, suffix)?)
}

pub(crate) fn lock_flush(path: &Path) -> io::Result<Option<FileLock>> {
    let file = lock_file(path, "flush")?;
    // A notifier may synchronously trigger a serverless write in the same home. Busy
    // flushes must skip, so that child's ordinary dispatch can finish (HS2-MFD44D).
    #[cfg(any(unix, target_os = "windows"))]
    {
        FileLock::try_acquire(file)
    }
    #[cfg(not(any(unix, target_os = "windows")))]
    {
        let _ = file;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "process-shared summary locking is unavailable",
        ))
    }
}

pub(crate) struct Journal<'a> {
    path: &'a Path,
    _lock: FileLock,
    entries: Vec<Entry>,
    needs_newline: bool,
}

impl<'a> Journal<'a> {
    pub(crate) fn open(path: &'a Path) -> io::Result<Self> {
        let guard = lock(path, "journal")?;
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(error),
        };
        let entries = text.lines().map(decode).collect::<io::Result<_>>()?;
        Ok(Self {
            path,
            _lock: guard,
            entries,
            needs_newline: !text.is_empty() && !text.ends_with('\n'),
        })
    }

    pub(crate) fn records(&self) -> Vec<DeliveryRecord> {
        self.entries
            .iter()
            .filter_map(|entry| match entry {
                Entry::Record(record) => Some(record.clone()),
                _ => None,
            })
            .collect()
    }

    /// Legacy summaries consume preceding records by position; reserved summaries consume
    /// exact IDs, including when their completion lands after a newly held notification.
    pub(crate) fn held(&self, channel: &str) -> Vec<DeliveryRecord> {
        let reservations: Vec<_> = self
            .entries
            .iter()
            .filter_map(|entry| match entry {
                Entry::Reservation(reservation) if reservation.record.channel == channel => {
                    Some(reservation)
                }
                _ => None,
            })
            .collect();
        let modern: HashSet<_> = reservations.iter().map(|r| &r.record.id).collect();
        let consumed: HashSet<_> = reservations.iter().flat_map(|r| &r.held_ids).collect();
        let records: Vec<_> = self
            .entries
            .iter()
            .filter_map(|entry| match entry {
                Entry::Record(record) if record.channel == channel => Some(record),
                _ => None,
            })
            .collect();
        let since = records
            .iter()
            .rposition(|r| r.event == EventKind::HeldSummary && !modern.contains(&r.id))
            .map_or(0, |index| index + 1);
        records[since..]
            .iter()
            .filter(|record| record.held && !consumed.contains(&record.id))
            .map(|record| (*record).clone())
            .collect()
    }

    /// The caller owns the flush lock, so an unresolved reservation cannot still be sending.
    /// Recovery is independent of current channel configuration, enabled state or hold time.
    pub(crate) fn recover_interrupted(&mut self) -> io::Result<Vec<DeliveryRecord>> {
        let completed: HashSet<_> = self
            .records()
            .into_iter()
            .filter(|record| record.event == EventKind::HeldSummary)
            .map(|record| (record.id, record.channel))
            .collect();
        let pending: Vec<_> = self
            .entries
            .iter()
            .filter_map(|entry| match entry {
                Entry::Reservation(reservation)
                    if !completed.contains(&(
                        reservation.record.id.clone(),
                        reservation.record.channel.clone(),
                    )) =>
                {
                    Some(reservation.record.clone())
                }
                _ => None,
            })
            .collect();
        let mut recovered = Vec::new();
        for mut record in pending {
            record.error =
                Some("held-summary delivery was interrupted; outcome unknown, not resent".into());
            self.append_record(&record)?;
            recovered.push(record);
        }
        Ok(recovered)
    }

    pub(crate) fn reserve(
        &mut self,
        record: &DeliveryRecord,
        held: &[DeliveryRecord],
    ) -> io::Result<()> {
        self.append(Entry::Reservation(Reservation {
            version: 1,
            record: record.clone(),
            held_ids: held.iter().map(|record| record.id.clone()).collect(),
        }))
    }

    pub(crate) fn append_record(&mut self, record: &DeliveryRecord) -> io::Result<()> {
        self.append(Entry::Record(record.clone()))
    }

    fn append(&mut self, entry: Entry) -> io::Result<()> {
        let mut line = Vec::new();
        if self.needs_newline {
            line.push(b'\n');
        }
        line.extend_from_slice(entry.encode()?.as_bytes());
        line.push(b'\n');
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.path)?;
        file.write_all(&line)?;
        file.sync_all()?;
        drop(file);
        sync_parent(self.path)?;
        self.entries.push(entry);
        self.needs_newline = false;
        if self.entries.len() > COMPACT_AT {
            self.compact()?;
        }
        Ok(())
    }

    fn compact(&mut self) -> io::Result<()> {
        let completed: HashSet<_> = self
            .records()
            .into_iter()
            .filter(|record| record.event == EventKind::HeldSummary)
            .map(|record| record.id)
            .collect();
        let count = self
            .entries
            .iter()
            .filter(|entry| !matches!(entry, Entry::Reservation(_)))
            .count();
        let mut skip = count.saturating_sub(KEEP);
        let retained: Vec<_> = self
            .entries
            .iter()
            .filter(|entry| {
                if matches!(entry, Entry::Reservation(_)) {
                    return false;
                }
                if skip > 0 {
                    skip -= 1;
                    false
                } else {
                    true
                }
            })
            .collect();
        let retained_ids: HashSet<_> = retained
            .iter()
            .filter_map(|entry| match entry {
                Entry::Record(record) => Some(&record.id),
                _ => None,
            })
            .collect();
        let retained_held: HashSet<_> = retained
            .iter()
            .filter_map(|entry| match entry {
                Entry::Record(record) if record.held => Some(&record.id),
                _ => None,
            })
            .collect();
        let mut remaining = count.saturating_sub(KEEP);
        let entries: Vec<_> = self
            .entries
            .iter()
            .filter(|entry| match entry {
                Entry::Reservation(reservation) => {
                    !completed.contains(&reservation.record.id)
                        || retained_ids.contains(&reservation.record.id)
                        || reservation
                            .held_ids
                            .iter()
                            .any(|id| retained_held.contains(id))
                }
                _ if remaining > 0 => {
                    remaining -= 1;
                    false
                }
                _ => true,
            })
            .cloned()
            .collect();
        let temporary = self
            .path
            .with_extension(format!("{}.tmp", ulid::Ulid::new()));
        let write = || -> io::Result<()> {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            for entry in &entries {
                writeln!(file, "{}", entry.encode()?)?;
            }
            file.sync_all()?;
            drop(file);
            std::fs::rename(&temporary, self.path)?;
            sync_parent(self.path)
        };
        if let Err(error) = write() {
            let _ = std::fs::remove_file(&temporary);
            return Err(error);
        }
        self.entries = entries;
        Ok(())
    }
}

#[cfg(unix)]
fn sync_parent(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        File::open(parent)?.sync_all()?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn sync_parent(_path: &Path) -> io::Result<()> {
    Ok(())
}

pub(crate) fn append_record(path: &Path, record: &DeliveryRecord) -> io::Result<()> {
    Journal::open(path)?.append_record(record)
}

pub(crate) fn read_records(path: &Path) -> io::Result<Vec<DeliveryRecord>> {
    let _guard = lock(path, "journal")?;
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    // Public reads retain their legacy contract: private entries and malformed lines
    // are skipped. Flush/append use the strict reservation parser and fail closed.
    Ok(text
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect())
}

#[cfg(test)]
#[path = "delivery_log_tests.rs"]
mod tests;
