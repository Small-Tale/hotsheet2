//! The local server: serving, certificates, lifecycle, and live server-backed reads.

use anyhow::Result;

use super::{Ctx, misrouted};
use crate::*;

pub(super) fn run(cmd: Cmd, ctx: &Ctx) -> Result<()> {
    let path = &ctx.path;
    match cmd {
        Cmd::Commands { cmd } => cmd_commands(path, cmd),
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
            list,
        } => cmd_serve(path, &bind, secret, stop, kill_all_terminals, list),
        Cmd::Cert { cmd } => cmd_cert(path, &cmd),
        _ => misrouted("server"),
    }
}

/// Run the server for this store in the foreground by exec'ing the sibling
/// `hotsheet-server` binary (the CLI stays free of a server dependency). Detached +
/// supervised start is client-owned (HS2-59 / HS2-4072GM).
pub(crate) fn cmd_serve(
    path: &Path,
    bind: &str,
    secret: Option<String>,
    stop: bool,
    kill_all_terminals: bool,
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
        kill_all_terminals,
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
