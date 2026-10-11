//! `hotsheet` — the Hot Sheet 2 command-line interface. A thin binary over
//! `hotsheet-ticketing`: it reads and writes ticket files directly on disk
//! (`docs/04-core-server-cli.md` §4.4) and imports HS1 exports (`docs/07`).

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

mod dispatch;

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use hotsheet_cli::{git_init, run_import};
use hotsheet_model::{
    CloseReason, Confidence, NoteKind, Priority, ReviewKind, ReviewRequest, StartedPhase, Status,
    Ticket, Timestamp, Ulid, parse_file, to_file_string,
};
use hotsheet_ticketing::{
    FsStore, GitProvider, KeyRegistry, MutationContext, NewTicket, OsKeychain, Person,
    ProviderConfigRegistry, ProviderDraft, ProviderPatch, ProviderRegistry, Roster, SortKey,
    StoreMetadata, TicketPatch, TicketProvider, TicketQuery, TicketRef, copy_between,
    git_connection_id, move_between, ops,
};
use time::{Duration, OffsetDateTime};

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
        /// Project directory to write the tool config into (defaults to the store path).
        #[arg(long)]
        project: Option<PathBuf>,
        /// Print the structured setup report, including user-action notices.
        #[arg(long)]
        json: bool,
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
    /// Show the usage/cost metrics rollup for this store (docs/14) — total cost + tokens,
    /// by model, and by day. DB-free: settled rollup files + a live scan of the raw tail.
    Metrics {
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

#[derive(Args)]
struct NewArgs {
    /// Ticket title (positional). Alternatively pass --title.
    title: Option<String>,
    /// Ticket title (alias for the positional form).
    #[arg(long = "title")]
    title_flag: Option<String>,
    #[arg(long, default_value = "issue")]
    category: String,
    #[arg(long, default_value = "default")]
    priority: String,
    #[arg(long)]
    details: Option<String>,
    /// Mark the new ticket Up Next.
    #[arg(long)]
    up_next: bool,
    /// Add a tag (repeatable): `--tag a --tag b`.
    #[arg(long = "tag")]
    tags: Vec<String>,
    /// Blocker ticket (slug or ULID), repeatable: `--blocked-by HS2-ABC --blocked-by HS2-DEF`.
    #[arg(long = "blocked-by")]
    blocked_by: Vec<String>,
}

#[derive(Args)]
struct GithubConnectArgs {
    /// Owner/repository reachable by the Hot Sheet GitHub App.
    locator: String,
    /// Credential reference printed by `github-sign-in` or created with `key set`.
    #[arg(long)]
    credential: String,
    /// Display name (defaults to GitHub Issues).
    #[arg(long)]
    name: Option<String>,
    /// Explicit connection id; otherwise use the existing locator or generate an id.
    #[arg(long)]
    id: Option<String>,
    /// Make this connection the default for its store and, with --checkout, that checkout.
    #[arg(long)]
    default: bool,
    /// Also link the connection to this registered checkout (id, alias, or path).
    #[arg(long)]
    checkout: Option<String>,
    /// Enable attachments by committing files to this `owner/repo` assets repository
    /// and linking them from issue comments (HS2-HSA64D). Kept on a later reconnect.
    #[arg(long, value_name = "OWNER/REPO", conflicts_with = "no_attachments")]
    attachment_repo: Option<String>,
    /// Folder inside the assets repository (default `hotsheet-attachments`).
    #[arg(long, requires = "attachment_repo")]
    attachment_folder: Option<String>,
    /// Branch uploads are committed to (default `main`).
    #[arg(long, requires = "attachment_repo")]
    attachment_branch: Option<String>,
    /// Remove the assets repository; the connection then reports no attachment support.
    #[arg(long)]
    no_attachments: bool,
}

#[derive(Args)]
struct AttachArgs {
    id: String,
    #[arg(required = true)]
    files: Vec<PathBuf>,
    /// Print attachment ids and filenames as JSON.
    #[arg(long)]
    json: bool,
    #[arg(long)]
    batch_id: Option<String>,
    #[arg(long)]
    batch_label: Option<String>,
    #[arg(long)]
    actor_role: Option<String>,
    #[arg(long)]
    actor_id: Option<String>,
    #[arg(long)]
    actor_name: Option<String>,
    #[arg(long)]
    purpose: Option<String>,
}

#[derive(Args)]
struct TriggerArgs {
    /// The tool to drive (e.g. `claude`, `codex`).
    tool: String,
    /// The turn content. Defaults to a "work the top Up Next ticket" prompt.
    #[arg(long)]
    prompt: Option<String>,
    /// Project directory the tool runs in (defaults to the store path).
    #[arg(long)]
    project: Option<PathBuf>,
    /// Resume a prior session id (channel tools).
    #[arg(long)]
    resume: Option<String>,
    /// Restrict a channel tool to only this MCP config (`--strict-mcp-config`), so it
    /// can't reach anything but the Hot Sheet shim (HS2-103 isolation).
    #[arg(long)]
    mcp_config: Option<PathBuf>,
    /// Claude permission mode for headless work (e.g. `acceptEdits`,
    /// `bypassPermissions`). Defaults to `acceptEdits`.
    #[arg(long)]
    permission_mode: Option<String>,
    /// Set an env var for the launched tool (repeatable): `--env CODEX_HOME=/path`.
    #[arg(long = "env")]
    envs: Vec<String>,
    /// Register the connection as a self-claim worker rather than the main session.
    #[arg(long)]
    worker: bool,
    /// Codex only: drive the shared app-server **daemon** for the (isolated) CODEX_HOME
    /// — reuse one codex instance across turns instead of a fresh process per turn
    /// (HS2-B7C66H). Needs the managed standalone install available to symlink.
    #[arg(long = "shared-daemon")]
    shared_daemon: bool,
}

#[derive(Args)]
struct WorkArgs {
    /// The tool to drive (e.g. `claude`).
    tool: String,
    /// Project directory the tool runs in (defaults to the store path).
    #[arg(long)]
    project: Option<PathBuf>,
    /// Maximum turns before stopping (a hard safety cap).
    #[arg(long, default_value_t = 50)]
    max: u32,
    /// Stop after this many consecutive turns that change nothing (thrash guard).
    #[arg(long = "max-stall", default_value_t = 3)]
    max_stall: u32,
    /// Register connections as a self-claim worker rather than the main session.
    #[arg(long)]
    worker: bool,
    /// Codex only: drive the shared app-server **daemon** for the isolated CODEX_HOME —
    /// one codex instance reused across the whole loop's turns instead of a fresh
    /// process per turn (HS2-B7C66H); the daemon is stopped when the loop ends.
    #[arg(long = "shared-daemon")]
    shared_daemon: bool,
}

#[derive(Subcommand)]
enum FeedbackSynthesisCmd {
    /// Read only the explicitly selected connections and prepare a private local draft.
    Prepare {
        #[arg(long = "connection", required = true)]
        connections: Vec<String>,
    },
    /// Advance the review ledger after a human has edited or rejected the draft.
    Accept {
        #[arg(long)]
        reviewed: bool,
    },
}

#[derive(Subcommand)]
enum CertCmd {
    /// Create the per-project CA + server cert (refuses if one already exists). Pass `--host`
    /// once per IP/DNS the server will be reached at off-loopback; `localhost` + `127.0.0.1`
    /// are always covered.
    Init {
        #[arg(long = "host")]
        hosts: Vec<String>,
    },
    /// Issue a client (device) cert signed by the project CA, printing the cert + key + CA
    /// PEMs to copy to the device. `name` identifies it for later revocation.
    Issue { name: String },
    /// Rotate a device certificate, revoking the old leaf and printing a fresh 90-day leaf.
    Renew { name: String },
    /// Set an issued device's explicit ACL role: read-only, read-write, or deny.
    Role { name: String, role: String },
    /// Revoke a previously-issued device by name (takes effect on the next TLS connection).
    Revoke { name: String },
}

#[derive(Subcommand)]
enum PluginCmd {
    /// List built-in + installed plugins, with provenance and whether each is detected.
    List,
    /// Show what a plugin declares it will write + launch (the trust disclosure).
    Info { id: String },
    /// Install an external plugin from a directory into the machine plugin dir.
    Install {
        dir: PathBuf,
        /// Skip the confirmation prompt (non-interactive install).
        #[arg(long)]
        yes: bool,
    },
    /// Verify a plugin structurally (loads, known MCP format, safe write targets).
    Verify { id: String },
    /// Remove an installed external plugin by id.
    Remove { id: String },
}

#[derive(Subcommand)]
enum PeopleCmd {
    /// List the roster (email → name).
    List,
    /// Add or update a person (matched by email).
    Add {
        /// The person's git email (their identity).
        email: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        github: Option<String>,
    },
    /// Seed people.json from GitHub collaborators that expose a public email.
    SeedGithub {
        /// Repository as owner/name.
        #[arg(long)]
        repo: String,
        /// GitHub API root (override for GitHub Enterprise/testing).
        #[arg(long, default_value = "https://api.github.com")]
        api_base: String,
    },
}

#[derive(Subcommand)]
enum SettingsCmd {
    /// Read a setting (effective precedence global < shared < local, unless --scope is given).
    Get {
        key: String,
        /// global | shared | local (default: the effective value).
        #[arg(long)]
        scope: Option<String>,
    },
    /// Write a setting. Value is parsed as JSON if possible, else stored as a string.
    Set {
        key: String,
        value: String,
        /// global | shared | local (default: shared).
        #[arg(long)]
        scope: Option<String>,
    },
    /// List settings (effective, unless --scope is given).
    List {
        #[arg(long)]
        scope: Option<String>,
    },
}

#[derive(Subcommand)]
enum CommandsCmd {
    /// List saved command definitions for a checkout.
    List { checkout: String },
    /// List saved command groups, including empty groups.
    Groups { checkout: String },
    /// Start a saved command and print its run record.
    Run { checkout: String, id: String },
    /// List recent runs for this checkout.
    History { checkout: String },
    /// Get a run, optionally returning only output after a sequence number.
    Get {
        checkout: String,
        id: String,
        #[arg(long, default_value_t = 0)]
        after: u64,
    },
    /// Cancel an active run in this checkout.
    Cancel { checkout: String, id: String },
}

#[derive(Subcommand)]
enum LifecycleCmd {
    /// Print whether the server is quiescing and which active work would block a restart.
    Status,
    /// Ask the server to restart once all server-owned work is quiescent. A busy server
    /// refuses and keeps its work; repeating the request is safe.
    Restart,
}

#[derive(Subcommand)]
enum NotificationsCmd {
    /// List notifications, optionally filtering by target or recipient.
    List {
        #[arg(long)]
        checkout: Option<String>,
        #[arg(long)]
        store: Option<String>,
        #[arg(long)]
        ticket: Option<String>,
        #[arg(long)]
        recipient: Option<String>,
    },
    /// Acknowledge one notification by its stable id.
    Ack { id: String },
}

#[derive(Subcommand)]
enum AiSettingsCmd {
    /// Print the effective defaults: this project's, else the machine-wide fallback.
    Get {
        #[arg(long)]
        json: bool,
        /// Read only the machine-wide fallback.
        #[arg(long)]
        global: bool,
    },
    /// Save this project's default provider with its model and effort (machine-local project
    /// settings), or the machine-wide fallback.
    Set {
        #[arg(long)]
        tool: String,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        effort: Option<String>,
        /// Save the machine-wide fallback used by projects without their own choice.
        #[arg(long)]
        global: bool,
    },
    /// Save one provider's default model and effort without changing the default provider
    /// (HS2-EK24KF); used whenever that provider is picked without an explicit model.
    SetProvider {
        #[arg(long)]
        tool: String,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        effort: Option<String>,
        /// Change the machine-wide fallback instead of this project.
        #[arg(long)]
        global: bool,
    },
}

/// Field edits shared by `edit` (one ticket) and `batch` (many tickets).
#[derive(Args)]
struct TicketEditArgs {
    #[arg(long)]
    title: Option<String>,
    #[arg(long)]
    details: Option<String>,
    #[arg(long)]
    category: Option<String>,
    #[arg(long)]
    priority: Option<String>,
    /// One of not_started|started|completed|verified|backlog|archive|deleted|moved.
    #[arg(long)]
    status: Option<String>,
    /// Progress within Started (analyzing|planning|working|initial_testing|integrating|final_testing).
    #[arg(long, conflicts_with = "clear_started_phase")]
    started_phase: Option<String>,
    /// Clear the Started phase.
    #[arg(long)]
    clear_started_phase: bool,
    /// Replace the tag list (repeatable): `--tag a --tag b`.
    #[arg(long = "tag")]
    tags: Vec<String>,
    /// Replace the blocker set (slug or ULID), repeatable. Ignored if --clear-blocked-by is set.
    #[arg(long = "blocked-by", conflicts_with = "clear_blocked_by")]
    blocked_by: Vec<String>,
    /// Clear all blockers.
    #[arg(long)]
    clear_blocked_by: bool,
    /// Set the user-facing explanation for why this ticket is blocked.
    #[arg(long, conflicts_with = "clear_blocked_reason")]
    blocked_reason: Option<String>,
    /// Clear the blocked reason without changing dependency blockers.
    #[arg(long)]
    clear_blocked_reason: bool,
    /// Mark Up Next.
    #[arg(long, conflicts_with = "no_up_next")]
    up_next: bool,
    /// Clear Up Next.
    #[arg(long)]
    no_up_next: bool,
    /// Append a note to the ticket.
    #[arg(long)]
    note: Option<String>,
    /// Read the note body from a UTF-8 file, or from stdin with `-`.
    #[arg(long, value_name = "PATH", conflicts_with = "note")]
    note_file: Option<PathBuf>,
    /// Permit literal `\\n` text outside Markdown code spans/blocks.
    #[arg(long, requires = "note")]
    allow_literal_backslash_n: bool,
    /// Edit this existing note ULID instead of appending a note.
    #[arg(long, conflicts_with = "note_kind")]
    edit_note: Option<String>,
    /// Kind for --note: regular | activity | feedback_needed | status.
    #[arg(long)]
    note_kind: Option<String>,
    /// Concise plain-text timeline headline for an activity/status note.
    #[arg(long, conflicts_with = "edit_note")]
    note_summary: Option<String>,
    /// AI completion confidence (integer 0-100) recorded on the appended note, or the
    /// corrected score of the note named by --edit-note (note text then optional).
    #[arg(long, value_name = "0-100", value_parser = parse_confidence_arg)]
    note_confidence: Option<Confidence>,
    /// Remove the completion confidence from the note named by --edit-note.
    #[arg(long, requires = "edit_note", conflicts_with = "note_confidence")]
    clear_note_confidence: bool,
}

#[derive(Subcommand)]
enum CheckoutCmd {
    /// Register or update a checkout. Repeating --store records all ticket stores it uses.
    Register {
        root: PathBuf,
        #[arg(long)]
        alias: Option<String>,
        #[arg(long = "store")]
        stores: Vec<PathBuf>,
        #[arg(long)]
        repository: Option<String>,
    },
    /// List registered checkouts as JSON.
    List,
    /// Resolve an id, id prefix, alias, or path and print its JSON record.
    Resolve { reference: String },
    /// Move a registered checkout while preserving its durable project id.
    Relocate {
        reference: String,
        root: PathBuf,
        #[arg(long)]
        alias: Option<String>,
    },
    /// Associate a provider-neutral ticket source with a checkout.
    AddSource {
        reference: String,
        connection_id: String,
        provider: String,
        locator: String,
        #[arg(long)]
        default: bool,
    },
    /// Remove a ticket source from this project; removing the default clears it. Sources
    /// belong to projects: a connection no other checkout links is deleted from the -C
    /// store's providers.json too (its account stays signed in).
    RemoveSource {
        reference: String,
        connection_id: String,
        /// Print the removal report as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Rename a non-Git source while retaining its old id as a durable alias.
    RenameSource {
        reference: String,
        connection_id: String,
        new_connection_id: String,
    },
    /// Inspect a linked source's project-local color as JSON.
    SourceColor {
        reference: String,
        connection_id: String,
    },
    /// Set a linked source's project-local color; use Gray (#6b7280) to restore the default.
    SetSourceColor {
        reference: String,
        connection_id: String,
        color: String,
    },
    /// Select the source used by unqualified creates, or clear it with --clear.
    SetDefault {
        reference: String,
        #[arg(required_unless_present = "clear")]
        connection_id: Option<String>,
        #[arg(long, conflicts_with = "connection_id")]
        clear: bool,
    },
}

#[derive(Subcommand)]
enum AccountCmd {
    /// List provider sign-ins with the ticket sources and projects using each.
    List {
        /// Emit the accounts as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Resolve missing GitHub usernames for a bounded batch of older managed sign-ins.
    /// This may read the OS keychain and contact GitHub; plain `account list` never does so.
    BackfillLogins {
        /// Maximum accounts to try in this run (1-10; default 5).
        #[arg(long, default_value_t = 5)]
        limit: usize,
        /// Start after this credential reference to reach later accounts when earlier ones fail.
        #[arg(long)]
        after: Option<String>,
    },
    /// Sign out: delete the account's credential. Refused while a ticket source uses it.
    SignOut { account: String },
}

#[derive(Subcommand)]
enum KeyCmd {
    /// Store/replace a provider key. Prompts securely on a terminal or reads piped stdin.
    Set { provider: String },
    /// Print a provider key to stdout (explicit secret-reveal operation).
    Get { provider: String },
    /// List registered provider names and their read-only environment fallbacks.
    List,
    /// Delete a provider key from the OS credential store and registry.
    Delete { provider: String },
}

/// Filters + sort for index-backed `ls`.
#[derive(Args)]
struct LsFilters {
    /// Only this status.
    #[arg(long)]
    status: Option<String>,
    /// Only this priority.
    #[arg(long)]
    priority: Option<String>,
    /// Only this category.
    #[arg(long)]
    category: Option<String>,
    /// Must carry every given tag (repeatable).
    #[arg(long = "tag")]
    tags: Vec<String>,
    /// Case-insensitive substring across title, details, and note text.
    #[arg(long)]
    text: Option<String>,
    /// Only Up Next tickets.
    #[arg(long)]
    up_next: bool,
    /// Only open tickets (not completed/verified/deleted/archived/moved).
    #[arg(long)]
    open: bool,
    /// Only tickets closed with this reason (completed|not_planned|duplicate|obsolete|works_as_designed).
    #[arg(long = "close-reason")]
    close_reason: Option<String>,
    /// Only tickets that have a close reason set.
    #[arg(long)]
    closed: bool,
    /// Only tickets assigned to this person (git email, or `me` for your git identity).
    #[arg(long)]
    assignee: Option<String>,
    /// Only tickets with a review request for this person (git email, or `me`).
    #[arg(long = "review-requested")]
    review_requested: Option<String>,
    /// Only tickets whose review was requested by this person (git email, or `me`).
    #[arg(long = "review-by")]
    review_by: Option<String>,
    /// Only tickets with a worker claim (a held lease).
    #[arg(long)]
    claimed: bool,
    /// Only tickets that are blocked (a blocker isn't done yet).
    #[arg(long, conflicts_with = "unblocked")]
    blocked: bool,
    /// Only tickets that are unblocked (all blockers done / none).
    #[arg(long)]
    unblocked: bool,
    /// Only completed/verified tickets whose derived confidence is at least this (0-100).
    #[arg(long, value_name = "0-100", value_parser = clap::value_parser!(u8).range(0..=100))]
    min_confidence: Option<u8>,
    /// Only completed/verified tickets whose derived confidence is at most this (0-100).
    #[arg(long, value_name = "0-100", value_parser = clap::value_parser!(u8).range(0..=100))]
    max_confidence: Option<u8>,
    /// Sort key: id | created | updated | priority | status | title | confidence
    /// (least confident first, unscored last).
    #[arg(long, default_value = "id")]
    sort: String,
    /// Cap the number of rows shown (after sort).
    #[arg(long)]
    limit: Option<usize>,
    /// Keyset cursor (a ULID): show rows strictly after this one, in `sort` order. Page a
    /// large store without OFFSET — pass the last slug/ULID of the previous page.
    #[arg(long = "page-after")]
    page_after: Option<String>,
}

impl LsFilters {
    /// `Some(true)` for `--blocked`, `Some(false)` for `--unblocked`, else `None`.
    fn blocked_filter(&self) -> Option<bool> {
        match (self.blocked, self.unblocked) {
            (true, _) => Some(true),
            (_, true) => Some(false),
            _ => None,
        }
    }
}

fn main() -> Result<()> {
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

/// The default "play" prompt: work the top Up Next ticket end to end, headless.
const DEFAULT_TRIGGER_PROMPT: &str = "Read the Hot Sheet Up Next queue (hotsheet tools or \
`hotsheet-cli ls --up-next`), take the highest-priority ticket, set it started, implement \
it, and mark it completed with a note on what you did. If nothing is Up Next, say so and \
stop.";

/// The CLI's per-turn event sink: stream assistant output to stdout, note permission asks.
fn stream_to_stdout(ev: &hotsheet_aitools::TurnEvent) {
    use hotsheet_aitools::TurnEvent;
    match ev {
        TurnEvent::Output(text) => {
            print!("{text}");
            let _ = std::io::Write::flush(&mut std::io::stdout());
        }
        TurnEvent::PermissionAsked(p) => {
            eprintln!("\n[permission] {} — {}", p.tool, p.summary)
        }
        TurnEvent::Usage(u) => {
            eprintln!("\n[usage] {} in / {} out", u.tokens_in, u.tokens_out)
        }
        // Native activity is for the attributed server sink. Do not dump raw tool inputs
        // into a CLI transcript; assistant output remains the human-facing stream.
        TurnEvent::NativeActivity { .. } => {}
        TurnEvent::Done(_) => {}
    }
}

/// The per-turn prompt for the `work` loop: like the trigger default, but explicit that
/// exactly one ticket should be taken this turn (the loop drives the next one).
const DEFAULT_TRIGGER_PROMPT_LOOP: &str = "Read the Hot Sheet Up Next queue (hotsheet tools \
or `hotsheet-cli ls --up-next`) and take ONLY the single highest-priority ticket: set it \
started, implement it, and mark it completed with a note on what you did. Do just that one \
ticket this turn, then stop. If nothing is Up Next, say so and stop.";

fn shell_quote_path(path: &Path) -> String {
    let value = path.to_string_lossy();
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// How `github-connect` changes a connection's attachment repository (HS2-HSA64D).
enum GitHubAttachmentChange {
    Keep,
    Set {
        repository: String,
        folder: Option<String>,
        branch: Option<String>,
    },
    Remove,
}

impl GitHubAttachmentChange {
    fn from_flags(
        repository: Option<String>,
        folder: Option<String>,
        branch: Option<String>,
        remove: bool,
    ) -> Self {
        match (repository, remove) {
            (Some(repository), _) => Self::Set {
                repository,
                folder,
                branch,
            },
            (None, true) => Self::Remove,
            (None, false) => Self::Keep,
        }
    }
}

fn configured_provider(path: &Path, connection_id: &str) -> Result<Arc<dyn TicketProvider>> {
    let store = FsStore::open(path)?;
    let git_id = git_connection_id(&store);
    if connection_id == git_id {
        return Ok(Arc::new(GitProvider::new(git_id, store)));
    }
    let connection = ProviderConfigRegistry::new(store.root().join("providers.json"))
        .load()?
        .into_iter()
        .find(|connection| connection.id == connection_id)
        .ok_or_else(|| anyhow::anyhow!("provider connection '{connection_id}' was not found"))?;
    if connection.disabled {
        return Err(hotsheet_ticketing::ProviderError::Disabled {
            connection_id: connection_id.into(),
        }
        .into());
    }
    let token = hotsheet_extsync::connection_access_token(
        &connection,
        &KeyRegistry::new(hotsheet_plugins::hotsheet_home(), OsKeychain),
        time::OffsetDateTime::now_utc().unix_timestamp(),
    )?;
    Ok(hotsheet_extsync::live_provider(&connection, token)?)
}

struct ProviderEditInput {
    title: Option<String>,
    details: Option<String>,
    status: Option<String>,
    expected_token: Option<String>,
    note: Option<String>,
    note_kind: NoteKind,
    note_summary: Option<String>,
    note_confidence: Option<Option<Confidence>>,
    edit_note: Option<String>,
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

/// Read-only first-run guidance. Detection never writes tool configuration and never
/// opens/migrates HS1's database; the printed commands are explicit trust gates.
fn print_onboarding_report(store: &Path, project: &Path) {
    let detected: Vec<_> = hotsheet_plugins::all_plugins(&hotsheet_plugins::default_dirs())
        .into_iter()
        .filter(|plugin| {
            plugin
                .manifest
                .detection
                .binaries
                .iter()
                .any(|binary| binary_on_path(binary))
        })
        .map(|plugin| {
            (
                plugin.manifest.product_name.clone(),
                plugin.id().to_string(),
            )
        })
        .collect();
    print!("{}", render_onboarding_report(store, project, &detected));
}

fn render_onboarding_report(store: &Path, project: &Path, detected: &[(String, String)]) -> String {
    use std::fmt::Write as _;
    let mut out = String::from("\nOnboarding (read-only):\n");
    if detected.is_empty() {
        writeln!(out, "  AI tools: none detected on PATH; no setup was run").unwrap();
    } else {
        writeln!(
            out,
            "  AI tools detected (setup is explicit and idempotent):"
        )
        .unwrap();
        for (product, id) in detected {
            writeln!(
                out,
                "    - {}: hotsheet-cli -C \"{}\" setup {} --project \"{}\"",
                product,
                store.display(),
                id,
                project.display()
            )
            .unwrap();
        }
        writeln!(out, "  No tool configuration was changed.").unwrap();
    }

    let hs1 = project.join(".hotsheet/db/PG_VERSION");
    if hs1.is_file() {
        writeln!(
            out,
            "  ! Hot Sheet 1 PGlite data detected at {}",
            hs1.display()
        )
        .unwrap();
        writeln!(out, "    Close the Hot Sheet 1 project before migrating.").unwrap();
        writeln!(
            out,
            "    Review and run explicitly: hotsheet-migrate \"{}\" -C \"{}\"",
            project.join(".hotsheet").display(),
            store.display()
        )
        .unwrap();
        writeln!(
            out,
            "    Migration was not started; the HS1 source remains untouched."
        )
        .unwrap();
    } else {
        writeln!(
            out,
            "  Hot Sheet 1 data: not detected (looked for .hotsheet/db/PG_VERSION)"
        )
        .unwrap();
    }
    out
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

fn choose_launch_source(project: &Path, candidates: &[PathBuf]) -> Result<PathBuf> {
    use std::io::Write;
    if candidates.is_empty() {
        let proposed = PathBuf::from(format!("{}.hs2", project.display()));
        eprint!(
            "No ticket source found. [c]reate {}, [s]elect existing, or [q]uit: ",
            proposed.display()
        );
        std::io::stderr().flush()?;
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer)?;
        if answer.trim().eq_ignore_ascii_case("c") {
            FsStore::init(&proposed, &StoreMetadata::new("HS"))?;
            git_init(&proposed);
            return Ok(proposed.canonicalize().unwrap_or(proposed));
        }
        if answer.trim().eq_ignore_ascii_case("s") {
            eprint!("Existing ticket store path: ");
            std::io::stderr().flush()?;
            answer.clear();
            std::io::stdin().read_line(&mut answer)?;
            let path = PathBuf::from(answer.trim());
            FsStore::open(&path)
                .with_context(|| format!("{} is not a Hot Sheet ticket store", path.display()))?;
            return Ok(path.canonicalize().unwrap_or(path));
        }
        bail!("ticket-source onboarding cancelled")
    }
    eprintln!("Choose a ticket source for {}:", project.display());
    for (index, path) in candidates.iter().enumerate() {
        eprintln!("  {}) {}", index + 1, path.display());
    }
    eprint!("Selection (or 0 to cancel): ");
    std::io::stderr().flush()?;
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    let selection = answer.trim().parse::<usize>().unwrap_or(0);
    hotsheet_cli::select_launch_candidate(candidates, selection)
        .context("ticket-source onboarding cancelled")
}

/// POST the ask to the server's `/permissions/ask`, returning its JSON reply.
/// The running Hot Sheet server a hook should talk to: the project's discovered instance, else
/// the URL and secret the terminal was launched with.
fn hook_server_route() -> Option<(String, String)> {
    let project = std::env::var("HOTSHEET_PROJECT").unwrap_or_default();
    let current = (!project.is_empty())
        .then(|| {
            hotsheet_cli::external_launch::discover_running_server(
                Path::new(&project),
                &hotsheet_plugins::hotsheet_home(),
            )
            .ok()
        })
        .flatten();
    current
        .map(|server| (server.url, server.secret))
        .or_else(|| {
            Some((
                std::env::var("HOTSHEET_SERVER").ok()?,
                std::env::var("HOTSHEET_SECRET").ok()?,
            ))
        })
}

fn hook_get_json(url: &str, secret: &str) -> Result<serde_json::Value> {
    let body = ureq::get(url)
        .set("X-Hotsheet-Secret", secret)
        .timeout(std::time::Duration::from_secs(5))
        .call()?
        .into_string()?;
    Ok(serde_json::from_str(&body)?)
}

/// Report a session lifecycle event against the hook's terminal: `POST` (halted) or `DELETE`
/// (resumed) `/terminals/{id}/halt`, and `POST` (session started) or `DELETE` (ended)
/// `/terminals/{id}/ai-connection`.
fn report_terminal_session(
    url: &str,
    secret: &str,
    terminal: &str,
    session: &hotsheet_cli::permission_hook::SessionHookEvent,
    agent: Option<&str>,
    session_id: Option<&str>,
) -> Result<()> {
    use hotsheet_cli::permission_hook::SessionHookEvent;
    let endpoint = |route: &str| {
        format!(
            "{}/terminals/{}/{route}",
            url.trim_end_matches('/'),
            urlencoding_component(terminal)
        )
    };
    let post = |route: &str, body: serde_json::Value| -> Result<()> {
        ureq::post(&endpoint(route))
            .set("X-Hotsheet-Secret", secret)
            .set("Content-Type", "application/json")
            .timeout(std::time::Duration::from_secs(5))
            .send_string(&body.to_string())?;
        Ok(())
    };
    let delete = |route: &str, session_id: Option<&str>| -> Result<()> {
        let url = match session_id.filter(|id| !id.is_empty()) {
            Some(id) => format!(
                "{}?session_id={}",
                endpoint(route),
                urlencoding_component(id)
            ),
            None => endpoint(route),
        };
        ureq::delete(&url)
            .set("X-Hotsheet-Secret", secret)
            .timeout(std::time::Duration::from_secs(5))
            .call()?;
        Ok(())
    };
    match session {
        SessionHookEvent::QuestionAsked {
            question,
            tool_use_id,
            questions,
        } => post(
            "question",
            serde_json::json!({ "question": question, "tool_use_id": tool_use_id, "session_id": session_id, "questions": questions }),
        ),
        SessionHookEvent::QuestionResolved { tool_use_id } => {
            let mut url = format!(
                "{}?tool_use_id={}",
                endpoint("question"),
                urlencoding_component(tool_use_id)
            );
            if let Some(session_id) = session_id {
                url.push_str("&session_id=");
                url.push_str(&urlencoding_component(session_id));
            }
            ureq::delete(&url)
                .set("X-Hotsheet-Secret", secret)
                .timeout(std::time::Duration::from_secs(5))
                .call()?;
            Ok(())
        }
        SessionHookEvent::Halted {
            error_type,
            message,
        } => post(
            "halt",
            serde_json::json!({ "error_type": error_type, "message": message, "agent": agent }),
        ),
        SessionHookEvent::Resumed => delete("halt", None),
        SessionHookEvent::Connected => post(
            "ai-connection",
            serde_json::json!({ "agent": agent, "session_id": session_id }),
        ),
        SessionHookEvent::Disconnected => delete("ai-connection", session_id),
    }
}

fn wait_for_terminal_question_answer(
    url: &str,
    secret: &str,
    terminal: &str,
    tool_use_id: &str,
    session_id: Option<&str>,
) -> Option<serde_json::Value> {
    let mut endpoint = format!(
        "{}/terminals/{}/question?tool_use_id={}",
        url.trim_end_matches('/'),
        urlencoding_component(terminal),
        urlencoding_component(tool_use_id),
    );
    if let Some(session_id) = session_id {
        endpoint.push_str("&session_id=");
        endpoint.push_str(&urlencoding_component(session_id));
    }
    // Claude's installed hook timeout is just over one day. Leave a small margin so Claude
    // gets its native prompt if nobody answers or the server disappears.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(86_400);
    while std::time::Instant::now() < deadline {
        match ureq::get(&endpoint)
            .set("X-Hotsheet-Secret", secret)
            .timeout(std::time::Duration::from_secs(5))
            .call()
        {
            Ok(response) if response.status() == 204 => {}
            Ok(response) => {
                let reply: serde_json::Value =
                    serde_json::from_str(&response.into_string().ok()?).ok()?;
                if reply["native"] == true {
                    return None;
                }
                return reply.get("answers").cloned();
            }
            Err(_) => {
                // A failed poll returns Claude to its native prompt. Clear the matching notice
                // when the server is still reachable; a newer session/tool cannot be removed.
                let _ = ureq::delete(&endpoint)
                    .set("X-Hotsheet-Secret", secret)
                    .timeout(std::time::Duration::from_secs(2))
                    .call();
                return None;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(350));
    }
    None
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

fn ask_server(url: &str, secret: &str, request: &serde_json::Value) -> Result<serde_json::Value> {
    let endpoint = format!("{}/permissions/ask", url.trim_end_matches('/'));
    let text = ureq::post(&endpoint)
        .set("X-Hotsheet-Secret", secret)
        .set("Content-Type", "application/json")
        .send_string(&request.to_string())?
        .into_string()?;
    Ok(serde_json::from_str(&text)?)
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

#[derive(Clone, Debug, serde::Deserialize, Eq, Ord, PartialEq, PartialOrd)]
struct RegisteredServer {
    pid: u32,
    url: String,
    store_path: String,
    started_at: String,
}

#[derive(Clone, Debug)]
struct RegistryRecord {
    source: PathBuf,
    server: Option<RegisteredServer>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ServerIdentity {
    pid: u32,
    url: String,
    started_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ServerInventoryEntry {
    identity: ServerIdentity,
    status: &'static str,
    stores: Vec<String>,
}

fn cmd_list_servers() -> Result<()> {
    let directory = hotsheet_plugins::hotsheet_home().join("instances");
    let records = read_registry_records(&directory)?;
    let agent = ureq::AgentBuilder::new()
        .user_agent("hotsheet2-server-inventory")
        .timeout_connect(std::time::Duration::from_millis(300))
        .timeout_read(std::time::Duration::from_millis(500))
        .build();
    let (servers, invalid) = server_inventory(records, process_is_alive, |url| {
        match agent
            .get(&format!("{}/health", url.trim_end_matches('/')))
            .call()
        {
            Ok(response) => {
                response
                    .into_string()
                    .ok()
                    .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
                    .and_then(|value| {
                        value
                            .get("status")
                            .and_then(|status| status.as_str())
                            .map(str::to_owned)
                    })
                    .as_deref()
                    == Some("ok")
            }
            Err(_) => false,
        }
    });
    print!("{}", render_server_inventory(&servers, &invalid));
    Ok(())
}

fn read_registry_records(directory: &Path) -> Result<Vec<RegistryRecord>> {
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error).context("could not read server instance registry"),
    };
    let mut records = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .map(|source| RegistryRecord {
            server: std::fs::read_to_string(&source)
                .ok()
                .and_then(|text| serde_json::from_str(&text).ok()),
            source,
        })
        .collect::<Vec<_>>();
    records.sort_by(|left, right| left.source.cmp(&right.source));
    Ok(records)
}

fn server_inventory(
    records: Vec<RegistryRecord>,
    is_alive: impl Fn(u32) -> bool,
    is_healthy: impl Fn(&str) -> bool,
) -> (Vec<ServerInventoryEntry>, Vec<PathBuf>) {
    let mut grouped = BTreeMap::<ServerIdentity, BTreeSet<String>>::new();
    let mut invalid = Vec::new();
    for record in records {
        let Some(server) = record.server else {
            invalid.push(record.source);
            continue;
        };
        let identity = ServerIdentity {
            pid: server.pid,
            url: server.url,
            started_at: server.started_at,
        };
        grouped
            .entry(identity)
            .or_default()
            .insert(server.store_path);
    }
    let mut servers = grouped
        .into_iter()
        .map(|(identity, stores)| {
            let status = if !is_alive(identity.pid) {
                "stale"
            } else if is_healthy(&identity.url) {
                "healthy"
            } else {
                "unhealthy"
            };
            ServerInventoryEntry {
                identity,
                status,
                stores: stores.into_iter().collect(),
            }
        })
        .collect::<Vec<_>>();
    servers.sort_by_key(|server| {
        (
            match server.status {
                "healthy" => 0,
                "unhealthy" => 1,
                _ => 2,
            },
            server.identity.pid,
        )
    });
    (servers, invalid)
}

fn render_server_inventory(servers: &[ServerInventoryEntry], invalid: &[PathBuf]) -> String {
    if servers.is_empty() && invalid.is_empty() {
        return "no registered Hot Sheet servers\n".into();
    }
    let mut output = String::new();
    for server in servers {
        output.push_str(&format!(
            "{:<9} pid={} url={} started={}\n",
            server.status, server.identity.pid, server.identity.url, server.identity.started_at
        ));
        for store in &server.stores {
            output.push_str(&format!("  store: {store}\n"));
        }
    }
    for path in invalid {
        output.push_str(&format!(
            "stale     invalid registry record: {}\n",
            path.display()
        ));
    }
    output
}

#[cfg(unix)]
fn process_is_alive(pid: u32) -> bool {
    if pid == 0 || pid > i32::MAX as u32 {
        return false;
    }
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(not(unix))]
fn process_is_alive(_pid: u32) -> bool {
    false
}

fn resolve_server_binary(current_exe: &Path, path: Option<&std::ffi::OsStr>) -> Result<PathBuf> {
    let sibling = current_exe
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!("hotsheet-server{}", std::env::consts::EXE_SUFFIX));
    if sibling.is_file() {
        return Ok(sibling);
    }
    if let Some(found) = path.and_then(|paths| {
        std::env::split_paths(paths)
            .map(|dir| dir.join(format!("hotsheet-server{}", std::env::consts::EXE_SUFFIX)))
            .find(|candidate| candidate.is_file())
    }) {
        return Ok(found);
    }
    bail!(
        "hotsheet-server is not installed: expected it beside hotsheet-cli at {} or on PATH",
        sibling.display()
    )
}

fn verify_server_version(exe: &Path) -> Result<()> {
    let output = hotsheet_ticketing::git::launch(exe)
        .arg("--version")
        .output()
        .with_context(|| format!("could not run {} --version", exe.display()))?;
    if !output.status.success() {
        bail!("{} --version exited with {}", exe.display(), output.status);
    }
    let reported = String::from_utf8_lossy(&output.stdout);
    let version = reported.split_whitespace().next_back().unwrap_or_default();
    if version != env!("CARGO_PKG_VERSION") {
        bail!(
            "hotsheet-cli {} cannot launch hotsheet-server {} at {}; install matching binaries",
            env!("CARGO_PKG_VERSION"),
            if version.is_empty() {
                "(unknown)"
            } else {
                version
            },
            exe.display()
        );
    }
    Ok(())
}

fn server_args(
    path: &Path,
    bind: &str,
    secret: Option<&str>,
    stop: bool,
    kill_all_terminals: bool,
) -> Vec<std::ffi::OsString> {
    let mut args = vec![
        "-C".into(),
        path.as_os_str().to_owned(),
        "--bind".into(),
        bind.into(),
    ];
    if let Some(secret) = secret {
        args.extend(["--secret".into(), secret.into()]);
    }
    if stop {
        args.push("--stop".into());
    }
    if kill_all_terminals {
        args.push("--kill-all-terminals".into());
    }
    args
}

/// A plain text 3-way merge via `git merge-file` (returns the merged text — with conflict
/// markers when it can't resolve — and whether it was clean).
fn git_merge_file(ours: &str, base: &str, theirs: &str) -> Result<(String, bool)> {
    let dir = tempfile::tempdir()?;
    let write = |name: &str, s: &str| -> Result<PathBuf> {
        let p = dir.path().join(name);
        std::fs::write(&p, s)?;
        Ok(p)
    };
    let (op, bp, tp) = (
        write("ours", ours)?,
        write("base", base)?,
        write("theirs", theirs)?,
    );
    let out = hotsheet_ticketing::git::command()
        .args([
            "merge-file",
            "-p",
            "-L",
            "ours",
            "-L",
            "base",
            "-L",
            "theirs",
        ])
        .args([&op, &bp, &tp])
        .output()
        .context("running `git merge-file`")?;
    Ok((
        String::from_utf8_lossy(&out.stdout).into_owned(),
        out.status.success(),
    ))
}

/// The suffix that reports a claim's ETA, if it has one.
fn eta_suffix(ticket: &hotsheet_model::Ticket) -> String {
    ticket
        .claim_eta_at
        .as_ref()
        .map(|eta| format!(", ETA {}", eta.as_str()))
        .unwrap_or_default()
}

fn parse_attachment_actor_role(value: &str) -> Result<hotsheet_model::AttachmentActorRole> {
    match value {
        "human" => Ok(hotsheet_model::AttachmentActorRole::Human),
        "ai" => Ok(hotsheet_model::AttachmentActorRole::Ai),
        "system" => Ok(hotsheet_model::AttachmentActorRole::System),
        "unknown" => Ok(hotsheet_model::AttachmentActorRole::Unknown),
        _ => anyhow::bail!("--actor-role must be human, ai, system, or unknown"),
    }
}

/// A validated ticket edit: [`TicketEditArgs`] with its note input read and checked.
#[derive(Clone)]
struct TicketEdit {
    title: Option<String>,
    details: Option<String>,
    category: Option<String>,
    priority: Option<String>,
    status: Option<String>,
    started_phase: Option<String>,
    clear_started_phase: bool,
    tags: Vec<String>,
    blocked_by: Vec<String>,
    clear_blocked_by: bool,
    blocked_reason: Option<String>,
    clear_blocked_reason: bool,
    up_next: bool,
    no_up_next: bool,
    note: Option<String>,
    note_kind: NoteKind,
    note_summary: Option<String>,
    note_confidence: Option<Option<Confidence>>,
    edit_note: Option<String>,
}

impl TicketEdit {
    /// Read `--note`/`--note-file` and validate the note modifiers before any write.
    fn prepare(args: TicketEditArgs) -> Result<Self> {
        let note = read_note_input(args.note, args.note_file, args.allow_literal_backslash_n)?;
        let note_confidence = confidence_change(args.note_confidence, args.clear_note_confidence);
        validate_note_modifiers(
            &note,
            args.note_kind.as_ref(),
            args.note_summary.as_ref(),
            note_confidence,
            args.edit_note.as_ref(),
        )?;
        Ok(Self {
            title: args.title,
            details: args.details,
            category: args.category,
            priority: args.priority,
            status: args.status,
            started_phase: args.started_phase,
            clear_started_phase: args.clear_started_phase,
            tags: args.tags,
            blocked_by: args.blocked_by,
            clear_blocked_by: args.clear_blocked_by,
            blocked_reason: args.blocked_reason,
            clear_blocked_reason: args.clear_blocked_reason,
            up_next: args.up_next,
            no_up_next: args.no_up_next,
            note,
            note_kind: parse_note_kind(args.note_kind.as_deref().unwrap_or("regular"))?,
            note_summary: args.note_summary,
            note_confidence,
            edit_note: args.edit_note,
        })
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

fn parse_started_phase(s: &str) -> Result<StartedPhase> {
    Ok(match s {
        "analyzing" => StartedPhase::Analyzing,
        "planning" => StartedPhase::Planning,
        "working" => StartedPhase::Working,
        "initial_testing" => StartedPhase::InitialTesting,
        "integrating" => StartedPhase::Integrating,
        "final_testing" => StartedPhase::FinalTesting,
        other => bail!(
            "invalid started phase '{other}' (analyzing|planning|working|initial_testing|integrating|final_testing)"
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

/// Validate and store AI defaults, keeping choices for providers that are not installed now.
fn save_ai_defaults(
    store: &Path,
    cwd: &Path,
    global: bool,
    defaults: &hotsheet_plugins::AiToolDefaults,
) -> Result<hotsheet_plugins::AiToolDefaults> {
    let tools = discovered_ai_tools();
    let (settings, scope) = ai_settings_target(store, cwd, global);
    let previous = stored_ai_defaults(&settings, scope)?;
    let prepared =
        hotsheet_plugins::prepare_ai_defaults_for_save(&tools, defaults, previous.as_ref())
            .map_err(anyhow::Error::msg)?;
    settings.set("ai.defaults", serde_json::to_value(&prepared)?, scope)?;
    Ok(hotsheet_plugins::sanitize_ai_defaults(&tools, prepared))
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

fn filtered_route(base: &str, filters: &[(&str, String)]) -> String {
    if filters.is_empty() {
        return base.to_owned();
    }
    let query = filters
        .iter()
        .map(|(key, value)| format!("{key}={}", urlencoding_component(value)))
        .collect::<Vec<_>>()
        .join("&");
    format!("{base}?{query}")
}

fn print_local_server_json(store: &Path, method: &str, route: &str) -> Result<()> {
    let body = local_server_json(store, method, route)?;
    println!("{}", serde_json::to_string_pretty(&body)?);
    Ok(())
}

fn local_server_json(store: &Path, method: &str, route: &str) -> Result<serde_json::Value> {
    let server = hotsheet_cli::external_launch::discover_running_server(
        store,
        &hotsheet_plugins::hotsheet_home(),
    )?;
    let endpoint = format!("{}/{}", server.url.trim_end_matches('/'), route);
    let request = ureq::request(method, &endpoint)
        .set("X-Hotsheet-Secret", &server.secret)
        .timeout(std::time::Duration::from_secs(10));
    let response = match request.call() {
        Ok(response) => response,
        Err(ureq::Error::Status(status, response)) => {
            let body = response.into_string().unwrap_or_default();
            let message = serde_json::from_str::<serde_json::Value>(&body)
                .ok()
                .and_then(|body| body["error"].as_str().map(str::to_owned))
                .unwrap_or_else(|| format!("server returned HTTP {status}"));
            bail!("Hot Sheet server request failed: {message}");
        }
        Err(error) => return Err(error.into()),
    };
    Ok(serde_json::from_reader(response.into_reader())?)
}

fn read_key_secret(
    provider: &str,
    terminal: bool,
    mut reader: impl std::io::Read,
    prompt: impl FnOnce(&str) -> Result<String>,
) -> Result<String> {
    let value = if terminal {
        prompt(&format!("Key for {provider}: "))?
    } else {
        let mut value = String::new();
        reader.read_to_string(&mut value)?;
        value
    };
    let value = value.trim_end_matches(['\r', '\n']).to_owned();
    if value.is_empty() {
        bail!("refusing to store an empty key")
    }
    Ok(value)
}

fn print_checkout_source_color(
    checkout: &hotsheet_ticketing::checkouts::Checkout,
    connection_id: &str,
) -> Result<()> {
    if checkout.source(connection_id).is_none() {
        bail!(
            "ticket source '{connection_id}' is not linked to checkout {}",
            checkout.id
        );
    }
    let color = hotsheet_ticketing::checkouts::effective_source_color(
        checkout
            .source_colors
            .get(connection_id)
            .map(String::as_str),
    );
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "checkout_id": checkout.id,
            "connection_id": connection_id,
            "color": color,
        }))?
    );
    Ok(())
}

/// Render a JSON setting value for the terminal: a bare string prints unquoted;
/// everything else prints as compact JSON.
fn render_value(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
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

fn lease_until(now: OffsetDateTime, minutes: i64) -> Timestamp {
    Timestamp::from_datetime(now + Duration::minutes(minutes))
}

#[cfg(test)]
mod server_wrapper_tests {
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
    fn key_input_prompts_on_a_terminal_and_reads_pipes_without_prompting() {
        let prompted = read_key_secret("github-live", true, "ignored".as_bytes(), |prompt| {
            assert_eq!(prompt, "Key for github-live: ");
            Ok("terminal-secret".into())
        })
        .unwrap();
        assert_eq!(prompted, "terminal-secret");

        let piped = read_key_secret("github-live", false, "pipe-secret\n".as_bytes(), |_| {
            panic!("non-terminal input must not prompt")
        })
        .unwrap();
        assert_eq!(piped, "pipe-secret");
        assert!(
            read_key_secret("github-live", false, "\n".as_bytes(), |_| unreachable!())
                .unwrap_err()
                .to_string()
                .contains("empty key")
        );
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

    #[test]
    fn sibling_server_wins_over_path_and_arguments_are_forwarded_exactly() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        let elsewhere = dir.path().join("elsewhere");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&elsewhere).unwrap();
        let cli = bin.join(format!("hotsheet-cli{}", std::env::consts::EXE_SUFFIX));
        let sibling = bin.join(format!("hotsheet-server{}", std::env::consts::EXE_SUFFIX));
        let path_server =
            elsewhere.join(format!("hotsheet-server{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&sibling, "").unwrap();
        std::fs::write(&path_server, "").unwrap();

        assert_eq!(
            resolve_server_binary(&cli, Some(elsewhere.as_os_str())).unwrap(),
            sibling
        );
        assert_eq!(
            server_args(
                Path::new("store path"),
                "127.0.0.1:0",
                Some("secret"),
                true,
                true
            ),
            [
                "-C",
                "store path",
                "--bind",
                "127.0.0.1:0",
                "--secret",
                "secret",
                "--stop",
                "--kill-all-terminals"
            ]
            .map(std::ffi::OsString::from)
        );
    }

    #[test]
    fn server_inventory_deduplicates_stores_checks_health_and_omits_secrets() {
        let dir = tempfile::tempdir().unwrap();
        let records = [
            (
                "one.json",
                r#"{"pid":42,"url":"http://127.0.0.1:8787","secret":"never-print-me","store_path":"/stores/one","started_at":"2026-09-10T01:00:00Z"}"#,
            ),
            (
                "two.json",
                r#"{"pid":42,"url":"http://127.0.0.1:8787","secret":"never-print-me","store_path":"/stores/two","started_at":"2026-09-10T01:00:00Z"}"#,
            ),
            (
                "stale.json",
                r#"{"pid":99,"url":"http://127.0.0.1:9999","secret":"also-private","store_path":"/stores/old","started_at":"2026-09-09T01:00:00Z"}"#,
            ),
            ("broken.json", "not json"),
        ];
        for (name, contents) in records {
            std::fs::write(dir.path().join(name), contents).unwrap();
        }
        let health_checks = std::cell::Cell::new(0);
        let (servers, invalid) = server_inventory(
            read_registry_records(dir.path()).unwrap(),
            |pid| pid == 42,
            |url| {
                health_checks.set(health_checks.get() + 1);
                url.ends_with(":8787")
            },
        );
        assert_eq!(servers.len(), 2, "two registrations share one live server");
        assert_eq!(health_checks.get(), 1, "stale processes are not probed");
        assert_eq!(servers[0].status, "healthy");
        assert_eq!(servers[0].stores, ["/stores/one", "/stores/two"]);
        assert_eq!(servers[1].status, "stale");
        assert_eq!(invalid, [dir.path().join("broken.json")]);
        let rendered = render_server_inventory(&servers, &invalid);
        assert!(rendered.contains("healthy   pid=42"));
        assert!(rendered.contains("stale     pid=99"));
        assert!(rendered.contains("invalid registry record"));
        assert!(!rendered.contains("never-print-me"));
        assert!(!rendered.contains("also-private"));
        assert!(!rendered.contains("secret"));
    }

    #[test]
    fn path_fallback_and_missing_binary_are_explicit() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        let elsewhere = dir.path().join("elsewhere");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&elsewhere).unwrap();
        let cli = bin.join("hotsheet-cli");
        let path_server =
            elsewhere.join(format!("hotsheet-server{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&path_server, "").unwrap();
        assert_eq!(
            resolve_server_binary(&cli, Some(elsewhere.as_os_str())).unwrap(),
            path_server
        );

        let error = resolve_server_binary(&cli, None).unwrap_err().to_string();
        assert!(error.contains("hotsheet-server is not installed"));
        assert!(error.contains("beside hotsheet-cli"));
    }

    #[test]
    fn onboarding_is_read_only_and_distinguishes_hs1_by_its_database_marker() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("project");
        let store = dir.path().join("store");
        std::fs::create_dir_all(project.join(".hotsheet/db")).unwrap();
        std::fs::write(project.join(".hotsheet/db/PG_VERSION"), "17").unwrap();
        let report =
            render_onboarding_report(&store, &project, &[("Claude Code".into(), "claude".into())]);

        assert!(report.contains("AI tools detected"));
        assert!(report.contains("setup claude --project"));
        assert!(report.contains("Hot Sheet 1 PGlite data detected"));
        assert!(report.contains("Close the Hot Sheet 1 project before migrating"));
        assert!(report.contains("hotsheet-migrate"));
        assert!(report.contains("Migration was not started"));
        assert_eq!(
            std::fs::read_to_string(project.join(".hotsheet/db/PG_VERSION")).unwrap(),
            "17"
        );
    }
}
