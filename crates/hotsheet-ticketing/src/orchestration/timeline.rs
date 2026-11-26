//! HS2-J89JC7: the orchestration timeline.
//!
//! One read of an owning checkout's plan (§26.15), its runs (§26.9), the latest
//! scheduler decision per step (§26.17) and its sessions (§26.16), arranged as
//! dependency waves: Done, Working Now, Next, Then and Later once orchestration was
//! kicked off, and First, Second, … for a proposed plan. Each item says where its
//! work runs, what state it is in and what it waits on.
//!
//! A pure function of typed inputs the runtime gathered from this machine's records:
//! it contacts nothing and writes nothing. Identifiers, codes and times only, except
//! the goal and target ticket labels the runtime read from stores on this machine.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::gates::GateState;
use super::readiness::{Prerequisite, ReadinessLevel, StepReadiness, TicketKey, ticket_key};
use super::{
    HomeOutage, OrchestratedSession, ProjectSites, Run, RunState, RunTarget, RunTicket,
    SessionState,
};

/// The wave length assumed until a run of this owner has finished (minutes).
pub const DEFAULT_WAVE_MINUTES: u64 = 60;
/// Waves of a running timeline after Working Now: Next, Then, and Later for the rest.
const LATER_RANK: usize = 3;

/// Where the orchestration as a whole stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TimelinePhase {
    /// No target is declared, so there is nothing to orchestrate yet.
    NoProjects,
    /// Targets are declared but the plan has no step.
    NoPlan,
    /// A plan exists and nothing was started: every pass is a dry run.
    Proposed,
    /// The policy places sessions automatically.
    Running,
    /// Work was started before, and placement is now off.
    Paused,
}

/// One column of the timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaveKind {
    Done,
    WorkingNow,
    Next,
    Then,
    Later,
    /// A wave of a proposed plan; `ordinal` 0 is the first.
    Planned,
}

/// What an item's estimated start rests on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EstimateBasis {
    /// The work started; the time is when.
    Started,
    /// From the median lead time of this owner's finished runs.
    Measured,
    /// No run finished yet: [`DEFAULT_WAVE_MINUTES`] per wave.
    Default,
}

/// The state a timeline card shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemState {
    Running,
    /// Started ahead of a prerequisite's pending gate (§26.22).
    Provisional,
    /// Nothing holds it back; it starts when a session is free for it.
    Ready,
    WaitsForPrerequisite,
    /// The orchestrator asked a person for permission to act on it.
    WaitsForPermission,
    /// A judgment call waits on the goal ticket's `FEEDBACK NEEDED` note.
    WaitsForDecision,
    Done,
}

impl ItemState {
    pub fn code(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Provisional => "provisional",
            Self::Ready => "ready",
            Self::WaitsForPrerequisite => "waits_for_prerequisite",
            Self::WaitsForPermission => "waits_for_permission",
            Self::WaitsForDecision => "waits_for_decision",
            Self::Done => "done",
        }
    }
}

/// Where a target's checkout is, as this machine's reviewed binding says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetLocation {
    Local,
    Remote,
    /// No reviewed binding on this machine.
    Unbound,
}

/// A declared target as the timeline shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimelineTarget {
    pub target_id: String,
    pub label: String,
    pub location: TargetLocation,
    /// The label of the paired server a remote target lives on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine_label: Option<String>,
    /// The checkout the binding names, on this machine or on the remote server.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checkout_id: Option<String>,
}

/// The permission request a step waits on. HS2-J89JC7 reads it through one seam:
/// the scheduler's latest start decision for the step logged `permission_required`
/// with the orchestrator request id (HS2-XHQ1EH), still pending in the ledger; the
/// action is the one asked (`session_start`, or `work_start` for the step's run).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionWait {
    /// `permission`: an orchestrator permission request (HS2-XHQ1EH).
    pub kind: String,
    pub request_id: String,
    /// The gated action, such as `session_start`.
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_at_ms: Option<u64>,
}

/// An unanswered `FEEDBACK NEEDED` note on a goal ticket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionWait {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note_id: Option<String>,
}

/// A ticket's display identity as read from a store on this machine.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TicketLabel {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
}

/// What the latest logged scheduler decision about a step proposed (§26.17).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlacementHint {
    pub at_ms: u64,
    /// `start`, `keep` or `unplaced`.
    pub decision: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
    /// HS2-9FS7YX: the decision's detail code, such as `replacements_exhausted`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// HS2-9FS7YX: when the scheduler looks at the step again, for a held step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_at_ms: Option<u64>,
}

/// One plan step with what the readiness evaluation says of it.
#[derive(Debug, Clone)]
pub struct TimelineStep {
    pub readiness: StepReadiness,
    /// Work runs on it ahead of a pending gate (§26.22).
    pub provisional_work: bool,
    /// HS2-H5H9QP: the ledger record of that work, as its card shows it.
    pub provisional: Option<TimelineProvisional>,
}

/// HS2-H5H9QP: work started ahead of a pending gate, as a card's orchestration line
/// words it: the ledger state and each gate's prerequisite and result (§26.22).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimelineProvisional {
    /// `provisional`, `confirmed`, `at_risk` or `reopened`.
    pub state: String,
    pub gates: Vec<TimelineProvisionalGate>,
}

/// One gate provisional work rests on: the prerequisite ticket and the gate's result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimelineProvisionalGate {
    pub target_id: String,
    pub connection_id: String,
    pub native_id: String,
    /// Filled from the plan's ticket labels when the timeline is built.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    /// `pending`, `green`, `red`, `superseded`, `cancelled` or `unknown`.
    pub result: String,
}

/// Why Start Orchestration can or cannot be pressed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KickOff {
    /// Turning automatic placement on would be accepted by the policy route.
    pub can_start: bool,
    /// `no_projects`, `no_plan`, `project_software` (HS2-J3ZF27) or `running`; absent
    /// when it can start. No placement actor is needed: the orchestrator acts as the
    /// orchestration project (HS2-XYAHB4). HS2-D9KP86: no saved policy does not block
    /// it; see `default_policy`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_blocked: Option<String>,
    /// HS2-D9KP86: no policy is saved yet, so Start Orchestration first saves the
    /// default policy (every member at the server's default) with placement on.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub default_policy: bool,
    /// Placement is automatic, so it can be paused.
    pub can_pause: bool,
    /// The policy revision a change is made against.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_revision: Option<String>,
}

/// The policy facts the timeline needs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TimelinePolicy {
    pub present: bool,
    pub revision: Option<String>,
    pub automatic: bool,
    /// HS2-R2N33M: the policy leaves `placement.automatic` unset, so placement is
    /// Auto: on once the plan has a step.
    pub auto: bool,
    /// HS2-J3ZF27: the project's type is Software, so nothing is placed and Start
    /// would do nothing.
    pub software: bool,
    /// HS2-38R8J2: placement was on for this owner at some time on this machine.
    pub ever_started: bool,
}

pub struct TimelineInput<'a> {
    pub owner_checkout_id: &'a str,
    pub now_ms: u64,
    /// The owner has a target declaration (an orchestration project).
    pub declared: bool,
    /// Active declared targets, in declaration order.
    pub targets: &'a [TimelineTarget],
    pub steps: &'a [TimelineStep],
    pub runs: &'a [Run],
    /// HS2-6GE7WM: the runs the scheduler dispatched for each step, by step id. Such a
    /// run may be on another site of the step's project than the one the step names.
    pub step_runs: &'a BTreeMap<String, BTreeSet<String>>,
    /// HS2-89578K: each run's CI verdict, by run id, for the runs a CI gate stands for.
    pub run_ci: &'a BTreeMap<String, GateState>,
    /// HS2-CQ0PNT: which targets are sites of one project, so a run, a label and a
    /// prerequisite are joined to a step through whichever site names the ticket, as
    /// readiness joins them. An empty grouping compares targets exactly.
    pub sites: &'a ProjectSites,
    pub sessions: &'a [OrchestratedSession],
    pub hints: &'a BTreeMap<String, PlacementHint>,
    pub permissions: &'a BTreeMap<String, PermissionWait>,
    /// Goal tickets `(connection_id, native_id)` with an unanswered decision.
    pub decisions: &'a BTreeMap<(String, String), DecisionWait>,
    pub goals: &'a BTreeMap<(String, String), TicketLabel>,
    pub tickets: &'a BTreeMap<TicketKey, TicketLabel>,
    pub policy: &'a TimelinePolicy,
    /// HS2-EX594V: the homes of multi-site projects this machine's scheduler last
    /// found unable to coordinate their project, from its ledger.
    pub home_outages: &'a [HomeOutage],
}

/// A prerequisite ticket an item waits on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaitsOn {
    pub target_id: String,
    pub connection_id: String,
    pub native_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    /// The step that works that ticket, when one does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_id: Option<String>,
    /// The readiness reason code.
    pub reason: String,
}

/// The run of an item, as this machine recorded it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimelineRun {
    pub run_id: String,
    pub state: RunState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lease_expires_at_ms: Option<u64>,
    pub paused: bool,
    pub cancel_requested: bool,
    /// The target could not be confirmed since this time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unreachable_since_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_confirmed_at_ms: Option<u64>,
    pub created_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at_ms: Option<u64>,
    /// HS2-89578K: the run's CI verdict (its standing CI gates rolled up); absent when
    /// no CI gate stands for it. A completed ticket whose CI then failed is Done with
    /// `failed` here, which its card shows as needing repair.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ci_gate: Option<GateState>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimelineItem {
    pub step_id: String,
    pub goal: RunTicket,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal_slug: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal_title: Option<String>,
    pub target: RunTarget,
    #[serde(flatten)]
    pub ticket: TicketLabel,
    /// HS2-6GE7WM: the declared target (site) the work runs on: its run's site, which
    /// the scheduler may have chosen among the project's sites, or before a run the one
    /// the step names. `project_label`, `location` and `machine_label` describe it.
    #[serde(default)]
    pub site_id: String,
    pub project_label: String,
    pub location: TargetLocation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machine_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_state: Option<SessionState>,
    pub state: ItemState,
    pub level: ReadinessLevel,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub waits_on: Vec<WaitsOn>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<TimelineRun>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permission: Option<PermissionWait>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<DecisionWait>,
    /// The scheduler's latest word on the step, when it logged one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<PlacementHint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at_ms: Option<u64>,
    /// When work on it is expected to start; absent once started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_start_ms: Option<u64>,
    /// HS2-H5H9QP: the ledger record of work started on it ahead of a pending gate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provisional: Option<TimelineProvisional>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Wave {
    pub kind: WaveKind,
    /// The position of a planned wave (0 is the first); 0 for the others.
    pub ordinal: u32,
    /// Done has none; Working Now has when its earliest work started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimated_start_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimate: Option<EstimateBasis>,
    pub items: Vec<TimelineItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Timeline {
    /// Changes exactly when what the timeline shows changes; estimates that move
    /// with the clock alone do not change it. A reader waits with it as its cursor.
    pub version: String,
    pub now_ms: u64,
    /// The owner declares targets: it is an orchestration project, even with none yet.
    pub declared: bool,
    pub phase: TimelinePhase,
    pub automatic: bool,
    /// HS2-R2N33M: placement runs because the policy leaves it Auto and a plan exists,
    /// not because a person pressed Start Orchestration. Views say sessions will start
    /// and offer Pause.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub automatic_by_default: bool,
    pub kick_off: KickOff,
    /// HS2-38R8J2: placement was never turned on for this owner and the orchestrator
    /// has started no session for it, so a plan whose work was started by hand reads
    /// Not Started rather than Paused. Absent while running and once placement was ever
    /// on.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub never_started: bool,
    /// When the earliest unfinished run started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub running_since_ms: Option<u64>,
    /// Unfinished sessions this owner's orchestrator started.
    pub sessions: u32,
    pub targets: Vec<TimelineTarget>,
    /// Oldest first: Done, Working Now, Next, Then, Later; or the proposed waves
    /// First, Second, …, after Done when anything is finished.
    pub waves: Vec<Wave>,
    /// Items per state code.
    pub counts: BTreeMap<String, usize>,
    pub wave_minutes: u64,
    pub estimate: EstimateBasis,
    /// HS2-EX594V: each multi-site project whose home site is unavailable now, so no
    /// dispatcher or integrator starts for it anywhere. Absent when none is.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub home_outages: Vec<HomeOutage>,
}

fn finished_at(run: &Run) -> Option<u64> {
    run.state.is_terminal().then(|| {
        run.history
            .iter()
            .rev()
            .find(|entry| entry.to.is_terminal())
            .map_or(run.updated_at_ms, |entry| entry.at_ms)
    })
}

/// The median lead time of completed runs in minutes, at least one.
fn measured_minutes(runs: &[Run]) -> Option<u64> {
    let mut leads: Vec<u64> = runs
        .iter()
        .filter(|run| run.state == RunState::Completed)
        .filter_map(|run| Some(finished_at(run)?.saturating_sub(run.created_at_ms)))
        .collect();
    if leads.is_empty() {
        return None;
    }
    leads.sort_unstable();
    Some((leads[leads.len() / 2] / 60_000).max(1))
}

/// The most recent run of a step: one on its target ticket through any site of the
/// step's project (HS2-CQ0PNT), or one the scheduler dispatched for it on any site
/// (HS2-6GE7WM). An unfinished one first.
fn run_of<'a>(input: &TimelineInput<'a>, step_id: &str, target: &RunTarget) -> Option<&'a Run> {
    let placed = input.step_runs.get(step_id);
    input
        .runs
        .iter()
        .filter(|run| {
            run.target == *target
                || input.sites.same_ticket(&run.target, target)
                || placed.is_some_and(|placed| placed.contains(&run.run_id))
        })
        .max_by_key(|run| {
            (
                !run.state.is_terminal(),
                run.created_at_ms,
                run.run_id.clone(),
            )
        })
}

fn state_of(
    step: &TimelineStep,
    permission: Option<&PermissionWait>,
    decision: Option<&DecisionWait>,
) -> ItemState {
    match step.readiness.level {
        ReadinessLevel::Finished => ItemState::Done,
        ReadinessLevel::InProgress if step.provisional_work => ItemState::Provisional,
        ReadinessLevel::InProgress => ItemState::Running,
        _ if permission.is_some() => ItemState::WaitsForPermission,
        // HS2-XHQ1EH: going ahead on an unconfirmed prerequisite is a judgment call
        // asked on the goal ticket.
        ReadinessLevel::Provisional | ReadinessLevel::Stale if decision.is_some() => {
            ItemState::WaitsForDecision
        }
        ReadinessLevel::Confirmed => ItemState::Ready,
        ReadinessLevel::Provisional
        | ReadinessLevel::Blocked
        | ReadinessLevel::Stale
        | ReadinessLevel::Cyclic => ItemState::WaitsForPrerequisite,
    }
}

/// Each unfinished step's depth over the unfinished steps it waits on. A running
/// timeline counts running steps as 0 and a step with nothing unfinished before it
/// as 1; a proposed plan counts both as 0. A cycle sinks to the last wave.
fn ranks(
    steps: &[TimelineStep],
    running_timeline: bool,
    sites: &ProjectSites,
) -> BTreeMap<String, usize> {
    // HS2-CQ0PNT: one ticket of a project, whichever site names it.
    let by_ticket: BTreeMap<TicketKey, &TimelineStep> = steps
        .iter()
        .map(|step| (sites.ticket_key(&step.readiness.step.target), step))
        .collect();
    let open = |step: &TimelineStep| step.readiness.level != ReadinessLevel::Finished;
    let before: BTreeMap<&str, Vec<&str>> = steps
        .iter()
        .filter(|step| open(step))
        .map(|step| {
            let mut deps: Vec<&str> = step
                .readiness
                .step
                .prerequisites
                .iter()
                .filter_map(Prerequisite::ticket_key)
                .filter_map(|key| by_ticket.get(&sites.canonical(&key)).copied())
                .filter(|other| {
                    open(other) && other.readiness.step.step_id != step.readiness.step.step_id
                })
                .map(|other| other.readiness.step.step_id.as_str())
                .collect();
            deps.sort_unstable();
            deps.dedup();
            (step.readiness.step.step_id.as_str(), deps)
        })
        .collect();
    let running: BTreeSet<&str> = steps
        .iter()
        .filter(|step| step.readiness.level == ReadinessLevel::InProgress)
        .map(|step| step.readiness.step.step_id.as_str())
        .collect();
    let floor = usize::from(running_timeline);
    let mut memo: BTreeMap<String, usize> = BTreeMap::new();
    fn visit<'s>(
        id: &'s str,
        before: &BTreeMap<&'s str, Vec<&'s str>>,
        running: &BTreeSet<&'s str>,
        floor: usize,
        visiting: &mut BTreeSet<&'s str>,
        memo: &mut BTreeMap<String, usize>,
    ) -> usize {
        if let Some(rank) = memo.get(id) {
            return *rank;
        }
        if running.contains(id) {
            memo.insert(id.to_owned(), 0);
            return 0;
        }
        if !visiting.insert(id) {
            return usize::MAX;
        }
        let mut rank = floor;
        for dep in before.get(id).map(Vec::as_slice).unwrap_or_default() {
            let deeper = visit(dep, before, running, floor, visiting, memo);
            rank = rank.max(deeper.saturating_add(1));
        }
        visiting.remove(id);
        memo.insert(id.to_owned(), rank);
        rank
    }
    for id in before.keys() {
        let mut visiting = BTreeSet::new();
        visit(id, &before, &running, floor, &mut visiting, &mut memo);
    }
    // A cyclic step never resolves by itself, so it goes last.
    for step in steps {
        if step.readiness.level == ReadinessLevel::Cyclic {
            memo.insert(step.readiness.step.step_id.clone(), usize::MAX);
        }
    }
    memo
}

fn phase(input: &TimelineInput<'_>) -> TimelinePhase {
    if !input.declared || input.targets.is_empty() {
        TimelinePhase::NoProjects
    } else if input.steps.is_empty() {
        TimelinePhase::NoPlan
    } else if input.policy.automatic {
        TimelinePhase::Running
    } else if input.steps.iter().any(|step| {
        run_of(
            input,
            &step.readiness.step.step_id,
            &step.readiness.step.target,
        )
        .is_some()
    }) {
        TimelinePhase::Paused
    } else {
        TimelinePhase::Proposed
    }
}

fn kick_off(input: &TimelineInput<'_>, phase: TimelinePhase) -> KickOff {
    let blocked = match phase {
        TimelinePhase::NoProjects => Some("no_projects"),
        TimelinePhase::NoPlan => Some("no_plan"),
        TimelinePhase::Running => Some("running"),
        _ if input.policy.software => Some("project_software"),
        _ => None,
    };
    KickOff {
        can_start: blocked.is_none(),
        start_blocked: blocked.map(Into::into),
        default_policy: !input.policy.present,
        can_pause: input.policy.automatic,
        policy_revision: input.policy.revision.clone(),
    }
}

/// HS2-CQ0PNT: a ticket's label, read through the site that names it or, failing
/// that, through any other site of its project.
fn label_of<'a>(input: &TimelineInput<'a>, key: &TicketKey) -> Option<&'a TicketLabel> {
    input.tickets.get(key).or_else(|| {
        let canonical = input.sites.canonical(key);
        input
            .tickets
            .iter()
            .find(|(known, _)| input.sites.canonical(known) == canonical)
            .map(|(_, label)| label)
    })
}

fn item(
    input: &TimelineInput<'_>,
    step: &TimelineStep,
    by_ticket: &BTreeMap<TicketKey, String>,
) -> TimelineItem {
    let plan = &step.readiness.step;
    let goal_key = (
        plan.ticket.connection_id.clone(),
        plan.ticket.native_id.clone(),
    );
    let permission = input.permissions.get(&plan.step_id).filter(|_| {
        !matches!(
            step.readiness.level,
            ReadinessLevel::Finished | ReadinessLevel::InProgress
        )
    });
    let decision = input.decisions.get(&goal_key);
    let state = state_of(step, permission, decision);
    let run = run_of(input, &plan.step_id, &plan.target);
    // HS2-6GE7WM: where the work runs is the site of its run, which the scheduler may
    // have placed on another site of the project; before a run, the site the step names.
    let site = run.map_or(&plan.target.target_id, |run| &run.target.target_id);
    let target = input
        .targets
        .iter()
        .find(|target| target.target_id == *site);
    let session = run.and_then(|run| {
        input
            .sessions
            .iter()
            .filter(|session| session.run_id.as_deref() == Some(run.run_id.as_str()))
            .max_by_key(|session| (!session.state.is_terminal(), session.created_at_ms))
    });
    let hint = input.hints.get(&plan.step_id);
    let mut waits_on: Vec<WaitsOn> = Vec::new();
    if !matches!(
        state,
        ItemState::Done | ItemState::Running | ItemState::Provisional
    ) {
        for reason in &step.readiness.reasons {
            let Some(key) = reason
                .prerequisite
                .as_ref()
                .and_then(Prerequisite::ticket_key)
            else {
                continue;
            };
            if waits_on.iter().any(|known| {
                (&known.target_id, &known.connection_id, &known.native_id)
                    == (&key.0, &key.1, &key.2)
            }) {
                continue;
            }
            waits_on.push(WaitsOn {
                slug: label_of(input, &key).and_then(|label| label.slug.clone()),
                step_id: by_ticket.get(&input.sites.canonical(&key)).cloned(),
                reason: reason.code.clone(),
                target_id: key.0,
                connection_id: key.1,
                native_id: key.2,
            });
        }
    }
    let goal = input.goals.get(&goal_key);
    let live = run.filter(|run| !run.state.is_terminal());
    TimelineItem {
        step_id: plan.step_id.clone(),
        goal: plan.ticket.clone(),
        goal_slug: goal.and_then(|goal| goal.slug.clone()),
        goal_title: goal.and_then(|goal| goal.title.clone()),
        target: plan.target.clone(),
        ticket: label_of(input, &ticket_key(&plan.target))
            .cloned()
            .unwrap_or_default(),
        site_id: site.clone(),
        project_label: target.map_or_else(|| site.clone(), |target| target.label.clone()),
        location: target.map_or(TargetLocation::Unbound, |target| target.location),
        machine_label: target.and_then(|target| target.machine_label.clone()),
        tool: session
            .map(|session| session.tool.clone())
            .or_else(|| hint.and_then(|hint| hint.tool.clone()))
            // HS2-HCW7MD: the tool the plan step prefers.
            .or_else(|| plan.tool.clone()),
        model: session
            .and_then(|session| session.model.clone())
            .or_else(|| hint.and_then(|hint| hint.model.clone())),
        session_id: session.map(|session| session.session_id.clone()),
        session_state: session.map(|session| session.state),
        state,
        level: step.readiness.level,
        reasons: step
            .readiness
            .reasons
            .iter()
            .map(|reason| reason.code.clone())
            .collect(),
        waits_on,
        run: run.map(|run| TimelineRun {
            run_id: run.run_id.clone(),
            state: run.state,
            worker_id: run.worker_id.clone(),
            lease_expires_at_ms: run.lease_expires_at_ms,
            paused: run.paused.is_some(),
            cancel_requested: run.cancel_requested,
            unreachable_since_ms: run
                .unreachable
                .as_ref()
                .map(|unreachable| unreachable.since_ms),
            last_confirmed_at_ms: run.observed.as_ref().map(|observed| observed.at_ms),
            created_at_ms: run.created_at_ms,
            finished_at_ms: finished_at(run),
            ci_gate: input.run_ci.get(&run.run_id).copied(),
        }),
        permission: permission.cloned(),
        decision: (state == ItemState::WaitsForDecision)
            .then(|| decision.cloned())
            .flatten(),
        placement: hint.cloned(),
        // The live run's start, or for finished work the start of its last run.
        started_at_ms: live
            .or(run.filter(|_| state == ItemState::Done))
            .map(|run| run.created_at_ms),
        finished_at_ms: (state == ItemState::Done)
            .then(|| run.and_then(finished_at))
            .flatten(),
        estimated_start_ms: None,
        // Only work that is still running carries its record; finished work needs no remark.
        provisional: step
            .provisional
            .clone()
            .filter(|_| state == ItemState::Provisional)
            .map(|mut provisional| {
                for gate in &mut provisional.gates {
                    let key = (
                        gate.target_id.clone(),
                        gate.connection_id.clone(),
                        gate.native_id.clone(),
                    );
                    gate.slug = label_of(input, &key).and_then(|label| label.slug.clone());
                }
                provisional
            }),
    }
}

/// The version of what a timeline shows: its content without the clock.
fn version(timeline: &Timeline) -> String {
    let mut stable = timeline.clone();
    stable.now_ms = 0;
    for wave in &mut stable.waves {
        wave.estimated_start_ms = None;
        for item in &mut wave.items {
            item.estimated_start_ms = None;
        }
    }
    let bytes = serde_json::to_vec(&stable).unwrap_or_default();
    let digest = Sha256::digest(&bytes);
    digest[..12]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Arrange the plan into waves. See the module documentation.
pub fn timeline(input: &TimelineInput<'_>) -> Timeline {
    let phase = phase(input);
    let running_timeline = matches!(phase, TimelinePhase::Running | TimelinePhase::Paused);
    let ranks = ranks(input.steps, running_timeline, input.sites);
    let by_ticket: BTreeMap<TicketKey, String> = input
        .steps
        .iter()
        .map(|step| {
            (
                input.sites.ticket_key(&step.readiness.step.target),
                step.readiness.step.step_id.clone(),
            )
        })
        .collect();
    let order = |target_id: &str| {
        input
            .targets
            .iter()
            .position(|target| target.target_id == target_id)
            .unwrap_or(usize::MAX)
    };
    let mut done: Vec<TimelineItem> = Vec::new();
    let mut by_rank: BTreeMap<usize, Vec<TimelineItem>> = BTreeMap::new();
    if !matches!(phase, TimelinePhase::NoProjects | TimelinePhase::NoPlan) {
        for step in input.steps {
            let item = item(input, step, &by_ticket);
            if item.state == ItemState::Done {
                done.push(item);
                continue;
            }
            let rank = ranks
                .get(&step.readiness.step.step_id)
                .copied()
                .unwrap_or(usize::MAX);
            let rank = if running_timeline {
                rank.min(LATER_RANK)
            } else {
                rank
            };
            by_rank.entry(rank).or_default().push(item);
        }
    }
    let sort = |items: &mut Vec<TimelineItem>| {
        items.sort_by(|a, b| {
            // HS2-6GE7WM: by the site the work runs on, so each site's cards gather.
            (order(&a.site_id), &a.step_id).cmp(&(order(&b.site_id), &b.step_id))
        });
    };
    let measured = measured_minutes(input.runs);
    let minutes = measured.unwrap_or(DEFAULT_WAVE_MINUTES);
    let basis = if measured.is_some() {
        EstimateBasis::Measured
    } else {
        EstimateBasis::Default
    };
    let wave_ms = minutes * 60_000;
    let mut waves: Vec<Wave> = Vec::new();
    if running_timeline || !done.is_empty() {
        sort(&mut done);
        waves.push(Wave {
            kind: WaveKind::Done,
            ordinal: 0,
            estimated_start_ms: None,
            estimate: None,
            items: done,
        });
    }
    if running_timeline {
        let mut working = by_rank.remove(&0).unwrap_or_default();
        sort(&mut working);
        let started = working.iter().filter_map(|item| item.started_at_ms).min();
        // Next starts when the earliest working item is expected to finish, or now.
        let next_ms = started.map_or(input.now_ms, |started| {
            (started + wave_ms).max(input.now_ms)
        });
        waves.push(Wave {
            kind: WaveKind::WorkingNow,
            ordinal: 0,
            estimated_start_ms: started,
            estimate: started.map(|_| EstimateBasis::Started),
            items: working,
        });
        for (offset, kind) in [WaveKind::Next, WaveKind::Then, WaveKind::Later]
            .into_iter()
            .enumerate()
        {
            let mut items = by_rank.remove(&(offset + 1)).unwrap_or_default();
            sort(&mut items);
            let at = next_ms + wave_ms * offset as u64;
            for item in &mut items {
                item.estimated_start_ms = Some(at);
            }
            waves.push(Wave {
                kind,
                ordinal: 0,
                estimated_start_ms: Some(at),
                estimate: Some(basis),
                items,
            });
        }
    } else {
        // Proposed: First starts on kick-off (now), then one wave length apart.
        let ranks: Vec<usize> = by_rank.keys().copied().collect();
        for (ordinal, rank) in ranks.into_iter().enumerate() {
            let mut items = by_rank.remove(&rank).unwrap_or_default();
            sort(&mut items);
            let at = input.now_ms + wave_ms * ordinal as u64;
            for item in &mut items {
                item.estimated_start_ms = Some(at);
            }
            waves.push(Wave {
                kind: WaveKind::Planned,
                ordinal: u32::try_from(ordinal).unwrap_or(u32::MAX),
                estimated_start_ms: Some(at),
                estimate: Some(basis),
                items,
            });
        }
    }
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for item in waves.iter().flat_map(|wave| &wave.items) {
        *counts.entry(item.state.code().to_owned()).or_default() += 1;
    }
    let mut timeline = Timeline {
        version: String::new(),
        now_ms: input.now_ms,
        declared: input.declared,
        phase,
        automatic: input.policy.automatic,
        automatic_by_default: input.policy.automatic && input.policy.auto,
        // A session the orchestrator started (such as an integrator) means it did start.
        never_started: phase != TimelinePhase::Running
            && !input.policy.ever_started
            && input.sessions.is_empty(),
        kick_off: kick_off(input, phase),
        running_since_ms: input
            .runs
            .iter()
            .filter(|run| !run.state.is_terminal())
            .map(|run| run.created_at_ms)
            .min(),
        sessions: u32::try_from(
            input
                .sessions
                .iter()
                .filter(|session| !session.state.is_terminal())
                .count(),
        )
        .unwrap_or(u32::MAX),
        targets: input.targets.to_vec(),
        waves,
        counts,
        wave_minutes: minutes,
        estimate: basis,
        home_outages: input
            .home_outages
            .iter()
            .filter(|outage| outage.current())
            .cloned()
            .collect(),
    };
    timeline.version = version(&timeline);
    timeline
}
