//! Milestone registry commands (HS2-9PYNCN, docs/38 §38.6): `milestones [--all] [--json]`
//! and `milestone create|show|edit|close|reopen`. They call the same
//! `hotsheet_ticketing::milestones` registry as the server routes and MCP.
//!
//! The registry lives in the project's code checkout. The CLI finds it as it finds the group
//! registry: the nearest ancestor of the working directory linked to this store. Without one
//! (`-C <store>` from elsewhere) there is no registry: the list is empty and every other
//! command is refused with `milestone_registry_required`. Each write commits its file as it
//! returns.

use std::path::Path;

use anyhow::{Context, Result};
use clap::Subcommand;
use hotsheet_ticketing::FsStore;
use hotsheet_ticketing::milestones::{
    self, CloseReason, Milestone, MilestoneEdit, MilestoneError, MilestoneRegistry, MilestoneState,
    MilestoneWrite,
};

#[derive(Subcommand)]
pub(crate) enum MilestoneCmd {
    /// Create an open milestone in the project's registry. Prints its id. A title another
    /// milestone has (ignoring case and spacing) is refused.
    Create {
        /// The milestone's title (one line, at most 120 characters).
        title: String,
        /// Due date, `YYYY-MM-DD`.
        #[arg(long)]
        due: Option<String>,
        /// Read the Markdown description from this file (`-` for stdin).
        #[arg(long)]
        description_file: Option<String>,
    },
    /// Show one milestone.
    Show {
        /// The milestone's id or title.
        milestone: String,
        /// Print the milestone as JSON.
        #[arg(long)]
        json: bool,
    },
    /// Change a milestone's title, description, or due date. Unchanged values commit nothing.
    Edit {
        /// The milestone's id or title.
        milestone: String,
        /// The new title.
        #[arg(long)]
        title: Option<String>,
        /// The new due date, `YYYY-MM-DD`.
        #[arg(long, conflicts_with = "clear_due")]
        due: Option<String>,
        /// Remove the due date.
        #[arg(long)]
        clear_due: bool,
        /// Read the new Markdown description from this file (`-` for stdin); an empty file
        /// clears it.
        #[arg(long)]
        description_file: Option<String>,
    },
    /// Close a milestone, as completed (the default) or not planned. Its tickets are not
    /// changed. Closing a closed milestone for the same reason changes nothing.
    Close {
        /// The milestone's id or title.
        milestone: String,
        /// `completed` or `not_planned`.
        #[arg(long, default_value = "completed")]
        reason: String,
    },
    /// Reopen a closed milestone. Reopening an open one changes nothing.
    Reopen {
        /// The milestone's id or title.
        milestone: String,
    },
}

/// The milestone registry of the project this CLI invocation runs in, if any.
fn registry_for(store: &FsStore) -> Option<MilestoneRegistry> {
    let cwd = std::env::current_dir().ok()?;
    super::groups::checkout_root_for(store.root(), &cwd)
        .map(|root| MilestoneRegistry::for_checkout(&root))
}

fn require_registry(store: &FsStore) -> Result<MilestoneRegistry, MilestoneError> {
    registry_for(store).ok_or(MilestoneError::RegistryRequired)
}

fn read_description(source: &str) -> Result<String> {
    if source == "-" {
        let mut text = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut text)
            .context("read the description from stdin")?;
        return Ok(text);
    }
    std::fs::read_to_string(source).with_context(|| format!("read {source}"))
}

/// One line for a listing: id, state, due date, title, and flags.
fn row(milestone: &Milestone) -> String {
    let state = match (milestone.state, milestone.closed_reason) {
        (MilestoneState::Open, _) => "open".to_string(),
        (MilestoneState::Closed, Some(CloseReason::NotPlanned)) => "not planned".to_string(),
        (MilestoneState::Closed, _) => "closed".to_string(),
        (MilestoneState::Conflicted, _) => "CONFLICTED".to_string(),
    };
    let due = milestone.due_on.as_deref().unwrap_or("-");
    let mut flags = Vec::new();
    if milestone.duplicate {
        flags.push("duplicate title; use the id".to_string());
    }
    if let Some(conflict) = &milestone.conflict {
        flags.push(format!(
            "{} {}; resolve it with Git",
            conflict.file, conflict.detail
        ));
    }
    let flags = if flags.is_empty() {
        String::new()
    } else {
        format!("  ({})", flags.join("; "))
    };
    format!(
        "{:<10}  {:<11}  {:<10}  {}{flags}",
        milestone.id, state, due, milestone.title
    )
}

pub(crate) fn cmd_milestones(path: &Path, json: bool, all: bool) -> Result<()> {
    let store = FsStore::open(path)?;
    let registry = registry_for(&store);
    let listing = milestones::listing(registry.as_ref(), all)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&listing)?);
        return Ok(());
    }
    if let Some(sync) = listing
        .registry_sync
        .as_ref()
        .filter(|sync| sync.state.needs_attention())
    {
        eprintln!("Milestone registry not published: {}", sync.detail);
    }
    if !listing.registry {
        eprintln!("{}", MilestoneError::RegistryRequired);
    }
    if listing.milestones.is_empty() {
        println!("(no milestones)");
    }
    for milestone in &listing.milestones {
        println!("{}", row(milestone));
    }
    Ok(())
}

fn print_write(verb: &str, write: &MilestoneWrite) {
    let milestone = &write.milestone;
    if write.changed {
        println!("{verb} milestone '{}' ({}).", milestone.title, milestone.id);
    } else {
        println!(
            "Milestone '{}' ({}) is unchanged.",
            milestone.title, milestone.id
        );
    }
}

pub(crate) fn cmd_milestone(path: &Path, cmd: MilestoneCmd) -> Result<()> {
    let store = FsStore::open(path)?;
    let registry = require_registry(&store)?;
    match cmd {
        MilestoneCmd::Create {
            title,
            due,
            description_file,
        } => {
            let description = description_file
                .as_deref()
                .map(read_description)
                .transpose()?;
            let milestone = registry.create(&title, description.as_deref(), due.as_deref())?;
            println!(
                "Created milestone '{}' ({}).",
                milestone.title, milestone.id
            );
        }
        MilestoneCmd::Show { milestone, json } => {
            let milestone = registry.resolve(&milestone)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&milestone)?);
                return Ok(());
            }
            println!("{}", row(&milestone));
            if let Some(closed_at) = &milestone.closed_at {
                println!("Closed at {closed_at}");
            }
            if let Some(description) = &milestone.description {
                println!();
                println!("{description}");
            }
        }
        MilestoneCmd::Edit {
            milestone,
            title,
            due,
            clear_due,
            description_file,
        } => {
            let id = registry.resolve(&milestone)?.id;
            let description = description_file
                .as_deref()
                .map(read_description)
                .transpose()?
                .map(|text| Some(text).filter(|text| !text.trim().is_empty()));
            let due_on = if clear_due { Some(None) } else { due.map(Some) };
            let write = registry.edit(
                &id,
                &MilestoneEdit {
                    title,
                    description,
                    due_on,
                },
            )?;
            print_write("Updated", &write);
        }
        MilestoneCmd::Close { milestone, reason } => {
            let reason = CloseReason::parse(&reason)?;
            let id = registry.resolve(&milestone)?.id;
            print_write("Closed", &registry.close(&id, reason)?);
        }
        MilestoneCmd::Reopen { milestone } => {
            let id = registry.resolve(&milestone)?.id;
            print_write("Reopened", &registry.reopen(&id)?);
        }
    }
    Ok(())
}
