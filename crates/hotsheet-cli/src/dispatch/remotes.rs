use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, Subcommand};
use hotsheet_cli::remote_servers::RemoteRuntime;
use hotsheet_ticketing::remote_servers::{
    RegisterRemote, ReviewRemote, ReviewedRemoteIdentity, normalize_remote_origin,
};

use super::Ctx;
use crate::Cmd;

#[derive(Args)]
pub(crate) struct RemoteArgs {
    #[command(subcommand)]
    command: RemoteCmd,
}

#[derive(Subcommand)]
enum RemoteCmd {
    /// Print this machine's durable authenticated server identity (no bearer or certificate).
    Identity,
    /// List reviewed bindings and cached observations without network requests.
    List,
    /// Verify and register a server using explicit pairing identity and credential input.
    Register {
        #[arg(long)]
        id: Option<String>,
        #[arg(long)]
        label: String,
        #[arg(long)]
        origin: String,
        #[arg(long)]
        expected_server_id: String,
        /// JSON bearer/CA/device certificate/key bundle; '-' reads bounded stdin.
        #[arg(long)]
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
        #[arg(long)]
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
}

pub(crate) fn run(command: Cmd, _context: &Ctx) -> Result<()> {
    let Cmd::Remote(arguments) = command else {
        super::misrouted("remote")
    };
    let home = hotsheet_plugins::hotsheet_home();
    let runtime = RemoteRuntime::live(&home);
    let output = match arguments.command {
        RemoteCmd::Identity => serde_json::to_value(
            hotsheet_ticketing::remote_servers::server_identity(&home, || {
                ulid::Ulid::new().to_string()
            })?,
        )?,
        RemoteCmd::List => serde_json::to_value(runtime.list()?)?,
        RemoteCmd::Register {
            id,
            label,
            origin,
            expected_server_id,
            credentials_file,
        } => {
            let origin = normalize_remote_origin(&origin)?;
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
                credentials: credentials(&credentials_file)?,
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
        } => serde_json::to_value(runtime.review(
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
                credentials: credentials(&credentials_file)?,
            },
        )?)?,
        RemoteCmd::Revoke {
            id,
            expected_revision,
        } => serde_json::to_value(runtime.revoke(&id, expected_revision)?)?,
        RemoteCmd::Checkouts { id } => serde_json::to_value(runtime.discover(&id)?)?,
    };
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn credentials(
    path: &std::path::Path,
) -> Result<hotsheet_ticketing::remote_servers::RemoteCredentials> {
    if path == std::path::Path::new("-") {
        Ok(hotsheet_cli::remote_servers::read_remote_credentials(
            std::io::stdin(),
        )?)
    } else {
        Ok(hotsheet_cli::remote_servers::read_remote_credentials(
            std::fs::File::open(path)?,
        )?)
    }
}
