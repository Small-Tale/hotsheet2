//! Durable provisional advisory claim intents (HS2-BPKM76).
//!
//! The provider stays authoritative. Rows contain only qualified IDs, worker IDs,
//! expiry and replay state; credentials and ticket content never enter this DB.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::Serialize;
use thiserror::Error;

use crate::advisory_claim::{AdvisoryClaimEvent, AdvisoryClaimEventKind};

const MAX_PENDING: i64 = 1000;
const MAX_TOTAL: i64 = 10_000;
const MAX_AGE_SECONDS: i64 = 7 * 24 * 60 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaimReplayState {
    Queued,
    Confirmed,
    NeedsAttention,
    Expired,
}

impl ClaimReplayState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Confirmed => "confirmed",
            Self::NeedsAttention => "needs_attention",
            Self::Expired => "expired",
        }
    }
    fn from_str(value: &str) -> Result<Self, ClaimLedgerError> {
        match value {
            "queued" => Ok(Self::Queued),
            "confirmed" => Ok(Self::Confirmed),
            "needs_attention" => Ok(Self::NeedsAttention),
            "expired" => Ok(Self::Expired),
            _ => Err(ClaimLedgerError::InvalidState),
        }
    }
}

#[derive(Debug, Error)]
pub enum ClaimLedgerError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("advisory claim identity must be nonempty")]
    EmptyIdentity,
    #[error("operation was retried with a different worker or claim expiry")]
    ChangedPayload,
    #[error("advisory claim ledger has reached its pending limit")]
    Backpressure,
    #[error("advisory claim ledger contains an invalid replay state")]
    InvalidState,
    #[error("provisional claim is outside the retry window")]
    Expired,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingClaim {
    pub project_scope: String,
    pub event: AdvisoryClaimEvent,
    pub state: ClaimReplayState,
    pub created_at: i64,
}

pub struct AdvisoryClaimLedger {
    db: Connection,
}

impl AdvisoryClaimLedger {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, ClaimLedgerError> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent)?;
        }
        let db = Connection::open(path)?;
        db.busy_timeout(std::time::Duration::from_secs(5))?;
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS provisional_claims (
            project_scope TEXT NOT NULL,
            connection_id TEXT NOT NULL,
            native_id TEXT NOT NULL,
            operation_id TEXT NOT NULL,
            worker TEXT NOT NULL,
            claim_expiry INTEGER NOT NULL,
            event_json TEXT NOT NULL,
            state TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            PRIMARY KEY (project_scope, connection_id, native_id, operation_id)
        );
        CREATE INDEX IF NOT EXISTS provisional_claims_replay
            ON provisional_claims(state, created_at);",
        )?;
        Ok(Self { db })
    }

    pub fn admit(
        &mut self,
        project_scope: &str,
        event: &AdvisoryClaimEvent,
        now_epoch: i64,
    ) -> Result<PendingClaim, ClaimLedgerError> {
        if project_scope.is_empty()
            || event.ticket.connection_id.is_empty()
            || event.ticket.native_id.is_empty()
            || event.operation_id.is_empty()
            || event.worker.is_empty()
        {
            return Err(ClaimLedgerError::EmptyIdentity);
        }
        let expiry = match event.kind {
            AdvisoryClaimEventKind::Claim { expires_unix_ms } => expires_unix_ms,
            _ => return Err(ClaimLedgerError::ChangedPayload),
        };
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        // Retire only settled rows after the documented retry window. Queued and
        // needs-attention evidence remains until explicitly reconciled.
        tx.execute("DELETE FROM provisional_claims WHERE state IN ('confirmed','expired') AND created_at < ?1",
            [now_epoch.saturating_sub(MAX_AGE_SECONDS)])?;
        let prior: Option<(String, i64, String, String, i64)> = tx
            .query_row(
                "SELECT worker, claim_expiry, event_json, state, created_at FROM provisional_claims
             WHERE project_scope=?1 AND connection_id=?2 AND native_id=?3 AND operation_id=?4",
                params![
                    project_scope,
                    event.ticket.connection_id,
                    event.ticket.native_id,
                    event.operation_id
                ],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .optional()?;
        if let Some((worker, claim_expiry, json, state, created_at)) = prior {
            if worker != event.worker || claim_expiry != expiry {
                return Err(ClaimLedgerError::ChangedPayload);
            }
            if now_epoch.saturating_sub(created_at) > MAX_AGE_SECONDS {
                return Err(ClaimLedgerError::Expired);
            }
            tx.commit()?;
            return Ok(PendingClaim {
                project_scope: project_scope.into(),
                event: serde_json::from_str(&json)?,
                state: ClaimReplayState::from_str(&state)?,
                created_at,
            });
        }
        if expiry <= now_epoch.saturating_mul(1000)
            || expiry
                > now_epoch
                    .saturating_add(MAX_AGE_SECONDS)
                    .saturating_mul(1000)
        {
            return Err(ClaimLedgerError::Expired);
        }
        let pending: i64 = tx.query_row(
            "SELECT COUNT(*) FROM provisional_claims WHERE state='queued'",
            [],
            |row| row.get(0),
        )?;
        if pending >= MAX_PENDING {
            return Err(ClaimLedgerError::Backpressure);
        }
        let total: i64 = tx.query_row("SELECT COUNT(*) FROM provisional_claims", [], |row| {
            row.get(0)
        })?;
        if total >= MAX_TOTAL {
            return Err(ClaimLedgerError::Backpressure);
        }
        let json = serde_json::to_string(event)?;
        tx.execute(
            "INSERT INTO provisional_claims VALUES (?1,?2,?3,?4,?5,?6,?7,'queued',?8)",
            params![
                project_scope,
                event.ticket.connection_id,
                event.ticket.native_id,
                event.operation_id,
                event.worker,
                expiry,
                json,
                now_epoch
            ],
        )?;
        tx.commit()?;
        Ok(PendingClaim {
            project_scope: project_scope.into(),
            event: event.clone(),
            state: ClaimReplayState::Queued,
            created_at: now_epoch,
        })
    }

    pub fn pending(&self, limit: usize) -> Result<Vec<PendingClaim>, ClaimLedgerError> {
        let mut stmt = self.db.prepare("SELECT project_scope,event_json,state,created_at FROM provisional_claims WHERE state='queued' ORDER BY created_at,project_scope,connection_id,native_id,operation_id LIMIT ?1")?;
        let rows = stmt.query_map([limit.min(MAX_PENDING as usize) as i64], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })?;
        rows.map(|row| {
            let (project_scope, json, state, created_at) = row?;
            Ok(PendingClaim {
                project_scope,
                event: serde_json::from_str(&json)?,
                state: ClaimReplayState::from_str(&state)?,
                created_at,
            })
        })
        .collect()
    }

    pub fn settle(
        &mut self,
        claim: &PendingClaim,
        state: ClaimReplayState,
    ) -> Result<(), ClaimLedgerError> {
        let allowed = match claim.state {
            ClaimReplayState::Queued => matches!(
                state,
                ClaimReplayState::Confirmed
                    | ClaimReplayState::NeedsAttention
                    | ClaimReplayState::Expired
            ),
            ClaimReplayState::NeedsAttention => matches!(
                state,
                ClaimReplayState::Confirmed | ClaimReplayState::Expired
            ),
            _ => false,
        };
        if !allowed {
            return Err(ClaimLedgerError::InvalidState);
        }
        let changed = self.db.execute("UPDATE provisional_claims SET state=?1 WHERE project_scope=?2 AND connection_id=?3 AND native_id=?4 AND operation_id=?5 AND state=?6",
            params![state.as_str(), claim.project_scope, claim.event.ticket.connection_id, claim.event.ticket.native_id, claim.event.operation_id, claim.state.as_str()])?;
        if changed != 1 {
            return Err(ClaimLedgerError::InvalidState);
        }
        Ok(())
    }

    pub fn for_ticket(
        &self,
        project_scope: &str,
        connection_id: &str,
        native_id: &str,
    ) -> Result<Vec<PendingClaim>, ClaimLedgerError> {
        let mut stmt = self.db.prepare("SELECT project_scope,event_json,state,created_at FROM provisional_claims WHERE project_scope=?1 AND connection_id=?2 AND native_id=?3 AND state IN ('queued','needs_attention') ORDER BY created_at,operation_id")?;
        let rows = stmt.query_map(params![project_scope, connection_id, native_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })?;
        rows.map(|row| {
            let (project_scope, json, state, created_at) = row?;
            Ok(PendingClaim {
                project_scope,
                event: serde_json::from_str(&json)?,
                state: ClaimReplayState::from_str(&state)?,
                created_at,
            })
        })
        .collect()
    }
}

pub fn now_epoch() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::TicketRef;

    fn event(worker: &str, expiry: i64) -> AdvisoryClaimEvent {
        AdvisoryClaimEvent {
            ticket: TicketRef {
                connection_id: "jira".into(),
                native_id: "ENG-9".into(),
            },
            worker: worker.into(),
            operation_id: "op-1".into(),
            provider_order: 0,
            kind: AdvisoryClaimEventKind::Claim {
                expires_unix_ms: expiry,
            },
        }
    }

    #[test]
    fn hs2_bpkm76_provisional_admission_is_durable_and_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("claims.sqlite");
        let mut ledger = AdvisoryClaimLedger::open(&path).unwrap();
        assert_eq!(
            ledger
                .admit("project-a", &event("worker-a", 5000), 1)
                .unwrap()
                .state,
            ClaimReplayState::Queued
        );
        assert_eq!(
            ledger
                .admit("project-a", &event("worker-a", 5000), 1)
                .unwrap()
                .state,
            ClaimReplayState::Queued
        );
        assert!(matches!(
            ledger.admit("project-a", &event("worker-b", 5000), 1),
            Err(ClaimLedgerError::ChangedPayload)
        ));
        drop(ledger);
        let mut reopened = AdvisoryClaimLedger::open(&path).unwrap();
        let pending = reopened.pending(10).unwrap();
        assert_eq!(pending.len(), 1);
        reopened
            .settle(&pending[0], ClaimReplayState::Confirmed)
            .unwrap();
        assert!(reopened.pending(10).unwrap().is_empty());
        assert!(
            reopened
                .for_ticket("project-a", "jira", "ENG-9")
                .unwrap()
                .is_empty()
        );
        assert!(matches!(
            reopened.admit(
                "project-a",
                &event("worker-a", 5000),
                1 + MAX_AGE_SECONDS + 1
            ),
            Err(ClaimLedgerError::Expired)
        ));
        let later = 1 + MAX_AGE_SECONDS + 1;
        let mut fresh = event("worker-b", (later + 3600) * 1000);
        fresh.operation_id = "op-2".into();
        reopened.admit("project-a", &fresh, later).unwrap();
        let retained: i64 = reopened
            .db
            .query_row("SELECT COUNT(*) FROM provisional_claims", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(retained, 1);
    }

    #[test]
    fn hs2_bpkm76_ledger_cap_and_attention_recovery_are_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let mut ledger = AdvisoryClaimLedger::open(dir.path().join("claims.sqlite")).unwrap();
        let first = ledger
            .admit("project-a", &event("worker-a", 5000), 1)
            .unwrap();
        ledger
            .settle(&first, ClaimReplayState::NeedsAttention)
            .unwrap();
        assert!(matches!(
            ledger.settle(&first, ClaimReplayState::Expired),
            Err(ClaimLedgerError::InvalidState)
        ));
        let attention = ledger
            .for_ticket("project-a", "jira", "ENG-9")
            .unwrap()
            .remove(0);
        ledger
            .settle(&attention, ClaimReplayState::Confirmed)
            .unwrap();
        let tx = ledger.db.transaction().unwrap();
        for n in 1..MAX_TOTAL {
            tx.execute("INSERT INTO provisional_claims VALUES ('project-a','jira','ENG-9',?1,'worker',5000,'{}','confirmed',1)",
                [format!("seed-{n}")]).unwrap();
        }
        tx.commit().unwrap();
        let mut fresh = event("worker-b", 6000);
        fresh.operation_id = "op-2".into();
        assert!(matches!(
            ledger.admit("project-a", &fresh, 2),
            Err(ClaimLedgerError::Backpressure)
        ));
        let later = MAX_AGE_SECONDS + 2;
        let mut later_event = event("worker-b", (later + 3600) * 1000);
        later_event.operation_id = "op-3".into();
        ledger.admit("project-a", &later_event, later).unwrap();
        assert_eq!(ledger.pending(10).unwrap().len(), 1);
    }
}
