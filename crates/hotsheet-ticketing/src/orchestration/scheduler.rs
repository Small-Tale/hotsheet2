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
use super::resources::ContentionLog;
use super::storage::{Storage, decode};
use super::{
    AdmissionConfig, AdmissionOutcome, AdmissionReason, AdmissionRequest, CapacityEntry,
    CapacitySnapshot, CapacityState, MAX_TARGETS, ORCHESTRATION_VERSION, OfflinePolicy,
    OrchestrationError, PolicyBody, PolicyDenial, ProjectSites, RequirementState,
    ResourceContention, ResourceState, RoleSessionKind, SelectionRequest, SessionCount,
    SessionRole, SignalSupport, StepRequirements, ToolScope, UnreadableClaims, UnreadableReason,
    decide_admission, requirement_misses, select,
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
    /// Rests on a pending gate. Ranked with the policy's risk weight, and placed only
    /// when the policy opts in and the provisional rule allows it (HS2-JSF985).
    Provisional,
}

/// HS2-JSF985: what the provisional rule (§26.22) says of one provisional item. The
/// scheduler does not decide it; whoever lists the work does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkSpeculation {
    pub allowed: bool,
    /// Why not, as the rule's stable code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refusal: Option<String>,
    /// Unconfirmed gates stacked beneath the work.
    #[serde(default)]
    pub depth: u32,
}

/// A resource one piece of work needs exclusively: declared on its plan step or, for
/// its whole target, in the policy (HS2-2FG91X).
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
    /// HS2-2FG91X: the machine whose claim backend holds it. A resource name means
    /// something only on its machine; absent, the claim counts on every machine.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_id: Option<String>,
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
    /// HS2-ZY4Z4G: what the target's machine must be. Work whose target's machine
    /// misses one is not placed there, not even on a free session.
    #[serde(default, skip_serializing_if = "StepRequirements::is_empty")]
    pub requirements: StepRequirements,
    /// HS2-JSF985: the provisional rule's answer for a provisional item. Absent, the
    /// item is ranked and never placed, as before the policy could opt in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speculation: Option<WorkSpeculation>,
    /// HS2-9FKTB3: the other sites of the work's project it may run on, besides
    /// `target_id`, which is tried first. Empty: only `target_id`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sites: Vec<String>,
}

impl WorkItem {
    /// HS2-6NQKP9: every resource the work needs on `site`: the step's own, then what
    /// the policy requires of every piece of work on that site
    /// (`placement.resources`), each once. Each site of a project is judged by its own
    /// policy entry, never by the entry of the site the step names.
    pub fn needs_on<'a>(&'a self, policy: &'a PolicyBody, site: &str) -> Vec<&'a ResourceNeed> {
        let mut needs: Vec<&ResourceNeed> = self.resources.iter().collect();
        for need in policy.target_resources(site) {
            if !needs.contains(&need) {
                needs.push(need);
            }
        }
        needs
    }
}

/// HS2-NPAZQW: the declared resources a pattern requirement can be given on one
/// machine, sorted: the exact resources the policy names for any target, those any
/// plan step of this pass names, and those someone holds there now. A name nobody
/// declared is never invented for a pattern.
fn declared_matches(
    input: &ScheduleInput<'_>,
    machine: &str,
    pattern: &ResourceNeed,
) -> Vec<ResourceNeed> {
    let held = input
        .held
        .iter()
        .filter(|held| held.server_id.as_deref().is_none_or(|id| id == machine))
        .map(|held| ResourceNeed {
            namespace: held.namespace.clone(),
            resource: held.resource.clone(),
        });
    input
        .policy
        .placement
        .resources
        .iter()
        .flat_map(|entry| entry.resources.iter().cloned())
        .chain(
            input
                .work
                .iter()
                .flat_map(|item| item.resources.iter().cloned()),
        )
        .chain(held)
        .filter(|candidate| pattern.satisfied_by(candidate))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
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
    /// HS2-TK9ADC: the session's role when it is not a worker. A role session is
    /// never given worker work; it is kept while its target needs it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<SessionRole>,
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
    /// HS2-JSF985. Start: provisional work the policy and the rule allow, which no
    /// running session can take.
    ProvisionalWork,
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
    /// HS2-NPTKS8. Wind down: it did not join its role in the target's pool in time,
    /// so its step's run is placed again. Keep, with detail `replacements_exhausted`:
    /// the run had its replacements and the session is left for a person.
    NeverJoined,
    /// HS2-NPTKS8. Wind down: its role prompt failed, so it will not join.
    RolePromptFailed,
    /// HS2-HV9ED3. Wind down: a person cancelled the run it works.
    RunCancelled,
    /// HS2-TK9ADC. Start: the target needs a dispatcher or integrator and its pool
    /// shows none live. Keep: a role session its target still needs. HS2-4JQ41V: a
    /// dispatcher's; an integrator's decisions have reasons of their own.
    RoleNeeded,
    /// HS2-4JQ41V. Start: the project's home pool shows handoffs pending and no
    /// integrator is live or starting; detail `handoff_pending`.
    IntegratorStarted,
    /// HS2-4JQ41V. Keep: the integrator is still needed; detail `queue` (handoffs wait
    /// to be integrated), `ci_pending` (an integrated commit waits on its CI gate) or
    /// `worker_run` (a worker it serves still has a run).
    IntegratorKept,
    /// HS2-4JQ41V. Wind down: nothing needed it for `wind_down.idle_minutes`; detail
    /// `idle`.
    IntegratorWoundDown,
}

impl PlacementReason {
    pub fn code(self) -> &'static str {
        match self {
            Self::ReadyWork => "ready_work",
            Self::ProvisionalWork => "provisional_work",
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
            Self::NeverJoined => "never_joined",
            Self::RolePromptFailed => "role_prompt_failed",
            Self::RunCancelled => "run_cancelled",
            Self::RoleNeeded => "role_needed",
            Self::IntegratorStarted => "integrator_started",
            Self::IntegratorKept => "integrator_kept",
            Self::IntegratorWoundDown => "integrator_wound_down",
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
    /// HS2-JSF985: the work this session is for rests on a pending gate.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub provisional: bool,
    /// HS2-TK9ADC: the role of the session started or kept, when it is not a worker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<SessionRole>,
    /// HS2-9FKTB3: the other sites of the work's project and why each was passed
    /// over; `target_id` is the site chosen. Empty for a one-site project.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sites: Vec<SitePass>,
    /// HS2-NPAZQW: for a start, the exact resources its work must hold on the chosen
    /// site, each pattern of the policy resolved to the one free resource this pass
    /// gave it. The session is told to acquire these.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<ResourceNeed>,
}

/// Why ready work gets no session in this pass. The wire spelling equals `code()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Blocker {
    /// Provisional work is ranked and not placed: the policy does not opt in.
    #[serde(rename = "provisional_not_dispatched")]
    Provisional,
    /// HS2-JSF985: the policy opts in, and the provisional rule refuses this work;
    /// `detail` is the rule's code.
    SpeculationRefused,
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
    /// HS2-2FG91X: the claims of a resource the work needs could not be read and its
    /// backend fails closed; `detail` names the resource.
    ResourceUnknown,
    /// The target wound a session down too recently to start one.
    Cooldown,
    /// The target refused a start and is not asked again yet.
    RefusalBackoff,
    /// HS2-ZY4Z4G: the target's machine is not what the work requires; `detail` names
    /// the first requirement it misses, such as `os:windows`.
    RequirementUnmet,
    /// HS2-ZY4Z4G: the target's machine did not report what a requirement asks about
    /// (an older peer, an unread value, a toolchain its owner did not declare);
    /// `detail` names the requirement. Nothing starts on a guess.
    RequirementUnknown,
}

impl Blocker {
    pub fn code(self) -> &'static str {
        match self {
            Self::Provisional => "provisional_not_dispatched",
            Self::SpeculationRefused => "speculation_refused",
            Self::PolicyDenied => "policy_denied",
            Self::TargetUnavailable => "target_unavailable",
            Self::TargetCap => "target_cap",
            Self::NoTool => "no_tool_available",
            Self::MachineRefused => "machine_refused",
            Self::ResourceHeld => "resource_held",
            Self::ResourceUnknown => "resource_unknown",
            Self::Cooldown => "cooldown",
            Self::RefusalBackoff => "refusal_backoff",
            Self::RequirementUnmet => "requirement_unmet",
            Self::RequirementUnknown => "requirement_unknown",
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
    /// HS2-9FKTB3: every other site of the work's project with its own reason;
    /// `target_id`, `reason` and `detail` are those of the site the step names.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sites: Vec<SitePass>,
}

/// HS2-4JQ41V: the detail of an integrator start.
pub const INTEGRATOR_HANDOFF_PENDING: &str = "handoff_pending";
/// HS2-4JQ41V: the details of a kept integrator.
pub const INTEGRATOR_QUEUE: &str = "queue";
pub const INTEGRATOR_CI_PENDING: &str = "ci_pending";
pub const INTEGRATOR_WORKER_RUN: &str = "worker_run";
/// HS2-4JQ41V: the detail of an integrator wound down, and of one not started because
/// the project's `placement.role_sessions` entry leaves the integrator out.
pub const INTEGRATOR_IDLE: &str = "idle";
pub const INTEGRATOR_POLICY_OPT_OUT: &str = "policy_opt_out";
pub const INTEGRATOR_TARGET_DISABLED: &str = "target_disabled";

/// HS2-4JQ41V: why a project with handoffs pending has no integrator starting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoleBlocker {
    /// This pass could not start one; `detail` is a [`Blocker`] code (`target_cap`,
    /// `machine_refused`, `no_tool_available`, `cooldown`, `refusal_backoff`,
    /// `target_unavailable`), `target_disabled` or `policy_opt_out`.
    IntegratorNotStarted,
    /// The scheduler's integrators failed to take up their role until their
    /// replacements were exhausted; none is started again before `retry_at_ms`.
    IntegratorUnavailable,
}

impl RoleBlocker {
    pub fn code(self) -> &'static str {
        match self {
            Self::IntegratorNotStarted => "integrator_not_started",
            Self::IntegratorUnavailable => "integrator_unavailable",
        }
    }
}

/// HS2-4JQ41V: a role session a project needs that this pass does not start, on the
/// project's home site.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleUnplaced {
    pub target_id: String,
    pub role: SessionRole,
    pub reason: RoleBlocker,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_at_ms: Option<u64>,
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
    /// HS2-2FG91X: every resource that deferred work in this pass, or that work was
    /// started on without its claims being readable.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contention: Vec<ResourceContention>,
    /// HS2-4JQ41V: each project whose pending handoffs get no integrator in this pass,
    /// and why.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roles_unplaced: Vec<RoleUnplaced>,
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
    /// HS2-9FKTB3: the other sites of the work's project and why each was passed over.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sites: Vec<SitePass>,
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
                sites: placement.sites.clone(),
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
                sites: item.sites.clone(),
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
    /// HS2-2FG91X: namespaces whose claims could not be read, per machine.
    pub unreadable: &'a [UnreadableClaims],
    pub memory: &'a [PlacementMemory],
    /// HS2-AWB7BX: targets whose capacity changes reach the loop as events (their host
    /// offers the capacity wait of HS2-P5GTSW). A capacity refusal there is lifted by
    /// the change, so it sets no retry time; elsewhere the backoff retry stays.
    pub capacity_streams: &'a [String],
    /// HS2-5P112Z: which project each target is a site of. Role sessions are decided
    /// per project and placed on its home site; an empty grouping makes every target
    /// its own one-site project, as before sites existed.
    pub sites: &'a ProjectSites,
}

/// HS2-AWB7BX: a refusal that says the machine had no room: a refused admission or one
/// of its reasons. A change of the machine's capacity can end it.
pub fn is_capacity_refusal(code: &str) -> bool {
    use super::capacity::AdmissionReason as R;
    code == super::SessionError::AdmissionRefused.code()
        || [
            R::ToolNotInstalled,
            R::ToolNotConfigured,
            R::SessionCapReached,
            R::AboveCpuCeiling,
            R::BelowDiskFloor,
            R::BelowRamFloor,
            R::CapacityUnknown,
        ]
        .iter()
        .any(|reason| reason.code() == code)
}

/// HS2-AWB7BX: forget a target's capacity refusal because its capacity changed, so the
/// next pass asks it again. Any other refusal, and every other entry, is kept.
pub fn lift_capacity_refusal(memory: &[PlacementMemory], target_id: &str) -> Vec<PlacementMemory> {
    memory
        .iter()
        .cloned()
        .map(|mut entry| {
            if entry.target_id == target_id
                && entry
                    .refusal_reason
                    .as_deref()
                    .is_some_and(is_capacity_refusal)
            {
                entry.refusals = 0;
                entry.refused_at_ms = None;
                entry.refusal_reason = None;
            }
            entry
        })
        .filter(|entry| !entry.is_empty())
        .collect()
}

/// HS2-TK9ADC: a worker session, as opposed to a dispatcher, integrator or planner.
fn is_worker(session: &ScheduledSession) -> bool {
    session.role.is_none_or(|role| role == SessionRole::Worker)
}

/// HS2-N5BDE4: the tools installed and signed in on a machine, which a target that
/// lists no tools (Auto) may use. Unknown reads as none: nothing starts on a guess.
fn machine_tools(snapshot: Option<&CapacitySnapshot>) -> Vec<String> {
    snapshot
        .and_then(|snapshot| snapshot.tools.as_ref())
        .map(|tools| {
            tools
                .iter()
                .filter(|tool| tool.installed && tool.configured)
                .map(|tool| tool.tool.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// The policy rule a running session breaks, if any.
fn session_denial(policy: &PolicyBody, session: &ScheduledSession) -> Option<PolicyDenial> {
    // HS2-N5BDE4: an unlisted target is Auto, and so is one that lists no tools.
    let target = policy.target(&session.target_id);
    if !target.enabled {
        return Some(PolicyDenial::TargetDisabled);
    }
    let Some(scope) = target.scope(&session.tool) else {
        return Some(PolicyDenial::ToolNotPermitted);
    };
    // HS2-N3HWP9: Sessions or Weight 0 turns the tool off.
    if policy.tool_policy(&session.tool).is_off() {
        return Some(PolicyDenial::ToolNotPermitted);
    }
    // HS2-1X1TXC: an entry may be a pattern.
    if !scope.allows(session.model.as_deref()) {
        return Some(PolicyDenial::ModelNotPermitted);
    }
    None
}

enum Fate {
    Free,
    /// HS2-TK9ADC: a role session, decided once the pass knows what its target needs.
    Role,
    Decided(
        PlacementAction,
        PlacementReason,
        Option<String>,
        Option<u64>,
    ),
    /// The step a free session takes, its score, whether it is provisional, and the
    /// other sites of its project passed over (HS2-9FKTB3).
    Refilled(String, ScoreParts, bool, Vec<SitePass>),
}

fn earliest(retry: &mut Option<u64>, at_ms: u64, now_ms: u64) {
    if at_ms > now_ms {
        *retry = Some(retry.map_or(at_ms, |known| known.min(at_ms)));
    }
}

/// Sessions of a tool, and of one of its accounts when named, in the pass's load.
fn running(load: &[SessionCount], tool: &str, account: Option<&str>) -> u32 {
    load.iter()
        .filter(|entry| {
            entry.tool == tool && account.is_none_or(|a| entry.account.as_deref() == Some(a))
        })
        .fold(0u32, |sum, entry| sum.saturating_add(entry.sessions))
}

// ---- HS2-9FKTB3: which site of a project runs a step ----

/// Why one site of a project was passed over for a step, or why it took no session
/// there. `reason` is a [`Blocker`] code, or `not_chosen` for a site that could have
/// taken the work when another was better; then `detail` says why: `free_session`
/// (another site had a free session), `preferred_tool` (another site could start the
/// preferred tool), `fewer_sessions` (another site's machine ran fewer sessions) or
/// `site_order` (a tie, broken by the named site, then the site id).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SitePass {
    pub target_id: String,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

pub const SITE_NOT_CHOSEN: &str = "not_chosen";

/// What one site would do with one step, decided without changing the pass.
enum SiteOutcome<'a> {
    /// A free session of the site takes it; `preferred` when the session has the
    /// work's preferred tool and model.
    Refill { index: usize, preferred: bool },
    /// A new session starts there.
    Start {
        placement: Box<Placement>,
        server: &'a str,
        /// Sessions on the site's machine, and on the site, with this one.
        machine_sessions: u32,
        target_sessions: u32,
        preferred: bool,
    },
    Blocked {
        reason: Blocker,
        detail: Option<String>,
        retry: Option<u64>,
    },
}

/// A resource the step waits on at one site, recorded only when the step is
/// placed nowhere.
struct WaitNote<'a> {
    machine: Option<&'a str>,
    need: &'a ResourceNeed,
    state: ResourceState,
    expires_at_ms: Option<u64>,
    taken_by: Option<&'a str>,
    unreadable: Option<UnreadableReason>,
}

struct SiteTry<'a> {
    site: &'a str,
    /// The machine that hosts the site, when it was read.
    machine: Option<&'a str>,
    outcome: SiteOutcome<'a>,
    /// Times a decision at this site changes by time alone.
    retries: Vec<u64>,
    waits: Vec<WaitNote<'a>>,
    /// Resources taken there without their claims being readable.
    unverified: Vec<(&'a ResourceNeed, UnreadableReason)>,
    /// HS2-NPAZQW: the exact resources the work would hold there, each pattern
    /// resolved to one free declared resource.
    resolved: Vec<ResourceNeed>,
}

impl SiteTry<'_> {
    fn note(&self, contention: &mut ContentionLog, step_id: &str) {
        for wait in &self.waits {
            let entry = contention.entry(wait.machine, wait.need, wait.state);
            if let Some(at) = wait.expires_at_ms {
                entry.expires_at_ms = Some(at);
            }
            if let Some(taker) = wait.taken_by {
                entry.taken_by = Some(taker.to_owned());
            }
            if let Some(reason) = wait.unreadable {
                entry.unreadable = Some(reason);
            }
            entry.waiting.push(step_id.to_owned());
        }
    }

    /// Lower is better: a free session of the preferred tool, a start of it, a free
    /// session of another tool, a start of another tool. Then the start whose
    /// machine runs fewest sessions; then the site's place in the candidate list.
    fn rank(&self, position: usize) -> Option<(u8, u32, u32, usize)> {
        match &self.outcome {
            SiteOutcome::Refill { preferred, .. } => {
                Some((if *preferred { 0 } else { 2 }, 0, 0, position))
            }
            SiteOutcome::Start {
                preferred,
                machine_sessions,
                target_sessions,
                ..
            } => Some((
                if *preferred { 1 } else { 3 },
                *machine_sessions,
                *target_sessions,
                position,
            )),
            SiteOutcome::Blocked { .. } => None,
        }
    }
}

/// The best site, or `None` when every site is blocked.
fn choose_site(tries: &[SiteTry<'_>]) -> Option<usize> {
    tries
        .iter()
        .enumerate()
        .filter_map(|(position, tried)| tried.rank(position))
        .min()
        .map(|(_, _, _, position)| position)
}

/// Every site but the chosen one (or the named one, when none was chosen), with why.
fn passed_over(tries: &[SiteTry<'_>], chosen: Option<usize>) -> Vec<SitePass> {
    let lead = chosen.unwrap_or(0);
    let best = chosen.and_then(|index| tries[index].rank(index));
    tries
        .iter()
        .enumerate()
        .filter(|(position, _)| *position != lead)
        .map(|(position, tried)| {
            let (reason, detail) = match (&tried.outcome, tried.rank(position), best) {
                (SiteOutcome::Blocked { reason, detail, .. }, _, _) => {
                    (reason.code().to_owned(), detail.clone())
                }
                (_, Some(mine), Some(best)) => {
                    let why = if best.0 % 2 == 0 {
                        "free_session"
                    } else if mine.0 > best.0 {
                        "preferred_tool"
                    } else if (mine.1, mine.2) > (best.1, best.2) {
                        "fewer_sessions"
                    } else {
                        "site_order"
                    };
                    (SITE_NOT_CHOSEN.to_owned(), Some(why.to_owned()))
                }
                _ => (SITE_NOT_CHOSEN.to_owned(), None),
            };
            SitePass {
                target_id: tried.site.to_owned(),
                reason,
                detail,
            }
        })
        .collect()
}

/// What one pass knows when it tries one site for one step.
struct SiteContext<'p, 'a> {
    input: &'p ScheduleInput<'a>,
    support: &'p dyn Fn(&str) -> SignalSupport,
    item: &'a WorkItem,
    parts: &'p ScoreParts,
    site: &'a str,
    sessions: &'p [&'a ScheduledSession],
    fates: &'p [Fate],
    load: &'p [SessionCount],
    on_target: &'p BTreeMap<&'a str, u32>,
    planned_target: &'p BTreeMap<&'a str, u32>,
    planned_machine: &'p BTreeMap<&'a str, u32>,
    allocated: &'p BTreeMap<(&'a str, ResourceNeed), &'a str>,
    cooldown_ms: u64,
}

/// HS2-9FKTB3: what one site would do with one step: every check the step names
/// (provisional opt-in, requirements, resources on the site's machine, a free
/// session, the policy, the site's cap, refusal backoff, cooldown, capacity, tool
/// accounts and admission), in the order a one-site project always used. Pure.
fn try_site<'a>(cx: &SiteContext<'_, 'a>) -> SiteTry<'a> {
    use PlacementReason as R;
    let input = cx.input;
    let policy = input.policy;
    let now = input.now_ms;
    let item = cx.item;
    let site = cx.site;
    let provisional = item.level == WorkLevel::Provisional;
    let mut tried = SiteTry {
        site,
        machine: None,
        outcome: SiteOutcome::Blocked {
            reason: Blocker::TargetUnavailable,
            detail: None,
            retry: None,
        },
        retries: Vec::new(),
        waits: Vec::new(),
        unverified: Vec::new(),
        resolved: Vec::new(),
    };
    macro_rules! finish {
        ($outcome:expr) => {{
            tried.outcome = $outcome;
            return tried;
        }};
    }
    let blocked =
        |reason: Blocker, detail: Option<String>, retry: Option<u64>| SiteOutcome::Blocked {
            reason,
            detail,
            retry,
        };
    if provisional {
        // HS2-JSF985: placed only when the policy opts in for the site and the
        // provisional rule allowed this exact item.
        match &item.speculation {
            Some(rule) if policy.speculation_allowed(site) => {
                if !rule.allowed {
                    finish!(blocked(
                        Blocker::SpeculationRefused,
                        rule.refusal.clone(),
                        None
                    ));
                }
            }
            _ => finish!(blocked(Blocker::Provisional, None, None)),
        }
    }
    let state = input.targets.iter().find(|state| state.target_id == site);
    let snapshot = state.and_then(|state| state.capacity.as_ref());
    // HS2-ZY4Z4G: work that names requirements goes only where the site's machine
    // meets them. A machine that misses one gets the work on no session, a free one
    // included; one that was not read, or did not say, is not trusted with it.
    if !item.requirements.is_empty() {
        let misses = snapshot.map(|snapshot| requirement_misses(&item.requirements, snapshot));
        let first = |state: RequirementState| {
            misses
                .iter()
                .flatten()
                .find(|miss| miss.state == state)
                .map(|miss| miss.requirement.clone())
        };
        if let Some(requirement) = first(RequirementState::Unmet) {
            finish!(blocked(Blocker::RequirementUnmet, Some(requirement), None));
        }
        if let Some(requirement) = first(RequirementState::Unknown) {
            finish!(blocked(
                Blocker::RequirementUnknown,
                Some(requirement),
                None
            ));
        }
        if snapshot.is_none() {
            finish!(blocked(
                Blocker::RequirementUnknown,
                Some("capacity_not_read".into()),
                None
            ));
        }
    }
    // HS2-2FG91X: a resource means something only on the machine that hosts the
    // site. Without a reading of that machine nothing is known about it.
    let machine = snapshot.map(|snapshot| snapshot.server_id.as_str());
    tried.machine = machine;
    let wait = |machine, need, state| WaitNote {
        machine,
        need,
        state,
        expires_at_ms: None,
        taken_by: None,
        unreadable: None,
    };
    // HS2-6NQKP9: the step's own resources and those this site's policy entry requires.
    let needs = item.needs_on(policy, site);
    for need in needs.iter().copied() {
        let Some(machine) = machine else {
            tried.waits.push(wait(None, need, ResourceState::NotRead));
            continue;
        };
        let held_until = |exact: &ResourceNeed| {
            input
                .held
                .iter()
                .filter(|held| {
                    held.namespace == exact.namespace
                        && held.resource == exact.resource
                        && held.expires_at_ms > now
                        && held.server_id.as_deref().is_none_or(|id| id == machine)
                })
                .map(|held| held.expires_at_ms)
                .max()
        };
        // HS2-NPAZQW: a pattern takes any one free declared resource it matches. An
        // exact resource the work already needs that matches satisfies it.
        if need.is_pattern() && needs.iter().any(|other| need.satisfied_by(other)) {
            continue;
        }
        let exact = if need.is_pattern() {
            let candidates = declared_matches(input, machine, need);
            let free = candidates.iter().find(|candidate| {
                held_until(candidate).is_none()
                    && !cx.allocated.contains_key(&(machine, (*candidate).clone()))
                    && !tried.resolved.contains(candidate)
            });
            match free {
                Some(free) => free.clone(),
                None if candidates.is_empty() => {
                    finish!(blocked(Blocker::ResourceUnknown, Some(need.name()), None));
                }
                None => {
                    let expires = candidates.iter().filter_map(&held_until).min();
                    tried.waits.push(WaitNote {
                        expires_at_ms: expires,
                        ..wait(Some(machine), need, ResourceState::Held)
                    });
                    if let Some(at) = expires {
                        tried.retries.push(at);
                    }
                    finish!(blocked(Blocker::ResourceHeld, Some(need.name()), expires));
                }
            }
        } else {
            need.clone()
        };
        let holder = held_until(&exact);
        if let Some(expires) = holder {
            tried.waits.push(WaitNote {
                expires_at_ms: Some(expires),
                ..wait(Some(machine), need, ResourceState::Held)
            });
            tried.retries.push(expires);
            finish!(blocked(
                Blocker::ResourceHeld,
                Some(need.name()),
                Some(expires)
            ));
        }
        if let Some(taker) = cx.allocated.get(&(machine, exact.clone())) {
            tried.waits.push(WaitNote {
                taken_by: Some(taker),
                ..wait(Some(machine), need, ResourceState::Taken)
            });
            finish!(blocked(Blocker::ResourceHeld, Some(need.name()), None));
        }
        let unread = input
            .unreadable
            .iter()
            .filter(|unread| unread.server_id == machine && unread.namespace == need.namespace)
            .min_by_key(|unread| unread.offline);
        match unread {
            Some(unread) if unread.offline == OfflinePolicy::FailClosed => {
                tried.waits.push(WaitNote {
                    unreadable: Some(unread.reason),
                    ..wait(Some(machine), need, ResourceState::Unreadable)
                });
                finish!(blocked(Blocker::ResourceUnknown, Some(need.name()), None));
            }
            Some(unread) => tried.unverified.push((need, unread.reason)),
            None => {}
        }
        tried.resolved.push(exact);
    }
    // A free session of the site takes the work: idle ones before those that wait
    // on a gate, which may be called back to their own ticket. HS2-TH53NN: the work's
    // tool and model are preferences. A free session that has them takes it first; a
    // new session of the preferred tool comes next; another free session of the site
    // takes it only when the preferred tool cannot start.
    let preferred = |session: &ScheduledSession| {
        item.tool.as_ref().is_none_or(|tool| tool == &session.tool)
            && item
                .model
                .as_ref()
                .is_none_or(|model| session.model.as_ref() == Some(model))
    };
    let free_where = |wanted: bool| {
        cx.sessions
            .iter()
            .enumerate()
            .filter(|(index, session)| {
                matches!(cx.fates[*index], Fate::Free)
                    // Work whose resources were not read is not handed to anyone.
                    && (machine.is_some() || needs.is_empty())
                    && session.target_id == site
                    && preferred(session) == wanted
            })
            .min_by_key(|(_, session)| (std::cmp::Reverse(session.activity), &session.session_id))
            .map(|(index, _)| index)
    };
    if let Some(index) = free_where(true) {
        finish!(SiteOutcome::Refill {
            index,
            preferred: true
        });
    }
    let other_free = free_where(false);
    // A blocked start falls back to another free session of the site.
    macro_rules! fallback {
        ($reason:expr, $detail:expr, $retry:expr $(,)?) => {{
            match other_free {
                Some(index) => finish!(SiteOutcome::Refill {
                    index,
                    preferred: false
                }),
                None => finish!(blocked($reason, $detail, $retry)),
            }
        }};
    }
    let auto_tools = machine_tools(snapshot);
    let selection_for = |tool: Option<&str>, model: Option<&str>| {
        select(
            &SelectionRequest {
                policy,
                target_id: site,
                tool,
                model,
                entries: input.tool_entries,
                load: cx.load,
                now_ms: now,
                auto_tools: &auto_tools,
            },
            cx.support,
        )
    };
    let mut selection = selection_for(item.tool.as_deref(), item.model.as_deref());
    // HS2-TH53NN: a preferred tool or model that leaves nothing to start on is a
    // preference, not a requirement: any allowed tool account takes the work.
    if selection.denied.is_none()
        && selection.candidates.is_empty()
        && (item.tool.is_some() || item.model.is_some())
    {
        if let Some(index) = other_free {
            finish!(SiteOutcome::Refill {
                index,
                preferred: false
            });
        }
        selection = selection_for(None, None);
    }
    if let Some(denied) = selection.denied {
        fallback!(Blocker::PolicyDenied, Some(denied.into()), None);
    }
    let target = policy.target(site);
    let target_cap = policy.target_cap(&target);
    let target_sessions = cx.on_target.get(site).copied().unwrap_or(0)
        + cx.planned_target.get(site).copied().unwrap_or(0);
    if target_sessions >= target_cap {
        fallback!(Blocker::TargetCap, None, None);
    }
    // What the policy and the site's cap say stands until a person changes it; a
    // refusal or a cooldown only postpones, so those are named after.
    let remembered = input.memory.iter().find(|entry| entry.target_id == site);
    if let Some(until) = remembered
        .and_then(PlacementMemory::refused_until_ms)
        .filter(|until| *until > now)
    {
        let reason = remembered.and_then(|entry| entry.refusal_reason.clone());
        // HS2-AWB7BX: a capacity refusal on a site whose changes arrive as events
        // waits for the change, not for a time.
        let evented = input.capacity_streams.iter().any(|id| id == site)
            && reason.as_deref().is_some_and(is_capacity_refusal);
        if evented {
            fallback!(Blocker::RefusalBackoff, reason, None);
        }
        tried.retries.push(until);
        fallback!(Blocker::RefusalBackoff, reason, Some(until));
    }
    if let Some(until) = remembered
        .and_then(|entry| entry.last_wound_down_at_ms)
        .map(|at| at.saturating_add(cx.cooldown_ms))
        .filter(|until| *until > now)
    {
        tried.retries.push(until);
        fallback!(Blocker::Cooldown, None, Some(until));
    }
    let Some(snapshot) = snapshot else {
        fallback!(
            Blocker::TargetUnavailable,
            Some(
                state
                    .and_then(|state| state.unavailable.clone())
                    .unwrap_or_else(|| "capacity_not_read".into()),
            ),
            None,
        );
    };
    let limits = policy.machine_limits(site);
    let on_machine = cx
        .planned_machine
        .get(snapshot.server_id.as_str())
        .copied()
        .unwrap_or(0);
    let mut nearest: Option<(Blocker, String, Option<u64>)> = None;
    let mut placed = None;
    for candidate in &selection.candidates {
        let account = candidate.account.as_deref();
        // A degraded account is tried by one session at a time.
        if candidate.status.state == CapacityState::Degraded
            && running(cx.load, &candidate.tool, account) > 0
        {
            nearest.get_or_insert((Blocker::NoTool, "probe_in_flight".into(), None));
            continue;
        }
        // HS2-TH53NN: the preferred model goes with the preferred tool (or with any
        // tool when none is preferred); another tool uses its own first model.
        let model = item
            .model
            .clone()
            .filter(|_| {
                item.tool
                    .as_ref()
                    .is_none_or(|tool| tool == &candidate.tool)
                    && target
                        .scope(&candidate.tool)
                        .is_some_and(|scope| scope.allows(item.model.as_deref()))
            })
            .or_else(|| {
                // HS2-1X1TXC: the first exact model; patterns alone name none.
                target
                    .tools
                    .iter()
                    .find(|scope| scope.tool == candidate.tool)
                    .and_then(ToolScope::start_model)
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
            tried.retries.push(at);
        }
        finish!(blocked(reason, Some(detail), retry));
    };
    // HS2-TH53NN: a start of another tool than the preferred one is no better than a
    // free session of the site that is already running.
    let preferred_start = item
        .tool
        .as_ref()
        .is_none_or(|tool| tool == &candidate.tool)
        && item
            .model
            .as_ref()
            .is_none_or(|wanted| model.as_ref() == Some(wanted));
    if let Some(index) = other_free.filter(|_| !preferred_start) {
        finish!(SiteOutcome::Refill {
            index,
            preferred: false
        });
    }
    let account = candidate.account.as_deref();
    let machine_sessions = snapshot.sessions.as_ref().map(|running| {
        u32::try_from(running.len())
            .unwrap_or(u32::MAX)
            .saturating_add(on_machine + 1)
    });
    finish!(SiteOutcome::Start {
        placement: Box::new(Placement {
            resources: Vec::new(),
            action: PlacementAction::Start,
            target_id: site.to_owned(),
            tool: candidate.tool.clone(),
            account: candidate.account.clone(),
            model,
            session_id: None,
            reason: if provisional {
                R::ProvisionalWork
            } else {
                R::ReadyWork
            },
            detail: None,
            provisional,
            steps: vec![item.step_id.clone()],
            score: Some(*cx.parts),
            probe: !candidate.verified,
            retry_at_ms: None,
            inputs: PlacementInputs {
                target_sessions: target_sessions + 1,
                target_cap: Some(target_cap),
                machine_sessions,
                machine_cap: Some(admission.limits.max_sessions),
                free_ram_mb: snapshot.memory.free_mb,
                footprint_mb: Some(admission.footprint_mb),
                tool_state: Some(candidate.status.state.code().into()),
                tool_free: Some(candidate.free_sessions),
                tool_sessions: Some(running(cx.load, &candidate.tool, None)),
                account_sessions: account.map(|account| running(
                    cx.load,
                    &candidate.tool,
                    Some(account)
                )),
            },
            role: None,
            sites: Vec::new(),
        }),
        server: snapshot.server_id.as_str(),
        machine_sessions: machine_sessions.unwrap_or(on_machine + 1),
        target_sessions: target_sessions + 1,
        preferred: preferred_start,
    });
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
            if !is_worker(session) {
                return Fate::Role;
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
        // HS2-4JQ41V: an integrator may run one over the cap, so its slot is not a
        // worker's to give up.
        let cap = policy.target_cap(target);
        let allowance = u32::from(
            cap > 0
                && sessions.iter().any(|session| {
                    session.target_id == target.target_id
                        && session.role == Some(SessionRole::Integrator)
                        && !session.winding_down
                }),
        );
        while staying(&fates) > (cap + allowance) as usize {
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
    // HS2-JSF985: where the policy opts in, confirmed work goes before provisional
    // work of the same score. Without the opt-in the order is what it always was.
    let speculates = policy.speculates();
    ranked.sort_by(|(a, x), (b, y)| {
        b.total
            .cmp(&a.total)
            .then(if speculates {
                x.level.cmp(&y.level)
            } else {
                std::cmp::Ordering::Equal
            })
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
    let mut planned_target: BTreeMap<&str, u32> = BTreeMap::new();
    let mut planned_machine: BTreeMap<&str, u32> = BTreeMap::new();
    // HS2-2FG91X: a resource given to work in this pass, per machine, and to which step.
    let mut allocated: BTreeMap<(&str, ResourceNeed), &str> = BTreeMap::new();
    let mut contention = ContentionLog::default();
    let mut starts = Vec::new();
    let mut unplaced = Vec::new();
    let mut roles_unplaced: Vec<RoleUnplaced> = Vec::new();

    // ---- HS2-TK9ADC: role sessions a target needs, before any worker ----
    // Work a session could be placed for, as the work loop below decides it.
    let placeable = |item: &WorkItem| match item.level {
        WorkLevel::Confirmed => true,
        WorkLevel::Provisional => {
            policy.speculation_allowed(&item.target_id)
                && item.speculation.as_ref().is_some_and(|rule| rule.allowed)
        }
    };
    let pool_of = |target_id: &str| {
        input
            .targets
            .iter()
            .find(|state| state.target_id == target_id)
            .and_then(|state| state.capacity.as_ref())
            .and_then(|snapshot| snapshot.pool)
    };
    // HS2-5P112Z: the targets a role session on `target_id` serves. A project's home
    // site serves every site of its project through its one pool; any other target
    // serves only itself, as before sites existed.
    let served = |target_id: &str| -> Vec<String> {
        if input.sites.home_of(target_id) == target_id {
            input.sites.sites_of(target_id)
        } else {
            vec![target_id.to_owned()]
        }
    };
    // HS2-R2N33M: a project the policy does not list is Auto: the home of every
    // project with plan work or a running session may get both roles, each only when
    // it helps below.
    // HS2-4JQ41V: so is the home of a project whose pool shows handoffs pending, so
    // its integrator starts after the workers that handed off have gone.
    let homes: BTreeSet<&str> = input
        .work
        .iter()
        .map(|item| item.target_id.as_str())
        .chain(sessions.iter().map(|session| session.target_id.as_str()))
        .chain(
            input
                .targets
                .iter()
                .filter(|state| {
                    pool_of(&state.target_id).is_some_and(|pool| pool.handoffs_pending > 0)
                })
                .map(|state| state.target_id.as_str()),
        )
        .map(|target_id| input.sites.home_of(target_id))
        .filter(|home| policy.placement.role_session_entry(home).is_none())
        .collect();
    let auto_entries: Vec<super::TargetRoleSessions> = homes
        .into_iter()
        .map(|target_id| super::TargetRoleSessions {
            target_id: target_id.to_owned(),
            roles: RoleSessionKind::ALL.to_vec(),
        })
        .collect();
    for entry in policy.placement.role_sessions.iter().chain(&auto_entries) {
        let target_id = entry.target_id.as_str();
        // HS2-5P112Z: role sessions are started per project, on its home site only. An
        // entry naming another site of a project starts nothing there: that site's
        // workers join the home's pool instead.
        if input.sites.home_of(target_id) != target_id {
            continue;
        }
        let target = policy.target(target_id);
        // HS2-4JQ41V: handoffs wait and no integrator is live or on its way, so a pass
        // that starts none says why.
        let integrator_wanted = pool_of(target_id)
            .is_some_and(|pool| pool.handoffs_pending > 0 && !pool.integrator_live)
            && !sessions.iter().any(|session| {
                session.target_id == target_id && session.role == Some(SessionRole::Integrator)
            });
        let mut not_started = |detail: &str, retry: Option<u64>| {
            if integrator_wanted {
                roles_unplaced.push(RoleUnplaced {
                    target_id: target_id.to_owned(),
                    role: SessionRole::Integrator,
                    reason: RoleBlocker::IntegratorNotStarted,
                    detail: Some(detail.to_owned()),
                    retry_at_ms: retry,
                });
            }
        };
        if !target.enabled {
            not_started(INTEGRATOR_TARGET_DISABLED, None);
            continue;
        }
        let target_cap = policy.target_cap(&target);
        // Unknown is not empty: nothing starts on a guess.
        let Some(pool) = pool_of(target_id) else {
            continue;
        };
        let project = served(target_id);
        let in_project = |id: &str| project.iter().any(|site| site == id);
        let ready = input
            .work
            .iter()
            .any(|item| in_project(&item.target_id) && placeable(item));
        // HS2-70Z2Z6: a site of the project can still take a worker for some ready
        // step after the dispatcher starts: the step may run there (the site it names
        // or one of its other sites), the site is enabled, its machine meets every
        // requirement the step names, and the site either runs a worker already or
        // keeps a free slot with the dispatcher counted on the home. Without such a
        // site the dispatcher would hold a slot for work it can give to nobody: ready
        // work that fits only the home's last slot, or work no site meets at all
        // (which stays unplaced with each site's reason).
        let site_takes_work = |site: &str, planned: &BTreeMap<&str, u32>, dispatcher_here: u32| {
            let other = policy.target(site);
            if !other.enabled {
                return false;
            }
            let worker_there = sessions
                .iter()
                .any(|session| session.target_id == site && is_worker(session));
            let room = on_target.get(site).copied().unwrap_or(0)
                + planned.get(site).copied().unwrap_or(0)
                + dispatcher_here
                < policy.target_cap(&other);
            if !worker_there && !room {
                return false;
            }
            let snapshot = input
                .targets
                .iter()
                .find(|state| state.target_id == site)
                .and_then(|state| state.capacity.as_ref());
            input.work.iter().any(|item| {
                in_project(&item.target_id)
                    && placeable(item)
                    && (item.target_id == site || item.sites.iter().any(|named| named == site))
                    && (item.requirements.is_empty()
                        || snapshot.is_some_and(|snapshot| {
                            requirement_misses(&item.requirements, snapshot).is_empty()
                        }))
            })
        };
        for kind in RoleSessionKind::ALL {
            let integrator = kind == RoleSessionKind::Integrator;
            // Reports only an integrator's reasons; a dispatcher's are not recorded.
            let mut blocked = |detail: &str, retry: Option<u64>| {
                if integrator {
                    not_started(detail, retry);
                }
            };
            if !entry.roles.contains(&kind) {
                // HS2-4JQ41V: the person's entry leaves the integrator out.
                blocked(INTEGRATOR_POLICY_OPT_OUT, None);
                continue;
            }
            let role = kind.role();
            let (live, wanted) = match kind {
                // HS2-5P112Z, HS2-70Z2Z6: the dispatcher starts only when some site of
                // the project can take a worker for ready work it can run.
                RoleSessionKind::Dispatcher => (
                    pool.dispatcher_live,
                    ready
                        && project.iter().any(|site| {
                            site_takes_work(site, &planned_target, u32::from(site == target_id))
                        }),
                ),
                RoleSessionKind::Integrator => (pool.integrator_live, pool.handoffs_pending > 0),
            };
            let held = sessions
                .iter()
                .any(|session| session.target_id == target_id && session.role == Some(role));
            if live || !wanted || held {
                continue;
            }
            let target_sessions = on_target.get(target_id).copied().unwrap_or(0)
                + planned_target.get(target_id).copied().unwrap_or(0);
            // HS2-4JQ41V: workers filling the cap never starve integration: the
            // integrator may run one over it. A cap of 0 still starts nothing.
            let reserve = u32::from(integrator && target_cap > 0);
            if target_sessions + 1 > target_cap + reserve {
                blocked(Blocker::TargetCap.code(), None);
                continue;
            }
            let remembered = memory(target_id);
            if let Some(until) = remembered
                .and_then(PlacementMemory::refused_until_ms)
                .filter(|until| *until > now)
            {
                let evented = input.capacity_streams.iter().any(|id| id == target_id)
                    && remembered
                        .and_then(|entry| entry.refusal_reason.as_deref())
                        .is_some_and(is_capacity_refusal);
                if !evented {
                    earliest(&mut retry_at, until, now);
                }
                blocked(Blocker::RefusalBackoff.code(), (!evented).then_some(until));
                continue;
            }
            if let Some(until) = remembered
                .and_then(|entry| entry.last_wound_down_at_ms)
                .map(|at| at.saturating_add(cooldown_ms))
                .filter(|until| *until > now)
            {
                earliest(&mut retry_at, until, now);
                blocked(Blocker::Cooldown.code(), Some(until));
                continue;
            }
            let Some(snapshot) = input
                .targets
                .iter()
                .find(|state| state.target_id == target_id)
                .and_then(|state| state.capacity.as_ref())
            else {
                blocked(Blocker::TargetUnavailable.code(), None);
                continue;
            };
            let selection = select(
                &SelectionRequest {
                    policy,
                    target_id,
                    tool: None,
                    model: None,
                    entries: input.tool_entries,
                    load: &load,
                    now_ms: now,
                    auto_tools: &machine_tools(Some(snapshot)),
                },
                support,
            );
            if selection.denied.is_some() || selection.candidates.is_empty() {
                blocked(Blocker::NoTool.code(), None);
                continue;
            }
            let limits = policy.machine_limits(target_id);
            let on_machine = planned_machine
                .get(snapshot.server_id.as_str())
                .copied()
                .unwrap_or(0);
            let placed = selection.candidates.iter().find_map(|candidate| {
                if candidate.status.state == CapacityState::Degraded
                    && running(&load, &candidate.tool, candidate.account.as_deref()) > 0
                {
                    return None;
                }
                let model = target
                    .tools
                    .iter()
                    .find(|scope| scope.tool == candidate.tool)
                    .and_then(ToolScope::start_model);
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
                (admission.outcome == AdmissionOutcome::Accepted)
                    .then_some((candidate, model, admission))
            });
            let Some((candidate, model, admission)) = placed else {
                blocked(Blocker::MachineRefused.code(), None);
                continue;
            };
            let account = candidate.account.as_deref();
            starts.push(Placement {
                resources: Vec::new(),
                action: A::Start,
                target_id: target_id.to_owned(),
                tool: candidate.tool.clone(),
                account: candidate.account.clone(),
                model,
                session_id: None,
                reason: if integrator {
                    R::IntegratorStarted
                } else {
                    R::RoleNeeded
                },
                detail: Some(
                    if integrator {
                        INTEGRATOR_HANDOFF_PENDING
                    } else {
                        role.code()
                    }
                    .to_owned(),
                ),
                provisional: false,
                steps: Vec::new(),
                score: None,
                probe: !candidate.verified,
                retry_at_ms: None,
                inputs: PlacementInputs {
                    target_sessions: target_sessions + 1,
                    target_cap: Some(target_cap),
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
                role: Some(role),
                sites: Vec::new(),
            });
            count(&mut load, &candidate.tool, account);
            *planned_target.entry(target_id).or_default() += 1;
            *planned_machine
                .entry(snapshot.server_id.as_str())
                .or_default() += 1;
        }
    }

    for (parts, item) in ranked.iter().map(|(parts, item)| (parts, *item)) {
        let provisional = item.level == WorkLevel::Provisional;
        // HS2-9FKTB3: the site the step names first, then the other sites of its project
        // it may run on by id, whatever order they were listed in. Each is tried on its
        // own; nothing changes until one is chosen.
        let others: BTreeSet<&str> = item
            .sites
            .iter()
            .map(String::as_str)
            .filter(|site| *site != item.target_id)
            .collect();
        let candidates: Vec<&str> = std::iter::once(item.target_id.as_str())
            .chain(others)
            .take(MAX_TARGETS)
            .collect();
        let tries: Vec<SiteTry<'_>> = candidates
            .iter()
            .map(|site| {
                try_site(&SiteContext {
                    input,
                    support,
                    item,
                    parts,
                    site,
                    sessions: &sessions,
                    fates: &fates,
                    load: &load,
                    on_target: &on_target,
                    planned_target: &planned_target,
                    planned_machine: &planned_machine,
                    allocated: &allocated,
                    cooldown_ms,
                })
            })
            .collect();
        let Some(chosen) = choose_site(&tries) else {
            // Nowhere: the named site's reason leads, every other site says its own.
            for tried in &tries {
                tried.note(&mut contention, &item.step_id);
                for at in &tried.retries {
                    earliest(&mut retry_at, *at, now);
                }
            }
            let (reason, detail, retry) = match &tries[0].outcome {
                SiteOutcome::Blocked {
                    reason,
                    detail,
                    retry,
                } => (*reason, detail.clone(), *retry),
                // choose_site found nothing, so every try is blocked.
                _ => (Blocker::TargetUnavailable, None, None),
            };
            unplaced.push(Unplaced {
                step_id: item.step_id.clone(),
                target_id: item.target_id.clone(),
                reason,
                detail,
                retry_at_ms: retry.filter(|at| *at > now),
                score: *parts,
                sites: passed_over(&tries, None),
            });
            continue;
        };
        let sites = passed_over(&tries, Some(chosen));
        let tried = &tries[chosen];
        for at in &tried.retries {
            earliest(&mut retry_at, *at, now);
        }
        // Given to this step: nothing ranked below it in this pass gets it too.
        if let Some(machine) = tried.machine {
            // HS2-NPAZQW: the exact resources, a pattern's resolved one included.
            for need in &tried.resolved {
                allocated.insert((machine, need.clone()), item.step_id.as_str());
            }
            for (need, reason) in &tried.unverified {
                let entry = contention.entry(Some(machine), need, ResourceState::Unverified);
                entry.unreadable = Some(*reason);
                entry.taken_by = Some(item.step_id.clone());
            }
        }
        match &tried.outcome {
            SiteOutcome::Refill { index, .. } => {
                fates[*index] = Fate::Refilled(item.step_id.clone(), *parts, provisional, sites);
            }
            SiteOutcome::Start {
                placement, server, ..
            } => {
                count(&mut load, &placement.tool, placement.account.as_deref());
                *planned_target.entry(tried.site).or_default() += 1;
                *planned_machine.entry(server).or_default() += 1;
                starts.push(Placement {
                    sites,
                    resources: tried.resolved.clone(),
                    ..(**placement).clone()
                });
            }
            SiteOutcome::Blocked { .. } => {}
        }
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

    // ---- HS2-TK9ADC: role sessions, kept while their target needs them ----
    for index in 0..sessions.len() {
        if !matches!(fates[index], Fate::Role) {
            continue;
        }
        let session = sessions[index];
        let target_id = session.target_id.as_str();
        // HS2-5P112Z: a role session on a project's home serves every site of it.
        let project = served(target_id);
        let in_project = |id: &str| project.iter().any(|site| site == id);
        // A worker the session serves still has a run: whoever serves it stays.
        let dependent = sessions.iter().any(|other| {
            in_project(&other.target_id) && is_worker(other) && other.run_id.is_some()
        });
        let integrator = session.role == Some(SessionRole::Integrator);
        // HS2-4JQ41V: why an integrator is still needed, most pressing first.
        let integrator_need = || {
            let pool = pool_of(target_id).unwrap_or_default();
            if pool.handoffs_pending > pool.ci_pending {
                Some(INTEGRATOR_QUEUE)
            } else if pool.ci_pending > 0 {
                Some(INTEGRATOR_CI_PENDING)
            } else if dependent {
                Some(INTEGRATOR_WORKER_RUN)
            } else {
                None
            }
        };
        let needed = match session.role {
            Some(SessionRole::Dispatcher) => {
                dependent
                    || input
                        .work
                        .iter()
                        .any(|item| in_project(&item.target_id) && placeable(item))
            }
            Some(SessionRole::Integrator) => integrator_need().is_some(),
            // A planner is a person's, through planning; the scheduler never stops it.
            _ => true,
        };
        let detail = session.role.map(|role| role.code().to_owned());
        fates[index] = if needed && integrator {
            Fate::Decided(
                A::Keep,
                R::IntegratorKept,
                integrator_need().map(str::to_owned),
                None,
            )
        } else if needed {
            Fate::Decided(A::Keep, R::RoleNeeded, detail, None)
        } else if session.activity != SessionActivity::Idle {
            Fate::Decided(A::Keep, R::Busy, detail, None)
        } else {
            let idle_until = session.idle_since_ms.unwrap_or(now).saturating_add(idle_ms);
            let cool_until = memory(target_id)
                .and_then(|entry| entry.last_started_at_ms)
                .map(|at| at.saturating_add(cooldown_ms))
                .filter(|until| *until > now);
            if idle_until > now {
                earliest(&mut retry_at, idle_until, now);
                Fate::Decided(A::Keep, R::IdleGrace, detail, Some(idle_until))
            } else if let Some(until) = cool_until {
                earliest(&mut retry_at, until, now);
                Fate::Decided(A::Keep, R::Cooldown, detail, Some(until))
            } else if !session.safe_point {
                // The host's safe point counts every unfinished assignment of the pool.
                Fate::Decided(A::Keep, R::AwaitingSafePoint, detail, None)
            } else if integrator {
                Fate::Decided(
                    A::WindDown,
                    R::IntegratorWoundDown,
                    Some(INTEGRATOR_IDLE.to_owned()),
                    None,
                )
            } else {
                Fate::Decided(A::WindDown, R::Idle, detail, None)
            }
        };
    }

    let mut placements = starts;
    let mut rest: Vec<Placement> = sessions
        .iter()
        .zip(fates)
        .map(|(session, fate)| {
            let mut provisional = false;
            let mut sites = Vec::new();
            let (action, reason, detail, steps, score, retry) = match fate {
                Fate::Refilled(step, parts, rests_on_gate, passed) => {
                    provisional = rests_on_gate;
                    sites = passed;
                    (A::Keep, R::Refill, None, vec![step], Some(parts), None)
                }
                Fate::Decided(action, reason, detail, retry) => {
                    (action, reason, detail, Vec::new(), None, retry)
                }
                // Every free and role session was decided above.
                Fate::Free => (A::Keep, R::Busy, None, Vec::new(), None, None),
                Fate::Role => (A::Keep, R::RoleNeeded, None, Vec::new(), None, None),
            };
            Placement {
                resources: Vec::new(),
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
                provisional,
                retry_at_ms: retry,
                inputs: PlacementInputs {
                    target_sessions: on_target
                        .get(session.target_id.as_str())
                        .copied()
                        .unwrap_or(0),
                    target_cap: Some(policy.target_cap(&policy.target(&session.target_id))),
                    ..PlacementInputs::default()
                },
                role: session.role.filter(|role| *role != SessionRole::Worker),
                sites,
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
        contention: contention.finish(),
        roles_unplaced,
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
    /// HS2-T16SK0: a placed step's run that nobody in the target's pool will assign,
    /// or whose pool could not be read, and what the pass did about it.
    Assignment,
    /// HS2-AH8CRS: a run's reconciliation disposition the pass acted on: a dispatch
    /// repeated under its retained operation id, or a person told on the goal ticket.
    Reconcile,
    /// HS2-4JQ41V: a project's integrator the pass did not start although handoffs
    /// wait: `integrator_not_started` or `integrator_unavailable` with its detail.
    RoleUnplaced,
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
    /// HS2-2FG91X: claim namespaces a machine could not read for this pass.
    #[serde(default)]
    pub unreadable_namespaces: u32,
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
    /// HS2-YC2P7K: the run an applied start or refill dispatched for its step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
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
    /// HS2-9FKTB3: the other sites of the step's project and why each was passed over.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sites: Vec<SitePass>,
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
            self.run_id.as_ref(),
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
        if self.sites.len() > MAX_TARGETS {
            return Err(OrchestrationError::Invalid("too many sites"));
        }
        for pass in &self.sites {
            valid_token(&pass.target_id)?;
            valid_identifier(&pass.reason)?;
            if pass.detail.as_ref().is_some_and(|detail| {
                !bounded_text(detail, 200) || detail.chars().any(char::is_whitespace)
            }) {
                return Err(OrchestrationError::Invalid("bounded decision detail"));
            }
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

/// HS2-7Q428Z: hold this owner's scheduling passes to one at a time on this machine,
/// across the server's loop, manual ticks and other processes over the same home. A
/// pass reads what the last one did, so a start is decided, applied and logged once.
/// Blocks until the pass before it ends; released when dropped.
pub fn scheduler_pass_lock(
    home: &Path,
    owner_id: &str,
) -> Result<crate::FileLock, OrchestrationError> {
    valid_identifier(owner_id)?;
    Storage::machine_named(
        home,
        &format!("orchestration-scheduler-pass-{owner_id}.json"),
    )
    .lock()
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

    /// HS2-AWB7BX: forget one target's capacity refusal after its capacity changed.
    /// True when anything was forgotten.
    pub fn lift_capacity_refusal(
        &self,
        owner_id: &str,
        target_id: &str,
    ) -> Result<bool, OrchestrationError> {
        valid_identifier(owner_id)?;
        let _lock = self.storage.lock()?;
        let mut file = self.read_file()?;
        let before = file.owners.get(owner_id).cloned().unwrap_or_default();
        let after = lift_capacity_refusal(&before, target_id);
        if after == before {
            return Ok(false);
        }
        if after.is_empty() {
            file.owners.remove(owner_id);
        } else {
            file.owners.insert(owner_id.into(), after);
        }
        let bytes = serde_json::to_vec(&file).map_err(|_| OrchestrationError::Corrupt)?;
        self.storage.write(&bytes, MAX_MEMORY_BYTES)?;
        Ok(true)
    }

    /// HS2-AWB7BX: forget every capacity refusal of the owner, after a change of its
    /// own policy limits that can give a machine room. True when anything changed.
    pub fn lift_capacity_refusals(&self, owner_id: &str) -> Result<bool, OrchestrationError> {
        let targets: Vec<String> = self
            .memory(owner_id)?
            .into_iter()
            .map(|entry| entry.target_id)
            .collect();
        let mut lifted = false;
        for target in targets {
            lifted |= self.lift_capacity_refusal(owner_id, &target)?;
        }
        Ok(lifted)
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
