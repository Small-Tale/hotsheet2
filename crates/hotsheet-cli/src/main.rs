//! `hotsheet` — the Hot Sheet 2 command-line interface. A thin binary over
//! `hotsheet-ticketing`: it reads and writes ticket files directly on disk
//! (`docs/04-core-server-cli.md` §4.4) and imports HS1 exports (`docs/07`).

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

mod dispatch;

use dispatch::providers::{
    AccountCmd, FeedbackSynthesisCmd, GithubConnectArgs, KeyCmd, ProviderConnectArgs,
};
use dispatch::server::{CertCmd, CommandsCmd, LifecycleCmd, NotificationsCmd, TerminalCmd};
use dispatch::setup::{AiSettingsCmd, PluginCmd, SettingsCmd, TriggerArgs, WorkArgs};
use dispatch::store::CheckoutCmd;
use dispatch::tickets::{AttachArgs, LsFilters, NewArgs, PeopleCmd, TicketEditArgs};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use hotsheet_model::{
    CloseReason, Confidence, NoteKind, Priority, ReviewKind, Status, Ticket, Timestamp, Ulid,
};
use hotsheet_ticketing::{FsStore, TicketQuery};
use time::OffsetDateTime;

mod feedback_synthesis;

#[derive(Parser)]
#[command(name = "hotsheet", version, about = "Hot Sheet 2 CLI")]
struct Cli {
    /// Store directory (defaults to the current directory).
    #[arg(short = 'C', long = "path", global = true, default_value = ".")]
    path: PathBuf,
    /// Who is acting: human | ai | system (HS2-RD4M29); falls back to
    /// `HOTSHEET_ACTOR_ROLE`. Unset means unspecified, never assumed AI. An `ai` actor must
    /// score a completion (`--note-confidence`).
    #[arg(long, global = true, value_name = "ROLE")]
    actor_role: Option<String>,
    /// Stable id of the acting worker, account, or tool (with --actor-role); falls back to
    /// `HOTSHEET_ACTOR_ID`.
    #[arg(long, global = true, value_name = "ID")]
    actor_id: Option<String>,
    #[command(subcommand)]
    command: Cmd,
}

impl Cli {
    /// The acting role, if one was given (flag or `HOTSHEET_ACTOR_ROLE`).
    fn actor(&self) -> Result<Option<hotsheet_ticketing::actor::MutationActor>> {
        let env = |name: &str| std::env::var(name).ok();
        let role = self
            .actor_role
            .clone()
            .or_else(|| env("HOTSHEET_ACTOR_ROLE"));
        let id = self.actor_id.clone().or_else(|| env("HOTSHEET_ACTOR_ID"));
        match role.as_deref().map(str::trim) {
            None | Some("") => {
                if id.as_deref().is_some_and(|id| !id.trim().is_empty()) {
                    bail!("--actor-id requires --actor-role (human, ai, or system)");
                }
                Ok(None)
            }
            Some(role) => hotsheet_ticketing::actor::MutationActor::parse(role, id)
                .map(Some)
                .map_err(anyhow::Error::msg),
        }
    }
}

#[derive(Subcommand)]
enum Cmd {
    /// Initialize a git-backed store here, or create and link a standalone one.
    Init {
        /// Display prefix for ticket slugs (e.g. HS → HS-7F3K9Q).
        #[arg(long, default_value = "HS")]
        prefix: String,
        /// Create a separate git-backed ticket store and link the current project to it.
        #[arg(long)]
        standalone: bool,
        /// Standalone store destination (default: ${HOTSHEET_HOME}/stores/<project>).
        #[arg(long, value_name = "PATH", requires = "standalone")]
        at: Option<PathBuf>,
        /// Configure this URL/path as the standalone store's origin remote.
        #[arg(long, value_name = "URL", requires = "standalone")]
        remote: Option<String>,
    },
    /// Idempotently prepare a code project and standalone HS2 store for headless AI work.
    Bootstrap {
        /// Code project to prepare (defaults to the current directory).
        #[arg(long, default_value = ".")]
        project: PathBuf,
        /// Standalone ticket store (defaults to a sibling `<project>.hs2` directory).
        #[arg(long, value_name = "PATH")]
        store: Option<PathBuf>,
        /// Display prefix for newly created ticket slugs.
        #[arg(long, default_value = "HS2")]
        prefix: String,
        /// Configure this existing Git remote as `origin` without replacing another URL.
        #[arg(long, value_name = "URL")]
        remote: Option<String>,
        /// Set up this AI tool even when it is not detected (repeatable).
        #[arg(long = "tool")]
        tools: Vec<String>,
    },
    /// Link this directory (a code repo) to its **standalone** ticket store, so later
    /// `hotsheet-cli` calls here find it without `-C` (docs/02 §2.8, HS2-5CXKZ0). Writes a
    /// gitignored `.hotsheet2/store` pointing at the store's absolute path.
    Link {
        /// Path to the existing standalone ticket store.
        store: PathBuf,
    },
    /// Explicitly activate the current pre-release store/ticket format.
    ActivateFormat {
        /// Confirm that older pre-release HS2 processes have been stopped and the
        /// compatibility break has been announced.
        #[arg(long)]
        acknowledge_pre_release_breakage: bool,
    },
    /// Report the formats this CLI creates and can open (for headless compatibility checks).
    Compatibility {
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Create a new ticket.
    New(NewArgs),
    /// List / query tickets with optional filters and sort.
    Ls {
        #[command(flatten)]
        filters: LsFilters,
    },
    /// Show the current ticket-provider connection and its capabilities.
    Providers {
        /// Emit the full provider descriptor as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Sign in with the Hot Sheet GitHub App using GitHub's device code flow.
    GithubSignIn {
        /// GitHub.com or the web origin of a configured GitHub Enterprise instance.
        #[arg(long, default_value = "https://github.com")]
        web_base: String,
    },
    /// Create or update a GitHub Issues connection in this ticket store, safely repeatable.
    GithubConnect(GithubConnectArgs),
    /// Create or update a GitLab or Jira ticket source, safely repeatable: the same validated
    /// workflow as the client form, optionally reusing a signed-in `--account`.
    ProviderConnect(ProviderConnectArgs),
    /// Attach files to a provider-native ticket through its connection (for GitHub, its
    /// configured assets repository). Fails explicitly when the provider cannot.
    ProviderAttach {
        connection: String,
        id: String,
        #[arg(required = true)]
        files: Vec<PathBuf>,
    },
    /// List tickets from one configured provider connection.
    ProviderLs { connection: String },
    /// Get one provider-native ticket.
    ProviderGet { connection: String, id: String },
    /// Prepare a private AI feedback draft or mark one human-reviewed; never publishes guidance.
    FeedbackSynthesis {
        #[command(subcommand)]
        command: FeedbackSynthesisCmd,
    },
    /// Temporarily disable an external provider connection: Hot Sheet stops reading from and
    /// writing to it, and its tickets are hidden until it is enabled again.
    ProviderDisable { connection: String },
    /// Re-enable a disabled provider connection.
    ProviderEnable { connection: String },
    /// Permanently remove an external provider connection and every local reference to it:
    /// checkout links and defaults, its `providers.json` entry, and a credential Hot Sheet
    /// minted for it (user-managed keys are kept). Safe to repeat.
    ProviderRemove {
        connection: String,
        /// Emit the removal report as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Create a ticket directly in a configured provider.
    ProviderNew {
        connection: String,
        title: String,
        #[arg(long, default_value = "issue")]
        category: String,
        #[arg(long, default_value = "default")]
        priority: String,
        #[arg(long)]
        details: Option<String>,
        #[arg(long = "tag")]
        tags: Vec<String>,
    },
    /// Update a provider-native ticket with optional optimistic concurrency.
    ProviderEdit {
        connection: String,
        id: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        details: Option<String>,
        #[arg(long)]
        status: Option<String>,
        #[arg(long)]
        expected_token: Option<String>,
        #[arg(long)]
        note: Option<String>,
        /// Read the note body from a UTF-8 file, or from stdin with `-`.
        #[arg(long, value_name = "PATH", conflicts_with = "note")]
        note_file: Option<PathBuf>,
        /// Permit literal `\\n` text outside Markdown code spans/blocks.
        #[arg(long, requires = "note")]
        allow_literal_backslash_n: bool,
        /// Edit this provider-native note id instead of appending a note.
        #[arg(long, conflicts_with = "note_kind")]
        edit_note: Option<String>,
        /// Kind for --note: regular | activity | feedback_needed | status.
        #[arg(long)]
        note_kind: Option<String>,
        /// Concise plain-text timeline headline for an activity/status note.
        #[arg(long, conflicts_with = "edit_note")]
        note_summary: Option<String>,
        /// Provider-native feedback request note id answered by this note.
        #[arg(long, conflicts_with = "edit_note")]
        response_to: Option<String>,
        /// AI completion confidence (integer 0-100) recorded on the appended note, or the
        /// corrected score of the note named by --edit-note (note text then optional).
        #[arg(long, value_name = "0-100", value_parser = parse_confidence_arg)]
        note_confidence: Option<Confidence>,
        /// Remove the completion confidence from the note named by --edit-note.
        #[arg(long, requires = "edit_note", conflicts_with = "note_confidence")]
        clear_note_confidence: bool,
    },
    /// Close a provider-native ticket.
    ProviderClose {
        connection: String,
        id: String,
        #[arg(long)]
        reason: String,
    },
    /// Replace provider assignees and/or request reviews when supported.
    ProviderAssign {
        connection: String,
        id: String,
        #[arg(long = "to", conflicts_with = "clear")]
        to: Vec<String>,
        #[arg(long)]
        clear: bool,
        #[arg(long = "review")]
        reviews: Vec<String>,
    },
    /// Restore a ticket in a git-backed provider's Trash.
    ProviderRestore { connection: String, id: String },
    /// Atomically reopen a Not Working ticket with a note and/or evidence files.
    ProviderReportNotWorking {
        connection: String,
        id: String,
        #[arg(long, conflicts_with = "note_file")]
        note: Option<String>,
        #[arg(long, conflicts_with = "note")]
        note_file: Option<PathBuf>,
        #[arg(long = "evidence")]
        evidence: Vec<PathBuf>,
        #[arg(long)]
        expected_token: Option<String>,
    },
    /// Print a ticket's file by slug or ULID.
    Show { id: String },
    /// Apply one update independently to several git-backed tickets.
    Batch {
        /// Ticket slugs or ULIDs; failures are reported per ticket.
        #[arg(required = true)]
        ids: Vec<String>,
        #[command(flatten)]
        update: TicketEditArgs,
    },
    /// Delete one note from a git-backed ticket.
    DeleteNote { id: String, note_id: String },
    /// Delete a provider-native note when that connection supports note deletion.
    ProviderDeleteNote {
        connection: String,
        id: String,
        note_id: String,
    },
    /// Attach one or more files as a single durable batch.
    Attach(AttachArgs),
    /// Rename a git-backed attachment by stable ULID.
    AttachmentRename {
        id: String,
        attachment: String,
        filename: String,
    },
    /// Delete a git-backed attachment and its payload by stable ULID.
    AttachmentDelete { id: String, attachment: String },
    /// Replace batch, actor, and purpose metadata for one or more attachment ULIDs.
    AttachmentMetadata {
        id: String,
        #[arg(long = "attachment", required = true)]
        attachments: Vec<String>,
        /// JSON metadata object (`{}` clears optional fields); `-` reads stdin.
        #[arg(long)]
        file: PathBuf,
    },
    /// Replace one attachment's media annotations from JSON (object or bare array).
    Annotate {
        id: String,
        attachment: String,
        #[arg(long, conflicts_with = "clear", required_unless_present = "clear")]
        file: Option<PathBuf>,
        #[arg(long, conflicts_with = "file")]
        clear: bool,
        #[arg(long)]
        json: bool,
    },
    /// Correct the actor provenance of one or more existing attachments.
    AttachmentActor {
        id: String,
        #[arg(required = true)]
        attachment_ids: Vec<String>,
        #[arg(long)]
        actor_role: String,
        #[arg(long)]
        actor_id: Option<String>,
        #[arg(long)]
        actor_name: Option<String>,
    },
    /// Edit a ticket's fields (by slug or ULID).
    Edit {
        id: String,
        #[command(flatten)]
        update: TicketEditArgs,
    },
    /// Record why a ticket was closed (close outcome; orthogonal to status).
    Close {
        id: String,
        /// completed | not_planned | duplicate | obsolete | works_as_designed.
        #[arg(long)]
        reason: String,
        /// The duplicate target (slug or ULID); required when reason=duplicate.
        #[arg(long)]
        duplicate_of: Option<String>,
    },
    /// Restore a ticket from Trash to the status it held before it was deleted.
    Restore { id: String },
    /// Permanently remove Trash tickets deleted at least this many days ago (the server
    /// runs the same sweep automatically). Git history keeps every purged file.
    PurgeTrash {
        /// Override the project's shared trash_cleanup_days setting for this run.
        #[arg(long)]
        older_than_days: Option<u32>,
        /// List how many (and which) Trash tickets would be purged, without removing any
        /// (headless parity with the Lifecycle retention purge preview, HS2-1APM9K).
        #[arg(long)]
        dry_run: bool,
    },
    /// Set up an AI tool to work with this project, headless (writes its instruction
    /// section, worklist skill, and MCP config). No server or client required.
    Setup {
        /// The tool to set up (e.g. `claude`). Omit together with --detect.
        tool: Option<String>,
        /// Set up every AI tool detected on this machine.
        #[arg(long, conflicts_with = "refresh")]
        detect: bool,
        /// Migrate settings and refresh every detected or previously managed tool.
        #[arg(long, conflicts_with = "tool")]
        refresh: bool,
        /// Explain setup and connection problems without changing any files.
        #[arg(long, conflicts_with_all = ["refresh", "detect", "tool"])]
        diagnose: bool,
        /// Limit --diagnose to one plugin id (for example, codex).
        #[arg(long = "tool", requires = "diagnose")]
        diagnose_tool: Option<String>,
        /// Select the project instruction writer; does not rewrite instruction content.
        #[arg(long, value_parser = ["builtin", "rulesync"], conflicts_with = "diagnose")]
        guidance_owner: Option<String>,
        /// Project directory to write the tool config into (defaults to the store path).
        #[arg(long)]
        project: Option<PathBuf>,
        /// Print the structured setup report, including user-action notices.
        #[arg(long)]
        json: bool,
    },
    /// Register pinned rule/skill sources and choose project guidance, headlessly.
    Guidance {
        /// Code project root; independent of its ticket providers.
        #[arg(long)]
        project: PathBuf,
        #[command(subcommand)]
        cmd: GuidanceCmd,
    },
    /// Manage AI-tool plugins (list built-in + installed; install/remove external ones).
    Plugin {
        #[command(subcommand)]
        cmd: PluginCmd,
    },
    /// List detected, drivable AI tools and their plugin-declared model choices.
    AiTools {
        #[arg(long)]
        json: bool,
    },
    /// Read or update the project's default AI tool/model/effort (HS2-SW5S13).
    AiSettings {
        #[command(subcommand)]
        cmd: AiSettingsCmd,
    },
    /// Read/write core-owned project settings (shared = committed, local = gitignored).
    Settings {
        #[command(subcommand)]
        cmd: SettingsCmd,
    },
    /// Inspect and run saved commands in a registered checkout through the local server.
    Commands {
        #[command(subcommand)]
        cmd: CommandsCmd,
    },
    /// Send a message to a live terminal AI session through the local server.
    Terminal {
        #[command(subcommand)]
        cmd: TerminalCmd,
    },
    /// Inspect or request a safe restart of the store's running local server.
    Lifecycle {
        #[command(subcommand)]
        cmd: LifecycleCmd,
    },
    /// Read the selected store's current ticket-flow summary from the local server.
    TicketFlow,
    /// Read the local server's recent activity timeline.
    Activity {
        #[arg(long)]
        ticket: Option<String>,
        #[arg(long)]
        session: Option<String>,
        #[arg(long, value_parser = ["low", "normal", "high"])]
        min_importance: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Inspect and acknowledge the local server's notification feed.
    Notifications {
        #[command(subcommand)]
        cmd: NotificationsCmd,
    },
    /// Manage global provider API keys in the OS credential store (values never enter settings).
    Key {
        #[command(subcommand)]
        cmd: KeyCmd,
    },
    /// Register and discover code checkouts independently of ticket stores.
    Checkout {
        #[command(subcommand)]
        cmd: CheckoutCmd,
    },
    /// Machine-wide provider sign-ins (GitHub, GitLab, Jira) and the projects using each.
    /// Ticket sources themselves belong to projects; see `checkout` (HS2-SM9PM8).
    Account {
        #[command(subcommand)]
        cmd: AccountCmd,
    },
    /// Import an HS1 `hotsheet-export.json` into the store (creates it if needed).
    Import {
        file: PathBuf,
        /// Prefix used if the store must be created first.
        #[arg(long, default_value = "HS")]
        prefix: String,
        /// Read-only JSON comparison of exported attachment identities and payloads.
        #[arg(long, conflicts_with_all = ["restore_attachment", "confirm_omission"])]
        diagnose_attachments: bool,
        /// Restore only this diagnostic selection (TICKET_ULID/ATTACHMENT_ULID); repeatable.
        #[arg(long, value_name = "SELECTION")]
        restore_attachment: Vec<String>,
        /// Confirm an intentionally deleted attachment should stay absent; repeatable.
        #[arg(long, value_name = "SELECTION")]
        confirm_omission: Vec<String>,
    },
    /// Copy a ticket into another store as a new ticket (new ULID; original untouched).
    Copy {
        /// Ticket to copy (slug or ULID) from the `-C` store.
        id: String,
        /// Destination store directory.
        #[arg(long = "to")]
        to: PathBuf,
    },
    /// Idempotently copy a ticket between provider connections (git stores initially).
    ProviderCopy {
        id: String,
        #[arg(long = "to")]
        to: PathBuf,
        /// Stable caller-generated id; retries with the same id resolve one destination.
        #[arg(long = "operation-id")]
        operation_id: String,
    },
    /// Move a ticket to another store, keeping its ULID (leaves a tombstone in the source).
    /// git never forgets — this does NOT purge it from the source's history/remote.
    Move {
        /// Ticket to move (slug or ULID) from the `-C` store.
        id: String,
        /// Destination store directory.
        #[arg(long = "to")]
        to: PathBuf,
        /// Confirm the move despite the retention/exposure caveat (required).
        #[arg(long)]
        yes: bool,
    },
    /// Idempotently copy then close a ticket across provider connections.
    ProviderMove {
        id: String,
        #[arg(long = "to")]
        to: PathBuf,
        #[arg(long = "operation-id")]
        operation_id: String,
        #[arg(long)]
        yes: bool,
    },
    /// Assign people to a ticket and/or request their involvement (docs/10 §10.2). Person
    /// identity is the git email; `hotsheet people` maps it to a name.
    Assign {
        id: String,
        /// A person expected to do the work (git email), repeatable — replaces the set.
        #[arg(long = "to")]
        to: Vec<String>,
        /// Clear all assignees.
        #[arg(long)]
        clear: bool,
        /// Request a person's involvement as `email:kind` (kind = feedback|review|fyi|work),
        /// repeatable. Soft (attention only) — use --blocked-by for hard ordering.
        #[arg(long = "review")]
        review: Vec<String>,
    },
    /// Manage the committed `people.json` roster (git email → friendly name).
    People {
        #[command(subcommand)]
        cmd: PeopleCmd,
    },
    /// Mark a ticket read on this machine (per-user local state; never committed).
    Read { id: String },
    /// Sync the store with its git remote now: fetch, integrate incoming changes through
    /// the semantic merge driver (rebase), and push local commits. Normally automatic — this
    /// is the explicit "sync now" (docs/02 §2.12).
    Sync,
    /// Check store health and print read-only first-run/setup/migration guidance.
    Doctor {
        /// Code-project directory to inspect for installed-tool setup and HS1 data.
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
    /// Rebuild the on-disk index (SQLite/FTS) from a full store walk. The index is a
    /// disposable cache, so this is always safe — use it after an external edit or if the
    /// index looks stale. Writes to the same path the server reads (docs/03 §3.4), then
    /// prunes index files older schema generations left that no running process holds.
    Reindex {
        /// Index database file (default: ${HOTSHEET_HOME}/index/<project-id>.v<schema>.sqlite).
        #[arg(long)]
        index: Option<PathBuf>,
    },
    /// Compare reported completion confidence with later outcomes (reopened vs. verified)
    /// per rubric band, to calibrate the bands over time (HS2-RD4M29).
    ConfidenceReport {
        /// Print the full report (bands and every completion event) as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Time per started_phase over completed tickets: per-ticket wall time and active
    /// (live-claim) time, with medians and percentiles per phase (HS2-JS7E61).
    PhaseReport {
        /// Only tickets completed at or after this day (YYYY-MM-DD, UTC) or RFC 3339 instant.
        #[arg(long)]
        since: Option<String>,
        /// Only tickets completed before this day (YYYY-MM-DD, UTC) or RFC 3339 instant.
        #[arg(long)]
        until: Option<String>,
        /// Print the full report, including every ticket's per-phase times, as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Show the usage/cost metrics rollup for this store (docs/14) — total cost + tokens,
    /// by model, and by day. DB-free: settled rollup files + a live scan of the raw tail.
    Metrics {
        /// Capture new numeric usage from this exact Codex Work session and worker.
        #[arg(long)]
        refresh_codex: bool,
        /// Settle history through this day (YYYY-MM-DD) into the per-contributor rollup
        /// file, advancing `last_rolled_up_through`, before reporting (docs/14 §14.3).
        #[arg(long, value_name = "YYYY-MM-DD")]
        roll_up: Option<String>,
        /// Retention: delete raw JSONL files whose day is before this (only what's already
        /// rolled up is removed). Runs after `--roll-up`.
        #[arg(long, value_name = "YYYY-MM-DD")]
        prune_before: Option<String>,
        /// Sum across every contributor's shared rollup file (a team view), not just this
        /// contributor (docs/14 §14.4).
        #[arg(long)]
        team: bool,
    },
    /// Review or explicitly copy legacy Git-store permission rules into one registered project.
    PermissionMigrate {
        /// Registered checkout id, alias, or root path.
        #[arg(long)]
        project: String,
        /// Linked Git ticket-source connection id to review.
        #[arg(long)]
        source: String,
        /// Candidate id from the preview; repeat to select several rules.
        #[arg(long = "id")]
        ids: Vec<String>,
        /// Confirm the selected rules' broader project scope.
        #[arg(long)]
        approve: bool,
    },
    /// Native lifecycle **permission hook** (docs/05 §5.7): reads PermissionRequest JSON
    /// from Claude or Codex, finds the current project server (falling back to
    /// $HOTSHEET_SERVER/$HOTSHEET_SECRET), and writes the provider-native decision. With no
    /// server or on transport failure it emits nothing, preserving the tool's normal prompt.
    PermissionHook {
        /// Provider that installed this hook; identifies the asking agent in the popup.
        #[arg(long)]
        agent: Option<String>,
    },
    /// Diagnose the terminal hook route separately from MCP, without changing connection state.
    HookDiagnose {
        #[arg(long)]
        json: bool,
    },
    /// Launch an interactive AI tool in this terminal with permission requests routed to
    /// the running Hot Sheet server. The ticket store is resolved from checkout sources,
    /// a `.hotsheet2/store` link (with legacy `.hotsheet/store` fallback), or conservative sibling discovery.
    Launch {
        /// The hook-capable tool to launch (currently `claude` or `codex`).
        tool: String,
        /// Project directory in which to run the tool (defaults to the current directory).
        #[arg(long)]
        project: Option<PathBuf>,
        /// Explicit existing git ticket store for deterministic non-interactive onboarding.
        #[arg(long, value_name = "PATH")]
        ticket_store: Option<PathBuf>,
        /// Create and associate this standalone git ticket store when none is linked.
        #[arg(long, value_name = "PATH", conflicts_with = "ticket_store")]
        create_ticket_store: Option<PathBuf>,
        /// Additional arguments passed to the tool after `--`.
        #[arg(last = true)]
        args: Vec<String>,
    },
    /// Regenerate the derived `worklist.md` at the store root from the current tickets
    /// (the file-based worklist any AI tool can read without the API; docs/03 §3.6). The
    /// server does this automatically on change — this is the headless "regenerate now".
    Worklist,
    /// Run the Hot Sheet server for this store in the foreground (execs the sibling
    /// `hotsheet-server` binary). Detached/supervised start is client-owned (HS2-4072GM).
    Serve {
        /// Address to bind. Loopback = Tier 0 (plaintext + shared secret); an off-loopback
        /// address serves over mTLS and needs `cert init` first (HS2-VT3JMF). Port 0 = ephemeral.
        #[arg(long, default_value = "127.0.0.1:8787")]
        bind: String,
        /// Shared secret for `X-Hotsheet-Secret` (generated + printed by the server if omitted).
        #[arg(long, conflicts_with = "list")]
        secret: Option<String>,
        /// Stop the running server for this store, then exit.
        #[arg(long, conflicts_with = "list")]
        stop: bool,
        /// With --stop, also kill every terminal retained for this project.
        #[arg(long, requires = "stop", conflicts_with = "list")]
        kill_all_terminals: bool,
        /// With --stop, also stop a server owned by the pinned production host
        /// (`local-host-snapshot`), which an ordinary stop refuses (HS2-9AWEX5).
        #[arg(long, requires = "stop", conflicts_with = "list")]
        force: bool,
        /// List every registered machine server and its hosted stores. Registrations are
        /// deduplicated by server identity, health-checked, and never expose bearer secrets.
        #[arg(long, conflicts_with = "stop")]
        list: bool,
    },
    /// Manage this project's Tier-1 mTLS material — a per-project CA + per-device client
    /// certs — so the server can bind off-loopback securely (HS2-VT3JMF).
    Cert {
        #[command(subcommand)]
        cmd: CertCmd,
    },
    /// Git-invoked semantic 3-way merge for ticket files (not run by hand — it's the
    /// `merge=hotsheet-ticket` driver `hotsheet init` registers). Args are git's
    /// %O (base) %A (ours/output) %B (theirs). Exit 0 = clean, 1 = body conflict.
    #[command(hide = true)]
    MergeDriver {
        base: PathBuf,
        ours: PathBuf,
        theirs: PathBuf,
    },
    /// Claim the next available ticket for a worker (local lease).
    ClaimNext {
        /// Worker id recorded on the claim.
        #[arg(long, default_value = "worker")]
        worker: String,
        /// Human-readable worker label.
        #[arg(long)]
        label: Option<String>,
        /// Lease length in minutes.
        #[arg(long, default_value_t = 30)]
        lease_minutes: i64,
        /// Estimated completion time for a non-trivial ticket: a duration from now (`45m`,
        /// `2h`, `1h30m`) or an RFC 3339 timestamp (HS2-DQQ0AX).
        #[arg(long)]
        eta: Option<String>,
    },
    /// Claim one exact open, unblocked ticket by slug or ULID (local lease).
    Claim {
        id: String,
        /// Worker id recorded on the claim.
        #[arg(long, default_value = "worker")]
        worker: String,
        /// Human-readable worker label.
        #[arg(long)]
        label: Option<String>,
        /// Lease length in minutes.
        #[arg(long, default_value_t = 30)]
        lease_minutes: i64,
        /// Compatibility flag; exact claims now always start Not Started tickets.
        #[arg(long)]
        start: bool,
        /// Estimated completion time for a non-trivial ticket: a duration from now (`45m`,
        /// `2h`, `1h30m`) or an RFC 3339 timestamp (HS2-DQQ0AX).
        #[arg(long)]
        eta: Option<String>,
    },
    /// Release a claim (only the holding worker, unless --force), or with `--all` every claim
    /// the worker holds.
    Release {
        #[arg(required_unless_present = "all")]
        id: Option<String>,
        #[arg(long, default_value = "worker")]
        worker: String,
        #[arg(long)]
        force: bool,
        /// Release every claim held by `--worker` (a launcher's end-of-session safety net).
        #[arg(long, conflicts_with_all = ["id", "force"])]
        all: bool,
    },
    /// Renew a claim's lease (must be the holding worker).
    Renew {
        id: String,
        #[arg(long, default_value = "worker")]
        worker: String,
        #[arg(long, default_value_t = 30)]
        lease_minutes: i64,
        /// A new estimated completion time, replacing the current one (HS2-DQQ0AX). Without it the
        /// estimate is kept, and renew warns once it has passed.
        #[arg(long)]
        eta: Option<String>,
    },
    /// Drive a real AI tool for this project (the headless "play"): launch/inject a turn
    /// and stream it. Applies HS2-103 launch safety. No server or client required.
    Trigger(TriggerArgs),
    /// Work the Up Next queue headlessly: drive the tool one turn at a time until Up Next
    /// is drained (or `--max` turns / a thrash stall). Applies HS2-103 launch safety.
    Work(WorkArgs),
}

#[derive(Subcommand, Debug)]
enum GuidanceCmd {
    /// Read local project evidence and pinned guidance recommendations as JSON.
    Recommendations,
    /// Preview the exact bounded context and questions for an optional AI interview.
    RecommendInterviewPreview,
    /// Record consent to share the previewed context with an AI tool of your choice.
    RecommendInterviewOpen {
        #[arg(long)]
        fingerprint: String,
        #[arg(long)]
        consent: bool,
    },
    /// Record bounded advisory JSON returned by the chosen AI tool.
    RecommendInterviewSubmit {
        #[arg(long)]
        fingerprint: String,
        #[arg(long)]
        answer_file: PathBuf,
    },
    /// Accept or dismiss one exact recommendation; accepting selects but does not apply it.
    RecommendDecide {
        #[arg(long)]
        source: String,
        #[arg(long, value_parser = ["rule", "skill"])]
        kind: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        fingerprint: String,
        #[arg(long, value_parser = ["accept", "dismiss"])]
        decision: String,
    },
    /// List installed and available rules/skills from registered sources.
    List {
        #[arg(long)]
        query: Option<String>,
        #[arg(long)]
        owner: Option<String>,
        #[arg(long)]
        tag: Option<String>,
    },
    /// Check registered source refs without changing their pinned commits.
    CheckUpdates,
    /// Review selected rule and skill changes at a source's observed revision without writing.
    Review {
        #[arg(long)]
        source: String,
    },
    /// Check standalone Rulesync lock freshness with an installed Rulesync 38 executable.
    CheckRulesyncUpdates,
    /// Resolve a source tag or commit and cache its verified manifest for offline refresh.
    Register {
        #[arg(long, default_value = "ai-playbook")]
        id: String,
        #[arg(long, default_value = hotsheet_plugins::guidance_sources::DEFAULT_REPOSITORY)]
        repository: String,
        #[arg(long = "ref", default_value = "main")]
        requested_ref: String,
        /// Only register if the ref still resolves to this reviewed full commit.
        #[arg(long)]
        expected_commit: Option<String>,
    },
    /// Replace this source's selected rules and skills; repeat each flag as needed.
    Select {
        #[arg(long)]
        source: String,
        #[arg(long = "rule")]
        rules: Vec<String>,
        #[arg(long = "skill")]
        skills: Vec<String>,
    },
    /// Apply selected guidance through the project's chosen writer.
    Apply,
    /// Select the built-in or Rulesync instruction writer for this project.
    Owner {
        #[arg(value_parser = ["builtin", "rulesync"])]
        owner: String,
    },
}

fn main() -> Result<()> {
    // `| head` closing our stdout is a normal end of output, not a panic (HS2-4BT3VF).
    hotsheet_cli::broken_pipe::install_broken_pipe_exit();
    if hotsheet_cli::print_revision_status_if_requested() {
        return Ok(());
    }
    let mut cli = Cli::parse();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    // Resolve which store to operate on: an explicit -C, else $HOTSHEET_STORE, else a
    // `.hotsheet2/store` link walked up from cwd — so a standalone store is found without -C
    // (HS2-5CXKZ0). `init`/`link` operate on the literal path, not a resolved one.
    if cli.command.resolves_store_path() {
        cli.path = hotsheet_cli::resolve_store_path(cli.path, &cwd);
    }
    let refresh = cli.command.refreshes_worklists();
    let ctx = dispatch::Ctx {
        actor: cli.actor()?,
        path: cli.path,
        cwd,
    };
    let result = dispatch::run(cli.command, &ctx);
    if result.is_ok() && refresh {
        refresh_checkout_worklists(&ctx.path, &ctx.cwd)?;
    }
    result
}

fn parse_review_kind(s: &str) -> Result<ReviewKind> {
    Ok(match s {
        "work" => ReviewKind::Work,
        "feedback" => ReviewKind::Feedback,
        "review" => ReviewKind::Review,
        "fyi" => ReviewKind::Fyi,
        other => bail!("invalid review kind '{other}' (work|feedback|review|fyi)"),
    })
}

/// Default index DB path for a store — mirrors the server's `default_index_path` so the
/// CLI and server share one index file (`${HOTSHEET_HOME}/index/<project-id>.v<schema>.sqlite`).
fn default_index_path(store: &FsStore) -> Result<PathBuf> {
    let root = store
        .root()
        .canonicalize()
        .unwrap_or_else(|_| store.root().to_path_buf());
    let id = &hotsheet_index::hash_bytes(root.to_string_lossy().as_bytes())[..16];
    let dir = hotsheet_plugins::hotsheet_home().join("index");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join(hotsheet_index::index_file_name(id)))
}

/// Percent-encode a terminal id for one URL path segment.
fn urlencoding_component(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

/// Refresh every registered checkout backed by `store_path`. If the current checkout is
/// not registered yet, still maintain its local projection from the resolved store.
fn refresh_checkout_worklists(store_path: &Path, cwd: &Path) -> Result<()> {
    let store_path = store_path
        .canonicalize()
        .unwrap_or_else(|_| store_path.to_path_buf());
    let registry = hotsheet_ticketing::checkouts::CheckoutRegistry::new(
        hotsheet_plugins::hotsheet_home().join("checkouts.json"),
    );
    let mut refreshed_current = false;
    let mut had_matching_store_checkout = false;
    for checkout in registry.list()? {
        let matches = checkout.stores.iter().any(|candidate| {
            Path::new(candidate)
                .canonicalize()
                .unwrap_or_else(|_| PathBuf::from(candidate))
                == store_path
        });
        if matches {
            had_matching_store_checkout = true;
            refreshed_current |= Path::new(&checkout.root).canonicalize().ok().as_ref()
                == cwd.canonicalize().ok().as_ref();
            let mut by_id: BTreeMap<Ulid, Ticket> = BTreeMap::new();
            for root in &checkout.stores {
                for ticket in indexed_up_next_tickets(&FsStore::open(root)?)? {
                    match by_id.get(&ticket.id) {
                        Some(existing) if existing.status != Status::Moved => {}
                        _ => {
                            by_id.insert(ticket.id, ticket);
                        }
                    }
                }
            }
            hotsheet_ticketing::worklist::regenerate_checkout_from_tickets(
                &checkout,
                &by_id.into_values().collect::<Vec<_>>(),
            )?;
        }
    }
    // Only fall back to a cwd-derived worklist when this store backs no registered checkout. When it
    // does (the standard separate ticket-store + code-checkout setup), the registered checkout above
    // already got the worklist, and this fallback would otherwise write a second one into the ticket
    // store itself whenever a mutation runs with cwd resolving to the store (HS2-00Q744). A standalone
    // store-that-is-also-the-project (no registered checkout) still gets its local worklist here.
    if !refreshed_current && !had_matching_store_checkout && FsStore::open(&store_path).is_ok() {
        let checkout_root = local_checkout_root(&store_path, cwd);
        let store = FsStore::open(&store_path)?;
        let tickets = indexed_up_next_tickets(&store)?;
        hotsheet_ticketing::worklist::regenerate_for_project_from_tickets(
            &tickets,
            &checkout_root.join(hotsheet_ticketing::worklist::CHECKOUT_WORKLIST),
            &checkout_root,
            [store.root()],
        )?;
    }
    Ok(())
}

fn indexed_up_next_tickets(store: &FsStore) -> Result<Vec<Ticket>> {
    let index = hotsheet_index::Index::open_reconciled(&default_index_path(store)?, store)?;
    index
        .query(&TicketQuery {
            up_next_only: true,
            open_only: true,
            ..Default::default()
        })?
        .into_iter()
        .map(|row| {
            let id = Ulid::from_string(&row.id)
                .with_context(|| format!("invalid indexed ticket id '{}'", row.id))?;
            Ok(store.read_ticket(&id)?)
        })
        .collect()
}

fn local_checkout_root(store_path: &Path, cwd: &Path) -> PathBuf {
    let linked = hotsheet_cli::resolve_store_path(PathBuf::from("."), cwd);
    let canonical = |path: &Path| path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if cwd.join(hotsheet_cli::STORE_LINK).is_file() && canonical(&linked) == canonical(store_path) {
        cwd.to_path_buf()
    } else {
        store_path.to_path_buf()
    }
}

fn read_note_input(
    note: Option<String>,
    note_file: Option<PathBuf>,
    allow_literal_backslash_n: bool,
) -> Result<Option<String>> {
    let Some(path) = note_file else {
        if !allow_literal_backslash_n
            && note
                .as_deref()
                .is_some_and(contains_literal_backslash_n_outside_code)
        {
            bail!(
                "--note contains a literal \\n sequence outside Markdown code; use --note-file \
                 <path> or --note-file - for multiline Markdown with real line breaks, or pass \
                 --allow-literal-backslash-n when the text is intentional"
            );
        }
        return Ok(note);
    };
    let text = if path == Path::new("-") {
        let mut text = String::new();
        std::io::stdin()
            .read_to_string(&mut text)
            .context("failed to read note body from stdin")?;
        text
    } else {
        std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read note body from {}", path.display()))?
    };
    Ok(Some(text))
}

fn contains_literal_backslash_n_outside_code(markdown: &str) -> bool {
    let bytes = markdown.as_bytes();
    let mut index = 0;
    let mut inline_ticks = None;
    let mut fenced_ticks = None;

    while index < bytes.len() {
        if bytes[index] == b'`' {
            let run = backtick_run(bytes, index);
            let fence_position = is_fence_position(bytes, index);

            if let Some(opening_run) = fenced_ticks {
                if fence_position && run >= opening_run && fence_closes_line(bytes, index + run) {
                    fenced_ticks = None;
                }
            } else if let Some(opening_run) = inline_ticks {
                if run == opening_run {
                    inline_ticks = None;
                }
            } else if !is_backslash_escaped(bytes, index) {
                if run >= 3 && fence_position {
                    fenced_ticks = Some(run);
                } else if has_matching_backtick_run(bytes, index + run, run) {
                    inline_ticks = Some(run);
                }
            }
            index += run;
            continue;
        }

        if inline_ticks.is_none()
            && fenced_ticks.is_none()
            && bytes[index] == b'\\'
            && bytes.get(index + 1) == Some(&b'n')
        {
            return true;
        }
        index += 1;
    }
    false
}

fn backtick_run(bytes: &[u8], start: usize) -> usize {
    bytes[start..]
        .iter()
        .take_while(|byte| **byte == b'`')
        .count()
}

fn is_fence_position(bytes: &[u8], index: usize) -> bool {
    let line_start = bytes[..index]
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |newline| newline + 1);
    index - line_start <= 3 && bytes[line_start..index].iter().all(|byte| *byte == b' ')
}

fn fence_closes_line(bytes: &[u8], after_run: usize) -> bool {
    bytes[after_run..]
        .iter()
        .take_while(|byte| **byte != b'\n')
        .all(|byte| byte.is_ascii_whitespace())
}

fn is_backslash_escaped(bytes: &[u8], index: usize) -> bool {
    bytes[..index]
        .iter()
        .rev()
        .take_while(|byte| **byte == b'\\')
        .count()
        % 2
        == 1
}

fn has_matching_backtick_run(bytes: &[u8], mut index: usize, expected: usize) -> bool {
    while index < bytes.len() {
        if bytes[index] != b'`' {
            index += 1;
            continue;
        }
        let run = backtick_run(bytes, index);
        if run == expected {
            return true;
        }
        index += run;
    }
    false
}

/// Clap value parser for `--note-confidence`: a plain integer from 0 to 100 (HS2-DWTJ43).
fn parse_confidence_arg(value: &str) -> Result<Confidence, String> {
    value
        .parse::<Confidence>()
        .map_err(|error| error.to_string())
}

/// `--note-confidence` / `--clear-note-confidence` as a change: `Some(Some(score))` sets,
/// `Some(None)` clears (edits only), `None` leaves the score alone (HS2-CY4CWC).
fn confidence_change(
    note_confidence: Option<Confidence>,
    clear_note_confidence: bool,
) -> Option<Option<Confidence>> {
    if clear_note_confidence {
        Some(None)
    } else {
        note_confidence.map(Some)
    }
}

fn validate_note_modifiers(
    note: &Option<String>,
    note_kind: Option<&String>,
    note_summary: Option<&String>,
    note_confidence: Option<Option<Confidence>>,
    edit_note: Option<&String>,
) -> Result<()> {
    if note.is_some() {
        return Ok(());
    }
    if note_kind.is_some() || note_summary.is_some() {
        bail!("--note-kind and --note-summary require --note or --note-file");
    }
    match (edit_note.is_some(), note_confidence.is_some()) {
        (true, false) => bail!(
            "--edit-note requires --note, --note-file, --note-confidence, or \
             --clear-note-confidence"
        ),
        (false, true) => {
            bail!("--note-confidence requires --note or --note-file when appending a note")
        }
        _ => Ok(()),
    }
}

fn parse_status_str(s: &str) -> Result<Status> {
    Ok(match s {
        "not_started" => Status::NotStarted,
        "started" => Status::Started,
        "completed" => Status::Completed,
        "verified" => Status::Verified,
        "backlog" => Status::Backlog,
        "archive" => Status::Archive,
        "deleted" => Status::Deleted,
        "moved" => Status::Moved,
        other => bail!(
            "invalid status '{other}' \
             (not_started|started|completed|verified|backlog|archive|deleted|moved)"
        ),
    })
}

fn parse_note_kind(s: &str) -> Result<NoteKind> {
    Ok(match s {
        "regular" => NoteKind::Regular,
        "activity" => NoteKind::Activity,
        "feedback_needed" => NoteKind::FeedbackNeeded,
        "status" => NoteKind::Status,
        other => bail!(
            "invalid note kind '{other}' \
             (regular|activity|feedback_needed|status)"
        ),
    })
}

fn parse_close_reason(s: &str) -> Result<CloseReason> {
    Ok(match s {
        "completed" => CloseReason::Completed,
        "not_planned" => CloseReason::NotPlanned,
        "duplicate" => CloseReason::Duplicate,
        "obsolete" => CloseReason::Obsolete,
        "works_as_designed" => CloseReason::WorksAsDesigned,
        other => bail!(
            "invalid close reason '{other}' (completed|not_planned|duplicate|obsolete|works_as_designed)"
        ),
    })
}

fn discovered_ai_tools() -> Vec<hotsheet_plugins::AiToolDescriptor> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    hotsheet_aitools::discover_ai_tool_descriptors(
        &hotsheet_plugins::default_dirs(),
        &cwd,
        &mut hotsheet_aitools::ModelCatalogCache::default(),
        false,
    )
}

/// A saved AI default from one scope, kept only while it still names an installed tool.
fn saved_ai_defaults(
    settings: &hotsheet_ticketing::Settings,
    scope: hotsheet_ticketing::Scope,
    tools: &[hotsheet_plugins::AiToolDescriptor],
) -> Result<Option<hotsheet_plugins::AiToolDefaults>> {
    Ok(stored_ai_defaults(settings, scope)?
        .filter(|defaults| hotsheet_plugins::validate_ai_defaults(tools, defaults).is_ok())
        .map(|defaults| hotsheet_plugins::sanitize_ai_defaults(tools, defaults)))
}

/// The AI defaults stored in one scope, unvalidated.
fn stored_ai_defaults(
    settings: &hotsheet_ticketing::Settings,
    scope: hotsheet_ticketing::Scope,
) -> Result<Option<hotsheet_plugins::AiToolDefaults>> {
    Ok(settings
        .get("ai.defaults", scope)?
        .and_then(|value| serde_json::from_value(value).ok()))
}

/// The settings file and scope an `ai-settings` write targets.
fn ai_settings_target(
    store: &Path,
    cwd: &Path,
    global: bool,
) -> (hotsheet_ticketing::Settings, hotsheet_ticketing::Scope) {
    if global {
        (
            hotsheet_ticketing::Settings::new(store),
            hotsheet_ticketing::Scope::Global,
        )
    } else {
        (
            settings_for_cli(store, cwd),
            hotsheet_ticketing::Scope::Local,
        )
    }
}

/// The effective AI defaults: this project's machine-local choice (HS2-SW5S13), else the
/// machine-wide fallback, else the discovered default tool. `global` reads only the fallback.
fn effective_ai_defaults(
    store: &Path,
    cwd: &Path,
    global: bool,
) -> Result<hotsheet_plugins::AiToolDefaults> {
    let tools = discovered_ai_tools();
    let project = if global {
        None
    } else {
        saved_ai_defaults(
            &settings_for_cli(store, cwd),
            hotsheet_ticketing::Scope::Local,
            &tools,
        )?
    };
    let saved = match project {
        Some(defaults) => Some(defaults),
        None => saved_ai_defaults(
            &hotsheet_ticketing::Settings::new(store),
            hotsheet_ticketing::Scope::Global,
            &tools,
        )?,
    };
    saved
        .or_else(|| hotsheet_plugins::default_ai_settings(&tools))
        .ok_or_else(|| anyhow::anyhow!("no drivable AI tools are installed"))
}

fn settings_for_cli(store: &Path, cwd: &Path) -> hotsheet_ticketing::Settings {
    let canonical = |path: &Path| path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let canonical_store = canonical(store);
    let canonical_cwd = canonical(cwd);
    let registry = hotsheet_ticketing::checkouts::CheckoutRegistry::new(
        hotsheet_plugins::hotsheet_home().join("checkouts.json"),
    );
    let mut matches = registry
        .list()
        .unwrap_or_default()
        .into_iter()
        .filter(|checkout| canonical_cwd.starts_with(canonical(Path::new(&checkout.root))))
        .filter(|checkout| {
            checkout.sources.iter().any(|source| {
                source.provider == "git" && canonical(Path::new(&source.locator)) == canonical_store
            })
        })
        .collect::<Vec<_>>();
    matches
        .sort_by_key(|checkout| std::cmp::Reverse(Path::new(&checkout.root).components().count()));
    if let Some(checkout) = matches.into_iter().next() {
        checkout.settings()
    } else {
        let project = local_checkout_root(store, cwd);
        hotsheet_ticketing::Settings::with_legacy_stores(project, [store])
    }
}

/// Prompt and read a yes/no from stdin (default no; EOF / non-interactive → no).
fn confirm(prompt: &str) -> Result<bool> {
    use std::io::{BufRead, Write};
    print!("{prompt}");
    std::io::stdout().flush().ok();
    let mut line = String::new();
    if std::io::stdin().lock().read_line(&mut line)? == 0 {
        return Ok(false); // EOF (piped/non-interactive)
    }
    Ok(matches!(
        line.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

/// Whether `name` is an executable file on `PATH` (mirrors the setup detector).
fn binary_on_path(name: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(name).is_file()))
        .unwrap_or(false)
}

// ---- helpers ---------------------------------------------------------------------

/// Resolve a ticket by ULID or slug, erroring if there's no match.
fn resolve(store: &FsStore, needle: &str) -> Result<Ticket> {
    if let Ok(id) = Ulid::from_string(needle) {
        return store
            .read_ticket(&id)
            .with_context(|| format!("no ticket matching '{needle}'"));
    }
    let index = hotsheet_index::Index::open_reconciled(&default_index_path(store)?, store)?;
    let id = index
        .resolve_id(needle)?
        .with_context(|| format!("no ticket matching '{needle}'"))?;
    Ok(store.read_ticket(&id)?)
}

fn parse_priority(s: &str) -> Result<Priority> {
    Ok(match s {
        "highest" => Priority::Highest,
        "high" => Priority::High,
        "default" => Priority::Default,
        "low" => Priority::Low,
        "lowest" => Priority::Lowest,
        other => bail!("invalid priority '{other}' (highest|high|default|low|lowest)"),
    })
}

fn now_ts() -> Timestamp {
    Timestamp::from_datetime(OffsetDateTime::now_utc())
}

#[cfg(test)]
mod server_wrapper_tests {
    use hotsheet_ticketing::{NewTicket, StoreMetadata, ops};

    use super::*;

    /// HS2-V08DXW: every subcommand (recursively) has its own non-empty help, and the
    /// provider disable/remove pair keeps distinct, correctly attached text.
    #[test]
    fn every_subcommand_has_non_empty_help() {
        // The full clap tree is large; build it on a roomy stack so debug builds do not overflow.
        std::thread::Builder::new()
            .stack_size(64 * 1024 * 1024)
            .spawn(check_every_subcommand_help)
            .unwrap()
            .join()
            .unwrap();
    }

    fn check_every_subcommand_help() {
        use clap::CommandFactory;
        fn walk(cmd: &clap::Command, path: &str, missing: &mut Vec<String>) {
            for sub in cmd.get_subcommands() {
                let name = format!("{path} {}", sub.get_name());
                if sub.get_name() != "help"
                    && sub
                        .get_about()
                        .is_none_or(|a| a.to_string().trim().is_empty())
                {
                    missing.push(name.clone());
                }
                walk(sub, &name, missing);
            }
        }
        let cli = Cli::command();
        let mut missing = Vec::new();
        walk(&cli, "hotsheet-cli", &mut missing);
        assert!(missing.is_empty(), "subcommands without help: {missing:?}");
        let about = |n: &str| {
            cli.find_subcommand(n)
                .and_then(|c| c.get_long_about().or(c.get_about()))
                .map(|a| a.to_string())
                .unwrap_or_default()
        };
        assert!(about("provider-disable").contains("Temporarily disable"));
        assert!(!about("provider-disable").contains("Permanently remove"));
        assert!(about("provider-remove").contains("Permanently remove"));
    }

    #[test]
    fn provider_copy_command_is_idempotent() {
        let source_dir = tempfile::tempdir().unwrap();
        let destination_dir = tempfile::tempdir().unwrap();
        let source = FsStore::init(source_dir.path(), &StoreMetadata::new("SRC")).unwrap();
        let destination =
            FsStore::init(destination_dir.path(), &StoreMetadata::new("DST")).unwrap();
        let ticket = ops::create(
            &source,
            Ulid::new(),
            "SRC",
            Timestamp::new("2026-08-26T02:00:00Z"),
            NewTicket {
                title: "copy from cli".into(),
                category: "task".into(),
                priority: Priority::Default,
                status: Status::NotStarted,
                details: String::new(),
                tags: vec![],
                up_next: false,
                blocked_by: vec![],
            },
        )
        .unwrap();
        for _ in 0..2 {
            crate::dispatch::providers::cmd_provider_transfer(
                source_dir.path(),
                &ticket.slug,
                destination_dir.path(),
                "cli-provider-op-1",
                false,
            )
            .unwrap();
        }
        assert_eq!(destination.list_tickets().unwrap().len(), 1);
    }
}
