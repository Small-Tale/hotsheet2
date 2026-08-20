//! `hotsheet` — the Hot Sheet 2 command-line interface. A thin binary over
//! `hotsheet-ticketing`: it reads and writes ticket files directly on disk
//! (`docs/04-core-server-cli.md` §4.4) and imports HS1 exports (`docs/07`).

mod import;

use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use hotsheet_model::{Priority, Status, Ticket, Ulid, derive_slug, to_file_string};
use hotsheet_ticketing::{FsStore, StoreMetadata};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

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
    /// List tickets (sorted by id).
    Ls,
    /// Print a ticket's file by slug or ULID.
    Show { id: String },
    /// Import an HS1 `hotsheet-export.json` into the store (creates it if needed).
    Import {
        file: PathBuf,
        /// Prefix used if the store must be created first.
        #[arg(long, default_value = "HS")]
        prefix: String,
    },
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
        Cmd::Ls => cmd_ls(&cli.path),
        Cmd::Show { id } => cmd_show(&cli.path, &id),
        Cmd::Import { file, prefix } => cmd_import(&cli.path, &file, &prefix),
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

fn cmd_ls(path: &PathBuf) -> Result<()> {
    let store = FsStore::open(path)?;
    let tickets = store.list_tickets()?;
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

fn cmd_show(path: &PathBuf, needle: &str) -> Result<()> {
    let store = FsStore::open(path)?;
    let ticket = resolve(&store, needle)?;
    print!("{}", to_file_string(&ticket));
    Ok(())
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
