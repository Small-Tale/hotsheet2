//! The local server: serving, certificates, lifecycle, and live server-backed reads.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::Subcommand;

use crate::{Cmd, urlencoding_component};

use super::{Ctx, misrouted};

pub(super) fn run(cmd: Cmd, ctx: &Ctx) -> Result<()> {
    let path = &ctx.path;
    match cmd {
        Cmd::Commands { cmd } => cmd_commands(path, cmd),
        Cmd::Terminal { cmd } => cmd_terminal(path, cmd),
        Cmd::Lifecycle { cmd } => match cmd {
            LifecycleCmd::Status => print_local_server_json(path, "GET", "lifecycle/quiescence"),
            LifecycleCmd::Restart => print_local_server_json(path, "POST", "lifecycle/restart"),
        },
        Cmd::TicketFlow => print_local_server_json(path, "GET", "analytics/tickets"),
        Cmd::Activity {
            ticket,
            session,
            min_importance,
            limit,
        } => cmd_activity(path, ticket, session, min_importance, limit),
        Cmd::Notifications { cmd } => cmd_notifications(path, cmd),
        Cmd::Serve {
            bind,
            secret,
            stop,
            kill_all_terminals,
            force,
            list,
        } => cmd_serve(
            path,
            &bind,
            secret,
            ServeStop {
                stop,
                kill_all_terminals,
                force,
            },
            list,
        ),
        Cmd::Cert { cmd } => cmd_cert(path, &cmd),
        _ => misrouted("server"),
    }
}

/// Run the server for this store in the foreground by exec'ing the sibling
/// `hotsheet-server` binary (the CLI stays free of a server dependency). Detached +
/// supervised start is client-owned (HS2-59 / HS2-4072GM).
/// The explicit-stop flags of `serve` (HS2-9AWEX5 adds `force`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ServeStop {
    pub stop: bool,
    pub kill_all_terminals: bool,
    pub force: bool,
}

pub(crate) fn cmd_serve(
    path: &Path,
    bind: &str,
    secret: Option<String>,
    stop: ServeStop,
    list: bool,
) -> Result<()> {
    if list {
        return cmd_list_servers();
    }
    let current = std::env::current_exe().context("could not locate hotsheet-cli")?;
    let exe = resolve_server_binary(&current, std::env::var_os("PATH").as_deref())?;
    verify_server_version(&exe)?;

    let mut cmd = hotsheet_ticketing::git::launch(&exe);
    cmd.args(server_args(
        path,
        bind,
        secret.as_deref(),
        stop,
        (!stop.stop).then_some("cli"),
    ));
    let status = cmd.status().map_err(|e| {
        anyhow::anyhow!(
            "could not launch `{}`: {e} — is hotsheet-server installed alongside hotsheet-cli?",
            exe.display()
        )
    })?;
    if !status.success() {
        bail!("hotsheet-server exited with {status}");
    }
    Ok(())
}

/// Manage the project's Tier-1 mTLS material (HS2-VT3JMF): a per-project CA, per-device
/// client certs, and revocation. Lives under `${HOTSHEET_HOME}/tls/<project-id>/`.
pub(crate) fn cmd_cert(store_path: &Path, cmd: &CertCmd) -> Result<()> {
    let paths = hotsheet_tls::Paths::for_store(store_path);
    match cmd {
        CertCmd::Init { hosts } => {
            hotsheet_tls::init_ca(&paths, hosts)?;
            println!(
                "Initialized mTLS for this store under {}",
                paths.dir.display()
            );
            println!("  CA cert:     {}", paths.ca_cert().display());
            println!("  server cert: {}", paths.server_cert().display());
            print!("  server SANs: localhost, 127.0.0.1");
            for h in hosts {
                print!(", {h}");
            }
            println!();
            println!("Next: `hotsheet-cli cert issue <device-name>` to enroll a client, then");
            println!("bind the server off-loopback (e.g. `serve --bind 0.0.0.0:8787`).");
        }
        CertCmd::Issue { name } => {
            let dev = hotsheet_tls::issue_device(&paths, name)?;
            eprintln!(
                "Issued client cert for '{}' (fingerprint {}). Copy all three PEMs to the device:",
                dev.name, dev.fingerprint
            );
            println!("# ===== client certificate ({}.crt) =====", dev.name);
            print!("{}", dev.cert_pem);
            println!("# ===== client private key ({}.key) =====", dev.name);
            print!("{}", dev.key_pem);
            println!("# ===== project CA (ca.crt — to verify the server) =====");
            print!("{}", dev.ca_pem);
        }
        CertCmd::Renew { name } => {
            let dev = hotsheet_tls::renew_device(&paths, name)?;
            eprintln!(
                "Renewed '{}' with a fresh 90-day certificate (fingerprint {}). The old certificate is revoked:",
                dev.name, dev.fingerprint
            );
            println!("# ===== client certificate ({}.crt) =====", dev.name);
            print!("{}", dev.cert_pem);
            println!("# ===== client private key ({}.key) =====", dev.name);
            print!("{}", dev.key_pem);
            println!("# ===== project CA (ca.crt — to verify the server) =====");
            print!("{}", dev.ca_pem);
        }
        CertCmd::Role { name, role } => {
            let role = match role.as_str() {
                "read-only" | "read_only" => hotsheet_tls::DeviceRole::ReadOnly,
                "read-write" | "read_write" => hotsheet_tls::DeviceRole::ReadWrite,
                "deny" => hotsheet_tls::DeviceRole::Deny,
                _ => bail!("role must be read-only, read-write, or deny"),
            };
            let fpr = hotsheet_tls::set_device_role(&paths, name, role)?;
            println!("Set '{name}' ({fpr}) to {role:?}. This applies on the next request.");
        }
        CertCmd::Revoke { name } => {
            let fpr = hotsheet_tls::revoke_device(&paths, name)?;
            println!("Revoked '{name}' ({fpr}). This applies on the next TLS connection.");
        }
    }
    Ok(())
}

/// Command runs live in the server's checkout-scoped manager. Route the CLI through that
/// manager so the web client and CLI share history, cancellation, and secret authorization.
pub(crate) fn cmd_commands(store: &Path, cmd: CommandsCmd) -> Result<()> {
    let (method, route) = match cmd {
        CommandsCmd::List { checkout } => (
            "GET",
            format!("checkouts/{}/commands", urlencoding_component(&checkout)),
        ),
        CommandsCmd::Groups { checkout } => (
            "GET",
            format!(
                "checkouts/{}/command-groups",
                urlencoding_component(&checkout)
            ),
        ),
        CommandsCmd::Run { checkout, id } => (
            "POST",
            format!(
                "checkouts/{}/commands/{}/run",
                urlencoding_component(&checkout),
                urlencoding_component(&id)
            ),
        ),
        CommandsCmd::History { checkout } => (
            "GET",
            format!(
                "checkouts/{}/command-runs",
                urlencoding_component(&checkout)
            ),
        ),
        CommandsCmd::Get {
            checkout,
            id,
            after,
        } => (
            "GET",
            format!(
                "checkouts/{}/command-runs/{}?after={after}",
                urlencoding_component(&checkout),
                urlencoding_component(&id)
            ),
        ),
        CommandsCmd::Cancel { checkout, id } => (
            "POST",
            format!(
                "checkouts/{}/command-runs/{}/cancel",
                urlencoding_component(&checkout),
                urlencoding_component(&id)
            ),
        ),
    };
    print_local_server_json(store, method, &route)
}

pub(crate) fn cmd_terminal(store: &Path, cmd: TerminalCmd) -> Result<()> {
    match cmd {
        TerminalCmd::Message { id, text } => {
            if text.trim().is_empty() {
                bail!("message text must not be empty");
            }
            let route = format!("connections/{}/message", urlencoding_component(&id));
            let body = serde_json::json!({ "text": text });
            let result = local_server_json_with_body(store, "POST", &route, Some(&body))?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            Ok(())
        }
    }
}

pub(crate) fn cmd_activity(
    store: &Path,
    ticket: Option<String>,
    session: Option<String>,
    min_importance: Option<String>,
    limit: Option<usize>,
) -> Result<()> {
    let mut filters = Vec::new();
    if let Some(value) = ticket {
        filters.push(("ticket", value));
    }
    if let Some(value) = session {
        filters.push(("session", value));
    }
    if let Some(value) = min_importance {
        filters.push(("min_importance", value));
    }
    if let Some(value) = limit {
        filters.push(("limit", value.to_string()));
    }
    print_local_server_json(store, "GET", &filtered_route("activity", &filters))
}

pub(crate) fn cmd_notifications(store: &Path, cmd: NotificationsCmd) -> Result<()> {
    let (method, route) = match cmd {
        NotificationsCmd::List {
            checkout,
            store,
            ticket,
            recipient,
        } => {
            let filters = [
                ("checkout", checkout),
                ("store", store),
                ("ticket", ticket),
                ("recipient", recipient),
            ]
            .into_iter()
            .filter_map(|(key, value)| value.map(|value| (key, value)))
            .collect::<Vec<_>>();
            ("GET", filtered_route("notifications", &filters))
        }
        NotificationsCmd::Ack { id } => (
            "POST",
            format!("notifications/{}/ack", urlencoding_component(&id)),
        ),
    };
    print_local_server_json(store, method, &route)
}

#[derive(Subcommand)]
pub(crate) enum CertCmd {
    /// Create the per-project CA + server cert (refuses if one already exists). Pass `--host`
    /// once per IP/DNS the server will be reached at off-loopback; `localhost` + `127.0.0.1`
    /// are always covered.
    Init {
        #[arg(long = "host")]
        hosts: Vec<String>,
    },
    /// Issue a client (device) cert signed by the project CA, printing the cert + key + CA
    /// PEMs to copy to the device. `name` identifies it for later revocation.
    Issue { name: String },
    /// Rotate a device certificate, revoking the old leaf and printing a fresh 90-day leaf.
    Renew { name: String },
    /// Set an issued device's explicit ACL role: read-only, read-write, or deny.
    Role { name: String, role: String },
    /// Revoke a previously-issued device by name (takes effect on the next TLS connection).
    Revoke { name: String },
}

#[derive(Subcommand)]
pub(crate) enum CommandsCmd {
    /// List saved command definitions for a checkout.
    List { checkout: String },
    /// List saved command groups, including empty groups.
    Groups { checkout: String },
    /// Start a saved command and print its run record.
    Run { checkout: String, id: String },
    /// List recent runs for this checkout.
    History { checkout: String },
    /// Get a run, optionally returning only output after a sequence number.
    Get {
        checkout: String,
        id: String,
        #[arg(long, default_value_t = 0)]
        after: u64,
    },
    /// Cancel an active run in this checkout.
    Cancel { checkout: String, id: String },
}

#[derive(Subcommand)]
pub(crate) enum TerminalCmd {
    /// Deliver a prompt to a live terminal AI session.
    Message {
        /// Terminal connection id.
        id: String,
        /// Message text to deliver; quote it to preserve spaces.
        #[arg(long)]
        text: String,
    },
}

#[derive(Subcommand)]
pub(crate) enum LifecycleCmd {
    /// Print whether the server is quiescing and which active work would block a restart.
    Status,
    /// Ask the server to restart once all server-owned work is quiescent. A busy server
    /// refuses and keeps its work; repeating the request is safe.
    Restart,
}

#[derive(Subcommand)]
pub(crate) enum NotificationsCmd {
    /// List notifications, optionally filtering by target or recipient.
    List {
        #[arg(long)]
        checkout: Option<String>,
        #[arg(long)]
        store: Option<String>,
        #[arg(long)]
        ticket: Option<String>,
        #[arg(long)]
        recipient: Option<String>,
    },
    /// Acknowledge one notification by its stable id.
    Ack { id: String },
}

#[derive(Clone, Debug, serde::Deserialize, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct RegisteredServer {
    pid: u32,
    url: String,
    store_path: String,
    started_at: String,
    /// Launch identity recorded by newer servers (HS2-GWFEAA); absent in legacy records.
    #[serde(default)]
    launcher: Option<String>,
    #[serde(default)]
    build_revision: Option<String>,
    #[serde(default)]
    executable: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct RegistryRecord {
    source: PathBuf,
    server: Option<RegisteredServer>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct ServerIdentity {
    pid: u32,
    url: String,
    started_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ServerInventoryEntry {
    identity: ServerIdentity,
    status: &'static str,
    stores: Vec<String>,
    /// Launch identity from the registration (HS2-GWFEAA); `None` for legacy records.
    launcher: Option<String>,
    build_revision: Option<String>,
    executable: Option<String>,
}

pub(crate) fn cmd_list_servers() -> Result<()> {
    let directory = hotsheet_plugins::hotsheet_home().join("instances");
    let records = read_registry_records(&directory)?;
    let agent = ureq::AgentBuilder::new()
        .user_agent("hotsheet2-server-inventory")
        .timeout_connect(std::time::Duration::from_millis(300))
        .timeout_read(std::time::Duration::from_millis(500))
        .build();
    let inventory = server_inventory(records, process_is_alive, |url| {
        match agent
            .get(&format!("{}/health", url.trim_end_matches('/')))
            .call()
        {
            Ok(response) => {
                response
                    .into_string()
                    .ok()
                    .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
                    .and_then(|value| {
                        value
                            .get("status")
                            .and_then(|status| status.as_str())
                            .map(str::to_owned)
                    })
                    .as_deref()
                    == Some("ok")
            }
            Err(_) => false,
        }
    });
    let pruned = prune_stale_records(&inventory.stale_records);
    print!("{}", render_server_inventory(&inventory, pruned));
    Ok(())
}

pub(crate) fn read_registry_records(directory: &Path) -> Result<Vec<RegistryRecord>> {
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error).context("could not read server instance registry"),
    };
    let mut records = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .map(|source| RegistryRecord {
            server: std::fs::read_to_string(&source)
                .ok()
                .and_then(|text| serde_json::from_str(&text).ok()),
            source,
        })
        .collect::<Vec<_>>();
    records.sort_by(|left, right| left.source.cmp(&right.source));
    Ok(records)
}

/// The registry inventory: live and stale servers, unparseable records, and the files of
/// dead registrations that `serve --list` prunes (HS2-GWFEAA).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ServerInventory {
    pub(crate) servers: Vec<ServerInventoryEntry>,
    pub(crate) invalid: Vec<PathBuf>,
    /// Registration files whose recorded pid is no longer alive.
    pub(crate) stale_records: Vec<PathBuf>,
}

#[derive(Default)]
struct GroupedRegistration {
    stores: BTreeSet<String>,
    sources: Vec<PathBuf>,
    launcher: Option<String>,
    build_revision: Option<String>,
    executable: Option<String>,
}

pub(crate) fn server_inventory(
    records: Vec<RegistryRecord>,
    is_alive: impl Fn(u32) -> bool,
    is_healthy: impl Fn(&str) -> bool,
) -> ServerInventory {
    let mut grouped = BTreeMap::<ServerIdentity, GroupedRegistration>::new();
    let mut invalid = Vec::new();
    for record in records {
        let Some(server) = record.server else {
            invalid.push(record.source);
            continue;
        };
        let identity = ServerIdentity {
            pid: server.pid,
            url: server.url,
            started_at: server.started_at,
        };
        let group = grouped.entry(identity).or_default();
        group.stores.insert(server.store_path);
        group.sources.push(record.source);
        group.launcher = group.launcher.take().or(server.launcher);
        group.build_revision = group.build_revision.take().or(server.build_revision);
        group.executable = group.executable.take().or(server.executable);
    }
    let mut stale_records = Vec::new();
    let mut servers = grouped
        .into_iter()
        .map(|(identity, group)| {
            let status = if !is_alive(identity.pid) {
                stale_records.extend(group.sources);
                "stale"
            } else if is_healthy(&identity.url) {
                "healthy"
            } else {
                "unhealthy"
            };
            ServerInventoryEntry {
                identity,
                status,
                stores: group.stores.into_iter().collect(),
                launcher: group.launcher,
                build_revision: group.build_revision,
                executable: group.executable,
            }
        })
        .collect::<Vec<_>>();
    servers.sort_by_key(|server| {
        (
            match server.status {
                "healthy" => 0,
                "unhealthy" => 1,
                _ => 2,
            },
            server.identity.pid,
        )
    });
    stale_records.sort();
    ServerInventory {
        servers,
        invalid,
        stale_records,
    }
}

pub(crate) fn render_server_inventory(inventory: &ServerInventory, pruned: usize) -> String {
    let ServerInventory {
        servers, invalid, ..
    } = inventory;
    if servers.is_empty() && invalid.is_empty() {
        return "no registered Hot Sheet servers\n".into();
    }
    let mut output = String::new();
    for server in servers {
        output.push_str(&format!(
            "{:<9} pid={} url={} started={} launcher={} build={}\n",
            server.status,
            server.identity.pid,
            server.identity.url,
            server.identity.started_at,
            server.launcher.as_deref().unwrap_or("unknown"),
            server.build_revision.as_deref().unwrap_or("unknown"),
        ));
        if let Some(executable) = &server.executable {
            output.push_str(&format!("  executable: {executable}\n"));
        }
        for store in &server.stores {
            output.push_str(&format!("  store: {store}\n"));
        }
    }
    for path in invalid {
        output.push_str(&format!(
            "stale     invalid registry record: {}\n",
            path.display()
        ));
    }
    if pruned > 0 {
        output.push_str(&format!(
            "pruned {pruned} stale registration file(s) whose server process is gone\n"
        ));
    }
    output
}

/// Remove the registration files of dead servers (HS2-GWFEAA). Live and unparseable
/// records are never touched; a file that disappears meanwhile counts as pruned.
pub(crate) fn prune_stale_records(stale_records: &[PathBuf]) -> usize {
    stale_records
        .iter()
        .filter(|path| match std::fs::remove_file(path) {
            Ok(()) => true,
            Err(error) => error.kind() == std::io::ErrorKind::NotFound,
        })
        .count()
}

#[cfg(unix)]
pub(crate) fn process_is_alive(pid: u32) -> bool {
    if pid == 0 || pid > i32::MAX as u32 {
        return false;
    }
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(not(unix))]
pub(crate) fn process_is_alive(_pid: u32) -> bool {
    false
}

pub(crate) fn resolve_server_binary(
    current_exe: &Path,
    path: Option<&std::ffi::OsStr>,
) -> Result<PathBuf> {
    let sibling = current_exe
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!("hotsheet-server{}", std::env::consts::EXE_SUFFIX));
    if sibling.is_file() {
        return Ok(sibling);
    }
    if let Some(found) = path.and_then(|paths| {
        std::env::split_paths(paths)
            .map(|dir| dir.join(format!("hotsheet-server{}", std::env::consts::EXE_SUFFIX)))
            .find(|candidate| candidate.is_file())
    }) {
        return Ok(found);
    }
    bail!(
        "hotsheet-server is not installed: expected it beside hotsheet-cli at {} or on PATH",
        sibling.display()
    )
}

pub(crate) fn verify_server_version(exe: &Path) -> Result<()> {
    let output = hotsheet_ticketing::git::launch(exe)
        .arg("--version")
        .output()
        .with_context(|| format!("could not run {} --version", exe.display()))?;
    if !output.status.success() {
        bail!("{} --version exited with {}", exe.display(), output.status);
    }
    let reported = String::from_utf8_lossy(&output.stdout);
    let version = reported.split_whitespace().next_back().unwrap_or_default();
    if version != env!("CARGO_PKG_VERSION") {
        bail!(
            "hotsheet-cli {} cannot launch hotsheet-server {} at {}; install matching binaries",
            env!("CARGO_PKG_VERSION"),
            if version.is_empty() {
                "(unknown)"
            } else {
                version
            },
            exe.display()
        );
    }
    Ok(())
}

pub(crate) fn server_args(
    path: &Path,
    bind: &str,
    secret: Option<&str>,
    stop: ServeStop,
    launcher: Option<&str>,
) -> Vec<std::ffi::OsString> {
    let mut args = vec![
        "-C".into(),
        path.as_os_str().to_owned(),
        "--bind".into(),
        bind.into(),
    ];
    if let Some(secret) = secret {
        args.extend(["--secret".into(), secret.into()]);
    }
    if stop.stop {
        args.push("--stop".into());
    }
    if stop.kill_all_terminals {
        args.push("--kill-all-terminals".into());
    }
    if stop.force {
        args.push("--force".into());
    }
    if let Some(launcher) = launcher {
        args.extend(["--launcher".into(), launcher.into()]);
    }
    args
}

pub(crate) fn filtered_route(base: &str, filters: &[(&str, String)]) -> String {
    if filters.is_empty() {
        return base.to_owned();
    }
    let query = filters
        .iter()
        .map(|(key, value)| format!("{key}={}", urlencoding_component(value)))
        .collect::<Vec<_>>()
        .join("&");
    format!("{base}?{query}")
}

pub(crate) fn print_local_server_json(store: &Path, method: &str, route: &str) -> Result<()> {
    let body = local_server_json(store, method, route)?;
    println!("{}", serde_json::to_string_pretty(&body)?);
    Ok(())
}

pub(crate) fn local_server_json(
    store: &Path,
    method: &str,
    route: &str,
) -> Result<serde_json::Value> {
    local_server_json_with_body(store, method, route, None)
}

fn local_server_json_with_body(
    store: &Path,
    method: &str,
    route: &str,
    body: Option<&serde_json::Value>,
) -> Result<serde_json::Value> {
    let server = hotsheet_cli::external_launch::discover_running_server(
        store,
        &hotsheet_plugins::hotsheet_home(),
    )?;
    let endpoint = format!("{}/{}", server.url.trim_end_matches('/'), route);
    let request = ureq::request(method, &endpoint)
        .set("X-Hotsheet-Secret", &server.secret)
        .timeout(std::time::Duration::from_secs(10));
    let result = match body {
        Some(body) => request
            .set("Content-Type", "application/json")
            .send_string(&serde_json::to_string(body)?),
        None => request.call(),
    };
    let response = match result {
        Ok(response) => response,
        Err(ureq::Error::Status(status, response)) => {
            let body = response.into_string().unwrap_or_default();
            let message = serde_json::from_str::<serde_json::Value>(&body)
                .ok()
                .and_then(|body| body["error"].as_str().map(str::to_owned))
                .unwrap_or_else(|| format!("server returned HTTP {status}"));
            bail!("Hot Sheet server request failed: {message}");
        }
        Err(error) => return Err(error.into()),
    };
    Ok(serde_json::from_reader(response.into_reader())?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sibling_server_wins_over_path_and_arguments_are_forwarded_exactly() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        let elsewhere = dir.path().join("elsewhere");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&elsewhere).unwrap();
        let cli = bin.join(format!("hotsheet-cli{}", std::env::consts::EXE_SUFFIX));
        let sibling = bin.join(format!("hotsheet-server{}", std::env::consts::EXE_SUFFIX));
        let path_server =
            elsewhere.join(format!("hotsheet-server{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&sibling, "").unwrap();
        std::fs::write(&path_server, "").unwrap();

        assert_eq!(
            resolve_server_binary(&cli, Some(elsewhere.as_os_str())).unwrap(),
            sibling
        );
        assert_eq!(
            server_args(
                Path::new("store path"),
                "127.0.0.1:0",
                Some("secret"),
                ServeStop {
                    stop: true,
                    kill_all_terminals: true,
                    force: false,
                },
                None
            ),
            [
                "-C",
                "store path",
                "--bind",
                "127.0.0.1:0",
                "--secret",
                "secret",
                "--stop",
                "--kill-all-terminals"
            ]
            .map(std::ffi::OsString::from)
        );
        // HS2-9AWEX5: a forced stop forwards --force so the server stops a pinned owner.
        assert_eq!(
            server_args(
                Path::new("s"),
                "127.0.0.1:0",
                None,
                ServeStop {
                    stop: true,
                    kill_all_terminals: false,
                    force: true,
                },
                None
            ),
            ["-C", "s", "--bind", "127.0.0.1:0", "--stop", "--force"].map(std::ffi::OsString::from)
        );
        // HS2-GWFEAA: a launch names its launcher so the registration can say who owns it.
        assert_eq!(
            server_args(
                Path::new("s"),
                "127.0.0.1:0",
                None,
                ServeStop::default(),
                Some("cli")
            ),
            ["-C", "s", "--bind", "127.0.0.1:0", "--launcher", "cli"].map(std::ffi::OsString::from)
        );
    }

    #[test]
    fn server_inventory_deduplicates_stores_checks_health_and_omits_secrets() {
        let dir = tempfile::tempdir().unwrap();
        let records = [
            (
                "one.json",
                r#"{"pid":42,"url":"http://127.0.0.1:8787","secret":"never-print-me","store_path":"/stores/one","started_at":"2026-09-10T01:00:00Z","launcher":"local-host-snapshot","build_revision":"source-sha256:abc","executable":"/tmp/snapshot/bin/hotsheet-server"}"#,
            ),
            (
                "two.json",
                r#"{"pid":42,"url":"http://127.0.0.1:8787","secret":"never-print-me","store_path":"/stores/two","started_at":"2026-09-10T01:00:00Z"}"#,
            ),
            (
                "stale.json",
                r#"{"pid":99,"url":"http://127.0.0.1:9999","secret":"also-private","store_path":"/stores/old","started_at":"2026-09-09T01:00:00Z"}"#,
            ),
            ("broken.json", "not json"),
        ];
        for (name, contents) in records {
            std::fs::write(dir.path().join(name), contents).unwrap();
        }
        let health_checks = std::cell::Cell::new(0);
        let inventory = server_inventory(
            read_registry_records(dir.path()).unwrap(),
            |pid| pid == 42,
            |url| {
                health_checks.set(health_checks.get() + 1);
                url.ends_with(":8787")
            },
        );
        let servers = &inventory.servers;
        assert_eq!(servers.len(), 2, "two registrations share one live server");
        assert_eq!(health_checks.get(), 1, "stale processes are not probed");
        assert_eq!(servers[0].status, "healthy");
        assert_eq!(servers[0].stores, ["/stores/one", "/stores/two"]);
        // HS2-GWFEAA: identity fields come from whichever registration carries them.
        assert_eq!(servers[0].launcher.as_deref(), Some("local-host-snapshot"));
        assert_eq!(
            servers[0].build_revision.as_deref(),
            Some("source-sha256:abc")
        );
        assert_eq!(servers[1].status, "stale");
        assert_eq!(servers[1].launcher, None, "legacy records have no launcher");
        assert_eq!(inventory.invalid, [dir.path().join("broken.json")]);
        assert_eq!(inventory.stale_records, [dir.path().join("stale.json")]);
        let rendered = render_server_inventory(&inventory, 0);
        assert!(rendered.contains("healthy   pid=42"));
        assert!(rendered.contains("launcher=local-host-snapshot build=source-sha256:abc"));
        assert!(rendered.contains("  executable: /tmp/snapshot/bin/hotsheet-server"));
        assert!(rendered.contains("stale     pid=99"));
        assert!(rendered.contains("launcher=unknown build=unknown"));
        assert!(rendered.contains("invalid registry record"));
        assert!(!rendered.contains("pruned"));
        assert!(!rendered.contains("never-print-me"));
        assert!(!rendered.contains("also-private"));
        assert!(!rendered.contains("secret"));

        // HS2-GWFEAA: listing prunes only the dead server's files; live and invalid stay.
        let pruned = prune_stale_records(&inventory.stale_records);
        assert_eq!(pruned, 1);
        assert!(!dir.path().join("stale.json").exists());
        assert!(dir.path().join("one.json").exists());
        assert!(dir.path().join("two.json").exists());
        assert!(dir.path().join("broken.json").exists());
        assert!(
            render_server_inventory(&inventory, pruned).contains("pruned 1 stale registration")
        );
        assert_eq!(
            prune_stale_records(&inventory.stale_records),
            1,
            "a vanished file still counts"
        );
        let again = server_inventory(
            read_registry_records(dir.path()).unwrap(),
            |pid| pid == 42,
            |_| true,
        );
        assert_eq!(again.servers.len(), 1);
        assert!(again.stale_records.is_empty());
    }

    #[test]
    fn path_fallback_and_missing_binary_are_explicit() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        let elsewhere = dir.path().join("elsewhere");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(&elsewhere).unwrap();
        let cli = bin.join("hotsheet-cli");
        let path_server =
            elsewhere.join(format!("hotsheet-server{}", std::env::consts::EXE_SUFFIX));
        std::fs::write(&path_server, "").unwrap();
        assert_eq!(
            resolve_server_binary(&cli, Some(elsewhere.as_os_str())).unwrap(),
            path_server
        );

        let error = resolve_server_binary(&cli, None).unwrap_err().to_string();
        assert!(error.contains("hotsheet-server is not installed"));
        assert!(error.contains("beside hotsheet-cli"));
    }
}
