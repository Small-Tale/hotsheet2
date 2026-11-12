//! HS2-SBQ3T8: append-only, bounded audit journal of orchestration decisions.
//!
//! One JSON-lines file per owning checkout under the machine home. Entries carry
//! identifiers, codes and outcomes only: never ticket text, credentials or origins.
//! A full segment rotates to one retained predecessor; `seq` never restarts.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::OrchestrationError;
use super::contract::{valid_identifier, valid_token};
use crate::FileLock;

pub const MAX_AUDIT_SEGMENT_BYTES: u64 = 1024 * 1024;
pub const MAX_AUDIT_PAGE: usize = 500;
const MAX_ENTRY_BYTES: usize = 2048;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditOutcome {
    /// Authority was confirmed and the action may proceed.
    Allowed,
    Denied,
    /// The authorized action reported its result.
    Succeeded,
    Failed,
    Granted,
    Revoked,
}

/// What a caller supplies; the journal assigns `seq`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuditRecord {
    pub at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
    /// A stable action code such as `tickets_write` or `grant_create`.
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    pub outcome: AuditOutcome,
    /// A stable reason code, never free text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grant_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditEntry {
    pub seq: u64,
    #[serde(flatten)]
    pub record: AuditRecord,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuditPage {
    pub entries: Vec<AuditEntry>,
    /// Pass as `after` to continue; equals the request cursor when nothing is newer.
    pub cursor: u64,
    /// Retention removed entries between the request cursor and the first one returned.
    pub gap: bool,
    /// More retained entries exist beyond this page.
    pub more: bool,
}

#[derive(Clone)]
pub struct AuditJournal {
    directory: PathBuf,
    owner: String,
    segment_limit: u64,
}

impl AuditJournal {
    pub fn new(home: &Path, owner_checkout_id: &str) -> Result<Self, OrchestrationError> {
        valid_identifier(owner_checkout_id)?;
        Ok(Self {
            directory: home.join("orchestration-audit"),
            owner: owner_checkout_id.into(),
            segment_limit: MAX_AUDIT_SEGMENT_BYTES,
        })
    }

    #[cfg(test)]
    pub(crate) fn with_segment_limit(mut self, limit: u64) -> Self {
        self.segment_limit = limit;
        self
    }

    fn current(&self) -> PathBuf {
        self.directory.join(format!("{}.jsonl", self.owner))
    }

    fn previous(&self) -> PathBuf {
        self.directory.join(format!("{}.1.jsonl", self.owner))
    }

    fn lock(&self) -> Result<FileLock, OrchestrationError> {
        fs::create_dir_all(&self.directory)?;
        real_directory(&self.directory)?;
        let path = self.directory.join(format!("{}.lock", self.owner));
        Ok(FileLock::acquire(open(&path, true, false)?)?)
    }

    /// Append one entry durably and return it with its assigned `seq`.
    pub fn append(&self, record: AuditRecord) -> Result<AuditEntry, OrchestrationError> {
        validate(&record)?;
        let _lock = self.lock()?;
        let seq = self.last_seq()? + 1;
        let entry = AuditEntry { seq, record };
        let mut line = serde_json::to_vec(&entry).map_err(|_| OrchestrationError::Corrupt)?;
        if line.len() > MAX_ENTRY_BYTES {
            return Err(OrchestrationError::Capacity);
        }
        line.push(b'\n');
        let current = self.current();
        let size = regular_len(&current)?;
        if size > 0 && size + line.len() as u64 > self.segment_limit {
            reject_nonregular(&self.previous())?;
            fs::rename(&current, self.previous())?;
        }
        let mut file = open(&current, true, true)?;
        // A crash can leave a torn final line; start the new entry on its own line.
        if needs_newline(&current)? {
            file.write_all(b"\n")?;
        }
        file.write_all(&line)?;
        file.sync_all()?;
        #[cfg(unix)]
        File::open(&self.directory)?.sync_all()?;
        Ok(entry)
    }

    /// Entries with `seq > after`, oldest first, at most `limit` (capped).
    pub fn read(&self, after: u64, limit: usize) -> Result<AuditPage, OrchestrationError> {
        let limit = limit.clamp(1, MAX_AUDIT_PAGE);
        let mut retained = self.entries(&self.previous())?;
        retained.extend(self.entries(&self.current())?);
        let first_retained = retained.first().map(|entry| entry.seq);
        let mut newer: Vec<_> = retained
            .into_iter()
            .filter(|entry| entry.seq > after)
            .collect();
        let more = newer.len() > limit;
        newer.truncate(limit);
        Ok(AuditPage {
            gap: first_retained.is_some_and(|first| first > after + 1),
            cursor: newer.last().map_or(after, |entry| entry.seq),
            more,
            entries: newer,
        })
    }

    fn last_seq(&self) -> Result<u64, OrchestrationError> {
        for path in [self.current(), self.previous()] {
            if let Some(entry) = self.entries(&path)?.last() {
                return Ok(entry.seq);
            }
        }
        Ok(0)
    }

    /// Parsed complete lines of one segment. A torn or foreign line is skipped, never
    /// rewritten, so a crash mid-append cannot hide earlier history.
    fn entries(&self, path: &Path) -> Result<Vec<AuditEntry>, OrchestrationError> {
        real_directory(&self.directory)?;
        reject_nonregular(path)?;
        let mut file = match open(path, false, false) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        let mut bytes = Vec::new();
        // Twice the limit tolerates one oversized pre-rotation append; beyond is foreign.
        (&mut file)
            .take(self.segment_limit.max(MAX_AUDIT_SEGMENT_BYTES) * 2 + 1)
            .read_to_end(&mut bytes)?;
        let mut entries: Vec<AuditEntry> = Vec::new();
        for line in bytes.split(|byte| *byte == b'\n') {
            let Ok(entry) = serde_json::from_slice::<AuditEntry>(line) else {
                continue;
            };
            if entries.last().is_none_or(|last| entry.seq > last.seq) {
                entries.push(entry);
            }
        }
        Ok(entries)
    }
}

fn validate(record: &AuditRecord) -> Result<(), OrchestrationError> {
    valid_identifier(&record.action)?;
    for value in [&record.actor_role, &record.actor_id, &record.reason]
        .into_iter()
        .flatten()
    {
        valid_identifier(value)?;
    }
    if let Some(operation) = &record.operation_id {
        valid_identifier(operation)?;
    }
    for value in [&record.target_id, &record.grant_id].into_iter().flatten() {
        valid_token(value)?;
    }
    Ok(())
}

fn real_directory(path: &Path) -> Result<(), OrchestrationError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => Err(
            OrchestrationError::Invalid("storage directories must be real directories"),
        ),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn reject_nonregular(path: &Path) -> Result<(), OrchestrationError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() || metadata.file_type().is_symlink() => Err(
            OrchestrationError::Invalid("storage paths must be regular files"),
        ),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn regular_len(path: &Path) -> Result<u64, OrchestrationError> {
    reject_nonregular(path)?;
    match fs::metadata(path) {
        Ok(metadata) => Ok(metadata.len()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(error.into()),
    }
}

fn needs_newline(path: &Path) -> Result<bool, OrchestrationError> {
    let mut file = match open(path, false, false) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    if file.metadata()?.len() == 0 {
        return Ok(false);
    }
    file.seek(SeekFrom::End(-1))?;
    let mut last = [0u8; 1];
    file.read_exact(&mut last)?;
    Ok(last[0] != b'\n')
}

fn open(path: &Path, create: bool, append: bool) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).create(create);
    if append {
        options.append(true);
    } else if create {
        options.write(true).truncate(false);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "regular file required",
        ));
    }
    Ok(file)
}
