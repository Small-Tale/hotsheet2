//! `hotsheet` — the Hot Sheet 2 command-line interface. A thin binary over
//! `hotsheet-ticketing`: it reads and writes ticket files directly on disk
//! (`docs/04-core-server-cli.md` §4.4) and imports HS1 exports (`docs/07`).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use hotsheet_cli::{git_init, run_import};
use hotsheet_model::{
    CloseReason, NoteKind, Priority, Status, Ticket, Timestamp, Ulid, to_file_string,
};
use hotsheet_ticketing::{
    FsStore, NewTicket, SortKey, StoreMetadata, TicketPatch, TicketQuery, ops,
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
    /// Initialize a new git-backed store here.
    Init {
        /// Display prefix for ticket slugs (e.g. HS → HS-7F3K9Q).
        #[arg(long, default_value = "HS")]
        prefix: String,
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
    /// Print a ticket's file by slug or ULID.
    Show { id: String },
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
        /// Mark Up Next.
        #[arg(long, conflicts_with = "no_up_next")]
        up_next: bool,
        /// Clear Up Next.
        #[arg(long)]
        no_up_next: bool,
        /// Append a note to the ticket.
        #[arg(long)]
        note: Option<String>,
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
    /// Import an HS1 `hotsheet-export.json` into the store (creates it if needed).
    Import {
        file: PathBuf,
        /// Prefix used if the store must be created first.
        #[arg(long, default_value = "HS")]
        prefix: String,
    },
    /// Check store health (metadata, parse errors, duplicate slugs, orphans).
    Doctor,
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
    },
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
enum SettingsCmd {
    /// Read a setting (effective = local over shared, unless --scope is given).
    Get {
        key: String,
        /// shared | local (default: the effective value).
        #[arg(long)]
        scope: Option<String>,
    },
    /// Write a setting. Value is parsed as JSON if possible, else stored as a string.
    Set {
        key: String,
        value: String,
        /// shared | local (default: shared).
        #[arg(long)]
        scope: Option<String>,
    },
    /// List settings (effective, unless --scope is given).
    List {
        #[arg(long)]
        scope: Option<String>,
    },
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
    /// Sort key: id | created | updated | priority | status | title.
    #[arg(long, default_value = "id")]
    sort: String,
    /// Cap the number of rows shown (after sort).
    #[arg(long)]
    limit: Option<usize>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Cmd::Init { prefix } => cmd_init(&cli.path, &prefix),
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
        Cmd::Show { id } => cmd_show(&cli.path, &id),
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
            up_next,
            no_up_next,
            note,
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
            up_next,
            no_up_next,
            note,
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
        Cmd::Import { file, prefix } => cmd_import(&cli.path, &file, &prefix),
        Cmd::Doctor => cmd_doctor(&cli.path),
        Cmd::ClaimNext {
            worker,
            label,
            lease_minutes,
        } => cmd_claim_next(&cli.path, &worker, label, lease_minutes),
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
        ),
        Cmd::Work {
            tool,
            project,
            max,
            max_stall,
            worker,
        } => cmd_work(&cli.path, &tool, project, max, max_stall, worker),
    }
}

/// The default "play" prompt: work the top Up Next ticket end to end, headless.
const DEFAULT_TRIGGER_PROMPT: &str = "Read the Hot Sheet Up Next queue (hotsheet tools or \
`hotsheet-cli ls --up-next`), take the highest-priority ticket, set it started, implement \
it, and mark it completed with a note on what you did. If nothing is Up Next, say so and \
stop.";

/// A launch-safe way to drive a tool one turn at a time. Built once (assembling the
/// HS2-103 safety — PATH shim, MCP isolation, HS1 guard), then reused for each turn so
/// `trigger` (one turn) and `work` (a loop) share exactly the same safety.
struct SafeTrigger {
    plugin: hotsheet_plugins::Plugin,
    cwd: PathBuf,
    env: Vec<(String, String)>,
    mcp_config: Option<PathBuf>,
    permission_mode: String,
    // Kept alive so the shim dir survives every turn; dropped when the SafeTrigger is.
    _shim: hotsheet_cli::launch_safety::ShimDir,
}

/// Resolve the tool, assemble the HS2-103 launch safety, and return a reusable
/// [`SafeTrigger`]. Fails (before launching anything) on the preflight gates.
fn prepare_trigger(
    store_path: &Path,
    tool: &str,
    project: Option<PathBuf>,
    mcp_config: Option<PathBuf>,
    permission_mode: Option<String>,
    envs: Vec<String>,
) -> Result<SafeTrigger> {
    use hotsheet_cli::launch_safety;

    let plugin = hotsheet_plugins::find(tool)
        .with_context(|| format!("unknown tool '{tool}' (no such plugin)"))?;
    let cwd = project.unwrap_or_else(|| store_path.to_path_buf());

    // `--env K=V` pairs for the launched tool (e.g. an isolated CODEX_HOME).
    let mut env: Vec<(String, String)> = envs
        .iter()
        .map(|kv| {
            kv.split_once('=')
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .with_context(|| format!("--env expects KEY=VALUE, got '{kv}'"))
        })
        .collect::<Result<_>>()?;

    // ---- HS2-103 launch safety (baked in so a bare `trigger`/`work` is safe) ----
    launch_safety::assert_no_hs1(&cwd)?;

    // Codex (`app-server`) isn't isolated by the MCP-config path below — it reads its
    // MCP servers from `$CODEX_HOME` — so its full isolation (a fresh CODEX_HOME) is a
    // follow-up (HS2-YRDQNX). Until then, refuse an un-isolated codex launch by default.
    let transport = plugin
        .manifest
        .drive
        .as_ref()
        .map(|d| d.transport.as_str())
        .unwrap_or("");
    if transport == "app-server" && !env.iter().any(|(k, _)| k == "CODEX_HOME") {
        bail!(
            "refusing to drive {tool} without an isolated CODEX_HOME (its MCP servers would \
             include your global config). Pass `--env CODEX_HOME=<dir>` for now; automatic \
             isolation is HS2-YRDQNX."
        );
    }

    // Put a `hotsheet` → `hotsheet-cli` shim (and the CLI's own dir) at the front of the
    // launched tool's PATH, so a bare `hotsheet` hits our safe CLI (not an HS1 launcher).
    let exe_dir = launch_safety::exe_dir()?;
    let hotsheet_cli = std::env::current_exe()?;
    let shim = launch_safety::ShimDir::create(&hotsheet_cli)?;
    let base_path = env
        .iter()
        .find(|(k, _)| k == "PATH")
        .map(|(_, v)| v.clone())
        .unwrap_or_else(|| std::env::var("PATH").unwrap_or_default());
    let child_path = launch_safety::prepend_path(&[shim.path(), &exe_dir], &base_path);
    launch_safety::assert_hotsheet_resolves(&child_path, shim.path())?;
    env.retain(|(k, _)| k != "PATH");
    env.push(("PATH".to_string(), child_path));

    // MCP isolation: restrict the tool to only the Hot Sheet shim by defaulting
    // `--mcp-config` to the tool's project config (Claude gets `--strict-mcp-config`).
    // This requires the tool to have been set up in the project.
    let mcp_config = match mcp_config {
        Some(p) => Some(p),
        None => {
            let target = cwd.join(&plugin.manifest.mcp.target);
            if !target.exists() {
                bail!(
                    "{tool} isn't set up for Hot Sheet in {} (no {}). Run \
                     `hotsheet-cli setup {tool}` there first — trigger needs it for HS2-103 \
                     MCP isolation.",
                    cwd.display(),
                    plugin.manifest.mcp.target
                );
            }
            Some(target)
        }
    };

    Ok(SafeTrigger {
        plugin,
        cwd,
        env,
        mcp_config,
        // Headless work needs a non-blocking permission mode (channel tools); the real
        // permission bridge is HS2-113.
        permission_mode: permission_mode.unwrap_or_else(|| "acceptEdits".to_string()),
        _shim: shim,
    })
}

impl SafeTrigger {
    /// Drive one turn, streaming the tool's output to stdout. Each call spawns a fresh
    /// process (session-resume continuity is HS2-3C1XK3).
    fn run_turn(
        &self,
        prompt: &str,
        resume: Option<&str>,
        worker: bool,
        registry: &mut hotsheet_aitools::ConnectionRegistry,
    ) -> Result<hotsheet_aitools::DoneReason> {
        use hotsheet_aitools::{LiveError, LiveTrigger, Role, TurnEvent, run_trigger};

        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let t = LiveTrigger {
            cwd: self.cwd.clone(),
            prompt: prompt.to_string(),
            role: if worker { Role::Worker } else { Role::Main },
            conn_id: format!("cli-{}", std::process::id()),
            resume: resume.map(str::to_string),
            mcp_config: self.mcp_config.clone(),
            permission_mode: Some(self.permission_mode.clone()),
            env: self.env.clone(),
            now_ms,
        };
        run_trigger(&self.plugin, &t, registry, &mut |ev| match ev {
            TurnEvent::Output(text) => {
                print!("{text}");
                let _ = std::io::Write::flush(&mut std::io::stdout());
            }
            TurnEvent::PermissionAsked(p) => {
                eprintln!("\n[permission] {} — {}", p.tool, p.summary)
            }
            TurnEvent::Done(_) => {}
        })
        .map_err(|e| match e {
            LiveError::NotDrivable(id) => {
                anyhow::anyhow!(
                    "'{id}' is not drivable (no [drive], or its transport isn't built yet)"
                )
            }
            other => anyhow::Error::new(other),
        })
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
) -> Result<()> {
    use hotsheet_aitools::{ConnectionRegistry, DoneReason};

    let safe = prepare_trigger(store_path, tool, project, mcp_config, permission_mode, envs)?;
    eprintln!("▶ driving {tool} in {} …", safe.cwd.display());
    let mut registry = ConnectionRegistry::new(30_000);
    let reason = safe.run_turn(
        &prompt.unwrap_or_else(|| DEFAULT_TRIGGER_PROMPT.to_string()),
        resume.as_deref(),
        worker,
        &mut registry,
    )?;
    println!();
    match reason {
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
fn cmd_work(
    store_path: &Path,
    tool: &str,
    project: Option<PathBuf>,
    max: u32,
    max_stall: u32,
    worker: bool,
) -> Result<()> {
    use hotsheet_aitools::{ConnectionRegistry, DoneReason};
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

    let safe = prepare_trigger(store_path, tool, project, None, None, vec![])?;
    let mut registry = ConnectionRegistry::new(30_000);
    let mut stall = Stall::default();
    let mut completed = 0u32;
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

        let reason = safe.run_turn(DEFAULT_TRIGGER_PROMPT_LOOP, None, worker, &mut registry)?;
        println!();
        match reason {
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

fn cmd_init(path: &PathBuf, prefix: &str) -> Result<()> {
    FsStore::init(path, &StoreMetadata::new(prefix))
        .with_context(|| format!("initializing store at {}", path.display()))?;
    git_init(path);
    println!("Initialized Hot Sheet store at {}", path.display());
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
    let query = TicketQuery {
        // Validate enum filters up front so a typo errors instead of matching none.
        status: f.status.as_deref().map(parse_status_str).transpose()?,
        priority: f.priority.as_deref().map(parse_priority).transpose()?,
        category: f.category.clone(),
        tags: f.tags.clone(),
        text: f.text.clone(),
        up_next_only: f.up_next,
        open_only: f.open,
        sort: f.sort.parse().map_err(|e: String| anyhow::anyhow!(e))?,
        limit: f.limit,
    };
    let tickets = ops::query(&store, &query)?;

    if tickets.is_empty() {
        println!("(no tickets)");
        return Ok(());
    }
    for t in &tickets {
        let marker = if t.up_next { "*" } else { " " };
        println!(
            "{marker} {:<12} {:<12} {}",
            t.slug,
            status_str(t.status),
            t.title
        );
    }
    Ok(())
}

fn cmd_doctor(path: &PathBuf) -> Result<()> {
    let store = FsStore::open(path)?;
    let meta = store.metadata()?;
    // list_tickets parses every file, so a parse error surfaces here.
    let tickets = store.list_tickets()?;

    println!(
        "Store: {} (prefix {}, {} sharding)",
        path.display(),
        meta.ticket_prefix,
        meta.shard
    );
    println!("Tickets: {}", tickets.len());

    let mut issues = 0usize;
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

    if issues == 0 {
        println!("No issues found.");
        Ok(())
    } else {
        bail!("{issues} issue(s) found")
    }
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
    up_next: bool,
    no_up_next: bool,
    note: Option<String>,
) -> Result<()> {
    let store = FsStore::open(path)?;
    let ticket = resolve(&store, id)?;
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
    };
    let updated = ops::update(&store, &ticket.id, now_ts(), patch)?;
    if let Some(text) = note.filter(|t| !t.is_empty()) {
        ops::add_note(
            &store,
            &ticket.id,
            Ulid::new(),
            now_ts(),
            NoteKind::Regular,
            text,
        )?;
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
            Some("shared") => Ok(Some(Scope::Shared)),
            Some("local") => Ok(Some(Scope::Local)),
            Some(other) => bail!("invalid scope '{other}' (shared|local)"),
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
            let where_ = if scope == Scope::Shared {
                "shared (committed)"
            } else {
                "local (gitignored)"
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
