//! HS2-W27N1D: headless Project Type and the projects an orchestration project adds.
//! The same workflow as Project Settings → General and Settings → Orchestration: every
//! request goes through the shared orchestration runtime; this file only shapes them.
use anyhow::{Result, bail};
use clap::{Args, Subcommand};
use serde_json::{Value, json};

#[derive(Args)]
pub(super) struct ProjectTypeArgs {
    /// Registered owning checkout id.
    #[arg(long)]
    project: Option<String>,
    /// Switch to `software` or `orchestration`; omit to show the current type. Switching
    /// to orchestration creates the empty target declaration on first use.
    #[arg(long)]
    set: Option<String>,
    /// With `--set software` while projects are added: keep them declared (nothing is
    /// dropped; they come back on a switch to orchestration).
    #[arg(long, requires = "set")]
    keep_projects: bool,
}

#[derive(Args)]
pub(super) struct ProjectArgs {
    /// Registered owning checkout id (the orchestration project).
    #[arg(long, global = true)]
    project: Option<String>,
    #[command(subcommand)]
    command: ProjectCmd,
}

#[derive(Subcommand)]
enum ProjectCmd {
    /// Add an open project: declare a target and bind it to that checkout in one step.
    /// Adding is a person's consent to let this project read and follow it.
    Add {
        /// The checkout id of the project to add, as this machine or the remote server
        /// registers it.
        checkout_id: String,
        /// The reviewed remote registration that hosts it; omit for this machine.
        #[arg(long)]
        registration_id: Option<String>,
        /// The name to show; defaults to the checkout id.
        #[arg(long)]
        label: Option<String>,
        /// A canonical ULID you keep for retries of this add.
        #[arg(long)]
        target_id: Option<String>,
    },
    /// Stop orchestrating one project: remove this machine's binding and tombstone its
    /// declaration. Sessions already running there keep running until stopped.
    Remove { target_id: String },
    /// HS2-92FJXM: the checkouts this server has open, which a client on another
    /// machine may add, each with whether it is already added.
    Hosted,
    /// HS2-92FJXM: add one of this server's open checkouts, the way a read-write client
    /// on another machine does. A checkout this server does not have open is refused.
    AddHosted {
        checkout_id: String,
        #[arg(long)]
        label: Option<String>,
        #[arg(long)]
        target_id: Option<String>,
    },
    /// HS2-0G80WD: stop orchestrating one project the way a read-write client on
    /// another machine does: a person only; a repeat changes nothing.
    RemoveHosted { target_id: String },
}

pub(super) fn project_type_route(
    arguments: ProjectTypeArgs,
) -> Result<(String, (&'static str, String, Value))> {
    let owner = arguments
        .project
        .ok_or_else(|| anyhow::anyhow!("--project <registered-checkout-id> is required"))?;
    let path = format!("/checkouts/{owner}/orchestration/project-type");
    let request = match arguments.set {
        None => ("GET", path, Value::Null),
        Some(kind) => {
            if !matches!(kind.as_str(), "software" | "orchestration") {
                bail!("--set takes software or orchestration");
            }
            (
                "PUT",
                path,
                json!({"project_type": kind, "keep_projects": arguments.keep_projects}),
            )
        }
    };
    Ok((owner, request))
}

pub(super) fn project_route(
    arguments: ProjectArgs,
) -> Result<(String, (&'static str, String, Value))> {
    let owner = arguments
        .project
        .ok_or_else(|| anyhow::anyhow!("--project <registered-checkout-id> is required"))?;
    let prefix = format!("/checkouts/{owner}/orchestration/projects");
    let request = match arguments.command {
        ProjectCmd::Add {
            checkout_id,
            registration_id,
            label,
            target_id,
        } => (
            "POST",
            prefix,
            json!({
                "destination": match registration_id {
                    Some(registration_id) => json!({"kind":"remote","registration_id":registration_id,"checkout_id":checkout_id}),
                    None => json!({"kind":"local","checkout_id":checkout_id}),
                },
                "label": label,
                "target_id": target_id,
            }),
        ),
        ProjectCmd::Remove { target_id } => ("DELETE", format!("{prefix}/{target_id}"), json!({})),
        ProjectCmd::Hosted => (
            "GET",
            format!("/checkouts/{owner}/orchestration/hosted-projects"),
            Value::Null,
        ),
        ProjectCmd::AddHosted {
            checkout_id,
            label,
            target_id,
        } => (
            "POST",
            format!("/checkouts/{owner}/orchestration/hosted-projects"),
            json!({"checkout_id": checkout_id, "label": label, "target_id": target_id}),
        ),
        ProjectCmd::RemoveHosted { target_id } => (
            "DELETE",
            format!("/checkouts/{owner}/orchestration/hosted-projects/{target_id}"),
            json!({}),
        ),
    };
    Ok((owner, request))
}
