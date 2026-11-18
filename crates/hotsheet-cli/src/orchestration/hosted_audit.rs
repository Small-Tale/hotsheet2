//! HS2-CRBR8X: the hosting machine's journal of the sessions orchestrators start and
//! stop on it.
//!
//! Hosting needs no opt-in: each start passes the orchestrating side's permission (the
//! consent), and this machine's admission file decides whether it runs here
//! (`max_sessions: 0` refuses every start). This journal lets the host's owner see what
//! ran without asking the orchestration side. It records each start this machine
//! launched or refused, each stop it carried out or refused, and each session it found
//! ended without a stop. A repeated start of the same operation launched nothing and is
//! not recorded again.
//!
//! The answer to the orchestrator is already decided when the entry is written, and a
//! launched terminal cannot be un-launched, so a journal that cannot be written never
//! changes that answer. HS2-S0FDKR: such a failure is logged (identifiers and the error
//! kind only) and counted beside the journal, and the journal read reports the count,
//! so the host's owner can tell the journal is incomplete.

use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::{
    AuditOutcome, HostedSession, HostedSessionAuditJournal, HostedSessionAuditPage,
    HostedSessionAuditRecord, HostedSessionEvent, JournalRecord,
};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::sessions::{
    HostedSessionStart, HostedSessionStop, HostedStartOutcome, HostedStartReply, HostedStopReply,
};
use super::*;

/// The local requester: a request this machine made itself, or one from its own CLI.
pub const LOCAL_REQUESTER: &str = "local";

/// HS2-S0FDKR: the journal entries this machine could not write for one checkout.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostedJournalFailures {
    /// Entries lost since the journal began.
    pub count: u64,
    /// When the latest was lost.
    pub last_at_ms: u64,
    /// The latest failure's error kind, a stable code.
    pub last_error: String,
}

/// A page of the hosting machine's session journal, with what it could not record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HostedAuditView {
    #[serde(flatten)]
    pub page: HostedSessionAuditPage,
    /// HS2-S0FDKR: absent while every entry was written.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub write_failures: Option<HostedJournalFailures>,
}

/// Where the failures of one checkout's journal are counted: beside the journal, in a
/// file of its own, so a journal file that cannot be written does not lose the count.
fn failures_path(home: &Path, checkout_id: &str) -> PathBuf {
    home.join(<HostedSessionAuditRecord as JournalRecord>::DIRECTORY)
        .join(format!("{checkout_id}.write-failures.json"))
}

fn read_failures(home: &Path, checkout_id: &str) -> Option<HostedJournalFailures> {
    let bytes = std::fs::read(failures_path(home, checkout_id)).ok()?;
    serde_json::from_slice::<HostedJournalFailures>(&bytes)
        .ok()
        .filter(|failures| failures.count > 0)
}

impl OrchestrationRuntime {
    /// HS2-CRBR8X: serve one request from `requester` (a device certificate's
    /// `cert-<fingerprint>`, a paired browser's `browser-<device id>`), so the hosting
    /// machine's session journal names who asked.
    #[must_use]
    pub fn with_requester(mut self, requester: impl Into<String>) -> Self {
        self.requester = Some(requester.into());
        self
    }

    fn hosted_journal(
        &self,
        checkout_id: &str,
    ) -> Result<HostedSessionAuditJournal, OrchestrationError> {
        HostedSessionAuditJournal::new(&self.home, checkout_id)
    }

    /// A record with who asked. An actor id that is not a bounded identifier is left
    /// out rather than losing the entry.
    fn hosted_record(
        &self,
        event: HostedSessionEvent,
        operation_id: &str,
        actor: Option<&MutationActor>,
        requested: bool,
    ) -> HostedSessionAuditRecord {
        let mut record = HostedSessionAuditRecord {
            at_ms: 0,
            event,
            outcome: AuditOutcome::Succeeded,
            reason: None,
            operation_id: operation_id.into(),
            session_id: None,
            tool: None,
            origin_checkout_id: None,
            requester: requested.then(|| {
                self.requester
                    .as_deref()
                    .unwrap_or(LOCAL_REQUESTER)
                    .to_owned()
            }),
            actor_role: super::access::role_code(actor),
            actor_id: actor.and_then(|actor| actor.id.clone()),
        };
        if record.validate().is_err() {
            record.actor_id = None;
        }
        record
    }

    fn journal_hosted(&self, checkout_id: &str, record: HostedSessionAuditRecord, now_ms: u64) {
        let record = HostedSessionAuditRecord {
            at_ms: now_ms,
            ..record
        };
        let (event, operation_id) = (record.event, record.operation_id.clone());
        let written = self
            .hosted_journal(checkout_id)
            .and_then(|journal| journal.append(record));
        if let Err(error) = written {
            self.journal_hosted_failed(checkout_id, event, &operation_id, &error, now_ms);
        }
    }

    /// HS2-S0FDKR: log a journal entry this machine could not write, with identifiers
    /// and the error kind only, and count it beside the journal. Best effort: when the
    /// count cannot be written either, the log line is all there is.
    fn journal_hosted_failed(
        &self,
        checkout_id: &str,
        event: HostedSessionEvent,
        operation_id: &str,
        error: &OrchestrationError,
        now_ms: u64,
    ) {
        tracing::warn!(
            checkout_id,
            operation_id,
            event = ?event,
            error = error.code(),
            "hosted session journal entry not written"
        );
        // Only a checkout id the journal accepts names a file.
        if HostedSessionAuditJournal::new(&self.home, checkout_id).is_err() {
            return;
        }
        let path = failures_path(&self.home, checkout_id);
        let mut failures = read_failures(&self.home, checkout_id).unwrap_or_default();
        failures.count = failures.count.saturating_add(1);
        failures.last_at_ms = now_ms;
        failures.last_error = error.code().into();
        let written = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| {
                let bytes = serde_json::to_vec(&failures).map_err(std::io::Error::other)?;
                let temporary = path.with_extension("json.partial");
                std::fs::write(&temporary, bytes)?;
                std::fs::rename(&temporary, &path)
            });
        if written.is_err() {
            tracing::warn!(
                checkout_id,
                "hosted session journal failure count not written"
            );
        }
    }

    /// The end of a session nobody stopped, once however many readers noticed it.
    pub(super) fn journal_hosted_end(&self, session: &HostedSession) {
        let (Some(ended_at_ms), Some(reason)) = (session.ended_at_ms, &session.end_reason) else {
            return;
        };
        let record = HostedSessionAuditRecord {
            at_ms: ended_at_ms,
            reason: Some(reason.clone()),
            session_id: Some(session.session_id.clone()),
            tool: Some(session.tool.clone()),
            origin_checkout_id: Some(session.origin.owner_checkout_id.clone()),
            ..self.hosted_record(HostedSessionEvent::End, &session.operation_id, None, false)
        };
        let at_ms = record.at_ms;
        let written = self
            .hosted_journal(&session.checkout_id)
            .and_then(|journal| {
                journal.append_unless(record, |previous| {
                    previous.event == HostedSessionEvent::End
                        && previous.operation_id == session.operation_id
                        && previous.session_id.as_deref() == Some(session.session_id.as_str())
                })
            });
        if let Err(error) = written {
            self.journal_hosted_failed(
                &session.checkout_id,
                HostedSessionEvent::End,
                &session.operation_id,
                &error,
                at_ms,
            );
        }
    }

    /// Start a session on this machine for an orchestration project, and journal what
    /// this machine did with the request.
    pub fn hosted_session_start(
        &self,
        checkout_id: &str,
        request: HostedSessionStart,
        tx: &mut Transaction<'_>,
    ) -> Result<HostedStartReply, OrchestrationError> {
        let base = HostedSessionAuditRecord {
            tool: Some(request.session.tool.clone()),
            origin_checkout_id: Some(request.session.origin.owner_checkout_id.clone()),
            ..self.hosted_record(
                HostedSessionEvent::Start,
                &request.session.operation_id,
                request.actor.as_ref(),
                true,
            )
        };
        let reply = self.hosted_session_start_unjournaled(checkout_id, request, tx)?;
        let record = match reply.outcome {
            // The same operation again launched nothing new.
            HostedStartOutcome::Existing => return Ok(reply),
            HostedStartOutcome::Started => HostedSessionAuditRecord {
                session_id: reply
                    .session
                    .as_ref()
                    .map(|view| view.session.session_id.clone()),
                ..base
            },
            HostedStartOutcome::Refused => HostedSessionAuditRecord {
                outcome: AuditOutcome::Denied,
                reason: reply.reason.clone(),
                ..base
            },
        };
        let checkout = reply.session.as_ref().map_or_else(
            || self.journal_checkout(checkout_id),
            |view| view.session.checkout_id.clone(),
        );
        self.journal_hosted(&checkout, record, tx.clock.now_ms());
        Ok(reply)
    }

    /// Stop a hosted session, and journal what this machine did with the request. A
    /// session that had already ended was not stopped again and is not recorded again.
    pub fn hosted_session_stop(
        &self,
        checkout_id: &str,
        request: HostedSessionStop,
        tx: &mut Transaction<'_>,
    ) -> Result<HostedStopReply, OrchestrationError> {
        let base = self.hosted_record(
            HostedSessionEvent::Stop,
            &request.operation_id,
            request.actor.as_ref(),
            true,
        );
        let reply = self.hosted_session_stop_unjournaled(checkout_id, request, tx)?;
        let acted = reply.fenced || reply.safe_point.is_some();
        if reply.stopped && !acted {
            return Ok(reply);
        }
        let session = reply.session.as_ref().map(|view| &view.session);
        let record = HostedSessionAuditRecord {
            outcome: if reply.stopped {
                AuditOutcome::Succeeded
            } else {
                AuditOutcome::Denied
            },
            // A stop that withdrew a start before it launched has no end reason.
            reason: reply
                .reason
                .clone()
                .or_else(|| reply.fenced.then(|| "withdrawn".to_owned())),
            session_id: session.map(|session| session.session_id.clone()),
            tool: session.map(|session| session.tool.clone()),
            origin_checkout_id: session.map(|session| session.origin.owner_checkout_id.clone()),
            ..base
        };
        let checkout = session.map_or_else(
            || self.journal_checkout(checkout_id),
            |session| session.checkout_id.clone(),
        );
        self.journal_hosted(&checkout, record, tx.clock.now_ms());
        Ok(reply)
    }

    /// The canonical id of a checkout this machine hosts, else the id as given.
    fn journal_checkout(&self, checkout_id: &str) -> String {
        self.owner(checkout_id)
            .map_or_else(|_| checkout_id.to_owned(), |checkout| checkout.id)
    }

    /// HS2-CRBR8X: the hosting machine's session journal for one checkout, oldest first
    /// after the cursor. Machine-local: only this machine's owner reads it. HS2-S0FDKR:
    /// with how many entries could not be written, when any.
    pub fn hosted_audit(
        &self,
        checkout_id: &str,
        after: u64,
        limit: usize,
    ) -> Result<HostedAuditView, OrchestrationError> {
        let checkout = self.journal_checkout(checkout_id);
        let page = self.hosted_journal(&checkout)?.read(after, limit)?;
        Ok(HostedAuditView {
            page,
            write_failures: read_failures(&self.home, &checkout),
        })
    }
}
