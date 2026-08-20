//! `hotsheet` — the Hot Sheet 2 command-line interface. A thin binary over
//! `hotsheet-ticketing`: it reads and writes ticket files directly on disk
//! (`docs/04-core-server-cli.md` §4.4) and imports HS1 exports (`docs/07`).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use hotsheet_cli::{git_init, run_import};
use hotsheet_model::{CloseReason, Priority, Status, Ticket, Timestamp, Ulid, to_file_string};
use hotsheet_ticketing::{FsStore, NewTicket, StoreMetadata, TicketPatch, TicketQuery, ops};
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
            ..Default::default()
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
    up_next: bool,
    no_up_next: bool,
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
    let patch = TicketPatch {
        title,
        details,
        category,
        priority: priority.as_deref().map(parse_priority).transpose()?,
        status: status.as_deref().map(parse_status_str).transpose()?,
        tags: (!tags.is_empty()).then_some(tags),
        up_next,
    };
    let updated = ops::update(&store, &ticket.id, now_ts(), patch)?;
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
