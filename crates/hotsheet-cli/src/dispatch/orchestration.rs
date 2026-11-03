//! HS2-4J6V46: headless target operations, with cleanup before host config access.
use anyhow::{Result, bail};
use clap::{Args, Subcommand};
use hotsheet_cli::orchestration::{OrchestrationRuntime, request_cleanup_now};
use hotsheet_ticketing::{ProviderConfigRegistry, checkouts::CheckoutRegistry};
use serde_json::{Value, json};

use super::Ctx;
use crate::Cmd;

#[derive(Args)]
pub(crate) struct OrchestrationArgs {
    #[command(subcommand)]
    command: OrchestrationCmd,
}
#[derive(Subcommand)]
enum OrchestrationCmd {
    /// List, declare, rename, remove, bind, unbind, resolve and inspect project targets.
    Target(TargetArgs),
}
#[derive(Args)]
struct TargetArgs {
    /// Registered owning checkout id, never a source/store id or label.
    #[arg(long, global = true)]
    project: Option<String>,
    #[command(subcommand)]
    command: TargetCmd,
}
#[derive(Subcommand)]
enum TargetCmd {
    /// Shared declarations; --target-id reads that exact local cached binding for recovery.
    List {
        #[arg(long)]
        target_id: Option<String>,
    },
    /// Omit expected-revision only for an absent manifest. Retain target-id for safe retries.
    Add {
        #[arg(long)]
        expected_revision: Option<String>,
        #[arg(long)]
        target_id: Option<String>,
        #[arg(long)]
        label: String,
        #[arg(long)]
        expected_repository_identity: Option<String>,
    },
    /// Rename one live target at the expected manifest revision; its id never changes.
    Rename {
        id: String,
        #[arg(long)]
        expected_revision: String,
        #[arg(long)]
        label: String,
        #[arg(long)]
        expected_repository_identity: Option<String>,
        #[arg(long)]
        clear_expected_repository_identity: bool,
    },
    /// Tombstone one target at the expected manifest revision; its id is never reused.
    Remove {
        id: String,
        #[arg(long)]
        expected_revision: String,
    },
    /// Public identity JSON only; credentials/origins are owned by remote registrations.
    Bind {
        id: String,
        #[arg(long)]
        expected_manifest_revision: String,
        #[arg(long)]
        expected_binding_revision: u64,
        #[arg(long)]
        destination: String,
        #[arg(long)]
        previous_destination: Option<String>,
    },
    /// Remove this machine's binding at the expected binding revision; idempotent when absent.
    Unbind {
        id: String,
        #[arg(long)]
        expected_binding_revision: u64,
        #[arg(long)]
        previous_destination: Option<String>,
    },
    /// Freshly verify the bound destination and report its explicit target state.
    Resolve { id: String },
    /// Fresh read-confirm-bind candidate metadata; never creates a binding/grant.
    Inspect {
        checkout_id: String,
        #[arg(long)]
        registration_id: Option<String>,
    },
}
fn identity(value: Option<String>) -> Result<Value> {
    match value {
        None => Ok(Value::Null),
        Some(value) => serde_json::from_str(&value).map_err(|_| {
            anyhow::anyhow!("destination must be a public reviewed identity JSON object")
        }),
    }
}
fn route(project: &str, command: TargetCmd) -> Result<(&'static str, String, Value)> {
    let prefix = format!("/checkouts/{}/orchestration/targets", project);
    let target = |id: &str| json!({"owner_checkout_id":project,"target_id":id});
    Ok(match command {
        TargetCmd::List { target_id: None } => ("GET", prefix, Value::Null),
        TargetCmd::List {
            target_id: Some(id),
        } => ("GET", format!("{prefix}/{id}/binding"), Value::Null),
        TargetCmd::Add {
            expected_revision,
            target_id,
            label,
            expected_repository_identity,
        } => (
            "POST",
            prefix,
            json!({"expected_revision":expected_revision,"target_id":target_id,"label":label,"expected_repository_identity":expected_repository_identity}),
        ),
        TargetCmd::Rename {
            id,
            expected_revision,
            label,
            expected_repository_identity,
            clear_expected_repository_identity,
        } => (
            "PATCH",
            format!("{prefix}/{id}"),
            json!({"expected_revision":expected_revision,"label":label,"expected_repository_identity":expected_repository_identity,"clear_expected_repository_identity":clear_expected_repository_identity}),
        ),
        TargetCmd::Remove {
            id,
            expected_revision,
        } => (
            "DELETE",
            format!("{prefix}/{id}"),
            json!({"expected_revision":expected_revision}),
        ),
        TargetCmd::Bind {
            id,
            expected_manifest_revision,
            expected_binding_revision,
            destination,
            previous_destination,
        } => (
            "PUT",
            format!("{prefix}/{id}/binding"),
            json!({"target":target(&id),"expected_manifest_revision":expected_manifest_revision,
                "expected_binding_revision":expected_binding_revision,"destination":identity(Some(destination))?,"previous_destination":identity(previous_destination)?}),
        ),
        TargetCmd::Unbind {
            id,
            expected_binding_revision,
            previous_destination,
        } => (
            "DELETE",
            format!("{prefix}/{id}/binding"),
            json!({"target":target(&id),"expected_binding_revision":expected_binding_revision,"previous_destination":identity(previous_destination)?}),
        ),
        TargetCmd::Resolve { id } => ("GET", format!("{prefix}/{id}/resolve"), Value::Null),
        TargetCmd::Inspect {
            checkout_id,
            registration_id,
        } => (
            "POST",
            format!("/checkouts/{project}/orchestration/inspect"),
            json!({"destination": match registration_id {
                Some(registration_id) => json!({"kind":"remote","registration_id":registration_id,"checkout_id":checkout_id}),
                None => json!({"kind":"local","checkout_id":checkout_id}),
            }}),
        ),
    })
}
pub(crate) fn run(command: Cmd, context: &Ctx) -> Result<()> {
    let Cmd::Orchestration(arguments) = command else {
        super::misrouted("orchestration")
    };
    let OrchestrationCmd::Target(arguments) = arguments.command;
    let owner_id = arguments
        .project
        .clone()
        .ok_or_else(|| anyhow::anyhow!("--project <registered-checkout-id> is required"))?;
    let (method, path, body) = route(&owner_id, arguments.command)?;
    let home = hotsheet_plugins::hotsheet_home();
    // HS2-4J6V46: do not even resolve -C or construct runtime/identity/config owners
    // before this independent cleanup subset has had its chance to return.
    let result = if let Some(result) = request_cleanup_now(&home, method, &path, &body) {
        result
    } else {
        let source = hotsheet_cli::resolve_store_path(context.path.clone(), &context.cwd);
        if !source.is_dir() {
            bail!(
                "host configuration directory is unavailable; pass -C <existing-host-source> (or use list --target-id/unbind for orphan cleanup)"
            );
        }
        let registry = CheckoutRegistry::new(home.join("checkouts.json"));
        let owner = registry.describe(&owner_id).map_err(|_| anyhow::anyhow!("registered project is unavailable; use its checkout id or list --target-id/unbind for orphan cleanup"))?;
        let config = source.join("providers.json");
        if owner.sources.iter().any(|source| source.provider != "git") && !config.is_file() {
            bail!(
                "provider configuration is unavailable; pass -C <configured-host-source> containing the project's existing providers.json; no source was created"
            );
        }
        OrchestrationRuntime::live(&home, registry, ProviderConfigRegistry::new(config))
            .and_then(|runtime| runtime.request_now(method, &path, &body))
    };
    let output = result.map_err(|error| anyhow::anyhow!("{}: {}", error.code(), error))?;
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}
