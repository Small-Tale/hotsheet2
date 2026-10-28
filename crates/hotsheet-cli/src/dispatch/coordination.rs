//! Headless local coordination; no server, provider tokens, or AI process launcher.

use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, Subcommand};
use hotsheet_cli::coordination::{
    Assignment, Authority, CAPABILITIES, Coordinator, HandoffEvidence, IntegrationSuspension,
    Phase, PrerequisiteGate, ReservationCheck, ReturnRoute, Role, WaitRequest,
};
use hotsheet_ticketing::TicketRef;
use hotsheet_ticketing::{FsStore, git_connection_id, ops};
use serde::Deserialize;

use super::{Ctx, misrouted};
use crate::Cmd;

#[path = "coordination_advisory.rs"]
mod advisory;
#[path = "coordination_claims.rs"]
mod claims;
#[path = "coordination_readiness.rs"]
mod readiness;

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
    /// Report the pool schemas, roles and assignment authorities this executable supports,
    /// without opening (and so without migrating) the pool.
    Capabilities,
    /// Check an exact worker handoff before writing its durable ticket note.
    Preflight {
        #[command(flatten)]
        worker: Identity,
        #[arg(long)]
        assignment: String,
        /// Structured code or findings review evidence; kept machine-local.
        #[arg(long)]
        evidence_file: PathBuf,
        /// Publish the checked handoff note, release the claim, and record mailbox handoff.
        #[arg(long)]
        publish: bool,
    },
    /// Inspect or publish a read-only advisory review of a frozen handoff.
    AdvisoryReview {
        #[command(flatten)]
        reviewer: Identity,
        #[arg(long)]
        assignment: String,
        /// Omit to print the immutable evidence snapshot needed by a reviewer.
        #[arg(long)]
        evidence_file: Option<PathBuf>,
        /// Recheck and publish one durable advisory result; requires evidence_file.
        #[arg(long, requires = "evidence_file")]
        publish: bool,
    },
    /// Address a bounded, durable advisory request to a separate reviewer session.
    AdvisoryRequest {
        #[command(flatten)]
        integrator: Identity,
        #[arg(long)]
        assignment: String,
        #[arg(long)]
        reviewer: String,
        #[arg(long)]
        reviewer_session: String,
        /// Explicit delivery path; this command records it but does not wake a reviewer.
        #[arg(long)]
        return_route: ReturnRoute,
    },
    /// Cancel a lost or superseded advisory request to free bounded review capacity.
    AdvisoryCancel {
        #[command(flatten)]
        integrator: Identity,
        #[arg(long)]
        assignment: String,
        #[arg(long)]
        request_note: String,
    },
    /// Read an advisory result and report whether the integrator may use it now.
    AdvisoryInspect {
        #[command(flatten)]
        integrator: Identity,
        #[arg(long)]
        assignment: String,
        #[arg(long)]
        note: String,
    },
    /// Reserve an exact eligible ticket for an available worker; inspect scope/claims first.
    /// Run by the role holding assignment authority: the integrator in legacy mode, otherwise
    /// the dispatcher (HS2-TWZW7X).
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
        /// Repository-relative path this assignment may change; repeat for each path.
        /// Required with dispatcher authority, which refuses overlapping scopes.
        #[arg(long)]
        scope: Vec<String>,
    },
    /// Publish the integrator's safe shipping base, blocked tickets and in-flight path scopes,
    /// which the dispatcher reads before each reservation (schema four).
    Publish {
        #[command(flatten)]
        integrator: Identity,
        #[arg(long)]
        base: String,
        /// Qualified `connection:native` ticket the dispatcher must not assign; repeatable.
        #[arg(long)]
        blocked: Vec<String>,
        /// Repository-relative path scope held by integration or review; repeatable. A
        /// `path#row:<id>` or `path#heading:<slug>` section requires that path in `--shared`.
        #[arg(long)]
        in_flight: Vec<String>,
        /// Document reviewed as section-addressable, so dispatcher scopes may name distinct
        /// sections of it; repeatable. Must be a file at `--base` (HS2-YHYNJ6).
        #[arg(long)]
        shared: Vec<String>,
        /// Confirms every participant was switched to a schema-five executable before the
        /// first shared document persists schema five.
        #[arg(long)]
        all_participants_upgraded: bool,
    },
    /// Switch assignment authority. Only the exact live integrator may run it; enabling the
    /// dispatcher persists schema four, which fences every schema-three executable.
    Authority {
        #[command(flatten)]
        integrator: Identity,
        #[arg(long, value_enum)]
        mode: Authority,
        /// Confirms every participant was stopped and switched to a schema-four executable.
        #[arg(long)]
        all_participants_upgraded: bool,
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
    /// Preserve an unpushed integration blocked by an independent failed local gate.
    SuspendIntegration {
        #[command(flatten)]
        integrator: Identity,
        #[arg(long)]
        assignment: String,
        /// Full current unpushed main SHA containing the handed-off change.
        #[arg(long)]
        main_commit: String,
        /// Qualified prerequisite ticket in the selected local source.
        #[arg(long)]
        prerequisite_connection: String,
        #[arg(long)]
        prerequisite_native_id: String,
        /// Exact regular `## Integration prerequisite` note on the assigned ticket.
        #[arg(long)]
        failure_note: String,
        /// Required on the first schema-six transition; old participants must stop.
        #[arg(long)]
        all_participants_upgraded: bool,
    },
    /// Reacquire the exclusive integration fence after the prerequisite's local gate.
    ResumeIntegration {
        #[command(flatten)]
        integrator: Identity,
        #[arg(long)]
        assignment: String,
        /// Full current main SHA containing the prerequisite fix.
        #[arg(long)]
        main_commit: String,
        /// Exact passed `HEAD^{tree}` from the root gate.
        #[arg(long)]
        gate_tree: String,
        /// Exact regular `## Prerequisite gate` note on the prerequisite ticket.
        #[arg(long)]
        evidence_note: String,
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
    if matches!(args.command, CoordinationCmd::Capabilities) {
        // Opening even for `status` may migrate an older pool; this preflight never opens it.
        println!("{}", serde_json::to_string_pretty(&CAPABILITIES)?);
        return Ok(());
    }
    let coordinator = Coordinator::open(&args.project, &ctx.path)?;
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    match args.command {
        CoordinationCmd::Setup { resume } => {
            if resume {
                coordinator.resume()?;
            }
        }
        CoordinationCmd::Status => {}
        CoordinationCmd::Capabilities => unreachable!("handled before opening the pool"),
        CoordinationCmd::Preflight {
            worker,
            assignment,
            evidence_file,
            publish,
        } => {
            let report = if publish {
                readiness::publish(
                    &coordinator,
                    &ctx.path,
                    &args.project,
                    &worker.id,
                    &worker.session,
                    &assignment,
                    &evidence_file,
                )?
            } else {
                readiness::preflight(
                    &coordinator,
                    &ctx.path,
                    &args.project,
                    &worker.id,
                    &worker.session,
                    &assignment,
                    &evidence_file,
                )?
            };
            println!("{}", serde_json::to_string_pretty(&report)?);
            return Ok(());
        }
        CoordinationCmd::AdvisoryReview {
            reviewer,
            assignment,
            evidence_file,
            publish,
        } => {
            let report = advisory::run(
                &coordinator,
                &ctx.path,
                &args.project,
                &reviewer,
                &assignment,
                evidence_file.as_deref(),
                publish,
            )?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            return Ok(());
        }
        CoordinationCmd::AdvisoryRequest {
            integrator,
            assignment,
            reviewer,
            reviewer_session,
            return_route,
        } => {
            let report = advisory::request(
                &coordinator,
                &ctx.path,
                &integrator,
                &assignment,
                &reviewer,
                &reviewer_session,
                &return_route,
            )?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            return Ok(());
        }
        CoordinationCmd::AdvisoryCancel {
            integrator,
            assignment,
            request_note,
        } => {
            let report = advisory::cancel(
                &coordinator,
                &ctx.path,
                &integrator,
                &assignment,
                &request_note,
            )?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            return Ok(());
        }
        CoordinationCmd::AdvisoryInspect {
            integrator,
            assignment,
            note,
        } => {
            let report = advisory::inspect(
                &coordinator,
                &ctx.path,
                &args.project,
                &integrator.id,
                &integrator.session,
                &assignment,
                &note,
            )?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            return Ok(());
        }
        CoordinationCmd::Publish {
            integrator: i,
            base,
            blocked,
            in_flight,
            shared,
            all_participants_upgraded,
        } => {
            for path in &shared {
                anyhow::ensure!(
                    git_blob(&args.project, &base, path)?.is_some(),
                    "shared document {path} is not a file at the published base {base}"
                );
            }
            let blocked = blocked
                .iter()
                .map(|raw| {
                    let (connection_id, native_id) = raw.split_once(':').ok_or_else(|| {
                        anyhow::anyhow!("blocked ticket must be connection:native, got {raw}")
                    })?;
                    Ok(TicketRef {
                        connection_id: connection_id.into(),
                        native_id: native_id.into(),
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            // Sampled at the serialized boundary, after the Git reads above (HS2-YHYNJ6).
            coordinator.publish(
                &i.id,
                &i.session,
                &|| time::OffsetDateTime::now_utc().unix_timestamp(),
                &base,
                blocked,
                &in_flight,
                &shared,
                all_participants_upgraded,
            )?;
        }
        CoordinationCmd::Authority {
            integrator: i,
            mode,
            all_participants_upgraded,
        } => coordinator.set_authority(&i.id, &i.session, now, mode, all_participants_upgraded)?,
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
            scope,
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
            let ticket_id: hotsheet_model::Ulid = native_id.parse()?;
            let (author, checked_worker, checked_generation, checked_operation) = (
                i.id.clone(),
                worker.clone(),
                worker_session.clone(),
                operation.clone(),
            );
            let evidence = route_override.clone().zip(route_evidence_note.clone());
            // HS2-TWZW7X: eligibility, dependencies and route evidence are checked at the
            // serialized reservation boundary, after the current authority is verified, from
            // fresh ticket reads. The evidence author is therefore the active assignment
            // authority: the dispatcher with dispatcher authority, otherwise the integrator.
            let check: ReservationCheck<'_> = Box::new(move |current, replayed| {
                let ticket = store.read_ticket(&ticket_id)?;
                if replayed.is_none() {
                    let tickets = store.list_tickets()?;
                    claims::assignable(
                        &ticket,
                        &tickets,
                        &checked_worker,
                        time::OffsetDateTime::now_utc(),
                    )?;
                    if current.authority == Authority::Dispatcher {
                        claims::dependencies_closed(&ticket, &tickets)?;
                    }
                }
                if let Some((route, note_id)) = &evidence {
                    // New operations require evidence authored by the active assignment
                    // authority. A replay only acknowledges the durable reservation: its
                    // original evidence still has to address this exact operation, worker,
                    // generation and route, and the reservation itself must be identical,
                    // but it need not be re-authored after an authority or identity change.
                    verify_addressed_route(
                        &ticket,
                        note_id,
                        &checked_operation,
                        &checked_worker,
                        &checked_generation,
                        route,
                        replayed.is_none().then_some(author.as_str()),
                    )?;
                }
                Ok(())
            });
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
            coordinator.assign_verified(
                &i.id,
                &i.session,
                &|| time::OffsetDateTime::now_utc().unix_timestamp(),
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
                    suspension: None,
                    suspension_history: Vec::new(),
                    scope,
                    transitions: Vec::new(),
                },
                check,
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
                            if let Some(sha) = commit.as_deref()
                                && !a.scope.is_empty()
                            {
                                review_commit_scope(&args.project, &a.base, sha, &a.scope)?;
                            }
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
            if phase == Phase::Ci
                && a.phase != Phase::Ci
                && state.version >= 6
                && matches!(a.handoff, Some(HandoffEvidence::Code { .. }))
            {
                let resumed_main = a
                    .suspension
                    .as_ref()
                    .map(|suspension| {
                        suspension
                            .resumed
                            .as_ref()
                            .map(|gate| gate.main_commit.as_str())
                            .ok_or_else(|| {
                                anyhow::anyhow!("suspended integration has no prerequisite gate")
                            })
                    })
                    .transpose()?;
                require_pushed_main(&args.project, resumed_main)?;
            }
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
        CoordinationCmd::SuspendIntegration {
            integrator: i,
            assignment,
            main_commit,
            prerequisite_connection,
            prerequisite_native_id,
            failure_note,
            all_participants_upgraded,
        } => {
            require_current_main(&args.project, &main_commit)?;
            require_unpushed_main(&args.project, &main_commit)?;
            let state = coordinator.state()?;
            let assigned = state
                .assignments
                .get(&assignment)
                .ok_or_else(|| anyhow::anyhow!("unknown assignment"))?;
            if let Some(previous_gate) = assigned
                .suspension
                .as_ref()
                .and_then(|suspension| suspension.resumed.as_ref())
            {
                let ancestry = git_output(
                    &args.project,
                    &[
                        "merge-base",
                        "--is-ancestor",
                        &previous_gate.main_commit,
                        &main_commit,
                    ],
                )?;
                anyhow::ensure!(
                    ancestry.status.success(),
                    "repeat suspension must retain the previous prerequisite gate commit"
                );
            }
            let store = local_assignment_store(ctx, &coordinator, assigned)?;
            let ticket = store.read_ticket(&assigned.ticket.native_id.parse()?)?;
            let prerequisite = TicketRef {
                connection_id: prerequisite_connection,
                native_id: prerequisite_native_id,
            };
            anyhow::ensure!(
                prerequisite.connection_id == git_connection_id(&store),
                "prerequisite ticket verification is unsupported for this source"
            );
            let _ = store.read_ticket(&prerequisite.native_id.parse()?)?;
            verify_integration_evidence(
                &ticket,
                &failure_note,
                "## Integration prerequisite",
                &AddressedIntegrationEvidence {
                    operation: assignment.clone(),
                    source_commit: assigned
                        .commit
                        .clone()
                        .ok_or_else(|| anyhow::anyhow!("suspension requires a code handoff SHA"))?,
                    main_commit: main_commit.clone(),
                    prerequisite: prerequisite.qualified(),
                    tree: None,
                },
                &i.id,
            )?;
            coordinator.suspend_integration(
                &i.id,
                &i.session,
                now,
                &assignment,
                IntegrationSuspension {
                    main_commit,
                    prerequisite,
                    failure_note,
                    resumed: None,
                },
                all_participants_upgraded,
            )?;
        }
        CoordinationCmd::ResumeIntegration {
            integrator: i,
            assignment,
            main_commit,
            gate_tree,
            evidence_note,
        } => {
            require_current_main(&args.project, &main_commit)?;
            require_passed_tree(&args.project, &gate_tree)?;
            let state = coordinator.state()?;
            let assigned = state
                .assignments
                .get(&assignment)
                .ok_or_else(|| anyhow::anyhow!("unknown assignment"))?;
            let suspension = assigned.suspension.as_ref().ok_or_else(|| {
                anyhow::anyhow!("assignment has no preserved integration suspension")
            })?;
            let ancestry = git_output(
                &args.project,
                &[
                    "merge-base",
                    "--is-ancestor",
                    &suspension.main_commit,
                    &main_commit,
                ],
            )?;
            anyhow::ensure!(
                ancestry.status.success(),
                "prerequisite main commit must descend from the suspended integration"
            );
            let store = local_assignment_store(ctx, &coordinator, assigned)?;
            anyhow::ensure!(
                suspension.prerequisite.connection_id == git_connection_id(&store),
                "prerequisite evidence verification is unsupported for this ticket source"
            );
            let prerequisite = store.read_ticket(&suspension.prerequisite.native_id.parse()?)?;
            verify_integration_evidence(
                &prerequisite,
                &evidence_note,
                "## Prerequisite gate",
                &AddressedIntegrationEvidence {
                    operation: assignment.clone(),
                    source_commit: assigned
                        .commit
                        .clone()
                        .ok_or_else(|| anyhow::anyhow!("resumption requires a code handoff SHA"))?,
                    main_commit: main_commit.clone(),
                    prerequisite: suspension.prerequisite.qualified(),
                    tree: Some(gate_tree.clone()),
                },
                &i.id,
            )?;
            coordinator.resume_integration(
                &i.id,
                &i.session,
                now,
                &assignment,
                PrerequisiteGate {
                    main_commit,
                    tree: gate_tree,
                    evidence_note,
                },
            )?;
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
    author: Option<&str>,
) -> Result<()> {
    let id = hotsheet_model::Ulid::from_string(note_id)?;
    let note = ticket
        .notes
        .iter()
        .find(|note| note.id == id)
        .ok_or_else(|| anyhow::anyhow!("route evidence note is absent from this exact ticket"))?;
    anyhow::ensure!(
        note.kind == hotsheet_model::NoteKind::Regular,
        "route evidence must be a regular note"
    );
    if let Some(author) = author {
        anyhow::ensure!(
            note.actor.as_ref().and_then(|actor| actor.id.as_deref()) == Some(author),
            "route evidence must be a regular note authored by the exact active assignment authority"
        );
    }
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

#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct AddressedIntegrationEvidence {
    operation: String,
    source_commit: String,
    main_commit: String,
    prerequisite: String,
    #[serde(default)]
    tree: Option<String>,
}

fn verify_integration_evidence(
    ticket: &hotsheet_model::Ticket,
    note_id: &str,
    heading: &str,
    expected: &AddressedIntegrationEvidence,
    integrator: &str,
) -> Result<()> {
    let id = hotsheet_model::Ulid::from_string(note_id)?;
    let note = ticket
        .notes
        .iter()
        .find(|note| note.id == id)
        .ok_or_else(|| {
            anyhow::anyhow!("integration evidence note is absent from the exact ticket")
        })?;
    anyhow::ensure!(
        note.kind == hotsheet_model::NoteKind::Regular
            && note.actor.as_ref().and_then(|actor| actor.id.as_deref()) == Some(integrator),
        "integration evidence must be a regular note by the exact integrator"
    );
    let body = note
        .text
        .strip_prefix(&format!("{heading}\n"))
        .ok_or_else(|| anyhow::anyhow!("integration evidence has the wrong heading"))?;
    let actual: AddressedIntegrationEvidence = serde_json::from_str(body.trim())?;
    anyhow::ensure!(
        actual == *expected,
        "integration evidence must address this exact operation, source SHA, main SHA, prerequisite and gate tree"
    );
    Ok(())
}

fn local_assignment_store(
    ctx: &Ctx,
    coordinator: &Coordinator,
    assignment: &Assignment,
) -> Result<FsStore> {
    anyhow::ensure!(
        ctx.path
            .join(hotsheet_ticketing::STORE_METADATA_FILE)
            .exists(),
        "provider integration verification is unsupported for this selected source"
    );
    let store = FsStore::open_without_maintenance(&ctx.path)?;
    anyhow::ensure!(
        assignment.ticket.connection_id == git_connection_id(&store),
        "provider integration verification is unsupported for this source; {}",
        coordinator.unsupported_source(&assignment.ticket.connection_id)?
    );
    Ok(store)
}

fn require_current_main(project: &std::path::Path, main_commit: &str) -> Result<()> {
    let head = git_output(project, &["rev-parse", "HEAD"])?;
    anyhow::ensure!(head.status.success(), "could not read shipping HEAD");
    anyhow::ensure!(
        String::from_utf8(head.stdout)?.trim() == main_commit,
        "main commit must equal the current shipping HEAD"
    );
    Ok(())
}

fn require_unpushed_main(project: &std::path::Path, main_commit: &str) -> Result<()> {
    let upstream = git_output(project, &["rev-parse", "@{upstream}"])?;
    anyhow::ensure!(
        upstream.status.success(),
        "suspension requires a configured upstream to prove the integrated commit is unpushed"
    );
    let upstream_sha = String::from_utf8(upstream.stdout)?;
    let upstream_sha = upstream_sha.trim();
    anyhow::ensure!(
        upstream_sha != main_commit,
        "this integrated main commit is already pushed"
    );
    let ancestor = git_output(
        project,
        &["merge-base", "--is-ancestor", upstream_sha, main_commit],
    )?;
    anyhow::ensure!(
        ancestor.status.success(),
        "the unpushed main commit must descend from its upstream"
    );
    Ok(())
}

fn require_pushed_main(project: &std::path::Path, resumed_main: Option<&str>) -> Result<()> {
    let head = git_output(project, &["rev-parse", "HEAD"])?;
    let upstream = git_output(project, &["rev-parse", "@{upstream}"])?;
    anyhow::ensure!(
        head.status.success()
            && upstream.status.success()
            && head.stdout.trim_ascii() == upstream.stdout.trim_ascii(),
        "code integration cannot enter CI before the exact main commit reaches its upstream"
    );
    if let Some(resumed_main) = resumed_main {
        let ancestor = git_output(
            project,
            &["merge-base", "--is-ancestor", resumed_main, "HEAD"],
        )?;
        anyhow::ensure!(
            ancestor.status.success(),
            "pushed main must retain the verified prerequisite commit"
        );
    }
    Ok(())
}

fn require_passed_tree(project: &std::path::Path, tree: &str) -> Result<()> {
    let actual = git_output(project, &["rev-parse", "HEAD^{tree}"])?;
    anyhow::ensure!(actual.status.success(), "could not read shipping tree");
    anyhow::ensure!(
        String::from_utf8(actual.stdout)?.trim() == tree,
        "passed tree must equal the current shipping tree"
    );
    let cache = git_output(
        project,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            "hotsheet-root-gate",
        ],
    )?;
    anyhow::ensure!(
        cache.status.success(),
        "could not locate local root-gate evidence"
    );
    let path = PathBuf::from(String::from_utf8(cache.stdout)?.trim()).join(tree);
    anyhow::ensure!(
        std::fs::read_to_string(path).ok().as_deref().map(str::trim) == Some(tree),
        "the exact current tree has no successful local root-gate receipt"
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

fn git_output(project: &std::path::Path, args: &[&str]) -> Result<std::process::Output> {
    Ok(hotsheet_ticketing::git::launch("git")
        .arg("-C")
        .arg(project)
        .args(args)
        .output()?)
}

/// A file's content at `rev`, or `None` where that revision has no file at `path`.
fn git_blob(project: &std::path::Path, rev: &str, path: &str) -> Result<Option<String>> {
    let spec = format!("{rev}:{path}");
    let kind = git_output(project, &["cat-file", "-t", &spec])?;
    if !kind.status.success() || kind.stdout.trim_ascii() != b"blob" {
        return Ok(None);
    }
    let blob = git_output(project, &["cat-file", "blob", &spec])?;
    anyhow::ensure!(blob.status.success(), "could not read {spec}");
    Ok(Some(String::from_utf8(blob.stdout)?))
}

/// A dispatcher-scoped code handoff must descend from its assignment base and change only
/// what its scope declares, sections included (HS2-YHYNJ6).
fn review_commit_scope(
    project: &std::path::Path,
    base: &str,
    commit: &str,
    scope: &[String],
) -> Result<()> {
    let ancestor = git_output(project, &["merge-base", "--is-ancestor", base, commit])?;
    anyhow::ensure!(
        ancestor.status.success(),
        "handoff commit {commit} does not descend from the assignment base {base}"
    );
    let diff = git_output(
        project,
        &["diff", "--name-only", "-z", "--no-renames", base, commit],
    )?;
    anyhow::ensure!(
        diff.status.success(),
        "could not diff {base}..{commit}: {}",
        String::from_utf8_lossy(&diff.stderr)
    );
    let mut changed = Vec::new();
    for path in String::from_utf8(diff.stdout)?
        .split('\0')
        .filter(|p| !p.is_empty())
    {
        let (before, after) = if hotsheet_cli::coordination::scope::sectioned(scope, path) {
            (
                git_blob(project, base, path)?,
                git_blob(project, commit, path)?,
            )
        } else {
            (None, None)
        };
        changed.push(hotsheet_cli::coordination::scope::ChangedPath {
            path: path.to_owned(),
            before,
            after,
        });
    }
    hotsheet_cli::coordination::scope::review_handoff(scope, &changed)
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
