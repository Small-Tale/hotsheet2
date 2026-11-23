use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, Subcommand};
use hotsheet_cli::remote_servers::{
    RemoteCredentialInput, RemoteRuntime, read_remote_credential_input,
};
use hotsheet_ticketing::remote_servers::{RegisterRemote, ReviewRemote, ReviewedRemoteIdentity};

use super::Ctx;
use crate::Cmd;

#[derive(Args)]
pub(crate) struct RemoteArgs {
    #[command(subcommand)]
    command: RemoteCmd,
}

#[derive(Subcommand)]
enum RemoteCmd {
    /// Private transport material for the same-machine desktop bridge; stdout must be captured.
    #[command(hide = true)]
    BridgeConnection { id: String },
    /// Manage host-approved browser sessions on this machine's running server.
    Sessions {
        #[command(subcommand)]
        command: SessionCmd,
    },
    /// Print this machine's durable authenticated server identity (no bearer or certificate).
    Identity,
    /// List reviewed bindings and cached observations without network requests.
    List,
    /// List the remote projects this desktop opens on startup.
    SelectedProjects,
    /// Remember one confirmed remote checkout on this machine.
    SelectProject { id: String, checkout: String },
    /// Stop restoring one remote checkout on this machine.
    UnselectProject { id: String, checkout: String },
    /// Verify and register a server using explicit pairing identity and credential input.
    /// A pairing bundle from `cert issue --bundle` supplies the server id and a default
    /// origin; re-running with the same origin updates the same registration (HS2-FZXXTG).
    Register {
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        label: String,
        /// Required unless the pairing bundle suggests one; an explicit origin wins.
        #[arg(long)]
        origin: Option<String>,
        /// Required unless the pairing bundle names it; when both are given they must match.
        #[arg(long)]
        expected_server_id: Option<String>,
        /// JSON credentials (bearer/CA/device certificate/key) or a pairing bundle from
        /// `cert issue --bundle`; '-' reads bounded stdin.
        #[arg(long, visible_alias = "bundle")]
        credentials_file: PathBuf,
    },
    /// Explicitly review a changed identity, origin, label or credential at one revision.
    Review {
        id: String,
        #[arg(long)]
        expected_revision: u64,
        #[arg(long)]
        previous_server_id: String,
        #[arg(long)]
        previous_ca_fingerprint: Option<String>,
        #[arg(long)]
        label: String,
        #[arg(long)]
        origin: String,
        #[arg(long)]
        expected_server_id: String,
        /// JSON credentials or a pairing bundle naming the same server id.
        #[arg(long, visible_alias = "bundle")]
        credentials_file: PathBuf,
    },
    /// Disable local access before retryable credential cleanup; does not revoke a remote certificate.
    Revoke {
        id: String,
        #[arg(long)]
        expected_revision: u64,
    },
    /// Perform fresh authenticated discovery; stale observations are never live checkout authority.
    Checkouts { id: String },
    /// Show this machine's paired session on a remote server: when its approval was renewed,
    /// when it expires, and the reminder day reached, or why it no longer works (HS2-4B0KSD).
    Session { id: String },
    /// Ask a remote server's host to renew this machine's approval in place. The binding,
    /// credential and selected projects stay; the host approves with `remote sessions renew`
    /// or Settings → Devices (HS2-4B0KSD).
    Renew { id: String },
}

#[derive(Subcommand)]
enum SessionCmd {
    /// List pending pairings and active devices; idempotent setup/status probe.
    Status,
    /// Approve one pending device from the trusted local CLI.
    Approve {
        id: String,
        #[arg(long, value_parser = ["read_only", "read_write"])]
        role: String,
        /// Let the device approve, reject, revoke and change other devices (HS2-DXM0YN).
        #[arg(long)]
        admin: bool,
        /// Trust the device as this host's own browser: folder chooser, open by path and
        /// setup (HS2-85W6BE). Grant it only to a browser on this machine.
        #[arg(long)]
        local: bool,
    },
    /// Refuse one pending pairing request.
    Reject { id: String },
    /// Change a paired device's role, admin grant or host access; omitted values stay.
    Update {
        id: String,
        #[arg(long, value_parser = ["read_only", "read_write"])]
        role: Option<String>,
        #[arg(long)]
        admin: Option<bool>,
        #[arg(long)]
        local: Option<bool>,
    },
    /// Renew one device's approval in place, before or after it expired: its 90-day lifetime
    /// restarts and it keeps its id, credential, role and grants (HS2-4B0KSD).
    Renew { id: String },
    /// Revoke one device immediately, including its live browser streams.
    Revoke { id: String },
    /// Revoke every browser session on this host.
    RevokeAll,
}

pub(crate) fn run(command: Cmd, context: &Ctx) -> Result<()> {
    let Cmd::Remote(arguments) = command else {
        super::misrouted("remote")
    };
    let home = hotsheet_plugins::hotsheet_home();
    let runtime = RemoteRuntime::live(&home);
    let output = match arguments.command {
        RemoteCmd::BridgeConnection { id } => {
            let (binding, credentials) = runtime.bridge_connection(&id)?;
            serde_json::json!({"binding": binding, "credentials": credentials})
        }
        RemoteCmd::Sessions { command } => match command {
            SessionCmd::Status => {
                super::server::host_json_with_body(&context.path, "GET", "remote-auth/owner", None)?
            }
            SessionCmd::Approve {
                id,
                role,
                admin,
                local,
            } => super::server::host_json_with_body(
                &context.path,
                "POST",
                "remote-auth/owner/approve",
                Some(&serde_json::json!({"id": id, "role": role, "admin": admin, "local": local})),
            )?,
            SessionCmd::Reject { id } => super::server::host_json_with_body(
                &context.path,
                "POST",
                "remote-auth/owner/reject",
                Some(&serde_json::json!({"id": id})),
            )?,
            SessionCmd::Update {
                id,
                role,
                admin,
                local,
            } => super::server::host_json_with_body(
                &context.path,
                "PATCH",
                &format!("remote-auth/owner/devices/{id}"),
                Some(&device_change(role, admin, local)?),
            )?,
            SessionCmd::Renew { id } => super::server::host_json_with_body(
                &context.path,
                "POST",
                &format!("remote-auth/owner/devices/{id}/renew"),
                None,
            )?,
            SessionCmd::Revoke { id } => super::server::host_json_with_body(
                &context.path,
                "DELETE",
                &format!("remote-auth/owner/devices/{id}"),
                None,
            )?,
            SessionCmd::RevokeAll => super::server::host_json_with_body(
                &context.path,
                "POST",
                "remote-auth/owner/revoke-all",
                None,
            )?,
        },
        RemoteCmd::Identity => serde_json::to_value(
            hotsheet_ticketing::remote_servers::server_identity(&home, || {
                ulid::Ulid::new().to_string()
            })?,
        )?,
        RemoteCmd::List => serde_json::to_value(runtime.list()?)?,
        RemoteCmd::SelectedProjects => serde_json::to_value(runtime.selected_projects()?)?,
        RemoteCmd::SelectProject { id, checkout } => {
            runtime.select_project(&id, &checkout)?;
            serde_json::json!({"selected": true})
        }
        RemoteCmd::UnselectProject { id, checkout } => {
            runtime.unselect_project(&id, &checkout)?;
            serde_json::json!({"selected": false})
        }
        RemoteCmd::Register {
            id,
            label,
            origin,
            expected_server_id,
            credentials_file,
        } => {
            let input = credential_input(&credentials_file)?;
            let origin = input.origin(origin.as_deref())?;
            let expected_server_id = input.expected_server_id(expected_server_id)?;
            let id = match id {
                Some(id) => id,
                None => runtime
                    .list()?
                    .into_iter()
                    .find(|binding| !binding.revoked && binding.origin == origin)
                    .map(|binding| binding.id)
                    .unwrap_or_else(|| ulid::Ulid::new().to_string()),
            };
            serde_json::to_value(runtime.register(RegisterRemote {
                id,
                label,
                origin,
                expected_server_id,
                credentials: input.credentials,
            })?)?
        }
        RemoteCmd::Review {
            id,
            expected_revision,
            previous_server_id,
            previous_ca_fingerprint,
            label,
            origin,
            expected_server_id,
            credentials_file,
        } => {
            let input = credential_input(&credentials_file)?;
            let expected_server_id = input.expected_server_id(Some(expected_server_id))?;
            serde_json::to_value(runtime.review(
                &id,
                ReviewRemote {
                    expected_revision,
                    previous_identity: ReviewedRemoteIdentity {
                        server_id: previous_server_id,
                        ca_fingerprint: previous_ca_fingerprint,
                    },
                    label,
                    origin,
                    expected_server_id,
                    credentials: input.credentials,
                },
            )?)?
        }
        RemoteCmd::Revoke {
            id,
            expected_revision,
        } => serde_json::to_value(runtime.revoke(&id, expected_revision)?)?,
        RemoteCmd::Checkouts { id } => serde_json::to_value(runtime.discover(&id)?)?,
        RemoteCmd::Session { id } => serde_json::to_value(runtime.peer_session(&id, false)?)?,
        RemoteCmd::Renew { id } => serde_json::to_value(runtime.peer_session(&id, true)?)?,
    };
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

/// The partial device change `remote sessions update` sends; at least one value is required.
fn device_change(
    role: Option<String>,
    admin: Option<bool>,
    local: Option<bool>,
) -> Result<serde_json::Value> {
    let mut change = serde_json::Map::new();
    if let Some(role) = role {
        change.insert("role".into(), role.into());
    }
    if let Some(admin) = admin {
        change.insert("admin".into(), admin.into());
    }
    if let Some(local) = local {
        change.insert("local".into(), local.into());
    }
    anyhow::ensure!(
        !change.is_empty(),
        "pass at least one of --role, --admin or --local"
    );
    Ok(serde_json::Value::Object(change))
}

fn credential_input(path: &std::path::Path) -> Result<RemoteCredentialInput> {
    if path == std::path::Path::new("-") {
        Ok(read_remote_credential_input(std::io::stdin())?)
    } else {
        Ok(read_remote_credential_input(std::fs::File::open(path)?)?)
    }
}

#[cfg(test)]
mod tests {
    use super::device_change;

    #[test]
    fn hs2_dxm0yn_update_sends_only_the_values_given() {
        assert_eq!(
            device_change(None, Some(true), None).unwrap(),
            serde_json::json!({"admin": true})
        );
        assert_eq!(
            device_change(Some("read_only".into()), Some(false), Some(true)).unwrap(),
            serde_json::json!({"role": "read_only", "admin": false, "local": true})
        );
        assert!(device_change(None, None, None).is_err());
    }
}
