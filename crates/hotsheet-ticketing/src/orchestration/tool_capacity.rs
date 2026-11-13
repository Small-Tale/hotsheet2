//! HS2-AHG7HW: AI tool capacity from configured limits and observed signals.
//!
//! One record per `(tool, account)` on this machine. Signals carry identifiers and
//! metrics only: a limit error, a reported reset time, a recovery, or a usage event
//! with its cost. [`apply_signal`] folds a signal into the record, [`state_at`] reads
//! the record at an instant, and [`select`] orders a target's allowed tools for the
//! scheduler. All three are pure; the clock is an argument.
//!
//! Unknown means unknown. A tool whose plugin cannot report limits, or that has not
//! been observed, is never described as available.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::contract::valid_identifier;
use super::storage::{Storage, decode};
use super::*;

pub const MAX_CAPACITY_ENTRIES: usize = 256;
/// Signals kept per record. De-duplication is exact inside this window; a signal
/// older than a full window is dropped as stale because it cannot be told from a repeat.
pub const MAX_RECENT_SIGNALS: usize = 16;
/// A limit without a reported reset time backs off for `base * factor^(n-1)`, capped,
/// where `n` counts limits since the last confirmed recovery.
pub const BACKOFF_BASE_MS: u64 = 5 * 60 * 1000;
pub const BACKOFF_FACTOR: u64 = 2;
pub const BACKOFF_CAP_MS: u64 = 4 * 60 * 60 * 1000;
/// A reported reset is believed only this far after its limit, like the stuck-cause
/// reader that parses it.
pub const MAX_RESET_AHEAD_MS: u64 = 8 * 24 * 60 * 60 * 1000;
/// Clock skew allowed between reporter and registry, and between a limit and its reset.
pub const MAX_SIGNAL_SKEW_MS: u64 = 5 * 60 * 1000;
/// One signal's cost ceiling; a larger value is a unit mistake.
pub const MAX_SIGNAL_COST_CENTS: u64 = 100_000_000;
const DAY_MS: u64 = 24 * 60 * 60 * 1000;
const MAX_CAPACITY_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalKind {
    /// A turn failed on a request-rate limit.
    RateLimit,
    /// A turn failed on a plan or quota usage limit.
    UsageLimit,
    /// The limit is known to be over: a limited session resumed, or a person cleared it.
    Recovered,
    /// A turn completed and reported its usage.
    Usage,
}

impl SignalKind {
    pub fn code(self) -> &'static str {
        match self {
            Self::RateLimit => "rate_limit",
            Self::UsageLimit => "usage_limit",
            Self::Recovered => "recovered",
            Self::Usage => "usage",
        }
    }

    pub fn is_limit(self) -> bool {
        matches!(self, Self::RateLimit | Self::UsageLimit)
    }
}

/// One observation about a tool account. Identifiers and metrics only: no provider
/// message, prompt or ticket text ever enters a signal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LimitSignal {
    pub tool: String,
    /// Absent when the observer cannot tell which account the session used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    pub kind: SignalKind,
    pub at_ms: u64,
    /// Limit signals only: when the provider says the limit ends.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at_ms: Option<u64>,
    /// Usage signals only: the turn's cost. Absent means the cost is unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_cents: Option<u64>,
    /// An opaque session id that keeps two sessions' otherwise equal signals apart.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
}

impl LimitSignal {
    /// Shape and plausibility against the registry's clock.
    pub fn validate(&self, now_ms: u64) -> Result<(), OrchestrationError> {
        valid_identifier(&self.tool)?;
        for value in [&self.account, &self.session].into_iter().flatten() {
            valid_identifier(value)?;
        }
        if self.at_ms == 0 || self.at_ms > now_ms.saturating_add(MAX_SIGNAL_SKEW_MS) {
            return Err(OrchestrationError::Invalid(
                "signal time must be a past unix millisecond time",
            ));
        }
        match self.resets_at_ms {
            Some(_) if !self.kind.is_limit() => {
                return Err(OrchestrationError::Invalid(
                    "only a limit signal carries a reset time",
                ));
            }
            Some(reset)
                if reset.saturating_add(MAX_SIGNAL_SKEW_MS) < self.at_ms
                    || reset > self.at_ms.saturating_add(MAX_RESET_AHEAD_MS) =>
            {
                return Err(OrchestrationError::Invalid(
                    "reset time must be within eight days after the limit",
                ));
            }
            _ => {}
        }
        match self.cost_cents {
            Some(_) if self.kind != SignalKind::Usage => Err(OrchestrationError::Invalid(
                "only a usage signal carries a cost",
            )),
            Some(cost) if cost > MAX_SIGNAL_COST_CENTS => {
                Err(OrchestrationError::Invalid("cost is in cents"))
            }
            _ => Ok(()),
        }
    }
}

/// What the tool's plugin lets the host observe. The default observes nothing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignalSupport {
    /// Some limit error is reported, so "no limit seen" is evidence.
    pub limits: bool,
    pub reset_times: bool,
    pub usage: bool,
    pub cost: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum CapacityState {
    Available,
    Limited {
        until_ms: u64,
    },
    /// Usable only as a probe: recovery is unconfirmed or the signals disagree.
    Degraded,
    Unknown,
}

impl CapacityState {
    pub fn code(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Limited { .. } => "limited",
            Self::Degraded => "degraded",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapacityReason {
    RateLimit,
    UsageLimit,
    /// The account's observed cost reached its daily budget; ends at the next UTC day.
    BudgetExceeded,
    /// A backoff without a reported reset time ended and no turn has succeeded since.
    RecoveryUnconfirmed,
    /// A turn completed while the account was backing off.
    InconsistentSignals,
    /// A limit was observed for this tool on an account that could not be identified.
    UnattributedLimit,
    /// The plugin cannot report limits for this tool.
    NotReported,
    /// Nothing has been observed for this account yet.
    NotObserved,
    /// A daily budget is set but the cost is not known, so the budget cannot be judged.
    BudgetUnknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct CapacityStatus {
    #[serde(flatten)]
    pub state: CapacityState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<CapacityReason>,
}

impl CapacityStatus {
    fn new(state: CapacityState, reason: CapacityReason) -> Self {
        Self {
            state,
            reason: Some(reason),
        }
    }

    /// Only an observed, unrestricted account is verified; nothing else is assumed.
    pub fn verified(self) -> bool {
        self.state == CapacityState::Available
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActiveLimit {
    pub kind: SignalKind,
    pub since_ms: u64,
    pub until_ms: u64,
    /// The provider stated `until_ms`; otherwise it is this registry's backoff.
    pub reset_reported: bool,
}

/// Cost observed during one UTC day.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DaySpend {
    /// Days since the Unix epoch, UTC.
    pub day: u64,
    pub cents: u64,
    /// Usage signals that day without a cost; any makes the day's total unknown.
    pub unpriced: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapacityEntry {
    pub tool: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<ActiveLimit>,
    /// Limits since the last confirmed recovery; it sets the next backoff.
    pub consecutive_limits: u32,
    /// A turn completed during a backoff, so neither reading is trusted yet.
    pub unconfirmed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_ok_at_ms: Option<u64>,
    /// Time of the newest signal that changed the limit state. Older limit and
    /// recovery signals are stale.
    pub changed_at_ms: u64,
    /// Time of the newest signal of any kind.
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spend: Option<DaySpend>,
    pub recent: Vec<LimitSignal>,
}

impl CapacityEntry {
    fn new(tool: &str, account: Option<&str>) -> Self {
        Self {
            tool: tool.into(),
            account: account.map(Into::into),
            limit: None,
            consecutive_limits: 0,
            unconfirmed: false,
            last_ok_at_ms: None,
            changed_at_ms: 0,
            updated_at_ms: 0,
            spend: None,
            recent: Vec::new(),
        }
    }

    fn is_for(&self, tool: &str, account: Option<&str>) -> bool {
        self.tool == tool && self.account.as_deref() == account
    }
}

/// What a signal did to its record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Applied {
    Applied,
    /// The identical signal was already recorded; nothing changed.
    Duplicate,
    /// Older than the state it would change, or than the de-duplication window;
    /// nothing changed.
    Stale,
}

/// The backoff after the `consecutive`-th limit in a row (1-based), bounded.
pub fn backoff_ms(consecutive: u32) -> u64 {
    let mut backoff = BACKOFF_BASE_MS;
    for _ in 1..consecutive {
        backoff = backoff.saturating_mul(BACKOFF_FACTOR);
        if backoff >= BACKOFF_CAP_MS {
            return BACKOFF_CAP_MS;
        }
    }
    backoff.min(BACKOFF_CAP_MS)
}

/// Fold one signal into its account's record. Pure and idempotent: a repeated signal
/// is a `Duplicate`, and a signal older than the state it would change is `Stale`, so
/// neither a retry nor a late delivery moves the state backwards.
pub fn apply_signal(
    entry: Option<CapacityEntry>,
    signal: &LimitSignal,
) -> (CapacityEntry, Applied) {
    let mut entry =
        entry.unwrap_or_else(|| CapacityEntry::new(&signal.tool, signal.account.as_deref()));
    if entry.recent.contains(signal) {
        return (entry, Applied::Duplicate);
    }
    let window_start = entry.recent.iter().map(|recent| recent.at_ms).min();
    if entry.recent.len() >= MAX_RECENT_SIGNALS
        && window_start.is_some_and(|start| signal.at_ms < start)
    {
        return (entry, Applied::Stale);
    }
    let at = signal.at_ms;
    match signal.kind {
        SignalKind::RateLimit | SignalKind::UsageLimit => {
            // A limit older than a turn that has since completed is already answered.
            if at < entry.changed_at_ms || entry.last_ok_at_ms.is_some_and(|ok| at < ok) {
                return (entry, Applied::Stale);
            }
            match &mut entry.limit {
                // Another report of the limit already in force: one episode, so the
                // backoff does not grow. A stated reset replaces a guessed one.
                Some(limit) if at < limit.until_ms => {
                    if let Some(reset) = signal.resets_at_ms {
                        limit.until_ms = reset;
                        limit.reset_reported = true;
                    }
                }
                previous => {
                    // Limited again after the provider's own reset time, or with no
                    // limit in force: a new sequence. Limited again after a backoff
                    // that was never confirmed over: the sequence continues.
                    let fresh = match previous {
                        Some(limit) => limit.reset_reported,
                        None => !entry.unconfirmed,
                    };
                    entry.consecutive_limits = if fresh {
                        1
                    } else {
                        entry.consecutive_limits.saturating_add(1)
                    };
                    entry.limit = Some(ActiveLimit {
                        kind: signal.kind,
                        since_ms: at,
                        until_ms: signal.resets_at_ms.unwrap_or_else(|| {
                            at.saturating_add(backoff_ms(entry.consecutive_limits))
                        }),
                        reset_reported: signal.resets_at_ms.is_some(),
                    });
                }
            }
            entry.unconfirmed = false;
            entry.changed_at_ms = at;
        }
        SignalKind::Recovered => {
            if at < entry.changed_at_ms {
                return (entry, Applied::Stale);
            }
            entry.limit = None;
            entry.unconfirmed = false;
            entry.consecutive_limits = 0;
            entry.last_ok_at_ms = Some(at);
            entry.changed_at_ms = at;
        }
        SignalKind::Usage => {
            let day = at / DAY_MS;
            match &mut entry.spend {
                Some(spend) if spend.day > day => {}
                Some(spend) if spend.day == day => add_cost(spend, signal.cost_cents),
                spend => {
                    let mut fresh = DaySpend {
                        day,
                        cents: 0,
                        unpriced: 0,
                    };
                    add_cost(&mut fresh, signal.cost_cents);
                    *spend = Some(fresh);
                }
            }
            // A turn that completed before the current state began proves nothing
            // about it; its cost still counts.
            if at >= entry.changed_at_ms {
                match &entry.limit {
                    // The provider named the reset time; a late turn does not outrank it.
                    Some(limit) if at < limit.until_ms && limit.reset_reported => {}
                    Some(limit) if at < limit.until_ms => {
                        if at > limit.since_ms {
                            entry.limit = None;
                            entry.unconfirmed = true;
                            entry.changed_at_ms = at;
                        }
                    }
                    _ => {
                        if entry.limit.is_some() || entry.unconfirmed {
                            entry.changed_at_ms = at;
                        }
                        entry.limit = None;
                        entry.unconfirmed = false;
                        entry.consecutive_limits = 0;
                        entry.last_ok_at_ms = Some(entry.last_ok_at_ms.map_or(at, |ok| ok.max(at)));
                    }
                }
            }
        }
    }
    entry.updated_at_ms = entry.updated_at_ms.max(at);
    if entry.recent.len() >= MAX_RECENT_SIGNALS {
        entry.recent.remove(0);
    }
    entry.recent.push(signal.clone());
    (entry, Applied::Applied)
}

fn add_cost(spend: &mut DaySpend, cost_cents: Option<u64>) {
    match cost_cents {
        Some(cost) => spend.cents = spend.cents.saturating_add(cost),
        None => spend.unpriced = spend.unpriced.saturating_add(1),
    }
}

/// The cost observed for the UTC day of `now_ms`, when every usage signal that day
/// carried one and the plugin reports costs. `None` means the day's cost is unknown.
pub fn known_spend_cents(
    entry: Option<&CapacityEntry>,
    support: SignalSupport,
    now_ms: u64,
) -> Option<u64> {
    if !support.cost {
        return None;
    }
    match entry
        .and_then(|entry| entry.spend.as_ref())
        .filter(|spend| spend.day == now_ms / DAY_MS)
    {
        Some(spend) if spend.unpriced > 0 => None,
        Some(spend) => Some(spend.cents),
        None => Some(0),
    }
}

/// One account's capacity at `now_ms`. Pure; an absent record is an unobserved account.
pub fn state_at(
    entry: Option<&CapacityEntry>,
    support: SignalSupport,
    budget_cents_per_day: Option<u64>,
    now_ms: u64,
) -> CapacityStatus {
    use CapacityReason as R;
    use CapacityState as S;
    let today = now_ms / DAY_MS;
    let limit = entry.and_then(|entry| entry.limit.as_ref());
    let spend = entry
        .and_then(|entry| entry.spend.as_ref())
        .filter(|spend| spend.day == today);
    let mut limited = limit
        .filter(|limit| now_ms < limit.until_ms)
        .map(|limit| match limit.kind {
            SignalKind::RateLimit => (limit.until_ms, R::RateLimit),
            _ => (limit.until_ms, R::UsageLimit),
        });
    // Cost that was observed counts against the budget even when more may be unknown.
    if budget_cents_per_day.is_some_and(|budget| spend.map_or(0, |spend| spend.cents) >= budget) {
        let next_day = (today + 1).saturating_mul(DAY_MS);
        if limited.is_none_or(|(until, _)| next_day > until) {
            limited = Some((next_day, R::BudgetExceeded));
        }
    }
    if let Some((until_ms, reason)) = limited {
        return CapacityStatus::new(S::Limited { until_ms }, reason);
    }
    if !support.limits {
        return CapacityStatus::new(S::Unknown, R::NotReported);
    }
    if limit.is_some_and(|limit| !limit.reset_reported) {
        return CapacityStatus::new(S::Degraded, R::RecoveryUnconfirmed);
    }
    if entry.is_some_and(|entry| entry.unconfirmed) {
        return CapacityStatus::new(S::Degraded, R::InconsistentSignals);
    }
    if budget_cents_per_day.is_some()
        && (!support.cost || spend.is_some_and(|spend| spend.unpriced > 0))
    {
        return CapacityStatus::new(S::Unknown, R::BudgetUnknown);
    }
    if !entry.is_some_and(|entry| entry.last_ok_at_ms.is_some() || entry.limit.is_some()) {
        return CapacityStatus::new(S::Unknown, R::NotObserved);
    }
    CapacityStatus {
        state: S::Available,
        reason: None,
    }
}

/// [`state_at`] for one policy account, with the tool's unattributed record laid over
/// it: a limit seen on an account nobody could identify makes every account of that
/// tool a probe, but never limits one. An account's own record is never changed by
/// another account's signals.
pub fn status_for(
    entries: &[CapacityEntry],
    tool: &str,
    account: Option<&str>,
    support: SignalSupport,
    budget_cents_per_day: Option<u64>,
    now_ms: u64,
) -> CapacityStatus {
    let find = |account: Option<&str>| entries.iter().find(|entry| entry.is_for(tool, account));
    let own = state_at(find(account), support, budget_cents_per_day, now_ms);
    if account.is_some()
        && !matches!(own.state, CapacityState::Limited { .. })
        && matches!(
            state_at(find(None), support, None, now_ms).state,
            CapacityState::Limited { .. }
        )
    {
        return CapacityStatus::new(CapacityState::Degraded, CapacityReason::UnattributedLimit);
    }
    own
}

/// Sessions the caller just observed running under one tool account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionCount {
    pub tool: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    pub sessions: u32,
}

pub struct SelectionRequest<'a> {
    pub policy: &'a PolicyBody,
    pub target_id: &'a str,
    /// The preferred tool; `None` asks for any tool the target allows.
    pub tool: Option<&'a str>,
    /// The model the session would use, checked against the target's model lists.
    pub model: Option<&'a str>,
    pub entries: &'a [CapacityEntry],
    pub load: &'a [SessionCount],
    pub now_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExclusionReason {
    /// The requested tool is not on the target's tool list.
    ToolNotPermitted,
    /// The target lists a tool the policy does not configure.
    ToolNotConfigured,
    ModelNotPermitted,
    Limited,
    ToolCap,
    AccountCap,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Candidate {
    pub tool: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    #[serde(flatten)]
    pub status: CapacityStatus,
    /// False for `degraded` and `unknown`: the scheduler may probe it, not rely on it.
    pub verified: bool,
    pub requested: bool,
    pub weight: u32,
    /// Sessions still allowed by the tool cap and the account cap.
    pub free_sessions: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Excluded {
    pub tool: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    pub excluded: ExclusionReason,
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub status: Option<CapacityStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Selection {
    /// Why the target takes no session at all, as a policy denial code.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub denied: Option<&'static str>,
    /// Usable tool accounts, best first.
    pub candidates: Vec<Candidate>,
    pub excluded: Vec<Excluded>,
}

/// Order the tool accounts a target may start a session on. Verified availability
/// ranks first, then a degraded probe, then an unverified unknown; inside a rank the
/// requested tool, then the policy weight, then free sessions. A limited or full
/// account is excluded with its reason, so a limited tool fails over to the next
/// allowed one. Pure; `support` answers what each tool's plugin can report.
pub fn select(
    request: &SelectionRequest<'_>,
    support: &dyn Fn(&str) -> SignalSupport,
) -> Selection {
    let denied = |denial: PolicyDenial| Selection {
        denied: Some(denial.code()),
        candidates: Vec::new(),
        excluded: Vec::new(),
    };
    let Some(target) = request.policy.target(request.target_id) else {
        return denied(PolicyDenial::TargetNotPermitted);
    };
    if !target.enabled {
        return denied(PolicyDenial::TargetDisabled);
    }
    if !target.actions.contains(&TargetAction::SessionsManage) {
        return denied(PolicyDenial::ActionNotPermitted);
    }
    let mut candidates = Vec::new();
    let mut excluded = Vec::new();
    if let Some(tool) = request.tool {
        if !target.tools.iter().any(|scope| scope.tool == tool) {
            excluded.push(Excluded {
                tool: tool.into(),
                account: None,
                excluded: ExclusionReason::ToolNotPermitted,
                status: None,
            });
        }
    }
    for scope in &target.tools {
        let mut exclude = |account: Option<&str>, reason, status| {
            excluded.push(Excluded {
                tool: scope.tool.clone(),
                account: account.map(Into::into),
                excluded: reason,
                status,
            });
        };
        let Some(tool) = request
            .policy
            .tools
            .iter()
            .find(|tool| tool.tool == scope.tool)
        else {
            exclude(None, ExclusionReason::ToolNotConfigured, None);
            continue;
        };
        if !scope.models.is_empty()
            && request
                .model
                .is_some_and(|model| !scope.models.iter().any(|allowed| allowed == model))
        {
            exclude(None, ExclusionReason::ModelNotPermitted, None);
            continue;
        }
        let sessions = |account: Option<&str>| {
            request
                .load
                .iter()
                .filter(|count| {
                    count.tool == tool.tool
                        && account.is_none_or(|account| count.account.as_deref() == Some(account))
                })
                .fold(0u32, |sum, count| sum.saturating_add(count.sessions))
        };
        let tool_free = tool.max_sessions.saturating_sub(sessions(None));
        // A tool without configured accounts runs as one unnamed account.
        let accounts: Vec<(Option<&str>, u32, Option<u64>)> = if tool.accounts.is_empty() {
            vec![(None, tool_free, None)]
        } else {
            tool.accounts
                .iter()
                .map(|account| {
                    (
                        Some(account.account.as_str()),
                        account
                            .max_sessions
                            .saturating_sub(sessions(Some(&account.account))),
                        account.budget_cents_per_day,
                    )
                })
                .collect()
        };
        for (account, account_free, budget) in accounts {
            let status = status_for(
                request.entries,
                &tool.tool,
                account,
                support(&tool.tool),
                budget,
                request.now_ms,
            );
            if tool_free == 0 {
                exclude(account, ExclusionReason::ToolCap, Some(status));
            } else if account_free == 0 {
                exclude(account, ExclusionReason::AccountCap, Some(status));
            } else if matches!(status.state, CapacityState::Limited { .. }) {
                exclude(account, ExclusionReason::Limited, Some(status));
            } else {
                candidates.push(Candidate {
                    tool: tool.tool.clone(),
                    account: account.map(Into::into),
                    status,
                    verified: status.verified(),
                    requested: request.tool == Some(tool.tool.as_str()),
                    weight: tool.weight,
                    free_sessions: tool_free.min(account_free),
                });
            }
        }
    }
    candidates.sort_by(|a, b| {
        let rank = |candidate: &Candidate| match candidate.status.state {
            CapacityState::Available => 0,
            CapacityState::Degraded => 1,
            _ => 2,
        };
        rank(a)
            .cmp(&rank(b))
            .then(b.requested.cmp(&a.requested))
            .then(b.weight.cmp(&a.weight))
            .then(b.free_sessions.cmp(&a.free_sessions))
            .then_with(|| a.tool.cmp(&b.tool))
            .then_with(|| a.account.cmp(&b.account))
    });
    Selection {
        denied: None,
        candidates,
        excluded,
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CapacityFile {
    schema_version: u32,
    entries: Vec<CapacityEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SignalOutcome {
    pub applied: Applied,
    pub entry: CapacityEntry,
}

/// This machine's capacity records, one per `(tool, account)`.
#[derive(Clone)]
pub struct ToolCapacityRegistry {
    storage: Storage,
}

impl ToolCapacityRegistry {
    pub fn new(home: &Path) -> Self {
        Self {
            storage: Storage::machine_named(home, "orchestration-tool-capacity.json"),
        }
    }

    fn read_file(&self) -> Result<CapacityFile, OrchestrationError> {
        let Some(bytes) = self.storage.read(MAX_CAPACITY_BYTES)? else {
            return Ok(CapacityFile {
                schema_version: ORCHESTRATION_VERSION,
                entries: Vec::new(),
            });
        };
        let file: CapacityFile = decode(&bytes)?;
        if file.entries.len() > MAX_CAPACITY_ENTRIES {
            return Err(OrchestrationError::Corrupt);
        }
        for (index, entry) in file.entries.iter().enumerate() {
            validate(entry).map_err(|_| OrchestrationError::Corrupt)?;
            if file.entries[..index]
                .iter()
                .any(|other| other.is_for(&entry.tool, entry.account.as_deref()))
            {
                return Err(OrchestrationError::Corrupt);
            }
        }
        Ok(file)
    }

    /// Every record. An absent file reads as empty and is never created by a read.
    pub fn entries(&self) -> Result<Vec<CapacityEntry>, OrchestrationError> {
        Ok(self.read_file()?.entries)
    }

    /// Record one signal. A duplicate or stale signal writes nothing. Corrupt or
    /// newer-version data is an error and is left untouched.
    pub fn record(
        &self,
        signal: &LimitSignal,
        now_ms: u64,
    ) -> Result<SignalOutcome, OrchestrationError> {
        signal.validate(now_ms)?;
        let _lock = self.storage.lock()?;
        let mut file = self.read_file()?;
        let index = file
            .entries
            .iter()
            .position(|entry| entry.is_for(&signal.tool, signal.account.as_deref()));
        let (entry, applied) = apply_signal(index.map(|index| file.entries[index].clone()), signal);
        if applied != Applied::Applied {
            return Ok(SignalOutcome { applied, entry });
        }
        match index {
            Some(index) => file.entries[index] = entry.clone(),
            None => {
                if file.entries.len() >= MAX_CAPACITY_ENTRIES {
                    evict(&mut file, now_ms)?;
                }
                file.entries.push(entry.clone());
            }
        }
        let bytes = serde_json::to_vec(&file).map_err(|_| OrchestrationError::Corrupt)?;
        self.storage.write(&bytes, MAX_CAPACITY_BYTES)?;
        Ok(SignalOutcome { applied, entry })
    }
}

/// At capacity, forget the longest-quiet record that holds nothing still in force: no
/// limit, no unconfirmed recovery and no spend for today. Otherwise refuse.
fn evict(file: &mut CapacityFile, now_ms: u64) -> Result<(), OrchestrationError> {
    let index = file
        .entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| {
            entry.limit.is_none()
                && !entry.unconfirmed
                && entry
                    .spend
                    .as_ref()
                    .is_none_or(|spend| spend.day < now_ms / DAY_MS)
        })
        .min_by_key(|(_, entry)| entry.updated_at_ms)
        .map(|(index, _)| index)
        .ok_or(OrchestrationError::Capacity)?;
    file.entries.remove(index);
    Ok(())
}

fn validate(entry: &CapacityEntry) -> Result<(), OrchestrationError> {
    valid_identifier(&entry.tool)?;
    if let Some(account) = &entry.account {
        valid_identifier(account)?;
    }
    if entry.recent.len() > MAX_RECENT_SIGNALS
        || entry.limit.as_ref().is_some_and(|limit| {
            !limit.kind.is_limit()
                || limit.until_ms.saturating_add(MAX_SIGNAL_SKEW_MS) < limit.since_ms
        })
        || entry
            .recent
            .iter()
            .any(|signal| !entry.is_for(&signal.tool, signal.account.as_deref()))
    {
        return Err(OrchestrationError::Corrupt);
    }
    Ok(())
}
