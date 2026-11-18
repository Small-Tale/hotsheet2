//! HS2-CRBR8X: the hosting machine's own journal of the sessions orchestrators start
//! and stop on it.
//!
//! Any machine that hosts a checkout keeps it, so its owner can see what ran there
//! without asking the orchestration side. It is the bounded audit journal
//! ([`super::Journal`]) with its own record type: one JSON-lines file per hosting
//! checkout under `${HOTSHEET_HOME}/hosted-session-audit/`, a 1 MiB segment with one
//! retained predecessor, and a `seq` that never restarts. Entries carry identifiers,
//! codes and timestamps only: never a label, a prompt or ticket text.

use serde::{Deserialize, Serialize};

use super::OrchestrationError;
use super::audit::{AuditOutcome, Journal, JournalEntry, JournalPage, JournalRecord};
use super::contract::{valid_identifier, valid_token};

/// What happened to a hosted session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostedSessionEvent {
    /// An orchestrator asked this machine to start a session (`POST target-sessions`).
    Start,
    /// An orchestrator or a person asked this machine to stop one (`DELETE`).
    Stop,
    /// A session ended without a stop request: its terminal went away.
    End,
}

/// One entry of the hosting machine's session journal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostedSessionAuditRecord {
    pub at_ms: u64,
    pub event: HostedSessionEvent,
    /// `succeeded` when the start launched or the stop stopped; `denied` when this
    /// machine refused it (`reason` says why).
    pub outcome: AuditOutcome,
    /// A stable code: the refusal reason, or how the session ended.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The orchestrator's operation id for the session.
    pub operation_id: String,
    /// This machine's terminal id, once one was reserved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// The orchestration project's checkout id on its own machine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_checkout_id: Option<String>,
    /// Who sent the request to this machine: `cert-<fingerprint>` for a device
    /// certificate, `browser-<device id>` for a paired browser, `local` for this
    /// machine itself. Absent for an end nobody requested.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requester: Option<String>,
    /// The actor the request named, for attribution only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
}

impl JournalRecord for HostedSessionAuditRecord {
    const DIRECTORY: &'static str = "hosted-session-audit";
    fn validate(&self) -> Result<(), OrchestrationError> {
        valid_identifier(&self.operation_id)?;
        for value in [
            &self.reason,
            &self.tool,
            &self.origin_checkout_id,
            &self.requester,
            &self.actor_role,
            &self.actor_id,
        ]
        .into_iter()
        .flatten()
        {
            valid_identifier(value)?;
        }
        if let Some(session) = &self.session_id {
            valid_token(session)?;
        }
        Ok(())
    }
}

pub type HostedSessionAuditEntry = JournalEntry<HostedSessionAuditRecord>;
pub type HostedSessionAuditPage = JournalPage<HostedSessionAuditRecord>;
pub type HostedSessionAuditJournal = Journal<HostedSessionAuditRecord>;
