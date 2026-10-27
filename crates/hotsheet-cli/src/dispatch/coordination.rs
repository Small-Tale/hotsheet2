//! Headless local coordination; no server, provider tokens, or AI process launcher.

use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, Subcommand};
use hotsheet_cli::coordination::{
    Assignment, Coordinator, HandoffEvidence, Phase, ReturnRoute, Role, WaitRequest,
};
use hotsheet_ticketing::TicketRef;
use hotsheet_ticketing::{FsStore, git_connection_id, ops};
use serde::Deserialize;

use super::{Ctx, misrouted};
use crate::Cmd;

#[path = "coordination_claims.rs"]
mod claims;

#[derive(Debug, Args)]
pub(crate) struct CoordinationArgs {
    /// Shipping checkout; all worker worktrees share its Git common directory.
    #[arg(long)]
    project: PathBuf,
    #[command(subcommand)]
    command: CoordinationCmd,
}

#[derive(Debug, Args)]
struct Identity {
    #[arg(long)]
    id: String,
    /// Stable token for this agent invocation; do not reuse across replacement sessions.
    #[arg(long)]
    session: String,
}

#[derive(Debug, Args)]
struct Registration {
    #[command(flatten)]
    identity: Identity,
    #[arg(long, value_enum)]
    role: Role,
    /// native:<agent-id>, terminal:<verified-connection-id>, or human.
    #[arg(long, default_value = "human")]
    route: ReturnRoute,
    /// Availability only, distinct from ticket claim leases.
    #[arg(long, default_value_t = 120)]
    lease_seconds: i64,
}

#[derive(Debug, Subcommand)]
enum CoordinationCmd {
    /// Create/open the local state idempotently; --resume explicitly reopens a stopped run.
    Setup {
        #[arg(long)]
        resume: bool,
    },
    /// Read durable workers, assignments, handoffs and local queue facts as JSON.
    Status,
    /// Announce this live session without blocking (renew before long implementation/gates).
    Register(Registration),
    /// Announce and wait for a worker's exact assignment, or an integrator's changed snapshot.
    /// Uses filesystem events, and retains availability while this process remains running.
    Next {
        #[command(flatten)]
        registration: Registration,
        /// Integrator's last snapshot cursor; omit on its first call.
        #[arg(long)]
        after: Option<String>,
    },
    /// Reserve an exact eligible ticket for an available worker; inspect scope/claims first.
    Assign {
        #[command(flatten)]
        integrator: Identity,
        #[arg(long)]
        worker: String,
        #[arg(long)]
        worker_session: String,
        /// Stable retry/idempotency key for this assignment.
        #[arg(long)]
        operation: String,
        #[arg(long)]
        connection: String,
        #[arg(long)]
        native_id: String,
        #[arg(long)]
        slug: String,
        #[arg(long)]
        base: String,
        /// Exact route for this operation. Omit to use the integrator's registered route.
        #[arg(long, requires = "route_evidence_note")]
        return_route: Option<ReturnRoute>,
        /// Regular note on this ticket with exact addressed route evidence.
        #[arg(long, requires = "return_route")]
        route_evidence_note: Option<String>,
    },
    /// Record exact claim acceptance, verified durable handoff/release, or a blocked report.
    Worker {
        #[command(flatten)]
        worker: Identity,
        #[arg(long)]
        assignment: String,
        #[arg(long, value_enum)]
        phase: Phase,
        #[arg(long)]
        commit: Option<String>,
        /// Exact durable `## Findings handoff` note id for a no-code handoff.
        #[arg(long, conflicts_with = "commit")]
        findings_note: Option<String>,
    },
    /// Record serialized integration, pending CI, completion, or explicit blocked recovery.
    Mark {
        #[command(flatten)]
        integrator: Identity,
        #[arg(long)]
        assignment: String,
        #[arg(long, value_enum)]
        phase: Phase,
    },
    /// Release a stale reservation only after inspecting/preserving ticket and worktree artifacts.
    Recover {
        #[command(flatten)]
        integrator: Identity,
        #[arg(long)]
        assignment: String,
        /// Durable ticket note documenting preserved branch/SHA/worktree and the recovery decision.
        #[arg(long)]
        evidence_note: String,
        #[arg(long, required = true)]
        inspected: bool,
    },
    /// Preserve a reviewed handoff and allow a fresh structured correction assignment.
    ReviewReturn {
        #[command(flatten)]
        integrator: Identity,
        #[arg(long)]
        assignment: String,
        /// Exact ticket regular Review return note naming assignment and full reviewed SHA.
        #[arg(long)]
        evidence_note: String,
        #[arg(long, required = true)]
        inspected: bool,
    },
    /// End only when work/CI are not in flight. Never treat unavailable provider data as empty.
    Finish {
        #[command(flatten)]
        integrator: Identity,
        #[arg(long, value_parser = ["drained", "blocked"])]
        reason: String,
    },
    /// Withdraw availability on every final exit; assignments/handoffs are preserved.
    Leave {
        #[command(flatten)]
        identity: Identity,
        #[arg(long, value_enum)]
        role: Role,
    },
}

pub(super) fn run(cmd: Cmd, ctx: &Ctx) -> Result<()> {
    let Cmd::Coordination(args) = cmd else {
        misrouted("coordination")
    };
    let coordinator = Coordinator::open(&args.project, &ctx.path)?;
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    match args.command {
        CoordinationCmd::Setup { resume } => {
            if resume {
                coordinator.resume()?;
            }
        }
        CoordinationCmd::Status => {}
        CoordinationCmd::Register(r) => {
            coordinator.register(
                r.role,
                &r.identity.id,
                &r.identity.session,
                now,
                r.lease_seconds,
                r.route,
            )?;
        }
        CoordinationCmd::Next {
            registration: r,
            after,
        } => {
            let snapshot = coordinator.next(&WaitRequest {
                role: r.role,
                id: r.identity.id,
                generation: r.identity.session,
                lease_seconds: r.lease_seconds,
                return_route: r.route,
                after,
            })?;
            println!("{}", serde_json::to_string_pretty(&snapshot)?);
            return Ok(());
        }
        CoordinationCmd::Assign {
            integrator: i,
            worker,
            worker_session,
            operation,
            connection,
            native_id,
            slug,
            base,
            return_route: route_override,
            route_evidence_note,
        } => {
            let state = coordinator.state()?;
            anyhow::ensure!(
                ctx.path
                    .join(hotsheet_ticketing::STORE_METADATA_FILE)
                    .exists(),
                "provider dispatch verification is unsupported for this selected source (HS2-3ETJ26)"
            );
            let store = FsStore::open_without_maintenance(&ctx.path)?;
            anyhow::ensure!(
                connection == git_connection_id(&store),
                "provider dispatch verification is unsupported for this connection (HS2-3ETJ26); {}",
                coordinator.unsupported_source(&connection)?
            );
            let existing = state.assignments.get(&operation);
            let ticket = store.read_ticket(&native_id.parse()?)?;
            if existing.is_none() {
                let tickets = store.list_tickets()?;
                claims::assignable(&ticket, &tickets, &worker, time::OffsetDateTime::now_utc())?;
            }
            if let (Some(route), Some(note_id)) = (&route_override, &route_evidence_note) {
                verify_addressed_route(
                    &ticket,
                    note_id,
                    &operation,
                    &worker,
                    &worker_session,
                    route,
                    &i.id,
                )?;
            }
            let default_route = state
                .integrator
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("register integrator first"))?
                .return_route
                .clone();
            // A retry without an override keeps the original operation's route even
            // when the integrator has since registered a different default (HS2-HM9FRV).
            let return_route = route_override.unwrap_or_else(|| {
                existing
                    .map(|assignment| assignment.return_route.clone())
                    .unwrap_or(default_route)
            });
            coordinator.assign(
                &i.id,
                &i.session,
                now,
                Assignment {
                    id: operation,
                    worker,
                    generation: worker_session,
                    ticket: TicketRef {
                        connection_id: connection,
                        native_id,
                    },
                    slug,
                    base,
                    phase: Phase::Offered,
                    commit: None,
                    handoff: None,
                    return_route,
                    recovery_note: None,
                },
            )?;
        }
        CoordinationCmd::Worker {
            worker: w,
            assignment,
            phase,
            commit,
            findings_note,
        } => {
            if matches!(phase, Phase::Accepted | Phase::Handoff | Phase::Blocked) {
                anyhow::ensure!(
                    ctx.path
                        .join(hotsheet_ticketing::STORE_METADATA_FILE)
                        .exists(),
                    "provider claim/handoff verification is unsupported for this selected source (HS2-3ETJ26)"
                );
                let state = coordinator.state()?;
                let a = state
                    .assignments
                    .get(&assignment)
                    .ok_or_else(|| anyhow::anyhow!("unknown assignment"))?;
                let store = FsStore::open_without_maintenance(&ctx.path)?;
                anyhow::ensure!(
                    a.ticket.connection_id == git_connection_id(&store),
                    "provider claim/handoff verification is unsupported for this source; do not treat mailbox acknowledgement as a ticket claim; {}",
                    coordinator.unsupported_source(&a.ticket.connection_id)?
                );
                let ticket = store.read_ticket(&a.ticket.native_id.parse()?)?;
                if phase == Phase::Accepted {
                    let tickets = store.list_tickets()?;
                    claims::accepted(&ticket, &tickets, &w.id, time::OffsetDateTime::now_utc())?;
                } else {
                    anyhow::ensure!(
                        ticket.claimed_by.is_none(),
                        "release the ticket claim before recording mailbox handoff"
                    );
                    if phase == Phase::Handoff {
                        if let Some(note_id) = findings_note.as_deref() {
                            let note_id = hotsheet_model::Ulid::from_string(note_id)?;
                            anyhow::ensure!(
                                ticket.notes.iter().any(|note| {
                                    note.id == note_id
                                        && note.kind == hotsheet_model::NoteKind::Regular
                                        && note.text.lines().next() == Some("## Findings handoff")
                                        && note
                                            .text
                                            .lines()
                                            .skip(1)
                                            .any(|line| !line.trim().is_empty())
                                }),
                                "durable Findings handoff note must exist on this exact ticket with findings"
                            );
                            anyhow::ensure!(
                                ticket.claim_history.last().is_some_and(|event| {
                                    event.kind == hotsheet_model::ClaimEventKind::Release
                                        && event.worker == w.id
                                }),
                                "findings handoff requires this worker's recorded claim release"
                            );
                        } else {
                            anyhow::ensure!(
                                commit.as_ref().is_some_and(|sha| ticket
                                    .notes
                                    .iter()
                                    .any(|n| n.text.contains("## Integration handoff")
                                        && n.text.contains(sha))),
                                "durable Integration handoff note must name this full commit before mailbox handoff"
                            );
                        }
                    }
                }
            }
            if let Some(note_id) = findings_note {
                anyhow::ensure!(phase == Phase::Handoff, "findings note is only for handoff");
                coordinator.worker_findings_transition(&w.id, &w.session, &assignment, note_id)?;
            } else {
                coordinator.worker_transition(&w.id, &w.session, &assignment, phase, commit)?;
            }
        }
        CoordinationCmd::Recover {
            integrator: i,
            assignment,
            evidence_note,
            inspected,
        } => {
            anyhow::ensure!(inspected, "inspect and preserve artifacts before recovery");
            anyhow::ensure!(
                ctx.path
                    .join(hotsheet_ticketing::STORE_METADATA_FILE)
                    .exists(),
                "provider recovery verification is unsupported for this selected source (HS2-3ETJ26)"
            );
            let state = coordinator.state()?;
            let a = state
                .assignments
                .get(&assignment)
                .ok_or_else(|| anyhow::anyhow!("unknown assignment"))?;
            let store = FsStore::open_without_maintenance(&ctx.path)?;
            anyhow::ensure!(
                a.ticket.connection_id == git_connection_id(&store),
                "provider recovery verification is unsupported for this source; {}",
                coordinator.unsupported_source(&a.ticket.connection_id)?
            );
            let t = store.read_ticket(&a.ticket.native_id.parse()?)?;
            claims::recoverable(&t, time::OffsetDateTime::now_utc())?;
            anyhow::ensure!(
                t.notes.iter().any(|n| n.id.to_string() == evidence_note),
                "inspection note must exist on this exact ticket"
            );
            coordinator.recover(&i.id, &i.session, now, &assignment, &evidence_note)?;
        }
        CoordinationCmd::Mark {
            integrator: i,
            assignment,
            phase,
        } => {
            let state = coordinator.state()?;
            let a = state
                .assignments
                .get(&assignment)
                .ok_or_else(|| anyhow::anyhow!("unknown assignment"))?;
            // HS2-FG7ZMF: metadata support never authorizes a foreign code or findings
            // transition, including records preserved from an older pool snapshot.
            anyhow::ensure!(
                ctx.path
                    .join(hotsheet_ticketing::STORE_METADATA_FILE)
                    .exists(),
                "provider integration verification is unsupported for this selected source; {}",
                coordinator.unsupported_source(&a.ticket.connection_id)?
            );
            let store = FsStore::open_without_maintenance(&ctx.path)?;
            anyhow::ensure!(
                a.ticket.connection_id == git_connection_id(&store),
                "provider integration verification is unsupported for this source; {}",
                coordinator.unsupported_source(&a.ticket.connection_id)?
            );
            if matches!(phase, Phase::Verifying | Phase::Done)
                && let Some(HandoffEvidence::Findings { note_id }) = &a.handoff
            {
                anyhow::ensure!(
                    ctx.path
                        .join(hotsheet_ticketing::STORE_METADATA_FILE)
                        .exists(),
                    "provider findings verification is unsupported for this selected source"
                );
                let store = FsStore::open_without_maintenance(&ctx.path)?;
                anyhow::ensure!(
                    a.ticket.connection_id == git_connection_id(&store),
                    "findings verification requires the assignment's exact ticket source"
                );
                let ticket = store.read_ticket(&a.ticket.native_id.parse()?)?;
                let note_id = hotsheet_model::Ulid::from_string(note_id)?;
                anyhow::ensure!(
                    ticket.notes.iter().any(|note| {
                        note.id == note_id
                            && note.kind == hotsheet_model::NoteKind::Regular
                            && note.text.lines().next() == Some("## Findings handoff")
                            && note
                                .text
                                .lines()
                                .skip(1)
                                .any(|line| !line.trim().is_empty())
                    }),
                    "findings note is missing or empty on the assigned ticket"
                );
                if phase == Phase::Verifying {
                    require_findings_claim(&ticket, &i.id, time::OffsetDateTime::now_utc())?;
                }
                if phase == Phase::Done {
                    anyhow::ensure!(
                        matches!(
                            ticket.status,
                            hotsheet_model::Status::Completed | hotsheet_model::Status::Verified
                        ),
                        "complete the findings ticket before resolving its mailbox"
                    );
                }
            }
            coordinator.mark(&i.id, &i.session, now, &assignment, phase)?;
        }
        CoordinationCmd::ReviewReturn {
            integrator: i,
            assignment,
            evidence_note,
            inspected,
        } => {
            anyhow::ensure!(
                inspected,
                "inspect and preserve the reviewed artifacts first"
            );
            coordinator.review_return(
                &i.id,
                &i.session,
                time::OffsetDateTime::now_utc(),
                &assignment,
                &evidence_note,
            )?;
        }
        CoordinationCmd::Finish {
            integrator: i,
            reason,
        } => coordinator.finish(&i.id, &i.session, now, &reason)?,
        CoordinationCmd::Leave { identity: i, role } => {
            coordinator.leave(role, &i.id, &i.session)?
        }
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&coordinator.snapshot(now)?)?
    );
    Ok(())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AddressedRouteEvidence {
    operation: String,
    worker: String,
    generation: String,
    route: String,
}

fn verify_addressed_route(
    ticket: &hotsheet_model::Ticket,
    note_id: &str,
    operation: &str,
    worker: &str,
    generation: &str,
    route: &ReturnRoute,
    integrator: &str,
) -> Result<()> {
    let id = hotsheet_model::Ulid::from_string(note_id)?;
    let note = ticket
        .notes
        .iter()
        .find(|note| note.id == id)
        .ok_or_else(|| anyhow::anyhow!("route evidence note is absent from this exact ticket"))?;
    anyhow::ensure!(
        note.kind == hotsheet_model::NoteKind::Regular
            && note.actor.as_ref().and_then(|actor| actor.id.as_deref()) == Some(integrator),
        "route evidence must be a regular note authored by the exact integrator"
    );
    let body = note
        .text
        .strip_prefix("## Assignment return route\n")
        .ok_or_else(|| anyhow::anyhow!("route evidence must start with the exact heading"))?;
    let evidence: AddressedRouteEvidence = serde_json::from_str(body.trim())?;
    let addressed_route: ReturnRoute = evidence.route.parse().map_err(anyhow::Error::msg)?;
    anyhow::ensure!(
        evidence.operation == operation
            && evidence.worker == worker
            && evidence.generation == generation
            && addressed_route == *route,
        "route evidence must address this exact operation, worker, generation and route"
    );
    Ok(())
}

fn require_findings_claim(
    ticket: &hotsheet_model::Ticket,
    integrator: &str,
    now: time::OffsetDateTime,
) -> Result<()> {
    // HS2-A1F615: ticket leases retain fractional seconds. The mailbox's integer
    // availability clock must not extend an expired ticket claim into verification.
    let at = hotsheet_model::Timestamp::from_datetime(now);
    anyhow::ensure!(
        ticket.claimed_by.as_deref() == Some(integrator) && !ops::claim_available(ticket, &at),
        "integrator must claim the exact findings ticket before verification"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hotsheet_model::{Ticket, Timestamp};
    use time::{Duration, OffsetDateTime};

    #[test]
    fn hs2_a1f615_findings_claim_preserves_same_second_authority_on_every_replay() {
        let second = OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap();
        let at = |milliseconds| second + Duration::milliseconds(milliseconds);
        let mut ticket = Ticket {
            claimed_by: Some("integrator".into()),
            claim_lease_expires_at: Some(Timestamp::from_datetime(at(200))),
            ..Ticket::default()
        };
        for _ in 0..2 {
            assert!(require_findings_claim(&ticket, "integrator", at(100)).is_ok());
            assert!(require_findings_claim(&ticket, "integrator", at(200)).is_err());
            assert!(require_findings_claim(&ticket, "integrator", at(800)).is_err());
        }
        ticket.claim_lease_expires_at = Some(Timestamp::from_datetime(at(900)));
        assert!(require_findings_claim(&ticket, "integrator", at(800)).is_ok());
        ticket.claimed_by = Some("replacement".into());
        assert!(require_findings_claim(&ticket, "integrator", at(800)).is_err());
        assert!(require_findings_claim(&ticket, "replacement", at(800)).is_ok());
        ticket.claimed_by = None;
        assert!(require_findings_claim(&ticket, "integrator", at(800)).is_err());
        ticket.claimed_by = Some("integrator".into());
        ticket.claim_lease_expires_at = None;
        assert!(require_findings_claim(&ticket, "integrator", at(800)).is_err());
    }
}
