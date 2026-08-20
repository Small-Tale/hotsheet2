//! `hotsheet` — the Hot Sheet 2 command-line interface. A thin binary over
//! `hotsheet-ticketing`: it reads and writes ticket files directly on disk
//! (`docs/04-core-server-cli.md` §4.4) and imports HS1 exports (`docs/07`).

mod import;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use hotsheet_model::{
    CloseReason, Priority, Status, Ticket, Timestamp, Ulid, derive_slug, to_file_string,
};
use hotsheet_ticketing::{FsStore, StoreMetadata};
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

use crate::import::{ExportFile, SUPPORTED_EXPORT_VERSION, import};

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
        #[arg(long)]
        title: String,
        #[arg(long, default_value = "issue")]
        category: String,
        #[arg(long, default_value = "default")]
        priority: String,
        #[arg(long)]
        details: Option<String>,
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
        /// Mark Up Next.
        #[arg(long, conflicts_with = "no_up_next")]
        up_next: bool,
        /// Clear Up Next.
        #[arg(long)]
        no_up_next: bool,
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
    /// Import an HS1 `hotsheet-export.json` into the store (creates it if needed).
    Import {
        file: PathBuf,
        /// Prefix used if the store must be created first.
        #[arg(long, default_value = "HS")]
        prefix: String,
    },
    /// Migrate a Hot Sheet 1 project into this store in one step (runs the bundled
    /// Node exporter against a COPY of its database, then imports).
    Migrate {
        /// Path to the old project's `.hotsheet` directory.
        hotsheet_dir: PathBuf,
        /// Prefix used if the store must be created first.
        #[arg(long, default_value = "HS")]
        prefix: String,
        /// Path to the migrator's `export.mjs` (auto-detected, or $HOTSHEET_MIGRATOR).
        #[arg(long)]
        migrator: Option<PathBuf>,
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
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Cmd::Init { prefix } => cmd_init(&cli.path, &prefix),
        Cmd::New {
            title,
            category,
            priority,
            details,
        } => cmd_new(&cli.path, title, category, &priority, details),
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
            up_next,
            no_up_next,
        } => cmd_edit(
            &cli.path, &id, title, details, category, priority, status, tags, up_next, no_up_next,
        ),
        Cmd::Close {
            id,
            reason,
            duplicate_of,
        } => cmd_close(&cli.path, &id, &reason, duplicate_of),
        Cmd::Import { file, prefix } => cmd_import(&cli.path, &file, &prefix),
        Cmd::Migrate {
            hotsheet_dir,
            prefix,
            migrator,
        } => cmd_migrate(&cli.path, &hotsheet_dir, &prefix, migrator),
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
    }
}

fn cmd_init(path: &PathBuf, prefix: &str) -> Result<()> {
    FsStore::init(path, &StoreMetadata::new(prefix))
        .with_context(|| format!("initializing store at {}", path.display()))?;
    git_init(path);
    println!("Initialized Hot Sheet store at {}", path.display());
    Ok(())
}

fn cmd_new(
    path: &PathBuf,
    title: String,
    category: String,
    priority: &str,
    details: Option<String>,
) -> Result<()> {
    let store = FsStore::open(path)?;
    let prefix = store.metadata()?.ticket_prefix;

    let id = Ulid::new();
    let now = now_rfc3339();
    let mut ticket = Ticket::new(
        id,
        derive_slug(&id, &prefix),
        title,
        category,
        now.clone(),
        now,
    );
    ticket.priority = parse_priority(priority)?;
    if let Some(details) = details {
        ticket.details = details;
    }

    let written = store.write_ticket(&ticket)?;
    println!("Created {} ({})", ticket.slug, written.display());
    Ok(())
}

fn cmd_ls(path: &PathBuf, f: &LsFilters) -> Result<()> {
    let store = FsStore::open(path)?;
    let mut tickets = store.list_tickets()?;

    // Validate enum filters up front so a typo errors instead of silently matching none.
    let status = f.status.as_deref().map(parse_status_str).transpose()?;
    let priority = f.priority.as_deref().map(parse_priority).transpose()?;
    let text = f.text.as_deref().map(str::to_lowercase);

    tickets.retain(|t| {
        status.is_none_or(|s| t.status == s)
            && priority.is_none_or(|p| t.priority == p)
            && f.category.as_deref().is_none_or(|c| t.category == c)
            && (!f.up_next || t.up_next)
            && (!f.open || is_open(t))
            && f.tags.iter().all(|tag| t.tags.iter().any(|x| x == tag))
            && text.as_deref().is_none_or(|q| ticket_matches_text(t, q))
    });
    sort_tickets(&mut tickets, &f.sort)?;

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
    let tickets = store.list_tickets()?;
    let done: HashSet<Ulid> = tickets
        .iter()
        .filter(|t| is_done(t))
        .map(|t| t.id)
        .collect();
    let now = OffsetDateTime::now_utc();

    let mut candidates: Vec<Ticket> = tickets
        .into_iter()
        .filter(|t| is_open(t) && !is_blocked(t, &done) && claim_available(t, now))
        .collect();

    // Prefer Up Next, then priority (highest first), then id (creation order).
    candidates.sort_by(|a, b| {
        b.up_next
            .cmp(&a.up_next)
            .then(priority_rank(a.priority).cmp(&priority_rank(b.priority)))
            .then(a.id.cmp(&b.id))
    });

    let Some(mut ticket) = candidates.into_iter().next() else {
        println!("No claimable tickets.");
        return Ok(());
    };

    ticket.claimed_by = Some(worker.to_string());
    ticket.claim_lease_expires_at = Some(lease_until(now, lease_minutes));
    ticket.worker_label = label;
    ticket.claim_count += 1;
    ticket.updated_at = now_ts();
    store.write_ticket(&ticket)?;
    println!(
        "Claimed {} for {worker} (lease {lease_minutes}m)",
        ticket.slug
    );
    Ok(())
}

fn cmd_release(path: &PathBuf, id: &str, worker: &str, force: bool) -> Result<()> {
    let store = FsStore::open(path)?;
    let mut ticket = resolve(&store, id)?;
    match &ticket.claimed_by {
        None => {
            println!("{} is not claimed.", ticket.slug);
            return Ok(());
        }
        Some(holder) if holder != worker && !force => {
            bail!(
                "{} is claimed by '{holder}', not '{worker}' (use --force)",
                ticket.slug
            );
        }
        _ => {}
    }
    ticket.claimed_by = None;
    ticket.claim_lease_expires_at = None;
    ticket.worker_label = None;
    ticket.updated_at = now_ts();
    store.write_ticket(&ticket)?;
    println!("Released {}", ticket.slug);
    Ok(())
}

fn cmd_renew(path: &PathBuf, id: &str, worker: &str, lease_minutes: i64) -> Result<()> {
    let store = FsStore::open(path)?;
    let mut ticket = resolve(&store, id)?;
    match &ticket.claimed_by {
        Some(holder) if holder == worker => {}
        Some(holder) => bail!("{} is claimed by '{holder}', not '{worker}'", ticket.slug),
        None => bail!("{} is not claimed", ticket.slug),
    }
    ticket.claim_lease_expires_at = Some(lease_until(OffsetDateTime::now_utc(), lease_minutes));
    ticket.updated_at = now_ts();
    store.write_ticket(&ticket)?;
    println!("Renewed {} (lease {lease_minutes}m)", ticket.slug);
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
    up_next: bool,
    no_up_next: bool,
) -> Result<()> {
    let store = FsStore::open(path)?;
    let mut ticket = resolve(&store, id)?;

    if let Some(v) = title {
        ticket.title = v;
    }
    if let Some(v) = details {
        ticket.details = v;
    }
    if let Some(v) = category {
        ticket.category = v;
    }
    if let Some(v) = priority {
        ticket.priority = parse_priority(&v)?;
    }
    if let Some(v) = status {
        apply_status(&mut ticket, parse_status_str(&v)?);
    }
    if !tags.is_empty() {
        ticket.tags = tags;
    }
    if up_next {
        ticket.up_next = true;
    } else if no_up_next {
        ticket.up_next = false;
    }

    ticket.updated_at = Timestamp::from(now_rfc3339());
    store.write_ticket(&ticket)?;
    println!("Updated {}", ticket.slug);
    Ok(())
}

fn cmd_close(path: &PathBuf, id: &str, reason: &str, duplicate_of: Option<String>) -> Result<()> {
    let store = FsStore::open(path)?;
    let mut ticket = resolve(&store, id)?;
    let reason_enum = parse_close_reason(reason)?;

    let dup = match duplicate_of {
        Some(d) => Some(resolve(&store, &d)?.id),
        None => None,
    };
    if reason_enum == CloseReason::Duplicate && dup.is_none() {
        bail!("--duplicate-of <id> is required when --reason duplicate");
    }

    ticket.close_reason = Some(reason_enum);
    ticket.closed_at = Some(Timestamp::from(now_rfc3339()));
    ticket.duplicate_of = dup;
    ticket.updated_at = Timestamp::from(now_rfc3339());
    store.write_ticket(&ticket)?;
    println!("Closed {} ({reason})", ticket.slug);
    Ok(())
}

/// Set a ticket's status, stamping completed_at / verified_at on the terminal ones.
fn apply_status(ticket: &mut Ticket, status: Status) {
    ticket.status = status;
    match status {
        Status::Completed if ticket.completed_at.is_none() => {
            ticket.completed_at = Some(Timestamp::from(now_rfc3339()));
        }
        Status::Verified if ticket.verified_at.is_none() => {
            ticket.verified_at = Some(Timestamp::from(now_rfc3339()));
        }
        _ => {}
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

fn parse_close_reason(s: &str) -> Result<CloseReason> {
    Ok(match s {
        "completed" => CloseReason::Completed,
        "not_planned" => CloseReason::NotPlanned,
        "duplicate" => CloseReason::Duplicate,
        "obsolete" => CloseReason::Obsolete,
        other => bail!("invalid close reason '{other}' (completed|not_planned|duplicate|obsolete)"),
    })
}

fn cmd_import(path: &PathBuf, file: &PathBuf, prefix: &str) -> Result<()> {
    let text = std::fs::read_to_string(file)
        .with_context(|| format!("reading export {}", file.display()))?;
    let export: ExportFile =
        serde_json::from_str(&text).with_context(|| format!("parsing {}", file.display()))?;

    if export.export_version != SUPPORTED_EXPORT_VERSION {
        eprintln!(
            "warning: export version {} differs from supported {SUPPORTED_EXPORT_VERSION}; \
             importing on a best-effort basis",
            export.export_version
        );
    }
    if let Some(name) = &export.project.name {
        println!("Importing project '{name}'…");
    }

    // Create the store on first import if it isn't one yet, preferring the export's
    // own ticket prefix over the flag default.
    let store = match FsStore::open(path) {
        Ok(store) => store,
        Err(_) => {
            let init_prefix = export.project.ticket_prefix.as_deref().unwrap_or(prefix);
            let store = FsStore::init(path, &StoreMetadata::new(init_prefix))?;
            git_init(path);
            store
        }
    };

    let base_dir = file.parent().unwrap_or_else(|| std::path::Path::new("."));
    let summary = import(&store, &export, base_dir)?;
    println!(
        "Imported {} ticket(s) ({} attachment file(s)), skipped {} already present.",
        summary.written, summary.attachments, summary.skipped
    );
    if summary.written > 0 {
        git_commit_all(
            path,
            &format!("Import {} tickets from Hot Sheet 1", summary.written),
        );
    }
    Ok(())
}

fn cmd_migrate(
    path: &PathBuf,
    hotsheet_dir: &Path,
    prefix: &str,
    migrator: Option<PathBuf>,
) -> Result<()> {
    let export_mjs = resolve_migrator(migrator)?;

    // A private temp dir for the export JSON + any staged attachment files. The
    // exporter itself only ever opens a COPY of the source database (read-only).
    let staging = std::env::temp_dir().join(format!("hotsheet-migrate-{}", std::process::id()));
    std::fs::create_dir_all(&staging)?;
    let export_json = staging.join("hotsheet-export.json");

    println!("Exporting {} …", hotsheet_dir.display());
    let status = Command::new("node")
        .arg(&export_mjs)
        .arg(hotsheet_dir)
        .arg("--out")
        .arg(&export_json)
        .status()
        .with_context(|| {
            format!(
                "running the migrator ({}) — is Node installed?",
                export_mjs.display()
            )
        })?;
    if !status.success() {
        bail!("migrator export failed ({status})");
    }

    let result = cmd_import(path, &export_json, prefix);
    let _ = std::fs::remove_dir_all(&staging);
    result
}

/// Find the Node exporter: an explicit path, `$HOTSHEET_MIGRATOR`, or a few
/// locations relative to the CWD / executable.
fn resolve_migrator(explicit: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(p) = explicit {
        if p.is_file() {
            return Ok(p);
        }
        bail!("migrator not found at {}", p.display());
    }
    if let Ok(env) = std::env::var("HOTSHEET_MIGRATOR") {
        let p = PathBuf::from(env);
        if p.is_file() {
            return Ok(p);
        }
    }
    let mut candidates = vec![PathBuf::from("migrator/src/export.mjs")];
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("../../migrator/src/export.mjs"));
            candidates.push(dir.join("../../../migrator/src/export.mjs"));
        }
    }
    candidates
        .into_iter()
        .find(|c| c.is_file())
        .with_context(|| {
            "could not find the migrator (migrator/src/export.mjs); \
             pass --migrator <path> or set HOTSHEET_MIGRATOR"
                .to_string()
        })
}

// ---- helpers ---------------------------------------------------------------------

/// Resolve a ticket by ULID (exact) or by slug (case-insensitive).
fn resolve(store: &FsStore, needle: &str) -> Result<Ticket> {
    if let Ok(id) = Ulid::from_string(needle) {
        return store.read_ticket(&id).map_err(Into::into);
    }
    let wanted = needle.to_uppercase();
    store
        .list_tickets()?
        .into_iter()
        .find(|t| t.slug.eq_ignore_ascii_case(&wanted))
        .with_context(|| format!("no ticket matching '{needle}'"))
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

fn now_rfc3339() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .expect("formatting the current time as RFC3339")
}

fn now_ts() -> Timestamp {
    Timestamp::from_datetime(OffsetDateTime::now_utc())
}

fn lease_until(now: OffsetDateTime, minutes: i64) -> Timestamp {
    Timestamp::from_datetime(now + Duration::minutes(minutes))
}

/// A ticket in a workflow-open state (not a terminal/hidden one).
fn is_open(t: &Ticket) -> bool {
    !matches!(
        t.status,
        Status::Completed | Status::Verified | Status::Deleted | Status::Archive | Status::Moved
    )
}

fn is_done(t: &Ticket) -> bool {
    matches!(t.status, Status::Completed | Status::Verified)
}

/// Blocked while any `blocked_by` dependency isn't done.
fn is_blocked(t: &Ticket, done: &HashSet<Ulid>) -> bool {
    t.blocked_by.iter().any(|b| !done.contains(b))
}

/// A claim is available if unclaimed, lease-less (stale), or the lease has expired.
fn claim_available(t: &Ticket, now: OffsetDateTime) -> bool {
    match (&t.claimed_by, &t.claim_lease_expires_at) {
        (None, _) => true,
        (Some(_), None) => true,
        (Some(_), Some(exp)) => exp.instant().is_none_or(|e| e <= now),
    }
}

fn ticket_matches_text(t: &Ticket, needle_lower: &str) -> bool {
    t.title.to_lowercase().contains(needle_lower)
        || t.details.to_lowercase().contains(needle_lower)
        || t.notes
            .iter()
            .any(|n| n.text.to_lowercase().contains(needle_lower))
}

fn priority_rank(p: Priority) -> u8 {
    match p {
        Priority::Highest => 0,
        Priority::High => 1,
        Priority::Default => 2,
        Priority::Low => 3,
        Priority::Lowest => 4,
    }
}

fn sort_tickets(tickets: &mut [Ticket], key: &str) -> Result<()> {
    match key {
        "id" => tickets.sort_by(|a, b| a.id.cmp(&b.id)),
        "created" => tickets.sort_by(|a, b| a.created_at.as_str().cmp(b.created_at.as_str())),
        "updated" => tickets.sort_by(|a, b| a.updated_at.as_str().cmp(b.updated_at.as_str())),
        "priority" => tickets.sort_by_key(|t| priority_rank(t.priority)),
        "status" => tickets.sort_by(|a, b| status_str(a.status).cmp(&status_str(b.status))),
        "title" => tickets.sort_by(|a, b| a.title.to_lowercase().cmp(&b.title.to_lowercase())),
        other => bail!("invalid sort '{other}' (id|created|updated|priority|status|title)"),
    }
    Ok(())
}

/// Best-effort `git init` of a new store (warns, never fails the command).
fn git_init(path: &PathBuf) {
    if path.join(".git").exists() {
        return;
    }
    run_git(path, &["init", "--quiet"]);
}

/// Best-effort `git add -A && git commit` (warns on failure; files are already written).
fn git_commit_all(path: &PathBuf, message: &str) {
    run_git(path, &["add", "-A"]);
    run_git(path, &["commit", "--quiet", "-m", message]);
}

fn run_git(path: &PathBuf, args: &[&str]) {
    match Command::new("git").current_dir(path).args(args).status() {
        Ok(status) if status.success() => {}
        Ok(status) => eprintln!("warning: git {} exited with {status}", args.join(" ")),
        Err(err) => eprintln!("warning: could not run git {}: {err}", args.join(" ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(id: &str) -> Ticket {
        Ticket::new(
            Ulid::from_string(id).unwrap(),
            "HS-X",
            "t",
            "issue",
            "2026-01-01T00:00:00Z",
            "2026-01-01T00:00:00Z",
        )
    }

    #[test]
    fn blocked_until_every_dependency_is_done() {
        let dep = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
        let mut ticket = sample("01ARZ3NDEKTSV4RRFFQ69G5FB0");
        ticket.blocked_by = vec![dep];
        let mut done = HashSet::new();
        assert!(is_blocked(&ticket, &done));
        done.insert(dep);
        assert!(!is_blocked(&ticket, &done));
    }

    #[test]
    fn claim_available_respects_the_lease() {
        let now = OffsetDateTime::now_utc();
        let mut ticket = sample("01ARZ3NDEKTSV4RRFFQ69G5FB1");
        assert!(claim_available(&ticket, now), "unclaimed is available");
        ticket.claimed_by = Some("w".into());
        ticket.claim_lease_expires_at = Some(lease_until(now, 30));
        assert!(!claim_available(&ticket, now), "an active lease is held");
        ticket.claim_lease_expires_at = Some(lease_until(now, -30));
        assert!(
            claim_available(&ticket, now),
            "an expired lease is reclaimable"
        );
    }

    #[test]
    fn is_open_excludes_terminal_states() {
        let mut ticket = sample("01ARZ3NDEKTSV4RRFFQ69G5FB2");
        assert!(is_open(&ticket));
        ticket.status = Status::Completed;
        assert!(!is_open(&ticket));
    }

    #[test]
    fn priority_rank_orders_highest_first() {
        assert!(priority_rank(Priority::Highest) < priority_rank(Priority::Default));
        assert!(priority_rank(Priority::Default) < priority_rank(Priority::Lowest));
    }
}
