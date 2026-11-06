//! `hotsheet-cli metrics reroute-bootstrap-usage` (HS2-B4EQ87): move Claude terminal usage
//! that a machine server recorded in its bootstrap store into the project store it belongs to.
//!
//! A row is moved only when its project is certain: an unticketed Claude row whose session
//! names exactly one transcript under the Claude projects directory, whose directory name is
//! the Claude-encoded root of exactly one registered checkout with a Git ticket store. Only
//! file and directory names are read there, never transcript content. Every other row stays.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Subcommand;
use hotsheet_ticketing::FsStore;
use hotsheet_ticketing::checkouts::{Checkout, CheckoutRegistry};
use hotsheet_ticketing::metrics::UsageEvent;
use hotsheet_ticketing::metrics::reroute::reroute_raw_usage;

#[derive(Subcommand, Debug)]
pub(crate) enum MetricsCmd {
    /// Move Claude terminal usage the machine server recorded in its bootstrap store into
    /// each project's own store (HS2-B4EQ87). Previews unless --apply is given; idempotent.
    RerouteBootstrapUsage {
        /// Write the moves. Without it (or with --dry-run) only the preview is printed.
        #[arg(long, conflicts_with = "dry_run")]
        apply: bool,
        /// Preview only (the default).
        #[arg(long)]
        dry_run: bool,
        /// Claude's projects directory (default: $CLAUDE_CONFIG_DIR/projects, else
        /// ~/.claude/projects). Only its directory and file names are read.
        #[arg(long, value_name = "DIR")]
        claude_projects: Option<PathBuf>,
    },
}

pub(crate) fn run(cmd: MetricsCmd) -> Result<()> {
    match cmd {
        MetricsCmd::RerouteBootstrapUsage {
            apply,
            dry_run: _,
            claude_projects,
        } => reroute_bootstrap_usage(
            &hotsheet_plugins::hotsheet_home(),
            &claude_projects.unwrap_or_else(default_claude_projects),
            apply,
        ),
    }
}

fn default_claude_projects() -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".claude")))
        .unwrap_or_default()
        .join("projects")
}

/// Claude Code names a session's transcript directory after its working directory with
/// every character other than an ASCII letter or digit replaced by `-`.
fn claude_project_dir_name(root: &str) -> String {
    root.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// Session id to the transcript directory names that hold `<session>.jsonl`.
fn transcript_dirs(projects: &Path) -> HashMap<String, Vec<String>> {
    let mut sessions: HashMap<String, Vec<String>> = HashMap::new();
    let Ok(dirs) = std::fs::read_dir(projects) else {
        return sessions;
    };
    for dir in dirs.flatten().filter(|dir| dir.path().is_dir()) {
        let Some(name) = dir.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Ok(files) = std::fs::read_dir(dir.path()) else {
            continue;
        };
        for file in files.flatten() {
            let file_name = file.file_name();
            if let Some(session) = file_name
                .to_str()
                .and_then(|file| file.strip_suffix(".jsonl"))
            {
                sessions
                    .entry(session.to_owned())
                    .or_default()
                    .push(name.clone());
            }
        }
    }
    sessions
}

#[derive(Default)]
struct Kept {
    ticketed: usize,
    other_tool: usize,
    no_transcript: usize,
    no_checkout: usize,
    no_store: usize,
}

pub(crate) fn reroute_bootstrap_usage(home: &Path, projects: &Path, apply: bool) -> Result<()> {
    let bootstrap = home.join(hotsheet_ticketing::store_git::MACHINE_ANCHOR_DIR);
    if !bootstrap.join("metrics").join("raw").is_dir() {
        println!("No bootstrap usage to reroute in {}.", bootstrap.display());
        return Ok(());
    }
    let source = FsStore::open_without_maintenance(&bootstrap)
        .with_context(|| format!("open the bootstrap store {}", bootstrap.display()))?;
    let checkouts = CheckoutRegistry::new(home.join("checkouts.json"))
        .list()
        .context("read the checkout registry")?;
    let mut by_dir: HashMap<String, Vec<&Checkout>> = HashMap::new();
    for checkout in &checkouts {
        by_dir
            .entry(claude_project_dir_name(&checkout.root))
            .or_default()
            .push(checkout);
    }
    let sessions = transcript_dirs(projects);
    let mut stores: HashMap<String, Option<FsStore>> = HashMap::new();
    let mut aliases: BTreeMap<PathBuf, String> = BTreeMap::new();
    let mut kept = Kept::default();
    let route = |_: &serde_json::Value, event: &UsageEvent| -> Option<FsStore> {
        // A ticket came from a bootstrap-store claim: the row is where it belongs.
        if event.ticket.is_some() {
            kept.ticketed += 1;
            return None;
        }
        if event.tool != "claude" {
            kept.other_tool += 1;
            return None;
        }
        let dirs = event
            .session
            .as_ref()
            .and_then(|session| sessions.get(session));
        let Some([dir]) = dirs.map(Vec::as_slice) else {
            kept.no_transcript += 1;
            return None;
        };
        let Some([checkout]) = by_dir.get(dir).map(Vec::as_slice) else {
            kept.no_checkout += 1;
            return None;
        };
        let Some(usage) = checkout.usage_source() else {
            kept.no_store += 1;
            return None;
        };
        let store = stores
            .entry(usage.locator.clone())
            .or_insert_with(|| FsStore::open_without_maintenance(&usage.locator).ok())
            .clone();
        match store {
            Some(store) => {
                aliases
                    .entry(
                        store
                            .root()
                            .canonicalize()
                            .unwrap_or_else(|_| store.root().to_path_buf()),
                    )
                    .or_insert_with(|| checkout.alias.clone());
                Some(store)
            }
            None => {
                kept.no_store += 1;
                None
            }
        }
    };
    let report = reroute_raw_usage(&source, route, apply)?;
    let verb = if apply { "Moved" } else { "Would move" };
    println!(
        "{verb} {} usage row(s) from {}{}",
        report.moved_total(),
        bootstrap.display(),
        if apply {
            ""
        } else {
            " (preview; pass --apply to write)"
        }
    );
    for (root, count) in &report.moved {
        let alias = aliases.get(root).map_or("?", String::as_str);
        println!("  {alias} ({}): {count}", root.display());
    }
    if report.already_in_target > 0 {
        println!(
            "  {} already present in the project store; removed only from the bootstrap store",
            report.already_in_target
        );
    }
    println!(
        "Kept: {} with a bootstrap ticket, {} from other tools, {} without one matching transcript, {} without one matching checkout, {} without a Git store, {} settled, {} unreadable",
        kept.ticketed,
        kept.other_tool,
        kept.no_transcript,
        kept.no_checkout,
        kept.no_store,
        report.settled,
        report.unreadable
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::claude_project_dir_name;

    #[test]
    fn hs2_b4eq87_claude_project_dir_names_replace_every_non_alphanumeric() {
        assert_eq!(
            claude_project_dir_name("/srv/work/hot.sheet_2"),
            "-srv-work-hot-sheet-2"
        );
    }
}
