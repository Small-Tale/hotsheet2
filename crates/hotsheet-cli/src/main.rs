//! `hotsheet` — the Hot Sheet 2 command-line interface. A thin binary over
//! `hotsheet-ticketing`: it reads and writes ticket files directly on disk
//! (`docs/04-core-server-cli.md` §4.4) and imports HS1 exports (`docs/07`).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use hotsheet_cli::{git_init, run_import};
use hotsheet_model::{
    CloseReason, NoteKind, Priority, ReviewKind, ReviewRequest, Status, Ticket, Timestamp, Ulid,
    parse_file, to_file_string,
};
use hotsheet_ticketing::{
    FsStore, GitProvider, KeyRegistry, MutationContext, NewTicket, OsKeychain, Person,
    ProviderConfigRegistry, ProviderDraft, ProviderPatch, ProviderRegistry, Roster, SortKey,
    StoreMetadata, TicketPatch, TicketProvider, TicketQuery, TicketRef, copy_between,
    git_connection_id, move_between, ops,
};
use time::{Duration, OffsetDateTime};

#[derive(Parser)]
#[command(name = "hotsheet", version, about = "Hot Sheet 2 CLI")]
struct Cli {
    /// Store directory (defaults to the current directory).
    #[arg(short = 'C', long = "path", global = true, default_value = ".")]
    path: PathBuf,
    #[command(subcommand)]
    command: Cmd,
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
    /// Link this directory (a code repo) to its **standalone** ticket store, so later
    /// `hotsheet-cli` calls here find it without `-C` (docs/02 §2.8, HS2-5CXKZ0). Writes a
    /// gitignored `.hotsheet/store` pointing at the store's absolute path.
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
    /// Create a new ticket.
    New {
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
    },
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
    /// List tickets from one configured provider connection.
    ProviderLs { connection: String },
    /// Get one provider-native ticket.
    ProviderGet { connection: String, id: String },
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
        /// Edit this provider-native note id instead of appending a note.
        #[arg(long, requires = "note", conflicts_with = "note_kind")]
        edit_note: Option<String>,
        /// Kind for --note: regular | activity | feedback_needed | status.
        #[arg(long, requires = "note")]
        note_kind: Option<String>,
    },
    /// Close a provider-native ticket.
    ProviderClose {
        connection: String,
        id: String,
        #[arg(long)]
        reason: String,
    },
    /// Print a ticket's file by slug or ULID.
    Show { id: String },
    /// Attach a file to a ticket with durable identity and creation time.
    Attach { id: String, file: PathBuf },
    /// Edit a ticket's fields (by slug or ULID).
    Edit {
        id: String,
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
        /// Edit this existing note ULID instead of appending a note.
        #[arg(long, requires = "note", conflicts_with = "note_kind")]
        edit_note: Option<String>,
        /// Kind for --note: regular | activity | feedback_needed | status.
        #[arg(long, requires = "note")]
        note_kind: Option<String>,
    },
    /// Record why a ticket was closed (close outcome; orthogonal to status).
    Close {
        id: String,
        /// completed | not_planned | duplicate | obsolete.
        #[arg(long)]
        reason: String,
        /// The duplicate target (slug or ULID); required when reason=duplicate.
        #[arg(long)]
        duplicate_of: Option<String>,
    },
    /// Set up an AI tool to work with this project, headless (writes its instruction
    /// section, worklist skill, and MCP config). No server or client required.
    Setup {
        /// The tool to set up (e.g. `claude`). Omit together with --detect.
        tool: Option<String>,
        /// Set up every AI tool detected on this machine.
        #[arg(long)]
        detect: bool,
        /// Project directory to write the tool config into (defaults to the store path).
        #[arg(long)]
        project: Option<PathBuf>,
    },
    /// Manage AI-tool plugins (list built-in + installed; install/remove external ones).
    Plugin {
        #[command(subcommand)]
        cmd: PluginCmd,
    },
    /// Read/write core-owned project settings (shared = committed, local = gitignored).
    Settings {
        #[command(subcommand)]
        cmd: SettingsCmd,
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
    /// Import an HS1 `hotsheet-export.json` into the store (creates it if needed).
    Import {
        file: PathBuf,
        /// Prefix used if the store must be created first.
        #[arg(long, default_value = "HS")]
        prefix: String,
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
    /// index looks stale. Writes to the same path the server reads (docs/03 §3.4).
    Reindex {
        /// Index database file (default: ${HOTSHEET_HOME}/index/<project-id>.sqlite).
        #[arg(long)]
        index: Option<PathBuf>,
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
    /// Claude PreToolUse **permission hook** (docs/05 §5.7, HS2-YMR9HE): reads the tool-use
    /// JSON on stdin, asks the running Hot Sheet server (via $HOTSHEET_SERVER/$HOTSHEET_SECRET)
    /// for an allow/deny, and writes the decision to stdout. With no server it emits `ask`
    /// (defer to Claude's normal flow). Register it as a Claude `PreToolUse` hook.
    PermissionHook,
    /// Launch an interactive AI tool in this terminal with permission requests routed to
    /// the running Hot Sheet server. The ticket store is discovered from the current
    /// checkout's `.hotsheet/store` link, so `-C` is normally unnecessary.
    Launch {
        /// The tool to launch (currently `claude`; other tools require a native adapter).
        tool: String,
        /// Project directory in which to run the tool (defaults to the current directory).
        #[arg(long)]
        project: Option<PathBuf>,
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
        #[arg(long)]
        secret: Option<String>,
        /// Stop the running server for this store, then exit.
        #[arg(long)]
        stop: bool,
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
        /// Also transition a Not Started ticket to Started in the same write.
        #[arg(long)]
        start: bool,
    },
    /// Release a claim (only the holding worker, unless --force).
    Release {
        id: String,
        #[arg(long, default_value = "worker")]
        worker: String,
        #[arg(long)]
        force: bool,
    },
    /// Renew a claim's lease (must be the holding worker).
    Renew {
        id: String,
        #[arg(long, default_value = "worker")]
        worker: String,
        #[arg(long, default_value_t = 30)]
        lease_minutes: i64,
    },
    /// Drive a real AI tool for this project (the headless "play"): launch/inject a turn
    /// and stream it. Applies HS2-103 launch safety. No server or client required.
    Trigger {
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
    },
    /// Work the Up Next queue headlessly: drive the tool one turn at a time until Up Next
    /// is drained (or `--max` turns / a thrash stall). Applies HS2-103 launch safety.
    Work {
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
    /// Associate a provider-neutral ticket source with a checkout.
    AddSource {
        reference: String,
        connection_id: String,
        provider: String,
        locator: String,
        #[arg(long)]
        default: bool,
    },
    /// Remove a ticket-source association; removing the default clears it.
    RemoveSource {
        reference: String,
        connection_id: String,
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

/// Filters + sort for `ls` (an in-memory scan; the SQLite/FTS index arrives with HS2-5).
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
    /// Only tickets closed with this reason (completed|not_planned|duplicate|obsolete).
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
    /// Sort key: id | created | updated | priority | status | title.
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
    let mut cli = Cli::parse();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    // Resolve which store to operate on: an explicit -C, else $HOTSHEET_STORE, else a
    // `.hotsheet/store` link walked up from cwd — so a standalone store is found without -C
    // (HS2-5CXKZ0). `init`/`link` operate on the literal path, not a resolved one.
    if !matches!(
        cli.command,
        Cmd::Init { .. } | Cmd::Link { .. } | Cmd::Checkout { .. }
    ) {
        cli.path = hotsheet_cli::resolve_store_path(cli.path, &cwd);
    }
    let refresh = !matches!(
        cli.command,
        Cmd::Init { .. } | Cmd::Link { .. } | Cmd::Checkout { .. }
    );
    let result = match cli.command {
        Cmd::Init {
            prefix,
            standalone,
            at,
            remote,
        } => cmd_init(
            &cli.path,
            &prefix,
            standalone,
            at.as_deref(),
            remote.as_deref(),
        ),
        Cmd::Link { store } => cmd_link(&store),
        Cmd::ActivateFormat {
            acknowledge_pre_release_breakage,
        } => {
            if !acknowledge_pre_release_breakage {
                bail!(
                    "format activation can break older pre-release HS2 processes; announce the change, stop them, then rerun with --acknowledge-pre-release-breakage"
                );
            }
            println!(
                "Activating a pre-release HS2 format boundary. Older HS2 processes may no longer open this store."
            );
            use std::io::Write as _;
            std::io::stdout().flush()?;
            FsStore::open(&cli.path)?.activate_current_format()?;
            println!(
                "Activated store format {}.",
                hotsheet_ticketing::STORE_SCHEMA_VERSION
            );
            Ok(())
        }
        Cmd::New {
            title,
            title_flag,
            category,
            priority,
            details,
            up_next,
            tags,
            blocked_by,
        } => cmd_new(
            &cli.path,
            title.or(title_flag),
            category,
            &priority,
            details,
            up_next,
            tags,
            blocked_by,
        ),
        Cmd::Ls { filters } => cmd_ls(&cli.path, &filters),
        Cmd::Providers { json } => cmd_providers(&cli.path, json),
        Cmd::ProviderLs { connection } => cmd_provider_ls(&cli.path, &connection),
        Cmd::ProviderGet { connection, id } => cmd_provider_get(&cli.path, &connection, &id),
        Cmd::ProviderNew {
            connection,
            title,
            category,
            priority,
            details,
            tags,
        } => cmd_provider_new(
            &cli.path,
            &connection,
            title,
            category,
            &priority,
            details.unwrap_or_default(),
            tags,
        ),
        Cmd::ProviderEdit {
            connection,
            id,
            title,
            details,
            status,
            expected_token,
            note,
            note_kind,
            edit_note,
        } => cmd_provider_edit(
            &cli.path,
            &connection,
            &id,
            ProviderEditInput {
                title,
                details,
                status,
                expected_token,
                note,
                note_kind: parse_note_kind(note_kind.as_deref().unwrap_or("regular"))?,
                edit_note,
            },
        ),
        Cmd::ProviderClose {
            connection,
            id,
            reason,
        } => cmd_provider_close(&cli.path, &connection, &id, &reason),
        Cmd::Show { id } => cmd_show(&cli.path, &id),
        Cmd::Attach { id, file } => cmd_attach(&cli.path, &id, &file),
        Cmd::Edit {
            id,
            title,
            details,
            category,
            priority,
            status,
            tags,
            blocked_by,
            clear_blocked_by,
            blocked_reason,
            clear_blocked_reason,
            up_next,
            no_up_next,
            note,
            note_kind,
            edit_note,
        } => cmd_edit(
            &cli.path,
            &id,
            title,
            details,
            category,
            priority,
            status,
            tags,
            blocked_by,
            clear_blocked_by,
            blocked_reason,
            clear_blocked_reason,
            up_next,
            no_up_next,
            note,
            parse_note_kind(note_kind.as_deref().unwrap_or("regular"))?,
            edit_note,
        ),
        Cmd::Close {
            id,
            reason,
            duplicate_of,
        } => cmd_close(&cli.path, &id, &reason, duplicate_of),
        Cmd::Setup {
            tool,
            detect,
            project,
        } => cmd_setup(&cli.path, tool, detect, project),
        Cmd::Plugin { cmd } => cmd_plugin(cmd),
        Cmd::Settings { cmd } => cmd_settings(&cli.path, cmd),
        Cmd::Key { cmd } => cmd_key(cmd),
        Cmd::Checkout { cmd } => cmd_checkout(cmd),
        Cmd::Import { file, prefix } => cmd_import(&cli.path, &file, &prefix),
        Cmd::Copy { id, to } => cmd_copy(&cli.path, &id, &to),
        Cmd::ProviderCopy {
            id,
            to,
            operation_id,
        } => cmd_provider_transfer(&cli.path, &id, &to, &operation_id, false),
        Cmd::Move { id, to, yes } => cmd_move(&cli.path, &id, &to, yes),
        Cmd::ProviderMove {
            id,
            to,
            operation_id,
            yes,
        } => {
            if !yes {
                bail!("provider-move requires --yes");
            }
            cmd_provider_transfer(&cli.path, &id, &to, &operation_id, true)
        }
        Cmd::Assign {
            id,
            to,
            clear,
            review,
        } => cmd_assign(&cli.path, &id, to, clear, review),
        Cmd::People { cmd } => cmd_people(&cli.path, cmd),
        Cmd::Read { id } => cmd_read(&cli.path, &id),
        Cmd::Sync => cmd_sync(&cli.path),
        Cmd::Doctor { project } => cmd_doctor(&cli.path, &project),
        Cmd::Reindex { index } => cmd_reindex(&cli.path, index),
        Cmd::Worklist => cmd_worklist(&cli.path, &cwd),
        Cmd::Metrics {
            roll_up,
            prune_before,
            team,
        } => cmd_metrics(&cli.path, roll_up, prune_before, team),
        Cmd::PermissionHook => cmd_permission_hook(),
        Cmd::Launch {
            tool,
            project,
            args,
        } => cmd_launch(&cli.path, &cwd, &tool, project, args),
        Cmd::Serve { bind, secret, stop } => cmd_serve(&cli.path, &bind, secret, stop),
        Cmd::Cert { cmd } => cmd_cert(&cli.path, &cmd),
        Cmd::MergeDriver { base, ours, theirs } => cmd_merge_driver(&base, &ours, &theirs),
        Cmd::ClaimNext {
            worker,
            label,
            lease_minutes,
        } => cmd_claim_next(&cli.path, &worker, label, lease_minutes),
        Cmd::Claim {
            id,
            worker,
            label,
            lease_minutes,
            start,
        } => cmd_claim(&cli.path, &id, &worker, label, lease_minutes, start),
        Cmd::Release { id, worker, force } => cmd_release(&cli.path, &id, &worker, force),
        Cmd::Renew {
            id,
            worker,
            lease_minutes,
        } => cmd_renew(&cli.path, &id, &worker, lease_minutes),
        Cmd::Trigger {
            tool,
            prompt,
            project,
            resume,
            mcp_config,
            permission_mode,
            envs,
            worker,
            shared_daemon,
        } => cmd_trigger(
            &cli.path,
            &tool,
            prompt,
            project,
            resume,
            mcp_config,
            permission_mode,
            envs,
            worker,
            shared_daemon,
        ),
        Cmd::Work {
            tool,
            project,
            max,
            max_stall,
            worker,
            shared_daemon,
        } => cmd_work(
            &cli.path,
            &tool,
            project,
            max,
            max_stall,
            worker,
            shared_daemon,
        ),
    };
    if result.is_ok() && refresh {
        refresh_checkout_worklists(&cli.path, &cwd)?;
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

#[allow(clippy::too_many_arguments)]
fn cmd_trigger(
    store_path: &Path,
    tool: &str,
    prompt: Option<String>,
    project: Option<PathBuf>,
    resume: Option<String>,
    mcp_config: Option<PathBuf>,
    permission_mode: Option<String>,
    envs: Vec<String>,
    worker: bool,
    shared_daemon: bool,
) -> Result<()> {
    use hotsheet_aitools::{ConnectionRegistry, DoneReason, prepare_trigger};

    let safe = prepare_trigger(
        store_path,
        tool,
        project,
        mcp_config,
        permission_mode,
        envs,
        shared_daemon,
    )?;
    eprintln!("▶ driving {tool} in {} …", safe.cwd.display());
    let mut registry = ConnectionRegistry::new(30_000);
    let done = safe.run_turn(
        &prompt.unwrap_or_else(|| DEFAULT_TRIGGER_PROMPT.to_string()),
        resume.as_deref(),
        worker,
        format!("cli-{}", std::process::id()),
        &mut registry,
        &mut stream_to_stdout,
    )?;
    println!();
    if let Some(sid) = &done.session_id {
        eprintln!("  session: {sid}");
    }
    match done.reason {
        DoneReason::Completed => {
            eprintln!("✔ {tool} turn completed");
            Ok(())
        }
        DoneReason::Failed(code) => bail!("{tool} turn failed (exit {code})"),
        DoneReason::Interrupted => bail!("{tool} turn interrupted"),
    }
}

/// `hotsheet-cli work <tool>`: drive the tool one turn at a time until Up Next is
/// drained, a turn cap is hit, or the queue stops changing (thrash guard). The
/// north-star headless loop (HS2-118), reusing `trigger`'s HS2-103 launch safety.
#[allow(clippy::too_many_arguments)]
fn cmd_work(
    store_path: &Path,
    tool: &str,
    project: Option<PathBuf>,
    max: u32,
    max_stall: u32,
    worker: bool,
    shared_daemon: bool,
) -> Result<()> {
    use hotsheet_aitools::{ConnectionRegistry, DoneReason, prepare_trigger};
    use hotsheet_cli::workloop::{Stall, queue_signature};

    let store = FsStore::open(store_path)?;
    let up_next_query = || TicketQuery {
        up_next_only: true,
        open_only: true,
        sort: SortKey::Priority,
        ..Default::default()
    };

    // Nothing to do? Exit before requiring setup / building the launch machinery.
    if ops::query(&store, &up_next_query())?.is_empty() {
        eprintln!("✔ Nothing Up Next — nothing to do.");
        return Ok(());
    }

    // The loop is the shared daemon's best case: one isolated CODEX_HOME + one codex
    // instance reused across every turn, torn down when this SafeTrigger drops (HS2-9M6T68).
    let safe = prepare_trigger(store_path, tool, project, None, None, vec![], shared_daemon)?;
    let mut registry = ConnectionRegistry::new(30_000);
    let mut stall = Stall::default();
    let mut completed = 0u32;
    // The tool's session/thread id, captured from turn 1 and passed as `resume` on every
    // subsequent turn so one session carries context across the loop (HS2-3C1XK3).
    let mut session: Option<String> = None;
    for turn in 1..=max {
        let before = ops::query(&store, &up_next_query())?;
        if before.is_empty() {
            eprintln!(
                "✔ Up Next drained after {} turn(s) ({completed} completed).",
                turn - 1
            );
            return Ok(());
        }
        let top = &before[0];
        eprintln!(
            "── turn {turn}/{max}: {} — {} ({} up next) ──",
            top.slug,
            top.title,
            before.len()
        );

        let done = safe.run_turn(
            DEFAULT_TRIGGER_PROMPT_LOOP,
            session.as_deref(),
            worker,
            format!("cli-{}", std::process::id()),
            &mut registry,
            &mut stream_to_stdout,
        )?;
        println!();
        // Remember the session so the next turn resumes it (first non-None wins).
        if session.is_none() {
            session = done.session_id.clone();
        }
        match done.reason {
            DoneReason::Completed => {}
            DoneReason::Failed(code) => eprintln!("⚠ turn {turn} failed (exit {code})"),
            DoneReason::Interrupted => bail!("interrupted during turn {turn}"),
        }

        // Progress = the Up Next queue changed (a ticket left it, or any of them was
        // edited). No change across a whole turn trips the thrash guard.
        let after = ops::query(&store, &up_next_query())?;
        let progressed = queue_signature(&before) != queue_signature(&after);
        if after.len() < before.len() {
            completed += (before.len() - after.len()) as u32;
        }
        let streak = stall.record(progressed);
        if streak >= max_stall {
            bail!(
                "no progress for {max_stall} turns (top ticket {} — {}); stopping. \
                 The tool may be stuck — check it, then re-run.",
                top.slug,
                top.title
            );
        }
    }
    let remaining = ops::query(&store, &up_next_query())?.len();
    bail!("reached --max {max} turns with {remaining} ticket(s) still Up Next");
}

/// The per-turn prompt for the `work` loop: like the trigger default, but explicit that
/// exactly one ticket should be taken this turn (the loop drives the next one).
const DEFAULT_TRIGGER_PROMPT_LOOP: &str = "Read the Hot Sheet Up Next queue (hotsheet tools \
or `hotsheet-cli ls --up-next`) and take ONLY the single highest-priority ticket: set it \
started, implement it, and mark it completed with a note on what you did. Do just that one \
ticket this turn, then stop. If nothing is Up Next, say so and stop.";

fn cmd_init(
    path: &PathBuf,
    prefix: &str,
    standalone: bool,
    at: Option<&Path>,
    remote: Option<&str>,
) -> Result<()> {
    if standalone {
        let project = std::env::current_dir()?;
        let name = project
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|n| !n.is_empty())
            .context("current project directory has no usable name; pass --at <path>")?;
        let destination = at
            .map(PathBuf::from)
            .unwrap_or_else(|| hotsheet_plugins::hotsheet_home().join("stores").join(name));
        if destination.exists() {
            bail!(
                "standalone store destination already exists: {}; choose a new --at path or link the existing store",
                destination.display()
            );
        }
        FsStore::init(&destination, &StoreMetadata::new(prefix))
            .with_context(|| format!("initializing store at {}", destination.display()))?;
        git_init(&destination);
        hotsheet_cli::register_merge_driver(&destination);
        if let Some(url) = remote {
            let status = std::process::Command::new("git")
                .current_dir(&destination)
                .args(["remote", "add", "origin", url])
                .status()
                .context("running git remote add origin")?;
            if !status.success() {
                bail!("git remote add origin exited with {status}");
            }
        }
        let linked = hotsheet_cli::link_store(&destination, &project)?;
        println!(
            "Initialized standalone Hot Sheet store at {} and linked {} via {}",
            linked.display(),
            project.display(),
            hotsheet_cli::STORE_LINK
        );
        print_onboarding_report(&destination, &project);
        return Ok(());
    }

    FsStore::init(path, &StoreMetadata::new(prefix))
        .with_context(|| format!("initializing store at {}", path.display()))?;
    git_init(path);
    hotsheet_cli::register_merge_driver(path);
    println!("Initialized Hot Sheet store at {}", path.display());
    print_onboarding_report(path, path);
    Ok(())
}

/// Link the current directory to a standalone store (`hotsheet-cli link <store>`).
fn cmd_link(store: &Path) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let abs = hotsheet_cli::link_store(store, &cwd)?;
    let registry = hotsheet_ticketing::checkouts::CheckoutRegistry::new(
        hotsheet_plugins::hotsheet_home().join("checkouts.json"),
    );
    let checkout = registry.register(&cwd, None, None, vec![abs.clone()])?;
    hotsheet_ticketing::worklist::regenerate_checkout(&checkout)?;
    println!(
        "Linked {} → {} (via {})",
        cwd.display(),
        abs.display(),
        hotsheet_cli::STORE_LINK
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn cmd_new(
    path: &PathBuf,
    title: Option<String>,
    category: String,
    priority: &str,
    details: Option<String>,
    up_next: bool,
    tags: Vec<String>,
    blocked_by: Vec<String>,
) -> Result<()> {
    let title = title.context("a title is required (positional or --title)")?;
    let store = FsStore::open(path)?;
    let prefix = store.metadata()?.ticket_prefix;
    let blocked_by = ops::resolve_blockers(&store, None, &blocked_by)?;
    let ticket = ops::create(
        &store,
        Ulid::new(),
        &prefix,
        now_ts(),
        NewTicket {
            title,
            category,
            priority: parse_priority(priority)?,
            status: Status::NotStarted,
            details: details.unwrap_or_default(),
            tags,
            up_next,
            blocked_by,
        },
    )?;
    println!(
        "Created {} ({})",
        ticket.slug,
        store.ticket_path(&ticket.id).display()
    );
    Ok(())
}

fn cmd_ls(path: &PathBuf, f: &LsFilters) -> Result<()> {
    let store = FsStore::open(path)?;
    // `me` in a person filter → this store's git user.email; an unresolvable `me` errors
    // rather than silently matching everyone (HS2-TCDTCH, docs/10 §10.3).
    let resolve_person = |v: &Option<String>| -> Result<Option<String>> {
        match v {
            None => Ok(None),
            Some(raw) if raw.eq_ignore_ascii_case(hotsheet_ticketing::ME) => {
                hotsheet_ticketing::current_user_email(path)
                    .map(Some)
                    .ok_or_else(|| anyhow::anyhow!("cannot resolve 'me': no git user.email set"))
            }
            Some(raw) => Ok(Some(raw.clone())),
        }
    };
    let page_after = match &f.page_after {
        Some(s) => Some(
            ops::resolve(&store, s)?
                .ok_or_else(|| anyhow::anyhow!("page-after: no ticket '{s}'"))?
                .id,
        ),
        None => None,
    };
    let query = TicketQuery {
        // Validate enum filters up front so a typo errors instead of matching none.
        status: f.status.as_deref().map(parse_status_str).transpose()?,
        priority: f.priority.as_deref().map(parse_priority).transpose()?,
        category: f.category.clone(),
        tags: f.tags.clone(),
        text: f.text.clone(),
        up_next_only: f.up_next,
        open_only: f.open,
        close_reason: f
            .close_reason
            .as_deref()
            .map(parse_close_reason)
            .transpose()?,
        closed: f.closed.then_some(true),
        assignee: resolve_person(&f.assignee)?,
        review_requested: resolve_person(&f.review_requested)?,
        review_by: resolve_person(&f.review_by)?,
        claimed: f.claimed.then_some(true),
        blocked: f.blocked_filter(),
        sort: f.sort.parse().map_err(|e: String| anyhow::anyhow!(e))?,
        limit: f.limit,
        page_after,
        ..Default::default()
    };
    let tickets = ops::query(&store, &query)?;

    if tickets.is_empty() {
        println!("(no tickets)");
        return Ok(());
    }
    // Per-user unread state comes from the local (gitignored) overlay (HS2-21).
    let overlay = hotsheet_ticketing::LocalOverlay::new(path.clone());
    for t in &tickets {
        let up = if t.up_next { "*" } else { " " };
        let unread = if overlay.is_unread(t).unwrap_or(false) {
            "●"
        } else {
            " "
        };
        println!(
            "{up}{unread} {:<12} {:<12} {}",
            t.slug,
            status_str(t.status),
            t.title
        );
    }
    Ok(())
}

fn cmd_providers(path: &Path, json: bool) -> Result<()> {
    let store = FsStore::open(path)?;
    let connections = ProviderConfigRegistry::new(store.root().join("providers.json")).load()?;
    let external_default = connections.iter().any(|connection| connection.default);
    let descriptor = GitProvider::new(git_connection_id(&store), store.clone())
        .with_default(!external_default)
        .descriptor();
    let mut descriptors = vec![descriptor];
    for connection in connections {
        if connection.provider != "git" {
            descriptors.push(hotsheet_extsync::descriptor(&connection)?);
        }
    }
    descriptors.sort_by(|a, b| a.connection_id.cmp(&b.connection_id));
    if json {
        println!("{}", serde_json::to_string_pretty(&descriptors)?);
    } else {
        for descriptor in descriptors {
            println!(
                "{}  {}  {}  {}",
                descriptor.connection_id,
                descriptor.provider,
                if descriptor.default { "default" } else { "" },
                descriptor.locator
            );
        }
    }
    Ok(())
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
    let credential = hotsheet_extsync::credential_reference(&connection)?;
    let token = KeyRegistry::new(hotsheet_plugins::hotsheet_home(), OsKeychain).get(credential)?;
    Ok(hotsheet_extsync::live_provider(&connection, token)?)
}

fn cmd_provider_ls(path: &Path, connection: &str) -> Result<()> {
    let tickets = configured_provider(path, connection)?.query(&TicketQuery::default())?;
    println!("{}", serde_json::to_string_pretty(&tickets)?);
    Ok(())
}

fn cmd_provider_get(path: &Path, connection: &str, id: &str) -> Result<()> {
    let ticket = configured_provider(path, connection)?.get(id)?;
    println!("{}", serde_json::to_string_pretty(&ticket)?);
    Ok(())
}

fn cmd_provider_new(
    path: &Path,
    connection: &str,
    title: String,
    category: String,
    priority: &str,
    details: String,
    tags: Vec<String>,
) -> Result<()> {
    let priority = parse_priority(priority)?;
    let ticket = configured_provider(path, connection)?.create(
        MutationContext {
            now: now_ts(),
            generated_id: Ulid::new(),
        },
        ProviderDraft {
            title,
            category,
            priority,
            status: Status::NotStarted,
            details,
            tags,
            up_next: false,
            blocked_by: vec![],
            transfer: None,
        },
    )?;
    println!("{}", serde_json::to_string_pretty(&ticket)?);
    Ok(())
}

struct ProviderEditInput {
    title: Option<String>,
    details: Option<String>,
    status: Option<String>,
    expected_token: Option<String>,
    note: Option<String>,
    note_kind: NoteKind,
    edit_note: Option<String>,
}

fn cmd_provider_edit(
    path: &Path,
    connection: &str,
    id: &str,
    input: ProviderEditInput,
) -> Result<()> {
    let provider = configured_provider(path, connection)?;
    if input.edit_note.is_some() && !provider.supports_note_edit() {
        bail!("provider connection '{connection}' does not support note editing");
    }
    let now = now_ts();
    let mut ticket = provider.update(
        id,
        now.clone(),
        ProviderPatch {
            expected_token: input.expected_token,
            title: input.title,
            details: input.details,
            status: input.status.as_deref().map(parse_status_str).transpose()?,
            ..Default::default()
        },
    )?;
    if let Some(note) = input.note {
        ticket = match input.edit_note {
            Some(note_id) => provider.edit_note(id, &note_id, now, note),
            None => provider.add_note(
                id,
                MutationContext {
                    now,
                    generated_id: Ulid::new(),
                },
                input.note_kind,
                note,
            ),
        }?;
    }
    println!("{}", serde_json::to_string_pretty(&ticket)?);
    Ok(())
}

fn cmd_provider_close(path: &Path, connection: &str, id: &str, reason: &str) -> Result<()> {
    let reason = parse_close_reason(reason)?;
    let ticket = configured_provider(path, connection)?.close(id, now_ts(), reason, None)?;
    println!("{}", serde_json::to_string_pretty(&ticket)?);
    Ok(())
}

fn cmd_copy(src_path: &Path, id: &str, to: &Path) -> Result<()> {
    let src = FsStore::open(src_path)?;
    let dest =
        FsStore::open(to).with_context(|| format!("opening destination store {}", to.display()))?;
    let ticket =
        ops::resolve(&src, id)?.ok_or_else(|| anyhow::anyhow!("no ticket matching '{id}'"))?;
    let copied = ops::copy_ticket(&src, &dest, &ticket.id, Ulid::new(), now_ts())?;
    println!(
        "Copied {} → {} in {} (copied_from {})",
        ticket.slug,
        copied.slug,
        to.display(),
        ticket.slug
    );
    Ok(())
}

fn cmd_provider_transfer(
    src_path: &Path,
    id: &str,
    to: &Path,
    operation_id: &str,
    move_source: bool,
) -> Result<()> {
    let source_store = FsStore::open(src_path)?;
    let destination_store =
        FsStore::open(to).with_context(|| format!("opening destination store {}", to.display()))?;
    let source_id = git_connection_id(&source_store);
    let destination_id = git_connection_id(&destination_store);
    let source_ticket = ops::resolve(&source_store, id)?
        .ok_or_else(|| anyhow::anyhow!("no ticket matching '{id}'"))?;
    let registry = ProviderRegistry::default();
    registry.register(Arc::new(GitProvider::new(source_id.clone(), source_store)))?;
    registry.register(Arc::new(GitProvider::new(
        destination_id.clone(),
        destination_store,
    )))?;
    let source = TicketRef {
        connection_id: source_id,
        native_id: source_ticket.id.to_string(),
    };
    let outcome = if move_source {
        move_between(&registry, source, &destination_id, operation_id, now_ts())?
    } else {
        copy_between(&registry, source, &destination_id, operation_id, now_ts())?
    };
    println!(
        "{} {} -> {} (operation {})",
        if move_source { "Moved" } else { "Copied" },
        outcome.source.qualified(),
        outcome.destination.qualified(),
        outcome.operation_id
    );
    Ok(())
}

fn cmd_move(src_path: &Path, id: &str, to: &Path, yes: bool) -> Result<()> {
    let src = FsStore::open(src_path)?;
    let dest =
        FsStore::open(to).with_context(|| format!("opening destination store {}", to.display()))?;
    let ticket =
        ops::resolve(&src, id)?.ok_or_else(|| anyhow::anyhow!("no ticket matching '{id}'"))?;

    // Retention + exposure caveat (docs/02 §2.13) — git never forgets.
    eprintln!(
        "warning: moving {} does NOT remove it from {}'s git history or remote; and moving \
         INTO a shared store exposes it (the sync engine will push it). True purge needs a \
         manual history rewrite.",
        ticket.slug,
        src_path.display()
    );
    if !yes {
        bail!("re-run with --yes to confirm the move (see the retention/exposure warning above)");
    }

    // Until store ids exist (HS2-87), identify the destination by its canonical path.
    let dest_id = std::fs::canonicalize(to)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| to.display().to_string());
    let out = ops::move_ticket(&src, &dest, &ticket.id, &dest_id, now_ts())?;
    println!(
        "Moved {} → {} in {} (tombstone left in {})",
        ticket.slug,
        out.moved.slug,
        to.display(),
        src_path.display()
    );
    Ok(())
}

fn cmd_assign(
    path: &Path,
    id: &str,
    to: Vec<String>,
    clear: bool,
    review: Vec<String>,
) -> Result<()> {
    let store = FsStore::open(path)?;
    let ticket =
        ops::resolve(&store, id)?.ok_or_else(|| anyhow::anyhow!("no ticket matching '{id}'"))?;

    // `--clear` empties the set; otherwise `--to` (if any) replaces it; else leave unchanged.
    let set_assignees = if clear {
        Some(Vec::new())
    } else if !to.is_empty() {
        Some(to)
    } else {
        None
    };

    let now = now_ts();
    let reviews = review
        .iter()
        .map(|spec| {
            let (who, kind) = spec
                .split_once(':')
                .with_context(|| format!("--review expects email:kind, got '{spec}'"))?;
            Ok(ReviewRequest {
                who: who.to_string(),
                kind: parse_review_kind(kind)?,
                by: Ulid::new(),
                at: now.clone(),
                requested_by: None,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let t = ops::assign(&store, &ticket.id, now, set_assignees, reviews)?;
    let names = if t.assignees.is_empty() {
        "(none)".to_string()
    } else {
        t.assignees.join(", ")
    };
    println!(
        "Assigned {} → {names}{}",
        t.slug,
        if t.review_requests.is_empty() {
            String::new()
        } else {
            format!("  ({} review request(s))", t.review_requests.len())
        }
    );
    Ok(())
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

fn cmd_people(path: &Path, cmd: PeopleCmd) -> Result<()> {
    FsStore::open(path)?; // validate it's a store (people.json lives at its root)
    match cmd {
        PeopleCmd::List => {
            let roster = Roster::load(path)?;
            if roster.people.is_empty() {
                println!("(no people)");
            }
            for p in &roster.people {
                println!("{:<32} {}", p.email, p.name.as_deref().unwrap_or(""));
            }
        }
        PeopleCmd::Add {
            email,
            name,
            github,
        } => {
            let mut roster = Roster::load(path)?;
            roster.upsert(Person {
                email: email.clone(),
                name,
                github,
            });
            roster.save(path)?;
            println!("Added {email} to people.json (commit to share it with the team)");
        }
        PeopleCmd::SeedGithub { repo, api_base } => {
            if !repo
                .split_once('/')
                .is_some_and(|(a, b)| !a.is_empty() && !b.is_empty() && !b.contains('/'))
            {
                bail!("--repo expects owner/name");
            }
            let token = hotsheet_ticketing::KeyRegistry::new(
                hotsheet_plugins::hotsheet_home(),
                hotsheet_ticketing::OsKeychain,
            )
            .get("github")?;
            let agent = ureq::AgentBuilder::new().user_agent("hotsheet2").build();
            let get = |url: &str| -> Result<serde_json::Value> {
                let response = agent
                    .get(url)
                    .set("Authorization", &format!("Bearer {token}"))
                    .set("Accept", "application/vnd.github+json")
                    .call()
                    .map_err(|e| anyhow::anyhow!("GitHub API: {e}"))?;
                Ok(serde_json::from_str(&response.into_string()?)?)
            };
            let collaborators = get(&format!(
                "{}/repos/{repo}/collaborators?per_page=100",
                api_base.trim_end_matches('/')
            ))?;
            let mut profiles = Vec::new();
            for login in collaborators
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v.get("login").and_then(serde_json::Value::as_str))
            {
                let v = get(&format!("{}/users/{login}", api_base.trim_end_matches('/')))?;
                profiles.push(hotsheet_ticketing::roster::GitHubProfile {
                    login: login.to_owned(),
                    name: v
                        .get("name")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned),
                    email: v
                        .get("email")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned),
                });
            }
            let mut roster = Roster::load(path)?;
            let (changed, skipped) =
                hotsheet_ticketing::roster::seed_github_profiles(&mut roster, profiles);
            roster.save(path)?;
            println!(
                "Seeded {changed} people from GitHub; skipped {} without public email{}",
                skipped.len(),
                if skipped.is_empty() {
                    String::new()
                } else {
                    format!(": {}", skipped.join(", "))
                }
            );
        }
    }
    Ok(())
}

fn cmd_read(path: &PathBuf, id: &str) -> Result<()> {
    let store = FsStore::open(path)?;
    let ticket =
        ops::resolve(&store, id)?.ok_or_else(|| anyhow::anyhow!("no ticket matching '{id}'"))?;
    hotsheet_ticketing::LocalOverlay::new(path.clone()).mark_read(&ticket.id, &now_ts())?;
    println!("Marked {} read.", ticket.slug);
    Ok(())
}

fn cmd_sync(path: &Path) -> Result<()> {
    use hotsheet_ticketing::SyncReport;
    FsStore::open(path)?; // validate it's a store before touching git
    match hotsheet_ticketing::sync_once(path) {
        SyncReport::NoRemote => println!("Local-only store — no remote to sync."),
        SyncReport::UpToDate => println!("Already up to date."),
        SyncReport::Synced { pulled, pushed } => {
            let mut parts = Vec::new();
            if pulled {
                parts.push("pulled remote changes");
            }
            if pushed {
                parts.push("pushed local commits");
            }
            println!("Synced — {}.", parts.join(" + "));
        }
        SyncReport::Offline => {
            println!("Remote unreachable — kept working locally; the next sync will retry.")
        }
        SyncReport::Conflict => bail!(
            "a ticket had a body change on both sides that couldn't auto-merge — resolve it \
             manually, then commit (the working tree was left clean; nothing was lost)"
        ),
    }
    Ok(())
}

fn cmd_doctor(path: &PathBuf, project: &Path) -> Result<()> {
    let store = FsStore::open(path)?;
    let meta = store.metadata()?;
    // Enumerate resiliently so a single unparseable file is reported as an issue rather
    // than aborting doctor on the first one — doctor is a diagnostic meant to surface
    // every problem, including corruption (HS2-9X9TZD / HS2-PRVPCQ).
    let listing = store.list_tickets_resilient()?;
    let tickets = listing.tickets;

    println!(
        "Store: {} (prefix {}, {} sharding)",
        path.display(),
        meta.ticket_prefix,
        meta.shard
    );
    println!("Tickets: {}", tickets.len());

    let mut issues = 0usize;

    if !listing.corrupt.is_empty() {
        println!("Corrupt: {}", listing.corrupt.len());
        for c in &listing.corrupt {
            let id =
                c.id.map(|i| i.to_string())
                    .unwrap_or_else(|| "unknown".into());
            let slug = c.slug.as_deref().unwrap_or("unknown");
            println!(
                "  ! corrupt ticket {} (id {id}, slug {slug}): {}",
                c.path.display(),
                c.error
            );
            issues += 1;
        }
    }

    let ids: HashSet<Ulid> = tickets.iter().map(|t| t.id).collect();

    let mut slug_counts = std::collections::HashMap::<&str, usize>::new();
    for t in &tickets {
        *slug_counts.entry(t.slug.as_str()).or_default() += 1;
    }
    for (slug, n) in &slug_counts {
        if *n > 1 {
            println!("  ! duplicate slug {slug} ({n} tickets)");
            issues += 1;
        }
    }

    for t in &tickets {
        for b in &t.blocked_by {
            if !ids.contains(b) {
                println!("  ! {} blocked_by unknown id {b}", t.slug);
                issues += 1;
            }
        }
        if let Some(d) = &t.duplicate_of {
            if !ids.contains(d) {
                println!("  ! {} duplicate_of unknown id {d}", t.slug);
                issues += 1;
            }
        }
        if t.close_reason == Some(CloseReason::Duplicate) && t.duplicate_of.is_none() {
            println!("  ! {} close_reason=duplicate but no duplicate_of", t.slug);
            issues += 1;
        }
        if !t.created_at.is_valid() {
            println!(
                "  ! {} created_at not valid RFC3339: {}",
                t.slug, t.created_at
            );
            issues += 1;
        }
    }

    // The semantic merge driver must be registered, or git would text-merge ticket files
    // and could dump conflict markers over structured data (HS2-18, docs/02 §2.7).
    if !hotsheet_cli::merge_driver_registered(path) {
        println!(
            "  ! semantic merge driver not registered (missing .gitattributes \
             `merge=hotsheet-ticket` or its git config) — re-run `hotsheet-cli init` here to \
             register it, or git may text-merge ticket files"
        );
        issues += 1;
    }

    print_onboarding_report(path, project);
    if issues == 0 {
        println!("No issues found.");
        Ok(())
    } else {
        bail!("{issues} issue(s) found")
    }
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
/// CLI and server share one index file (`${HOTSHEET_HOME}/index/<project-id>.sqlite`).
fn default_index_path(store: &FsStore) -> Result<PathBuf> {
    let root = store
        .root()
        .canonicalize()
        .unwrap_or_else(|_| store.root().to_path_buf());
    let id = &hotsheet_index::hash_bytes(root.to_string_lossy().as_bytes())[..16];
    let dir = hotsheet_plugins::hotsheet_home().join("index");
    std::fs::create_dir_all(&dir)?;
    Ok(dir.join(format!("{id}.sqlite")))
}

/// Rebuild the disposable index from a full store walk (`hotsheet-cli reindex`).
fn cmd_reindex(path: &Path, index: Option<PathBuf>) -> Result<()> {
    let store = FsStore::open(path)?;
    let index_path = match index {
        Some(p) => p,
        None => default_index_path(&store)?,
    };
    // Same store_id derivation the server uses (the store root), so the rebuilt file is
    // the one the server reconciles against.
    let idx = hotsheet_index::Index::open(&index_path, store.root().display().to_string())?;
    let n = idx.rebuild_from_store(&store)?;
    println!("reindexed {n} ticket(s) → {}", index_path.display());
    Ok(())
}

/// Show the usage/cost rollup for the store (`hotsheet-cli metrics`, docs/14).
fn cmd_metrics(
    path: &Path,
    roll_up: Option<String>,
    prune_before: Option<String>,
    team: bool,
) -> Result<()> {
    use hotsheet_ticketing::metrics;
    let store = FsStore::open(path)?;

    if let Some(day) = roll_up.as_deref() {
        metrics::roll_up_through(&store, day)?;
        eprintln!("✔ rolled up through {day}");
    }
    if let Some(day) = prune_before.as_deref() {
        let n = metrics::prune_raw_before(&store, day)?;
        eprintln!("✔ pruned {n} settled raw file(s) before {day}");
    }

    // DB-free read path: settled rollup + live tail (or the cross-contributor team sum).
    let r = if team {
        metrics::team_summary(&store)?
    } else {
        metrics::summary_settled(&store)?
    };
    println!(
        "Usage{}: {} events · {} tokens in / {} out · ${:.4} total",
        if team { " (team)" } else { "" },
        r.events,
        r.tokens_in,
        r.tokens_out,
        r.cost_usd
    );
    if !r.by_model.is_empty() {
        println!("By model:");
        for (m, c) in &r.by_model {
            println!("  {m}: ${c:.4}");
        }
    }
    if !r.by_day.is_empty() {
        println!("By day:");
        for (d, c) in &r.by_day {
            println!("  {d}: ${c:.4}");
        }
    }
    Ok(())
}

/// Claude PreToolUse permission hook (HS2-YMR9HE): stdin tool-use JSON → ask the running
/// server → stdout decision. Always exits 0 (a hook failure must not wedge the tool); the
/// decision word carries allow/deny/ask. Fail-safe: any error → `ask` (defer to Claude).
fn cmd_permission_hook() -> Result<()> {
    use hotsheet_cli::permission_hook::{
        HookDecision, decision_from_server, hook_connection, hook_decision_json, hook_tool_action,
    };
    let input: serde_json::Value =
        serde_json::from_reader(std::io::stdin()).unwrap_or(serde_json::Value::Null);

    let decision = match (
        std::env::var("HOTSHEET_SERVER").ok(),
        std::env::var("HOTSHEET_SECRET").ok(),
    ) {
        // Governed by a Hot Sheet server: raise a blocking request and honor the answer.
        (Some(url), Some(secret)) => {
            let (tool, action) = hook_tool_action(&input);
            let connection = hook_connection(&input);
            let project = std::env::var("HOTSHEET_PROJECT").unwrap_or_default();
            match ask_server(&url, &secret, &project, &connection, &tool, &action) {
                Ok(reply) => decision_from_server(&reply),
                // Server unreachable / error → defer to Claude rather than block it.
                Err(_) => HookDecision::Ask,
            }
        }
        // Not a Hot Sheet-governed run → defer to Claude's normal permission flow.
        _ => HookDecision::Ask,
    };
    println!("{}", hook_decision_json(decision));
    Ok(())
}

/// Set up and replace this process with a hook-capable interactive AI tool. Because the
/// child inherits the ordinary terminal, this behaves like invoking the tool directly.
fn cmd_launch(
    store: &Path,
    cwd: &Path,
    tool: &str,
    project: Option<PathBuf>,
    args: Vec<String>,
) -> Result<()> {
    let project = project.unwrap_or_else(|| cwd.to_path_buf());
    hotsheet_cli::setup::run_setup(store, &project, Some(tool), false)?;
    let launch = hotsheet_cli::external_launch::prepare(
        store,
        tool,
        args,
        &hotsheet_plugins::hotsheet_home(),
    )?;
    let program = hotsheet_aitools::launch_safety::resolve_program(&launch.program)?;
    let permission_project = FsStore::open(store)
        .map(|store| store.root().display().to_string())
        .unwrap_or_else(|_| store.display().to_string());
    let mut command = std::process::Command::new(&program);
    command
        .args(&launch.args)
        .current_dir(&project)
        .env("HOTSHEET_SERVER", &launch.server.url)
        .env("HOTSHEET_SECRET", &launch.server.secret)
        .env("HOTSHEET_PROJECT", permission_project);

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        Err(command.exec().into())
    }
    #[cfg(not(unix))]
    {
        let status = command.status()?;
        if status.success() {
            Ok(())
        } else {
            bail!("{} exited with {status}", program.display())
        }
    }
}

/// POST the ask to the server's `/permissions/ask`, returning its JSON reply.
fn ask_server(
    url: &str,
    secret: &str,
    project: &str,
    connection: &str,
    tool: &str,
    action: &str,
) -> Result<serde_json::Value> {
    let endpoint = format!("{}/permissions/ask", url.trim_end_matches('/'));
    let body = serde_json::json!({
        "project": project, "connection": connection, "tool": tool, "action": action,
    })
    .to_string();
    let text = ureq::post(&endpoint)
        .set("X-Hotsheet-Secret", secret)
        .set("Content-Type", "application/json")
        .send_string(&body)?
        .into_string()?;
    Ok(serde_json::from_str(&text)?)
}

/// Regenerate the checkout-local worklist from all sources registered for this checkout.
fn cmd_worklist(path: &Path, cwd: &Path) -> Result<()> {
    let store = FsStore::open(path)?;
    let checkout_root = local_checkout_root(path, cwd);
    let output = checkout_root.join(hotsheet_ticketing::worklist::CHECKOUT_WORKLIST);
    let registry = hotsheet_ticketing::checkouts::CheckoutRegistry::new(
        hotsheet_plugins::hotsheet_home().join("checkouts.json"),
    );
    let registered = registry.list()?.into_iter().find(|checkout| {
        Path::new(&checkout.root).canonicalize().ok().as_ref()
            == checkout_root.canonicalize().ok().as_ref()
    });
    let n = if let Some(checkout) = registered {
        hotsheet_ticketing::worklist::regenerate_checkout(&checkout)?
    } else {
        hotsheet_ticketing::worklist::regenerate_to(&store, &output)?
    };
    println!("wrote {} ({n} Up Next ticket(s))", output.display());
    Ok(())
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
    for checkout in registry.list()? {
        let matches = checkout.stores.iter().any(|candidate| {
            Path::new(candidate)
                .canonicalize()
                .unwrap_or_else(|_| PathBuf::from(candidate))
                == store_path
        });
        if matches {
            refreshed_current |= Path::new(&checkout.root).canonicalize().ok().as_ref()
                == cwd.canonicalize().ok().as_ref();
            hotsheet_ticketing::worklist::regenerate_checkout(&checkout)?;
        }
    }
    if !refreshed_current && FsStore::open(&store_path).is_ok() {
        let checkout_root = local_checkout_root(&store_path, cwd);
        hotsheet_ticketing::worklist::regenerate_to(
            &FsStore::open(&store_path)?,
            &checkout_root.join(hotsheet_ticketing::worklist::CHECKOUT_WORKLIST),
        )?;
    }
    Ok(())
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

/// Run the server for this store in the foreground by exec'ing the sibling
/// `hotsheet-server` binary (the CLI stays free of a server dependency). Detached +
/// supervised start is client-owned (HS2-59 / HS2-4072GM).
fn cmd_serve(path: &Path, bind: &str, secret: Option<String>, stop: bool) -> Result<()> {
    let current = std::env::current_exe().context("could not locate hotsheet-cli")?;
    let exe = resolve_server_binary(&current, std::env::var_os("PATH").as_deref())?;
    verify_server_version(&exe)?;

    let mut cmd = std::process::Command::new(&exe);
    cmd.args(server_args(path, bind, secret.as_deref(), stop));
    let status = cmd.status().map_err(|e| {
        anyhow::anyhow!(
            "could not launch `{}`: {e} — is hotsheet-server installed alongside hotsheet-cli?",
            exe.display()
        )
    })?;
    if !status.success() {
        bail!("hotsheet-server exited with {status}");
    }
    Ok(())
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
    let output = std::process::Command::new(exe)
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
    args
}

/// Manage the project's Tier-1 mTLS material (HS2-VT3JMF): a per-project CA, per-device
/// client certs, and revocation. Lives under `${HOTSHEET_HOME}/tls/<project-id>/`.
fn cmd_cert(store_path: &Path, cmd: &CertCmd) -> Result<()> {
    let paths = hotsheet_tls::Paths::for_store(store_path);
    match cmd {
        CertCmd::Init { hosts } => {
            hotsheet_tls::init_ca(&paths, hosts)?;
            println!(
                "Initialized mTLS for this store under {}",
                paths.dir.display()
            );
            println!("  CA cert:     {}", paths.ca_cert().display());
            println!("  server cert: {}", paths.server_cert().display());
            print!("  server SANs: localhost, 127.0.0.1");
            for h in hosts {
                print!(", {h}");
            }
            println!();
            println!("Next: `hotsheet-cli cert issue <device-name>` to enroll a client, then");
            println!("bind the server off-loopback (e.g. `serve --bind 0.0.0.0:8787`).");
        }
        CertCmd::Issue { name } => {
            let dev = hotsheet_tls::issue_device(&paths, name)?;
            eprintln!(
                "Issued client cert for '{}' (fingerprint {}). Copy all three PEMs to the device:",
                dev.name, dev.fingerprint
            );
            println!("# ===== client certificate ({}.crt) =====", dev.name);
            print!("{}", dev.cert_pem);
            println!("# ===== client private key ({}.key) =====", dev.name);
            print!("{}", dev.key_pem);
            println!("# ===== project CA (ca.crt — to verify the server) =====");
            print!("{}", dev.ca_pem);
        }
        CertCmd::Renew { name } => {
            let dev = hotsheet_tls::renew_device(&paths, name)?;
            eprintln!(
                "Renewed '{}' with a fresh 90-day certificate (fingerprint {}). The old certificate is revoked:",
                dev.name, dev.fingerprint
            );
            println!("# ===== client certificate ({}.crt) =====", dev.name);
            print!("{}", dev.cert_pem);
            println!("# ===== client private key ({}.key) =====", dev.name);
            print!("{}", dev.key_pem);
            println!("# ===== project CA (ca.crt — to verify the server) =====");
            print!("{}", dev.ca_pem);
        }
        CertCmd::Role { name, role } => {
            let role = match role.as_str() {
                "read-only" | "read_only" => hotsheet_tls::DeviceRole::ReadOnly,
                "read-write" | "read_write" => hotsheet_tls::DeviceRole::ReadWrite,
                "deny" => hotsheet_tls::DeviceRole::Deny,
                _ => bail!("role must be read-only, read-write, or deny"),
            };
            let fpr = hotsheet_tls::set_device_role(&paths, name, role)?;
            println!("Set '{name}' ({fpr}) to {role:?}. This applies on the next request.");
        }
        CertCmd::Revoke { name } => {
            let fpr = hotsheet_tls::revoke_device(&paths, name)?;
            println!("Revoked '{name}' ({fpr}). This applies on the next TLS connection.");
        }
    }
    Ok(())
}

/// The `merge=hotsheet-ticket` git driver: read git's base/ours/theirs, merge semantically,
/// write the result back to `ours` (git's `%A`). Exit 0 = clean, 1 = a body conflict git
/// should surface. Parse failure falls back to git's text merge so data is never lost.
fn cmd_merge_driver(base: &Path, ours: &Path, theirs: &Path) -> Result<()> {
    let read = |p: &Path| std::fs::read_to_string(p).unwrap_or_default();
    let (base_txt, ours_txt, theirs_txt) = (read(base), read(ours), read(theirs));

    let (b, o, t) = match (
        parse_file(&base_txt),
        parse_file(&ours_txt),
        parse_file(&theirs_txt),
    ) {
        (Ok(b), Ok(o), Ok(t)) => (b, o, t),
        _ => {
            // Unparseable input → git's plain text 3-way merge (never silent data loss).
            let (merged, clean) = git_merge_file(&ours_txt, &base_txt, &theirs_txt)?;
            std::fs::write(ours, merged)?;
            std::process::exit(if clean { 0 } else { 1 });
        }
    };

    let outcome = hotsheet_ticketing::merge_tickets(&b, &o, &t);
    let mut ticket = outcome.ticket;
    let clean = match &outcome.body {
        hotsheet_ticketing::BodyMerge::Resolved(_) => true,
        hotsheet_ticketing::BodyMerge::Conflict {
            base: bb,
            ours: oo,
            theirs: tt,
        } => {
            // Only the prose paragraph conflicts; frontmatter + notes stay cleanly merged.
            let (body, clean) = git_merge_file(oo, bb, tt)?;
            ticket.details = body;
            clean
        }
    };
    std::fs::write(ours, to_file_string(&ticket))?;
    std::process::exit(if clean { 0 } else { 1 });
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
    let out = std::process::Command::new("git")
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

fn cmd_claim_next(
    path: &PathBuf,
    worker: &str,
    label: Option<String>,
    lease_minutes: i64,
) -> Result<()> {
    let store = FsStore::open(path)?;
    let now_dt = OffsetDateTime::now_utc();
    let now = Timestamp::from_datetime(now_dt);
    let lease = lease_until(now_dt, lease_minutes);
    match ops::claim_next(&store, &now, lease, worker, label)? {
        Some(ticket) => println!(
            "Claimed {} for {worker} (lease {lease_minutes}m)",
            ticket.slug
        ),
        None => println!("No claimable tickets."),
    }
    Ok(())
}

fn cmd_claim(
    path: &PathBuf,
    id: &str,
    worker: &str,
    label: Option<String>,
    lease_minutes: i64,
    start: bool,
) -> Result<()> {
    let store = FsStore::open(path)?;
    let ticket = resolve(&store, id)?;
    let now_dt = OffsetDateTime::now_utc();
    let now = Timestamp::from_datetime(now_dt);
    let lease = lease_until(now_dt, lease_minutes);
    let claimed = if start {
        ops::claim_and_start(&store, &ticket.id, &now, lease, worker, label)?
    } else {
        ops::claim(&store, &ticket.id, &now, lease, worker, label)?
    };
    println!(
        "Claimed {} for {worker} (lease {lease_minutes}m)",
        claimed.slug
    );
    Ok(())
}

fn cmd_release(path: &PathBuf, id: &str, worker: &str, force: bool) -> Result<()> {
    let store = FsStore::open(path)?;
    let ticket = resolve(&store, id)?;
    let released = ops::release(&store, &ticket.id, now_ts(), worker, force)?;
    println!("Released {}", released.slug);
    Ok(())
}

fn cmd_renew(path: &PathBuf, id: &str, worker: &str, lease_minutes: i64) -> Result<()> {
    let store = FsStore::open(path)?;
    let ticket = resolve(&store, id)?;
    let lease = lease_until(OffsetDateTime::now_utc(), lease_minutes);
    let renewed = ops::renew(&store, &ticket.id, now_ts(), lease, worker)?;
    println!("Renewed {} (lease {lease_minutes}m)", renewed.slug);
    Ok(())
}

fn cmd_show(path: &PathBuf, needle: &str) -> Result<()> {
    let store = FsStore::open(path)?;
    let ticket = resolve(&store, needle)?;
    print!("{}", to_file_string(&ticket));
    Ok(())
}

fn cmd_attach(path: &PathBuf, needle: &str, file: &Path) -> Result<()> {
    let store = FsStore::open(path)?;
    let ticket = resolve(&store, needle)?;
    let filename = file
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow::anyhow!("attachment path has no UTF-8 filename"))?;
    let bytes = std::fs::read(file)?;
    let attachment_id = Ulid::new();
    let (_, written) =
        store.write_attachment(&ticket.id, attachment_id, now_ts(), filename, &bytes)?;
    println!("Attached {attachment_id} ({})", written.display());
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn cmd_edit(
    path: &PathBuf,
    id: &str,
    title: Option<String>,
    details: Option<String>,
    category: Option<String>,
    priority: Option<String>,
    status: Option<String>,
    tags: Vec<String>,
    blocked_by: Vec<String>,
    clear_blocked_by: bool,
    blocked_reason: Option<String>,
    clear_blocked_reason: bool,
    up_next: bool,
    no_up_next: bool,
    note: Option<String>,
    note_kind: NoteKind,
    edit_note: Option<String>,
) -> Result<()> {
    let store = FsStore::open(path)?;
    let ticket = resolve(&store, id)?;
    let edit_note = edit_note
        .map(|note_id| {
            Ulid::from_string(&note_id)
                .map_err(|_| anyhow::anyhow!("invalid note ULID '{note_id}'"))
        })
        .transpose()?;
    if let Some(note_id) = edit_note
        && !ticket.notes.iter().any(|note| note.id == note_id)
    {
        bail!("note '{note_id}' was not found on ticket '{}'", ticket.slug);
    }
    let up_next = if up_next {
        Some(true)
    } else if no_up_next {
        Some(false)
    } else {
        None
    };
    // Present (non-empty) --blocked-by replaces the set; --clear-blocked-by empties it;
    // neither leaves it unchanged.
    let blocked_by = if clear_blocked_by {
        Some(Vec::new())
    } else if blocked_by.is_empty() {
        None
    } else {
        Some(ops::resolve_blockers(
            &store,
            Some(&ticket.id),
            &blocked_by,
        )?)
    };
    let patch = TicketPatch {
        title,
        details,
        category,
        priority: priority.as_deref().map(parse_priority).transpose()?,
        status: status.as_deref().map(parse_status_str).transpose()?,
        tags: (!tags.is_empty()).then_some(tags),
        up_next,
        blocked_by,
        blocked_reason: if clear_blocked_reason {
            Some(None)
        } else {
            blocked_reason.map(Some)
        },
    };
    let updated = ops::update(&store, &ticket.id, now_ts(), patch)?;
    if let Some(text) = note.filter(|t| !t.is_empty()) {
        if let Some(note_id) = edit_note {
            ops::edit_note(&store, &ticket.id, &note_id, now_ts(), text)?;
        } else {
            ops::add_note(&store, &ticket.id, Ulid::new(), now_ts(), note_kind, text)?;
        }
    }
    println!("Updated {}", updated.slug);
    Ok(())
}

fn cmd_close(path: &PathBuf, id: &str, reason: &str, duplicate_of: Option<String>) -> Result<()> {
    let store = FsStore::open(path)?;
    let ticket = resolve(&store, id)?;
    let reason_enum = parse_close_reason(reason)?;
    let dup = match duplicate_of {
        Some(d) => Some(resolve(&store, &d)?.id),
        None => None,
    };
    let closed = ops::close(&store, &ticket.id, now_ts(), reason_enum, dup)?;
    println!("Closed {} ({reason})", closed.slug);
    Ok(())
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
        other => bail!("invalid close reason '{other}' (completed|not_planned|duplicate|obsolete)"),
    })
}

fn cmd_setup(
    store: &Path,
    tool: Option<String>,
    detect: bool,
    project: Option<PathBuf>,
) -> Result<()> {
    let project_dir = project.unwrap_or_else(|| store.to_path_buf());
    // Setup also makes the checkout discoverable to server/MCP consumers. This records
    // context; generated MCP config still points directly at the store for headless use.
    let repository = std::process::Command::new("git")
        .args([
            "-C",
            project_dir.to_str().unwrap_or("."),
            "config",
            "--get",
            "remote.origin.url",
        ])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty());
    hotsheet_ticketing::checkouts::CheckoutRegistry::new(
        hotsheet_plugins::hotsheet_home().join("checkouts.json"),
    )
    .register(&project_dir, None, repository, vec![store.to_path_buf()])?;
    let reports = hotsheet_cli::run_setup(store, &project_dir, tool.as_deref(), detect)?;
    for r in &reports {
        println!("Set up {} in {}:", r.tool, project_dir.display());
        for w in &r.wrote {
            println!("  wrote {w}");
        }
    }
    Ok(())
}

fn cmd_plugin(cmd: PluginCmd) -> Result<()> {
    use hotsheet_plugins::{PluginSource, all_plugins, default_dirs, machine_plugins_dir};
    match cmd {
        PluginCmd::List => {
            for p in all_plugins(&default_dirs()) {
                let src = match &p.source {
                    PluginSource::BuiltIn => "built-in".to_string(),
                    PluginSource::Disk(path) => path.display().to_string(),
                };
                let detected = p
                    .manifest
                    .detection
                    .binaries
                    .iter()
                    .any(|b| binary_on_path(b));
                println!(
                    "{:<10} {:<16} {:<9} {}",
                    p.id(),
                    p.manifest.product_name,
                    if detected { "detected" } else { "-" },
                    src
                );
            }
        }
        PluginCmd::Info { id } => {
            let p =
                hotsheet_plugins::find(&id).with_context(|| format!("unknown plugin '{id}'"))?;
            println!("{}", hotsheet_cli::plugin::describe(&p));
            let issues = hotsheet_cli::plugin::verify(&p);
            if issues.is_empty() {
                println!("  verify:     ok");
            } else {
                for i in &issues {
                    println!("  verify:     ! {i}");
                }
            }
        }
        PluginCmd::Verify { id } => {
            let p =
                hotsheet_plugins::find(&id).with_context(|| format!("unknown plugin '{id}'"))?;
            let issues = hotsheet_cli::plugin::verify(&p);
            if issues.is_empty() {
                println!("Plugin '{id}' passed verification.");
            } else {
                for i in &issues {
                    eprintln!("  ! {i}");
                }
                bail!(
                    "plugin '{id}' failed verification ({} issue(s))",
                    issues.len()
                );
            }
        }
        PluginCmd::Install { dir, yes } => {
            // Trust gate: disclose what the plugin writes + launches before installing.
            let p = hotsheet_plugins::Plugin::from_fs_dir(&dir)
                .with_context(|| format!("not a valid plugin directory: {}", dir.display()))?;
            println!("About to install this plugin:");
            println!("{}", hotsheet_cli::plugin::describe(&p));
            let issues = hotsheet_cli::plugin::verify(&p);
            if !issues.is_empty() {
                for i in &issues {
                    eprintln!("  ! {i}");
                }
                bail!("refusing to install: plugin failed verification");
            }
            if !yes && !confirm("Install this plugin? [y/N] ")? {
                println!("Aborted.");
                return Ok(());
            }
            let dest = machine_plugins_dir();
            let id = hotsheet_cli::plugin::install(&dir, &dest)?;
            println!("Installed plugin '{id}' into {}", dest.join(&id).display());
            eprintln!(
                "note: an installed plugin's code is not yet sandboxed (behavioral boundary: HS2-93)"
            );
        }
        PluginCmd::Remove { id } => {
            if hotsheet_cli::plugin::remove(&id, &machine_plugins_dir())? {
                println!("Removed plugin '{id}'");
            } else {
                println!("No installed plugin '{id}'");
            }
        }
    }
    Ok(())
}

fn cmd_settings(store: &Path, cmd: SettingsCmd) -> Result<()> {
    use hotsheet_ticketing::{Scope, Settings};
    let parse_scope = |s: &Option<String>| -> Result<Option<Scope>> {
        match s.as_deref() {
            None => Ok(None),
            Some("global") => Ok(Some(Scope::Global)),
            Some("shared") => Ok(Some(Scope::Shared)),
            Some("local") => Ok(Some(Scope::Local)),
            Some(other) => bail!("invalid scope '{other}' (global|shared|local)"),
        }
    };
    let settings = Settings::new(store);
    match cmd {
        SettingsCmd::Get { key, scope } => {
            let value = match parse_scope(&scope)? {
                Some(s) => settings.get(&key, s)?,
                None => settings.get_effective(&key)?,
            };
            match value {
                Some(v) => println!("{}", render_value(&v)),
                None => bail!("no setting '{key}'"),
            }
        }
        SettingsCmd::Set { key, value, scope } => {
            let scope = parse_scope(&scope)?.unwrap_or(Scope::Shared);
            // Parse as JSON (numbers/bools/arrays/objects), else store the raw string.
            let parsed = serde_json::from_str(&value).unwrap_or(serde_json::Value::String(value));
            settings.set(&key, parsed, scope)?;
            let where_ = match scope {
                Scope::Global => "global (machine-wide)",
                Scope::Shared => "shared (committed)",
                Scope::Local => "local (gitignored)",
            };
            println!("Set {key} in {where_}");
        }
        SettingsCmd::List { scope } => {
            let map = match parse_scope(&scope)? {
                Some(s) => settings.map(s)?,
                None => settings.effective()?,
            };
            if map.is_empty() {
                println!("(no settings)");
            }
            for (k, v) in &map {
                println!("{k} = {}", render_value(v));
            }
        }
    }
    Ok(())
}

fn cmd_key(cmd: KeyCmd) -> Result<()> {
    use std::io::IsTerminal;

    use hotsheet_ticketing::{KeyRegistry, OsKeychain};

    let registry = KeyRegistry::new(hotsheet_plugins::hotsheet_home(), OsKeychain);
    match cmd {
        KeyCmd::Set { provider } => {
            let stdin = std::io::stdin();
            let secret = read_key_secret(&provider, stdin.is_terminal(), stdin.lock(), |prompt| {
                rpassword::prompt_password(prompt).map_err(Into::into)
            })?;
            registry.set(&provider, &secret)?;
            println!("Stored key for {provider} in the OS credential store");
        }
        KeyCmd::Get { provider } => println!("{}", registry.get(&provider)?),
        KeyCmd::List => {
            let keys = registry.list()?;
            if keys.is_empty() {
                println!("(no registered keys)");
            }
            for key in keys {
                println!("{} (fallback: {})", key.provider, key.env);
            }
        }
        KeyCmd::Delete { provider } => {
            if registry.delete(&provider)? {
                println!("Deleted key for {provider}");
            } else {
                println!("No registered key for {provider}");
            }
        }
    }
    Ok(())
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

fn cmd_checkout(cmd: CheckoutCmd) -> Result<()> {
    use hotsheet_ticketing::checkouts::CheckoutRegistry;
    let registry = CheckoutRegistry::new(hotsheet_plugins::hotsheet_home().join("checkouts.json"));
    match cmd {
        CheckoutCmd::Register {
            root,
            alias,
            stores,
            repository,
        } => {
            let repository = repository.or_else(|| {
                std::process::Command::new("git")
                    .args(["-C", root.to_str()?, "config", "--get", "remote.origin.url"])
                    .output()
                    .ok()
                    .filter(|o| o.status.success())
                    .and_then(|o| String::from_utf8(o.stdout).ok())
                    .map(|v| v.trim().to_owned())
                    .filter(|v| !v.is_empty())
            });
            let entry = registry.register(&root, alias.as_deref(), repository, stores)?;
            println!("{}", serde_json::to_string_pretty(&entry)?);
        }
        CheckoutCmd::List => println!("{}", serde_json::to_string_pretty(&registry.list()?)?),
        CheckoutCmd::Resolve { reference } => {
            println!(
                "{}",
                serde_json::to_string_pretty(&registry.resolve(&reference)?)?
            );
        }
        CheckoutCmd::AddSource {
            reference,
            connection_id,
            provider,
            locator,
            default,
        } => {
            let source = if provider == "git" {
                let source = hotsheet_ticketing::checkouts::TicketSource::git(&locator);
                if source.connection_id != connection_id {
                    bail!("git source id must be {}", source.connection_id);
                }
                source
            } else {
                hotsheet_ticketing::checkouts::TicketSource {
                    connection_id,
                    provider,
                    locator,
                }
            };
            println!(
                "{}",
                serde_json::to_string_pretty(&registry.add_source(&reference, source, default)?)?
            );
        }
        CheckoutCmd::RemoveSource {
            reference,
            connection_id,
        } => println!(
            "{}",
            serde_json::to_string_pretty(&registry.remove_source(&reference, &connection_id)?)?
        ),
        CheckoutCmd::SetDefault {
            reference,
            connection_id,
            clear,
        } => {
            let selected = if clear {
                None
            } else {
                connection_id.as_deref()
            };
            println!(
                "{}",
                serde_json::to_string_pretty(&registry.set_default_source(&reference, selected)?)?
            );
        }
    }
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

fn cmd_import(path: &Path, file: &Path, prefix: &str) -> Result<()> {
    let summary = run_import(path, file, prefix)?;
    println!(
        "Imported {} ticket(s) ({} attachment file(s)), skipped {} already present.",
        summary.written, summary.attachments, summary.skipped
    );
    Ok(())
}

// ---- helpers ---------------------------------------------------------------------

/// Resolve a ticket by ULID or slug, erroring if there's no match.
fn resolve(store: &FsStore, needle: &str) -> Result<Ticket> {
    ops::resolve(store, needle)?.with_context(|| format!("no ticket matching '{needle}'"))
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

fn status_str(s: Status) -> String {
    serde_json::to_value(s)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default()
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
            cmd_provider_transfer(
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
            server_args(Path::new("store path"), "127.0.0.1:0", Some("secret"), true),
            [
                "-C",
                "store path",
                "--bind",
                "127.0.0.1:0",
                "--secret",
                "secret",
                "--stop"
            ]
            .map(std::ffi::OsString::from)
        );
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
