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
    // HS2-JFE7JJ: a shim inside a Hot Sheet-launched session serves through the server
    // that launched it, never a second one it starts. Under a shared host (the Codex
    // app-server daemon) the inherited route belongs to another session and is ignored.
    let launched = launched_route(
        |key| std::env::var(key).ok(),
        presence::INHERITED_IDENTITY
            .iter()
            .any(|key| std::env::var_os(key).is_some())
            && presence::hosted_by_shared_host(
                &presence::ancestor_commands(hotsheet_plugins::shared_host::ANCESTOR_DEPTH),
                &hotsheet_plugins::shared_mcp_hosts(),
            ),
    );
    let (project_server, checkout_id) = if let Some(root) = project.as_ref() {
        let home = hotsheet_plugins::hotsheet_home();
        let (url, secret) = match &launched {
            Some(launched) => attach_route(launched, &home)?,
            None => {
                let bootstrap = home.join("server-bootstrap.hs2");
                let server =
                    hotsheet_cli::external_launch::ensure_running_server(&bootstrap, &home)?;
                (server.url, server.secret.into_inner())
            }
        };
        let id = open_project(root, &url, &secret)?;
        (Some((url, secret)), Some(id))
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
        // HS2-MRR6Z0: under a shared host (the Codex app-server daemon) the terminal environment
        // belongs to whichever session started that host, so it names no terminal here.
        presence::identity_env(
            |key| std::env::var(key).ok(),
            presence::INHERITED_IDENTITY
                .iter()
                .any(|key| hotsheet_cli::launched_env::value(key).is_some())
                && presence::hosted_by_shared_host(
                    &presence::ancestor_commands(hotsheet_plugins::shared_host::ANCESTOR_DEPTH),
                    &hotsheet_plugins::shared_mcp_hosts(),
                ),
        ),
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
        // HS2-4BG2QN: like a CLI command, each serverless write asks the running host to
        // sync its store, or says the commits may be local only.
        (Some(path), None, None) => Box::new(
            CoreBackend::open(&path)?.with_host_sync_kick(hotsheet_plugins::hotsheet_home()),
        ),
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
                launched,
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
    // HS2-081PXN: project identity and the custom bearer stay on the selected origin.
    let response = ureq::AgentBuilder::new()
        .redirects(0)
        .build()
        .post(&format!("{url}/projects/open"))
        .set("X-Hotsheet-Secret", secret)
        .set("Content-Type", "application/json")
        .send_string(&serde_json::json!({"root":root}).to_string())?;
    if (300..400).contains(&response.status()) {
        bail!("project-open redirect refused (HTTP {})", response.status());
    }
    let response: serde_json::Value = serde_json::from_str(&response.into_string()?)?;
    response["checkout"]["id"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| anyhow::anyhow!("project-open response has no checkout id"))
}

/// The route a Hot Sheet-launched session names for the server that launched it:
/// `HOTSHEET_SERVER` and `HOTSHEET_SECRET`, unless the shim runs under a shared host
/// whose environment belongs to another session (HS2-JFE7JJ, HS2-MRR6Z0).
fn launched_route(
    env: impl Fn(&str) -> Option<String>,
    shared_host: bool,
) -> Option<(String, String)> {
    let env = presence::identity_env(env, shared_host);
    let value = |key: &str| env(key).filter(|value| !value.trim().is_empty());
    Some((
        value("HOTSHEET_SERVER")?.trim_end_matches('/').to_owned(),
        value("HOTSHEET_SECRET")?,
    ))
}

/// Whether a live server recorded under `home` is the one at `route`. Only a server of
/// this shim's own machine home counts: a route inherited from another home (another
/// Hot Sheet's terminal) is never used.
fn recorded_here(route: &(String, String), home: &Path) -> bool {
    hotsheet_cli::external_launch::running_servers(home)
        .iter()
        .any(|server| {
            server.url.trim_end_matches('/') == route.0 && server.secret.expose() == route.1
        })
}

/// HS2-JFE7JJ: the server a launched session's shim uses. The launching server while it
/// runs (recorded live under this machine home); otherwise a machine host that is already
/// running (the launching server may have been replaced). Never one this shim starts: a
/// launched session must not leave a second server and broker behind, nor take over
/// another server's instance record.
fn attach_route(launched: &(String, String), home: &Path) -> Result<(String, String)> {
    if recorded_here(launched, home) {
        return Ok(launched.clone());
    }
    let machine = hotsheet_cli::external_launch::machine_host_store(home);
    let server =
        hotsheet_cli::external_launch::discover_running_server(&machine, home).map_err(|_| {
            anyhow::anyhow!(
                "the Hot Sheet server that launched this session ({}) is not running under this \
                 machine home, and no machine host is running; this session's MCP tools start \
                 no server of their own",
                launched.0
            )
        })?;
    Ok((server.url, server.secret.into_inner()))
}

/// A project shim follows a restarted machine server without changing checkout identity.
struct ProjectBackend {
    root: PathBuf,
    home: PathBuf,
    route: Mutex<(String, String, Vec<u8>)>,
    /// HS2-JFE7JJ: the launching server's route, when a Hot Sheet session started this
    /// shim. Then no server is ever started from here.
    launched: Option<(String, String)>,
}

impl ProjectBackend {
    // HS2-4J6V46: cleanup must not reopen/register a missing owner or start/relink
    // a server. A stale cached endpoint returns its ordinary connection/auth error.
    fn route_for_request(
        &self,
        method: &str,
        path: &str,
    ) -> Result<(String, String), BackendError> {
        if hotsheet_cli::orchestration::is_cleanup_request(method, path) {
            return self
                .route
                .lock()
                .map(|route| (route.0.clone(), route.1.clone()))
                .map_err(|_| BackendError {
                    status: None,
                    message: "cached machine route is unavailable".into(),
                });
        }
        self.current_route()
    }

    fn current_route(&self) -> Result<(String, String), BackendError> {
        let failed = |error: anyhow::Error| BackendError {
            status: None,
            message: error.to_string(),
        };
        let server = match &self.launched {
            // HS2-JFE7JJ: the route the session started with, rechecked only after a
            // request to it failed (see `route_for_request`).
            Some(_) => {
                let route = self.route.lock().map_err(|error| BackendError {
                    status: None,
                    message: error.to_string(),
                })?;
                return Ok((route.0.clone(), route.1.clone()));
            }
            None => {
                let bootstrap = self.home.join("server-bootstrap.hs2");
                hotsheet_cli::external_launch::ensure_running_server(&bootstrap, &self.home)
                    .map_err(failed)?
            }
        };
        let mut route = self.route.lock().map_err(|error| BackendError {
            status: None,
            message: error.to_string(),
        })?;
        let registry = std::fs::read(self.home.join("checkouts.json")).unwrap_or_default();
        if route.0 != server.url || route.1 != server.secret.expose() || route.2 != registry {
            open_project(&self.root, &server.url, server.secret.expose()).map_err(|error| {
                BackendError {
                    status: None,
                    message: error.to_string(),
                }
            })?;
            *route = (
                server.url,
                server.secret.into_inner(),
                std::fs::read(self.home.join("checkouts.json")).unwrap_or_default(),
            );
        }
        Ok((route.0.clone(), route.1.clone()))
    }
}

impl Backend for ProjectBackend {
    fn machine_local(&self) -> bool {
        // HS2-1Y45MZ: capability preflight reads the cached origin without launching
        // or relocating a server. HttpBackend rechecks the actual route before sending.
        self.route.lock().ok().is_some_and(|route| {
            hotsheet_ticketing::remote_servers::remote_registry_origin_is_loopback(&route.0)
        })
    }
    fn get(
        &self,
        path: &str,
        query: &[(String, String)],
    ) -> Result<serde_json::Value, BackendError> {
        self.call("GET", path, |backend| backend.get(path, query))
    }

    fn send(
        &self,
        method: &str,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, BackendError> {
        self.call(method, path, |backend| backend.send(method, path, body))
    }
}

impl ProjectBackend {
    /// One request on the current route. HS2-JFE7JJ: a launched session whose server no
    /// longer answers (no HTTP status) attaches once to a running machine host and asks
    /// again; it never starts a server.
    fn call(
        &self,
        method: &str,
        path: &str,
        request: impl Fn(&HttpBackend) -> Result<serde_json::Value, BackendError>,
    ) -> Result<serde_json::Value, BackendError> {
        let (url, secret) = self.route_for_request(method, path)?;
        let answer = request(&HttpBackend::new(url.clone(), secret));
        let unreachable = matches!(&answer, Err(error) if error.status.is_none());
        let Some(launched) = self.launched.as_ref().filter(|_| unreachable) else {
            return answer;
        };
        if hotsheet_cli::orchestration::is_cleanup_request(method, path) {
            return answer;
        }
        let Ok((next_url, next_secret)) = attach_route(launched, &self.home) else {
            return answer;
        };
        if next_url == url {
            return answer;
        }
        open_project(&self.root, &next_url, &next_secret).map_err(|error| BackendError {
            status: None,
            message: error.to_string(),
        })?;
        if let Ok(mut route) = self.route.lock() {
            route.0.clone_from(&next_url);
            route.1.clone_from(&next_secret);
        }
        request(&HttpBackend::new(next_url, next_secret))
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
    let canonical = hotsheet_sync::paths::canonicalize(path)?;
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

    /// HS2-JFE7JJ: the launched route is the terminal's own server and secret, trimmed;
    /// blank values name none, and a shim under a shared host ignores the inherited pair.
    #[test]
    fn hs2_jfe7jj_the_launched_route_comes_from_the_terminal_unless_a_shared_host_runs_it() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |key: &str| {
                pairs
                    .iter()
                    .find(|(name, _)| *name == key)
                    .map(|(_, value)| (*value).to_owned())
            }
        };
        let terminal = env(&[
            ("HOTSHEET_SERVER", "http://127.0.0.1:4175/"),
            ("HOTSHEET_SECRET", "launching-secret"),
        ]);
        assert_eq!(
            launched_route(terminal, false),
            Some(("http://127.0.0.1:4175".into(), "launching-secret".into()))
        );
        assert_eq!(launched_route(terminal, true), None);
        assert_eq!(
            launched_route(env(&[("HOTSHEET_SERVER", "http://127.0.0.1:4175")]), false),
            None
        );
        assert_eq!(
            launched_route(
                env(&[("HOTSHEET_SERVER", " "), ("HOTSHEET_SECRET", "s")]),
                false
            ),
            None
        );
        assert_eq!(launched_route(env(&[]), false), None);
    }

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
    #[test]
    fn hs2_1y45mz_project_registry_preflight_accepts_only_cached_literal_loopback() {
        for (origin, allowed) in [
            ("http://127.0.0.1:1234", true),
            ("https://[::1]:1234", true),
            ("http://localhost:1234", false),
            ("https://remote.example.test", false),
            ("not an origin", false),
        ] {
            let backend = ProjectBackend {
                root: PathBuf::from("unused-synthetic-project"),
                home: PathBuf::from("unused-synthetic-home"),
                route: Mutex::new((origin.into(), "synthetic".into(), Vec::new())),
                launched: None,
            };
            assert_eq!(backend.machine_local(), allowed);
        }
        let backend = ProjectBackend {
            root: PathBuf::new(),
            home: PathBuf::new(),
            route: Mutex::new((
                "http://127.0.0.1:1234".into(),
                "synthetic".into(),
                Vec::new(),
            )),
            launched: None,
        };
        let _ = std::panic::catch_unwind(|| {
            let _guard = backend.route.lock().unwrap();
            panic!("synthetic lock interruption");
        });
        assert!(!backend.machine_local());
    }
}

#[cfg(test)]
mod project_cleanup_tests;
