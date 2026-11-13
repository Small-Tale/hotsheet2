//! HS2-EBHDSB: dispatching work to an exact target ticket and supervising the run.
//!
//! The target ticket's own claim is the only lock. `direct` takes that claim for a
//! named worker through the target's claim and lease operations; `dispatcher` marks
//! the ticket Up Next and lets the target's own dispatcher assign it. Integration and
//! pushing stay with the target's workflow.
use hotsheet_model::{NoteKind, OrchestrationLink, OrchestrationRelation, Timestamp, Ulid};
use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::DenialReason;
use hotsheet_ticketing::orchestration::{
    DispatchMode, NewRun, Run, RunControl, RunError, RunEvent, RunEventJournal, RunEventKind,
    RunEventPage, RunEventSource, RunObservation, RunRegistry, RunState, RunTarget, RunTicket,
    RunUnreachable, RunUpdate, TargetAction, TargetFacts, TargetOpError, align,
};
use hotsheet_ticketing::{ApiTicket, FsStore, OpError, ops};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::tickets::{encode, identifier, native, timestamp};
use super::*;

pub const DEFAULT_LEASE_MINUTES: u32 = 30;
pub const MAX_LEASE_MINUTES: u32 = 240;

/// Hosted exact claim, renewal or release of one ticket for a worker.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostedClaim {
    pub connection_id: String,
    pub native_id: String,
    pub worker: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease_minutes: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DispatchRun {
    pub ticket: OrchestrationTicket,
    #[serde(flatten)]
    pub target: TargetTicket,
    /// Retain across retries: the same id is the same run.
    pub operation_id: String,
    /// `direct` (the default) or `dispatcher`.
    #[serde(default)]
    pub mode: Option<DispatchMode>,
    /// Required for `direct`; the target's dispatcher chooses otherwise.
    #[serde(default)]
    pub worker_id: Option<String>,
    #[serde(default)]
    pub worker_label: Option<String>,
    #[serde(default)]
    pub lease_minutes: Option<u32>,
    /// HS2-42TARA: ask a person to let this exact operation start although the plan
    /// says its work is not ready. Never set by automation on its own.
    #[serde(default)]
    pub override_readiness: bool,
}

/// The longest one events request waits; a client re-polls with its cursor.
pub const MAX_EVENT_WAIT_MS: u64 = 20_000;

/// HS2-7QHWHJ: a run for a viewer, with how far its target state can be trusted.
#[derive(Debug, Clone, Serialize)]
pub struct RunStatus {
    #[serde(flatten)]
    pub run: Run,
    /// `reachable`, `unreachable` or `unknown` (never observed yet).
    pub reachability: &'static str,
    /// The observed target state may no longer be true.
    pub stale: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_confirmed_at_ms: Option<u64>,
    /// HS2-JSF985: the run's work was started ahead of a pending gate: which gates,
    /// and what became of them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provisional: Option<ProvisionalWork>,
}

/// A run with what could not be confirmed against the target just now.
#[derive(Debug, Clone, Serialize)]
pub struct RunView {
    #[serde(flatten)]
    pub run: Run,
    /// Names of external state this answer could not verify, such as `target_claim`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub unverified: Vec<&'static str>,
}

impl From<Run> for RunView {
    fn from(run: Run) -> Self {
        Self {
            run,
            unverified: Vec::new(),
        }
    }
}

fn lease(minutes: Option<u32>) -> Result<u32, OrchestrationError> {
    let minutes = minutes.unwrap_or(DEFAULT_LEASE_MINUTES);
    if !(5..=MAX_LEASE_MINUTES).contains(&minutes) {
        return Err(OrchestrationError::Invalid(
            "lease minutes must be 5 to 240",
        ));
    }
    Ok(minutes)
}

fn claim_error(error: OpError) -> OrchestrationError {
    OrchestrationError::TargetOperation(match error {
        // Someone else holds it, it is closed, or it is not claimed by this worker.
        OpError::WrongWorker { .. } | OpError::NotClaimed(_) | OpError::ClaimUnavailable { .. } => {
            TargetOpError::Conflict
        }
        OpError::Store(error) if error.is_io_kind(std::io::ErrorKind::NotFound) => {
            TargetOpError::NotFound
        }
        _ => TargetOpError::Failed,
    })
}

fn after(now: &Timestamp, minutes: u32) -> Timestamp {
    let at = now.instant().unwrap_or(time::OffsetDateTime::UNIX_EPOCH)
        + time::Duration::minutes(i64::from(minutes));
    Timestamp::from_datetime(at)
}

/// Milliseconds of an RFC 3339 field of a wire ticket, when present and parseable.
pub(super) fn field_ms(ticket: &Value, key: &str) -> Option<u64> {
    let text = ticket.get(key)?.as_str()?;
    let at = Timestamp::new(text).instant()?;
    u64::try_from(at.unix_timestamp_nanos() / 1_000_000).ok()
}

pub(super) fn text<'a>(ticket: &'a Value, key: &str) -> Option<&'a str> {
    ticket.get(key).and_then(Value::as_str)
}

impl OrchestrationRuntime {
    // ---- Hosted half ----

    /// The git store of one hosted source. Exact claims exist only there; another
    /// provider reports the operation as unsupported rather than approximating it.
    fn hosted_claim_store(
        &self,
        checkout_id: &str,
        request: &HostedClaim,
    ) -> Result<(FsStore, Ulid), OrchestrationError> {
        identifier(&request.connection_id, "bounded connection id required")?;
        identifier(&request.worker, "bounded worker id required")?;
        native(&request.native_id)?;
        let checkout = self.owner(checkout_id)?;
        let Some(source) = checkout.source(&request.connection_id) else {
            return Err(OrchestrationError::TargetOperation(TargetOpError::NotFound));
        };
        if source.provider != "git" {
            return Err(OrchestrationError::TargetOperation(
                TargetOpError::Unsupported,
            ));
        }
        let store = checkout
            .verified_git_source(&request.connection_id)
            .map_err(checkout_error)?;
        let id = ops::resolve(&store, &request.native_id)
            .map_err(|_| OrchestrationError::TargetOperation(TargetOpError::Failed))?
            .ok_or(OrchestrationError::TargetOperation(TargetOpError::NotFound))?
            .id;
        Ok((store, id))
    }

    fn hosted_claim_reply(
        ticket: &hotsheet_model::Ticket,
        connection_id: &str,
    ) -> Result<HostedTicket, OrchestrationError> {
        Ok(HostedTicket {
            ticket: serde_json::to_value(ApiTicket::from_provider(ticket, connection_id, None))
                .map_err(|_| OrchestrationError::Corrupt)?,
            created: false,
        })
    }

    /// Take the exact ticket's claim for `worker`. A ticket another worker holds, or
    /// one that cannot be claimed, is a conflict; nothing is forced.
    pub fn hosted_claim(
        &self,
        checkout_id: &str,
        request: HostedClaim,
        tx: &mut Transaction<'_>,
    ) -> Result<HostedTicket, OrchestrationError> {
        let minutes = lease(request.lease_minutes)?;
        let (store, id) = self.hosted_claim_store(checkout_id, &request)?;
        let now = timestamp(tx);
        let ticket = ops::claim_with_eta(
            &store,
            &id,
            &now,
            after(&now, minutes),
            &request.worker,
            request.label.clone(),
            None,
        )
        .map_err(claim_error)?;
        Self::hosted_claim_reply(&ticket, &request.connection_id)
    }

    pub fn hosted_claim_renew(
        &self,
        checkout_id: &str,
        request: HostedClaim,
        tx: &mut Transaction<'_>,
    ) -> Result<HostedTicket, OrchestrationError> {
        let minutes = lease(request.lease_minutes)?;
        let (store, id) = self.hosted_claim_store(checkout_id, &request)?;
        let now = timestamp(tx);
        let lease_expires = after(&now, minutes);
        let ticket = ops::renew_with_eta(&store, &id, now, lease_expires, &request.worker, None)
            .map_err(claim_error)?;
        Self::hosted_claim_reply(&ticket, &request.connection_id)
    }

    /// Release the worker's own claim. Idempotent when already released; never forces
    /// another holder's claim away.
    pub fn hosted_claim_release(
        &self,
        checkout_id: &str,
        request: HostedClaim,
        tx: &mut Transaction<'_>,
    ) -> Result<HostedTicket, OrchestrationError> {
        let (store, id) = self.hosted_claim_store(checkout_id, &request)?;
        let ticket = ops::release(&store, &id, timestamp(tx), &request.worker, false)
            .map_err(claim_error)?;
        Self::hosted_claim_reply(&ticket, &request.connection_id)
    }

    // ---- Orchestrating half ----

    pub(super) fn runs(&self) -> RunRegistry {
        RunRegistry::new(&self.home)
    }

    /// HS2-7QHWHJ: record one fact in the owner's run event stream. Best effort: a run
    /// operation is never failed because its event could not be written.
    pub(super) fn run_event(&self, owner_id: &str, event: RunEvent) {
        let at_ms = event.at_ms;
        if let Ok(journal) = RunEventJournal::new(&self.home, owner_id) {
            let _ = journal.record(event);
        }
        // HS2-42TARA: whatever changed may change what is ready.
        self.readiness_reevaluate(owner_id, at_ms);
    }

    /// Apply a change to a run and publish what actually changed as events. Every run
    /// update goes through here, so the stream and the record cannot drift apart.
    pub(super) fn run_apply(
        &self,
        owner_id: &str,
        run_id: &str,
        update: RunUpdate,
        tx: &mut Transaction<'_>,
    ) -> Result<Run, OrchestrationError> {
        let before = self.runs().get(owner_id, run_id)?;
        let source = if update.from_target {
            RunEventSource::Target
        } else {
            RunEventSource::Orchestrator
        };
        let actor_id = update.actor_id.clone();
        let after = self.runs().update(owner_id, run_id, update, tx)?;
        let event = |kind, state, reason: Option<String>, at_ms, dedupe: Option<String>| RunEvent {
            at_ms,
            run_id: run_id.into(),
            kind,
            source,
            state,
            reason,
            actor_id: actor_id.clone(),
            dedupe,
            gate_id: None,
            gate_state: None,
        };
        let now_ms = after.updated_at_ms;
        // A full history drops an old entry for each new one, so its length alone does
        // not show a transition (HS2-WBREDM).
        if after.history.len() != before.history.len()
            || after.history.last() != before.history.last()
        {
            // Keyed by position in the run's history: reported once however many
            // callers observed the same transition.
            self.run_event(
                owner_id,
                event(
                    // HS2-WBREDM: a control action is published as what it was.
                    match after.history.last().and_then(|entry| entry.control) {
                        None => RunEventKind::State,
                        Some(RunControl::Pause) => RunEventKind::Paused,
                        Some(RunControl::Resume) => RunEventKind::Resumed,
                        Some(RunControl::Handoff) => RunEventKind::Handoff,
                        Some(RunControl::Correct) => RunEventKind::Corrected,
                    },
                    Some(after.state),
                    after.history.last().and_then(|entry| entry.reason.clone()),
                    now_ms,
                    Some(format!(
                        "state-{}-{}",
                        after.history.len(),
                        after.updated_at_ms
                    )),
                ),
            );
        } else if after.lease_expires_at_ms != before.lease_expires_at_ms
            && after.lease_expires_at_ms.is_some()
        {
            self.run_event(
                owner_id,
                event(RunEventKind::Lease, None, None, now_ms, None),
            );
        }
        if after.cancel_requested && !before.cancel_requested {
            self.run_event(
                owner_id,
                event(RunEventKind::CancelRequested, None, None, now_ms, None),
            );
        }
        if let Some(observed) = after
            .observed
            .as_ref()
            .filter(|observed| before.observed.as_ref() != Some(*observed))
        {
            // Only a change of facts is news; re-reading the same facts is not.
            if before
                .observed
                .as_ref()
                .is_none_or(|previous| !previous.same_facts(observed))
            {
                self.run_event(
                    owner_id,
                    event(
                        RunEventKind::Observed,
                        None,
                        Some(observed.status.clone()),
                        observed.at_ms,
                        // Keyed by the facts, so two readers of one change record it once
                        // while a later change at the same instant is still its own event.
                        Some(format!(
                            "observed-{}-{}-{}-{}",
                            observed.at_ms,
                            observed.status,
                            observed.claimed_by.as_deref().unwrap_or("none"),
                            observed.lease_expires_at_ms.unwrap_or(0)
                        )),
                    ),
                );
            }
        }
        match (&before.unreachable, &after.unreachable) {
            (None, Some(down)) => self.run_event(
                owner_id,
                event(
                    RunEventKind::TargetUnreachable,
                    None,
                    Some(down.reason.clone()),
                    down.since_ms,
                    None,
                ),
            ),
            (Some(_), None) => self.run_event(
                owner_id,
                event(RunEventKind::TargetReachable, None, None, now_ms, None),
            ),
            _ => {}
        }
        // HS2-WBREDM: a reconciliation that reached a new outcome is news; re-checking
        // an unchanged world stored nothing and reports nothing.
        if let Some(found) = after
            .reconciliation
            .as_ref()
            .filter(|found| before.reconciliation.as_ref() != Some(*found))
        {
            self.run_event(
                owner_id,
                RunEvent {
                    at_ms: found.at_ms,
                    run_id: run_id.into(),
                    kind: RunEventKind::Reconciled,
                    source: RunEventSource::Orchestrator,
                    // The run's state did not change by being reconciled; a state
                    // change it led to is its own `state` event.
                    state: None,
                    reason: Some(found.disposition.code().into()),
                    actor_id: actor_id.clone(),
                    dedupe: None,
                    gate_id: None,
                    gate_state: None,
                },
            );
        }
        Ok(after)
    }

    /// The run's status for a viewer, without contacting the target: the record, the
    /// last confirmed target state and whether it is stale.
    pub fn run_status(
        &self,
        owner_id: &str,
        run_id: &str,
    ) -> Result<RunStatus, OrchestrationError> {
        let run = self.run(owner_id, run_id)?;
        Ok(RunStatus {
            reachability: match (&run.unreachable, &run.observed) {
                (Some(_), _) => "unreachable",
                (None, Some(_)) => "reachable",
                (None, None) => "unknown",
            },
            stale: run.unreachable.is_some() || run.observed.is_none(),
            last_confirmed_at_ms: run.observed.as_ref().map(|observed| observed.at_ms),
            provisional: self.provisional_on_target(owner_id, &run.target),
            run,
        })
    }

    /// Events after a cursor, for one run or all of the owner's. With `wait_ms`, an
    /// empty answer waits for the next event (woken by the journal changing, never by
    /// a timer loop) and returns empty with the same cursor at the deadline.
    pub fn run_events(
        &self,
        owner_id: &str,
        after: u64,
        limit: usize,
        run_id: Option<&str>,
        wait_ms: u64,
    ) -> Result<RunEventPage, OrchestrationError> {
        let journal = RunEventJournal::new(&self.home, owner_id)?;
        if wait_ms == 0 {
            return journal.read(after, limit, run_id);
        }
        // Watch before reading, so an event appended in between is not missed.
        let waiter = hotsheet_jobs::ChangeWaiter::new(journal.directory())?;
        let deadline = std::time::Instant::now()
            + std::time::Duration::from_millis(wait_ms.min(MAX_EVENT_WAIT_MS));
        loop {
            let page = journal.read(after, limit, run_id)?;
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if !page.entries.is_empty() || page.gap || remaining.is_zero() {
                return Ok(page);
            }
            waiter.wait(remaining);
        }
    }

    pub fn run(&self, owner_id: &str, run_id: &str) -> Result<Run, OrchestrationError> {
        self.runs().get(owner_id, run_id)
    }

    pub fn run_list(&self, owner_id: &str, active: bool) -> Result<Vec<Run>, OrchestrationError> {
        self.runs().list(owner_id, active)
    }

    pub(super) fn run_target(run: &Run) -> TargetTicket {
        TargetTicket {
            target_id: run.target.target_id.clone(),
            connection_id: run.target.connection_id.clone(),
            native_id: run.target.native_id.clone(),
        }
    }

    /// An identifiers-only activity note on the orchestration ticket. Best effort: a
    /// run is never failed because its note could not be written.
    pub(super) fn run_note(
        &self,
        owner_id: &str,
        run: &Run,
        event: &str,
        tx: &mut Transaction<'_>,
    ) {
        let ticket = OrchestrationTicket {
            connection_id: run.ticket.connection_id.clone(),
            native_id: run.ticket.native_id.clone(),
        };
        if let Ok((store, id, _)) = self.orchestration_store(owner_id, &ticket) {
            let _ = ops::add_note(
                &store,
                &id,
                hotsheet_ticketing::mint_ulid(tx.clock, tx.rng),
                timestamp(tx),
                NoteKind::Activity,
                format!(
                    "Orchestration run `{}` {event}: target `{}`, ticket `{}:{}`.",
                    run.run_id,
                    run.target.target_id,
                    run.target.connection_id,
                    run.target.native_id
                ),
            );
        }
    }

    /// Dispatch work on one exact target ticket. Repeating the request with the same
    /// operation id returns the same run and never claims twice.
    pub fn run_dispatch(
        &self,
        owner_id: &str,
        request: DispatchRun,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<RunView, OrchestrationError> {
        identifier(&request.operation_id, "bounded operation id required")?;
        identifier(
            &request.target.connection_id,
            "bounded connection id required",
        )?;
        native(&request.target.native_id)?;
        let mode = request.mode.unwrap_or(DispatchMode::Direct);
        let minutes = lease(request.lease_minutes)?;
        match (mode, &request.worker_id) {
            (DispatchMode::Direct, None) => {
                return Err(OrchestrationError::Invalid(
                    "a direct dispatch names its worker",
                ));
            }
            (DispatchMode::Dispatcher, Some(_)) => {
                return Err(OrchestrationError::Invalid(
                    "the target's dispatcher chooses the worker",
                ));
            }
            _ => {}
        }
        if let Some(worker) = &request.worker_id {
            identifier(worker, "bounded worker id required")?;
        }
        let target = Self::target_ref(owner_id, &request.target.target_id)?;
        let (store, source_id, source_connection) =
            self.orchestration_store(owner_id, &request.ticket)?;
        let authorized = self.authorize(
            &target,
            actor,
            ActionRequest::new(TargetAction::WorkStart).operation(request.operation_id.clone()),
            tx,
        )?;
        let outcome = (|| {
            Self::check_source(&authorized, &request.target.connection_id)?;
            let destination = Self::destination(&authorized)?;
            // Read first: a ticket that does not exist is never recorded as a run, and
            // the run names the provider's canonical id whatever the caller typed.
            let seen: HostedTicket = serde_json::from_value(self.hosted_call(
                destination,
                "GET",
                &format!(
                    "?connection_id={}&native_id={}",
                    encode(&request.target.connection_id),
                    encode(&request.target.native_id)
                ),
                None,
                tx,
            )?)
            .map_err(|_| OrchestrationError::Observation(TargetFailure::InvalidResponse))?;
            let native_id = text(&seen.ticket, "native_id")
                .unwrap_or(&request.target.native_id)
                .to_owned();
            // HS2-42TARA: planned work starts only when it is ready, or when a person
            // overrides that for this exact operation.
            self.readiness_guard(
                owner_id,
                &super::readiness::GuardedDispatch {
                    target: &RunTarget {
                        target_id: request.target.target_id.clone(),
                        connection_id: request.target.connection_id.clone(),
                        native_id: native_id.clone(),
                    },
                    found: &seen.ticket,
                    operation_id: &request.operation_id,
                    override_readiness: request.override_readiness,
                },
                &authorized,
                actor,
                tx,
            )?;
            let (run, created) = self.runs().begin(
                owner_id,
                NewRun {
                    operation_id: request.operation_id.clone(),
                    ticket: RunTicket {
                        connection_id: source_connection.clone(),
                        native_id: source_id.to_string(),
                    },
                    target: RunTarget {
                        target_id: request.target.target_id.clone(),
                        connection_id: request.target.connection_id.clone(),
                        native_id: native_id.clone(),
                    },
                    mode,
                    worker_id: request.worker_id.clone(),
                },
                actor.and_then(|actor| actor.id.clone()),
                tx,
            )?;
            // A duplicate of a dispatch that already went past `requested`.
            if !created && run.state != RunState::Requested {
                return Ok(RunView::from(run));
            }
            // HS2-WBREDM: a paused run asks nothing of its target.
            if run.paused.is_some() {
                return Err(OrchestrationError::Run(RunError::Paused));
            }
            let dispatched = match mode {
                DispatchMode::Direct => {
                    let body = serde_json::to_value(HostedClaim {
                        connection_id: run.target.connection_id.clone(),
                        native_id: native_id.clone(),
                        worker: request.worker_id.clone().unwrap_or_default(),
                        label: request.worker_label.clone(),
                        lease_minutes: Some(minutes),
                    })
                    .map_err(|_| OrchestrationError::Corrupt)?;
                    self.hosted_call_at(destination, "/target-claims", "POST", "", Some(&body), tx)
                        .and_then(|reply| {
                            serde_json::from_value::<HostedTicket>(reply).map_err(|_| {
                                OrchestrationError::Observation(TargetFailure::InvalidResponse)
                            })
                        })
                        .map(|hosted| RunUpdate {
                            to: Some(RunState::Dispatched),
                            reason: Some("claimed".into()),
                            lease_expires_at_ms: Some(field_ms(
                                &hosted.ticket,
                                "claim_lease_expires_at",
                            )),
                            ..RunUpdate::default()
                        })
                }
                DispatchMode::Dispatcher => {
                    let body = json!({"connection_id": run.target.connection_id,
                        "native_id": native_id, "up_next": true});
                    self.hosted_call(destination, "PATCH", "", Some(&body), tx)
                        .map(|_| RunUpdate {
                            to: Some(RunState::Queued),
                            reason: Some("queued_for_dispatcher".into()),
                            ..RunUpdate::default()
                        })
                }
            };
            let update = match dispatched {
                Ok(update) => update,
                // The target said no for good: the run ends, and the same operation id
                // keeps answering with that failed run.
                Err(OrchestrationError::TargetOperation(
                    error @ (TargetOpError::Conflict
                    | TargetOpError::Unsupported
                    | TargetOpError::NotFound
                    | TargetOpError::Invalid
                    | TargetOpError::Disabled),
                )) => {
                    self.run_apply(
                        owner_id,
                        &run.run_id,
                        RunUpdate {
                            to: Some(RunState::Failed),
                            reason: Some(error.code().into()),
                            ..RunUpdate::default()
                        },
                        tx,
                    )?;
                    return Err(OrchestrationError::TargetOperation(error));
                }
                // Anything else may be transient: the run stays `requested` so the
                // same request can be repeated.
                Err(error) => return Err(error),
            };
            let run = self.run_apply(
                owner_id,
                &run.run_id,
                RunUpdate {
                    actor_id: actor.and_then(|actor| actor.id.clone()),
                    ..update
                },
                tx,
            )?;
            let _ = ops::upsert_orchestration_link(
                &store,
                &source_id,
                OrchestrationLink {
                    target_id: run.target.target_id.clone(),
                    connection_id: run.target.connection_id.clone(),
                    native_id: Some(native_id),
                    relation: OrchestrationRelation::Observed,
                    operation_id: None,
                },
                timestamp(tx),
            );
            self.run_note(
                owner_id,
                &run,
                match mode {
                    DispatchMode::Direct => "dispatched to its worker",
                    DispatchMode::Dispatcher => "queued for the target's dispatcher",
                },
                tx,
            );
            Ok(RunView::from(run))
        })();
        self.finish(&target, &authorized, &outcome, tx)?;
        outcome
    }

    /// Extend the lease of a directly dispatched run.
    pub fn run_renew(
        &self,
        owner_id: &str,
        run_id: &str,
        lease_minutes: Option<u32>,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<RunView, OrchestrationError> {
        let minutes = lease(lease_minutes)?;
        let run = self.run(owner_id, run_id)?;
        if run.mode != DispatchMode::Direct || run.state != RunState::Dispatched {
            return Err(OrchestrationError::Run(RunError::NotActive));
        }
        // HS2-WBREDM: a paused run's lease is not extended on the orchestrator's say-so.
        if run.paused.is_some() {
            return Err(OrchestrationError::Run(RunError::Paused));
        }
        let target = Self::target_ref(owner_id, &run.target.target_id)?;
        let authorized = self.authorize(
            &target,
            actor,
            ActionRequest::new(TargetAction::WorkStart)
                .operation(run.operation_id.clone())
                .continuing(),
            tx,
        )?;
        let outcome = (|| {
            let body = serde_json::to_value(HostedClaim {
                connection_id: run.target.connection_id.clone(),
                native_id: run.target.native_id.clone(),
                worker: run.worker_id.clone().unwrap_or_default(),
                label: None,
                lease_minutes: Some(minutes),
            })
            .map_err(|_| OrchestrationError::Corrupt)?;
            let hosted: HostedTicket = serde_json::from_value(self.hosted_call_at(
                Self::destination(&authorized)?,
                "/target-claims",
                "PATCH",
                "",
                Some(&body),
                tx,
            )?)
            .map_err(|_| OrchestrationError::Observation(TargetFailure::InvalidResponse))?;
            self.run_apply(
                owner_id,
                run_id,
                RunUpdate {
                    lease_expires_at_ms: Some(field_ms(&hosted.ticket, "claim_lease_expires_at")),
                    ..RunUpdate::default()
                },
                tx,
            )
            .map(RunView::from)
        })();
        self.finish(&target, &authorized, &outcome, tx)?;
        outcome
    }

    /// Cancel a run: release its claim, or withdraw the Up Next mark it set. If the
    /// target cannot be told, the intent is recorded and the answer says so; the run
    /// stays active until a later cancel or sync confirms it.
    pub fn run_cancel(
        &self,
        owner_id: &str,
        run_id: &str,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<RunView, OrchestrationError> {
        let run = self.run(owner_id, run_id)?;
        if run.state.is_terminal() {
            return Err(OrchestrationError::Run(RunError::NotActive));
        }
        // HS2-HV9ED3: a person's cancel is recorded as theirs; the scheduler respects it.
        let reason = if actor
            .is_some_and(|actor| actor.role == hotsheet_ticketing::actor::ActorRole::Human)
        {
            super::cancel_hold::CANCELLED_BY_PERSON
        } else {
            "cancelled"
        };
        let cancelled = |tx: &mut Transaction<'_>| {
            self.run_apply(
                owner_id,
                run_id,
                RunUpdate {
                    to: Some(RunState::Cancelled),
                    reason: Some(reason.into()),
                    actor_id: actor.and_then(|actor| actor.id.clone()),
                    ..RunUpdate::default()
                },
                tx,
            )
        };
        // Nothing is held in the target for these states.
        let holds_claim = run.mode == DispatchMode::Direct && run.state == RunState::Dispatched;
        let holds_queue = run.mode == DispatchMode::Dispatcher && run.state == RunState::Queued;
        if !holds_claim && !holds_queue {
            let run = cancelled(tx)?;
            self.run_note(owner_id, &run, "cancelled", tx);
            return Ok(run.into());
        }
        let target = Self::target_ref(owner_id, &run.target.target_id)?;
        let told = self
            .authorize(
                &target,
                actor,
                ActionRequest::new(TargetAction::WorkStart)
                    .operation(run.operation_id.clone())
                    .continuing(),
                tx,
            )
            .and_then(|authorized| {
                let destination = Self::destination(&authorized)?;
                let outcome = if holds_claim {
                    let body = serde_json::to_value(HostedClaim {
                        connection_id: run.target.connection_id.clone(),
                        native_id: run.target.native_id.clone(),
                        worker: run.worker_id.clone().unwrap_or_default(),
                        label: None,
                        lease_minutes: None,
                    })
                    .map_err(|_| OrchestrationError::Corrupt)?;
                    self.hosted_call_at(
                        destination,
                        "/target-claims",
                        "DELETE",
                        "",
                        Some(&body),
                        tx,
                    )
                } else {
                    let body = json!({"connection_id": run.target.connection_id,
                        "native_id": run.target.native_id, "up_next": false});
                    self.hosted_call(destination, "PATCH", "", Some(&body), tx)
                };
                self.finish(&target, &authorized, &outcome, tx)?;
                outcome
            });
        match told {
            // A claim someone else now holds is theirs; the run just lets go.
            Ok(_) | Err(OrchestrationError::TargetOperation(TargetOpError::Conflict)) => {
                let run = cancelled(tx)?;
                self.run_note(owner_id, &run, "cancelled", tx);
                Ok(run.into())
            }
            // HS2-WC3W2G: the gate refused this actor. That is the answer, typed and
            // audited by the gate; the run records nothing and no later
            // reconciliation repeats a cancel its caller was not allowed to make. A
            // target that could not be resolved is not a refusal of the actor.
            Err(error @ OrchestrationError::Denied(reason))
                if reason != DenialReason::TargetUnavailable =>
            {
                Err(error)
            }
            Err(_) => {
                let run = self.run_apply(
                    owner_id,
                    run_id,
                    RunUpdate {
                        cancel_requested: Some(true),
                        ..RunUpdate::default()
                    },
                    tx,
                )?;
                Ok(RunView {
                    run,
                    unverified: vec![if holds_claim {
                        "target_claim"
                    } else {
                        "target_queue"
                    }],
                })
            }
        }
    }

    /// Read the target ticket and bring the run in line with it: completed when the
    /// ticket is, dispatched when the target's dispatcher assigned it, and intervened
    /// when a person or another worker holds or released the claim. A successful read
    /// becomes the run's last confirmed observation; a target that cannot be confirmed
    /// marks the run unreachable and leaves that observation in place as stale.
    pub fn run_sync(
        &self,
        owner_id: &str,
        run_id: &str,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<RunView, OrchestrationError> {
        let run = self.run(owner_id, run_id)?;
        if run.state.is_terminal() {
            return Ok(run.into());
        }
        let actor_id = actor.and_then(|actor| actor.id.clone());
        let unreachable = |reason: &'static str, tx: &mut Transaction<'_>| {
            let _ = self.run_apply(
                owner_id,
                run_id,
                RunUpdate {
                    unreachable: Some(Some(RunUnreachable {
                        since_ms: tx.clock.now_ms(),
                        reason: reason.into(),
                    })),
                    from_target: true,
                    ..RunUpdate::default()
                },
                tx,
            );
        };
        let target = Self::target_ref(owner_id, &run.target.target_id)?;
        let authorized = match self.authorize(
            &target,
            actor,
            ActionRequest::new(TargetAction::TicketsRead),
            tx,
        ) {
            Ok(authorized) => authorized,
            Err(error) => {
                // Lost access to the target is an outage of the view, not just a refusal.
                if matches!(
                    error,
                    OrchestrationError::Denied(DenialReason::TargetUnavailable)
                ) {
                    unreachable("target_unavailable", tx);
                }
                return Err(error);
            }
        };
        let outcome = (|| {
            let ticket = match self.read_authorized(&authorized, &Self::run_target(&run), tx) {
                Ok(ticket) => ticket,
                // The ticket is gone from the target: nothing is left to supervise.
                Err(OrchestrationError::TargetOperation(TargetOpError::NotFound)) => {
                    return self
                        .run_apply(
                            owner_id,
                            run_id,
                            RunUpdate {
                                to: Some(RunState::Failed),
                                reason: align(&run, &TargetFacts::missing(), tx.clock.now_ms())
                                    .reason
                                    .map(Into::into),
                                actor_id: actor_id.clone(),
                                from_target: true,
                                ..RunUpdate::default()
                            },
                            tx,
                        )
                        .map(RunView::from);
                }
                Err(error) => {
                    unreachable(error.code(), tx);
                    return Err(error);
                }
            };
            let holder = text(&ticket, "claimed_by");
            let lease = field_ms(&ticket, "claim_lease_expires_at");
            let status = text(&ticket, "status").unwrap_or_default();
            // HS2-WBREDM: the alignment rules live in the pure core, shared with
            // reconciliation.
            let aligned = align(
                &run,
                &TargetFacts {
                    exists: true,
                    status: status.into(),
                    claimed_by: holder.map(Into::into),
                    lease_expires_at_ms: lease,
                },
                tx.clock.now_ms(),
            );
            let update = RunUpdate {
                to: aligned.to,
                reason: aligned.reason.map(Into::into),
                worker_id: aligned.worker_id,
                lease_expires_at_ms: aligned.lease_expires_at_ms,
                ..RunUpdate::default()
            };
            let finished = update.to.is_some_and(RunState::is_terminal);
            let updated = self.run_apply(
                owner_id,
                run_id,
                RunUpdate {
                    actor_id: actor_id.clone(),
                    observed: Some(RunObservation {
                        at_ms: tx.clock.now_ms(),
                        status: status.into(),
                        started_phase: text(&ticket, "started_phase").map(Into::into),
                        claimed_by: holder.map(Into::into),
                        lease_expires_at_ms: lease,
                    }),
                    unreachable: Some(None),
                    from_target: true,
                    ..update
                },
                tx,
            )?;
            if finished {
                self.run_note(owner_id, &updated, "finished", tx);
            }
            Ok(RunView::from(updated))
        })();
        // HS2-2C5N1W: a concurrent sync (the server's follower) finished the run while
        // this one read the target. A sync of a finished run answers the record, so
        // this one does too, rather than `run_not_active`.
        let outcome = match outcome {
            Err(OrchestrationError::Run(RunError::NotActive)) => self
                .run(owner_id, run_id)
                .ok()
                .filter(|run| run.state.is_terminal())
                .map(RunView::from)
                .ok_or(OrchestrationError::Run(RunError::NotActive)),
            other => other,
        };
        self.finish(&target, &authorized, &outcome, tx)?;
        outcome
    }
}
