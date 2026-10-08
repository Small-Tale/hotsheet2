//! Durable, provider-neutral admission log for provisional external ticket updates.
//!
//! The provider remains authoritative. This database contains ticket snapshots and
//! mutation intent, never provider credentials. Admission is one SQLite IMMEDIATE
//! transaction so retries, capacity checks, and per-ticket ordering agree across
//! processes. The database is separate from the disposable ticket search index.

use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
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
                UNIQUE (connection_id, native_id, sequence)
            );
            CREATE INDEX IF NOT EXISTS provider_outbox_pending
                ON provider_outbox (state, connection_id, native_id, sequence);",
        )?;
        Ok(Self { db, max_pending })
    }

    /// Atomically admit a batch. An identical operation id returns its existing row;
    /// a changed retry rejects the whole batch. Ordering includes confirmed history.
    pub fn admit_batch(
        &mut self,
        admissions: &[OutboxAdmission],
    ) -> Result<Vec<OutboxOperation>, OutboxError> {
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
                    "SELECT payload_json FROM provider_outbox WHERE operation_id = ?1",
                    [&admission.operation_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            if let Some(existing) = existing {
                if existing != payload {
                    return Err(OutboxError::ChangedPayload(admission.operation_id.clone()));
                }
                accepted.push(get_in(&tx, &admission.operation_id)?.expect("row just read"));
                continue;
            }
            if pending + added >= self.max_pending {
                return Err(OutboxError::Backpressure {
                    max: self.max_pending,
                    pending,
                    requested: added + 1,
                });
            }
            let sequence: i64 = tx.query_row(
                "SELECT COALESCE(MAX(sequence), 0) + 1 FROM provider_outbox
                 WHERE connection_id = ?1 AND native_id = ?2",
                params![admission.connection_id, admission.native_id],
                |row| row.get(0),
            )?;
            tx.execute(
                "INSERT INTO provider_outbox
                 (operation_id, connection_id, native_id, sequence, base_token,
                  base_ticket_json, patch_json, payload_json, state)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'queued')",
                params![
                    admission.operation_id,
                    admission.connection_id,
                    admission.native_id,
                    sequence,
                    admission.base_token,
                    serde_json::to_string(&admission.base_ticket)?,
                    serde_json::to_string(&admission.patch)?,
                    payload,
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

    /// Future dispatch can mark an acknowledged intent without losing its idempotency key.
    /// Repeating confirmation is a no-op; an unknown id returns false.
    pub fn confirm(&mut self, operation_id: &str) -> Result<bool, OutboxError> {
        let changed = self.db.execute(
            "UPDATE provider_outbox SET state = 'confirmed'
             WHERE operation_id = ?1 AND state = 'queued'",
            [operation_id],
        )?;
        Ok(changed > 0)
    }
}

fn get_in(db: &Connection, operation_id: &str) -> Result<Option<OutboxOperation>, OutboxError> {
    let row = db
        .query_row(
            "SELECT operation_id, connection_id, native_id, sequence, base_token,
                    base_ticket_json, patch_json, state
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
    )) = row
    else {
        return Ok(None);
    };
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
