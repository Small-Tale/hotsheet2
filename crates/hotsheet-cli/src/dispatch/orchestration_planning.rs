//! HS2-HCW7MD: the headless Propose Plan surface. Every request goes through the
//! shared orchestration runtime; this file only shapes them.
use anyhow::Result;
use clap::{Args, Subcommand};
use serde_json::{Value, json};

#[derive(Args)]
pub(super) struct PlanArgs {
    /// Registered checkout id of the orchestration project.
    #[arg(long, global = true)]
    pub(super) project: Option<String>,
    #[command(subcommand)]
    command: PlanCmd,
}

#[derive(Subcommand)]
enum PlanCmd {
    /// Start the project's planning AI session, which reads the open goal tickets and
    /// each declared project's tickets and proposes the dry-run plan: target tickets
    /// to create or link, plan steps with prerequisites, resources and a preferred
    /// tool. It runs in this project through the ordinary AI terminal launch and
    /// shows in the drawer. One planner at a time: a live one answers instead, and a
    /// retained --operation-id never launches twice. With no open goal ticket it
    /// plans the default goal, working through all open tickets of the targets, and
    /// opens that goal ticket once (HS2-YSRT6Y). Nothing is placed until the policy
    /// places sessions.
    Propose {
        /// The AI tool the planner runs in, by plugin id.
        #[arg(long)]
        tool: String,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        effort: Option<String>,
        /// Plan only this goal ticket (slug or id); every open goal ticket otherwise.
        #[arg(long)]
        goal: Option<String>,
        /// The goal ticket's source; omit when the project has one git source.
        #[arg(long, default_value = "")]
        source: String,
        /// A retained id; the same id answers with the same planner.
        #[arg(long)]
        operation_id: Option<String>,
    },
    /// Where planning stands: idle, planning or ended, the live or latest planner, the
    /// open goal tickets with their plan steps, whether only the default goal
    /// (working through all open tickets) applies, whether the policy places
    /// sessions yet, and the latest plan update. Starts nothing.
    Show,
    /// HS2-190RDX: Update Plan. Re-plan in the background without pausing: open a
    /// draft of the plan beside the live one and ask the planner (a new one, or the
    /// open one with a new proposal) to draft it. Running work and placement continue
    /// on the live plan until the update finishes. One update at a time: a second
    /// request answers with the drafting one.
    Update {
        /// The AI tool a new planner runs in, by plugin id.
        #[arg(long)]
        tool: String,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        effort: Option<String>,
        /// A retained id; the same id answers with the same update.
        #[arg(long)]
        operation_id: Option<String>,
    },
    /// HS2-190RDX: swap the live plan to the update's draft in one write. Unchanged
    /// steps keep running; the work of removed or changed steps is withdrawn and its
    /// sessions wind down at a safe point. A conflicting or invalid draft fails the
    /// update and leaves the plan as it was.
    UpdateFinish {
        /// The update; the drafting one when omitted.
        update_id: Option<String>,
    },
    /// HS2-190RDX: cancel the drafting update; the live plan is unchanged.
    UpdateCancel {
        /// The update; the drafting one when omitted.
        update_id: Option<String>,
    },
}

pub(super) fn route(project: &str, arguments: PlanArgs) -> Result<(&'static str, String, Value)> {
    let base = format!("/checkouts/{project}/orchestration/planning");
    Ok(match arguments.command {
        PlanCmd::Propose {
            tool,
            model,
            effort,
            goal,
            source,
            operation_id,
        } => (
            "POST",
            format!("{base}/propose"),
            json!({
                "tool": tool,
                "model": model,
                "effort": effort,
                "goal": goal.map(|goal| json!({"connection_id": source, "native_id": goal})),
                "operation_id": operation_id,
            }),
        ),
        PlanCmd::Show => ("POST", format!("{base}/show"), json!({})),
        PlanCmd::Update {
            tool,
            model,
            effort,
            operation_id,
        } => (
            "POST",
            format!("{base}/update"),
            json!({"tool": tool, "model": model, "effort": effort, "operation_id": operation_id}),
        ),
        PlanCmd::UpdateFinish { update_id } => (
            "POST",
            format!("{base}/update_finish"),
            json!({"update_id": update_id}),
        ),
        PlanCmd::UpdateCancel { update_id } => (
            "POST",
            format!("{base}/update_cancel"),
            json!({"update_id": update_id}),
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[derive(Parser)]
    struct Harness {
        #[command(flatten)]
        arguments: PlanArgs,
    }
    fn request(line: &[&str]) -> Result<(&'static str, String, Value)> {
        let parsed = Harness::try_parse_from(std::iter::once("plan").chain(line.iter().copied()))?;
        route("owner-1", parsed.arguments)
    }

    #[test]
    fn hs2_hcw7md_every_plan_command_is_one_request_to_the_shared_runtime() {
        let base = "/checkouts/owner-1/orchestration/planning";
        assert_eq!(
            request(&["propose", "--tool", "claude"]).unwrap(),
            (
                "POST",
                format!("{base}/propose"),
                json!({"tool":"claude","model":null,"effort":null,"goal":null,
                    "operation_id":null})
            )
        );
        assert_eq!(
            request(&[
                "propose",
                "--tool",
                "codex",
                "--model",
                "m",
                "--effort",
                "high",
                "--goal",
                "OWN-1",
                "--source",
                "tickets",
                "--operation-id",
                "press-1",
            ])
            .unwrap(),
            (
                "POST",
                format!("{base}/propose"),
                json!({"tool":"codex","model":"m","effort":"high",
                    "goal":{"connection_id":"tickets","native_id":"OWN-1"},
                    "operation_id":"press-1"})
            )
        );
        assert_eq!(
            request(&["show"]).unwrap(),
            ("POST", format!("{base}/show"), json!({}))
        );
        // A press names its tool.
        assert!(request(&["propose"]).is_err());
        // HS2-190RDX: Update Plan, finish and cancel.
        assert_eq!(
            request(&["update", "--tool", "claude"]).unwrap(),
            (
                "POST",
                format!("{base}/update"),
                json!({"tool":"claude","model":null,"effort":null,"operation_id":null})
            )
        );
        assert!(request(&["update"]).is_err());
        assert_eq!(
            request(&["update-finish", "U1"]).unwrap(),
            (
                "POST",
                format!("{base}/update_finish"),
                json!({"update_id":"U1"})
            )
        );
        assert_eq!(
            request(&["update-cancel"]).unwrap(),
            (
                "POST",
                format!("{base}/update_cancel"),
                json!({"update_id":null})
            )
        );
    }
}
