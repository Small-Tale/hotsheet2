//! HS2-7J2ZSE: the provisional readiness requests (HS2-2SJPCH) in one place, so the
//! CLI, the authenticated routes and the MCP tool call the same `Coordinator`
//! functions with the same answers. Each request names a registered checkout's project
//! root and its local ticket source; nothing here takes a caller-supplied path.

use std::path::Path;
use std::time::Duration;

use anyhow::{Result, bail, ensure};
use hotsheet_ticketing::provisional::{
    GateObservation, GateProbe, GateResult, IntegrationRule, SpeculationPolicy,
};
use hotsheet_ticketing::{FsStore, Settings};
use serde::Deserialize;
use serde_json::Value;

use super::Coordinator;
use super::speculation::{Rebase, follow};

/// Longest single `sync` wait the CLI accepts; a caller that needs longer asks again.
pub const MAX_SYNC_WAIT_MS: u64 = 10 * 60 * 1000;
/// Longest single `sync` wait over HTTP and MCP, under client request timeouts.
pub const MAX_HTTP_SYNC_WAIT_MS: u64 = 25_000;

/// A change to the project's committed provisional readiness policy. Absent members
/// leave the policy as it is.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyChange {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub max_depth: Option<u32>,
    #[serde(default)]
    pub integration: Option<IntegrationRule>,
    /// A branch name sets it; `null` with `clear_base_branch` clears it.
    #[serde(default)]
    pub base_branch: Option<String>,
    #[serde(default)]
    pub clear_base_branch: bool,
}

/// One provisional readiness request.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum SpeculationRequest {
    Explain {
        ticket: String,
    },
    Status,
    Result {
        gate_commit: String,
        result: GateResult,
        #[serde(default)]
        run: Option<String>,
    },
    Sync {
        #[serde(default)]
        wait_ms: u64,
    },
    Rebase {
        assignment: String,
        replaces: String,
        gate_commit: String,
        #[serde(default)]
        base: Option<String>,
    },
    Reopen {
        assignment: String,
        reason: String,
    },
}

impl SpeculationRequest {
    /// Whether the request changes the ledger or a ticket (a read-only device is refused).
    pub fn mutates(&self) -> bool {
        !matches!(self, Self::Explain { .. } | Self::Status)
    }
}

/// Read, and with a change write, the project's provisional readiness policy. The same
/// change again writes nothing.
pub fn speculation_policy(project: &Path, change: &PolicyChange) -> Result<SpeculationPolicy> {
    ensure!(
        !(change.base_branch.is_some() && change.clear_base_branch),
        "base_branch and clear_base_branch conflict"
    );
    let settings = Settings::for_project(project);
    let mut policy = SpeculationPolicy::read(&settings)?;
    let before = policy.clone();
    if let Some(enabled) = change.enabled {
        policy.enabled = enabled;
    }
    if let Some(depth) = change.max_depth {
        policy.max_depth = depth;
    }
    if let Some(rule) = change.integration {
        policy.integration = rule;
    }
    if let Some(branch) = &change.base_branch {
        policy.base_branch = Some(branch.clone());
    }
    if change.clear_base_branch {
        policy.base_branch = None;
    }
    if policy != before {
        policy.write(&settings)?;
        policy = SpeculationPolicy::read(&settings)?;
    }
    Ok(policy)
}

/// Answer one request for the project's pool. `store` is the local ticket source;
/// `explain` and `reopen` need it. `max_wait_ms` bounds `sync`.
pub fn speculation_request(
    coordinator: &Coordinator,
    project: &Path,
    store: Option<&FsStore>,
    request: SpeculationRequest,
    actor: Option<&str>,
    max_wait_ms: u64,
    now: i64,
) -> Result<Value> {
    let policy = SpeculationPolicy::read(&Settings::for_project(project))?;
    Ok(match request {
        SpeculationRequest::Status => serde_json::json!({
            "policy": policy,
            "status": coordinator.speculation_ledger()?.unwrap_or_default().status(),
        }),
        SpeculationRequest::Explain { ticket } => {
            let Some(store) = store else {
                bail!("explaining readiness needs the local ticket source");
            };
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
        SpeculationRequest::Result {
            gate_commit,
            result,
            run,
        } => {
            ensure!(
                result != GateResult::Unknown,
                "an unknown result records nothing; omit the request"
            );
            serde_json::to_value(coordinator.speculation_result(
                &gate_commit,
                GateObservation { result, run },
                store,
                now,
            )?)?
        }
        SpeculationRequest::Sync { wait_ms } => {
            ensure!(
                wait_ms <= max_wait_ms,
                "wait-ms is at most {max_wait_ms}; call again to keep waiting"
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
                        store,
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
        SpeculationRequest::Rebase {
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
                actor,
            )?)?
        }
        SpeculationRequest::Reopen { assignment, reason } => {
            let Some(store) = store else {
                bail!("reopening writes a ticket note; it needs the local ticket source");
            };
            serde_json::to_value(coordinator.speculation_reopen_noted(
                store,
                &assignment,
                &reason,
                now,
                actor,
            )?)?
        }
    })
}
