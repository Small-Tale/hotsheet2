//! `hotsheet-cli remote-change`: reusable connections to external event services and the
//! sources bound to them (HS2-8X2S08, `docs/37-remote-change-notifications.md` §37.6). Every
//! command is idempotent, prints JSON, never prints a credential, and needs no running server:
//! it calls `hotsheet_notify::remote_change::workflow`, which the server's settings call too.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow, bail};
use clap::Subcommand;
use hotsheet_notify::remote_change::builtin_adapters;
use hotsheet_notify::remote_change::model::RepoIdentity;
use hotsheet_notify::remote_change::workflow::{Workflow, WorkflowError};
use hotsheet_ticketing::OsKeychain;
use serde_json::{Value, json};

use super::{Ctx, misrouted};
use crate::Cmd;

#[derive(Subcommand)]
pub(crate) enum RemoteChangeCmd {
    /// List the remote-change plugins this build supports, with their settings and credential.
    Plugins,
    /// List every connection and binding (a credential reads `stored` or `missing`).
    List,
    /// Add, inspect, test or remove a reusable connection to an event service.
    Connection {
        #[command(subcommand)]
        cmd: ConnectionCmd,
    },
    /// Bind a source (a code checkout or a Git ticket store) to a connection's channel.
    Bind {
        /// `checkout:<checkout id>` or `store:<instance id>`; or pass `--store`.
        source: Option<String>,
        /// A Git ticket store; binds its shared identity `store:<instanceId>`.
        #[arg(long, value_name = "PATH", conflicts_with = "source")]
        store: Option<PathBuf>,
        #[arg(long)]
        connection: String,
        /// The channel on the service.
        #[arg(long)]
        channel: String,
        /// The repository the source expects events about, `owner/name`.
        #[arg(long, value_name = "OWNER/NAME")]
        repository: Option<String>,
        /// The service's numeric repository id, if the events carry it.
        #[arg(long)]
        repository_id: Option<u64>,
        /// A ref to follow: an exact name or a prefix ending in `*` (repeatable; default
        /// `refs/heads/*`).
        #[arg(long = "ref", value_name = "REF")]
        refs: Vec<String>,
    },
    /// Remove one binding. Removing one that is not there is not an error.
    Unbind {
        source: Option<String>,
        #[arg(long, value_name = "PATH", conflicts_with = "source")]
        store: Option<PathBuf>,
        #[arg(long)]
        connection: String,
        #[arg(long)]
        channel: String,
    },
}

#[derive(Subcommand)]
pub(crate) enum ConnectionCmd {
    /// Add or change a connection. Run again with the same arguments: nothing changes.
    Add {
        id: String,
        #[arg(long)]
        plugin: String,
        /// A plain setting `key=value` (repeatable). Never a credential.
        #[arg(long = "setting", value_name = "KEY=VALUE")]
        settings: Vec<String>,
    },
    /// Show one connection: its settings, whether its credential is stored, and its bindings.
    Show { id: String },
    /// Make one cheap real call through the plugin and report the result.
    Test { id: String },
    /// Remove a connection and its credential. Refused while anything is bound to it.
    Remove { id: String },
    /// Store the connection's credential in the OS credential store (value on standard input).
    Secret {
        id: String,
        /// Remove the stored credential instead.
        #[arg(long)]
        delete: bool,
    },
}

fn home() -> PathBuf {
    hotsheet_plugins::hotsheet_home()
}

fn print(value: &Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn fail(error: WorkflowError) -> anyhow::Error {
    anyhow!("{}: {error}", error.code())
}

fn source_of(source: Option<String>, store: Option<&std::path::Path>) -> Result<String> {
    let key = match (source, store) {
        // A store's shared identity comes from the store itself.
        (None, Some(store)) => super::resource_backends::source_key(None, Some(store))?,
        (Some(source), None) => source,
        _ => bail!("invalid: name a source (checkout:<id> or store:<id>) or pass --store <path>"),
    };
    if !hotsheet_notify::remote_change::registry::valid_source(&key) {
        bail!("invalid: a source is checkout:<checkout id> or store:<instance id>, not '{key}'");
    }
    Ok(key)
}

pub(crate) fn run(cmd: Cmd, _ctx: &Ctx) -> Result<()> {
    let Cmd::RemoteChange { cmd } = cmd else {
        misrouted("remote-change")
    };
    let home = home();
    let adapters = builtin_adapters();
    let secrets = OsKeychain::at(&home);
    let workflow = Workflow {
        home: &home,
        adapters: &adapters,
        secrets: &secrets,
    };
    match cmd {
        RemoteChangeCmd::Plugins => print(&json!({ "plugins": workflow.plugins() })),
        RemoteChangeCmd::List => print(&workflow.list().map_err(fail)?),
        RemoteChangeCmd::Connection { cmd } => match cmd {
            ConnectionCmd::Add {
                id,
                plugin,
                settings,
            } => {
                let mut parsed = BTreeMap::new();
                for entry in &settings {
                    let (key, value) = entry
                        .split_once('=')
                        .ok_or_else(|| anyhow!("invalid: --setting takes key=value"))?;
                    if parsed.insert(key.to_owned(), value.to_owned()).is_some() {
                        bail!("invalid: --setting names '{key}' more than once");
                    }
                }
                let (change, next_steps) = workflow
                    .add_connection(&id, &plugin, parsed)
                    .map_err(fail)?;
                print(&json!({
                    "change": change,
                    "connection": workflow.show(&id).map_err(fail)?,
                    "next_steps": next_steps,
                }))
            }
            ConnectionCmd::Show { id } => print(&workflow.show(&id).map_err(fail)?),
            ConnectionCmd::Test { id } => {
                let report = workflow.test(&id).map_err(fail)?;
                print(&serde_json::to_value(&report)?)?;
                if report.ok {
                    Ok(())
                } else {
                    bail!(
                        "{}: {}",
                        report.code.unwrap_or("failed"),
                        report.error.unwrap_or_default()
                    )
                }
            }
            ConnectionCmd::Remove { id } => {
                let change = workflow.remove_connection(&id).map_err(fail)?;
                print(&json!({ "change": change, "id": id }))
            }
            ConnectionCmd::Secret { id, delete } => {
                let change = if delete {
                    workflow.set_secret(&id, None)
                } else {
                    let mut value = String::new();
                    std::io::stdin()
                        .take(8192)
                        .read_to_string(&mut value)
                        .context("reading the credential from standard input")?;
                    workflow.set_secret(&id, Some(&value))
                }
                .map_err(fail)?;
                print(&json!({ "change": change, "connection": id }))
            }
        },
        RemoteChangeCmd::Bind {
            source,
            store,
            connection,
            channel,
            repository,
            repository_id,
            refs,
        } => {
            let key = source_of(source, store.as_deref())?;
            let identity = RepoIdentity {
                id: repository_id,
                full_name: repository,
            };
            let change = workflow
                .bind(&key, &connection, &channel, identity, refs)
                .map_err(fail)?;
            print(
                &json!({ "change": change, "source": key, "connection": connection, "channel": channel }),
            )
        }
        RemoteChangeCmd::Unbind {
            source,
            store,
            connection,
            channel,
        } => {
            let key = source_of(source, store.as_deref())?;
            let change = workflow.unbind(&key, &connection, &channel).map_err(fail)?;
            print(
                &json!({ "change": change, "source": key, "connection": connection, "channel": channel }),
            )
        }
    }
}
