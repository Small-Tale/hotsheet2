//! Headless provisional readiness commands (HS2-2SJPCH): the project policy, the
//! explanation of one ticket, the ledger status, CI results (reported or read from the
//! CI watch Jobs without polling), rebase, reopen and the worktree branch record. The
//! rule is `hotsheet_ticketing::provisional`; the transitions and I/O are
//! `hotsheet_cli::coordination::speculation` and `::speculation_runtime`.

use std::path::Path;

use anyhow::Result;
use hotsheet_cli::coordination::Coordinator;
use hotsheet_cli::coordination::speculation_api::{
    MAX_SYNC_WAIT_MS, PolicyChange, SpeculationRequest, speculation_policy, speculation_request,
};
use hotsheet_ticketing::FsStore;
use hotsheet_ticketing::provisional::{GateResult, IntegrationRule};

use super::{CoordinationCmd, Ctx};

pub(super) fn parse_integration(raw: &str) -> Result<IntegrationRule, String> {
    serde_json::from_value(serde_json::Value::String(raw.to_owned()))
        .map_err(|_| "integration must be push-per-ticket or parked".to_owned())
}

pub(super) fn parse_result(raw: &str) -> Result<GateResult, String> {
    serde_json::from_value(serde_json::Value::String(raw.to_owned()))
        .map_err(|_| "result must be pending, green, red, superseded or cancelled".to_owned())
}

/// Run `speculation-policy` against the project's committed settings, without opening
/// the pool; false for any other command. HS2-7J2ZSE: the same write the routes use.
pub(super) fn policy(project: &Path, command: &CoordinationCmd) -> Result<bool> {
    let CoordinationCmd::SpeculationPolicy {
        enable,
        disable,
        max_depth,
        integration,
        base_branch,
        clear_base_branch,
    } = command
    else {
        return Ok(false);
    };
    let change = PolicyChange {
        enabled: if *enable {
            Some(true)
        } else if *disable {
            Some(false)
        } else {
            None
        },
        max_depth: *max_depth,
        integration: *integration,
        base_branch: base_branch.clone(),
        clear_base_branch: *clear_base_branch,
    };
    let policy = speculation_policy(project, &change)?;
    println!("{}", serde_json::to_string_pretty(&policy)?);
    Ok(true)
}

fn local_store(ctx: &Ctx) -> Result<Option<FsStore>> {
    Ok(
        if ctx
            .path
            .join(hotsheet_ticketing::STORE_METADATA_FILE)
            .exists()
        {
            Some(FsStore::open_without_maintenance(&ctx.path)?)
        } else {
            None
        },
    )
}

fn actor(ctx: &Ctx) -> Option<String> {
    ctx.actor.as_ref().and_then(|actor| actor.id.clone())
}

pub(super) fn run(
    coordinator: &Coordinator,
    ctx: &Ctx,
    project: &Path,
    command: CoordinationCmd,
    now: i64,
) -> Result<serde_json::Value> {
    let store = local_store(ctx)?;
    let request = match command {
        CoordinationCmd::SpeculationStatus => SpeculationRequest::Status,
        CoordinationCmd::SpeculationExplain { ticket } => SpeculationRequest::Explain { ticket },
        CoordinationCmd::SpeculationResult {
            gate_commit,
            result,
            run,
        } => SpeculationRequest::Result {
            gate_commit,
            result,
            run,
        },
        CoordinationCmd::SpeculationSync { wait_ms } => SpeculationRequest::Sync { wait_ms },
        CoordinationCmd::SpeculationRebase {
            assignment,
            replaces,
            gate_commit,
            base,
        } => SpeculationRequest::Rebase {
            assignment,
            replaces,
            gate_commit,
            base,
        },
        CoordinationCmd::SpeculationReopen { assignment, reason } => {
            SpeculationRequest::Reopen { assignment, reason }
        }
        // A worktree is a local path: the branch record stays a command of this machine.
        CoordinationCmd::SpeculationBranch {
            assignment,
            worktree,
        } => {
            return Ok(serde_json::to_value(
                coordinator.speculation_branch(&worktree, &assignment)?,
            )?);
        }
        _ => unreachable!("only speculation commands are routed here"),
    };
    speculation_request(
        coordinator,
        project,
        store.as_ref(),
        request,
        actor(ctx).as_deref(),
        MAX_SYNC_WAIT_MS,
        now,
    )
}
