//! HS2-4J6V46: headless target operations, with cleanup before host config access.
//! HS2-SBQ3T8 adds access checks and the audit journal on the same runtime;
//! HS2-YCJDEB the throughput policy; HS2-XHQ1EH permission requests and remembered
//! rules in place of grants and approvals; HS2-AZ15GA target tickets;
//! HS2-EBHDSB dispatch runs; HS2-AHG7HW tool capacity; HS2-J8PRTV target capacity and
//! admission; HS2-WN0MXZ validation steps; HS2-5JBYPV CI and deployment gates of runs;
//! HS2-GDJXGZ orchestrator-owned sessions; HS2-HCW7MD Propose Plan.
use anyhow::{Result, bail};
use clap::{Args, Subcommand};
use hotsheet_cli::orchestration::{OrchestrationRuntime, request_cleanup_now};
use hotsheet_ticketing::{ProviderConfigRegistry, checkouts::CheckoutRegistry};
use serde_json::{Value, json};

use super::Ctx;
use super::orchestration_capacity::{self as capacity, Planned};
use super::orchestration_planning as planning;
use super::orchestration_projects as projects;
use super::orchestration_readiness as readiness;
use super::orchestration_scheduler as scheduler;
use super::orchestration_sessions as sessions;
use super::orchestration_validation as validation;
use crate::Cmd;

#[derive(Args)]
pub(crate) struct OrchestrationArgs {
    #[command(subcommand)]
    command: OrchestrationCmd,
}
#[derive(Subcommand)]
enum OrchestrationCmd {
    /// HS2-W27N1D: show or switch the project type (software or orchestration).
    ProjectType(projects::ProjectTypeArgs),
    /// HS2-W27N1D: add an open project (declare and bind in one step) or remove one.
    Project(projects::ProjectArgs),
    /// List, declare, rename, remove, bind, unbind, resolve and inspect project targets.
    Target(TargetArgs),
    /// Create, read, update and link tickets in a declared target.
    Ticket(TicketArgs),
    /// Dispatch work to an exact target ticket and supervise the run.
    Run(RunArgs),
    /// Start, list, sync, stop and take over the AI sessions this project owns on the
    /// servers that host its targets.
    Session(sessions::SessionArgs),
    /// Link the CI gate of a target commit to a run, read gates, and request a deployment.
    Gate(GateArgs),
    /// Show, set, validate and dry-run the checked-in throughput policy.
    Policy(PolicyArgs),
    /// HS2-XHQ1EH: list and answer the orchestrator's permission requests, forget a
    /// remembered rule, and check what an action would need.
    Permission(PermissionArgs),
    /// Show observed AI tool capacity, report a limit signal, and select a tool.
    ToolCapacity(ToolCapacityArgs),
    /// Read the capacity of the machine that hosts a target or a local checkout.
    Capacity(capacity::CapacityArgs),
    /// Show or set this machine's admission limits, or ask whether a session may start.
    Admission(capacity::AdmissionArgs),
    /// Define, list, run and remove extra validation steps of an orchestration ticket.
    Validation(validation::ValidationArgs),
    /// HS2-42TARA: plan prerequisites between orchestration steps and target tickets,
    /// and read what is ready, blocked, stale or cyclic.
    Readiness(readiness::ReadinessArgs),
    /// HS2-ECWG50: plan where sessions run, explain why a ticket is not running, read
    /// the decision log, or run one scheduling pass.
    Scheduler(scheduler::SchedulerArgs),
    /// HS2-HCW7MD: Propose Plan: start the project's planning AI session, which proposes
    /// the dry-run plan from the open goal tickets, or show where planning stands.
    Plan(planning::PlanArgs),
    /// HS2-M7EV18: the detached runner of one hosted validation step. Started by the
    /// hosting process; not for direct use.
    #[command(hide = true)]
    StepRunner {
        #[arg(long)]
        checkout: String,
        #[arg(long)]
        execution: String,
    },
    /// Read this machine's audit journal of orchestration decisions for one project.
    Audit {
        /// Registered owning checkout id.
        #[arg(long)]
        project: Option<String>,
        /// Return entries after this sequence number.
        #[arg(long, default_value_t = 0)]
        after: u64,
        #[arg(long, default_value_t = 100)]
        limit: u64,
    },
    /// HS2-CRBR8X: read this machine's journal of the sessions orchestrators started and
    /// stopped on one checkout it hosts: who asked, which tool, and what this machine did.
    HostedAudit {
        /// Registered checkout id on this machine (the hosted project).
        #[arg(long)]
        checkout: String,
        /// Return entries after this sequence number.
        #[arg(long, default_value_t = 0)]
        after: u64,
        #[arg(long, default_value_t = 100)]
        limit: u64,
    },
    /// HS2-S13719: lead time, blocked time, retries, CI and deployment outcomes and known
    /// AI cost of this project's runs, per run, work item and target. Identifiers and
    /// metrics only; a measure that cannot be computed says why instead of reading zero.
    /// Answers from this machine's records and contacts no target.
    Summary {
        /// Registered owning checkout id.
        #[arg(long)]
        project: Option<String>,
        /// Only the runs of this orchestration ticket (its id).
        #[arg(long)]
        ticket: Option<String>,
        /// The ticket source of --ticket, when the project has several.
        #[arg(long, requires = "ticket")]
        connection_id: Option<String>,
    },
    /// HS2-J89JC7: the plan as dependency waves (Done, Working Now, Next, Then, Later;
    /// First, Second, ... before kick-off), each item with its project, machine, tool,
    /// state and what it waits on. Answers from this machine's records and contacts no
    /// target. With --after <version> and --wait-seconds, waits for the next change.
    Timeline {
        /// Registered owning checkout id.
        #[arg(long)]
        project: Option<String>,
        /// The version of the timeline already seen.
        #[arg(long)]
        after: Option<String>,
        /// With --after: wait this long for a change (at most 20).
        #[arg(long, default_value_t = 0, requires = "after")]
        wait_seconds: u64,
    },
    /// HS2-S3FKJ7: from a target project's side, the orchestration line of each of its
    /// tickets an orchestration project on this machine plans (state, where it runs,
    /// goal ticket), one per ticket. Answers from this machine's records and contacts
    /// no target. With --after <version> and --wait-seconds, waits for the next change.
    ///
    /// HS2-3D8D6V: with --registration-id, --project is a checkout on that remote
    /// server, and the lines are those an orchestration project on this machine plans
    /// through a reviewed remote binding; the remote server is not contacted.
    TargetLines {
        /// Registered target checkout id, or with --registration-id the remote checkout id.
        #[arg(long)]
        project: Option<String>,
        /// The reviewed remote registration that serves --project.
        #[arg(long)]
        registration_id: Option<String>,
        /// The version of the lines already seen.
        #[arg(long)]
        after: Option<String>,
        /// With --after: wait this long for a change (at most 20).
        #[arg(long, default_value_t = 0, requires = "after")]
        wait_seconds: u64,
    },
    /// HS2-KB2Y51: send this machine's orchestration lines to the remote servers
    /// that host the project's targets once now, so viewers of those targets see
    /// them. The running server does this by itself on every change; this is the
    /// headless re-assert. Sends display facts only, and only to reviewed bindings.
    TargetLinesPush {
        /// Registered orchestration project checkout id.
        #[arg(long)]
        project: Option<String>,
    },
}
#[derive(Args)]
struct GateArgs {
    /// Registered owning checkout id.
    #[arg(long, global = true)]
    project: Option<String>,
    #[command(subcommand)]
    command: GateCmd,
}
#[derive(Subcommand)]
enum GateCmd {
    /// Tie the CI gate of the integrated target commit to a run and start watching it.
    /// Repeating it returns the same gate, and watches again after a failed or stopped
    /// watch; another commit supersedes the older link. Observing never asks a person.
    Link {
        run_id: String,
        /// The integrated target commit: all 40 hex characters.
        #[arg(long)]
        commit: String,
        /// owner/name. Defaults to, and must agree with, the GitHub repository the
        /// target declares.
        #[arg(long)]
        repo: Option<String>,
        /// The workflow name (default CI).
        #[arg(long)]
        workflow: Option<String>,
        /// The shipping branch whose runs count (default main).
        #[arg(long)]
        branch: Option<String>,
        /// The CI provider adapter: github-actions (default), gitlab-ci (HS2-TPAHYA),
        /// buildkite (`--repo <organization>/<pipeline>`) or circleci (`--repo
        /// <vcs>/<organization>/<project>`, HS2-17HVNP).
        #[arg(long)]
        provider: Option<String>,
        /// HS2-TQG6AT: the artifact whose digest a passed verdict records, by name;
        /// needed when the workflow builds several.
        #[arg(long)]
        artifact: Option<String>,
        /// Do not start this machine's Job monitor. The gate then stays pending until
        /// `hotsheet-cli job watch start` or another process observes the provider.
        #[arg(long)]
        no_monitor: bool,
    },
    /// The standing gates of a run, or of one commit of a target, with the rolled-up
    /// state. Reads this machine's records and contacts nothing.
    List {
        #[arg(long)]
        run: Option<String>,
        #[arg(long)]
        target: Option<String>,
        #[arg(long)]
        commit: Option<String>,
    },
    /// One gate with its evidence and history, superseded ones included.
    Show { gate_id: String },
    /// Bring a run's gates in line with what the provider adapter recorded. With
    /// --wait-seconds and nothing new it waits for the next change instead of the
    /// caller polling. Contacts no provider.
    Sync {
        run_id: String,
        /// At most 20.
        #[arg(long, default_value_t = 0)]
        wait_seconds: u64,
    },
    /// Ask for a deployment of the commit the run's CI gate passed, through the
    /// target's checked-in script. Asks a person through a permission request unless a
    /// remembered rule allows it; with --production a person is always asked. Answer
    /// with `orchestration permission answer`, then repeat this command with the same
    /// --operation-id.
    Deploy {
        run_id: String,
        #[arg(long)]
        environment: String,
        /// The deploy script, relative to the target repository root.
        #[arg(long)]
        script: String,
        #[arg(long)]
        operation_id: String,
        /// An irreversible production change.
        #[arg(long)]
        production: bool,
        /// The deployment adapter (default target-script).
        #[arg(long)]
        provider: Option<String>,
        /// The built artifact's name, with --artifact-digest.
        #[arg(long, requires = "artifact_digest")]
        artifact_name: Option<String>,
        /// The artifact's lowercase SHA-256.
        #[arg(long, requires = "artifact_name")]
        artifact_digest: Option<String>,
    },
    /// HS2-GD3NNK: mark the code review of the run's commit. `requested` (with
    /// --commit) links a review gate awaiting approval; `approved` and
    /// `changes-requested` answer the review that stands for the run. Bookkeeping only:
    /// it never asks a person.
    Review {
        run_id: String,
        #[arg(long, value_enum)]
        review: ReviewArg,
        /// The commit under review: all 40 hex characters. Required for `requested`.
        #[arg(long)]
        commit: Option<String>,
    },
}
#[derive(Clone, Copy, clap::ValueEnum)]
enum ReviewArg {
    Requested,
    Approved,
    ChangesRequested,
}
/// The request of a gate command, and whether the Job monitor should run afterwards.
fn gate_route(project: &str, command: GateCmd) -> ((&'static str, String, Value), bool) {
    let base = format!("/checkouts/{project}/orchestration");
    match command {
        GateCmd::Link {
            run_id,
            commit,
            repo,
            workflow,
            branch,
            provider,
            artifact,
            no_monitor,
        } => (
            (
                "POST",
                format!("{base}/runs/{run_id}/gates"),
                json!({"commit":commit,"repository":repo,"workflow":workflow,"branch":branch,
                    "provider":provider,"artifact":artifact}),
            ),
            !no_monitor,
        ),
        GateCmd::List {
            run,
            target,
            commit,
        } => {
            let query: Vec<String> = [("run", run), ("target_id", target), ("commit", commit)]
                .into_iter()
                .filter_map(|(key, value)| value.map(|value| format!("{key}={value}")))
                .collect();
            (
                (
                    "GET",
                    format!("{base}/gates?{}", query.join("&")),
                    Value::Null,
                ),
                false,
            )
        }
        GateCmd::Show { gate_id } => (
            ("GET", format!("{base}/gates/{gate_id}"), Value::Null),
            false,
        ),
        GateCmd::Sync {
            run_id,
            wait_seconds,
        } => (
            (
                "POST",
                format!("{base}/runs/{run_id}/gate-sync"),
                json!({"wait_ms":wait_seconds.saturating_mul(1000)}),
            ),
            false,
        ),
        GateCmd::Review {
            run_id,
            review,
            commit,
        } => (
            (
                "POST",
                format!("{base}/runs/{run_id}/reviews"),
                json!({"review": match review {
                    ReviewArg::Requested => "requested",
                    ReviewArg::Approved => "approved",
                    ReviewArg::ChangesRequested => "changes_requested",
                }, "commit": commit}),
            ),
            false,
        ),
        GateCmd::Deploy {
            run_id,
            environment,
            script,
            operation_id,
            production,
            provider,
            artifact_name,
            artifact_digest,
        } => (
            (
                "POST",
                format!("{base}/runs/{run_id}/deployments"),
                json!({"environment":environment,"script":script,"operation_id":operation_id,
                    "production":production,"provider":provider,
                    "artifact":artifact_name.zip(artifact_digest)
                        .map(|(name, digest)| json!({"name":name,"digest":digest}))}),
            ),
            false,
        ),
    }
}
#[derive(Args)]
struct RunArgs {
    /// Registered owning checkout id.
    #[arg(long, global = true)]
    project: Option<String>,
    #[command(subcommand)]
    command: RunCmd,
}
#[derive(Subcommand)]
enum RunCmd {
    /// Start work on one exact target ticket. Repeating the command with the same
    /// --operation-id returns the same run and never claims twice.
    Dispatch {
        /// The orchestration ticket (slug or id) the work is for.
        #[arg(long)]
        ticket: String,
        #[arg(long, default_value = "")]
        source: String,
        #[arg(long)]
        target: String,
        #[arg(long)]
        connection: String,
        /// The exact target ticket.
        #[arg(long)]
        id: String,
        #[arg(long)]
        operation_id: String,
        /// direct (default): claim the ticket for --worker. dispatcher: mark it Up Next
        /// and let the target's own dispatcher assign it.
        #[arg(long)]
        via: Option<String>,
        #[arg(long)]
        worker: Option<String>,
        #[arg(long)]
        worker_label: Option<String>,
        /// Lease length for a direct claim, 5 to 240 (default 30).
        #[arg(long)]
        lease_minutes: Option<u32>,
        /// HS2-42TARA: start this operation although its plan step is not ready. A
        /// person's own override goes ahead; an AI's waits as `decision_required` on a
        /// decision note on the goal ticket (HS2-XHQ1EH); repeat it after the answer.
        #[arg(long)]
        override_readiness: bool,
    },
    /// Runs of this project; --active leaves out finished ones.
    List {
        #[arg(long)]
        active: bool,
    },
    /// One run with its state, lease, history and last confirmed target state.
    Show { run_id: String },
    /// Extend the lease of a directly dispatched run.
    Renew {
        run_id: String,
        #[arg(long)]
        lease_minutes: Option<u32>,
    },
    /// Release the run's claim (or withdraw its Up Next mark) and end the run.
    Cancel { run_id: String },
    /// Read the target ticket and bring the run in line with it.
    Sync { run_id: String },
    /// The run with its last confirmed target state and whether that is stale.
    /// Contacts nothing.
    Status { run_id: String },
    /// Reconcile one run, or every unfinished run without a run id, with its target
    /// after an interruption. Starts nothing; names what could not be verified.
    Reconcile { run_id: Option<String> },
    /// Stop renewing and repeating a run until it is resumed. The target is not told.
    Pause {
        run_id: String,
        /// A stable reason code, such as maintenance.
        #[arg(long)]
        reason: Option<String>,
    },
    /// Resume a paused run and reconcile it at once.
    Resume { run_id: String },
    /// Hand a run to another worker, or without --to-worker to a person: the run lets
    /// go of its claim and keeps following the ticket.
    Handoff {
        run_id: String,
        #[arg(long)]
        to_worker: Option<String>,
        #[arg(long)]
        worker_label: Option<String>,
        /// Lease length for the new worker's claim, 5 to 240 (default 30).
        #[arg(long)]
        lease_minutes: Option<u32>,
    },
    /// Correct a run's record by hand (a person's action; pass --actor-role human).
    /// Only this machine's record changes.
    Correct {
        run_id: String,
        /// requested, queued, dispatched, intervened, completed, failed or cancelled.
        #[arg(long)]
        state: String,
        /// A stable reason code.
        #[arg(long)]
        reason: String,
        /// The worker that really holds the work, when that was wrong too.
        #[arg(long)]
        worker: Option<String>,
    },
    /// Events after a cursor, oldest first. With --wait-seconds an empty answer waits
    /// for the next event instead of the caller polling.
    Events {
        #[arg(long, default_value_t = 0)]
        after: u64,
        #[arg(long, default_value_t = 100)]
        limit: u64,
        /// Only this run's events, or this plan step's (its readiness and provisional
        /// events name it as `step_id`, HS2-J7RG42).
        #[arg(long)]
        run: Option<String>,
        /// At most 20.
        #[arg(long, default_value_t = 0)]
        wait_seconds: u64,
    },
}
fn run_route(project: &str, command: RunCmd) -> (&'static str, String, Value) {
    let base = format!("/checkouts/{project}/orchestration/runs");
    match command {
        RunCmd::Dispatch {
            ticket,
            source,
            target,
            connection,
            id,
            operation_id,
            via,
            worker,
            worker_label,
            lease_minutes,
            override_readiness,
        } => (
            "POST",
            base,
            json!({"ticket":{"connection_id":source,"native_id":ticket},"target_id":target,
                "connection_id":connection,"native_id":id,"operation_id":operation_id,
                "mode":via,"worker_id":worker,"worker_label":worker_label,
                "lease_minutes":lease_minutes,"override_readiness":override_readiness}),
        ),
        RunCmd::List { active } => ("GET", format!("{base}?active={active}"), Value::Null),
        RunCmd::Show { run_id } => ("GET", format!("{base}/{run_id}"), Value::Null),
        RunCmd::Renew {
            run_id,
            lease_minutes,
        } => (
            "POST",
            format!("{base}/{run_id}/renew"),
            json!({"lease_minutes":lease_minutes}),
        ),
        RunCmd::Cancel { run_id } => ("POST", format!("{base}/{run_id}/cancel"), json!({})),
        RunCmd::Sync { run_id } => ("POST", format!("{base}/{run_id}/sync"), json!({})),
        RunCmd::Status { run_id } => ("GET", format!("{base}/{run_id}/status"), Value::Null),
        // HS2-WBREDM
        RunCmd::Reconcile { run_id: None } => (
            "POST",
            format!("/checkouts/{project}/orchestration/run-reconcile"),
            json!({}),
        ),
        RunCmd::Reconcile {
            run_id: Some(run_id),
        } => ("POST", format!("{base}/{run_id}/reconcile"), json!({})),
        RunCmd::Pause { run_id, reason } => (
            "POST",
            format!("{base}/{run_id}/pause"),
            json!({"reason":reason}),
        ),
        RunCmd::Resume { run_id } => ("POST", format!("{base}/{run_id}/resume"), json!({})),
        RunCmd::Handoff {
            run_id,
            to_worker,
            worker_label,
            lease_minutes,
        } => (
            "POST",
            format!("{base}/{run_id}/handoff"),
            json!({"to_worker":to_worker,"worker_label":worker_label,
                "lease_minutes":lease_minutes}),
        ),
        RunCmd::Correct {
            run_id,
            state,
            reason,
            worker,
        } => (
            "POST",
            format!("{base}/{run_id}/correct"),
            json!({"state":state,"reason":reason,"worker_id":worker}),
        ),
        RunCmd::Events {
            after,
            limit,
            run,
            wait_seconds,
        } => (
            "GET",
            format!(
                "/checkouts/{project}/orchestration/run-events?after={after}&limit={limit}&wait_ms={}{}",
                wait_seconds.saturating_mul(1000),
                run.map(|run| format!("&run={run}")).unwrap_or_default()
            ),
            Value::Null,
        ),
    }
}
#[derive(Args)]
struct TicketArgs {
    /// Registered owning checkout id.
    #[arg(long, global = true)]
    project: Option<String>,
    #[command(subcommand)]
    command: TicketCmd,
}
/// The fields an orchestration project may set on a target ticket.
#[derive(Args)]
struct TicketFields {
    #[arg(long)]
    title: Option<String>,
    #[arg(long)]
    details: Option<String>,
    #[arg(long)]
    category: Option<String>,
    #[arg(long)]
    priority: Option<String>,
    #[arg(long)]
    status: Option<String>,
    /// Replaces the ticket's tags. Repeatable.
    #[arg(long = "tag")]
    tags: Vec<String>,
    #[arg(long)]
    up_next: Option<bool>,
}
impl TicketFields {
    fn into_map(self) -> serde_json::Map<String, Value> {
        let mut map = serde_json::Map::new();
        for (key, value) in [
            ("title", self.title),
            ("details", self.details),
            ("category", self.category),
            ("priority", self.priority),
            ("status", self.status),
        ] {
            if let Some(value) = value {
                map.insert(key.into(), value.into());
            }
        }
        if !self.tags.is_empty() {
            map.insert("tags".into(), self.tags.into());
        }
        if let Some(up_next) = self.up_next {
            map.insert("up_next".into(), up_next.into());
        }
        map
    }
}
#[derive(Subcommand)]
enum TicketCmd {
    /// Create a ticket in a target for an orchestration ticket. Repeating the command
    /// with the same --operation-id never creates a second ticket.
    Create {
        /// The orchestration ticket (slug or id) the new ticket is for.
        #[arg(long)]
        ticket: String,
        /// The orchestration ticket's source connection; omit when the project has
        /// one git ticket source.
        #[arg(long, default_value = "")]
        source: String,
        #[arg(long)]
        target: String,
        /// The target's ticket source to create in.
        #[arg(long)]
        connection: String,
        #[arg(long)]
        operation_id: String,
        #[command(flatten)]
        fields: TicketFields,
    },
    /// Read one target ticket.
    Show {
        #[arg(long)]
        target: String,
        #[arg(long)]
        connection: String,
        #[arg(long)]
        id: String,
    },
    /// Change fields of one target ticket through its own provider.
    Update {
        #[arg(long)]
        target: String,
        #[arg(long)]
        connection: String,
        #[arg(long)]
        id: String,
        /// The provider's concurrency token from a previous read.
        #[arg(long)]
        expected_token: Option<String>,
        #[arg(long)]
        operation_id: Option<String>,
        #[command(flatten)]
        fields: TicketFields,
    },
    /// Link an existing target ticket to an orchestration ticket.
    Link {
        #[arg(long)]
        ticket: String,
        #[arg(long, default_value = "")]
        source: String,
        #[arg(long)]
        target: String,
        #[arg(long)]
        connection: String,
        #[arg(long)]
        id: String,
        /// observed (default) or validated.
        #[arg(long)]
        relation: Option<String>,
    },
    /// Remove an orchestration ticket's link; the target ticket is not touched.
    Unlink {
        #[arg(long)]
        ticket: String,
        #[arg(long, default_value = "")]
        source: String,
        #[arg(long)]
        target: String,
        #[arg(long)]
        connection: String,
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        operation_id: Option<String>,
    },
    /// List or search a target's open tickets, one bounded page at a time (HS2-D85G19).
    /// Search before creating, so an existing ticket is linked rather than doubled.
    List {
        #[arg(long)]
        target: String,
        /// The target's ticket source to list.
        #[arg(long)]
        connection: String,
        /// Text to search for in the target's open tickets.
        #[arg(long)]
        text: Option<String>,
        #[arg(long)]
        category: Option<String>,
        /// Only tickets carrying this tag.
        #[arg(long)]
        tag: Option<String>,
        /// The `next_cursor` of the previous page.
        #[arg(long)]
        cursor: Option<String>,
        /// Rows per page, 1 to 100 (default 50).
        #[arg(long)]
        limit: Option<u32>,
    },
    /// An orchestration ticket's links; --resolve also reads each target ticket.
    Links {
        #[arg(long)]
        ticket: String,
        #[arg(long, default_value = "")]
        source: String,
        #[arg(long)]
        resolve: bool,
    },
}
fn ticket_route(project: &str, command: TicketCmd) -> (&'static str, String, Value) {
    let path = |operation: &str| format!("/checkouts/{project}/orchestration/tickets/{operation}");
    let orchestration =
        |ticket: String, source: String| json!({"connection_id":source,"native_id":ticket});
    let with = |mut body: Value, fields: TicketFields| {
        if let Some(object) = body.as_object_mut() {
            object.extend(fields.into_map());
        }
        body
    };
    match command {
        TicketCmd::Create {
            ticket,
            source,
            target,
            connection,
            operation_id,
            fields,
        } => (
            "POST",
            path("create"),
            with(
                json!({"ticket":orchestration(ticket, source),"target_id":target,
                    "connection_id":connection,"operation_id":operation_id}),
                fields,
            ),
        ),
        TicketCmd::Show {
            target,
            connection,
            id,
        } => (
            "POST",
            path("read"),
            json!({"target_id":target,"connection_id":connection,"native_id":id}),
        ),
        TicketCmd::Update {
            target,
            connection,
            id,
            expected_token,
            operation_id,
            fields,
        } => (
            "POST",
            path("update"),
            with(
                json!({"target_id":target,"connection_id":connection,"native_id":id,
                    "expected_token":expected_token,"operation_id":operation_id}),
                fields,
            ),
        ),
        TicketCmd::Link {
            ticket,
            source,
            target,
            connection,
            id,
            relation,
        } => (
            "POST",
            path("link"),
            json!({"ticket":orchestration(ticket, source),"target_id":target,
                "connection_id":connection,"native_id":id,"relation":relation}),
        ),
        TicketCmd::Unlink {
            ticket,
            source,
            target,
            connection,
            id,
            operation_id,
        } => (
            "POST",
            path("unlink"),
            json!({"ticket":orchestration(ticket, source),"target_id":target,
                "connection_id":connection,"native_id":id,"operation_id":operation_id}),
        ),
        TicketCmd::List {
            target,
            connection,
            text,
            category,
            tag,
            cursor,
            limit,
        } => (
            "POST",
            path("list"),
            json!({"target_id":target,"connection_id":connection,"text":text,
                "category":category,"tag":tag,"cursor":cursor,"limit":limit}),
        ),
        TicketCmd::Links {
            ticket,
            source,
            resolve,
        } => (
            "POST",
            path("links"),
            json!({"ticket":orchestration(ticket, source),"resolve":resolve}),
        ),
    }
}
#[derive(Args)]
struct PolicyArgs {
    /// Registered owning checkout id.
    #[arg(long, global = true)]
    project: Option<String>,
    #[command(subcommand)]
    command: PolicyCmd,
}
#[derive(Subcommand)]
enum PolicyCmd {
    /// The stored policy and its revision; an absent policy reads as disabled.
    Show,
    /// Replace the policy from a JSON file (`-` for stdin). Re-running with the same
    /// content changes nothing. A person runs this; an AI actor is refused.
    Set {
        #[arg(long)]
        file: String,
        /// Require this stored revision; omitted, the current one is read first.
        #[arg(long)]
        expected_revision: Option<String>,
    },
    /// Report every problem in a policy file, or in the stored policy. Writes nothing.
    Validate {
        #[arg(long)]
        file: Option<String>,
    },
    /// What a policy's limits would let the orchestrator do per declared target, and
    /// whether a person would be asked or a remembered rule answers. Writes nothing
    /// and contacts no target.
    DryRun {
        #[arg(long)]
        file: Option<String>,
    },
    /// HS2-TK9ADC: which role sessions the scheduler starts by itself on one target
    /// (`placement.role_sessions`). Reads the stored policy, replaces that target's
    /// entry and sets it at the revision read, as the Limits form does. Without
    /// `--role` the target gets none. A target the policy does not list is Auto: the
    /// scheduler starts either role there, with placement, when it helps
    /// (HS2-R2N33M); `--auto` returns the target to that. A person runs this; an AI
    /// actor is refused.
    RoleSessions {
        #[arg(long)]
        target: String,
        /// `dispatcher` or `integrator`; repeat for both.
        #[arg(long = "role", value_parser = ["dispatcher", "integrator"], conflicts_with = "auto")]
        roles: Vec<String>,
        /// HS2-R2N33M: remove the target's entry, so its role sessions are Auto.
        #[arg(long)]
        auto: bool,
    },
}
#[derive(Args)]
struct PermissionArgs {
    /// Registered owning checkout id.
    #[arg(long, global = true)]
    project: Option<String>,
    #[command(subcommand)]
    command: PermissionCmd,
}
#[derive(Subcommand)]
enum PermissionCmd {
    /// This project's permission requests with their state, its remembered rules, and
    /// any retired grant or approval files on this machine.
    List,
    /// Answer one waiting request as a person: allow_once (used once, when the action
    /// happens), always_allow (also remembers a rule for its target, action and tool)
    /// or deny (final for that operation). An AI actor is refused.
    Answer {
        request_id: String,
        #[arg(long, value_parser = ["allow_once", "always_allow", "deny"])]
        answer: String,
    },
    /// Remember a rule ahead of any request, as Always Allow would: actions of this
    /// kind on the target (and, for session_start, with this tool) stop asking. A
    /// person runs this; an AI actor is refused. Idempotent.
    Remember {
        target_id: String,
        /// tickets_write, work_start, session_start, validation_run, deployments_run
        /// or provisional_dispatch.
        #[arg(long)]
        action: String,
        /// The AI tool, for session_start only.
        #[arg(long)]
        tool: Option<String>,
    },
    /// Forget one remembered rule; later actions with its key ask again.
    Forget { rule_id: String },
    /// HS2-JWV5S1: show the retired grant and approval files on this machine with
    /// their counts. With `--remove`, a person removes them and the counts removed are
    /// printed; running it again removes nothing. Neither file is ever authority.
    Retired {
        #[arg(long)]
        remove: bool,
    },
    /// Report what an action on a target would need now, against the cached binding.
    /// Files nothing and is not authority.
    Check {
        target_id: String,
        #[arg(long)]
        actor: String,
        #[arg(long)]
        action: String,
        /// The exact operation, to report its permission request.
        #[arg(long)]
        operation_id: Option<String>,
        #[arg(long)]
        tool: Option<String>,
        #[arg(long)]
        model: Option<String>,
        /// The check concerns starting one more session.
        #[arg(long)]
        starting: bool,
        /// HS2-YCJDEB policy caps: orchestrator sessions already on the target.
        #[arg(long, default_value_t = 0)]
        target_sessions: u32,
        /// Orchestrator sessions already on the target's machine.
        #[arg(long, default_value_t = 0)]
        machine_sessions: u32,
        /// Orchestrator sessions already using the tool.
        #[arg(long, default_value_t = 0)]
        tool_sessions: u32,
        /// The tool account the session would run under.
        #[arg(long)]
        account: Option<String>,
        #[arg(long, default_value_t = 0)]
        account_sessions: u32,
    },
}
fn permission_route(
    project: &str,
    command: PermissionCmd,
) -> Result<(&'static str, String, Value)> {
    let prefix = format!("/checkouts/{project}/orchestration");
    Ok(match command {
        PermissionCmd::List => ("GET", format!("{prefix}/permissions"), Value::Null),
        PermissionCmd::Answer { request_id, answer } => (
            "POST",
            format!("{prefix}/permissions/{request_id}"),
            json!({ "answer": answer }),
        ),
        PermissionCmd::Remember {
            target_id,
            action,
            tool,
        } => (
            "POST",
            format!("{prefix}/permission-rules"),
            json!({ "target_id": target_id, "action": action, "tool": tool }),
        ),
        PermissionCmd::Forget { rule_id } => (
            "DELETE",
            format!("{prefix}/permission-rules/{rule_id}"),
            json!({}),
        ),
        PermissionCmd::Retired { remove } => (
            if remove { "DELETE" } else { "GET" },
            format!("{prefix}/retired-permissions"),
            if remove { json!({}) } else { Value::Null },
        ),
        PermissionCmd::Check {
            target_id,
            actor,
            action,
            operation_id,
            tool,
            model,
            starting,
            target_sessions,
            machine_sessions,
            tool_sessions,
            account,
            account_sessions,
        } => {
            // A session check always states its load; zeros mean an idle machine.
            let load = tool.as_ref().map(|_| {
                json!({"target_sessions":target_sessions,"machine_sessions":machine_sessions,
                    "tool_sessions":tool_sessions,"account":account,"account_sessions":account_sessions})
            });
            let session = match tool {
                Some(tool) => json!({"tool":tool,"model":model,"starting":starting}),
                None if model.is_some() || starting => bail!("--model and --starting need --tool"),
                None => Value::Null,
            };
            (
                "POST",
                format!("{prefix}/targets/{target_id}/access"),
                json!({"actor_id":actor,"action":action,"operation_id":operation_id,
                    "session":session,"load":load}),
            )
        }
    })
}
#[derive(Args)]
struct ToolCapacityArgs {
    /// Registered owning checkout id.
    #[arg(long, global = true)]
    project: Option<String>,
    #[command(subcommand)]
    command: ToolCapacityCmd,
}
#[derive(Subcommand)]
enum ToolCapacityCmd {
    /// The capacity state of every tool account the policy configures: available,
    /// limited until a time, degraded or unknown. Unknown is never assumed available.
    Show,
    /// Report one observed signal for a configured tool. Re-running the same report
    /// changes nothing. `recovered` clears a limit; an AI actor is refused for it.
    Signal {
        #[arg(long)]
        tool: String,
        /// The policy account; omitted, the signal is recorded as unattributed.
        #[arg(long)]
        account: Option<String>,
        /// rate_limit, usage_limit, recovered or usage.
        #[arg(long)]
        kind: String,
        /// When it was observed, in Unix milliseconds (default: now).
        #[arg(long)]
        at_ms: Option<u64>,
        /// rate_limit/usage_limit: when the provider says the limit resets.
        #[arg(long)]
        resets_at_ms: Option<u64>,
        /// usage: the turn's cost in cents; omitted, the cost is unknown.
        #[arg(long)]
        cost_cents: Option<u64>,
        /// An opaque session id that keeps two sessions' equal signals apart.
        #[arg(long)]
        session: Option<String>,
    },
    /// The ordered tool accounts a session for one target may start on now, and why
    /// each other one is excluded. Reads only.
    Select {
        /// The declared target's id.
        #[arg(long)]
        target: String,
        /// The preferred tool; omitted, any tool the target allows.
        #[arg(long)]
        tool: Option<String>,
        #[arg(long)]
        model: Option<String>,
        /// Sessions now running, as `tool=N` or `tool/account=N`; repeat per tool
        /// account. Omit when none are running.
        #[arg(long = "load")]
        load: Vec<String>,
    },
}
/// `tool=N` or `tool/account=N` as one observed session count.
fn session_count(value: &str) -> Result<Value> {
    let invalid = || anyhow::anyhow!("--load must be tool=N or tool/account=N");
    let (name, sessions) = value.split_once('=').ok_or_else(invalid)?;
    let sessions: u32 = sessions.parse().map_err(|_| invalid())?;
    let (tool, account) = match name.split_once('/') {
        Some((tool, account)) => (tool, Some(account)),
        None => (name, None),
    };
    if tool.is_empty() || account.is_some_and(str::is_empty) {
        return Err(invalid());
    }
    Ok(json!({"tool":tool,"account":account,"sessions":sessions}))
}
fn tool_capacity_route(
    project: &str,
    command: ToolCapacityCmd,
) -> Result<(&'static str, String, Value)> {
    let base = format!("/checkouts/{project}/orchestration/tool-capacity");
    Ok(match command {
        ToolCapacityCmd::Show => ("GET", base, Value::Null),
        ToolCapacityCmd::Signal {
            tool,
            account,
            kind,
            at_ms,
            resets_at_ms,
            cost_cents,
            session,
        } => {
            let at_ms = match at_ms {
                Some(at_ms) => at_ms,
                None => u64::try_from(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)?
                        .as_millis(),
                )?,
            };
            (
                "POST",
                format!("{base}/signals"),
                json!({"tool":tool,"account":account,"kind":kind,"at_ms":at_ms,
                    "resets_at_ms":resets_at_ms,"cost_cents":cost_cents,"session":session}),
            )
        }
        ToolCapacityCmd::Select {
            target,
            tool,
            model,
            load,
        } => (
            "POST",
            format!("{base}/select"),
            json!({"target_id":target,"tool":tool,"model":model,
                "load":load.iter().map(|value| session_count(value)).collect::<Result<Vec<_>>>()?}),
        ),
    })
}
/// HS2-TK9ADC: `policy` with one target's `placement.role_sessions` entry replaced
/// by `edit` (`{target_id, roles}`), in place. HS2-R2N33M: no roles is kept as the
/// choice of none; `auto` removes the entry, so the target is Auto again. Every other
/// member round-trips unchanged.
fn with_role_sessions(mut policy: Value, edit: &Value) -> Value {
    let target = edit["target_id"].clone();
    let entry =
        (edit["auto"] != json!(true)).then(|| json!({"target_id": target, "roles": edit["roles"]}));
    if !policy.is_object() {
        policy = json!({});
    }
    let placement = &mut policy["placement"];
    if !placement.is_object() {
        *placement = json!({});
    }
    let mut list: Vec<Value> = placement["role_sessions"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    match (
        list.iter().position(|item| item["target_id"] == target),
        entry,
    ) {
        (Some(at), Some(entry)) => list[at] = entry,
        (Some(at), None) => {
            list.remove(at);
        }
        (None, Some(entry)) => list.push(entry),
        (None, None) => {}
    }
    if let Some(object) = placement.as_object_mut() {
        if list.is_empty() {
            object.remove("role_sessions");
        } else {
            object.insert("role_sessions".into(), Value::Array(list));
        }
    }
    policy
}

/// HS2-SYZB33: one policy warning as a line a person reads.
fn policy_warning_line(warning: &Value) -> String {
    let text = |key: &str| warning[key].as_str().unwrap_or("unknown").to_owned();
    match warning["code"].as_str() {
        Some(hotsheet_ticketing::orchestration::ROLE_SESSIONS_NOT_HOME) => format!(
            "warning: role_sessions_not_home: target {} is a site of the project whose home is {}; role sessions start only on the home, so this entry has no effect. Set them with --target {}.",
            text("subject"),
            text("home_target_id"),
            text("home_target_id"),
        ),
        _ => format!(
            "warning: {}: {} ({})",
            text("code"),
            text("message"),
            text("subject")
        ),
    }
}

/// A policy body from a file, or stdin for `-`.
fn policy_file(path: &str) -> Result<Value> {
    let text = if path == "-" {
        std::io::read_to_string(std::io::stdin())?
    } else {
        std::fs::read_to_string(path)
            .map_err(|error| anyhow::anyhow!("cannot read policy file: {error}"))?
    };
    let mut value: Value =
        serde_json::from_str(&text).map_err(|_| anyhow::anyhow!("policy file must be JSON"))?;
    // Accept either a bare policy body or the `show` output, so show > edit > set works.
    if let Some(policy) = value.get_mut("policy").map(Value::take) {
        value = policy;
    }
    Ok(value)
}
#[derive(Args)]
struct TargetArgs {
    /// Registered owning checkout id, never a source/store id or label.
    #[arg(long, global = true)]
    project: Option<String>,
    #[command(subcommand)]
    command: TargetCmd,
}
#[derive(Subcommand)]
enum TargetCmd {
    /// Shared declarations; --target-id reads that exact local cached binding for recovery.
    List {
        #[arg(long)]
        target_id: Option<String>,
    },
    /// Omit expected-revision only for an absent manifest. Retain target-id for safe retries.
    Add {
        #[arg(long)]
        expected_revision: Option<String>,
        #[arg(long)]
        target_id: Option<String>,
        #[arg(long)]
        label: String,
        #[arg(long)]
        expected_repository_identity: Option<String>,
        /// HS2-SHH7WW: add the target as another site of this project target.
        #[arg(long)]
        site_of: Option<String>,
    },
    /// Rename one live target at the expected manifest revision; its id never changes.
    Rename {
        id: String,
        #[arg(long)]
        expected_revision: String,
        #[arg(long)]
        label: String,
        #[arg(long)]
        expected_repository_identity: Option<String>,
        #[arg(long)]
        clear_expected_repository_identity: bool,
        /// HS2-SHH7WW: make the target a site of this project target.
        #[arg(long, conflicts_with = "clear_site_of")]
        site_of: Option<String>,
        /// HS2-SHH7WW: make the target a project of its own again.
        #[arg(long)]
        clear_site_of: bool,
    },
    /// HS2-NAA3HV: make a site the home of its project at the expected manifest
    /// revision; the old home and every other site become its sites, and the policy's
    /// per-target entries move with the home.
    MakeHome {
        id: String,
        #[arg(long)]
        expected_revision: String,
    },
    /// HS2-SHH7WW: each project with its sites and this machine's binding of each.
    Sites {
        /// HS2-D4G81Q: also read what each bound site's machine is (OS, architecture,
        /// CPU, GPU, AI tool and toolchain versions) through the gated capacity read.
        #[arg(long)]
        capabilities: bool,
    },
    /// Tombstone one target at the expected manifest revision; its id is never reused.
    Remove {
        id: String,
        #[arg(long)]
        expected_revision: String,
    },
    /// Public identity JSON only; credentials/origins are owned by remote registrations.
    Bind {
        id: String,
        #[arg(long)]
        expected_manifest_revision: String,
        #[arg(long)]
        expected_binding_revision: u64,
        #[arg(long)]
        destination: String,
        #[arg(long)]
        previous_destination: Option<String>,
    },
    /// Remove this machine's binding at the expected binding revision; idempotent when absent.
    Unbind {
        id: String,
        #[arg(long)]
        expected_binding_revision: u64,
        #[arg(long)]
        previous_destination: Option<String>,
    },
    /// Freshly verify the bound destination and report its explicit target state.
    Resolve { id: String },
    /// Fresh read-confirm-bind candidate metadata; never creates a binding/grant.
    Inspect {
        checkout_id: String,
        #[arg(long)]
        registration_id: Option<String>,
    },
}
fn identity(value: Option<String>) -> Result<Value> {
    match value {
        None => Ok(Value::Null),
        Some(value) => serde_json::from_str(&value).map_err(|_| {
            anyhow::anyhow!("destination must be a public reviewed identity JSON object")
        }),
    }
}
/// HS2-BVGWK6: reading each site's machine passes the target gate, which refuses an
/// unidentified reader. Without an actor every site answered `invalid_request`, which
/// did not say what was missing, so the command refuses up front and names the flags.
fn sites_actor_check(command: &TargetCmd, has_actor: bool) -> Result<()> {
    if matches!(command, TargetCmd::Sites { capabilities: true }) && !has_actor {
        bail!(
            "target sites --capabilities reads each site's machine through the target gate, \
             which needs an identified actor: pass --actor-role <human|ai> --actor-id <id> \
             (or set HOTSHEET_ACTOR_ROLE and HOTSHEET_ACTOR_ID)"
        );
    }
    Ok(())
}
fn route(project: &str, command: TargetCmd) -> Result<(&'static str, String, Value)> {
    let prefix = format!("/checkouts/{}/orchestration/targets", project);
    let target = |id: &str| json!({"owner_checkout_id":project,"target_id":id});
    Ok(match command {
        TargetCmd::List { target_id: None } => ("GET", prefix, Value::Null),
        TargetCmd::List {
            target_id: Some(id),
        } => ("GET", format!("{prefix}/{id}/binding"), Value::Null),
        TargetCmd::Add {
            expected_revision,
            target_id,
            label,
            expected_repository_identity,
            site_of,
        } => (
            "POST",
            prefix,
            json!({"expected_revision":expected_revision,"target_id":target_id,"label":label,"expected_repository_identity":expected_repository_identity,"site_of":site_of}),
        ),
        TargetCmd::Rename {
            id,
            expected_revision,
            label,
            expected_repository_identity,
            clear_expected_repository_identity,
            site_of,
            clear_site_of,
        } => (
            "PATCH",
            format!("{prefix}/{id}"),
            json!({"expected_revision":expected_revision,"label":label,"expected_repository_identity":expected_repository_identity,"clear_expected_repository_identity":clear_expected_repository_identity,"site_of":site_of,"clear_site_of":clear_site_of}),
        ),
        TargetCmd::MakeHome {
            id,
            expected_revision,
        } => (
            "POST",
            format!("{prefix}/{id}/make-home"),
            json!({"expected_revision":expected_revision}),
        ),
        // HS2-D4G81Q: reading the machines passes the gate, so the actor rides in a body.
        TargetCmd::Sites { capabilities } => {
            if capabilities {
                (
                    "POST",
                    format!("/checkouts/{project}/orchestration/sites"),
                    json!({"capabilities": true}),
                )
            } else {
                (
                    "GET",
                    format!("/checkouts/{project}/orchestration/sites"),
                    Value::Null,
                )
            }
        }
        TargetCmd::Remove {
            id,
            expected_revision,
        } => (
            "DELETE",
            format!("{prefix}/{id}"),
            json!({"expected_revision":expected_revision}),
        ),
        TargetCmd::Bind {
            id,
            expected_manifest_revision,
            expected_binding_revision,
            destination,
            previous_destination,
        } => (
            "PUT",
            format!("{prefix}/{id}/binding"),
            json!({"target":target(&id),"expected_manifest_revision":expected_manifest_revision,
                "expected_binding_revision":expected_binding_revision,"destination":identity(Some(destination))?,"previous_destination":identity(previous_destination)?}),
        ),
        TargetCmd::Unbind {
            id,
            expected_binding_revision,
            previous_destination,
        } => (
            "DELETE",
            format!("{prefix}/{id}/binding"),
            json!({"target":target(&id),"expected_binding_revision":expected_binding_revision,"previous_destination":identity(previous_destination)?}),
        ),
        TargetCmd::Resolve { id } => ("GET", format!("{prefix}/{id}/resolve"), Value::Null),
        TargetCmd::Inspect {
            checkout_id,
            registration_id,
        } => (
            "POST",
            format!("/checkouts/{project}/orchestration/inspect"),
            json!({"destination": match registration_id {
                Some(registration_id) => json!({"kind":"remote","registration_id":registration_id,"checkout_id":checkout_id}),
                None => json!({"kind":"local","checkout_id":checkout_id}),
            }}),
        ),
    })
}
pub(crate) fn run(command: Cmd, context: &Ctx) -> Result<()> {
    let Cmd::Orchestration(arguments) = command else {
        super::misrouted("orchestration")
    };
    let required = |project: Option<String>| {
        project.ok_or_else(|| anyhow::anyhow!("--project <registered-checkout-id> is required"))
    };
    // HS2-5JBYPV: a linked CI gate only advances while this machine's Job monitor runs.
    let mut ensure_monitor = false;
    let mut monitor_off = false;
    let (owner_id, (method, path, mut body)) = match arguments.command {
        OrchestrationCmd::StepRunner {
            checkout,
            execution,
        } => {
            let home = hotsheet_plugins::hotsheet_home();
            return hotsheet_cli::orchestration::run_validation_step(&home, &checkout, &execution)
                .map_err(|error| anyhow::anyhow!(error.code()));
        }
        OrchestrationCmd::Capacity(arguments) => match capacity::capacity(arguments)? {
            Planned::Request(owner_id, request) => (owner_id, request),
            Planned::Done => return Ok(()),
        },
        OrchestrationCmd::Admission(arguments) => match capacity::admission(arguments, context)? {
            Planned::Request(owner_id, request) => (owner_id, request),
            Planned::Done => return Ok(()),
        },
        OrchestrationCmd::ProjectType(arguments) => projects::project_type_route(arguments)?,
        OrchestrationCmd::Project(arguments) => projects::project_route(arguments)?,
        OrchestrationCmd::Target(arguments) => {
            let owner_id = required(arguments.project.clone())?;
            sites_actor_check(
                &arguments.command,
                context
                    .actor
                    .as_ref()
                    .is_some_and(|actor| actor.id.is_some()),
            )?;
            let request = route(&owner_id, arguments.command)?;
            (owner_id, request)
        }
        OrchestrationCmd::Ticket(arguments) => {
            let owner_id = required(arguments.project.clone())?;
            let request = ticket_route(&owner_id, arguments.command);
            (owner_id, request)
        }
        OrchestrationCmd::Run(arguments) => {
            let owner_id = required(arguments.project.clone())?;
            let request = run_route(&owner_id, arguments.command);
            (owner_id, request)
        }
        OrchestrationCmd::Session(arguments) => sessions::route(arguments)?,
        OrchestrationCmd::Validation(arguments) => {
            let owner_id = required(arguments.project.clone())?;
            let request = validation::route(&owner_id, arguments)?;
            (owner_id, request)
        }
        OrchestrationCmd::Readiness(arguments) => {
            let owner_id = required(arguments.project.clone())?;
            let request = readiness::route(&owner_id, arguments)?;
            (owner_id, request)
        }
        OrchestrationCmd::Scheduler(arguments) => {
            let owner_id = required(arguments.project.clone())?;
            let request = scheduler::route(&owner_id, arguments)?;
            (owner_id, request)
        }
        OrchestrationCmd::Plan(arguments) => {
            let owner_id = required(arguments.project.clone())?;
            let request = planning::route(&owner_id, arguments)?;
            (owner_id, request)
        }
        OrchestrationCmd::Gate(arguments) => {
            let owner_id = required(arguments.project.clone())?;
            // HS2-VVHVF0: `--no-monitor` also keeps the runtime from starting one.
            monitor_off = matches!(
                &arguments.command,
                GateCmd::Link {
                    no_monitor: true,
                    ..
                }
            );
            let (request, monitor) = gate_route(&owner_id, arguments.command);
            ensure_monitor = monitor;
            (owner_id, request)
        }
        OrchestrationCmd::ToolCapacity(arguments) => {
            let owner_id = required(arguments.project.clone())?;
            let request = tool_capacity_route(&owner_id, arguments.command)?;
            (owner_id, request)
        }
        OrchestrationCmd::Policy(arguments) => {
            let owner_id = required(arguments.project.clone())?;
            let prefix = format!("/checkouts/{owner_id}/orchestration/policy");
            let proposal = |file: Option<String>| -> Result<Value> {
                Ok(json!({
                    "policy": file.as_deref().map(policy_file).transpose()?,
                }))
            };
            let request = match arguments.command {
                PolicyCmd::Show => ("GET", prefix, Value::Null),
                PolicyCmd::Set {
                    file,
                    expected_revision,
                } => (
                    "PUT",
                    prefix,
                    // `expected_revision` is filled in below when it was omitted.
                    json!({"policy": policy_file(&file)?, "expected_revision": expected_revision,
                        "read_revision": expected_revision.is_none()}),
                ),
                PolicyCmd::Validate { file } => {
                    ("POST", format!("{prefix}/validate"), proposal(file)?)
                }
                PolicyCmd::DryRun { file } => {
                    ("POST", format!("{prefix}/dry-run"), proposal(file)?)
                }
                PolicyCmd::RoleSessions {
                    target,
                    roles,
                    auto,
                } => (
                    "PUT",
                    prefix,
                    // Turned into the edited policy below, at the revision read.
                    json!({"role_sessions_edit": {"target_id": target, "roles": roles, "auto": auto}}),
                ),
            };
            (owner_id, request)
        }
        OrchestrationCmd::Permission(arguments) => {
            let owner_id = required(arguments.project.clone())?;
            let request = permission_route(&owner_id, arguments.command)?;
            (owner_id, request)
        }
        OrchestrationCmd::Audit {
            project,
            after,
            limit,
        } => {
            let owner_id = required(project)?;
            let path =
                format!("/checkouts/{owner_id}/orchestration/audit?after={after}&limit={limit}");
            (owner_id, ("GET", path, Value::Null))
        }
        OrchestrationCmd::HostedAudit {
            checkout,
            after,
            limit,
        } => {
            let path = format!(
                "/checkouts/{checkout}/orchestration/hosted-audit?after={after}&limit={limit}"
            );
            (checkout, ("GET", path, Value::Null))
        }
        OrchestrationCmd::Summary {
            project,
            ticket,
            connection_id,
        } => {
            let owner_id = required(project)?;
            let query: Vec<String> = [("ticket", ticket), ("connection_id", connection_id)]
                .into_iter()
                .filter_map(|(key, value)| value.map(|value| format!("{key}={value}")))
                .collect();
            let mut path = format!("/checkouts/{owner_id}/orchestration/summary");
            if !query.is_empty() {
                path = format!("{path}?{}", query.join("&"));
            }
            (owner_id, ("GET", path, Value::Null))
        }
        OrchestrationCmd::Timeline {
            project,
            after,
            wait_seconds,
        } => {
            let owner_id = required(project)?;
            let mut path = format!("/checkouts/{owner_id}/orchestration/timeline");
            if let Some(after) = after {
                let wait_ms = wait_seconds.min(20) * 1000;
                path = format!("{path}?after={after}&wait_ms={wait_ms}");
            }
            (owner_id, ("GET", path, Value::Null))
        }
        OrchestrationCmd::TargetLines {
            project,
            registration_id,
            after,
            wait_seconds,
        } => {
            let checkout_id = required(project)?;
            let mut path = match &registration_id {
                Some(registration) => format!(
                    "/remotes/{registration}/checkouts/{checkout_id}/orchestration/target-lines"
                ),
                None => format!("/checkouts/{checkout_id}/orchestration/target-lines"),
            };
            if let Some(after) = after {
                let wait_ms = wait_seconds.min(20) * 1000;
                path = format!("{path}?after={after}&wait_ms={wait_ms}");
            }
            (checkout_id, ("GET", path, Value::Null))
        }
        OrchestrationCmd::TargetLinesPush { project } => {
            let owner_id = required(project)?;
            let path = format!("/checkouts/{owner_id}/orchestration/target-lines/push");
            (owner_id, ("POST", path, json!({})))
        }
    };
    // HS2-SBQ3T8: authority decisions record who acted; the runtime strips this again
    // before decoding a request body.
    if let (Some(actor), Some(object)) = (&context.actor, body.as_object_mut()) {
        object.insert("actor".into(), serde_json::to_value(actor)?);
    }
    let home = hotsheet_plugins::hotsheet_home();
    // HS2-4J6V46: do not even resolve -C or construct runtime/identity/config owners
    // before this independent cleanup subset has had its chance to return.
    let result = if let Some(result) = request_cleanup_now(&home, method, &path, &body) {
        result
    } else {
        let source = hotsheet_cli::resolve_store_path(context.path.clone(), &context.cwd);
        if !source.is_dir() {
            bail!(
                "host configuration directory is unavailable; pass -C <existing-host-source> (or use list --target-id/unbind for orphan cleanup)"
            );
        }
        let registry = CheckoutRegistry::new(home.join("checkouts.json"));
        // HS2-7QPJ05: the machine's one provider configuration, resolved through the -C
        // source. "Configured" means it holds a connection; an unreadable one still
        // counts, so its error is reported below.
        let config = ProviderConfigRegistry::for_store_in(&home, &source)?;
        let configured = config.path().is_file()
            && config
                .load()
                .map_or(true, |connections| !connections.is_empty());
        // HS2-3D8D6V: a remote checkout is not registered here; its read uses no
        // provider configuration either.
        let remote = path.starts_with("/remotes/");
        let owner = if remote {
            None
        } else {
            Some(registry.describe(&owner_id).map_err(|_| anyhow::anyhow!("registered project is unavailable; use its checkout id or list --target-id/unbind for orphan cleanup"))?)
        };
        if owner.is_some_and(|owner| owner.sources.iter().any(|source| source.provider != "git"))
            && !configured
        {
            bail!(
                "provider configuration is unavailable; pass -C <configured-host-source> whose provider connections this machine holds; no source was created"
            );
        }
        OrchestrationRuntime::live(&home, registry, config).and_then(|runtime| {
            let runtime = if monitor_off {
                runtime.with_gate_provider(std::sync::Arc::new(
                    hotsheet_cli::orchestration::GithubActionsGates::new(&home),
                ))
            } else {
                runtime
            };
            // HS2-TK9ADC: one target's role sessions, edited into the stored policy.
            if let Some(edit) = body
                .as_object_mut()
                .and_then(|object| object.remove("role_sessions_edit"))
            {
                let mut stored = runtime.request_now("GET", &path, &Value::Null)?;
                // The actor this command attached stays with the request.
                body["policy"] = with_role_sessions(stored["policy"].take(), &edit);
                body["expected_revision"] = stored["revision"].take();
                let mut answer = runtime.request_now(method, &path, &body)?;
                // HS2-SYZB33: say when the entry names a site that is not its
                // project's home, where it starts nothing.
                let checked =
                    runtime.request_now("POST", &format!("{path}/validate"), &json!({}))?;
                if let Some(warnings) = checked.get("warnings") {
                    answer["warnings"] = warnings.clone();
                }
                return Ok(answer);
            }
            // HS2-YCJDEB: `policy set` without a revision is read-then-set, so a
            // repeated setup run is a no-op and a concurrent edit still conflicts.
            if body.get("read_revision").is_some() {
                let read = body["read_revision"] == true;
                if let Some(object) = body.as_object_mut() {
                    object.remove("read_revision");
                }
                if read {
                    body["expected_revision"] =
                        runtime.request_now("GET", &path, &Value::Null)?["revision"].take();
                }
            }
            // HS2-WN0MXZ: a validation run waits for its attempt unless told not to.
            if let Some(result) = validation::until_finished(&runtime, &path, &mut body) {
                return result;
            }
            runtime.request_now(method, &path, &body)
        })
    };
    let mut output = result.map_err(|error| anyhow::anyhow!("{}: {}", error.code(), error))?;
    if ensure_monitor && output["watching"] == false {
        match super::jobs::ensure_default_monitor(&home) {
            Ok(started) => {
                output["watching"] = true.into();
                output["monitor_started"] = started.into();
            }
            // The link stands; the gate stays pending until something observes it.
            Err(_) => output["monitor_error"] = "job_monitor_unavailable".into(),
        }
    }
    println!("{}", serde_json::to_string_pretty(&output)?);
    // HS2-SYZB33: a policy entry that has no effect is said on stderr too.
    for warning in output["warnings"].as_array().into_iter().flatten() {
        eprintln!("{}", policy_warning_line(warning));
    }
    // HS2-YYNW9F: checking a profile file fails the command when it has problems.
    if (path.ends_with("/target-validation-profiles") || path.ends_with("/validations/profiles"))
        && output["valid"] == false
    {
        bail!("validation_profiles_invalid: the profile file has problems; see `problems`");
    }
    Ok(())
}
