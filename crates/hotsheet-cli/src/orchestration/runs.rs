//! HS2-EBHDSB: dispatching work to an exact target ticket and supervising the run.
//!
//! The target ticket's own claim is the only lock. `direct` takes that claim for a
//! named worker through the target's claim and lease operations; `dispatcher` marks
//! the ticket Up Next and lets the target's own dispatcher assign it. Integration and
//! pushing stay with the target's workflow.
use hotsheet_model::{NoteKind, OrchestrationLink, OrchestrationRelation, Timestamp, Ulid};
use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::{
    DispatchMode, NewRun, Run, RunError, RunRegistry, RunState, RunTarget, RunTicket, RunUpdate,
    TargetAction, TargetOpError,
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
fn field_ms(ticket: &Value, key: &str) -> Option<u64> {
    let text = ticket.get(key)?.as_str()?;
    let at = Timestamp::new(text).instant()?;
    u64::try_from(at.unix_timestamp_nanos() / 1_000_000).ok()
}

fn text<'a>(ticket: &'a Value, key: &str) -> Option<&'a str> {
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

    fn runs(&self) -> RunRegistry {
        RunRegistry::new(&self.home)
    }

    pub fn run(&self, owner_id: &str, run_id: &str) -> Result<Run, OrchestrationError> {
        self.runs().get(owner_id, run_id)
    }

    pub fn run_list(&self, owner_id: &str, active: bool) -> Result<Vec<Run>, OrchestrationError> {
        self.runs().list(owner_id, active)
    }

    fn run_target(run: &Run) -> TargetTicket {
        TargetTicket {
            target_id: run.target.target_id.clone(),
            connection_id: run.target.connection_id.clone(),
            native_id: run.target.native_id.clone(),
        }
    }

    /// An identifiers-only activity note on the orchestration ticket. Best effort: a
    /// run is never failed because its note could not be written.
    fn run_note(&self, owner_id: &str, run: &Run, event: &str, tx: &mut Transaction<'_>) {
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
                    self.runs().update(
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
            let run = self.runs().update(
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
            self.runs()
                .update(
                    owner_id,
                    run_id,
                    RunUpdate {
                        lease_expires_at_ms: Some(field_ms(
                            &hosted.ticket,
                            "claim_lease_expires_at",
                        )),
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
        let cancelled = |tx: &mut Transaction<'_>| {
            self.runs().update(
                owner_id,
                run_id,
                RunUpdate {
                    to: Some(RunState::Cancelled),
                    reason: Some("cancelled".into()),
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
            Err(_) => {
                let run = self.runs().update(
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
    /// when a person or another worker holds or released the claim.
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
        let target = Self::target_ref(owner_id, &run.target.target_id)?;
        let authorized = self.authorize(
            &target,
            actor,
            ActionRequest::new(TargetAction::TicketsRead),
            tx,
        )?;
        let outcome = (|| {
            let ticket = self.read_authorized(&authorized, &Self::run_target(&run), tx)?;
            let holder = text(&ticket, "claimed_by");
            let lease = field_ms(&ticket, "claim_lease_expires_at");
            // A lapsed lease is not a live claim, whoever's name is still on it.
            let live = holder.filter(|_| lease.is_some_and(|at| at > tx.clock.now_ms()));
            let ours = live.is_some() && live == run.worker_id.as_deref();
            let status = text(&ticket, "status").unwrap_or_default();
            let step = |to, reason: &str| RunUpdate {
                to: Some(to),
                reason: Some(reason.into()),
                ..RunUpdate::default()
            };
            let update = match (status, run.state) {
                ("completed" | "verified" | "archive", _) => {
                    Some(step(RunState::Completed, "target_completed"))
                }
                ("deleted", _) => Some(step(RunState::Failed, "target_deleted")),
                (_, RunState::Queued) => live.map(|worker| RunUpdate {
                    worker_id: Some(worker.into()),
                    lease_expires_at_ms: Some(lease),
                    ..step(RunState::Dispatched, "assigned_by_dispatcher")
                }),
                // A claim taken before the answer was recorded is the run's own.
                (_, RunState::Requested | RunState::Intervened)
                    if ours && run.mode == DispatchMode::Direct =>
                {
                    Some(RunUpdate {
                        lease_expires_at_ms: Some(lease),
                        ..step(RunState::Dispatched, "claim_confirmed")
                    })
                }
                (_, RunState::Dispatched) if ours => Some(RunUpdate {
                    lease_expires_at_ms: Some(lease),
                    ..RunUpdate::default()
                }),
                (_, RunState::Dispatched) => Some(step(
                    RunState::Intervened,
                    if live.is_some() {
                        "claim_taken"
                    } else {
                        "claim_released"
                    },
                )),
                _ => None,
            };
            let Some(update) = update else {
                return Ok(RunView::from(run.clone()));
            };
            let finished = update.to.is_some_and(RunState::is_terminal);
            let updated = self.runs().update(
                owner_id,
                run_id,
                RunUpdate {
                    actor_id: actor.and_then(|actor| actor.id.clone()),
                    ..update
                },
                tx,
            )?;
            if finished {
                self.run_note(owner_id, &updated, "finished", tx);
            }
            Ok(RunView::from(updated))
        })();
        self.finish(&target, &authorized, &outcome, tx)?;
        outcome
    }
}
