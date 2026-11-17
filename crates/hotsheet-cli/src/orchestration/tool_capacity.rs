//! HS2-AHG7HW: AI tool capacity over the shared runtime. The policy configures tools
//! and accounts; this machine's registry holds what was observed; the tools' plugin
//! manifests say what can be observed at all. Reads write nothing and contact no tool.
use std::collections::BTreeMap;

use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::{
    Applied, AuditOutcome, AuditRecord, CapacityEntry, CapacityStatus, LimitSignal, PolicyBody,
    Selection, SelectionRequest, SessionCount, SignalKind, SignalSupport, ToolCapacityRegistry,
    known_spend_cents, select, status_for,
};
use serde::{Deserialize, Serialize};

use super::*;

/// One policy tool account as the scheduler and the workspace see it.
#[derive(Debug, Serialize)]
pub struct ToolCapacityRow {
    pub tool: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    #[serde(flatten)]
    pub status: CapacityStatus,
    /// Only `available` is verified; `degraded` and `unknown` are never assumed usable.
    pub verified: bool,
    /// What the tool's plugin lets this machine observe.
    pub signals: SignalSupport,
    pub max_sessions: u32,
    pub weight: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget_cents_per_day: Option<u64>,
    /// Today's observed cost (UTC); absent when it is not fully known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spent_cents_today: Option<u64>,
    pub consecutive_limits: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_signal_at_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_ok_at_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct ToolCapacityView {
    /// False when the project has no policy, and so no configured tools.
    pub enabled: bool,
    pub now_ms: u64,
    pub tools: Vec<ToolCapacityRow>,
}

#[derive(Debug, Serialize)]
pub struct SignalResult {
    /// `applied`, or `duplicate` / `stale` when the signal changed nothing.
    pub applied: Applied,
    pub tool: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    /// The signalled tool account's capacity after the signal.
    #[serde(flatten)]
    pub status: CapacityStatus,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectTool {
    pub target_id: String,
    /// The preferred tool; omit to ask for any tool the target allows.
    #[serde(default)]
    pub tool: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    /// Sessions the caller just observed per tool account. Required, because caps
    /// cannot be judged without it; an empty list states that none are running.
    pub load: Vec<SessionCount>,
}

#[derive(Debug, Serialize)]
pub struct ToolSelection {
    pub now_ms: u64,
    #[serde(flatten)]
    pub selection: Selection,
}

/// What each loaded plugin declares it can report; an unlisted tool reports nothing.
pub(super) fn supports(home: &Path) -> BTreeMap<String, SignalSupport> {
    hotsheet_plugins::all_plugins(&[home.join("plugins")])
        .into_iter()
        .map(|plugin| {
            let declared = plugin.capacity();
            (
                plugin.id().to_string(),
                SignalSupport {
                    limits: declared.reports_limits(),
                    reset_times: declared.reset_times.is_reported(),
                    usage: declared.usage_events.is_reported(),
                    cost: declared.cost.is_reported(),
                },
            )
        })
        .collect()
}

fn row(
    policy: &PolicyBody,
    entries: &[CapacityEntry],
    supports: &BTreeMap<String, SignalSupport>,
    tool: &str,
    account: Option<&str>,
    now_ms: u64,
) -> Option<ToolCapacityRow> {
    let configured = policy.tools.iter().find(|entry| entry.tool == tool)?;
    let account_policy = match account {
        Some(account) => Some(
            configured
                .accounts
                .iter()
                .find(|entry| entry.account == account)?,
        ),
        None => None,
    };
    let support = supports.get(tool).copied().unwrap_or_default();
    let budget = account_policy.and_then(|account| account.budget_cents_per_day);
    let entry = entries
        .iter()
        .find(|entry| entry.tool == tool && entry.account.as_deref() == account);
    let status = status_for(entries, tool, account, support, budget, now_ms);
    Some(ToolCapacityRow {
        tool: tool.into(),
        account: account.map(Into::into),
        status,
        verified: status.verified(),
        signals: support,
        max_sessions: account_policy
            .map_or(configured.max_sessions, |account| account.max_sessions),
        weight: configured.weight,
        budget_cents_per_day: budget,
        spent_cents_today: known_spend_cents(entry, support, now_ms),
        consecutive_limits: entry.map_or(0, |entry| entry.consecutive_limits),
        last_signal_at_ms: entry.map(|entry| entry.updated_at_ms),
        last_ok_at_ms: entry.and_then(|entry| entry.last_ok_at_ms),
    })
}

impl OrchestrationRuntime {
    /// Record a signal this machine's own detection observed, for any project. The
    /// tool must be a loaded plugin, which keeps the registry to real tools.
    pub fn record_tool_signal_at(
        home: &Path,
        signal: &LimitSignal,
        now_ms: u64,
    ) -> Result<Applied, OrchestrationError> {
        if !supports(home).contains_key(&signal.tool) {
            return Err(OrchestrationError::Invalid("tool is not a loaded plugin"));
        }
        let registry = ToolCapacityRegistry::new(home);
        // HS2-XFMKHQ: a usage signal arrives with every turn; it wakes the scheduler only
        // when it changes the account's standing, not when it only adds cost.
        let standing = |entry: &hotsheet_ticketing::orchestration::CapacityEntry| {
            (
                entry.limit.is_some(),
                entry.unconfirmed,
                entry.last_ok_at_ms.is_some(),
            )
        };
        let before = (signal.kind == hotsheet_ticketing::orchestration::SignalKind::Usage)
            .then(|| registry.entries())
            .transpose()?
            .map(|entries| {
                entries
                    .iter()
                    .find(|entry| entry.is_for(&signal.tool, signal.account.as_deref()))
                    .map(standing)
            });
        let outcome = registry.record(signal, now_ms)?;
        let changed = match before {
            Some(before) => before != Some(standing(&outcome.entry)),
            None => true,
        };
        if outcome.applied == Applied::Applied && changed {
            // HS2-ECWG50: a limit or a recovery changes where sessions may start.
            Self::scheduler_wake_at(home);
        }
        Ok(outcome.applied)
    }

    /// The capacity of every tool account the project's policy configures. A tool
    /// with accounts lists each account; one without lists itself once.
    pub fn tool_capacity(
        &self,
        owner_id: &str,
        tx: &mut Transaction<'_>,
    ) -> Result<ToolCapacityView, OrchestrationError> {
        let now_ms = tx.clock.now_ms();
        let Some(policy) = self.policy(owner_id)?.policy else {
            return Ok(ToolCapacityView {
                enabled: false,
                now_ms,
                tools: Vec::new(),
            });
        };
        let entries = ToolCapacityRegistry::new(&self.home).entries()?;
        let supports = supports(&self.home);
        let mut tools = Vec::new();
        for tool in &policy.tools {
            let accounts: Vec<Option<&str>> = if tool.accounts.is_empty() {
                vec![None]
            } else {
                tool.accounts
                    .iter()
                    .map(|account| Some(account.account.as_str()))
                    .collect()
            };
            for account in accounts {
                tools.extend(row(
                    &policy, &entries, &supports, &tool.tool, account, now_ms,
                ));
            }
        }
        Ok(ToolCapacityView {
            enabled: true,
            now_ms,
            tools,
        })
    }

    /// Report one signal for a tool the policy configures. A signal without an account
    /// is recorded as unattributed. Clearing a limit with `recovered` is a person's or
    /// the system's act: the session a limit holds back must not lift it.
    pub fn tool_capacity_signal(
        &self,
        owner_id: &str,
        signal: LimitSignal,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<SignalResult, OrchestrationError> {
        let now_ms = tx.clock.now_ms();
        signal.validate(now_ms)?;
        if signal.kind == SignalKind::Recovered && actor.is_some_and(MutationActor::is_ai) {
            return Err(OrchestrationError::Forbidden(
                "an AI actor cannot clear a tool capacity limit",
            ));
        }
        let policy = self
            .policy(owner_id)?
            .policy
            .ok_or(OrchestrationError::Invalid(
                "tool capacity needs a throughput policy",
            ))?;
        let configured = policy
            .tools
            .iter()
            .find(|tool| tool.tool == signal.tool)
            .ok_or(OrchestrationError::Invalid(
                "tool is not configured in the policy",
            ))?;
        if signal.account.as_ref().is_some_and(|account| {
            !configured
                .accounts
                .iter()
                .any(|entry| &entry.account == account)
        }) {
            return Err(OrchestrationError::Invalid(
                "account is not configured for the tool",
            ));
        }
        let registry = ToolCapacityRegistry::new(&self.home);
        let support = supports(&self.home)
            .get(&signal.tool)
            .copied()
            .unwrap_or_default();
        let budget = configured
            .accounts
            .iter()
            .find(|entry| Some(&entry.account) == signal.account.as_ref())
            .and_then(|account| account.budget_cents_per_day);
        let status = |entries: &[CapacityEntry]| {
            status_for(
                entries,
                &signal.tool,
                signal.account.as_deref(),
                support,
                budget,
                now_ms,
            )
        };
        let before = status(&registry.entries()?);
        let applied = registry.record(&signal, now_ms)?.applied;
        if applied == Applied::Applied {
            Self::scheduler_wake_at(&self.home);
        }
        let status = status(&registry.entries()?);
        if applied == Applied::Applied && status.state.code() != before.state.code() {
            self.audit_record(
                owner_id,
                AuditRecord {
                    at_ms: now_ms,
                    actor_role: access::role_code(actor),
                    actor_id: actor.and_then(|actor| actor.id.clone()),
                    target_id: None,
                    action: format!("tool_capacity_{}", signal.kind.code()),
                    operation_id: None,
                    outcome: AuditOutcome::Changed,
                    reason: Some(status.state.code().into()),
                    permission_id: None,
                    requested_by: None,
                },
            )?;
        }
        Ok(SignalResult {
            applied,
            tool: signal.tool,
            account: signal.account,
            status,
        })
    }

    /// The ordered tool accounts a session for one target may start on now, and why
    /// each other one is excluded. Reads only; it authorizes nothing.
    pub fn tool_select(
        &self,
        owner_id: &str,
        request: SelectTool,
        tx: &mut Transaction<'_>,
    ) -> Result<ToolSelection, OrchestrationError> {
        let now_ms = tx.clock.now_ms();
        let target = TargetRef {
            owner_checkout_id: owner_id.into(),
            target_id: request.target_id,
        };
        target.validate()?;
        if request.load.len() > 4096 {
            return Err(OrchestrationError::Invalid("too many load entries"));
        }
        let policy = self
            .policy(owner_id)?
            .policy
            .ok_or(OrchestrationError::Invalid(
                "tool capacity needs a throughput policy",
            ))?;
        let entries = ToolCapacityRegistry::new(&self.home).entries()?;
        let supports = supports(&self.home);
        let selection = select(
            &SelectionRequest {
                policy: &policy,
                target_id: &target.target_id,
                tool: request.tool.as_deref(),
                model: request.model.as_deref(),
                entries: &entries,
                load: &request.load,
                now_ms,
            },
            &|tool| supports.get(tool).copied().unwrap_or_default(),
        );
        Ok(ToolSelection { now_ms, selection })
    }
}
