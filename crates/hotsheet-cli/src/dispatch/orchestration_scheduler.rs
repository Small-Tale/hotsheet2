//! HS2-ECWG50: the headless scheduler surface. Every request goes through the shared
//! orchestration runtime; this file only shapes them.
use anyhow::{Result, bail};
use clap::{Args, Subcommand};
use serde_json::{Map, Value, json};

#[derive(Args)]
pub(super) struct SchedulerArgs {
    /// Registered owning checkout id.
    #[arg(long, global = true)]
    pub(super) project: Option<String>,
    #[command(subcommand)]
    command: SchedulerCmd,
}

#[derive(Subcommand)]
enum SchedulerCmd {
    /// Where and how many sessions the scheduler would start, keep or wind down now,
    /// with every piece of ready work ranked and a reason for each one not placed.
    /// A dry run: it reads target capacity (gates_observe) and changes and logs nothing.
    Plan,
    /// Why one piece of work is not running: by plan step, or by exact target ticket
    /// with --target, --connection and --id (the provider's own ticket id).
    Explain {
        step_id: Option<String>,
        /// The declared target project.
        #[arg(long)]
        target: Option<String>,
        /// The target's ticket source.
        #[arg(long)]
        connection: Option<String>,
        /// The target ticket's provider-native id.
        #[arg(long)]
        id: Option<String>,
    },
    /// The decision log after a cursor, oldest first: one record per pass and one per
    /// decision, each with its reason, its inputs and, when applied, its outcome.
    Decisions {
        #[arg(long, default_value_t = 0)]
        after: u64,
        /// Page size, 1 to 200.
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Run one scheduling pass and log its decisions. Sessions are started or wound
    /// down only when the policy sets placement.automatic; otherwise, or with
    /// --dry-run, nothing is applied. The server runs this on events by itself for a
    /// policy that enables it, so there is nothing to call on a timer.
    Tick {
        /// Compute and log the decisions and apply none of them.
        #[arg(long)]
        dry_run: bool,
    },
}

pub(super) fn route(
    project: &str,
    arguments: SchedulerArgs,
) -> Result<(&'static str, String, Value)> {
    let base = format!("/checkouts/{project}/orchestration/scheduler");
    Ok(match arguments.command {
        SchedulerCmd::Plan => ("POST", format!("{base}/plan"), json!({})),
        SchedulerCmd::Explain {
            step_id,
            target,
            connection,
            id,
        } => {
            let body = match (step_id, target, connection, id) {
                (Some(step_id), None, None, None) => json!({"step_id": step_id}),
                (None, Some(target), Some(connection), Some(id)) => {
                    json!({"target_id": target, "connection_id": connection, "native_id": id})
                }
                _ => bail!("explain takes a step id, or --target, --connection and --id"),
            };
            ("POST", format!("{base}/explain"), body)
        }
        SchedulerCmd::Decisions { after, limit } => {
            let mut body = Map::new();
            body.insert("after".into(), after.into());
            if let Some(limit) = limit {
                body.insert("limit".into(), limit.into());
            }
            ("POST", format!("{base}/decisions"), Value::Object(body))
        }
        SchedulerCmd::Tick { dry_run } => {
            ("POST", format!("{base}/tick"), json!({"dry_run": dry_run}))
        }
    })
}
