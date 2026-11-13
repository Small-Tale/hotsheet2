//! HS2-4J6V46: headless target operations, with cleanup before host config access.
//! HS2-SBQ3T8 adds grants, access checks and the audit journal on the same runtime;
//! HS2-YCJDEB the throughput policy and approvals.
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
    /// List, issue, revoke and check delegated authority over one bound target.
    Grant(GrantArgs),
    /// Show, set, validate and dry-run the checked-in throughput policy.
    Policy(PolicyArgs),
    /// List and answer decisions the policy will not take without a person.
    Approval(ApprovalArgs),
    /// Read this machine's audit journal of orchestration decisions for one project.
    Audit {
        /// Registered owning checkout id.
        #[arg(long)]
        project: Option<String>,
        /// Return entries after this sequence number.
        #[arg(long, default_value_t = 0)]
        after: u64,
        #[arg(long, default_value_t = 100)]
        limit: u64,
    },
}
#[derive(Args)]
struct PolicyArgs {
    /// Registered owning checkout id.
    #[arg(long, global = true)]
    project: Option<String>,
    #[command(subcommand)]
    command: PolicyCmd,
}
#[derive(Subcommand)]
enum PolicyCmd {
    /// The stored policy and its revision; an absent policy reads as disabled.
    Show,
    /// Replace the policy from a JSON file (`-` for stdin). Re-running with the same
    /// content changes nothing. A person runs this; an AI actor is refused.
    Set {
        #[arg(long)]
        file: String,
        /// Require this stored revision; omitted, the current one is read first.
        #[arg(long)]
        expected_revision: Option<String>,
    },
    /// Report every problem in a policy file, or in the stored policy. Writes nothing.
    Validate {
        #[arg(long)]
        file: Option<String>,
    },
    /// What a policy would let the orchestrator do per declared target, and whether
    /// an actor's grants agree. Writes nothing and contacts no target.
    DryRun {
        #[arg(long)]
        file: Option<String>,
        #[arg(long)]
        actor: Option<String>,
    },
}
#[derive(Args)]
struct ApprovalArgs {
    /// Registered owning checkout id.
    #[arg(long, global = true)]
    project: Option<String>,
    #[command(subcommand)]
    command: ApprovalCmd,
}
#[derive(Subcommand)]
enum ApprovalCmd {
    /// Requests for this project with their state.
    List,
    /// Allow one pending request; the approval is used once, within an hour.
    Approve { approval_id: String },
    /// Refuse one pending request.
    Deny { approval_id: String },
}
/// A policy body from a file, or stdin for `-`.
fn policy_file(path: &str) -> Result<Value> {
    let text = if path == "-" {
        std::io::read_to_string(std::io::stdin())?
    } else {
        std::fs::read_to_string(path)
            .map_err(|error| anyhow::anyhow!("cannot read policy file: {error}"))?
    };
    let mut value: Value =
        serde_json::from_str(&text).map_err(|_| anyhow::anyhow!("policy file must be JSON"))?;
    // Accept either a bare policy body or the `show` output, so show > edit > set works.
    if let Some(policy) = value.get_mut("policy").map(Value::take) {
        value = policy;
    }
    Ok(value)
}
#[derive(Args)]
struct GrantArgs {
    /// Registered owning checkout id, never a source/store id or label.
    #[arg(long, global = true)]
    project: Option<String>,
    #[command(subcommand)]
    command: GrantCmd,
}
#[derive(Subcommand)]
enum GrantCmd {
    /// Retained grants for a target with their state against this machine's binding.
    List { target_id: String },
    /// Issue a grant for one actor. A person runs this; an AI actor is refused.
    Add {
        target_id: String,
        /// Retain an id across retries; omission mints one.
        #[arg(long)]
        grant_id: Option<String>,
        /// The actor id the grant covers (a worker id, account, or tool name).
        #[arg(long)]
        actor: String,
        /// tickets_read, tickets_write, work_start, gates_observe, validation_run,
        /// sessions_manage. Repeatable.
        #[arg(long = "action", required = true)]
        actions: Vec<String>,
        /// Lifetime such as 30m, 8h or 14d (at most 90d).
        #[arg(long, conflicts_with = "expires_at_ms")]
        expires_in: Option<String>,
        /// Absolute expiry in Unix milliseconds; use it to retry with a retained id.
        #[arg(long)]
        expires_at_ms: Option<u64>,
        /// For sessions_manage: `tool` or `tool:model,model`. Repeatable.
        #[arg(long = "tool")]
        tools: Vec<String>,
        /// For sessions_manage: most sessions this actor may run on the target.
        #[arg(long)]
        max_concurrency: Option<u32>,
    },
    /// Withdraw a grant; idempotent, and works for an unreachable or unbound target.
    Revoke { target_id: String, grant_id: String },
    /// Report whether the cached binding and grants would allow an action. Not authority.
    Check {
        target_id: String,
        #[arg(long)]
        actor: String,
        #[arg(long)]
        action: String,
        #[arg(long)]
        tool: Option<String>,
        #[arg(long)]
        model: Option<String>,
        /// Sessions this actor already runs on the target.
        #[arg(long, default_value_t = 0)]
        running: u32,
        /// The check concerns starting one more session.
        #[arg(long)]
        starting: bool,
        /// HS2-YCJDEB policy caps: orchestrator sessions already on the target.
        #[arg(long, default_value_t = 0)]
        target_sessions: u32,
        /// Orchestrator sessions already on the target's machine.
        #[arg(long, default_value_t = 0)]
        machine_sessions: u32,
        /// Orchestrator sessions already using the tool.
        #[arg(long, default_value_t = 0)]
        tool_sessions: u32,
        /// The tool account the session would run under.
        #[arg(long)]
        account: Option<String>,
        #[arg(long, default_value_t = 0)]
        account_sessions: u32,
    },
}
/// `30m`, `8h`, `14d`, or bare seconds.
fn lifetime_ms(value: &str) -> Result<u64> {
    let value = value.trim();
    let (digits, unit) = match value.char_indices().find(|(_, c)| !c.is_ascii_digit()) {
        Some((index, _)) => value.split_at(index),
        None => (value, "s"),
    };
    let scale: u64 = match unit {
        "s" => 1_000,
        "m" => 60_000,
        "h" => 3_600_000,
        "d" => 86_400_000,
        _ => bail!("lifetime must look like 30m, 8h or 14d"),
    };
    digits
        .parse::<u64>()
        .ok()
        .and_then(|count| count.checked_mul(scale))
        .filter(|ms| *ms > 0)
        .ok_or_else(|| anyhow::anyhow!("lifetime must look like 30m, 8h or 14d"))
}
fn grant_route(project: &str, command: GrantCmd) -> Result<(&'static str, String, Value)> {
    let prefix = |id: &str| format!("/checkouts/{project}/orchestration/targets/{id}");
    Ok(match command {
        GrantCmd::List { target_id } => {
            ("GET", format!("{}/grants", prefix(&target_id)), Value::Null)
        }
        GrantCmd::Add {
            target_id,
            grant_id,
            actor,
            actions,
            expires_in,
            expires_at_ms,
            tools,
            max_concurrency,
        } => {
            let expires_at_ms = match (expires_in, expires_at_ms) {
                (Some(lifetime), None) => {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|time| u64::try_from(time.as_millis()).unwrap_or(u64::MAX))
                        .unwrap_or(0);
                    now.saturating_add(lifetime_ms(&lifetime)?)
                }
                (None, Some(at)) => at,
                _ => bail!("pass exactly one of --expires-in or --expires-at-ms"),
            };
            let session_scope = match (tools.is_empty(), max_concurrency) {
                (true, None) => Value::Null,
                (false, Some(max_concurrency)) => json!({
                    "max_concurrency": max_concurrency,
                    "tools": tools.iter().map(|tool| match tool.split_once(':') {
                        Some((tool, models)) => json!({"tool":tool,"models":models.split(',').collect::<Vec<_>>()}),
                        None => json!({"tool":tool}),
                    }).collect::<Vec<_>>(),
                }),
                _ => bail!("a session scope needs both --tool and --max-concurrency"),
            };
            (
                "POST",
                format!("{}/grants", prefix(&target_id)),
                json!({"grant_id":grant_id,"actor_id":actor,"actions":actions,
                    "session_scope":session_scope,"expires_at_ms":expires_at_ms}),
            )
        }
        GrantCmd::Revoke {
            target_id,
            grant_id,
        } => (
            "DELETE",
            format!("{}/grants/{grant_id}", prefix(&target_id)),
            json!({}),
        ),
        GrantCmd::Check {
            target_id,
            actor,
            action,
            tool,
            model,
            running,
            starting,
            target_sessions,
            machine_sessions,
            tool_sessions,
            account,
            account_sessions,
        } => {
            // A session check always states its load; zeros mean an idle machine.
            let load = tool.as_ref().map(|_| {
                json!({"target_sessions":target_sessions,"machine_sessions":machine_sessions,
                    "tool_sessions":tool_sessions,"account":account,"account_sessions":account_sessions})
            });
            let session = match tool {
                Some(tool) => {
                    json!({"tool":tool,"model":model,"running":running,"starting":starting})
                }
                None if model.is_some() || starting || running > 0 => {
                    bail!("--model, --running and --starting need --tool")
                }
                None => Value::Null,
            };
            (
                "POST",
                format!("{}/access", prefix(&target_id)),
                json!({"actor_id":actor,"action":action,"session":session,"load":load}),
            )
        }
    })
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
    let required = |project: Option<String>| {
        project.ok_or_else(|| anyhow::anyhow!("--project <registered-checkout-id> is required"))
    };
    let (owner_id, (method, path, mut body)) = match arguments.command {
        OrchestrationCmd::Target(arguments) => {
            let owner_id = required(arguments.project.clone())?;
            let request = route(&owner_id, arguments.command)?;
            (owner_id, request)
        }
        OrchestrationCmd::Grant(arguments) => {
            let owner_id = required(arguments.project.clone())?;
            let request = grant_route(&owner_id, arguments.command)?;
            (owner_id, request)
        }
        OrchestrationCmd::Policy(arguments) => {
            let owner_id = required(arguments.project.clone())?;
            let prefix = format!("/checkouts/{owner_id}/orchestration/policy");
            let proposal = |file: Option<String>, actor: Option<String>| -> Result<Value> {
                Ok(json!({
                    "policy": file.as_deref().map(policy_file).transpose()?,
                    "actor_id": actor,
                }))
            };
            let request = match arguments.command {
                PolicyCmd::Show => ("GET", prefix, Value::Null),
                PolicyCmd::Set {
                    file,
                    expected_revision,
                } => (
                    "PUT",
                    prefix,
                    // `expected_revision` is filled in below when it was omitted.
                    json!({"policy": policy_file(&file)?, "expected_revision": expected_revision,
                        "read_revision": expected_revision.is_none()}),
                ),
                PolicyCmd::Validate { file } => {
                    ("POST", format!("{prefix}/validate"), proposal(file, None)?)
                }
                PolicyCmd::DryRun { file, actor } => {
                    ("POST", format!("{prefix}/dry-run"), proposal(file, actor)?)
                }
            };
            (owner_id, request)
        }
        OrchestrationCmd::Approval(arguments) => {
            let owner_id = required(arguments.project.clone())?;
            let prefix = format!("/checkouts/{owner_id}/orchestration/approvals");
            let request = match arguments.command {
                ApprovalCmd::List => ("GET", prefix, Value::Null),
                ApprovalCmd::Approve { approval_id } => (
                    "POST",
                    format!("{prefix}/{approval_id}"),
                    json!({"approve": true}),
                ),
                ApprovalCmd::Deny { approval_id } => (
                    "POST",
                    format!("{prefix}/{approval_id}"),
                    json!({"approve": false}),
                ),
            };
            (owner_id, request)
        }
        OrchestrationCmd::Audit {
            project,
            after,
            limit,
        } => {
            let owner_id = required(project)?;
            let path =
                format!("/checkouts/{owner_id}/orchestration/audit?after={after}&limit={limit}");
            (owner_id, ("GET", path, Value::Null))
        }
    };
    // HS2-SBQ3T8: authority decisions record who acted; the runtime strips this again
    // before decoding a request body.
    if let (Some(actor), Some(object)) = (&context.actor, body.as_object_mut()) {
        object.insert("actor".into(), serde_json::to_value(actor)?);
    }
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
        OrchestrationRuntime::live(&home, registry, ProviderConfigRegistry::new(config)).and_then(
            |runtime| {
                // HS2-YCJDEB: `policy set` without a revision is read-then-set, so a
                // repeated setup run is a no-op and a concurrent edit still conflicts.
                if body.get("read_revision").is_some() {
                    let read = body["read_revision"] == true;
                    if let Some(object) = body.as_object_mut() {
                        object.remove("read_revision");
                    }
                    if read {
                        body["expected_revision"] =
                            runtime.request_now("GET", &path, &Value::Null)?["revision"].take();
                    }
                }
                runtime.request_now(method, &path, &body)
            },
        )
    };
    let output = result.map_err(|error| anyhow::anyhow!("{}: {}", error.code(), error))?;
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}
