//! Store bootstrap, linking, format, import, and maintenance commands.

use anyhow::{Result, bail};
use hotsheet_ticketing::FsStore;

use super::{Ctx, misrouted};
use crate::*;

pub(super) fn run(cmd: Cmd, ctx: &Ctx) -> Result<()> {
    let path = &ctx.path;
    match cmd {
        Cmd::Init {
            prefix,
            standalone,
            at,
            remote,
        } => cmd_init(path, &prefix, standalone, at.as_deref(), remote.as_deref()),
        Cmd::Bootstrap {
            project,
            store,
            prefix,
            remote,
            tools,
        } => cmd_bootstrap(
            &project,
            store.as_deref(),
            &prefix,
            remote.as_deref(),
            &tools,
        ),
        Cmd::Link { store } => cmd_link(&store),
        Cmd::ActivateFormat {
            acknowledge_pre_release_breakage,
        } => activate_format(path, acknowledge_pre_release_breakage),
        Cmd::Compatibility { json } => compatibility(path, json),
        Cmd::Import {
            file,
            prefix,
            diagnose_attachments,
            restore_attachment,
            confirm_omission,
        } => {
            if diagnose_attachments {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&hotsheet_cli::import_recovery::diagnose(
                        path, &file
                    )?)?
                );
                Ok(())
            } else if !restore_attachment.is_empty() || !confirm_omission.is_empty() {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&hotsheet_cli::import_recovery::recover(
                        path,
                        &file,
                        &restore_attachment,
                        &confirm_omission
                    )?)?
                );
                Ok(())
            } else {
                cmd_import(path, &file, &prefix)
            }
        }
        Cmd::Checkout { cmd } => cmd_checkout(
            cmd,
            &hotsheet_cli::resolve_store_path(path.clone(), &ctx.cwd),
        ),
        Cmd::Sync => cmd_sync(path),
        Cmd::Doctor { project } => cmd_doctor(path, &project),
        Cmd::Reindex { index } => cmd_reindex(path, index),
        Cmd::Worklist => cmd_worklist(path, &ctx.cwd),
        Cmd::PurgeTrash { older_than_days } => cmd_purge_trash(path, &ctx.cwd, older_than_days),
        Cmd::MergeDriver { base, ours, theirs } => cmd_merge_driver(&base, &ours, &theirs),
        _ => misrouted("store"),
    }
}

fn activate_format(path: &Path, acknowledged: bool) -> Result<()> {
    if !acknowledged {
        bail!(
            "format activation can break older pre-release HS2 processes; announce the change, stop them, then rerun with --acknowledge-pre-release-breakage"
        );
    }
    println!(
        "Activating a pre-release HS2 format boundary. Older HS2 processes may no longer open this store."
    );
    use std::io::Write as _;
    std::io::stdout().flush()?;
    FsStore::open(path)?.activate_current_format()?;
    println!(
        "Activated store format {}.",
        hotsheet_ticketing::STORE_SCHEMA_VERSION
    );
    Ok(())
}

fn compatibility(path: &Path, json: bool) -> Result<()> {
    let selected_store = path
        .join(hotsheet_ticketing::STORE_METADATA_FILE)
        .is_file()
        .then(|| std::fs::read(path.join(hotsheet_ticketing::STORE_METADATA_FILE)))
        .transpose()?
        .map(|bytes| serde_json::from_slice::<hotsheet_ticketing::StoreMetadata>(&bytes))
        .transpose()?
        .map(|metadata| metadata.schema_version);
    let value = serde_json::json!({
        "generation": "hs2",
        "application_version": env!("CARGO_PKG_VERSION"),
        "setup_assets_fingerprint": hotsheet_plugins::builtin_setup_assets_fingerprint(),
        "store_schema": {
            "min": 1,
            "max": hotsheet_ticketing::STORE_SCHEMA_VERSION,
            "creates": hotsheet_ticketing::STORE_SCHEMA_VERSION,
        },
        "selected_store_schema": selected_store,
    });
    if json {
        println!("{}", serde_json::to_string(&value)?);
    } else {
        println!(
            "Hot Sheet 2 CLI {} creates store schema {} and opens schemas 1–{}.",
            env!("CARGO_PKG_VERSION"),
            hotsheet_ticketing::STORE_SCHEMA_VERSION,
            hotsheet_ticketing::STORE_SCHEMA_VERSION
        );
        if let Some(schema) = selected_store {
            println!("Selected store uses schema {schema}.");
        }
    }
    Ok(())
}
