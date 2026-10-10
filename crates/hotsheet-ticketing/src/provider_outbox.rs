//! Durable, provider-neutral admission log for provisional external ticket updates.
//!
//! The provider remains authoritative. This database contains ticket snapshots and
//! mutation intent, never provider credentials. Admission is one SQLite IMMEDIATE
//! transaction so retries, capacity checks, and per-ticket ordering agree across
//! processes. The database is separate from the disposable ticket search index.

use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{ApiTicket, ProviderPatch};

#[derive(Debug, Error)]
pub enum OutboxError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("outbox operation id, connection id, and native ticket id must be nonempty")]
    EmptyIdentity,
    #[error("operation {0} was retried with a different mutation")]
    ChangedPayload(String),
    #[error("operation {0} is older than the outbox retry window")]
    ExpiredOperation(String),
    #[error("operation {0} has an invalid or future-dated v2 id")]
    InvalidOperationId(String),
    #[error(
        "outbox has {pending} pending operations; admitting {requested} would exceed its limit of {max}"
    )]
    Backpressure {
        max: usize,
        pending: usize,
        requested: usize,
    },
    #[error(
        "the base ticket's connection, native id, or concurrency token does not match the intent"
    )]
    MismatchedBase,
    #[error("outbox pending limit must be positive")]
    InvalidCapacity,
    #[error("pending operation {0} does not match the projected ticket or is out of order")]
    InvalidProjectionOrder(String),
    #[error("pending operation {operation_id} changes unsupported field {field}")]
    UnsupportedProjectionField {
        operation_id: String,
        field: &'static str,
    },
    #[error("projected ticket must be a JSON object with connection_id and native_id")]
    InvalidProjectionTicket,
    #[error("operation {0} is not in a state that allows this transition")]
    InvalidTransition(String),
}

/// One caller-supplied stable operation id and the provider ticket version it edits.
/// Construct this from an `ApiTicket` so no credential bundle enters the outbox.
#[derive(Debug, Clone)]
pub struct OutboxAdmission {
    pub operation_id: String,
    pub connection_id: String,
    pub native_id: String,
    pub base_token: Option<String>,
    pub base_ticket: Value,
    pub patch: ProviderPatch,
}

impl OutboxAdmission {
    pub fn new(
        operation_id: impl Into<String>,
        base_ticket: &ApiTicket,
        mut patch: ProviderPatch,
    ) -> Result<Self, OutboxError> {
        if patch.expected_token.is_none() {
            patch.expected_token = base_ticket.concurrency_token.clone();
        }
        Ok(Self {
            operation_id: operation_id.into(),
            connection_id: base_ticket.connection_id.clone(),
            native_id: base_ticket.native_id.clone(),
            base_token: base_ticket.concurrency_token.clone(),
            base_ticket: serde_json::to_value(base_ticket)?,
            patch,
        })
    }

    fn validate(&self) -> Result<(), OutboxError> {
        if self.operation_id.trim().is_empty()
            || self.connection_id.trim().is_empty()
            || self.native_id.trim().is_empty()
        {
            return Err(OutboxError::EmptyIdentity);
        }
        if self
            .base_ticket
            .get("connection_id")
            .and_then(Value::as_str)
            != Some(self.connection_id.as_str())
            || self.base_ticket.get("native_id").and_then(Value::as_str)
                != Some(self.native_id.as_str())
            || self.base_ticket.get("concurrency_token")
                != Some(
                    &self
                        .base_token
                        .as_ref()
                        .map_or(Value::Null, |token| Value::String(token.clone())),
                )
            || self.patch.expected_token != self.base_token
        {
            return Err(OutboxError::MismatchedBase);
        }
        Ok(())
    }

    fn payload_json(&self) -> Result<String, OutboxError> {
        Ok(serde_json::to_string(&Payload {
            connection_id: &self.connection_id,
            native_id: &self.native_id,
            base_token: &self.base_token,
            base_ticket: &self.base_ticket,
            patch: &self.patch,
        })?)
    }
}

#[derive(Serialize)]
struct Payload<'a> {
    connection_id: &'a str,
    native_id: &'a str,
    base_token: &'a Option<String>,
    base_ticket: &'a Value,
    patch: &'a ProviderPatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutboxState {
    Queued,
    Confirmed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DispatchState {
    Queued,
    Sending,
    RateLimited,
    NeedsAttention,
    Confirmed,
    Discarded,
}

impl DispatchState {
    fn from_db(value: &str) -> Self {
        match value {
            "sending" => Self::Sending,
            "rate_limited" => Self::RateLimited,
            "needs_attention" => Self::NeedsAttention,
            "confirmed" => Self::Confirmed,
            "discarded" => Self::Discarded,
            _ => Self::Queued,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxOperation {
    pub operation_id: String,
    pub connection_id: String,
    pub native_id: String,
    pub sequence: i64,
    pub base_token: Option<String>,
    pub base_ticket: Value,
    pub patch: ProviderPatch,
    pub state: OutboxState,
    pub dispatch_state: DispatchState,
    pub attempts: i64,
    pub next_attempt_at: i64,
    pub last_error: Option<String>,
    pub conflict: Option<Value>,
}

/// Project Jira's supported provisional fields onto an authoritative full ticket.
/// Callers pass pending operations in per-ticket sequence order. This preserves the
/// remote concurrency token; the returned `pending_operation_ids` array explicitly
/// marks the result as provisional. Empty input returns the remote ticket unchanged.
pub fn project_pending_ticket(
    remote: &Value,
    pending: &[OutboxOperation],
) -> Result<Value, OutboxError> {
    let connection = remote
        .get("connection_id")
        .and_then(Value::as_str)
        .ok_or(OutboxError::InvalidProjectionTicket)?;
    let native = remote
        .get("native_id")
        .and_then(Value::as_str)
        .ok_or(OutboxError::InvalidProjectionTicket)?;
    let mut projected = remote.clone();
    let object = projected
        .as_object_mut()
        .ok_or(OutboxError::InvalidProjectionTicket)?;
    if pending.is_empty() {
        return Ok(projected);
    }
    let mut previous_sequence = 0;
    let mut ids = Vec::with_capacity(pending.len());
    for operation in pending {
        if operation.connection_id != connection
            || operation.native_id != native
            || operation.state != OutboxState::Queued
            || operation.sequence <= previous_sequence
        {
            return Err(OutboxError::InvalidProjectionOrder(
                operation.operation_id.clone(),
            ));
        }
        let patch = &operation.patch;
        let unsupported = [
            ("status", patch.status.is_some()),
            ("started_phase", patch.started_phase.is_some()),
            ("up_next", patch.up_next.is_some()),
            ("blocked_by", patch.blocked_by.is_some()),
            ("blocked_reason", patch.blocked_reason.is_some()),
        ];
        if let Some((field, _)) = unsupported.into_iter().find(|(_, set)| *set) {
            return Err(OutboxError::UnsupportedProjectionField {
                operation_id: operation.operation_id.clone(),
                field,
            });
        }
        if let Some(title) = &patch.title {
            object.insert("title".into(), Value::String(title.clone()));
        }
        if let Some(details) = &patch.details {
            object.insert("details".into(), Value::String(details.clone()));
        }
        if let Some(category) = &patch.category {
            object.insert("category".into(), Value::String(category.clone()));
        }
        if let Some(priority) = patch.priority {
            object.insert("priority".into(), serde_json::to_value(priority)?);
        }
        if let Some(tags) = &patch.tags {
            object.insert("tags".into(), serde_json::to_value(tags)?);
        }
        ids.push(Value::String(operation.operation_id.clone()));
        previous_sequence = operation.sequence;
    }
    object.insert("pending_operation_ids".into(), Value::Array(ids));
    Ok(projected)
}

/// A persistent queue with a bound on unconfirmed intent, safe to reopen after a crash.
pub struct ProviderOutbox {
    db: Connection,
    max_pending: usize,
}

/// Full settled payloads remain available for diagnostics and exact retry responses for 30 days.
/// Compact rows retain exact retry evidence until a durable admission floor fences them.
const SETTLED_PAYLOAD_RETENTION: i64 = 30 * 24 * 60 * 60;
const LEGACY_ADMISSION_GRACE: i64 = 30 * 24 * 60 * 60;
const FUTURE_ID_SKEW_MS: i64 = 5 * 60 * 1000;

fn operation_timestamp_ms(id: &str) -> Option<i64> {
    let (timestamp, random) = id.strip_prefix("v2:")?.split_once(':')?;
    if timestamp.len() != 13 || !timestamp.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    if !(16..=80).contains(&random.len())
        || !random
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return None;
    }
    timestamp.parse().ok()
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn payload_digest(payload: &str) -> String {
    format!("{:x}", Sha256::digest(payload.as_bytes()))
}

impl ProviderOutbox {
    pub fn open(path: impl AsRef<Path>, max_pending: usize) -> Result<Self, OutboxError> {
        if max_pending == 0 {
            return Err(OutboxError::InvalidCapacity);
        }
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent)?;
        }
        let db = Connection::open(path)?;
        db.busy_timeout(Duration::from_secs(5))?;
        db.pragma_update(None, "journal_mode", "WAL")?;
        db.pragma_update(None, "synchronous", "FULL")?;
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS provider_outbox (
                operation_id TEXT PRIMARY KEY,
                connection_id TEXT NOT NULL,
                native_id TEXT NOT NULL,
                sequence INTEGER NOT NULL,
                base_token TEXT,
                base_ticket_json TEXT NOT NULL,
                patch_json TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                state TEXT NOT NULL CHECK (state IN ('queued', 'confirmed')),
                dispatch_state TEXT NOT NULL DEFAULT 'queued',
                attempts INTEGER NOT NULL DEFAULT 0,
                next_attempt_at INTEGER NOT NULL DEFAULT 0,
                last_error TEXT,
                conflict_json TEXT,
                settled_at INTEGER,
                payload_digest TEXT,
                compacted INTEGER NOT NULL DEFAULT 0,
                id_version INTEGER NOT NULL DEFAULT 1,
                UNIQUE (connection_id, native_id, sequence)
            );
            CREATE INDEX IF NOT EXISTS provider_outbox_pending
                ON provider_outbox (state, connection_id, native_id, sequence);
            CREATE TABLE IF NOT EXISTS provider_outbox_sequence (
                connection_id TEXT NOT NULL,
                native_id TEXT NOT NULL,
                last_sequence INTEGER NOT NULL,
                PRIMARY KEY (connection_id, native_id)
            );
            CREATE TABLE IF NOT EXISTS provider_outbox_retention (
                singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
                floor_ms INTEGER NOT NULL,
                legacy_cutoff_ms INTEGER NOT NULL
            );",
        )?;
        db.execute(
            "INSERT INTO provider_outbox_sequence (connection_id, native_id, last_sequence)
             SELECT connection_id, native_id, MAX(sequence) FROM provider_outbox
             GROUP BY connection_id, native_id
             ON CONFLICT (connection_id, native_id) DO UPDATE SET
               last_sequence = MAX(last_sequence, excluded.last_sequence)",
            [],
        )?;
        db.execute(
            "INSERT OR IGNORE INTO provider_outbox_retention
             (singleton, floor_ms, legacy_cutoff_ms) VALUES (1, 0, ?1)",
            [unix_now()
                .saturating_add(LEGACY_ADMISSION_GRACE)
                .saturating_mul(1000)],
        )?;
        for (name, definition) in [
            ("dispatch_state", "TEXT NOT NULL DEFAULT 'queued'"),
            ("attempts", "INTEGER NOT NULL DEFAULT 0"),
            ("next_attempt_at", "INTEGER NOT NULL DEFAULT 0"),
            ("last_error", "TEXT"),
            ("conflict_json", "TEXT"),
            ("settled_at", "INTEGER"),
            ("payload_digest", "TEXT"),
            ("compacted", "INTEGER NOT NULL DEFAULT 0"),
            ("id_version", "INTEGER NOT NULL DEFAULT 1"),
        ] {
            let present: bool = db.query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info('provider_outbox') WHERE name = ?1)",
                [name],
                |row| row.get(0),
            )?;
            if !present {
                db.execute_batch(&format!(
                    "ALTER TABLE provider_outbox ADD COLUMN {name} {definition}"
                ))?;
            }
        }
        db.execute_batch(
            "UPDATE provider_outbox SET dispatch_state = 'confirmed'
             WHERE state = 'confirmed' AND dispatch_state = 'queued';
             UPDATE provider_outbox SET dispatch_state = 'queued'
             WHERE state = 'queued' AND dispatch_state = 'sending';",
        )?;
        db.execute(
            "UPDATE provider_outbox SET settled_at = ?1
             WHERE state = 'confirmed' AND settled_at IS NULL",
            [unix_now()],
        )?;
        db.execute_batch(
            "CREATE INDEX IF NOT EXISTS provider_outbox_compaction
             ON provider_outbox (state, compacted, settled_at);",
        )?;
        let mut outbox = Self { db, max_pending };
        outbox.compact_terminal_before(unix_now() - SETTLED_PAYLOAD_RETENTION)?;
        outbox.prune_terminal_rows()?;
        Ok(outbox)
    }

    /// Atomically admit a batch. An identical operation id returns its existing row;
    /// a changed retry rejects the whole batch. Ordering includes confirmed history.
    pub fn admit_batch(
        &mut self,
        admissions: &[OutboxAdmission],
    ) -> Result<Vec<OutboxOperation>, OutboxError> {
        self.compact_terminal_before(unix_now() - SETTLED_PAYLOAD_RETENTION)?;
        self.prune_terminal_rows()?;
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let pending: usize = tx.query_row(
            "SELECT COUNT(*) FROM provider_outbox WHERE state = 'queued'",
            [],
            |row| row.get(0),
        )?;
        let mut added = 0;
        let mut accepted = Vec::with_capacity(admissions.len());
        for admission in admissions {
            admission.validate()?;
            let payload = admission.payload_json()?;
            let existing = tx
                .query_row(
                    "SELECT payload_json, payload_digest, compacted
                     FROM provider_outbox WHERE operation_id = ?1",
                    [&admission.operation_id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, Option<String>>(1)?,
                            row.get::<_, bool>(2)?,
                        ))
                    },
                )
                .optional()?;
            if let Some((existing, digest, compacted)) = existing {
                if compacted {
                    if digest.as_deref() != Some(payload_digest(&payload).as_str()) {
                        return Err(OutboxError::ChangedPayload(admission.operation_id.clone()));
                    }
                    return Err(OutboxError::ExpiredOperation(
                        admission.operation_id.clone(),
                    ));
                }
                if existing != payload {
                    return Err(OutboxError::ChangedPayload(admission.operation_id.clone()));
                }
                accepted.push(get_in(&tx, &admission.operation_id)?.expect("row just read"));
                continue;
            }
            let (floor_ms, legacy_cutoff_ms): (i64, i64) = tx.query_row(
                "SELECT floor_ms, legacy_cutoff_ms FROM provider_outbox_retention WHERE singleton = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            let now_ms = unix_now().saturating_mul(1000);
            if admission.operation_id.starts_with("v2:") {
                let timestamp =
                    operation_timestamp_ms(&admission.operation_id).ok_or_else(|| {
                        OutboxError::InvalidOperationId(admission.operation_id.clone())
                    })?;
                if timestamp > now_ms.saturating_add(FUTURE_ID_SKEW_MS) {
                    return Err(OutboxError::InvalidOperationId(
                        admission.operation_id.clone(),
                    ));
                }
                if timestamp < floor_ms {
                    return Err(OutboxError::ExpiredOperation(
                        admission.operation_id.clone(),
                    ));
                }
            } else if now_ms >= legacy_cutoff_ms {
                return Err(OutboxError::ExpiredOperation(
                    admission.operation_id.clone(),
                ));
            }
            if pending + added >= self.max_pending {
                return Err(OutboxError::Backpressure {
                    max: self.max_pending,
                    pending,
                    requested: added + 1,
                });
            }
            let sequence: i64 = tx
                .query_row(
                    "SELECT COALESCE(last_sequence, 0) + 1 FROM provider_outbox_sequence
                 WHERE connection_id = ?1 AND native_id = ?2",
                    params![admission.connection_id, admission.native_id],
                    |row| row.get(0),
                )
                .optional()?
                .unwrap_or(1);
            tx.execute(
                "INSERT INTO provider_outbox_sequence (connection_id, native_id, last_sequence)
                 VALUES (?1, ?2, ?3) ON CONFLICT (connection_id, native_id)
                 DO UPDATE SET last_sequence = excluded.last_sequence",
                params![admission.connection_id, admission.native_id, sequence],
            )?;
            let id_version = if admission.operation_id.starts_with("v2:") {
                2
            } else {
                1
            };
            tx.execute(
                "INSERT INTO provider_outbox
                 (operation_id, connection_id, native_id, sequence, base_token,
                  base_ticket_json, patch_json, payload_json, state, id_version)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'queued', ?9)",
                params![
                    admission.operation_id,
                    admission.connection_id,
                    admission.native_id,
                    sequence,
                    admission.base_token,
                    serde_json::to_string(&admission.base_ticket)?,
                    serde_json::to_string(&admission.patch)?,
                    payload,
                    id_version,
                ],
            )?;
            accepted.push(get_in(&tx, &admission.operation_id)?.expect("row just inserted"));
            added += 1;
        }
        tx.commit()?;
        Ok(accepted)
    }

    pub fn get(&self, operation_id: &str) -> Result<Option<OutboxOperation>, OutboxError> {
        get_in(&self.db, operation_id)
    }

    /// Replace old terminal snapshots with a permanent compact retry fence. Pending rows are
    /// never touched; `sequence` and the operation id remain, including through restarts.
    pub fn compact_terminal_before(&mut self, cutoff: i64) -> Result<usize, OutboxError> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let rows = {
            let mut statement = tx.prepare(
                "SELECT operation_id, payload_json FROM provider_outbox
                 WHERE state = 'confirmed' AND compacted = 0 AND settled_at <= ?1",
            )?;
            statement
                .query_map([cutoff], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?
        };
        for (id, payload) in &rows {
            tx.execute(
                "UPDATE provider_outbox SET base_token = NULL, base_ticket_json = 'null',
                 patch_json = 'null', payload_json = '', payload_digest = ?2,
                 last_error = NULL, conflict_json = NULL, compacted = 1
                 WHERE operation_id = ?1",
                params![id, payload_digest(payload)],
            )?;
        }
        tx.commit()?;
        Ok(rows.len())
    }

    /// Advance the durable admission floor before removing old settled rows. Existing
    /// queued IDs remain addressable even if their timestamps are below the floor.
    pub fn prune_terminal_rows(&mut self) -> Result<usize, OutboxError> {
        let now = unix_now();
        let floor = now
            .saturating_sub(SETTLED_PAYLOAD_RETENTION)
            .saturating_mul(1000);
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "UPDATE provider_outbox_retention SET floor_ms = MAX(floor_ms, ?1)
             WHERE singleton = 1",
            [floor],
        )?;
        let (saved_floor, legacy_cutoff): (i64, i64) = tx.query_row(
            "SELECT floor_ms, legacy_cutoff_ms FROM provider_outbox_retention WHERE singleton = 1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let count = tx.execute(
            "DELETE FROM provider_outbox WHERE state = 'confirmed' AND compacted = 1
             AND settled_at <= ?1 AND
             (CASE WHEN id_version = 2 THEN
                CAST(substr(operation_id, 4, 13) AS INTEGER) < ?2
              ELSE ?3 >= ?4 END)",
            params![
                now.saturating_sub(SETTLED_PAYLOAD_RETENTION),
                saved_floor,
                now.saturating_mul(1000),
                legacy_cutoff
            ],
        )?;
        tx.commit()?;
        Ok(count)
    }

    pub fn pending_count(&self) -> Result<usize, OutboxError> {
        Ok(self.db.query_row(
            "SELECT COUNT(*) FROM provider_outbox WHERE state = 'queued'",
            [],
            |row| row.get(0),
        )?)
    }

    /// Replay order is stable across restarts, ordered by ticket then sequence.
    pub fn list_pending(&self) -> Result<Vec<OutboxOperation>, OutboxError> {
        list_in(
            &self.db,
            "SELECT operation_id FROM provider_outbox WHERE state = 'queued'
             ORDER BY connection_id, native_id, sequence",
            [],
        )
    }

    pub fn pending_for_ticket(
        &self,
        connection_id: &str,
        native_id: &str,
    ) -> Result<Vec<OutboxOperation>, OutboxError> {
        list_in(
            &self.db,
            "SELECT operation_id FROM provider_outbox
             WHERE state = 'queued' AND connection_id = ?1 AND native_id = ?2
             ORDER BY sequence",
            params![connection_id, native_id],
        )
    }

    pub fn recent_for_connection(
        &self,
        connection_id: &str,
        limit: usize,
    ) -> Result<Vec<OutboxOperation>, OutboxError> {
        list_in(
            &self.db,
            "SELECT operation_id FROM provider_outbox
             WHERE connection_id = ?1 AND (
               state = 'queued' OR rowid IN (
                 SELECT rowid FROM provider_outbox
                 WHERE connection_id = ?1 AND state = 'confirmed' AND compacted = 0
                 ORDER BY rowid DESC LIMIT ?2
               )
             ) ORDER BY rowid DESC",
            params![connection_id, limit as i64],
        )
    }

    /// Future dispatch can mark an acknowledged intent without losing its idempotency key.
    /// Repeating confirmation is a no-op; an unknown id returns false.
    pub fn confirm(&mut self, operation_id: &str) -> Result<bool, OutboxError> {
        let changed = self.db.execute(
            "UPDATE provider_outbox SET state = 'confirmed', dispatch_state = 'confirmed',
             next_attempt_at = 0, last_error = NULL, settled_at = ?2
             WHERE operation_id = ?1 AND state = 'queued' AND dispatch_state = 'sending'",
            params![operation_id, unix_now()],
        )?;
        Ok(changed > 0)
    }

    /// A readback can prove an uncertain write landed even after dispatch was reset.
    pub fn settle_applied(&mut self, operation_id: &str) -> Result<bool, OutboxError> {
        let changed = self.db.execute(
            "UPDATE provider_outbox SET state = 'confirmed', dispatch_state = 'confirmed',
             next_attempt_at = 0, last_error = NULL, settled_at = ?2
             WHERE operation_id = ?1 AND state = 'queued' AND dispatch_state != 'sending'",
            params![operation_id, unix_now()],
        )?;
        Ok(changed > 0)
    }

    /// Claim only the first pending intent for each ticket. A transaction excludes a second
    /// dispatcher and keeps later local intent behind an attention state.
    pub fn claim_ready(
        &mut self,
        now: i64,
        limit: usize,
    ) -> Result<Vec<OutboxOperation>, OutboxError> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let ids = list_in(
            &tx,
            "SELECT operation_id FROM provider_outbox AS candidate
             WHERE state = 'queued'
               AND dispatch_state IN ('queued', 'rate_limited')
               AND next_attempt_at <= ?1
               AND NOT EXISTS (
                 SELECT 1 FROM provider_outbox AS earlier
                 WHERE earlier.connection_id = candidate.connection_id
                   AND earlier.native_id = candidate.native_id
                   AND earlier.state = 'queued' AND earlier.sequence < candidate.sequence
               )
             ORDER BY sequence LIMIT ?2",
            params![now, limit as i64],
        )?;
        let mut claimed = Vec::with_capacity(ids.len());
        for operation in ids {
            tx.execute(
                "UPDATE provider_outbox SET dispatch_state = 'sending', attempts = attempts + 1
                 WHERE operation_id = ?1",
                [&operation.operation_id],
            )?;
            claimed.push(get_in(&tx, &operation.operation_id)?.expect("claimed row exists"));
        }
        tx.commit()?;
        Ok(claimed)
    }

    pub fn defer(
        &mut self,
        operation_id: &str,
        until: i64,
        error: &str,
        rate_limited: bool,
    ) -> Result<(), OutboxError> {
        let state = if rate_limited {
            "rate_limited"
        } else {
            "queued"
        };
        let changed = self.db.execute(
            "UPDATE provider_outbox SET dispatch_state = ?2, next_attempt_at = ?3,
             last_error = ?4 WHERE operation_id = ?1 AND state = 'queued'
             AND dispatch_state = 'sending'",
            params![operation_id, state, until, error],
        )?;
        if changed == 0 {
            return Err(OutboxError::InvalidTransition(operation_id.into()));
        }
        Ok(())
    }

    pub fn needs_attention(
        &mut self,
        operation_id: &str,
        error: &str,
        conflict: Option<&Value>,
    ) -> Result<(), OutboxError> {
        let changed = self.db.execute(
            "UPDATE provider_outbox SET dispatch_state = 'needs_attention', last_error = ?2,
             conflict_json = ?3 WHERE operation_id = ?1 AND state = 'queued'
             AND dispatch_state = 'sending'",
            params![
                operation_id,
                error,
                conflict.map(serde_json::to_string).transpose()?
            ],
        )?;
        if changed == 0 {
            return Err(OutboxError::InvalidTransition(operation_id.into()));
        }
        Ok(())
    }

    pub fn retry(&mut self, operation_id: &str) -> Result<(), OutboxError> {
        let changed = self.db.execute(
            "UPDATE provider_outbox SET dispatch_state = 'queued', next_attempt_at = 0,
             last_error = NULL WHERE operation_id = ?1 AND state = 'queued'
             AND dispatch_state = 'needs_attention'",
            [operation_id],
        )?;
        if changed == 0 {
            return Err(OutboxError::InvalidTransition(operation_id.into()));
        }
        Ok(())
    }

    pub fn discard(&mut self, operation_id: &str) -> Result<(), OutboxError> {
        let changed = self.db.execute(
            "UPDATE provider_outbox SET state = 'confirmed', dispatch_state = 'discarded',
             settled_at = ?2
             WHERE operation_id = ?1 AND state = 'queued'
             AND dispatch_state IN ('queued', 'rate_limited', 'needs_attention')",
            params![operation_id, unix_now()],
        )?;
        if changed == 0 {
            return Err(OutboxError::InvalidTransition(operation_id.into()));
        }
        Ok(())
    }

    pub fn defer_connection(&mut self, connection_id: &str, until: i64) -> Result<(), OutboxError> {
        self.db.execute(
            "UPDATE provider_outbox SET next_attempt_at = MAX(next_attempt_at, ?2),
             dispatch_state = 'rate_limited' WHERE connection_id = ?1 AND state = 'queued'
             AND dispatch_state IN ('queued', 'rate_limited')",
            params![connection_id, until],
        )?;
        Ok(())
    }

    pub fn record_conflict(
        &mut self,
        operation_id: &str,
        conflict: &Value,
    ) -> Result<(), OutboxError> {
        let changed = self.db.execute(
            "UPDATE provider_outbox SET conflict_json = ?2 WHERE operation_id = ?1
             AND state = 'queued' AND dispatch_state = 'sending'",
            params![operation_id, serde_json::to_string(conflict)?],
        )?;
        if changed == 0 {
            return Err(OutboxError::InvalidTransition(operation_id.into()));
        }
        Ok(())
    }
}

fn get_in(db: &Connection, operation_id: &str) -> Result<Option<OutboxOperation>, OutboxError> {
    let row = db
        .query_row(
            "SELECT operation_id, connection_id, native_id, sequence, base_token,
                    base_ticket_json, patch_json, state, dispatch_state, attempts,
                    next_attempt_at, last_error, conflict_json, compacted
             FROM provider_outbox WHERE operation_id = ?1",
            [operation_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, i64>(9)?,
                    row.get::<_, i64>(10)?,
                    row.get::<_, Option<String>>(11)?,
                    row.get::<_, Option<String>>(12)?,
                    row.get::<_, bool>(13)?,
                ))
            },
        )
        .optional()?;
    let Some((
        operation_id,
        connection_id,
        native_id,
        sequence,
        base_token,
        base_json,
        patch_json,
        state,
        dispatch_state,
        attempts,
        next_attempt_at,
        last_error,
        conflict_json,
        compacted,
    )) = row
    else {
        return Ok(None);
    };
    if compacted {
        return Ok(None);
    }
    Ok(Some(OutboxOperation {
        operation_id,
        connection_id,
        native_id,
        sequence,
        base_token,
        base_ticket: serde_json::from_str(&base_json)?,
        patch: serde_json::from_str(&patch_json)?,
        state: if state == "queued" {
            OutboxState::Queued
        } else {
            OutboxState::Confirmed
        },
        dispatch_state: DispatchState::from_db(&dispatch_state),
        attempts,
        next_attempt_at,
        last_error,
        conflict: conflict_json
            .map(|value| serde_json::from_str(&value))
            .transpose()?,
    }))
}

fn list_in<P: rusqlite::Params>(
    db: &Connection,
    query: &str,
    params: P,
) -> Result<Vec<OutboxOperation>, OutboxError> {
    let mut statement = db.prepare(query)?;
    let ids = statement
        .query_map(params, |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    ids.into_iter()
        .map(|id| Ok(get_in(db, &id)?.expect("selected row exists")))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn admission(id: &str, ticket: &str, title: &str) -> OutboxAdmission {
        OutboxAdmission {
            operation_id: id.into(),
            connection_id: "github-main".into(),
            native_id: ticket.into(),
            base_token: Some("etag-1".into()),
            base_ticket: json!({
                "connection_id": "github-main",
                "native_id": ticket,
                "concurrency_token": "etag-1",
                "title": "Before",
            }),
            patch: ProviderPatch {
                expected_token: Some("etag-1".into()),
                title: Some(title.into()),
                ..ProviderPatch::default()
            },
        }
    }

    #[test]
    fn batch_is_atomic_and_backpressure_counts_only_new_pending_intents() {
        let dir = tempfile::tempdir().unwrap();
        let mut outbox = ProviderOutbox::open(dir.path().join("outbox.sqlite"), 2).unwrap();
        let first = admission("one", "42", "First");
        let second = admission("two", "42", "Second");
        let result = outbox
            .admit_batch(&[first.clone(), second.clone()])
            .unwrap();
        assert_eq!(
            result.iter().map(|item| item.sequence).collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(
            outbox.admit_batch(std::slice::from_ref(&first)).unwrap(),
            vec![result[0].clone()]
        );
        assert_eq!(outbox.pending_count().unwrap(), 2);

        let error = outbox
            .admit_batch(&[first, admission("three", "43", "Third")])
            .unwrap_err();
        assert!(matches!(error, OutboxError::Backpressure { .. }));
        assert!(outbox.get("three").unwrap().is_none());
        assert_eq!(
            outbox.pending_for_ticket("github-main", "42").unwrap(),
            result
        );
    }

    #[test]
    fn conflicting_retry_rolls_back_other_batch_insertions() {
        let mut outbox = ProviderOutbox::open(":memory:", 4).unwrap();
        let first = admission("stable", "42", "Original");
        outbox.admit_batch(&[first]).unwrap();
        let error = outbox
            .admit_batch(&[
                admission("new", "43", "New"),
                admission("stable", "42", "Changed"),
            ])
            .unwrap_err();
        assert!(matches!(error, OutboxError::ChangedPayload(id) if id == "stable"));
        assert!(outbox.get("new").unwrap().is_none());
        assert_eq!(outbox.pending_count().unwrap(), 1);
    }

    #[test]
    fn reopening_replays_pending_rows_and_preserves_sequence_after_confirmation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("provider-outbox.sqlite");
        {
            let mut outbox = ProviderOutbox::open(&path, 2).unwrap();
            outbox.admit_batch(&[admission("one", "42", "A")]).unwrap();
            assert_eq!(outbox.claim_ready(0, 2).unwrap().len(), 1);
            assert!(outbox.confirm("one").unwrap());
            assert!(!outbox.confirm("one").unwrap());
            outbox.admit_batch(&[admission("two", "42", "B")]).unwrap();
        }
        let mut reopened = ProviderOutbox::open(&path, 2).unwrap();
        let replay = reopened.list_pending().unwrap();
        assert_eq!(replay.len(), 1);
        assert_eq!(replay[0].operation_id, "two");
        assert_eq!(replay[0].sequence, 2);
        assert_eq!(replay[0].base_ticket["title"], "Before");
        assert_eq!(replay[0].patch.title.as_deref(), Some("B"));
        assert_eq!(
            reopened.get("one").unwrap().unwrap().state,
            OutboxState::Confirmed
        );
        assert_eq!(
            reopened
                .admit_batch(&[admission("one", "42", "A")])
                .unwrap()[0]
                .state,
            OutboxState::Confirmed
        );
        assert_eq!(
            reopened
                .admit_batch(&[admission("three", "42", "C")])
                .unwrap()[0]
                .sequence,
            3
        );
    }

    #[test]
    fn compacted_terminal_rows_fence_late_retries_and_preserve_sequence_after_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("outbox.sqlite");
        let mut outbox = ProviderOutbox::open(&path, 4).unwrap();
        let settled = admission("settled", "42", "First");
        let discarded = admission("discarded", "42", "Second");
        let pending = admission("pending", "42", "Third");
        outbox
            .admit_batch(&[settled.clone(), discarded.clone(), pending.clone()])
            .unwrap();
        assert_eq!(outbox.claim_ready(0, 1).unwrap().len(), 1);
        assert!(outbox.confirm("settled").unwrap());
        outbox.discard("discarded").unwrap();
        // A one-second margin races the clock when the setup crosses a second boundary.
        assert_eq!(outbox.compact_terminal_before(unix_now() - 60).unwrap(), 0);
        assert_eq!(outbox.compact_terminal_before(unix_now() + 60).unwrap(), 2);
        assert_eq!(outbox.compact_terminal_before(unix_now() + 60).unwrap(), 0);
        assert!(outbox.get("settled").unwrap().is_none());
        assert!(outbox.get("discarded").unwrap().is_none());
        assert_eq!(
            outbox
                .recent_for_connection("github-main", 10)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(outbox.list_pending().unwrap()[0].operation_id, "pending");
        let (base, patch, payload, digest): (String, String, String, String) = outbox
            .db
            .query_row(
                "SELECT base_ticket_json, patch_json, payload_json, payload_digest
                 FROM provider_outbox WHERE operation_id = 'settled'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            (base.as_str(), patch.as_str(), payload.as_str()),
            ("null", "null", "")
        );
        assert_eq!(digest.len(), 64);
        drop(outbox);

        let mut reopened = ProviderOutbox::open(&path, 4).unwrap();
        for prior in [settled, discarded] {
            assert!(matches!(
                reopened.admit_batch(std::slice::from_ref(&prior)),
                Err(OutboxError::ExpiredOperation(id)) if id == prior.operation_id
            ));
        }
        assert!(matches!(
            reopened.admit_batch(&[admission("settled", "42", "Changed")]),
            Err(OutboxError::ChangedPayload(id)) if id == "settled"
        ));
        assert!(matches!(
            reopened.admit_batch(&[
                admission("never-admitted", "43", "New"),
                admission("settled", "42", "First"),
            ]),
            Err(OutboxError::ExpiredOperation(id)) if id == "settled"
        ));
        assert!(reopened.get("never-admitted").unwrap().is_none());
        assert_eq!(reopened.pending_count().unwrap(), 1);
        assert_eq!(
            reopened
                .admit_batch(&[admission("new", "42", "Fourth")])
                .unwrap()[0]
                .sequence,
            4
        );
        assert_eq!(reopened.list_pending().unwrap().len(), 2);
    }

    #[test]
    fn timestamp_floor_prunes_settled_ids_but_preserves_pending_and_sequence_after_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("outbox.sqlite");
        let mut outbox = ProviderOutbox::open(&path, 4).unwrap();
        let old_ms = (unix_now() - 20 * 24 * 60 * 60) * 1000;
        let old = admission(
            &format!("v2:{old_ms}:aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"),
            "42",
            "Old",
        );
        let pending = admission(
            &format!("v2:{old_ms}:bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb"),
            "42",
            "Pending",
        );
        outbox.admit_batch(&[old.clone(), pending.clone()]).unwrap();
        outbox.claim_ready(0, 1).unwrap();
        outbox.confirm(&old.operation_id).unwrap();
        outbox.compact_terminal_before(i64::MAX).unwrap();
        outbox
            .db
            .execute(
                "UPDATE provider_outbox SET settled_at = ?1 WHERE operation_id = ?2",
                params![unix_now() - 31 * 24 * 60 * 60, old.operation_id],
            )
            .unwrap();
        outbox
            .db
            .execute(
                "UPDATE provider_outbox_retention SET floor_ms = ?1, legacy_cutoff_ms = 0",
                [(unix_now() - 19 * 24 * 60 * 60) * 1000],
            )
            .unwrap();
        assert_eq!(outbox.prune_terminal_rows().unwrap(), 1);
        drop(outbox);

        let mut reopened = ProviderOutbox::open(&path, 4).unwrap();
        assert!(matches!(
            reopened.admit_batch(std::slice::from_ref(&old)),
            Err(OutboxError::ExpiredOperation(id)) if id == old.operation_id
        ));
        assert_eq!(
            reopened
                .admit_batch(std::slice::from_ref(&pending))
                .unwrap()[0]
                .sequence,
            2
        );
        assert!(matches!(
            reopened.admit_batch(&[admission("legacy-new", "42", "Legacy")]),
            Err(OutboxError::ExpiredOperation(_))
        ));
        assert!(matches!(
            reopened.admit_batch(&[admission(
                "v2:9999999999999:cccccccccccccccc",
                "42",
                "Future"
            )]),
            Err(OutboxError::InvalidOperationId(_))
        ));
        let fresh_ms = unix_now() * 1000;
        let fresh = admission(&format!("v2:{fresh_ms}:dddddddddddddddd"), "42", "Fresh");
        assert_eq!(reopened.admit_batch(&[fresh]).unwrap()[0].sequence, 3);
    }

    #[test]
    fn legacy_cutoff_prunes_old_rows_and_rejects_unknown_legacy_ids() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("outbox.sqlite");
        let mut outbox = ProviderOutbox::open(&path, 2).unwrap();
        let legacy = admission("opaque-legacy", "42", "Original");
        outbox.admit_batch(std::slice::from_ref(&legacy)).unwrap();
        outbox.discard(&legacy.operation_id).unwrap();
        outbox.compact_terminal_before(i64::MAX).unwrap();
        let prefixed_legacy = "v2:9999999999999:old-opaque-client-id";
        outbox
            .db
            .execute(
                "UPDATE provider_outbox SET operation_id = ?1, settled_at = ?2 WHERE operation_id = ?3",
                params![prefixed_legacy, unix_now() - 31 * 24 * 60 * 60, legacy.operation_id],
            )
            .unwrap();
        outbox
            .db
            .execute(
                "UPDATE provider_outbox_retention SET legacy_cutoff_ms = 0",
                [],
            )
            .unwrap();
        assert_eq!(outbox.prune_terminal_rows().unwrap(), 1);
        assert!(outbox.get(prefixed_legacy).unwrap().is_none());
        drop(outbox);
        let mut reopened = ProviderOutbox::open(&path, 2).unwrap();
        for id in ["opaque-legacy", "unseen-legacy"] {
            assert!(matches!(
                reopened.admit_batch(&[admission(id, "42", "Changed")]),
                Err(OutboxError::ExpiredOperation(_))
            ));
        }
        let current_ms = unix_now() * 1000;
        assert_eq!(
            reopened
                .admit_batch(&[admission(
                    &format!("v2:{current_ms}:eeeeeeeeeeeeeeee"),
                    "42",
                    "New"
                )])
                .unwrap()[0]
                .sequence,
            2
        );
    }

    #[test]
    fn dispatch_claims_one_per_ticket_and_survives_rate_limit_attention_and_restart() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dispatch.sqlite");
        let mut outbox = ProviderOutbox::open(&path, 5).unwrap();
        outbox
            .admit_batch(&[
                admission("first", "42", "First"),
                admission("later", "42", "Later"),
                admission("other", "43", "Other"),
            ])
            .unwrap();
        let claimed = outbox.claim_ready(100, 5).unwrap();
        assert_eq!(
            claimed
                .iter()
                .map(|item| item.operation_id.as_str())
                .collect::<Vec<_>>(),
            vec!["first", "other"]
        );
        assert_eq!(outbox.get("first").unwrap().unwrap().attempts, 1);
        outbox.defer("first", 200, "Jira 429", true).unwrap();
        outbox.defer_connection("github-main", 200).unwrap();
        assert!(outbox.claim_ready(199, 5).unwrap().is_empty());
        let claimed = outbox.claim_ready(200, 5).unwrap();
        assert_eq!(
            claimed
                .iter()
                .map(|item| item.operation_id.as_str())
                .collect::<Vec<_>>(),
            vec!["first"]
        );
        outbox
            .needs_attention(
                "first",
                "uncertain response",
                Some(&json!({"title":"Remote"})),
            )
            .unwrap();
        assert_eq!(
            outbox.get("first").unwrap().unwrap().conflict,
            Some(json!({"title":"Remote"}))
        );
        assert!(outbox.claim_ready(201, 5).unwrap().is_empty());
        outbox.retry("first").unwrap();
        assert_eq!(outbox.claim_ready(201, 5).unwrap().len(), 1);
        assert!(outbox.confirm("first").unwrap());
        assert_eq!(outbox.claim_ready(201, 5).unwrap()[0].operation_id, "later");
        drop(outbox);
        let mut reopened = ProviderOutbox::open(&path, 5).unwrap();
        assert_eq!(
            reopened.get("later").unwrap().unwrap().dispatch_state,
            DispatchState::Queued
        );
        reopened.discard("later").unwrap();
        assert_eq!(
            reopened.get("later").unwrap().unwrap().dispatch_state,
            DispatchState::Discarded
        );
        assert_eq!(reopened.pending_count().unwrap(), 1);
        assert!(!reopened.confirm("later").unwrap());
    }

    #[test]
    fn recent_status_keeps_old_pending_operations_alongside_bounded_history() {
        let mut outbox = ProviderOutbox::open(":memory:", 3).unwrap();
        outbox
            .admit_batch(&[admission("old", "42", "Old")])
            .unwrap();
        assert_eq!(outbox.claim_ready(0, 1).unwrap().len(), 1);
        outbox.needs_attention("old", "review", None).unwrap();
        for (id, native_id) in [("newer", "43"), ("newest", "44")] {
            outbox.admit_batch(&[admission(id, native_id, id)]).unwrap();
            assert_eq!(outbox.claim_ready(0, 3).unwrap().len(), 1);
            assert!(outbox.confirm(id).unwrap());
        }
        let recent = outbox.recent_for_connection("github-main", 1).unwrap();
        assert_eq!(
            recent
                .iter()
                .map(|item| item.operation_id.as_str())
                .collect::<Vec<_>>(),
            vec!["newest", "old"]
        );
    }

    #[test]
    fn opening_a_phase_two_outbox_adds_dispatch_columns_without_replacing_the_log() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("legacy.sqlite");
        let db = Connection::open(&path).unwrap();
        db.execute_batch(
            "CREATE TABLE provider_outbox (
                operation_id TEXT PRIMARY KEY, connection_id TEXT NOT NULL,
                native_id TEXT NOT NULL, sequence INTEGER NOT NULL, base_token TEXT,
                base_ticket_json TEXT NOT NULL, patch_json TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                state TEXT NOT NULL CHECK (state IN ('queued', 'confirmed')),
                UNIQUE (connection_id, native_id, sequence)
            );",
        )
        .unwrap();
        drop(db);
        let mut outbox = ProviderOutbox::open(&path, 2).unwrap();
        outbox
            .admit_batch(&[admission("migrated", "42", "New")])
            .unwrap();
        assert_eq!(
            outbox.claim_ready(0, 1).unwrap()[0].dispatch_state,
            DispatchState::Sending
        );
    }

    #[test]
    fn a_base_token_mismatch_cannot_be_admitted() {
        let mut outbox = ProviderOutbox::open(":memory:", 1).unwrap();
        let mut item = admission("one", "42", "A");
        item.patch.expected_token = Some("other".into());
        assert!(matches!(
            outbox.admit_batch(&[item]),
            Err(OutboxError::MismatchedBase)
        ));
        assert_eq!(outbox.pending_count().unwrap(), 0);
    }

    #[test]
    fn concurrent_connections_cannot_overfill_or_reuse_a_ticket_sequence() {
        use std::sync::{Arc, Barrier};

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("outbox.sqlite");
        ProviderOutbox::open(&path, 1).unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let handles = ["one", "two"].map(|id| {
            let path = path.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let mut outbox = ProviderOutbox::open(path, 1).unwrap();
                barrier.wait();
                outbox.admit_batch(&[admission(id, "42", id)])
            })
        });
        let results = handles.map(|handle| handle.join().unwrap());
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| matches!(result, Err(OutboxError::Backpressure { .. })))
                .count(),
            1
        );
        let outbox = ProviderOutbox::open(&path, 1).unwrap();
        assert_eq!(outbox.pending_count().unwrap(), 1);
        assert_eq!(outbox.list_pending().unwrap()[0].sequence, 1);
    }

    #[test]
    fn projection_applies_ordered_jira_fields_and_keeps_remote_token() {
        let mut outbox = ProviderOutbox::open(":memory:", 3).unwrap();
        let mut first = admission("one", "42", "First");
        first.patch.details = Some("Draft details".into());
        first.patch.tags = Some(vec!["ready".into()]);
        let mut second = admission("two", "42", "Second");
        second.patch.category = Some("bug".into());
        second.patch.priority = Some(hotsheet_model::Priority::High);
        second.patch.tags = Some(vec![]);
        outbox.admit_batch(&[first, second]).unwrap();
        let remote = json!({
            "connection_id": "github-main",
            "native_id": "42",
            "concurrency_token": "remote-etag",
            "title": "Remote title",
            "details": "Remote details",
            "priority": "default",
            "tags": ["old"],
            "status": "not_started",
        });
        let projected = project_pending_ticket(&remote, &outbox.list_pending().unwrap()).unwrap();
        assert_eq!(projected["title"], "Second");
        assert_eq!(projected["details"], "Draft details");
        assert_eq!(projected["category"], "bug");
        assert_eq!(projected["priority"], "high");
        assert_eq!(projected["tags"], json!([]));
        assert_eq!(projected["status"], "not_started");
        assert_eq!(projected["concurrency_token"], "remote-etag");
        assert_eq!(projected["pending_operation_ids"], json!(["one", "two"]));
        assert_eq!(remote["title"], "Remote title");
        assert_eq!(project_pending_ticket(&remote, &[]).unwrap(), remote);
    }

    #[test]
    fn projection_rejects_unsupported_fields_and_cross_ticket_or_disordered_rows() {
        let mut outbox = ProviderOutbox::open(":memory:", 3).unwrap();
        let remote = json!({
            "connection_id": "github-main",
            "native_id": "42",
            "concurrency_token": "remote-etag",
        });
        let mut unsupported = admission("status", "42", "Title");
        unsupported.patch.status = Some(hotsheet_model::Status::Started);
        let mut operation = outbox.admit_batch(&[unsupported]).unwrap().remove(0);
        assert!(matches!(
            project_pending_ticket(&remote, std::slice::from_ref(&operation)),
            Err(OutboxError::UnsupportedProjectionField {
                field: "status",
                ..
            })
        ));
        operation.patch.status = None;
        operation.native_id = "other".into();
        assert!(matches!(
            project_pending_ticket(&remote, std::slice::from_ref(&operation)),
            Err(OutboxError::InvalidProjectionOrder(_))
        ));
        operation.native_id = "42".into();
        assert!(matches!(
            project_pending_ticket(&remote, &[operation.clone(), operation]),
            Err(OutboxError::InvalidProjectionOrder(_))
        ));
    }
}
