//! `hotsheet-cli terminal-relay`: the headless install, status, and removal of the macOS
//! terminal launch relay (HS2-HYF56Y). The desktop app runs `install` through this same
//! command, so both clients share one workflow.

#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;

#[cfg(unix)]
use anyhow::Context;
use anyhow::Result;
use clap::Subcommand;

#[derive(Subcommand)]
pub(crate) enum TerminalRelayCmd {
    /// Install or refresh the per-user LaunchAgent and load it into the desktop session.
    /// Idempotent: an unchanged, running relay is left alone. Run it from a desktop login.
    Install {
        /// The `hotsheet-server` binary launchd runs (default: beside this CLI, else `PATH`).
        #[arg(long)]
        server: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Report whether the relay is installed, loaded, current, and answering.
    Status {
        /// The `hotsheet-server` binary the installed definition should name.
        #[arg(long)]
        server: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Unload and remove the LaunchAgent. Running brokers and their terminals keep running.
    Uninstall {
        #[arg(long)]
        json: bool,
    },
}

pub(crate) fn run(cmd: TerminalRelayCmd) -> Result<()> {
    if cfg!(target_os = "macos") {
        macos(cmd)
    } else {
        let json = match cmd {
            TerminalRelayCmd::Install { json, .. }
            | TerminalRelayCmd::Status { json, .. }
            | TerminalRelayCmd::Uninstall { json } => json,
        };
        if json {
            println!("{}", serde_json::json!({ "supported": false }));
        } else {
            println!(
                "The terminal launch relay is only used on macOS; terminals here need no relay."
            );
        }
        Ok(())
    }
}

#[cfg(unix)]
fn server_binary(explicit: Option<PathBuf>) -> Result<PathBuf> {
    let path = match explicit {
        Some(path) => path,
        None => {
            let current = std::env::current_exe().context("cannot locate hotsheet-cli")?;
            super::server::resolve_server_binary(&current, std::env::var_os("PATH").as_deref())?
        }
    };
    // launchd needs an absolute, stable program path.
    path.canonicalize()
        .with_context(|| format!("hotsheet-server not found at {}", path.display()))
}

#[cfg(unix)]
fn agent(program: &Path) -> Result<hotsheet_terminals::launch_agent::RelayAgent> {
    let home = hotsheet_plugins::hotsheet_home();
    // The relay socket is keyed by the canonical home; create it so the server, which always
    // has its home, derives the same key.
    std::fs::create_dir_all(&home)
        .with_context(|| format!("cannot create Hot Sheet home {}", home.display()))?;
    let default_home = std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".hotsheet2"));
    let agents = hotsheet_terminals::launch_agent::default_agents_dir()?;
    Ok(hotsheet_terminals::launch_agent::RelayAgent::for_home(
        &home,
        program,
        &agents,
        default_home.as_deref(),
    )?)
}

#[cfg(unix)]
fn macos(cmd: TerminalRelayCmd) -> Result<()> {
    use hotsheet_terminals::launch_agent::{
        InstallOutcome, READY_TIMEOUT, SystemLaunchctl, UninstallOutcome, install, status,
        uninstall,
    };
    let launchctl = SystemLaunchctl::from_env(|program| hotsheet_ticketing::git::launch(program));
    // SAFETY: geteuid takes no pointers and has no preconditions.
    let uid = unsafe { libc::geteuid() };
    match cmd {
        TerminalRelayCmd::Install { server, json } => {
            let agent = agent(&server_binary(server)?)?;
            let report = install(&agent, &launchctl, uid, READY_TIMEOUT)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                let verb = match report.outcome {
                    InstallOutcome::Installed => "installed",
                    InstallOutcome::Updated => "updated",
                    InstallOutcome::AlreadyCurrent => "already installed",
                    InstallOutcome::Restarted => "restarted",
                };
                println!("Terminal launch relay {verb}: {}", agent.label);
                println!("  agent:  {}", agent.plist.display());
                println!(
                    "  relay:  pid {} launches {}",
                    report.relay.pid,
                    report.relay.broker.display()
                );
                println!("  socket: {}", agent.socket.display());
            }
        }
        TerminalRelayCmd::Status { server, json } => {
            let program = match server {
                Some(path) => server_binary(Some(path))?,
                None => server_binary(None).unwrap_or_default(),
            };
            let report = status(&agent(&program)?, &launchctl, uid);
            if json {
                let mut value = serde_json::to_value(&report)?;
                value["healthy"] = report.healthy().into();
                println!("{}", serde_json::to_string_pretty(&value)?);
            } else {
                let yes_no = |value: bool| if value { "yes" } else { "no" };
                println!(
                    "Terminal launch relay {}: {}",
                    report.agent.label,
                    if report.healthy() {
                        "healthy"
                    } else {
                        "not in use"
                    }
                );
                println!(
                    "  agent:  {} (present: {}, current: {}, loaded: {})",
                    report.agent.plist.display(),
                    yes_no(report.plist_present),
                    yes_no(report.plist_current),
                    yes_no(report.loaded)
                );
                match (&report.relay, &report.relay_error) {
                    (Some(relay), _) => println!(
                        "  relay:  pid {} launches {} (bootstrap {:?})",
                        relay.pid,
                        relay.broker.display(),
                        relay.bootstrap
                    ),
                    (None, Some(error)) => println!("  relay:  {error}"),
                    (None, None) => {}
                }
                if !report.healthy() {
                    println!(
                        "  Servers spawn terminal brokers directly. Run `hotsheet-cli \
                         terminal-relay install` from a desktop session to use the relay."
                    );
                }
            }
        }
        TerminalRelayCmd::Uninstall { json } => {
            let agent = agent(Path::new(""))?;
            let outcome = uninstall(&agent, &launchctl, uid)?;
            if json {
                println!(
                    "{}",
                    serde_json::json!({ "outcome": outcome, "label": agent.label })
                );
            } else {
                match outcome {
                    UninstallOutcome::Removed => println!(
                        "Removed terminal launch relay {}; running terminals keep running.",
                        agent.label
                    ),
                    UninstallOutcome::NotInstalled => {
                        println!("Terminal launch relay {} was not installed.", agent.label)
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn macos(_cmd: TerminalRelayCmd) -> Result<()> {
    anyhow::bail!("the terminal launch relay is macOS-only")
}
