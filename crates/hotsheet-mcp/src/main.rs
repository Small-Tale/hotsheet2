//! `hotsheet-mcp` — the MCP shim binary. Reads newline-delimited JSON-RPC on stdin,
//! serves the `hotsheet_*` tools, and writes replies on stdout. Two modes:
//!   - `--path <store>`  → serverless, direct to disk (no server needed).
//!   - `--server <url> --secret <s>` → proxy a running hotsheet-server over HTTP.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use clap::Parser;
use hotsheet_mcp::presence::{self, HttpPresence, Presence, PresenceTarget};
use hotsheet_mcp::{Backend, CoreBackend, HttpBackend, handle_message};

#[derive(Parser)]
#[command(
    name = "hotsheet-mcp",
    version,
    about = "MCP shim exposing the hotsheet_* tools — serverless (--path) or over a server (--server)"
)]
struct Cli {
    /// Serve a store directly from disk (serverless). The headless default.
    #[arg(short = 'C', long = "path")]
    path: Option<PathBuf>,
    /// Base URL of a running server (proxy mode). Requires --secret.
    #[arg(long)]
    server: Option<String>,
    /// The server's shared secret (proxy mode only).
    #[arg(long)]
    secret: Option<String>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let cwd = std::env::current_dir()?;
    let path = cli.path.map(|path| {
        resolve_store_path(
            path,
            &cwd,
            std::env::var_os("HOTSHEET_STORE").map(PathBuf::from),
        )
    });

    // HS2-HNXE38: the server this shim reports presence to, if any.
    let pid = std::process::id();
    let lease_id = format!(
        "mcp-{pid}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    );
    let presence_target = PresenceTarget::resolve(
        cli.server.as_deref(),
        cli.secret.as_deref(),
        |key| std::env::var(key).ok(),
        // HS2-E4J72D: a serverless shim outside a Hot Sheet terminal finds the store's
        // running server in the machine-local instance registry.
        || {
            path.as_deref().and_then(|store| {
                presence::discover_server(
                    &hotsheet_plugins::hotsheet_home().join("instances"),
                    store,
                )
            })
        },
        pid,
        std::env::current_dir()
            .ok()
            .map(|dir| dir.display().to_string()),
        lease_id,
    );
    let mut presence_target = presence_target;
    // A failed heartbeat re-reads the registry for this store (stale terminal route-back,
    // server restarted on a new port). Proxy mode has no store path and never relocates.
    let relocate_store = path.clone();
    let relocate = move || {
        relocate_store.as_deref().and_then(|store| {
            presence::discover_server(&hotsheet_plugins::hotsheet_home().join("instances"), store)
        })
    };
    let mut relocate = Some(relocate);
    let mut _presence: Option<Presence> = None;

    let backend: Box<dyn Backend> = match (path, cli.server) {
        (Some(path), None) => Box::new(CoreBackend::open(&path)?),
        (None, Some(server)) => {
            let secret = cli
                .secret
                .ok_or_else(|| anyhow::anyhow!("--server requires --secret"))?;
            Box::new(HttpBackend::new(server, secret))
        }
        (Some(_), Some(_)) => bail!("pass either --path (serverless) or --server, not both"),
        (None, None) => bail!("pass --path <store> (serverless) or --server <url> --secret <s>"),
    };

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(msg) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue; // ignore un-parseable input rather than crashing the shim
        };
        if presence::is_initialize(&msg)
            && let (Some(target), Some(relocate)) = (presence_target.take(), relocate.take())
        {
            _presence = Some(Presence::start_relocating(
                target,
                HttpPresence::default(),
                presence::HEARTBEAT,
                relocate,
            ));
        }
        if let Some(response) = handle_message(&msg, backend.as_ref()) {
            writeln!(stdout, "{}", serde_json::to_string(&response)?)?;
            stdout.flush()?;
        }
    }
    // stdin closed: dropping the presence handle releases the lease.
    drop(_presence);
    Ok(())
}

/// A portable project MCP entry passes `--path .`. Resolve its local link at launch,
/// while preserving explicit paths supplied by a user or a wholly local config.
fn resolve_store_path(path: PathBuf, cwd: &Path, env_store: Option<PathBuf>) -> PathBuf {
    if path != Path::new(".") {
        return path;
    }
    if let Some(store) = env_store.filter(|store| !store.as_os_str().is_empty()) {
        return store;
    }
    for dir in cwd.ancestors() {
        for link in [".hotsheet2/store", ".hotsheet/store"] {
            if let Ok(contents) = std::fs::read_to_string(dir.join(link)) {
                let linked = contents.trim();
                if !linked.is_empty() {
                    return PathBuf::from(linked);
                }
            }
        }
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_path_follows_a_link_from_nested_project_directory() {
        let root = tempfile::tempdir().unwrap();
        let nested = root.path().join("src/nested");
        let link_dir = root.path().join(".hotsheet2");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(&link_dir).unwrap();
        let store = root.path().join("tickets");
        std::fs::write(link_dir.join("store"), format!("{}\n", store.display())).unwrap();
        assert_eq!(resolve_store_path(".".into(), &nested, None), store);
        assert_eq!(
            resolve_store_path(".".into(), &nested, Some(root.path().join("override"))),
            root.path().join("override")
        );
        assert_eq!(
            resolve_store_path("/explicit/store".into(), &nested, None),
            PathBuf::from("/explicit/store")
        );
    }
}
