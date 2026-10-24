//! `hotsheet-mcp` — the MCP shim binary. Reads newline-delimited JSON-RPC on stdin,
//! serves the `hotsheet_*` tools, and writes replies on stdout. Two modes:
//!   - `--path <store>`  → serverless, direct to disk (no server needed).
//!   - `--project <checkout>` → open a code project and all its registered sources.
//!   - `--server <url> --secret <s>` → proxy a running hotsheet-server over HTTP.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use clap::Parser;
use hotsheet_mcp::presence::{self, HttpPresence, Presence, PresenceTarget};
use hotsheet_mcp::{Backend, BackendError, CoreBackend, HttpBackend, handle_message_for_checkout};
use std::sync::Mutex;

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
    /// Registered code project path or identity. Uses the machine-local project server.
    #[arg(long)]
    project: Option<PathBuf>,
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
    if cli.project.is_some() && (cli.path.is_some() || cli.server.is_some() || cli.secret.is_some())
    {
        bail!("--project cannot be combined with --path, --server, or --secret");
    }
    let path = cli.path.map(|path| {
        resolve_store_path(
            path,
            &cwd,
            std::env::var_os("HOTSHEET_STORE").map(PathBuf::from),
        )
    });
    let project = cli
        .project
        .map(|reference| resolve_project_root(&reference, &cwd))
        .transpose()?;
    let (project_server, checkout_id) = if let Some(root) = project.as_ref() {
        let home = hotsheet_plugins::hotsheet_home();
        let bootstrap = home.join("server-bootstrap.hs2");
        let server = hotsheet_cli::external_launch::ensure_running_server(&bootstrap, &home)?;
        let id = open_project(root, &server.url, &server.secret)?;
        (Some((server.url, server.secret)), Some(id))
    } else {
        (None, None)
    };

    // HS2-HNXE38: the server this shim reports presence to, if any.
    let pid = std::process::id();
    let lease_id = format!(
        "mcp-{pid}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    );
    let presence_target = PresenceTarget::resolve(
        project_server
            .as_ref()
            .map(|(url, _)| url.as_str())
            .or(cli.server.as_deref()),
        project_server
            .as_ref()
            .map(|(_, secret)| secret.as_str())
            .or(cli.secret.as_deref()),
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
    let relocate_store = path.clone().or_else(|| {
        project
            .as_ref()
            .map(|_| hotsheet_plugins::hotsheet_home().join("server-bootstrap.hs2"))
    });
    let relocate = move || {
        relocate_store.as_deref().and_then(|store| {
            presence::discover_server(&hotsheet_plugins::hotsheet_home().join("instances"), store)
        })
    };
    let mut relocate = Some(relocate);
    let mut _presence: Option<Presence> = None;

    let backend: Box<dyn Backend> = match (path, cli.server, project_server) {
        (Some(path), None, None) => Box::new(CoreBackend::open(&path)?),
        (None, Some(server), None) => {
            let secret = cli
                .secret
                .ok_or_else(|| anyhow::anyhow!("--server requires --secret"))?;
            Box::new(HttpBackend::new(server, secret))
        }
        (None, None, Some((server, secret))) => {
            let home = hotsheet_plugins::hotsheet_home();
            let registry = std::fs::read(home.join("checkouts.json")).unwrap_or_default();
            Box::new(ProjectBackend {
                root: project.expect("project route has a root"),
                home,
                route: Mutex::new((server, secret, registry)),
            })
        }
        (Some(_), Some(_), _) => bail!("pass either --path (serverless) or --server, not both"),
        (None, None, None) => {
            bail!("pass --project <code project>, --path <store>, or --server <url> --secret <s>")
        }
        _ => bail!("invalid MCP backend configuration"),
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
        if let Some(response) =
            handle_message_for_checkout(&msg, backend.as_ref(), checkout_id.as_deref())
        {
            writeln!(stdout, "{}", serde_json::to_string(&response)?)?;
            stdout.flush()?;
        }
    }
    // stdin closed: dropping the presence handle releases the lease.
    drop(_presence);
    Ok(())
}

fn open_project(root: &Path, url: &str, secret: &str) -> Result<String> {
    let response = ureq::post(&format!("{url}/projects/open"))
        .set("X-Hotsheet-Secret", secret)
        .set("Content-Type", "application/json")
        .send_string(&serde_json::json!({"root":root}).to_string())?;
    let response: serde_json::Value = serde_json::from_str(&response.into_string()?)?;
    response["checkout"]["id"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| anyhow::anyhow!("project-open response has no checkout id"))
}

/// A project shim follows a restarted machine server without changing checkout identity.
struct ProjectBackend {
    root: PathBuf,
    home: PathBuf,
    route: Mutex<(String, String, Vec<u8>)>,
}

impl ProjectBackend {
    fn current_route(&self) -> Result<(String, String), BackendError> {
        let bootstrap = self.home.join("server-bootstrap.hs2");
        let server = hotsheet_cli::external_launch::ensure_running_server(&bootstrap, &self.home)
            .map_err(|error| BackendError {
            status: None,
            message: error.to_string(),
        })?;
        let mut route = self.route.lock().map_err(|error| BackendError {
            status: None,
            message: error.to_string(),
        })?;
        let registry = std::fs::read(self.home.join("checkouts.json")).unwrap_or_default();
        if route.0 != server.url || route.1 != server.secret || route.2 != registry {
            open_project(&self.root, &server.url, &server.secret).map_err(|error| {
                BackendError {
                    status: None,
                    message: error.to_string(),
                }
            })?;
            *route = (
                server.url,
                server.secret,
                std::fs::read(self.home.join("checkouts.json")).unwrap_or_default(),
            );
        }
        Ok((route.0.clone(), route.1.clone()))
    }
}

impl Backend for ProjectBackend {
    fn get(
        &self,
        path: &str,
        query: &[(String, String)],
    ) -> Result<serde_json::Value, BackendError> {
        let (url, secret) = self.current_route()?;
        HttpBackend::new(url, secret).get(path, query)
    }

    fn send(
        &self,
        method: &str,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, BackendError> {
        let (url, secret) = self.current_route()?;
        HttpBackend::new(url, secret).send(method, path, body)
    }
}

fn resolve_project_root(reference: &Path, cwd: &Path) -> Result<PathBuf> {
    let registry = hotsheet_ticketing::checkouts::CheckoutRegistry::new(
        hotsheet_plugins::hotsheet_home().join("checkouts.json"),
    );
    if let Ok(checkout) = registry.resolve(reference.to_string_lossy().as_ref()) {
        return Ok(PathBuf::from(checkout.root));
    }
    let path = if reference == Path::new(".") {
        cwd
    } else {
        reference
    };
    let canonical = path.canonicalize()?;
    if let Ok(checkout) = registry.resolve(canonical.to_string_lossy().as_ref()) {
        return Ok(PathBuf::from(checkout.root));
    }
    for parent in canonical.ancestors() {
        if let Ok(checkout) = registry.resolve(parent.to_string_lossy().as_ref()) {
            return Ok(PathBuf::from(checkout.root));
        }
    }
    Ok(canonical)
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
