//! HS2-J8PRTV: headless target capacity, the admission question, and this machine's
//! own admission file. Requests go through the shared orchestration runtime; only the
//! admission file is read and written here, because it is the machine owner's and has
//! no network path.
use anyhow::{Result, bail};
use clap::{Args, Subcommand};
use hotsheet_ticketing::orchestration::{AdmissionConfig, AdmissionStore};
use serde_json::{Value, json};

use super::Ctx;

#[derive(Args)]
pub(super) struct CapacityArgs {
    #[command(subcommand)]
    command: CapacityCmd,
}
#[derive(Subcommand)]
enum CapacityCmd {
    /// Free and total RAM, CPU load, disk headroom, AI tools, running sessions and held
    /// resource claims of the machine that hosts a checkout. Unmeasured values are null.
    /// With --target (and --project) this asks the machine that hosts the target and
    /// needs a gates_observe grant; with --checkout it reads this machine.
    Show {
        /// Registered owning checkout id (with --target).
        #[arg(long, requires = "target")]
        project: Option<String>,
        /// The declared target whose machine is asked.
        #[arg(long, requires = "project", conflicts_with = "checkout")]
        target: Option<String>,
        /// A checkout this machine hosts.
        #[arg(long)]
        checkout: Option<String>,
        /// Also list live claims of this resource-claim namespace. Repeatable.
        #[arg(long = "claims")]
        claims: Vec<String>,
        /// The `version` of the snapshot you have; with --wait-seconds, answer when the
        /// capacity changes instead of at once (HS2-P5GTSW).
        #[arg(long, requires = "wait_seconds")]
        after: Option<String>,
        /// How long to wait for a change, at most 8 seconds per request.
        #[arg(long, requires = "after")]
        wait_seconds: Option<u64>,
    },
}

#[derive(Args)]
pub(super) struct AdmissionArgs {
    #[command(subcommand)]
    command: AdmissionCmd,
}
#[derive(Subcommand)]
enum AdmissionCmd {
    /// This machine's own admission limits (HOTSHEET_HOME/orchestration-admission.json).
    Show,
    /// Set this machine's admission limits. They can only make an orchestrator's policy
    /// stricter. Repeating the same command changes nothing.
    Set {
        /// Most AI sessions this machine accepts, 0 to 64.
        #[arg(long)]
        max_sessions: Option<u32>,
        #[arg(long)]
        min_free_ram_mb: Option<u64>,
        /// 1 to 100.
        #[arg(long)]
        max_cpu_percent: Option<u32>,
        #[arg(long)]
        min_free_disk_mb: Option<u64>,
        /// Admit on a measurement this machine could not take (default false).
        #[arg(long)]
        allow_unknown: Option<bool>,
        /// Estimated resident memory of one session of a tool without its own entry.
        #[arg(long)]
        default_footprint_mb: Option<u64>,
        /// Estimated resident memory of one session, as <tool>=<megabytes>. Repeatable.
        #[arg(long = "tool-footprint")]
        tool_footprints: Vec<String>,
        /// Remove one limit: max_sessions, min_free_ram_mb, max_cpu_percent,
        /// min_free_disk_mb, default_footprint_mb, or tool_footprint:<tool>. Repeatable.
        #[arg(long = "clear")]
        clear: Vec<String>,
    },
    /// Ask whether sessions of a tool may start now. With --target (and --project) the
    /// machine that hosts the target answers under the project's policy limits, and a
    /// sessions_manage grant for the tool is needed; with --checkout this machine
    /// answers under the policy defaults and its own admission file.
    Check {
        #[arg(long, requires = "target")]
        project: Option<String>,
        #[arg(long, requires = "project", conflicts_with = "checkout")]
        target: Option<String>,
        #[arg(long)]
        checkout: Option<String>,
        #[arg(long)]
        tool: String,
        #[arg(long)]
        model: Option<String>,
        /// How many sessions to ask for (default 1).
        #[arg(long)]
        sessions: Option<u32>,
    },
}

/// What a capacity or admission command needs from the shared runtime.
pub(super) enum Planned {
    /// Answered here; nothing more to do.
    Done,
    /// `(registered checkout id, (method, path, body))` for the runtime.
    Request(String, (&'static str, String, Value)),
}

fn encoded(namespaces: &[String]) -> String {
    // Namespaces are lowercase letters, digits, '.', '_' and '-': safe in a query.
    namespaces.join(",")
}

pub(super) fn capacity(arguments: CapacityArgs) -> Result<Planned> {
    let CapacityCmd::Show {
        project,
        target,
        checkout,
        claims,
        after,
        wait_seconds,
    } = arguments.command;
    let wait_ms = wait_seconds.map(|seconds| seconds.saturating_mul(1000));
    Ok(match (project, target, checkout) {
        (Some(project), Some(target), None) => {
            let path = format!("/checkouts/{project}/orchestration/targets/{target}/capacity");
            Planned::Request(
                project,
                (
                    "POST",
                    path,
                    json!({"claim_namespaces": claims, "after": after, "wait_ms": wait_ms}),
                ),
            )
        }
        (None, None, Some(checkout)) => {
            let mut parts = Vec::new();
            if !claims.is_empty() {
                parts.push(format!("claim_namespaces={}", encoded(&claims)));
            }
            if let (Some(after), Some(wait_ms)) = (&after, wait_ms) {
                // A version is hexadecimal: safe in a query.
                parts.push(format!("after={after}&wait_ms={wait_ms}"));
            }
            let query = if parts.is_empty() {
                String::new()
            } else {
                format!("?{}", parts.join("&"))
            };
            let path = format!("/checkouts/{checkout}/target-capacity{query}");
            Planned::Request(checkout, ("GET", path, Value::Null))
        }
        _ => bail!("pass --project <owner> --target <id>, or --checkout <id> for this machine"),
    })
}

fn print(value: &impl serde::Serialize) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn footprint(entry: &str) -> Result<(String, u64)> {
    let parsed = entry
        .split_once('=')
        .and_then(|(tool, megabytes)| Some((tool.to_owned(), megabytes.parse::<u64>().ok()?)));
    parsed.ok_or_else(|| anyhow::anyhow!("--tool-footprint takes <tool>=<megabytes>"))
}

fn clear(config: &mut AdmissionConfig, field: &str) -> Result<()> {
    match field {
        "max_sessions" => config.max_sessions = None,
        "min_free_ram_mb" => config.min_free_ram_mb = None,
        "max_cpu_percent" => config.max_cpu_percent = None,
        "min_free_disk_mb" => config.min_free_disk_mb = None,
        "default_footprint_mb" => config.default_footprint_mb = None,
        other => match other.strip_prefix("tool_footprint:") {
            Some(tool) => {
                config.tool_footprint_mb.remove(tool);
            }
            None => bail!("unknown admission limit: {other}"),
        },
    }
    Ok(())
}

pub(super) fn admission(arguments: AdmissionArgs, context: &Ctx) -> Result<Planned> {
    let home = hotsheet_plugins::hotsheet_home();
    let store = AdmissionStore::new(&home);
    let failed = |error: hotsheet_ticketing::orchestration::OrchestrationError| {
        anyhow::anyhow!("{}: {}", error.code(), error)
    };
    match arguments.command {
        AdmissionCmd::Show => {
            print(&store.read().map_err(failed)?)?;
            Ok(Planned::Done)
        }
        AdmissionCmd::Set {
            max_sessions,
            min_free_ram_mb,
            max_cpu_percent,
            min_free_disk_mb,
            allow_unknown,
            default_footprint_mb,
            tool_footprints,
            clear: cleared,
        } => {
            // The session these limits govern must not loosen them.
            if context.actor.as_ref().is_some_and(|actor| actor.is_ai()) {
                bail!("forbidden: an AI actor cannot change this machine's admission limits");
            }
            let footprints = tool_footprints
                .iter()
                .map(|entry| footprint(entry))
                .collect::<Result<Vec<_>>>()?;
            // Check the names before the file is touched.
            let mut probe = AdmissionConfig::default();
            for field in &cleared {
                clear(&mut probe, field)?;
            }
            let view = store
                .update(|config| {
                    for field in &cleared {
                        let _ = clear(config, field);
                    }
                    config.max_sessions = max_sessions.or(config.max_sessions);
                    config.min_free_ram_mb = min_free_ram_mb.or(config.min_free_ram_mb);
                    config.max_cpu_percent = max_cpu_percent.or(config.max_cpu_percent);
                    config.min_free_disk_mb = min_free_disk_mb.or(config.min_free_disk_mb);
                    config.allow_unknown = allow_unknown.unwrap_or(config.allow_unknown);
                    config.default_footprint_mb =
                        default_footprint_mb.or(config.default_footprint_mb);
                    config.tool_footprint_mb.extend(footprints);
                })
                .map_err(failed)?;
            print(&view)?;
            Ok(Planned::Done)
        }
        AdmissionCmd::Check {
            project,
            target,
            checkout,
            tool,
            model,
            sessions,
        } => {
            let body = json!({"tool": tool, "model": model, "sessions": sessions.unwrap_or(1)});
            Ok(match (project, target, checkout) {
                (Some(project), Some(target), None) => {
                    let path =
                        format!("/checkouts/{project}/orchestration/targets/{target}/admission");
                    Planned::Request(project, ("POST", path, body))
                }
                (None, None, Some(checkout)) => {
                    let path = format!("/checkouts/{checkout}/target-admission");
                    Planned::Request(checkout, ("POST", path, body))
                }
                _ => bail!(
                    "pass --project <owner> --target <id>, or --checkout <id> for this machine"
                ),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct Cli {
        #[command(subcommand)]
        command: CapacityCmd,
    }
    fn plan(arguments: &[&str]) -> Planned {
        let parsed =
            Cli::try_parse_from(std::iter::once("capacity").chain(arguments.iter().copied()))
                .unwrap();
        capacity(CapacityArgs {
            command: parsed.command,
        })
        .unwrap()
    }

    #[test]
    fn hs2_p5gtsw_capacity_show_waits_for_a_change_by_version() {
        let Planned::Request(owner, (method, path, body)) = plan(&[
            "show",
            "--project",
            "owner",
            "--target",
            "t1",
            "--after",
            "ab12",
            "--wait-seconds",
            "8",
        ]) else {
            panic!("a request");
        };
        assert_eq!((owner.as_str(), method), ("owner", "POST"));
        assert_eq!(path, "/checkouts/owner/orchestration/targets/t1/capacity");
        assert_eq!(
            body,
            json!({"claim_namespaces": [], "after": "ab12", "wait_ms": 8000})
        );
        let Planned::Request(_, (_, path, _)) = plan(&[
            "show",
            "--checkout",
            "c1",
            "--claims",
            "vm",
            "--after",
            "ab12",
            "--wait-seconds",
            "3",
        ]) else {
            panic!("a request");
        };
        assert_eq!(
            path,
            "/checkouts/c1/target-capacity?claim_namespaces=vm&after=ab12&wait_ms=3000"
        );
        // One without the other is refused by the parser.
        assert!(
            Cli::try_parse_from(["capacity", "show", "--checkout", "c1", "--after", "ab"]).is_err()
        );
    }
}
