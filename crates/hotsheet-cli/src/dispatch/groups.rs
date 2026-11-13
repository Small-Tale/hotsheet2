//! Ticket group commands (HS2-PDSHNN, HS2-8CDFKR, HS2-9KZSD4): `groups` and `group
//! create|rename|ungroup|merge|migrate`, plus the name resolution `new`, `edit`, `batch`,
//! and `ls` share. They call the same `hotsheet_ticketing::groups` workflows as the server
//! routes.
//!
//! Group names live in the project's registry under the code checkout. The CLI finds the
//! checkout by walking up from the working directory to a project linked to this store
//! (its `.hotsheet2/store` link, or a registered checkout of the store). With no
//! checkout (`-C <store>` from elsewhere) it lists ids under placeholders and accepts
//! only ids.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::Subcommand;
use hotsheet_ticketing::FsStore;
use hotsheet_ticketing::groups::{self, GroupDirectory, GroupRegistry};

#[derive(Subcommand)]
pub(crate) enum GroupCmd {
    /// Create a group in the project's registry without assigning tickets, or print the
    /// existing group with that name. Prints the group id.
    Create {
        /// The group's name.
        name: String,
    },
    /// Rename a group: one registry edit, no ticket changes. Fails when the new name is
    /// another listed group; changing only letter case or spacing is allowed. Also names
    /// a group this project shows under a placeholder.
    Rename {
        /// The group's id or current name (case and spacing do not matter).
        group: String,
        /// The new name.
        to: String,
    },
    /// Remove every ticket from a group. Its registry entry is kept.
    Ungroup {
        /// The group's id or name (case and spacing do not matter).
        group: String,
    },
    /// Move every member of one group into another, as after two clones each created a
    /// group with the same name.
    Merge {
        /// The group whose members move (an id, or a name that is not a duplicate).
        from: String,
        /// The group they join.
        into: String,
    },
    /// Rewrite every ticket that still stores a group *name* (written before group ids)
    /// to a registry id, creating one registry entry per distinct name. Safe to rerun.
    Migrate,
}

/// The project checkout whose registry names this store's groups: the nearest ancestor
/// of `cwd` that links to `store` (`.hotsheet2/store`) or is a registered checkout of
/// it. `None` when the CLI runs outside such a project.
pub(crate) fn checkout_root_for(store: &Path, cwd: &Path) -> Option<PathBuf> {
    let canonical = |path: &Path| path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let store = canonical(store);
    let registered = hotsheet_ticketing::checkouts::CheckoutRegistry::new(
        hotsheet_plugins::hotsheet_home().join("checkouts.json"),
    )
    .list()
    .unwrap_or_default();
    cwd.ancestors().find_map(|dir| {
        let linked = std::fs::read_to_string(dir.join(hotsheet_cli::STORE_LINK))
            .ok()
            .is_some_and(|text| {
                let text = text.trim();
                !text.is_empty() && canonical(Path::new(text)) == store
            });
        let registered = registered.iter().any(|checkout| {
            canonical(Path::new(&checkout.root)) == canonical(dir)
                && checkout
                    .stores
                    .iter()
                    .any(|root| canonical(Path::new(root)) == store)
        });
        (linked || registered).then(|| dir.to_path_buf())
    })
}

/// The registry of the project this CLI invocation runs in, if any.
pub(crate) fn registry_for(store: &FsStore) -> Option<GroupRegistry> {
    let cwd = std::env::current_dir().ok()?;
    checkout_root_for(store.root(), &cwd).map(|root| GroupRegistry::for_checkout(&root))
}

fn require_registry(store: &FsStore) -> Result<GroupRegistry> {
    registry_for(store).context(
        "group names need the project's checkout: run this inside a code checkout linked to \
         the store (`hotsheet-cli link <store>`), or pass group ids",
    )
}

/// Resolve a requested group change to the value to store: an id as is, a name through
/// the project's registry (creating the group when new). The registry is read only when
/// a group is set.
pub(crate) fn resolve_for_store(
    store: &FsStore,
    change: Option<Option<String>>,
) -> Result<Option<Option<String>>> {
    if !matches!(change, Some(Some(_))) {
        return Ok(change);
    }
    Ok(groups::resolve_group_change(
        registry_for(store).as_ref(),
        change,
    )?)
}

/// The registry ids a typed `--group` filter name resolves to (HS2-9KZSD4).
pub(crate) fn filter_ids(store: &FsStore, input: &str) -> Result<Vec<String>> {
    let (Some(registry), Some(key)) = (
        registry_for(store),
        hotsheet_model::group_key_of_input(input),
    ) else {
        return Ok(Vec::new());
    };
    Ok(registry
        .names()?
        .into_iter()
        .filter(|(_, name)| hotsheet_model::group_key(name) == key)
        .map(|(id, _)| id)
        .collect())
}

/// The display name of a stored group value for listings: the registry's name, a legacy
/// name as is, or the placeholder.
pub(crate) fn display_name(registry: Option<&groups::GroupNames>, value: &str) -> String {
    match hotsheet_model::GroupValue::of(value) {
        hotsheet_model::GroupValue::Id(id) => registry
            .and_then(|names| names.get(id).cloned())
            .unwrap_or_else(|| hotsheet_model::group_placeholder_name(id)),
        hotsheet_model::GroupValue::Legacy(name) => name.to_string(),
    }
}

pub(crate) fn cmd_groups(path: &Path, json: bool) -> Result<()> {
    let store = FsStore::open(path)?;
    let registry = registry_for(&store);
    let summaries = groups::list_groups(&GroupDirectory::scan(&store)?, registry.as_ref())?;
    // HS2-9EGWC4: how the project's registry was last committed and published.
    let sync = registry.as_ref().and_then(GroupRegistry::sync_status);
    if json {
        let mut body = serde_json::json!({
            "registry": registry.is_some(),
            "groups": summaries,
        });
        if let Some(sync) = &sync {
            body["registry_sync"] = serde_json::to_value(sync)?;
        }
        println!("{}", serde_json::to_string_pretty(&body)?);
        return Ok(());
    }
    if let Some(sync) = sync.filter(|sync| sync.state.needs_attention()) {
        eprintln!("Group registry not published: {}", sync.detail);
    }
    if summaries.is_empty() {
        println!("(no groups)");
    }
    for group in &summaries {
        let mut flags = Vec::new();
        if group.legacy {
            flags.push("legacy name; run `hotsheet-cli group migrate`");
        }
        if group.duplicate {
            flags.push("duplicate name; see `hotsheet-cli group merge`");
        }
        let flags = if flags.is_empty() {
            String::new()
        } else {
            format!("  ({})", flags.join("; "))
        };
        let id = if group.legacy { "-" } else { group.id.as_str() };
        println!("{:>5}  {:<10}  {}{flags}", group.count, id, group.name);
    }
    Ok(())
}

pub(crate) fn cmd_group(path: &Path, cmd: GroupCmd) -> Result<()> {
    let store = FsStore::open(path)?;
    let stores = [&store];
    match cmd {
        GroupCmd::Create { name } => {
            let registry = require_registry(&store)?;
            let (id, name, created) = groups::create_group(Some(&registry), &name)?;
            if created {
                println!("Created group '{name}' ({id}).");
            } else {
                println!("Group '{name}' already exists ({id}).");
            }
        }
        GroupCmd::Rename { group, to } => {
            let registry = require_registry(&store)?;
            let directory = GroupDirectory::scan(&store)?;
            let renamed = groups::rename_group(&stores, &directory, Some(&registry), &group, &to)?;
            let (id, name) = (
                renamed.id.unwrap_or_default(),
                renamed.name.unwrap_or_default(),
            );
            if renamed.updated == 0 {
                println!("Renamed group {id} to '{name}'.");
            } else {
                println!(
                    "Renamed group to '{name}' ({id}); moved {} ticket(s) off the legacy name.",
                    renamed.updated
                );
            }
        }
        GroupCmd::Ungroup { group } => {
            let directory = GroupDirectory::scan(&store)?;
            let outcome =
                groups::ungroup(&stores, &directory, registry_for(&store).as_ref(), &group)?;
            println!("Ungrouped {} ticket(s).", outcome.updated);
        }
        GroupCmd::Merge { from, into } => {
            let directory = GroupDirectory::scan(&store)?;
            let merged = groups::merge_groups(
                &stores,
                &directory,
                registry_for(&store).as_ref(),
                &from,
                &into,
            )?;
            println!(
                "Merged {} ticket(s) into '{}' ({}).",
                merged.updated,
                merged.name.unwrap_or_default(),
                merged.id.unwrap_or_default()
            );
        }
        GroupCmd::Migrate => {
            let Ok(registry) = require_registry(&store) else {
                bail!(
                    "group migrate needs the project's checkout, whose registry receives the \
                     names: run it inside a code checkout linked to the store"
                );
            };
            let directory = GroupDirectory::scan(&store)?;
            let migration = groups::migrate_groups(&stores, &directory, &registry)?;
            if migration.updated == 0 && migration.created == 0 && migration.reused == 0 {
                println!("No legacy group names to migrate.");
            } else {
                println!(
                    "Migrated {} ticket(s): created {} group(s), reused {} existing group(s).",
                    migration.updated, migration.created, migration.reused
                );
            }
        }
    }
    Ok(())
}
