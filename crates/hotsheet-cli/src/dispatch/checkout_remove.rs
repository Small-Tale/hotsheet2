//! `hotsheet-cli checkout remove` (HS2-X4RZ73): the headless path for removing a registered
//! checkout from the machine registry.
//!
//! Both paths run [`hotsheet_ticketing::checkout_removal::remove_checkout`]. When live
//! servers are registered under `HOTSHEET_HOME/instances`, the CLI first asks each of them
//! for a dry run, because only a server knows whether a client has the project open, and
//! then removes through one of them so its in-memory state follows. With no live server it
//! runs the same function directly against the `-C` store's `providers.json`, as
//! `checkout remove-source` does.

use std::path::Path;

use anyhow::{Context, Result};
use hotsheet_cli::external_launch::ServerInstance;
use hotsheet_ticketing::FsStore;
use hotsheet_ticketing::ProviderConfigRegistry;
use hotsheet_ticketing::checkout_removal::{
    CheckoutRemoval, TEMPORARY_REGISTRY, TemporaryCheckoutPrune, abandoned_temporary_candidates,
    clear_abandoned_skeleton, registry_is_temporary, remove_checkout,
};
use hotsheet_ticketing::checkouts::CheckoutRegistry;

use super::server::{process_is_alive, server_json_with_body};
use crate::urlencoding_component;

pub(crate) fn cmd_checkout_remove(
    registry: &CheckoutRegistry,
    store: &Path,
    reference: &str,
    dry_run: bool,
    json: bool,
) -> Result<()> {
    let servers = live_servers(&hotsheet_plugins::hotsheet_home().join("instances"));
    let report = remove_one(registry, store, reference, dry_run, &servers)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print!("{}", render(&report));
    }
    Ok(())
}

/// Remove one checkout through the live servers when there are any, else directly.
fn remove_one(
    registry: &CheckoutRegistry,
    store: &Path,
    reference: &str,
    dry_run: bool,
    servers: &[ServerInstance],
) -> Result<CheckoutRemoval> {
    let target = registry.find_for_removal(reference)?;
    let mut report = match (&target, servers) {
        (Some(checkout), [first, ..]) => {
            let route = |dry: bool| {
                format!(
                    "checkouts/{}{}",
                    urlencoding_component(&checkout.id),
                    if dry { "?dry_run=true" } else { "" }
                )
            };
            let mut plan = None;
            for server in servers {
                plan = Some(ask(server, &route(true))?);
            }
            match plan {
                Some(plan) if dry_run => plan,
                _ => ask(first, &route(false))?,
            }
        }
        _ => {
            let store_root = FsStore::open(store)
                .map(|opened| opened.root().to_path_buf())
                .unwrap_or_else(|_| store.to_path_buf());
            remove_checkout(
                &ProviderConfigRegistry::new(store_root.join("providers.json")),
                registry,
                reference,
                dry_run,
            )?
        }
    };
    report.reference = reference.to_owned();
    Ok(report)
}

/// `hotsheet-cli checkout prune` (HS2-HM0Y6X): unregister abandoned temporary checkouts,
/// the same set the server prunes when it starts.
pub(crate) fn cmd_checkout_prune(
    registry: &CheckoutRegistry,
    store: &Path,
    dry_run: bool,
    json: bool,
) -> Result<()> {
    let servers = live_servers(&hotsheet_plugins::hotsheet_home().join("instances"));
    let mut report = TemporaryCheckoutPrune {
        dry_run,
        skipped: registry_is_temporary(registry).then(|| TEMPORARY_REGISTRY.to_owned()),
        ..TemporaryCheckoutPrune::default()
    };
    for checkout in abandoned_temporary_candidates(registry)? {
        let removal = remove_one(registry, store, &checkout.id, dry_run, &servers)?;
        if removal.removed {
            clear_abandoned_skeleton(Path::new(&checkout.root));
        }
        if removal.registered {
            report.removed.push(removal);
        }
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else if let Some(reason) = &report.skipped {
        println!("Nothing pruned: {reason}.");
    } else if report.removed.is_empty() {
        println!("No abandoned temporary checkouts are registered.");
    } else {
        let verb = if dry_run { "Would remove" } else { "Removed" };
        for removal in &report.removed {
            println!(
                "{verb} {} ({})",
                removal.checkout_id.as_deref().unwrap_or_default(),
                removal.root.as_deref().unwrap_or_default()
            );
        }
    }
    Ok(())
}

fn ask(server: &ServerInstance, route: &str) -> Result<CheckoutRemoval> {
    let body = server_json_with_body(server, "DELETE", route, None).with_context(|| {
        format!(
            "the running Hot Sheet server at {} must confirm the project is closed before it is removed",
            server.url
        )
    })?;
    Ok(serde_json::from_value(body)?)
}

/// Live server registrations, one per URL. A record whose process is gone is stale.
fn live_servers(directory: &Path) -> Vec<ServerInstance> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut paths = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect::<Vec<_>>();
    paths.sort();
    let mut servers: Vec<ServerInstance> = Vec::new();
    for path in paths {
        let Some(server) = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str::<ServerInstance>(&text).ok())
        else {
            continue;
        };
        if process_is_alive(server.pid) && !servers.iter().any(|known| known.url == server.url) {
            servers.push(server);
        }
    }
    servers
}

pub(crate) fn render(report: &CheckoutRemoval) -> String {
    if !report.registered {
        return format!(
            "No checkout is registered as '{}'; nothing to remove.\n",
            report.reference
        );
    }
    let would = report.dry_run;
    let mut out = format!(
        "{} checkout {} ({}) at {} from the registry.\n",
        if would { "Would remove" } else { "Removed" },
        report.alias.as_deref().unwrap_or_default(),
        report.checkout_id.as_deref().unwrap_or_default(),
        report.root.as_deref().unwrap_or_default(),
    );
    for store in &report.kept_git_stores {
        out.push_str(&format!("Kept ticket store {store} (only unlinked).\n"));
    }
    for connection in &report.removed_connections {
        out.push_str(&format!(
            "{} connection '{connection}': no other project uses it.\n",
            if would { "Would delete" } else { "Deleted" }
        ));
    }
    for (connection, users) in &report.shared_connections {
        out.push_str(&format!(
            "Kept connection '{connection}', still used by: {}.\n",
            users.join(", ")
        ));
    }
    for credential in &report.kept_credentials {
        out.push_str(&format!("Account '{credential}' stays signed in.\n"));
    }
    out.push_str("The checkout directory and its files are not changed.\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hs2_x4rz73_render_names_what_is_removed_and_kept() {
        let absent = CheckoutRemoval {
            reference: "gone".into(),
            ..CheckoutRemoval::default()
        };
        assert_eq!(
            render(&absent),
            "No checkout is registered as 'gone'; nothing to remove.\n"
        );
        let mut report = CheckoutRemoval {
            reference: "app".into(),
            registered: true,
            removed: true,
            checkout_id: Some("app-0123456789ab".into()),
            alias: Some("app".into()),
            root: Some("/work/app".into()),
            kept_git_stores: vec!["/work/app.hs2".into()],
            removed_connections: vec!["github-main".into()],
            kept_credentials: vec!["github-app-01ABC".into()],
            ..CheckoutRemoval::default()
        };
        report
            .shared_connections
            .insert("github-docs".into(), vec!["docs-0123456789ab".into()]);
        let text = render(&report);
        assert!(text.starts_with(
            "Removed checkout app (app-0123456789ab) at /work/app from the registry.\n"
        ));
        assert!(text.contains("Kept ticket store /work/app.hs2 (only unlinked).\n"));
        assert!(text.contains("Deleted connection 'github-main': no other project uses it.\n"));
        assert!(
            text.contains("Kept connection 'github-docs', still used by: docs-0123456789ab.\n")
        );
        assert!(text.contains("Account 'github-app-01ABC' stays signed in.\n"));
        report.dry_run = true;
        report.removed = false;
        let text = render(&report);
        assert!(text.starts_with("Would remove checkout app"));
        assert!(text.contains("Would delete connection 'github-main'"));
    }

    #[test]
    fn hs2_x4rz73_live_servers_skip_stale_invalid_and_duplicate_records() {
        let dir = tempfile::tempdir().unwrap();
        let record = |name: &str, pid: u32, url: &str| {
            std::fs::write(
                dir.path().join(name),
                serde_json::json!({"pid": pid, "url": url, "secret": "fixture-only", "store_path": "/unused"})
                    .to_string(),
            )
            .unwrap();
        };
        let me = std::process::id();
        record("a.json", me, "http://127.0.0.1:9");
        record("b.json", me, "http://127.0.0.1:9");
        record("c.json", u32::MAX - 1, "http://127.0.0.1:10");
        std::fs::write(dir.path().join("d.json"), "not json").unwrap();
        std::fs::write(dir.path().join("e.txt"), "{}").unwrap();
        let servers = live_servers(dir.path());
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].url, "http://127.0.0.1:9");
        assert!(live_servers(&dir.path().join("missing")).is_empty());
    }
}
