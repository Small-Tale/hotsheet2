//! Headless provisional readiness commands (HS2-2SJPCH): the project policy, the
//! explanation of one ticket, the ledger status, CI results (reported or read from the
//! CI watch Jobs without polling), rebase, reopen and the worktree branch record. The
//! rule is `hotsheet_ticketing::provisional`; the transitions and I/O are
//! `hotsheet_cli::coordination::speculation` and `::speculation_runtime`.

use std::path::Path;
use std::time::Duration;

use anyhow::{Result, ensure};
use hotsheet_cli::coordination::Coordinator;
use hotsheet_cli::coordination::speculation::{Rebase, follow};
use hotsheet_ticketing::provisional::{
    GateObservation, GateProbe, GateResult, IntegrationRule, SpeculationPolicy,
};
use hotsheet_ticketing::{FsStore, Settings};

use super::{CoordinationCmd, Ctx};

/// Longest single wait `speculation-sync` accepts; a caller that needs longer calls again.
const MAX_WAIT_MS: u64 = 10 * 60 * 1000;

pub(super) fn parse_integration(raw: &str) -> Result<IntegrationRule, String> {
    serde_json::from_value(serde_json::Value::String(raw.to_owned()))
        .map_err(|_| "integration must be push-per-ticket or parked".to_owned())
}

pub(super) fn parse_result(raw: &str) -> Result<GateResult, String> {
    serde_json::from_value(serde_json::Value::String(raw.to_owned()))
        .map_err(|_| "result must be pending, green, red, superseded or cancelled".to_owned())
}

/// Run `speculation-policy` against the project's committed settings, without opening
/// the pool; false for any other command.
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
    let settings = Settings::for_project(project);
    let mut policy = SpeculationPolicy::read(&settings)?;
    let before = policy.clone();
    if *enable {
        policy.enabled = true;
    }
    if *disable {
        policy.enabled = false;
    }
    if let Some(depth) = max_depth {
        policy.max_depth = *depth;
    }
    if let Some(rule) = integration {
        policy.integration = *rule;
    }
    if let Some(branch) = base_branch {
        policy.base_branch = Some(branch.clone());
    }
    if *clear_base_branch {
        policy.base_branch = None;
    }
    if policy != before {
        policy.write(&settings)?;
        policy = SpeculationPolicy::read(&settings)?;
    }
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
    let policy = SpeculationPolicy::read(&Settings::for_project(project))?;
    let store = local_store(ctx)?;
    Ok(match command {
        CoordinationCmd::SpeculationStatus => serde_json::json!({
            "policy": policy,
            "status": coordinator.speculation_ledger()?.unwrap_or_default().status(),
        }),
        CoordinationCmd::SpeculationExplain { ticket } => {
            let store = store.ok_or_else(|| {
                anyhow::anyhow!("explaining readiness needs the local ticket source (-C)")
            })?;
            let tickets = store.list_tickets()?;
            let found = tickets
                .iter()
                .find(|t| t.slug.eq_ignore_ascii_case(&ticket) || t.id.to_string() == ticket)
                .ok_or_else(|| anyhow::anyhow!("no ticket {ticket} in this source"))?;
            let probe = coordinator.speculation_probe(&policy);
            serde_json::to_value(
                coordinator.speculation_explain(&policy, &probe, found, &tickets)?,
            )?
        }
        CoordinationCmd::SpeculationResult {
            gate_commit,
            result,
            run,
        } => {
            ensure!(
                result != GateResult::Unknown,
                "an unknown result records nothing; omit the command"
            );
            serde_json::to_value(coordinator.speculation_result(
                &gate_commit,
                GateObservation { result, run },
                store.as_ref(),
                now,
            )?)?
        }
        CoordinationCmd::SpeculationSync { wait_ms } => {
            ensure!(
                wait_ms <= MAX_WAIT_MS,
                "wait-ms is at most {MAX_WAIT_MS}; call again to keep waiting"
            );
            let probe = coordinator.speculation_probe(&policy);
            let waiting = coordinator
                .speculation_ledger()?
                .is_some_and(|ledger| !ledger.watched_commits().is_empty());
            let mut last = None;
            follow(
                Duration::from_millis(if waiting { wait_ms } else { 0 }),
                || {
                    // The revision is read before the gates, so a change during the
                    // read is seen by the wait.
                    let revision = probe.revision();
                    let report = coordinator.speculation_sync(
                        &probe,
                        store.as_ref(),
                        time::OffsetDateTime::now_utc().unix_timestamp(),
                    )?;
                    let changes = report.changes.clone();
                    last = Some(report);
                    Ok((changes, revision))
                },
                |revision, remaining| Ok(probe.wait(revision, remaining)),
            )?;
            let mut report = serde_json::to_value(last)?;
            // `none` means no CI watch exists for this checkout: the call returned at
            // once, and repeating it in a loop would be polling.
            report["event_source"] = if probe.watchable() {
                "ci_watch"
            } else {
                "none"
            }
            .into();
            report
        }
        CoordinationCmd::SpeculationRebase {
            assignment,
            replaces,
            gate_commit,
            base,
        } => {
            let probe = coordinator.speculation_probe(&policy);
            let gate = probe.gate(&gate_commit);
            serde_json::to_value(coordinator.speculation_rebase_checked(
                &probe,
                &assignment,
                &Rebase {
                    replaces,
                    gate_commit,
                    gate,
                    base,
                },
                now,
                actor(ctx).as_deref(),
            )?)?
        }
        CoordinationCmd::SpeculationReopen { assignment, reason } => {
            let store = store.ok_or_else(|| {
                anyhow::anyhow!("reopening writes a ticket note; pass the local ticket source (-C)")
            })?;
            serde_json::to_value(coordinator.speculation_reopen_noted(
                &store,
                &assignment,
                &reason,
                now,
                actor(ctx).as_deref(),
            )?)?
        }
        CoordinationCmd::SpeculationBranch {
            assignment,
            worktree,
        } => serde_json::to_value(coordinator.speculation_branch(&worktree, &assignment)?)?,
        _ => unreachable!("only speculation commands are routed here"),
    })
}
