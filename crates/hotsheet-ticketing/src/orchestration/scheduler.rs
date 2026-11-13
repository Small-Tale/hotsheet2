//! HS2-ECWG50: the throughput scheduler's scoring and packing core.
//!
//! [`plan`] decides, from typed inputs only, which sessions to start, keep or wind
//! down per target, tool and model. It is pure: the caller supplies the ready work,
//! the policy, each target's capacity snapshot, the running sessions, the tool
//! capacity records, the held resources, what earlier passes remember, and the time.
//! The same inputs give the same schedule in any order.
//!
//! Tool choice is [`select`] (§26.11) and the machine check is [`decide_admission`]
//! (§26.10), so a reason here is the reason those answer. Nothing is scheduled that
//! the caller did not list as confirmed work.
//!
//! Integer units only: scores are points, times are milliseconds.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::audit::{Journal, JournalEntry, JournalPage, JournalRecord};
use super::contract::{bounded_text, valid_identifier, valid_token};
use super::storage::{Storage, decode};
use super::{
    AdmissionConfig, AdmissionOutcome, AdmissionReason, AdmissionRequest, CapacityEntry,
    CapacitySnapshot, CapacityState, MAX_TARGETS, ORCHESTRATION_VERSION, OrchestrationError,
    PolicyBody, PolicyDenial, SelectionRequest, SessionCount, SignalSupport, TargetAction,
    decide_admission, select,
};

/// One priority band, in points. Every other term is expressed against it.
pub const SCORE_BAND: u64 = 1000;
pub const SCORE_UP_NEXT: u64 = 1500;
pub const SCORE_OVERDUE: u64 = 2000;
pub const SCORE_DUE_SOON: u64 = 1000;
pub const SCORE_DUE_THIS_WEEK: u64 = 300;
pub const SCORE_PER_UNBLOCKED: u64 = 200;
pub const MAX_SCORED_UNBLOCKS: u32 = 5;
/// The repair of a red gate that other work stands on, plus a share per stacked step.
pub const SCORE_RED_GATE: u64 = 2000;
pub const SCORE_PER_STACKED: u64 = 250;
pub const MAX_SCORED_STACK: u32 = 8;
const DAY_MS: u64 = 24 * 60 * 60 * 1000;

/// A refused start is not asked again for this long, doubling per refusal to the cap.
pub const REFUSAL_BACKOFF_BASE_MS: u64 = 60 * 1000;
pub const REFUSAL_BACKOFF_CAP_MS: u64 = 30 * 60 * 1000;
/// Work items and sessions one pass considers.
pub const MAX_SCHEDULE_WORK: usize = 256;
pub const MAX_SCHEDULE_SESSIONS: usize = 1024;
const MAX_MEMORY_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkLevel {
    Confirmed,
    /// Rests on a pending gate. Ranked with the policy's risk weight and never placed
    /// here (HS2-JSF985).
    Provisional,
}

/// A resource one piece of work needs exclusively (declared by HS2-2FG91X).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceNeed {
    pub namespace: String,
    pub resource: String,
}

/// A resource someone holds now. A held resource is unavailable capacity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeldResource {
    pub namespace: String,
    pub resource: String,
    pub expires_at_ms: u64,
}

/// One piece of ready work as the scheduler ranks it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkItem {
    pub step_id: String,
    pub target_id: String,
    pub connection_id: String,
    pub native_id: String,
    pub level: WorkLevel,
    /// The target ticket's own priority name; an unknown or absent one is `default`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
    #[serde(default)]
    pub up_next: bool,
    /// A due date or milestone date, when the ticket has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due_at_ms: Option<u64>,
    /// Unfinished steps waiting on this one.
    #[serde(default)]
    pub unblocks: u32,
    /// Steps blocked by a failed gate of this work's ticket: it is that gate's repair.
    #[serde(default)]
    pub red_gate_stack: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ready_since_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at_ms: Option<u64>,
    /// A tool or model the work asks for; any allowed one when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<ResourceNeed>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionActivity {
    /// Working its ticket.
    Busy,
    /// Its ticket waits on CI, a deployment or a review: free capacity, not busy.
    WaitingOnGate,
    Idle,
}

/// Everything the scheduler knows about one running session. This is the scheduler's
/// own view; whoever starts sessions maps its records onto it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScheduledSession {
    pub session_id: String,
    pub target_id: String,
    pub tool: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The run the session works. A session without a run has no work the
    /// scheduler knows of, so it is free capacity whatever `activity` says: a session
    /// that was just started for ready work takes that work instead of a second
    /// session starting for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    /// The owning checkout whose orchestrator started the session. A session of
    /// another owner, or one a person started, counts as load and is never managed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_id: Option<String>,
    pub activity: SessionActivity,
    pub started_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle_since_ms: Option<u64>,
    /// No claim, pending handoff or uncommitted work: it may be wound down now.
    #[serde(default)]
    pub safe_point: bool,
    /// A person took the session over. It is left alone.
    #[serde(default)]
    pub human_takeover: bool,
    /// A wind-down was already asked for.
    #[serde(default)]
    pub winding_down: bool,
}

impl ScheduledSession {
    pub fn validate(&self) -> Result<(), OrchestrationError> {
        valid_identifier(&self.session_id)?;
        valid_token(&self.target_id)?;
        valid_identifier(&self.tool)?;
        for value in [&self.account, &self.run_id, &self.owner_id]
            .into_iter()
            .flatten()
        {
            valid_identifier(value)?;
        }
        if self
            .model
            .as_ref()
            .is_some_and(|model| !bounded_text(model, 128))
        {
            return Err(OrchestrationError::Invalid("bounded model name required"));
        }
        Ok(())
    }
}

/// What the machine hosting one target reported. Without a snapshot nothing starts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetState {
    pub target_id: String,
    pub capacity: Option<CapacitySnapshot>,
    /// Why there is no snapshot, as a stable code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unavailable: Option<String>,
}

/// What earlier passes did on one target: the oscillation guard and refusal backoff
/// read it. Machine-local.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlacementMemory {
    pub target_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_started_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_wound_down_at_ms: Option<u64>,
    /// Refusals since the last successful start.
    #[serde(default)]
    pub refusals: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refused_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refusal_reason: Option<String>,
}

impl PlacementMemory {
    fn is_empty(&self) -> bool {
        self.last_started_at_ms.is_none()
            && self.last_wound_down_at_ms.is_none()
            && self.refusals == 0
    }

    /// When a refused target may be asked again.
    pub fn refused_until_ms(&self) -> Option<u64> {
        (self.refusals > 0).then(|| {
            self.refused_at_ms
                .unwrap_or(0)
                .saturating_add(refusal_backoff_ms(self.refusals))
        })
    }
}

/// How long a target is left alone after its `refusals`-th refusal in a row.
pub fn refusal_backoff_ms(refusals: u32) -> u64 {
    if refusals == 0 {
        return 0;
    }
    REFUSAL_BACKOFF_BASE_MS
        .checked_shl(refusals - 1)
        .filter(|backoff| backoff >> (refusals - 1) == REFUSAL_BACKOFF_BASE_MS)
        .map_or(REFUSAL_BACKOFF_CAP_MS, |backoff| {
            backoff.min(REFUSAL_BACKOFF_CAP_MS)
        })
}

/// What applying a schedule did on one target, for [`remember`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlacementOutcome {
    Started,
    /// The target, its machine or the executor refused a start, with a stable code.
    Refused(String),
    WoundDown,
}

/// Fold what one pass did into the memory. Pure. A start clears the target's
/// refusals; a refusal counts once per target and pass; an empty record is dropped.
pub fn remember(
    memory: &[PlacementMemory],
    applied: &[(String, PlacementOutcome)],
    now_ms: u64,
) -> Vec<PlacementMemory> {
    let mut next: BTreeMap<String, PlacementMemory> = memory
        .iter()
        .map(|entry| (entry.target_id.clone(), entry.clone()))
        .collect();
    let mut refused = BTreeSet::new();
    for (target_id, outcome) in applied {
        let entry = next
            .entry(target_id.clone())
            .or_insert_with(|| PlacementMemory {
                target_id: target_id.clone(),
                ..PlacementMemory::default()
            });
        match outcome {
            PlacementOutcome::Started => {
                entry.last_started_at_ms = Some(now_ms);
                entry.refusals = 0;
                entry.refused_at_ms = None;
                entry.refusal_reason = None;
            }
            PlacementOutcome::WoundDown => entry.last_wound_down_at_ms = Some(now_ms),
            PlacementOutcome::Refused(reason) => {
                if refused.insert(target_id.clone()) {
                    entry.refusals = entry.refusals.saturating_add(1).min(32);
                    entry.refused_at_ms = Some(now_ms);
                    entry.refusal_reason = Some(reason.clone());
                }
            }
        }
    }
    // A start in the same pass outranks a refusal of another request there.
    for (target_id, outcome) in applied {
        if *outcome == PlacementOutcome::Started
            && let Some(entry) = next.get_mut(target_id)
        {
            entry.refusals = 0;
            entry.refused_at_ms = None;
            entry.refusal_reason = None;
        }
    }
    next.into_values()
        .filter(|entry| !entry.is_empty())
        .take(MAX_TARGETS)
        .collect()
}

/// Why work ranks where it does. Every term is kept so a decision can be read back.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScoreParts {
    pub priority: u64,
    pub up_next: u64,
    pub due: u64,
    pub unblocks: u64,
    pub red_gate: u64,
    pub aging: u64,
    /// The share taken off provisional work; 0 for confirmed work.
    pub risk_percent: u32,
    pub total: u64,
}

fn priority_band(priority: Option<&str>) -> u64 {
    match priority {
        Some("highest") => 4,
        Some("high") => 3,
        Some("low") => 1,
        Some("lowest") => 0,
        _ => 2,
    }
}

/// Score one piece of work. Priority sets the band; urgency (Up Next, a due date,
/// what it unblocks, a red gate it repairs) adds to it; waiting adds one band per
/// `aging_minutes` without a ceiling, so old low-priority work eventually outranks
/// anything that keeps arriving. Provisional work loses the policy's risk share.
pub fn score(item: &WorkItem, policy: &PolicyBody, now_ms: u64) -> ScoreParts {
    let due = match item.due_at_ms {
        Some(due) if due <= now_ms => SCORE_OVERDUE,
        Some(due) if due - now_ms <= DAY_MS => SCORE_DUE_SOON,
        Some(due) if due - now_ms <= 7 * DAY_MS => SCORE_DUE_THIS_WEEK,
        _ => 0,
    };
    let red_gate = if item.red_gate_stack == 0 {
        0
    } else {
        SCORE_RED_GATE + SCORE_PER_STACKED * u64::from(item.red_gate_stack.min(MAX_SCORED_STACK))
    };
    let waited_ms = item
        .ready_since_ms
        .or(item.created_at_ms)
        .map_or(0, |since| now_ms.saturating_sub(since));
    let aging_ms = u64::from(policy.placement.aging_minutes.max(1)) * 60 * 1000;
    let mut parts = ScoreParts {
        priority: priority_band(item.priority.as_deref()) * SCORE_BAND,
        up_next: if item.up_next { SCORE_UP_NEXT } else { 0 },
        due,
        unblocks: SCORE_PER_UNBLOCKED * u64::from(item.unblocks.min(MAX_SCORED_UNBLOCKS)),
        red_gate,
        aging: u64::try_from(u128::from(waited_ms) * u128::from(SCORE_BAND) / u128::from(aging_ms))
            .unwrap_or(u64::MAX),
        risk_percent: 0,
        total: 0,
    };
    let sum = [
        parts.priority,
        parts.up_next,
        parts.due,
        parts.unblocks,
        parts.red_gate,
        parts.aging,
    ]
    .into_iter()
    .fold(0u64, u64::saturating_add);
    parts.total = match item.level {
        WorkLevel::Confirmed => sum,
        WorkLevel::Provisional => {
            parts.risk_percent = policy.speculation.risk_weight.min(100);
            u64::try_from(u128::from(sum) * u128::from(100 - parts.risk_percent) / 100)
                .unwrap_or(u64::MAX)
        }
    };
    parts
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlacementAction {
    Start,
    Keep,
    WindDown,
}

impl PlacementAction {
    pub fn code(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Keep => "keep",
            Self::WindDown => "wind_down",
        }
    }
}

/// Why a session is started, kept or wound down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlacementReason {
    /// Start: ready work no running session can take.
    ReadyWork,
    /// Keep: a free session takes ready work instead of a new one starting.
    Refill,
    Busy,
    /// Keep: its ticket waits on a gate and nothing else is ready for it.
    WaitingOnGate,
    /// Keep: one of the idle sessions the policy keeps per target.
    KeepWarm,
    /// Keep: idle for less than the policy's idle time.
    IdleGrace,
    /// Keep: the target started a session too recently to wind one down.
    Cooldown,
    /// Keep: it should stop, and will at its next safe point.
    AwaitingSafePoint,
    HumanTakeover,
    NotOwned,
    /// Keep: a wind-down was already asked for.
    WindingDown,
    /// Wind down: idle past the policy's idle time.
    Idle,
    /// Wind down: the policy no longer permits this session.
    PolicyNotPermitted,
    /// Wind down: the target runs more sessions than its cap now allows.
    OverCap,
}

impl PlacementReason {
    pub fn code(self) -> &'static str {
        match self {
            Self::ReadyWork => "ready_work",
            Self::Refill => "refill",
            Self::Busy => "busy",
            Self::WaitingOnGate => "waiting_on_gate",
            Self::KeepWarm => "keep_warm",
            Self::IdleGrace => "idle_grace",
            Self::Cooldown => "cooldown",
            Self::AwaitingSafePoint => "awaiting_safe_point",
            Self::HumanTakeover => "human_takeover",
            Self::NotOwned => "not_owned",
            Self::WindingDown => "winding_down",
            Self::Idle => "idle",
            Self::PolicyNotPermitted => "policy_not_permitted",
            Self::OverCap => "over_cap",
        }
    }
}

/// The figures a placement was decided on.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlacementInputs {
    /// Sessions on the target, counting the ones this pass already placed.
    pub target_sessions: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_cap: Option<u32>,
    /// Sessions on the target's machine, counting the ones this pass placed there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine_sessions: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine_cap: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub free_ram_mb: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub footprint_mb: Option<u64>,
    /// The chosen tool account's capacity state code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_free: Option<u32>,
    /// Sessions of the chosen tool, and of its account, before this start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_sessions: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_sessions: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Placement {
    pub action: PlacementAction,
    pub target_id: String,
    pub tool: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The session kept or wound down; absent for a start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub reason: PlacementReason,
    /// A policy denial, a retry time's cause or another stable code behind the reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// The ready work this session is for.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<ScoreParts>,
    /// The tool account is not verified available: this start is how it is found out.
    #[serde(default)]
    pub probe: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_at_ms: Option<u64>,
    #[serde(default)]
    pub inputs: PlacementInputs,
}

/// Why ready work gets no session in this pass. The wire spelling equals `code()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Blocker {
    /// Provisional work is ranked and never placed here.
    #[serde(rename = "provisional_not_dispatched")]
    Provisional,
    /// The policy does not let a session run on the target; `detail` is its code.
    PolicyDenied,
    /// The target's machine could not be read; `detail` says why.
    TargetUnavailable,
    TargetCap,
    /// No allowed tool account can take a session; `detail` is the nearest cause.
    #[serde(rename = "no_tool_available")]
    NoTool,
    /// The target's machine is at a cap or below a floor; `detail` is the limit.
    MachineRefused,
    /// A resource the work needs is held; `detail` names it.
    ResourceHeld,
    /// The target wound a session down too recently to start one.
    Cooldown,
    /// The target refused a start and is not asked again yet.
    RefusalBackoff,
}

impl Blocker {
    pub fn code(self) -> &'static str {
        match self {
            Self::Provisional => "provisional_not_dispatched",
            Self::PolicyDenied => "policy_denied",
            Self::TargetUnavailable => "target_unavailable",
            Self::TargetCap => "target_cap",
            Self::NoTool => "no_tool_available",
            Self::MachineRefused => "machine_refused",
            Self::ResourceHeld => "resource_held",
            Self::Cooldown => "cooldown",
            Self::RefusalBackoff => "refusal_backoff",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Unplaced {
    pub step_id: String,
    pub target_id: String,
    pub reason: Blocker,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// When the reason is known to end; the scheduler looks again then.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_at_ms: Option<u64>,
    pub score: ScoreParts,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ranked {
    pub step_id: String,
    pub score: ScoreParts,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Schedule {
    /// Starts in rank order, then wind-downs, then keeps, each by target and session.
    pub placements: Vec<Placement>,
    pub unplaced: Vec<Unplaced>,
    /// Every piece of work considered, best first.
    pub ranking: Vec<Ranked>,
    /// The earliest time a decision here changes by time alone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_at_ms: Option<u64>,
}

/// Where one piece of work stands in a schedule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Standing {
    pub step_id: String,
    /// `start`, `keep` (a free session takes it) or `not_placed`.
    pub standing: &'static str,
    pub reason: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry_at_ms: Option<u64>,
    /// 1 is the best-ranked work of the pass.
    pub rank: usize,
    pub score: ScoreParts,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placement: Option<Placement>,
}

impl Schedule {
    /// Where a step stands, or `None` when the pass did not consider it.
    pub fn standing(&self, step_id: &str) -> Option<Standing> {
        let rank = self
            .ranking
            .iter()
            .position(|ranked| ranked.step_id == step_id)?;
        let score = self.ranking[rank].score;
        let placed = self
            .placements
            .iter()
            .find(|placement| placement.steps.iter().any(|step| step == step_id));
        let unplaced = self.unplaced.iter().find(|item| item.step_id == step_id);
        Some(match (placed, unplaced) {
            (Some(placement), _) => Standing {
                step_id: step_id.into(),
                standing: placement.action.code(),
                reason: placement.reason.code(),
                detail: placement.detail.clone(),
                retry_at_ms: None,
                rank: rank + 1,
                score,
                placement: Some(placement.clone()),
            },
            (None, Some(item)) => Standing {
                step_id: step_id.into(),
                standing: "not_placed",
                reason: item.reason.code(),
                detail: item.detail.clone(),
                retry_at_ms: item.retry_at_ms,
                rank: rank + 1,
                score,
                placement: None,
            },
            (None, None) => return None,
        })
    }
}

pub struct ScheduleInput<'a> {
    pub now_ms: u64,
    /// The owning checkout whose sessions this pass manages.
    pub owner_id: &'a str,
    pub policy: &'a PolicyBody,
    pub work: &'a [WorkItem],
    pub targets: &'a [TargetState],
    pub sessions: &'a [ScheduledSession],
    pub tool_entries: &'a [CapacityEntry],
    pub held: &'a [HeldResource],
    pub memory: &'a [PlacementMemory],
}

/// The policy rule a running session breaks, if any.
fn session_denial(policy: &PolicyBody, session: &ScheduledSession) -> Option<PolicyDenial> {
    let Some(target) = policy.target(&session.target_id) else {
        return Some(PolicyDenial::TargetNotPermitted);
    };
    if !target.enabled {
        return Some(PolicyDenial::TargetDisabled);
    }
    if !target.actions.contains(&TargetAction::SessionsManage) {
        return Some(PolicyDenial::ActionNotPermitted);
    }
    let Some(scope) = target.tools.iter().find(|scope| scope.tool == session.tool) else {
        return Some(PolicyDenial::ToolNotPermitted);
    };
    if !scope.models.is_empty()
        && !session
            .model
            .as_ref()
            .is_some_and(|model| scope.models.contains(model))
    {
        return Some(PolicyDenial::ModelNotPermitted);
    }
    None
}

enum Fate {
    Free,
    Decided(
        PlacementAction,
        PlacementReason,
        Option<String>,
        Option<u64>,
    ),
    Refilled(String, ScoreParts),
}

fn earliest(retry: &mut Option<u64>, at_ms: u64, now_ms: u64) {
    if at_ms > now_ms {
        *retry = Some(retry.map_or(at_ms, |known| known.min(at_ms)));
    }
}

/// Decide every placement for one owner. See the module documentation.
pub fn plan(input: &ScheduleInput<'_>, support: &dyn Fn(&str) -> SignalSupport) -> Schedule {
    use PlacementAction as A;
    use PlacementReason as R;
    let now = input.now_ms;
    let policy = input.policy;
    let cooldown_ms = u64::from(policy.placement.cooldown_minutes) * 60 * 1000;
    let idle_ms = u64::from(policy.wind_down.idle_minutes) * 60 * 1000;
    let memory = |target: &str| input.memory.iter().find(|entry| entry.target_id == target);
    let mut retry_at = None;

    let mut sessions: Vec<&ScheduledSession> = input.sessions.iter().collect();
    sessions.sort_by(|a, b| (&a.target_id, &a.session_id).cmp(&(&b.target_id, &b.session_id)));
    sessions.dedup_by(|a, b| a.session_id == b.session_id && a.target_id == b.target_id);

    // ---- What each running session is, before any work is considered ----
    let mut fates: Vec<Fate> = sessions
        .iter()
        .map(|session| {
            if session.owner_id.as_deref() != Some(input.owner_id) {
                return Fate::Decided(A::Keep, R::NotOwned, None, None);
            }
            if session.human_takeover {
                return Fate::Decided(A::Keep, R::HumanTakeover, None, None);
            }
            if session.winding_down {
                return Fate::Decided(A::Keep, R::WindingDown, None, None);
            }
            if let Some(denial) = session_denial(policy, session) {
                let detail = Some(denial.code().to_owned());
                return if session.safe_point {
                    Fate::Decided(A::WindDown, R::PolicyNotPermitted, detail, None)
                } else {
                    Fate::Decided(A::Keep, R::AwaitingSafePoint, detail, None)
                };
            }
            match session.activity {
                SessionActivity::Busy if session.run_id.is_some() => {
                    Fate::Decided(A::Keep, R::Busy, None, None)
                }
                _ => Fate::Free,
            }
        })
        .collect();

    // ---- A cap lowered below what runs: the freest sessions leave first ----
    let mut on_target: BTreeMap<&str, u32> = BTreeMap::new();
    for session in sessions.iter().copied() {
        *on_target.entry(session.target_id.as_str()).or_default() += 1;
    }
    for target in &policy.targets {
        let staying = |fates: &[Fate]| {
            sessions
                .iter()
                .zip(fates)
                .filter(|(session, fate)| {
                    session.target_id == target.target_id
                        && !session.winding_down
                        && !matches!(fate, Fate::Decided(A::WindDown, ..))
                })
                .count()
        };
        while staying(&fates) > target.max_sessions as usize {
            let pick = sessions
                .iter()
                .enumerate()
                .filter(|(index, session)| {
                    session.target_id == target.target_id
                        && session.safe_point
                        && matches!(fates[*index], Fate::Free)
                })
                .min_by_key(|(_, session)| {
                    (
                        std::cmp::Reverse(session.activity),
                        session.idle_since_ms.unwrap_or(u64::MAX),
                        &session.session_id,
                    )
                })
                .map(|(index, _)| index);
            let Some(index) = pick else {
                break;
            };
            fates[index] = Fate::Decided(A::WindDown, R::OverCap, None, None);
        }
    }

    // ---- Rank the work ----
    let mut ranked: Vec<(ScoreParts, &WorkItem)> = input
        .work
        .iter()
        .take(MAX_SCHEDULE_WORK)
        .map(|item| (score(item, policy, now), item))
        .collect();
    ranked.sort_by(|(a, x), (b, y)| {
        b.total
            .cmp(&a.total)
            .then(
                x.ready_since_ms
                    .unwrap_or(u64::MAX)
                    .cmp(&y.ready_since_ms.unwrap_or(u64::MAX)),
            )
            .then_with(|| x.step_id.cmp(&y.step_id))
    });
    ranked.dedup_by(|(_, a), (_, b)| a.step_id == b.step_id);

    // ---- Place it: a free session first, a new one only when every limit allows ----
    let mut load: Vec<SessionCount> = Vec::new();
    let count = |load: &mut Vec<SessionCount>, tool: &str, account: Option<&str>| match load
        .iter_mut()
        .find(|entry| entry.tool == tool && entry.account.as_deref() == account)
    {
        Some(entry) => entry.sessions = entry.sessions.saturating_add(1),
        None => load.push(SessionCount {
            tool: tool.into(),
            account: account.map(Into::into),
            sessions: 1,
        }),
    };
    for session in &sessions {
        count(&mut load, &session.tool, session.account.as_deref());
    }
    let running = |load: &[SessionCount], tool: &str, account: Option<&str>| {
        load.iter()
            .filter(|entry| {
                entry.tool == tool && account.is_none_or(|a| entry.account.as_deref() == Some(a))
            })
            .fold(0u32, |sum, entry| sum.saturating_add(entry.sessions))
    };
    let mut planned_target: BTreeMap<&str, u32> = BTreeMap::new();
    let mut planned_machine: BTreeMap<&str, u32> = BTreeMap::new();
    let mut allocated: BTreeSet<&ResourceNeed> = BTreeSet::new();
    let mut starts = Vec::new();
    let mut unplaced = Vec::new();

    for (parts, item) in ranked.iter().map(|(parts, item)| (parts, *item)) {
        let mut skip = |reason: Blocker, detail: Option<String>, retry: Option<u64>| {
            unplaced.push(Unplaced {
                step_id: item.step_id.clone(),
                target_id: item.target_id.clone(),
                reason,
                detail,
                retry_at_ms: retry.filter(|at| *at > now),
                score: *parts,
            });
        };
        if item.level == WorkLevel::Provisional {
            skip(Blocker::Provisional, None, None);
            continue;
        }
        let held = item.resources.iter().find_map(|need| {
            let holder = input
                .held
                .iter()
                .filter(|held| {
                    held.namespace == need.namespace
                        && held.resource == need.resource
                        && held.expires_at_ms > now
                })
                .map(|held| held.expires_at_ms)
                .max();
            match holder {
                Some(expires) => Some((need, Some(expires))),
                None if allocated.contains(need) => Some((need, None)),
                None => None,
            }
        });
        if let Some((need, expires)) = held {
            if let Some(at) = expires {
                earliest(&mut retry_at, at, now);
            }
            skip(
                Blocker::ResourceHeld,
                Some(format!("{}/{}", need.namespace, need.resource)),
                expires,
            );
            continue;
        }
        // A free session of the target takes the work: idle ones before those that
        // wait on a gate, which may be called back to their own ticket.
        let free = sessions
            .iter()
            .enumerate()
            .filter(|(index, session)| {
                matches!(fates[*index], Fate::Free)
                    && session.target_id == item.target_id
                    && item.tool.as_ref().is_none_or(|tool| tool == &session.tool)
                    && item
                        .model
                        .as_ref()
                        .is_none_or(|model| session.model.as_ref() == Some(model))
            })
            .min_by_key(|(_, session)| (std::cmp::Reverse(session.activity), &session.session_id))
            .map(|(index, _)| index);
        if let Some(index) = free {
            fates[index] = Fate::Refilled(item.step_id.clone(), *parts);
            allocated.extend(item.resources.iter());
            continue;
        }
        let selection = select(
            &SelectionRequest {
                policy,
                target_id: &item.target_id,
                tool: item.tool.as_deref(),
                model: item.model.as_deref(),
                entries: input.tool_entries,
                load: &load,
                now_ms: now,
            },
            support,
        );
        if let Some(denied) = selection.denied {
            skip(Blocker::PolicyDenied, Some(denied.into()), None);
            continue;
        }
        let Some(target) = policy.target(&item.target_id) else {
            skip(
                Blocker::PolicyDenied,
                Some(PolicyDenial::TargetNotPermitted.code().into()),
                None,
            );
            continue;
        };
        let target_sessions = on_target.get(item.target_id.as_str()).copied().unwrap_or(0)
            + planned_target
                .get(item.target_id.as_str())
                .copied()
                .unwrap_or(0);
        if target_sessions >= target.max_sessions {
            skip(Blocker::TargetCap, None, None);
            continue;
        }
        // What the policy and the target's cap say stands until a person changes it;
        // a refusal or a cooldown only postpones, so those are named after.
        let remembered = memory(&item.target_id);
        if let Some(until) = remembered
            .and_then(PlacementMemory::refused_until_ms)
            .filter(|until| *until > now)
        {
            earliest(&mut retry_at, until, now);
            skip(
                Blocker::RefusalBackoff,
                remembered.and_then(|entry| entry.refusal_reason.clone()),
                Some(until),
            );
            continue;
        }
        if let Some(until) = remembered
            .and_then(|entry| entry.last_wound_down_at_ms)
            .map(|at| at.saturating_add(cooldown_ms))
            .filter(|until| *until > now)
        {
            earliest(&mut retry_at, until, now);
            skip(Blocker::Cooldown, None, Some(until));
            continue;
        }
        let state = input
            .targets
            .iter()
            .find(|state| state.target_id == item.target_id);
        let Some(snapshot) = state.and_then(|state| state.capacity.as_ref()) else {
            skip(
                Blocker::TargetUnavailable,
                Some(
                    state
                        .and_then(|state| state.unavailable.clone())
                        .unwrap_or_else(|| "capacity_not_read".into()),
                ),
                None,
            );
            continue;
        };
        let limits = policy.machine_limits(&item.target_id);
        let on_machine = planned_machine
            .get(snapshot.server_id.as_str())
            .copied()
            .unwrap_or(0);
        let mut nearest: Option<(Blocker, String, Option<u64>)> = None;
        let mut placed = None;
        for candidate in &selection.candidates {
            let account = candidate.account.as_deref();
            // A degraded account is tried by one session at a time.
            if candidate.status.state == CapacityState::Degraded
                && running(&load, &candidate.tool, account) > 0
            {
                nearest.get_or_insert((Blocker::NoTool, "probe_in_flight".into(), None));
                continue;
            }
            let model = item.model.clone().or_else(|| {
                target
                    .tools
                    .iter()
                    .find(|scope| scope.tool == candidate.tool)
                    .and_then(|scope| scope.models.first().cloned())
            });
            let admission = decide_admission(
                snapshot,
                &AdmissionConfig::default(),
                &AdmissionRequest {
                    tool: candidate.tool.clone(),
                    model: model.clone(),
                    sessions: on_machine + 1,
                    limits: Some(limits),
                },
            );
            if admission.outcome == AdmissionOutcome::Accepted {
                placed = Some((candidate, model, admission));
                break;
            }
            let reason = admission.reason.unwrap_or(AdmissionReason::CapacityUnknown);
            if matches!(
                reason,
                AdmissionReason::ToolNotInstalled | AdmissionReason::ToolNotConfigured
            ) {
                // Another allowed tool may be present on that machine.
                nearest = Some((Blocker::NoTool, reason.code().into(), None));
                continue;
            }
            nearest = Some((Blocker::MachineRefused, reason.code().into(), None));
            break;
        }
        let Some((candidate, model, admission)) = placed else {
            let (reason, detail, retry) = nearest.unwrap_or_else(|| {
                // Nothing was even a candidate: name the exclusion that ends soonest.
                let limited = selection
                    .excluded
                    .iter()
                    .filter_map(
                        |excluded| match excluded.status.map(|status| status.state) {
                            Some(CapacityState::Limited { until_ms }) => Some(until_ms),
                            _ => None,
                        },
                    )
                    .min();
                let detail = match (limited, selection.excluded.first()) {
                    (Some(_), _) => "limited".to_owned(),
                    (None, Some(first)) => serde_json::to_value(first.excluded)
                        .ok()
                        .and_then(|value| value.as_str().map(str::to_owned))
                        .unwrap_or_else(|| "excluded".into()),
                    (None, None) => "no_tool_permitted".into(),
                };
                (Blocker::NoTool, detail, limited)
            });
            if let Some(at) = retry {
                earliest(&mut retry_at, at, now);
            }
            skip(reason, Some(detail), retry);
            continue;
        };
        let account = candidate.account.as_deref();
        starts.push(Placement {
            action: A::Start,
            target_id: item.target_id.clone(),
            tool: candidate.tool.clone(),
            account: candidate.account.clone(),
            model,
            session_id: None,
            reason: R::ReadyWork,
            detail: None,
            steps: vec![item.step_id.clone()],
            score: Some(*parts),
            probe: !candidate.verified,
            retry_at_ms: None,
            inputs: PlacementInputs {
                target_sessions: target_sessions + 1,
                target_cap: Some(target.max_sessions),
                machine_sessions: snapshot.sessions.as_ref().map(|running| {
                    u32::try_from(running.len())
                        .unwrap_or(u32::MAX)
                        .saturating_add(on_machine + 1)
                }),
                machine_cap: Some(admission.limits.max_sessions),
                free_ram_mb: snapshot.memory.free_mb,
                footprint_mb: Some(admission.footprint_mb),
                tool_state: Some(candidate.status.state.code().into()),
                tool_free: Some(candidate.free_sessions),
                tool_sessions: Some(running(&load, &candidate.tool, None)),
                account_sessions: account
                    .map(|account| running(&load, &candidate.tool, Some(account))),
            },
        });
        count(&mut load, &candidate.tool, account);
        *planned_target.entry(item.target_id.as_str()).or_default() += 1;
        *planned_machine
            .entry(snapshot.server_id.as_str())
            .or_default() += 1;
        allocated.extend(item.resources.iter());
    }

    // ---- Free sessions nothing was found for ----
    let mut warm: BTreeMap<&str, u32> = BTreeMap::new();
    let mut idle: Vec<usize> = (0..sessions.len())
        .filter(|index| matches!(fates[*index], Fate::Free))
        .collect();
    // The most recently idle session is the warm one.
    idle.sort_by_key(|index| {
        (
            std::cmp::Reverse(sessions[*index].idle_since_ms.unwrap_or(now)),
            &sessions[*index].session_id,
        )
    });
    for index in idle {
        let session = sessions[index];
        match session.activity {
            SessionActivity::WaitingOnGate => {
                fates[index] = Fate::Decided(A::Keep, R::WaitingOnGate, None, None);
                continue;
            }
            // It says it is busy, on nothing this machine recorded: leave it be.
            SessionActivity::Busy => {
                fates[index] = Fate::Decided(A::Keep, R::Busy, None, None);
                continue;
            }
            SessionActivity::Idle => {}
        }
        let kept = warm.entry(session.target_id.as_str()).or_default();
        if *kept < policy.wind_down.keep_warm {
            *kept += 1;
            fates[index] = Fate::Decided(A::Keep, R::KeepWarm, None, None);
            continue;
        }
        let idle_until = session.idle_since_ms.unwrap_or(now).saturating_add(idle_ms);
        let cool_until = memory(&session.target_id)
            .and_then(|entry| entry.last_started_at_ms)
            .map(|at| at.saturating_add(cooldown_ms))
            .filter(|until| *until > now);
        fates[index] = if idle_until > now {
            earliest(&mut retry_at, idle_until, now);
            Fate::Decided(A::Keep, R::IdleGrace, None, Some(idle_until))
        } else if let Some(until) = cool_until {
            earliest(&mut retry_at, until, now);
            Fate::Decided(A::Keep, R::Cooldown, None, Some(until))
        } else if !session.safe_point {
            Fate::Decided(A::Keep, R::AwaitingSafePoint, None, None)
        } else {
            Fate::Decided(A::WindDown, R::Idle, None, None)
        };
    }

    let mut placements = starts;
    let mut rest: Vec<Placement> = sessions
        .iter()
        .zip(fates)
        .map(|(session, fate)| {
            let (action, reason, detail, steps, score, retry) = match fate {
                Fate::Refilled(step, parts) => {
                    (A::Keep, R::Refill, None, vec![step], Some(parts), None)
                }
                Fate::Decided(action, reason, detail, retry) => {
                    (action, reason, detail, Vec::new(), None, retry)
                }
                // Every free session was decided above.
                Fate::Free => (A::Keep, R::Busy, None, Vec::new(), None, None),
            };
            Placement {
                action,
                target_id: session.target_id.clone(),
                tool: session.tool.clone(),
                account: session.account.clone(),
                model: session.model.clone(),
                session_id: Some(session.session_id.clone()),
                reason,
                detail,
                steps,
                score,
                probe: false,
                retry_at_ms: retry,
                inputs: PlacementInputs {
                    target_sessions: on_target
                        .get(session.target_id.as_str())
                        .copied()
                        .unwrap_or(0),
                    target_cap: policy
                        .target(&session.target_id)
                        .map(|target| target.max_sessions),
                    ..PlacementInputs::default()
                },
            }
        })
        .collect();
    rest.sort_by(|a, b| {
        (std::cmp::Reverse(a.action), &a.target_id, &a.session_id).cmp(&(
            std::cmp::Reverse(b.action),
            &b.target_id,
            &b.session_id,
        ))
    });
    placements.extend(rest);
    Schedule {
        placements,
        unplaced,
        ranking: ranked
            .iter()
            .map(|(parts, item)| Ranked {
                step_id: item.step_id.clone(),
                score: *parts,
            })
            .collect(),
        retry_at_ms: retry_at,
    }
}

// ---- Decision journal ----

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionMode {
    /// Computed and logged; nothing was started or wound down.
    DryRun,
    Applied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionKind {
    /// One pass: what it read and how much it decided.
    Tick,
    Start,
    Keep,
    WindDown,
    Unplaced,
}

/// What one pass read, as counts and identifiers.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TickInputs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_revision: Option<String>,
    pub automatic: bool,
    pub work: u32,
    pub confirmed: u32,
    pub sessions: u32,
    pub targets: u32,
    pub targets_unavailable: u32,
    /// Tool accounts with a limit in force.
    pub tools_limited: u32,
    pub held_resources: u32,
    /// `available`, or why the sessions could not be listed or changed.
    pub executor: String,
    pub starts: u32,
    pub keeps: u32,
    pub wind_downs: u32,
    pub unplaced: u32,
}

/// One logged decision, or the summary of the pass it belongs to. Identifiers,
/// codes and numbers only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionRecord {
    pub at_ms: u64,
    /// Every record of one pass carries the same id.
    pub tick_id: String,
    pub mode: DecisionMode,
    /// What caused the pass: `manual`, `event`, `retry` or `start`.
    pub trigger: String,
    pub kind: DecisionKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// What applying the decision did; absent for a dry run and for a kept session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome_detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    #[serde(default)]
    pub probe: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<ScoreParts>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inputs: Option<PlacementInputs>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tick: Option<TickInputs>,
}

impl JournalRecord for DecisionRecord {
    const DIRECTORY: &'static str = "orchestration-scheduler-decisions";
    fn validate(&self) -> Result<(), OrchestrationError> {
        valid_token(&self.tick_id)?;
        if let Some(target_id) = &self.target_id {
            valid_token(target_id)?;
        }
        for value in [
            Some(&self.trigger),
            self.actor_id.as_ref(),
            self.tool.as_ref(),
            self.account.as_ref(),
            self.session_id.as_ref(),
            self.step_id.as_ref(),
            self.reason.as_ref(),
            self.outcome.as_ref(),
            self.outcome_detail.as_ref(),
            self.operation_id.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            valid_identifier(value)?;
        }
        // A detail may name a resource (`namespace/resource`) or a model.
        for value in [&self.detail, &self.model].into_iter().flatten() {
            if !bounded_text(value, 200) || value.chars().any(char::is_whitespace) {
                return Err(OrchestrationError::Invalid("bounded decision detail"));
            }
        }
        Ok(())
    }
}

pub type DecisionEntry = JournalEntry<DecisionRecord>;
pub type DecisionPage = JournalPage<DecisionRecord>;
/// The decision log of one owning checkout.
pub type DecisionJournal = Journal<DecisionRecord>;

// ---- Placement memory ----

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MemoryFile {
    schema_version: u32,
    owners: BTreeMap<String, Vec<PlacementMemory>>,
}

/// This machine's placement memory, per owning checkout.
#[derive(Clone)]
pub struct SchedulerStore {
    storage: Storage,
}

impl SchedulerStore {
    pub fn new(home: &Path) -> Self {
        Self {
            storage: Storage::machine_named(home, "orchestration-scheduler.json"),
        }
    }

    fn read_file(&self) -> Result<MemoryFile, OrchestrationError> {
        let Some(bytes) = self.storage.read(MAX_MEMORY_BYTES)? else {
            return Ok(MemoryFile {
                schema_version: ORCHESTRATION_VERSION,
                owners: BTreeMap::new(),
            });
        };
        let file: MemoryFile = decode(&bytes)?;
        for (owner, entries) in &file.owners {
            valid_identifier(owner).map_err(|_| OrchestrationError::Corrupt)?;
            if entries.len() > MAX_TARGETS
                || entries
                    .iter()
                    .any(|entry| valid_token(&entry.target_id).is_err())
            {
                return Err(OrchestrationError::Corrupt);
            }
        }
        Ok(file)
    }

    /// An absent file reads as empty and is never created by a read.
    pub fn memory(&self, owner_id: &str) -> Result<Vec<PlacementMemory>, OrchestrationError> {
        Ok(self
            .read_file()?
            .owners
            .remove(owner_id)
            .unwrap_or_default())
    }

    /// Fold one pass into the owner's memory. Nothing applied writes nothing.
    pub fn record(
        &self,
        owner_id: &str,
        applied: &[(String, PlacementOutcome)],
        now_ms: u64,
    ) -> Result<Vec<PlacementMemory>, OrchestrationError> {
        valid_identifier(owner_id)?;
        if applied.is_empty() {
            return self.memory(owner_id);
        }
        let _lock = self.storage.lock()?;
        let mut file = self.read_file()?;
        let before = file.owners.get(owner_id).cloned().unwrap_or_default();
        let after = remember(&before, applied, now_ms);
        if after == before {
            return Ok(after);
        }
        if after.is_empty() {
            file.owners.remove(owner_id);
        } else {
            file.owners.insert(owner_id.into(), after.clone());
        }
        let bytes = serde_json::to_vec(&file).map_err(|_| OrchestrationError::Corrupt)?;
        self.storage.write(&bytes, MAX_MEMORY_BYTES)?;
        Ok(after)
    }
}
