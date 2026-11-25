//! Headless local coordination; no server, provider tokens, or AI process launcher.

use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, Subcommand};
use hotsheet_cli::coordination::development_strategy::CustomDevelopmentStrategy;
use hotsheet_cli::coordination::{
    Assignment, Authority, CAPABILITIES, Coordinator, DevelopmentPolicy, DevelopmentPreset,
    HandoffEvidence, IntegrationStrategy, IntegrationSuspension, NextPlanEntry, Phase,
    PreferenceRequest, PrerequisiteGate, RefusalReason, ReservationCheck, ReturnRoute, Role,
    StrategyPolicy, StrategyRequest, TicketFilter, WaitRequest, WorkerPreferences,
};
use hotsheet_ticketing::TicketRef;
use hotsheet_ticketing::{FsStore, git_connection_id, ops};
use serde::Deserialize;

use super::{Ctx, misrouted};
use crate::Cmd;

#[path = "coordination_advisory.rs"]
mod advisory;
#[path = "coordination_batch.rs"]
mod batch;
#[path = "coordination_claims.rs"]
mod claims;
#[path = "coordination_readiness.rs"]
mod readiness;
#[path = "coordination_speculation.rs"]
mod speculation;

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
    /// Integrator only: the integration strategy for this session. Must be allowed by the
    /// project and implemented; omit to use the project default (HS2-NEY1QH).
    #[arg(long, value_enum)]
    strategy: Option<IntegrationStrategy>,
    /// Built-in or named custom development strategy for this integrator generation.
    #[arg(long)]
    development_strategy: Option<String>,
    /// Integrator: confirms every participant runs a schema-eight executable before the
    /// first non-serial strategy is recorded. Worker: confirms every participant runs a
    /// schema-nine executable before the first ticket preferences are recorded.
    #[arg(long)]
    all_participants_upgraded: bool,
    /// Worker only: rank tickets matching this filter first within the normal priority
    /// order; any eligible ticket remains possible. `tag:<name>`, `category:<name>`,
    /// `priority:<level>` or `tier:<Tn>`; repeatable (HS2-6WFCZR).
    #[arg(long)]
    prefer: Vec<TicketFilter>,
    /// Worker only: receive only tickets matching at least one of these filters; strict,
    /// and an idle worker reports why nothing matches. Repeatable.
    #[arg(long)]
    only: Vec<TicketFilter>,
    /// Worker only: never receive tickets matching any of these filters; strict. Repeatable.
    #[arg(long)]
    avoid: Vec<TicketFilter>,
    /// Worker only: ask the assignment authority for this exact ticket slug. It is honored
    /// only when the ticket passes every assignment check; otherwise a refusal is recorded.
    #[arg(long)]
    request: Option<String>,
    /// Worker only: drop this session's stated preferences and request.
    #[arg(long, conflicts_with_all = ["prefer", "only", "avoid", "request"])]
    clear_preferences: bool,
}

impl Registration {
    /// Flags given replace the session's whole preference set; none keeps it (HS2-6WFCZR).
    fn preference_request(&self) -> Result<PreferenceRequest> {
        let stated = !self.prefer.is_empty()
            || !self.only.is_empty()
            || !self.avoid.is_empty()
            || self.request.is_some()
            || self.clear_preferences;
        if self.role != Role::Worker {
            anyhow::ensure!(
                !stated,
                "only a worker states ticket preferences or requests"
            );
            return Ok(PreferenceRequest::keep());
        }
        if !stated {
            return Ok(PreferenceRequest {
                set: None,
                all_participants_upgraded: self.all_participants_upgraded,
            });
        }
        Ok(PreferenceRequest::replace(
            WorkerPreferences::stated(
                self.prefer.clone(),
                self.only.clone(),
                self.avoid.clone(),
                self.request.clone(),
            )?,
            self.all_participants_upgraded,
        ))
    }

    /// Resolve the strategy against the committed project policy before any state lock.
    fn strategy_request(&self, project: &std::path::Path) -> Result<StrategyRequest> {
        if self.role != Role::Integrator {
            anyhow::ensure!(
                self.strategy.is_none(),
                "only the integrator selects an integration strategy"
            );
            anyhow::ensure!(
                self.development_strategy.is_none(),
                "only the integrator selects a development strategy"
            );
            anyhow::ensure!(
                self.role == Role::Worker || !self.all_participants_upgraded,
                "--all-participants-upgraded on registration is for an integrator strategy or worker preferences"
            );
            return Ok(StrategyRequest::default());
        }
        let settings = hotsheet_ticketing::Settings::for_project(project);
        let integration_policy = StrategyPolicy::read(&settings)?;
        let development = DevelopmentPolicy::read(&settings)?;
        let explicitly_selected = settings
            .get(
                hotsheet_cli::coordination::development_strategy::DEVELOPMENT_POLICY_SETTING,
                hotsheet_ticketing::Scope::Shared,
            )?
            .is_some();
        let selected = if explicitly_selected || self.development_strategy.is_some() {
            let resolved = development.resolve_named(
                &integration_policy,
                self.development_strategy.as_deref(),
                None,
            )?;
            if resolved.custom.is_some() {
                development.validate_project_capabilities(project)?;
            }
            if let Some(strategy) = self.strategy {
                anyhow::ensure!(
                    strategy == resolved.integration,
                    "explicit integration strategy {strategy} conflicts with development preset {}",
                    resolved.preset
                );
            }
            Some(resolved.integration)
        } else {
            self.strategy
        };
        StrategyRequest::from_policy(
            &integration_policy,
            selected,
            self.all_participants_upgraded,
        )
    }
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
    /// Read the source-scoped worker view, including claim-only and disconnected rows.
    Workers {
        /// Cursor from a previous workers read. Wait for an event or lease deadline.
        #[arg(long)]
        after: Option<String>,
        /// Maximum event wait (default 30 seconds, capped at 30 seconds).
        #[arg(long)]
        wait_ms: Option<u64>,
        /// Also list past workers: expired sessions that hold no non-terminal work.
        #[arg(long)]
        include_past: bool,
    },
    /// Report, and with --apply delete, `<remote>/merge-pending/<SLUG>` branches whose
    /// commits are all on the shipping branch (or patch-equivalent there) and whose ticket
    /// is completed, verified, archived or deleted (HS2-ZTG8FD). Dry run by default;
    /// never opens the coordination pool.
    MergePendingPrune {
        /// Remote that holds the merge-pending branches.
        #[arg(long, default_value = "origin")]
        remote: String,
        /// Shipping ref to compare against (default `<remote>/HEAD`, else `<remote>/main`).
        #[arg(long)]
        shipping: Option<String>,
        /// Delete eligible branches with `git push <remote> --delete`.
        #[arg(long)]
        apply: bool,
        /// Use the current remote-tracking refs without `git fetch --prune` first.
        #[arg(long)]
        no_fetch: bool,
    },
    /// Retire every non-done assignment whose ticket is closed (completed, verified,
    /// archived, deleted or moved), including review returns and findings handoffs.
    /// Writes a regular inspection note on each ticket and keeps the record for audit.
    RetireFinished {
        #[command(flatten)]
        integrator: Identity,
        /// Report what would be retired without writing notes or the pool.
        #[arg(long)]
        dry_run: bool,
    },
    /// Replace ordered tentative intent; this never claims or changes a ticket.
    PlanReplace {
        #[command(flatten)]
        authority: Identity,
        #[arg(long)]
        expected_revision: u64,
        /// JSON array of qualified plan entries.
        #[arg(long)]
        entries_file: PathBuf,
        /// Required before the first schema-seven plan write.
        #[arg(long)]
        all_participants_upgraded: bool,
    },
    /// Replay bounded, ordered plan revisions; overflow requires a fresh workers read.
    PlanEvents {
        #[arg(long)]
        after_revision: u64,
    },
    /// Promote one planned operation through the existing verified reservation path.
    PlanDispatch {
        #[command(flatten)]
        authority: Identity,
        #[arg(long)]
        expected_revision: u64,
        #[arg(long)]
        operation: String,
        #[arg(long)]
        base: String,
    },
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
    /// Show the project's integration strategy policy and the strategy registry, without
    /// opening the pool (HS2-NEY1QH).
    Strategies,
    /// Read the project development and testing preset, without opening the pool.
    DevelopmentStrategies,
    /// Replace the shared project default; requires its integration strategy to be allowed.
    DevelopmentStrategySet {
        #[arg(long, value_enum)]
        preset: DevelopmentPreset,
        /// Steady Main's green-main goal in minutes (default 360).
        #[arg(long)]
        green_main_goal_minutes: Option<u32>,
        /// CI First's best-effort local budget in minutes (1-1440; default 10, HS2-ZGS7JP).
        #[arg(long)]
        best_effort_budget_minutes: Option<u32>,
    },
    /// Add or replace a named version-2 custom policy from a declarative JSON file.
    DevelopmentCustomSet {
        #[arg(long)]
        name: String,
        #[arg(long)]
        file: PathBuf,
        #[arg(long)]
        select_default: bool,
    },
    /// Select an already validated custom policy as the shared project default.
    DevelopmentCustomSelect {
        #[arg(long)]
        name: String,
    },
    /// Preview project/session/ticket precedence and mandatory gates without changing policy.
    DevelopmentStrategyResolve {
        #[arg(long)]
        session: Option<String>,
        #[arg(long)]
        ticket: Option<String>,
    },
    /// Allow a non-default integration strategy, recording why the project deviates.
    StrategyAllow {
        #[arg(long, value_enum)]
        strategy: IntegrationStrategy,
        #[arg(long)]
        reason: String,
        /// Batch and ci-staged: consecutive red batches that make the integrator fall back
        /// to serial, recorded per strategy (default 2; HS2-9G0M3V, HS2-GT57BQ).
        #[arg(long)]
        max_red_batches: Option<u32>,
        /// Batch and ci-staged: minutes main may stay red before falling back to serial,
        /// recorded per strategy (default 120; HS2-9G0M3V, HS2-GT57BQ).
        #[arg(long)]
        max_red_minutes: Option<u32>,
        /// ci-staged only: the integration branch batches are pushed to before main
        /// (default `integration`; HS2-GT57BQ).
        #[arg(long)]
        staging_branch: Option<String>,
    },
    /// Withdraw an allowed integration strategy; the project default cannot be withdrawn.
    StrategyDisallow {
        #[arg(long, value_enum)]
        strategy: IntegrationStrategy,
    },
    /// Set the project's default integration strategy (serial or an allowed strategy).
    StrategyDefault {
        #[arg(long, value_enum)]
        strategy: IntegrationStrategy,
    },
    /// Record why the assignment authority did not honor a worker's pending `--request`
    /// (HS2-6WFCZR). The worker's next wait reports it; the request stays until restated.
    RefuseRequest {
        #[command(flatten)]
        authority: Identity,
        #[arg(long)]
        worker: String,
        #[arg(long)]
        worker_session: String,
        /// The exact requested slug being refused.
        #[arg(long)]
        slug: String,
        #[arg(long, value_enum)]
        reason: RefusalReason,
        /// One-line explanation the worker sees, for example the overlapping scope.
        #[arg(long)]
        detail: Option<String>,
    },
    /// Propose a batch: ready code handoffs without live claims, in priority then handoff
    /// order, excluding any whose paths overlap an earlier one (HS2-9G0M3V). Read-only.
    BatchPlan {
        /// Largest batch to propose.
        #[arg(long)]
        max: Option<usize>,
    },
    /// Open a batch: every listed handoff enters integration atomically (batch strategy).
    BatchOpen {
        #[command(flatten)]
        integrator: Identity,
        /// Stable batch id; replaying the same open is idempotent.
        #[arg(long)]
        batch: String,
        /// Member assignment in integration order; repeat for each.
        #[arg(long = "assignment", required = true)]
        assignments: Vec<String>,
        /// ci-staged: the remote whose tracking `main` a batch without a pending parent is
        /// built on (HS2-GT57BQ).
        #[arg(long, default_value = "origin")]
        remote: String,
    },
    /// The smoke gate or integration failed before push: return every member to handoff.
    BatchAbort {
        #[command(flatten)]
        integrator: Identity,
        #[arg(long)]
        batch: String,
        #[arg(long)]
        reason: String,
    },
    /// Record the batch's one confirmed push; every member enters CI.
    BatchPushed {
        #[command(flatten)]
        integrator: Identity,
        #[arg(long)]
        batch: String,
        /// Full pushed main SHA, equal to the checked-out HEAD and its upstream.
        #[arg(long)]
        main_commit: String,
        /// `<assignment>=<full shipping SHA>`; repeat for each member and commit.
        #[arg(long = "shipping", required = true)]
        shipping: Vec<String>,
        /// A smoke-gate check that passed; repeat for each.
        #[arg(long = "smoke-check", required = true)]
        smoke_checks: Vec<String>,
    },
    /// Record the covering CI run's conclusion for a pushed batch.
    BatchResult {
        #[command(flatten)]
        integrator: Identity,
        #[arg(long)]
        batch: String,
        #[arg(long)]
        run: String,
        #[arg(long, value_enum)]
        conclusion: hotsheet_cli::coordination::batch::BatchConclusion,
    },
    /// Record the follow-up bug for each failure a red batch caused.
    BatchFollowUps {
        #[command(flatten)]
        integrator: Identity,
        #[arg(long)]
        batch: String,
        /// `<failure-key>=<follow-up slug>`; repeatable.
        #[arg(long = "failure")]
        failures: Vec<String>,
        /// HS2-HAHX20 `ci-failure-attribution.mjs` output; its non-flaky `filed` entries.
        #[arg(long)]
        attribution_file: Option<PathBuf>,
    },
    /// Clear a tripped circuit breaker after a green main run covering the last red push.
    BatchRecover {
        #[command(flatten)]
        integrator: Identity,
        #[arg(long)]
        run: String,
        /// The green run's full head SHA on main.
        #[arg(long)]
        main_commit: String,
    },
    /// ci-staged: record the batch's one confirmed push to the integration branch; every
    /// member enters CI and main is untouched (HS2-GT57BQ).
    BatchStaged {
        #[command(flatten)]
        integrator: Identity,
        #[arg(long)]
        batch: String,
        /// Full pushed integration-branch SHA, equal to the remote-tracking staging ref.
        #[arg(long)]
        staged_commit: String,
        /// `<assignment>=<full shipping SHA>`; repeat for each member and commit.
        #[arg(long = "shipping", required = true)]
        shipping: Vec<String>,
        /// A smoke-gate check that passed; repeat for each.
        #[arg(long = "smoke-check", required = true)]
        smoke_checks: Vec<String>,
        #[arg(long, default_value = "origin")]
        remote: String,
    },
    /// ci-staged: record that main was fast-forwarded to a green batch's exact staged SHA;
    /// its green ancestors are promoted with it (HS2-GT57BQ).
    BatchPromote {
        #[command(flatten)]
        integrator: Identity,
        #[arg(long)]
        batch: String,
        /// Full main SHA, equal to the remote-tracking main after the fast-forward.
        #[arg(long)]
        main_commit: String,
        #[arg(long, default_value = "origin")]
        remote: String,
    },
    /// ci-staged: withdraw a red (follow-ups recorded), stale or abandoned staged batch;
    /// every member returns to handoff and batches stacked above are invalidated.
    BatchRestage {
        #[command(flatten)]
        integrator: Identity,
        #[arg(long)]
        batch: String,
        #[arg(long)]
        reason: String,
        /// A red batch's suspect to keep out of new batches until it is review-returned;
        /// repeatable.
        #[arg(long = "hold")]
        holds: Vec<String>,
    },
    /// Print the batch ledger: every batch and the circuit breaker.
    BatchStatus,
    /// Show or change the project's provisional readiness policy (HS2-2SJPCH), without
    /// opening the pool. Off by default: a dependency then satisfies only once closed.
    SpeculationPolicy {
        /// Let a `final_testing` dependency with a pushed `CI gate commit` satisfy
        /// dependents provisionally.
        #[arg(long, conflicts_with = "disable")]
        enable: bool,
        #[arg(long)]
        disable: bool,
        /// How many unconfirmed CI gates may stack beneath one ticket (default 1).
        #[arg(long)]
        max_depth: Option<u32>,
        /// `push-per-ticket` (land after own local gates) or `parked` (stay on the branch
        /// until the base gate is green).
        #[arg(long, value_parser = speculation::parse_integration)]
        integration: Option<hotsheet_ticketing::provisional::IntegrationRule>,
        /// Branch a gate commit must be reachable from; default is the shipping upstream.
        #[arg(long, conflicts_with = "clear_base_branch")]
        base_branch: Option<String>,
        #[arg(long)]
        clear_base_branch: bool,
        /// The CI workflow whose push runs judge a gate commit, as the provider names it
        /// (default `CI`). Provisional work watches its gates there (HS2-X6FB9V).
        #[arg(long, conflicts_with = "clear_ci_workflow")]
        ci_workflow: Option<String>,
        #[arg(long)]
        clear_ci_workflow: bool,
    },
    /// Explain one ticket's readiness: confirmed, `provisional on <slug> CI <run>` with
    /// its base SHA and pending gate, or each blocking reason. Read-only.
    SpeculationExplain {
        /// Slug or ULID in the selected source.
        #[arg(long)]
        ticket: String,
    },
    /// Print every provisional assignment with its base SHA, pending gates and state,
    /// the repairs that come first and the gates to re-evaluate. Read-only.
    SpeculationStatus,
    /// Record a CI result for a gate commit when no CI watch reports it. Red marks every
    /// dependent at risk with a ticket note naming the run; green confirms.
    SpeculationResult {
        #[arg(long)]
        gate_commit: String,
        #[arg(long, value_parser = speculation::parse_result)]
        result: hotsheet_ticketing::provisional::GateResult,
        /// Provider run identifier, for example `<run id>.<attempt>`.
        #[arg(long)]
        run: Option<String>,
    },
    /// Apply the CI watch Jobs' current gate results (HS2-3R9V0R). With --wait-ms it
    /// blocks on the Job store's file events until a result changes; it never polls and
    /// never contacts the CI provider.
    SpeculationSync {
        #[arg(long, default_value_t = 0)]
        wait_ms: u64,
    },
    /// Record that at-risk or stale work now rests on the repaired prerequisite's gate
    /// commit, and on a new base when it was rebased.
    SpeculationRebase {
        #[arg(long)]
        assignment: String,
        /// The red, superseded or cancelled gate commit being replaced.
        #[arg(long)]
        replaces: String,
        /// The repaired prerequisite's pushed commit.
        #[arg(long)]
        gate_commit: String,
        /// The base the work was rebased onto; required while it is still on its branch.
        #[arg(long)]
        base: Option<String>,
    },
    /// Reopen provisional work the fix invalidated, with a ticket note. Never discards.
    SpeculationReopen {
        #[arg(long)]
        assignment: String,
        #[arg(long)]
        reason: String,
    },
    /// Record the speculative base and pending gates on the worker's worktree branch.
    SpeculationBranch {
        #[arg(long)]
        assignment: String,
        #[arg(long)]
        worktree: PathBuf,
    },
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
        /// HS2-TRP2MJ: the worktree this assignment's work is done in (an existing
        /// directory), recorded on the assignment so an orchestrator counts only its
        /// uncommitted work for this worker. Pass it with `accepted` and `handoff`.
        #[arg(long)]
        worktree: Option<PathBuf>,
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
    // HS2-ZTG8FD: merge-pending pruning reads Git and ticket status, never the pool.
    if let CoordinationCmd::MergePendingPrune {
        remote,
        shipping,
        apply,
        no_fetch,
    } = &args.command
    {
        let store = FsStore::open_without_maintenance(&ctx.path)?;
        let options = hotsheet_cli::merge_pending::PruneOptions {
            project: args.project.clone(),
            remote: remote.clone(),
            shipping: shipping.clone(),
            apply: *apply,
            fetch: !*no_fetch,
        };
        let report = hotsheet_cli::merge_pending::prune(&options, &|slug| {
            Ok(ops::resolve(&store, slug)?.map(|ticket| ticket.status))
        })?;
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }
    // HS2-NEY1QH: the strategy policy is committed project settings, not pool state.
    if strategy_policy(&args.project, &args.command)? {
        return Ok(());
    }
    // HS2-2SJPCH: so is the provisional readiness policy.
    if speculation::policy(&args.project, &args.command)? {
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
        CoordinationCmd::PlanReplace {
            authority,
            expected_revision,
            entries_file,
            all_participants_upgraded,
        } => {
            let entries: Vec<NextPlanEntry> =
                serde_json::from_slice(&std::fs::read(entries_file)?)?;
            let revision = coordinator.replace_next_plan(
                &authority.id,
                &authority.session,
                &|| time::OffsetDateTime::now_utc().unix_timestamp(),
                expected_revision,
                entries,
                all_participants_upgraded,
            )?;
            println!("{}", serde_json::json!({"plan_revision": revision}));
            return Ok(());
        }
        CoordinationCmd::PlanEvents { after_revision } => {
            println!(
                "{}",
                serde_json::to_string_pretty(&coordinator.next_plan_events(after_revision)?)?
            );
            return Ok(());
        }
        CoordinationCmd::PlanDispatch {
            authority,
            expected_revision,
            operation,
            base,
        } => {
            let assigned = coordinator.dispatch_next_plan(
                &authority.id,
                &authority.session,
                &|| time::OffsetDateTime::now_utc().unix_timestamp(),
                expected_revision,
                &operation,
                &base,
            )?;
            println!("{}", serde_json::to_string_pretty(&assigned)?);
            return Ok(());
        }
        CoordinationCmd::RetireFinished {
            integrator: i,
            dry_run,
        } => {
            let report = coordinator.retire_finished(
                &i.id,
                &i.session,
                time::OffsetDateTime::now_utc(),
                hotsheet_ticketing::actor::note_actor(ctx.actor.as_ref()),
                dry_run,
            )?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            return Ok(());
        }
        CoordinationCmd::Workers {
            after,
            wait_ms,
            include_past,
        } => {
            let wait_ms = wait_ms.unwrap_or(30_000);
            anyhow::ensure!(wait_ms <= 30_000, "wait_ms must be at most 30000");
            let snapshot = match after {
                Some(after) => coordinator.watch_worker_snapshot(
                    hotsheet_cli::coordination::worker_view::WorkerView::coordination_cursor(
                        &after,
                    ),
                    std::time::Duration::from_millis(wait_ms),
                )?,
                None => coordinator.snapshot(now)?,
            };
            let project = if include_past {
                hotsheet_cli::coordination::worker_view::WorkerView::from_snapshot_including_past
            } else {
                hotsheet_cli::coordination::worker_view::WorkerView::from_snapshot
            };
            let view = project(
                &snapshot,
                time::OffsetDateTime::now_utc().unix_timestamp(),
                &std::collections::BTreeMap::new(),
                false,
            );
            println!("{}", serde_json::to_string_pretty(&view)?);
            return Ok(());
        }
        CoordinationCmd::Capabilities
        | CoordinationCmd::Strategies
        | CoordinationCmd::DevelopmentStrategies
        | CoordinationCmd::DevelopmentStrategySet { .. }
        | CoordinationCmd::DevelopmentCustomSet { .. }
        | CoordinationCmd::DevelopmentCustomSelect { .. }
        | CoordinationCmd::DevelopmentStrategyResolve { .. }
        | CoordinationCmd::StrategyAllow { .. }
        | CoordinationCmd::StrategyDisallow { .. }
        | CoordinationCmd::StrategyDefault { .. }
        | CoordinationCmd::MergePendingPrune { .. } => {
            unreachable!("handled before opening the pool")
        }
        CoordinationCmd::BatchPlan { .. }
        | CoordinationCmd::BatchOpen { .. }
        | CoordinationCmd::BatchAbort { .. }
        | CoordinationCmd::BatchPushed { .. }
        | CoordinationCmd::BatchResult { .. }
        | CoordinationCmd::BatchFollowUps { .. }
        | CoordinationCmd::BatchRecover { .. }
        | CoordinationCmd::BatchStaged { .. }
        | CoordinationCmd::BatchPromote { .. }
        | CoordinationCmd::BatchRestage { .. }
        | CoordinationCmd::BatchStatus => {
            let report = batch::run(&coordinator, ctx, &args.project, args.command, now)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            return Ok(());
        }
        CoordinationCmd::SpeculationPolicy { .. } => {
            unreachable!("handled before opening the pool")
        }
        CoordinationCmd::SpeculationExplain { .. }
        | CoordinationCmd::SpeculationStatus
        | CoordinationCmd::SpeculationResult { .. }
        | CoordinationCmd::SpeculationSync { .. }
        | CoordinationCmd::SpeculationRebase { .. }
        | CoordinationCmd::SpeculationReopen { .. }
        | CoordinationCmd::SpeculationBranch { .. } => {
            let report = speculation::run(&coordinator, ctx, &args.project, args.command, now)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            return Ok(());
        }
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
                    hotsheet_cli::coordination::scope::git_blob(&args.project, &base, path)?
                        .is_some(),
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
            let strategy = r.strategy_request(&args.project)?;
            let preferences = r.preference_request()?;
            coordinator.register_full(
                r.role,
                &r.identity.id,
                &r.identity.session,
                now,
                r.lease_seconds,
                r.route,
                strategy,
                preferences,
            )?;
        }
        CoordinationCmd::RefuseRequest {
            authority,
            worker,
            worker_session,
            slug,
            reason,
            detail,
        } => {
            coordinator.refuse_request(
                &authority.id,
                &authority.session,
                now,
                &worker,
                &worker_session,
                &slug,
                reason,
                detail,
            )?;
        }
        CoordinationCmd::Next {
            registration: r,
            after,
        } => {
            let strategy = r.strategy_request(&args.project)?;
            let preferences = r.preference_request()?;
            let snapshot = coordinator.next(&WaitRequest {
                role: r.role,
                id: r.identity.id,
                generation: r.identity.session,
                lease_seconds: r.lease_seconds,
                return_route: r.route,
                after,
                strategy,
                preferences,
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
            let connection_for_watch = connection.clone();
            let (author, checked_worker, checked_generation, checked_operation) = (
                i.id.clone(),
                worker.clone(),
                worker_session.clone(),
                operation.clone(),
            );
            let evidence = route_override.clone().zip(route_evidence_note.clone());
            // HS2-2SJPCH: the project's opt-in. Disabled, the dependency check below is
            // exactly the one that ran before the policy existed.
            let speculation = hotsheet_ticketing::provisional::SpeculationPolicy::read(
                &hotsheet_ticketing::Settings::for_project(&args.project),
            )?;
            let probe = speculation
                .enabled
                .then(|| coordinator.speculation_probe(&speculation));
            let reserved = std::cell::RefCell::new(None);
            let (reserved_at, probe_at, coordinator_at, speculation_at, checked_base) =
                (&reserved, &probe, &coordinator, &speculation, base.clone());
            // HS2-TWZW7X: eligibility, dependencies and route evidence are checked at the
            // serialized reservation boundary, after the current authority is verified, from
            // fresh ticket reads. The evidence author is therefore the active assignment
            // authority: the dispatcher with dispatcher authority, otherwise the integrator.
            let check: ReservationCheck<'_> = Box::new(move |_current, replayed| {
                let ticket = store.read_ticket(&ticket_id)?;
                if replayed.is_none() {
                    let tickets = store.list_tickets()?;
                    claims::assignable(
                        &ticket,
                        &tickets,
                        &checked_worker,
                        time::OffsetDateTime::now_utc(),
                    )?;
                    match probe_at {
                        // Confirmed, or provisional on a pushed prerequisite awaiting CI.
                        Some(probe) => {
                            *reserved_at.borrow_mut() = coordinator_at.speculation_reservation(
                                speculation_at,
                                probe,
                                &ticket,
                                &tickets,
                                &checked_base,
                            )?;
                        }
                        // Under either authority, as automatic selection does (HS2-PWDXB6).
                        None => claims::dependencies_closed(&ticket, &tickets)?,
                    }
                } else if let Some(probe) = probe_at {
                    // A replay repairs a ledger write interrupted after the reservation;
                    // it never refuses the durable reservation it acknowledges.
                    let tickets = store.list_tickets()?;
                    *reserved_at.borrow_mut() = coordinator_at
                        .speculation_reservation(
                            speculation_at,
                            probe,
                            &ticket,
                            &tickets,
                            &checked_base,
                        )
                        .ok()
                        .flatten();
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
            // HS2-26JAEN: the ledger record is written in the reservation's own lock
            // session, so no reader sees the reservation without it.
            let integration = speculation.integration;
            let record: hotsheet_cli::coordination::ReservationRecord<'_> =
                Box::new(move |c, state, assigned| {
                    if let Some(reservation) = reserved_at.borrow().as_ref() {
                        c.record_reserved(state, assigned, reservation, integration, now)?;
                    }
                    Ok(())
                });
            coordinator.assign_recorded(
                &i.id,
                &i.session,
                &|| time::OffsetDateTime::now_utc().unix_timestamp(),
                Assignment {
                    id: operation.clone(),
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
                    retirement_note: None,
                    worktree: None,
                },
                check,
                record,
            )?;
            if let Some(reservation) = reserved.into_inner() {
                let gates = reservation.gates;
                // HS2-X6FB9V: follow the pending gates from now on.
                coordinator.speculation_watch_reserved(&speculation, &connection_for_watch, &gates);
            }
        }
        CoordinationCmd::Worker {
            worker: w,
            assignment,
            phase,
            commit,
            findings_note,
            worktree,
        } => {
            // HS2-TRP2MJ: an existing directory, recorded by its resolved path.
            let worktree = worktree
                .map(|path| {
                    hotsheet_sync::paths::canonicalize(&path)
                        .ok()
                        .filter(|resolved| resolved.is_dir())
                        .and_then(|resolved| resolved.to_str().map(str::to_owned))
                        .ok_or_else(|| anyhow::anyhow!("worktree must be an existing directory"))
                })
                .transpose()?;
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
            // After the transition, so a refused transition records nothing.
            if let Some(path) = worktree {
                coordinator.worker_worktree(&w.id, &w.session, &assignment, path)?;
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
                    // HS2-V9BXWF: an archived (or otherwise closed) findings ticket is
                    // finished too; only an open ticket blocks resolving its mailbox.
                    // HS2-KBK83M: findings that leave the ticket open (for example still
                    // blocked) are not Done; the integrator review-returns them instead.
                    anyhow::ensure!(
                        !ops::is_open(&ticket),
                        "complete or close the findings ticket before resolving its mailbox; \
                         to accept findings that leave the ticket open, release your claim \
                         and use review-return (HS2-KBK83M)"
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

/// Run a strategy policy command against the project's committed settings; false for any
/// other command. The same validated read/write backs `PUT .../integration-strategies`.
fn strategy_policy(project: &std::path::Path, command: &CoordinationCmd) -> Result<bool> {
    if !matches!(
        command,
        CoordinationCmd::Strategies
            | CoordinationCmd::DevelopmentStrategies
            | CoordinationCmd::DevelopmentStrategySet { .. }
            | CoordinationCmd::DevelopmentCustomSet { .. }
            | CoordinationCmd::DevelopmentCustomSelect { .. }
            | CoordinationCmd::DevelopmentStrategyResolve { .. }
            | CoordinationCmd::StrategyAllow { .. }
            | CoordinationCmd::StrategyDisallow { .. }
            | CoordinationCmd::StrategyDefault { .. }
    ) {
        return Ok(false);
    }
    let settings = hotsheet_ticketing::Settings::for_project(project);
    if matches!(
        command,
        CoordinationCmd::DevelopmentStrategies
            | CoordinationCmd::DevelopmentStrategySet { .. }
            | CoordinationCmd::DevelopmentCustomSet { .. }
            | CoordinationCmd::DevelopmentCustomSelect { .. }
            | CoordinationCmd::DevelopmentStrategyResolve { .. }
    ) {
        let mut policy = DevelopmentPolicy::read(&settings)?;
        let integration = StrategyPolicy::read(&settings)?;
        match command {
            CoordinationCmd::DevelopmentStrategySet {
                preset,
                green_main_goal_minutes,
                best_effort_budget_minutes,
            } => {
                policy.default = *preset;
                policy.default_custom = None;
                if let Some(minutes) = green_main_goal_minutes {
                    policy.green_main_goal_minutes = *minutes;
                }
                if let Some(minutes) = best_effort_budget_minutes {
                    policy.best_effort_budget_minutes = Some(*minutes);
                }
                policy.write(&settings)?;
                policy = DevelopmentPolicy::read(&settings)?;
            }
            CoordinationCmd::DevelopmentCustomSet {
                name,
                file,
                select_default,
            } => {
                let custom: CustomDevelopmentStrategy =
                    serde_json::from_slice(&std::fs::read(file)?)?;
                policy.version = 2;
                policy.custom.insert(name.clone(), custom);
                if *select_default {
                    policy.default_custom = Some(name.clone());
                }
                policy.validate_project_capabilities(project)?;
                policy.write(&settings)?;
                policy = DevelopmentPolicy::read(&settings)?;
            }
            CoordinationCmd::DevelopmentCustomSelect { name } => {
                policy.version = 2;
                policy.default_custom = Some(name.clone());
                policy.validate_project_capabilities(project)?;
                policy.write(&settings)?;
                policy = DevelopmentPolicy::read(&settings)?;
            }
            CoordinationCmd::DevelopmentStrategyResolve { session, ticket } => {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&policy.resolve_named(
                        &integration,
                        session.as_deref(),
                        ticket.as_deref()
                    )?)?
                );
                return Ok(true);
            }
            _ => {}
        }
        println!(
            "{}",
            serde_json::to_string_pretty(&policy.report(&integration)?)?
        );
        return Ok(true);
    }
    let mut policy = StrategyPolicy::read(&settings)?;
    match command {
        CoordinationCmd::StrategyAllow {
            strategy,
            reason,
            max_red_batches,
            max_red_minutes,
            staging_branch,
        } => {
            let breaker = (max_red_batches.is_some() || max_red_minutes.is_some()).then(|| {
                let defaults = hotsheet_cli::coordination::strategy::CircuitBreaker::default();
                hotsheet_cli::coordination::strategy::CircuitBreaker {
                    max_red_batches: max_red_batches.unwrap_or(defaults.max_red_batches),
                    max_red_minutes: max_red_minutes.unwrap_or(defaults.max_red_minutes),
                }
            });
            policy.allow_with_options(*strategy, reason, breaker, staging_branch.clone())?
        }
        CoordinationCmd::StrategyDisallow { strategy } => policy.disallow(*strategy)?,
        CoordinationCmd::StrategyDefault { strategy } => policy.set_default(*strategy)?,
        _ => {}
    }
    if !matches!(command, CoordinationCmd::Strategies) {
        policy.write(&settings)?;
        policy = StrategyPolicy::read(&settings)?;
    }
    println!("{}", serde_json::to_string_pretty(&policy.report())?);
    Ok(true)
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

/// A dispatcher-scoped code handoff must descend from its assignment base and change only
/// what its scope declares, sections included (HS2-YHYNJ6). HS2-5P112Z: the library's
/// [`hotsheet_cli::coordination::scope::review_commit`], which a hosted pool shares.
fn review_commit_scope(
    project: &std::path::Path,
    base: &str,
    commit: &str,
    scope: &[String],
) -> Result<()> {
    hotsheet_cli::coordination::scope::review_commit(project, base, commit, scope)
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
