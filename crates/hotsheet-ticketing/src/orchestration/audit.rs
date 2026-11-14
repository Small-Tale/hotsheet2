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
    /// HS2-SBQ3T8 grant history; retired by HS2-XHQ1EH, kept so old entries read.
    Granted,
    /// A retired grant was revoked, or (HS2-XHQ1EH) a remembered rule was forgotten.
    Revoked,
    /// A person allowed a permission request (HS2-XHQ1EH), or an old approval.
    Approved,
    /// A person denied a permission request, or an old approval.
    Rejected,
    /// The shared policy was replaced.
    Changed,
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
    /// A stable action code such as `tickets_write` or `permission_answer`.
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    pub outcome: AuditOutcome,
    /// A stable reason code, never free text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// HS2-XHQ1EH: the permission request or remembered rule behind the answer. Older
    /// entries carried a retired grant id under `grant_id`, read here unchanged.
    #[serde(default, alias = "grant_id", skip_serializing_if = "Option::is_none")]
    pub permission_id: Option<String>,
}

/// HS2-SV4SMY: how long an identical observation read is recorded only once.
pub const AUDIT_COALESCE_MS: u64 = 10 * 60 * 1000;

/// HS2-SV4SMY: whether a record may be folded into an identical earlier one: an
/// observation (`gates_observe`) that was allowed or succeeded, outside any operation,
/// or a continued validation wait ([`continued_wait`]). Denials, failures and every
/// other action that acts on a target are always recorded.
pub fn coalescible(record: &AuditRecord) -> bool {
    continued_wait(record)
        || (record.action == "gates_observe"
            && record.operation_id.is_none()
            && matches!(
                record.outcome,
                AuditOutcome::Allowed | AuditOutcome::Succeeded
            ))
}

/// HS2-5ZT4GY: a `validation_run` request that continues an operation already
/// decided: each bounded wait on a running attempt passes the gate again (grant and
/// destination are re-checked every time) and is allowed as `continuing`. Recording
/// every such wait would fill the bounded journal during one long step.
pub fn continued_wait(record: &AuditRecord) -> bool {
    record.action == "validation_run"
        && record.operation_id.is_some()
        && record.outcome == AuditOutcome::Allowed
        && record.reason.as_deref() == Some("continuing")
}

/// The same actor's same action on the same target.
fn same_key(a: &AuditRecord, b: &AuditRecord) -> bool {
    a.actor_id == b.actor_id && a.target_id == b.target_id && a.action == b.action
}

/// `next` says what `previous` said, within [`AUDIT_COALESCE_MS`] of it.
fn identical(previous: &AuditRecord, next: &AuditRecord) -> bool {
    previous.outcome == next.outcome
        && previous.actor_role == next.actor_role
        && previous.reason == next.reason
        && previous.permission_id == next.permission_id
        && previous.operation_id == next.operation_id
        && next.at_ms >= previous.at_ms
        && next.at_ms - previous.at_ms < AUDIT_COALESCE_MS
}

/// HS2-SV4SMY: decides, entry by entry from the newest, whether a coalescible record
/// repeats what the journal already says about its subject. A success repeats when the
/// newest entry of its subject is the same success. An allowance repeats when the
/// newest entry of its subject is a success inside the window and the newest decision
/// before it is the same allowance. Anything else in between (a denial, a failure, a
/// different reason) means the record is written.
pub struct RepeatCheck<'a> {
    next: &'a AuditRecord,
    /// 0: looking for the newest entry of the subject; 1: an allowance looks for the
    /// newest decision; 2: decided.
    stage: std::cell::Cell<u8>,
}

impl<'a> RepeatCheck<'a> {
    pub fn new(next: &'a AuditRecord) -> Self {
        Self {
            next,
            stage: std::cell::Cell::new(if coalescible(next) { 0 } else { 2 }),
        }
    }

    /// True once the entries seen so far, newest first, make `next` a repeat.
    pub fn sees(&self, previous: &AuditRecord) -> bool {
        let next = self.next;
        if self.stage.get() == 2 || !same_key(previous, next) {
            return false;
        }
        let decided = |repeat: bool| {
            self.stage.set(2);
            repeat
        };
        // HS2-5ZT4GY: a continued wait repeats when the newest entry of its own
        // operation is the same continued wait; other operations' entries between them
        // do not count, so concurrent steps fold their waits independently.
        if continued_wait(next) {
            return if previous.operation_id == next.operation_id {
                decided(identical(previous, next))
            } else {
                false
            };
        }
        match (self.stage.get(), next.outcome) {
            (0, AuditOutcome::Succeeded) => decided(identical(previous, next)),
            (0, AuditOutcome::Allowed) => {
                let succeeded = previous.outcome == AuditOutcome::Succeeded
                    && previous.operation_id.is_none()
                    && next.at_ms >= previous.at_ms
                    && next.at_ms - previous.at_ms < AUDIT_COALESCE_MS;
                if succeeded {
                    self.stage.set(1);
                    false
                } else {
                    decided(false)
                }
            }
            (1, _) => match previous.outcome {
                AuditOutcome::Succeeded | AuditOutcome::Failed => false,
                _ => decided(identical(previous, next)),
            },
            _ => decided(false),
        }
    }
}

/// HS2-SV4SMY: whether `next` repeats the journal, given its entries newest first.
pub fn repeats(newest_first: &[AuditRecord], next: &AuditRecord) -> bool {
    let check = RepeatCheck::new(next);
    newest_first.iter().any(|previous| check.sees(previous))
}

/// HS2-7QHWHJ: what a journal stores. Each record type has its own directory under the
/// machine home and validates itself as identifiers and codes only.
pub trait JournalRecord: Serialize + serde::de::DeserializeOwned + Clone {
    const DIRECTORY: &'static str;
    fn validate(&self) -> Result<(), OrchestrationError>;
}

impl JournalRecord for AuditRecord {
    const DIRECTORY: &'static str = "orchestration-audit";
    fn validate(&self) -> Result<(), OrchestrationError> {
        validate(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound = "R: JournalRecord")]
pub struct JournalEntry<R> {
    pub seq: u64,
    #[serde(flatten)]
    pub record: R,
}

pub type AuditEntry = JournalEntry<AuditRecord>;
pub type AuditPage = JournalPage<AuditRecord>;
pub type AuditJournal = Journal<AuditRecord>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(bound = "R: JournalRecord")]
pub struct JournalPage<R> {
    pub entries: Vec<JournalEntry<R>>,
    /// Pass as `after` to continue; equals the request cursor when nothing is newer.
    pub cursor: u64,
    /// Retention removed entries between the request cursor and the first one returned.
    pub gap: bool,
    /// More retained entries exist beyond this page.
    pub more: bool,
}

#[derive(Clone)]
pub struct Journal<R> {
    directory: PathBuf,
    owner: String,
    segment_limit: u64,
    record: std::marker::PhantomData<fn() -> R>,
}

impl<R: JournalRecord> Journal<R> {
    pub fn new(home: &Path, owner_checkout_id: &str) -> Result<Self, OrchestrationError> {
        valid_identifier(owner_checkout_id)?;
        Ok(Self {
            directory: home.join(R::DIRECTORY),
            owner: owner_checkout_id.into(),
            segment_limit: MAX_AUDIT_SEGMENT_BYTES,
            record: std::marker::PhantomData,
        })
    }

    /// The directory a waiter watches to learn of a new entry without polling.
    pub fn directory(&self) -> &Path {
        &self.directory
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
    pub fn append(&self, record: R) -> Result<JournalEntry<R>, OrchestrationError> {
        self.append_unless(record, |_| false)
            .map(|(entry, _)| entry)
    }

    /// HS2-7QHWHJ: append unless a retained entry already says the same thing, decided
    /// under the journal lock so two reporters of one fact record it once. Returns the
    /// existing entry and `false` in that case.
    pub fn append_unless(
        &self,
        record: R,
        duplicate: impl Fn(&R) -> bool,
    ) -> Result<(JournalEntry<R>, bool), OrchestrationError> {
        record.validate()?;
        let _lock = self.lock()?;
        let mut retained = self.entries(&self.previous())?;
        retained.extend(self.entries(&self.current())?);
        if let Some(existing) = retained.iter().rev().find(|entry| duplicate(&entry.record)) {
            return Ok((existing.clone(), false));
        }
        let seq = retained.last().map_or(0, |entry| entry.seq) + 1;
        let entry = JournalEntry { seq, record };
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
        Ok((entry, true))
    }

    /// Entries with `seq > after`, oldest first, at most `limit` (capped).
    pub fn read(&self, after: u64, limit: usize) -> Result<JournalPage<R>, OrchestrationError> {
        self.read_matching(after, limit, |_| true)
    }

    /// Entries after the cursor that `keep` accepts. The cursor still advances past
    /// entries that were filtered out, so a filtered reader never re-reads them.
    pub fn read_matching(
        &self,
        after: u64,
        limit: usize,
        keep: impl Fn(&R) -> bool,
    ) -> Result<JournalPage<R>, OrchestrationError> {
        let limit = limit.clamp(1, MAX_AUDIT_PAGE);
        let mut retained = self.entries(&self.previous())?;
        retained.extend(self.entries(&self.current())?);
        let first_retained = retained.first().map(|entry| entry.seq);
        let last_retained = retained.last().map(|entry| entry.seq);
        let mut newer: Vec<_> = retained
            .into_iter()
            .filter(|entry| entry.seq > after && keep(&entry.record))
            .collect();
        let more = newer.len() > limit;
        newer.truncate(limit);
        if !more && newer.len() < limit {
            // Nothing further matches: resume after everything that was examined.
            return Ok(JournalPage {
                gap: first_retained.is_some_and(|first| first > after + 1),
                cursor: last_retained.map_or(after, |last| last.max(after)),
                more,
                entries: newer,
            });
        }
        Ok(JournalPage {
            gap: first_retained.is_some_and(|first| first > after + 1),
            cursor: newer.last().map_or(after, |entry| entry.seq),
            more,
            entries: newer,
        })
    }

    /// Parsed complete lines of one segment. A torn or foreign line is skipped, never
    /// rewritten, so a crash mid-append cannot hide earlier history.
    fn entries(&self, path: &Path) -> Result<Vec<JournalEntry<R>>, OrchestrationError> {
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
        let mut entries: Vec<JournalEntry<R>> = Vec::new();
        for line in bytes.split(|byte| *byte == b'\n') {
            let Ok(entry) = serde_json::from_slice::<JournalEntry<R>>(line) else {
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
    for value in [&record.target_id, &record.permission_id]
        .into_iter()
        .flatten()
    {
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
