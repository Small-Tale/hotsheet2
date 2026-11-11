//! Ticket group commands (HS2-PDSHNN, HS2-8CDFKR): `groups`, `group rename`, and
//! `group ungroup`, plus the spelling resolution `new`, `edit`, and `batch` share. They
//! call the same `hotsheet_ticketing::groups` operations as the server routes.

use std::path::Path;

use anyhow::Result;
use clap::Subcommand;
use hotsheet_model::Ulid;
use hotsheet_ticketing::FsStore;
use hotsheet_ticketing::groups::{self, GroupDirectory};

use crate::now_ts;

#[derive(Subcommand)]
pub(crate) enum GroupCmd {
    /// Rename a group on every ticket that names it. Fails when the new name is another
    /// existing group; changing only letter case or spacing is allowed.
    Rename {
        /// The group's current name (case and spacing do not matter).
        from: String,
        /// The new name.
        to: String,
    },
    /// Remove every ticket from a group, which dissolves it.
    Ungroup {
        /// The group's name (case and spacing do not matter).
        name: String,
    },
}

/// Validate a requested group change for the tickets in `patched` and reuse the spelling
/// other tickets of the store already carry. The store is read only when a name is set.
pub(crate) fn resolve_for_store(
    store: &FsStore,
    change: Option<Option<String>>,
    patched: &[Ulid],
) -> Result<Option<Option<String>>> {
    if !matches!(change, Some(Some(_))) {
        return Ok(change);
    }
    let directory = GroupDirectory::scan(store)?;
    Ok(groups::resolve_group_change(&directory, change, patched)?)
}

pub(crate) fn cmd_groups(path: &Path, json: bool) -> Result<()> {
    let store = FsStore::open(path)?;
    let summaries = GroupDirectory::scan(&store)?.summaries();
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({ "groups": summaries }))?
        );
        return Ok(());
    }
    if summaries.is_empty() {
        println!("(no groups)");
    }
    for group in &summaries {
        println!("{:>5}  {}", group.count, group.name);
    }
    Ok(())
}

pub(crate) fn cmd_group(path: &Path, cmd: GroupCmd) -> Result<()> {
    let store = FsStore::open(path)?;
    match cmd {
        GroupCmd::Rename { from, to } => {
            let renamed = groups::rename_group(&store, &from, &to, now_ts())?;
            println!(
                "Renamed group to '{}' on {} ticket(s).",
                renamed.name, renamed.write.updated
            );
        }
        GroupCmd::Ungroup { name } => {
            let write = groups::ungroup(&store, &name, now_ts())?;
            println!("Ungrouped {} ticket(s).", write.updated);
        }
    }
    Ok(())
}
