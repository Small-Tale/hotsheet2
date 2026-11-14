//! HS2-42TARA: prerequisites between orchestration steps and qualified target
//! tickets, and the readiness computed from them.
//!
//! A plan step is one orchestration ticket's work on one exact target ticket. Its
//! prerequisites name other qualified target tickets (with the gates that must be
//! green) and validation steps. Nothing here rewrites or reads a provider's own
//! dependency fields: an edge exists because a step's prerequisite names the ticket
//! another step works. Evaluation is a pure function over typed facts the runtime
//! gathered from this machine's records.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::contract::{valid_identifier, valid_token};
use super::reconcile::ticket_finished;
use super::storage::{Storage, decode};
use super::*;

/// Steps one owning checkout may plan.
pub const MAX_PLAN_STEPS: usize = 256;
pub const MAX_STEP_PLAN_PREREQUISITES: usize = 16;
const MAX_PLAN_STEPS_TOTAL: usize = 2048;
const MAX_PLAN_BYTES: usize = 8 * 1024 * 1024;
const MAX_NATIVE_ID_BYTES: usize = 256;

/// Which gates of a prerequisite ticket must be green.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum GateRequirement {
    /// The ticket being confirmed finished is enough.
    None,
    /// Every standing CI gate passed. The default.
    #[default]
    Ci,
    /// CI passed and the requested deployment passed.
    Deployment,
}

/// Something that must be true before a step's work is dispatched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Prerequisite {
    /// A qualified target ticket is confirmed finished and its gates are green. The
    /// gates are those of the latest run on that ticket, or of `commit` when named.
    Ticket {
        target_id: String,
        connection_id: String,
        native_id: String,
        #[serde(default)]
        gates: GateRequirement,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        commit: Option<String>,
    },
    /// The latest evidence of a validation step (§26.12) is `passed`.
    Validation { ticket: RunTicket, step_id: String },
}

impl Prerequisite {
    /// The qualified ticket a `ticket` prerequisite names.
    pub fn ticket_key(&self) -> Option<TicketKey> {
        match self {
            Self::Ticket {
                target_id,
                connection_id,
                native_id,
                ..
            } => Some((target_id.clone(), connection_id.clone(), native_id.clone())),
            Self::Validation { .. } => None,
        }
    }
}

/// `(target_id, connection_id, native_id)`: a ticket qualified by its target project.
pub type TicketKey = (String, String, String);

pub fn ticket_key(target: &RunTarget) -> TicketKey {
    (
        target.target_id.clone(),
        target.connection_id.clone(),
        target.native_id.clone(),
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanStep {
    pub step_id: String,
    pub owner_checkout_id: String,
    /// The orchestration ticket the work is for.
    pub ticket: RunTicket,
    /// The exact target ticket the step works. One step per target ticket.
    pub target: RunTarget,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prerequisites: Vec<Prerequisite>,
    /// HS2-2FG91X: resources the session working this step must hold, sorted. They
    /// are not prerequisites: readiness ignores them and the scheduler waits on them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<ResourceNeed>,
    /// HS2-HCW7MD: the AI tool the planner would prefer for this work, by plugin id.
    /// A preference only: the scheduler tries it first (HS2-TH53NN).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// HS2-TH53NN: the model preferred for this work, with `tool` or any tool that
    /// offers it. A preference only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Starts at 1 and grows with every changed definition.
    pub revision: u32,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_by: Option<String>,
}

/// What a caller supplies to plan a step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPlanStep {
    pub ticket: RunTicket,
    pub target: RunTarget,
    pub prerequisites: Vec<Prerequisite>,
    pub resources: Vec<ResourceNeed>,
    /// HS2-HCW7MD: the preferred AI tool, if any.
    pub tool: Option<String>,
    /// HS2-TH53NN: the preferred model, if any.
    pub model: Option<String>,
}

/// A target ticket as of one successful read. Codes and identifiers only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TicketSeen {
    pub at_ms: u64,
    /// False when the target answered that it has no such ticket.
    pub exists: bool,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
    /// Absent when the read did not say, as with a run's observation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub up_next: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at_ms: Option<u64>,
    /// HS2-JSF985: whether the ticket carried the speculation opt-out tag. Absent
    /// when the read did not say, which never counts as "not opted out".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opted_out: Option<bool>,
    /// HS2-J89JC7: the ticket's display slug, an identifier for the timeline. Absent
    /// when the read did not say.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
}

/// The last confirmed state of one target ticket and whether it can still be trusted.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TicketFacts {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed: Option<TicketSeen>,
    /// Set while the target could not be confirmed; `observed` is then stale.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unreachable: Option<RunUnreachable>,
}

impl TicketFacts {
    /// Combine two accounts of the same ticket. The newer read wins, and `other` wins
    /// a tie, so a caller passes the live account second. The priority inputs only
    /// the older read may hold are kept. An outage counts only when it began at or
    /// after the newest read, so a late report of an old outage never makes a
    /// fresher read stale.
    pub fn merge(self, other: Self) -> Self {
        let observed = match (self.observed, other.observed) {
            (Some(a), Some(b)) => {
                let (mut newer, older) = if b.at_ms >= a.at_ms { (b, a) } else { (a, b) };
                if newer.exists {
                    newer.priority = newer.priority.or(older.priority);
                    newer.up_next = newer.up_next.or(older.up_next);
                    newer.created_at_ms = newer.created_at_ms.or(older.created_at_ms);
                    newer.opted_out = newer.opted_out.or(older.opted_out);
                    newer.slug = newer.slug.or(older.slug);
                }
                Some(newer)
            }
            (a, b) => a.or(b),
        };
        let unreachable = match (self.unreachable, other.unreachable) {
            (Some(a), Some(b)) => Some(if b.since_ms > a.since_ms { b } else { a }),
            (a, b) => a.or(b),
        }
        .filter(|down| {
            observed
                .as_ref()
                .is_none_or(|seen| down.since_ms >= seen.at_ms)
        });
        Self {
            observed,
            unreachable,
        }
    }

    /// The read that can be relied on now, or the code of why none can.
    fn fresh(&self) -> Result<&TicketSeen, &'static str> {
        match (&self.unreachable, &self.observed) {
            (Some(_), _) => Err("target_unreachable"),
            (None, None) => Err("not_observed"),
            (None, Some(seen)) => Ok(seen),
        }
    }
}

/// The rolled-up gates a prerequisite is judged by (§26.13). Superseded gates are
/// already left out by the roll-up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateFacts {
    pub ci: GateState,
    pub ci_reason: Option<String>,
    pub deployment: Option<GateState>,
    /// The commits the standing CI gates are about.
    pub commits: Vec<String>,
    /// HS2-JSF985: the standing CI gates themselves, oldest link first.
    pub gate_ids: Vec<String>,
}

impl GateFacts {
    /// No gate stands.
    pub fn none() -> Self {
        Self::from(&summarize(Vec::new()))
    }
}

impl From<&GateSummary> for GateFacts {
    fn from(summary: &GateSummary) -> Self {
        Self {
            ci: summary.ci,
            ci_reason: summary.ci_reason.clone(),
            deployment: summary.deployment,
            commits: summary.commits.clone(),
            gate_ids: summary
                .gates
                .iter()
                .filter(|gate| gate.kind == GateKind::Ci)
                .map(|gate| gate.gate_id.clone())
                .collect(),
        }
    }
}

/// Everything evaluation reads. The runtime fills it from this machine's records.
#[derive(Debug, Clone, Default)]
pub struct PlanFacts {
    pub tickets: BTreeMap<TicketKey, TicketFacts>,
    /// Tickets an unfinished run stands on.
    pub active: BTreeSet<TicketKey>,
    /// The gates of the latest run on a ticket.
    pub gates: BTreeMap<TicketKey, GateFacts>,
    /// The gates of one target commit: `(target_id, commit)`.
    pub commit_gates: BTreeMap<(String, String), GateFacts>,
    /// The evidence of a validation step: `(connection_id, native_id, step_id)` of
    /// the orchestration ticket. Absent when the step is not defined.
    pub validations: BTreeMap<(String, String, String), (ValidationState, Option<String>)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessLevel {
    /// Every prerequisite is confirmed: tickets finished, gates green, evidence
    /// passed. The only level work is dispatched on by default.
    Confirmed,
    /// Nothing is against the step, but at least one prerequisite rests on an
    /// integrated commit whose gate is still pending or whose ticket is not yet
    /// confirmed finished. `base_commits` names those commits.
    Provisional,
    /// A prerequisite is confirmed not to hold.
    Blocked,
    /// Nothing is known to be against the step, but something it rests on could not
    /// be confirmed.
    Stale,
    /// The step's prerequisites lead back to itself.
    Cyclic,
    /// An unfinished run already stands on the step's ticket.
    InProgress,
    /// The step's own ticket is confirmed finished.
    Finished,
}

impl ReadinessLevel {
    pub const ALL: [Self; 7] = [
        Self::Confirmed,
        Self::Provisional,
        Self::Blocked,
        Self::Stale,
        Self::Cyclic,
        Self::InProgress,
        Self::Finished,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Self::Confirmed => "confirmed",
            Self::Provisional => "provisional",
            Self::Blocked => "blocked",
            Self::Stale => "stale",
            Self::Cyclic => "cyclic",
            Self::InProgress => "in_progress",
            Self::Finished => "finished",
        }
    }

    /// Work exists that could be started at some level.
    pub fn is_ready(self) -> bool {
        matches!(self, Self::Confirmed | Self::Provisional)
    }

    /// Why a dispatch is refused at this level; `None` when it is not.
    pub fn refusal(self) -> Option<ReadinessError> {
        match self {
            Self::Confirmed | Self::InProgress | Self::Finished => None,
            Self::Provisional => Some(ReadinessError::Provisional),
            Self::Blocked => Some(ReadinessError::Blocked),
            Self::Stale => Some(ReadinessError::Stale),
            Self::Cyclic => Some(ReadinessError::Cyclic),
        }
    }
}

/// One thing that keeps a step from the confirmed level. A stable code, the
/// prerequisite it is about (absent when it is about the step itself) and, for a
/// provisional prerequisite, the commits it rests on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadinessReason {
    pub code: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prerequisite: Option<Prerequisite>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commits: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepReadiness {
    pub step: PlanStep,
    pub level: ReadinessLevel,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<ReadinessReason>,
    /// The recorded base commits of a provisional step, sorted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub base_commits: Vec<String>,
    /// Unfinished steps that name this step's ticket as a prerequisite, sorted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unblocks: Vec<String>,
    /// The steps of the cycle a cyclic step is part of, itself included, sorted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cycle: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessError {
    StepNotFound,
    /// A dispatch was refused at this level. A person may override it.
    Blocked,
    Stale,
    Cyclic,
    /// Only prerequisites with a pending gate stand in the way; dispatching on it is
    /// a policy decision this machine does not take yet.
    Provisional,
    /// A person answered no to the override of this exact operation.
    OverrideDenied,
    /// HS2-JSF985: the policies allow provisional work, and the provisional rule
    /// refuses this step. `readiness show` names the reason.
    ProvisionalRefused,
    /// HS2-JSF985: the step has no provisional work that could be reopened.
    NotProvisional,
    /// HS2-HCW7MD: Propose Plan found no declared project to plan work in.
    NoTargets,
    /// HS2-HCW7MD: Propose Plan found no open goal ticket to plan.
    NoGoals,
    /// HS2-HCW7MD: the goal ticket named for Propose Plan is not open.
    GoalNotOpen,
}

impl ReadinessError {
    pub fn code(self) -> &'static str {
        match self {
            Self::StepNotFound => "readiness_step_not_found",
            Self::Blocked => "readiness_blocked",
            Self::Stale => "readiness_stale",
            Self::Cyclic => "readiness_cyclic",
            Self::Provisional => "readiness_provisional",
            Self::OverrideDenied => "readiness_override_denied",
            Self::ProvisionalRefused => "readiness_provisional_refused",
            Self::NotProvisional => "readiness_not_provisional",
            Self::NoTargets => "plan_no_targets",
            Self::NoGoals => "plan_no_goals",
            Self::GoalNotOpen => "plan_goal_not_open",
        }
    }
}

pub(super) enum Verdict {
    Satisfied,
    Provisional(&'static str, Vec<String>),
    Blocked(&'static str),
    Stale(&'static str),
}

fn ticket_verdict(
    key: &TicketKey,
    gates: GateRequirement,
    commit: Option<&String>,
    facts: &PlanFacts,
) -> Verdict {
    let seen = match facts.tickets.get(key).map(TicketFacts::fresh) {
        None => return Verdict::Stale("not_observed"),
        Some(Err(code)) => return Verdict::Stale(code),
        Some(Ok(seen)) => seen,
    };
    if !seen.exists {
        return Verdict::Blocked("prerequisite_ticket_missing");
    }
    let finished = ticket_finished(&seen.status);
    if gates == GateRequirement::None {
        return if finished {
            Verdict::Satisfied
        } else {
            Verdict::Blocked("prerequisite_not_finished")
        };
    }
    let none = GateFacts::none();
    let gate = match commit {
        Some(commit) => facts.commit_gates.get(&(key.0.clone(), commit.clone())),
        None => facts.gates.get(key),
    }
    .unwrap_or(&none);
    // The commits a provisional answer rests on. Without one there is no recorded
    // base, so nothing can be built on it.
    let base = || match commit {
        Some(commit) => vec![commit.clone()],
        None => gate.commits.clone(),
    };
    match gate.ci {
        GateState::Failed => return Verdict::Blocked("gate_failed"),
        GateState::Pending if !base().is_empty() => {
            return Verdict::Provisional("gate_pending", base());
        }
        GateState::Passed => {}
        _ if !finished => return Verdict::Blocked("prerequisite_not_finished"),
        GateState::Unknown if gate.ci_reason.as_deref() == Some("no_gate") => {
            return Verdict::Blocked("gate_not_linked");
        }
        _ => return Verdict::Stale("gate_unknown"),
    }
    if gates == GateRequirement::Deployment {
        match gate.deployment {
            None => return Verdict::Blocked("deployment_not_requested"),
            Some(GateState::Passed) => {}
            Some(GateState::Failed) => return Verdict::Blocked("deployment_failed"),
            Some(GateState::Pending | GateState::AwaitingApproval) => {
                return Verdict::Blocked("deployment_pending");
            }
            Some(GateState::Unknown | GateState::Superseded) => {
                return Verdict::Stale("deployment_unknown");
            }
        }
    }
    if finished {
        Verdict::Satisfied
    } else if base().is_empty() {
        Verdict::Blocked("prerequisite_not_finished")
    } else {
        // The integrated commit is green; only the ticket's own closure is unconfirmed.
        Verdict::Provisional("prerequisite_not_finished", base())
    }
}

pub(super) fn verdict(prerequisite: &Prerequisite, facts: &PlanFacts) -> Verdict {
    match prerequisite {
        Prerequisite::Ticket { gates, commit, .. } => ticket_verdict(
            &prerequisite.ticket_key().unwrap_or_default(),
            *gates,
            commit.as_ref(),
            facts,
        ),
        Prerequisite::Validation { ticket, step_id } => match facts.validations.get(&(
            ticket.connection_id.clone(),
            ticket.native_id.clone(),
            step_id.clone(),
        )) {
            None => Verdict::Blocked("validation_step_missing"),
            Some((ValidationState::Passed, _)) => Verdict::Satisfied,
            Some((ValidationState::Failed, _)) => Verdict::Blocked("validation_failed"),
            Some((ValidationState::Running, _)) => Verdict::Blocked("validation_running"),
            Some((ValidationState::Skipped, _)) => Verdict::Blocked("validation_skipped"),
            Some((ValidationState::Unavailable, _)) => Verdict::Blocked("validation_unavailable"),
            Some((ValidationState::Cancelled, _)) => Verdict::Blocked("validation_cancelled"),
            Some((ValidationState::Unverified, reason)) => match reason.as_deref() {
                Some("not_run") => Verdict::Blocked("validation_not_run"),
                // Evidence for an earlier definition proves nothing about this one.
                Some("step_changed") => Verdict::Blocked("validation_step_changed"),
                _ => Verdict::Stale("validation_unverified"),
            },
        },
    }
}

/// For each step, the steps it can reach through prerequisites (itself only when a
/// path leads back).
fn reachable(edges: &[Vec<usize>]) -> Vec<BTreeSet<usize>> {
    (0..edges.len())
        .map(|start| {
            let mut seen = BTreeSet::new();
            let mut stack = edges[start].clone();
            while let Some(next) = stack.pop() {
                if seen.insert(next) {
                    stack.extend(edges[next].iter().copied());
                }
            }
            seen
        })
        .collect()
}

/// The readiness of every step of one plan, ordered by step id. Deterministic: the
/// same steps and facts give the same answer whatever order they arrive in.
pub fn evaluate_plan(steps: &[PlanStep], facts: &PlanFacts) -> Vec<StepReadiness> {
    let mut steps: Vec<&PlanStep> = steps.iter().collect();
    steps.sort_by(|a, b| a.step_id.cmp(&b.step_id));
    let by_ticket: BTreeMap<TicketKey, usize> = steps
        .iter()
        .enumerate()
        .map(|(index, step)| (ticket_key(&step.target), index))
        .collect();
    let edges: Vec<Vec<usize>> = steps
        .iter()
        .map(|step| {
            step.prerequisites
                .iter()
                .filter_map(Prerequisite::ticket_key)
                .filter_map(|key| by_ticket.get(&key).copied())
                .collect()
        })
        .collect();
    let reach = reachable(&edges);
    let mut answers: Vec<StepReadiness> = steps
        .iter()
        .enumerate()
        .map(|(index, step)| {
            let key = ticket_key(&step.target);
            let mut answer = StepReadiness {
                step: (*step).clone(),
                level: ReadinessLevel::Confirmed,
                reasons: Vec::new(),
                base_commits: Vec::new(),
                unblocks: Vec::new(),
                cycle: Vec::new(),
            };
            let own = |code: &str| ReadinessReason {
                code: code.into(),
                prerequisite: None,
                commits: Vec::new(),
            };
            if facts.active.contains(&key) {
                answer.level = ReadinessLevel::InProgress;
                return answer;
            }
            let seen = facts
                .tickets
                .get(&key)
                .map_or(Err("not_observed"), TicketFacts::fresh);
            if seen.is_ok_and(|seen| seen.exists && ticket_finished(&seen.status)) {
                answer.level = ReadinessLevel::Finished;
                return answer;
            }
            if reach[index].contains(&index) {
                answer.level = ReadinessLevel::Cyclic;
                answer.reasons.push(own("plan_cycle"));
                answer.cycle = reach[index]
                    .iter()
                    .filter(|other| reach[**other].contains(&index))
                    .map(|other| steps[*other].step_id.clone())
                    .collect();
                answer.cycle.sort();
                return answer;
            }
            match seen {
                Err(code) => {
                    answer.level = ReadinessLevel::Stale;
                    answer.reasons.push(own(code));
                    return answer;
                }
                Ok(seen) if !seen.exists => {
                    answer.level = ReadinessLevel::Blocked;
                    answer.reasons.push(own("target_ticket_missing"));
                    return answer;
                }
                Ok(_) => {}
            }
            let (mut blocked, mut stale) = (false, false);
            for prerequisite in &step.prerequisites {
                let (code, commits) = match verdict(prerequisite, facts) {
                    Verdict::Satisfied => continue,
                    Verdict::Provisional(code, commits) => (code, commits),
                    Verdict::Blocked(code) => {
                        blocked = true;
                        // A prerequisite inside a cycle cannot be reached by planned work.
                        let cyclic = prerequisite
                            .ticket_key()
                            .and_then(|key| by_ticket.get(&key))
                            .is_some_and(|other| reach[*other].contains(other));
                        (
                            if cyclic {
                                "prerequisite_in_cycle"
                            } else {
                                code
                            },
                            Vec::new(),
                        )
                    }
                    Verdict::Stale(code) => {
                        stale = true;
                        (code, Vec::new())
                    }
                };
                for commit in &commits {
                    if !answer.base_commits.contains(commit) {
                        answer.base_commits.push(commit.clone());
                    }
                }
                answer.reasons.push(ReadinessReason {
                    code: code.into(),
                    prerequisite: Some(prerequisite.clone()),
                    commits,
                });
            }
            answer.level = if blocked {
                ReadinessLevel::Blocked
            } else if stale {
                ReadinessLevel::Stale
            } else if answer.reasons.is_empty() {
                ReadinessLevel::Confirmed
            } else {
                ReadinessLevel::Provisional
            };
            if answer.level != ReadinessLevel::Provisional {
                answer.base_commits.clear();
            }
            answer.base_commits.sort();
            answer
        })
        .collect();
    let finished: Vec<bool> = answers
        .iter()
        .map(|answer| answer.level == ReadinessLevel::Finished)
        .collect();
    for (index, answer) in answers.iter_mut().enumerate() {
        answer.unblocks = edges
            .iter()
            .enumerate()
            .filter(|(other, edges)| !finished[*other] && edges.contains(&index))
            .map(|(other, _)| steps[other].step_id.clone())
            .collect();
    }
    answers
}

/// The level last published for a step, so a change is reported once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublishedLevel {
    pub level: ReadinessLevel,
    pub since_ms: u64,
    /// Counts this step's level changes; it identifies the change as an event.
    pub seq: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanRecord {
    pub step: PlanStep,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published: Option<PublishedLevel>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TicketRecord {
    pub owner_checkout_id: String,
    pub target: RunTarget,
    pub facts: TicketFacts,
}

/// A step whose published level changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LevelChange {
    pub step_id: String,
    pub from: Option<ReadinessLevel>,
    pub to: ReadinessLevel,
    pub seq: u64,
}

/// A newer account of one ticket. `Some(None)` records that the target answered.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TicketUpdate {
    pub observed: Option<TicketSeen>,
    pub unreachable: Option<Option<RunUnreachable>>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlanFile {
    schema_version: u32,
    steps: Vec<PlanRecord>,
    #[serde(default)]
    tickets: Vec<TicketRecord>,
}

/// The machine-local plan: steps, the tickets read for them, and published levels.
#[derive(Clone)]
pub struct PlanRegistry {
    storage: Storage,
}

impl PlanRegistry {
    pub fn new(home: &Path) -> Self {
        Self {
            storage: Storage::machine_named(home, "orchestration-plan.json"),
        }
    }

    fn read_file(&self) -> Result<PlanFile, OrchestrationError> {
        let Some(bytes) = self.storage.read(MAX_PLAN_BYTES)? else {
            return Ok(PlanFile {
                schema_version: ORCHESTRATION_VERSION,
                steps: Vec::new(),
                tickets: Vec::new(),
            });
        };
        let file: PlanFile = decode(&bytes)?;
        for record in &file.steps {
            validate_step(&record.step).map_err(|_| OrchestrationError::Corrupt)?;
        }
        for record in &file.tickets {
            valid_identifier(&record.owner_checkout_id)
                .and_then(|()| validate_target(&record.target))
                .map_err(|_| OrchestrationError::Corrupt)?;
        }
        Ok(file)
    }

    fn write_file(&self, file: &PlanFile) -> Result<(), OrchestrationError> {
        let bytes = serde_json::to_vec(file).map_err(|_| OrchestrationError::Corrupt)?;
        self.storage.write(&bytes, MAX_PLAN_BYTES)
    }

    /// True when the owner plans nothing. Cheap enough to ask on every event.
    pub fn is_empty(&self, owner_checkout_id: &str) -> Result<bool, OrchestrationError> {
        Ok(self.records(owner_checkout_id)?.is_empty())
    }

    /// The owner's steps with their published levels, ordered by step id.
    pub fn records(&self, owner_checkout_id: &str) -> Result<Vec<PlanRecord>, OrchestrationError> {
        valid_identifier(owner_checkout_id)?;
        let mut records: Vec<PlanRecord> = self
            .read_file()?
            .steps
            .into_iter()
            .filter(|record| record.step.owner_checkout_id == owner_checkout_id)
            .collect();
        records.sort_by(|a, b| a.step.step_id.cmp(&b.step.step_id));
        Ok(records)
    }

    pub fn get(
        &self,
        owner_checkout_id: &str,
        step_id: &str,
    ) -> Result<PlanStep, OrchestrationError> {
        valid_token(step_id)?;
        self.records(owner_checkout_id)?
            .into_iter()
            .find(|record| record.step.step_id == step_id)
            .map(|record| record.step)
            .ok_or(OrchestrationError::Readiness(ReadinessError::StepNotFound))
    }

    /// The step that works this exact target ticket, when one is planned.
    pub fn find_target(
        &self,
        owner_checkout_id: &str,
        target: &RunTarget,
    ) -> Result<Option<PlanStep>, OrchestrationError> {
        Ok(self
            .records(owner_checkout_id)?
            .into_iter()
            .find(|record| record.step.target == *target)
            .map(|record| record.step))
    }

    /// Plan a step, or change the one that already works the same target ticket. The
    /// same definition again changes nothing; a changed one gets the next revision.
    /// The same target ticket for another orchestration ticket conflicts.
    pub fn define(
        &self,
        owner_checkout_id: &str,
        new: NewPlanStep,
        actor_id: Option<String>,
        tx: &mut Transaction<'_>,
    ) -> Result<(PlanStep, bool), OrchestrationError> {
        self.define_observed(owner_checkout_id, new, Vec::new(), actor_id, tx)
    }

    /// HS2-E7HR4R: [`Self::define`] together with the reads of the tickets the step
    /// names, in one write. A step written without them, and observed a moment later,
    /// could be evaluated in between with a prerequisite not yet observed and published
    /// as `stale`. Returns whether the step itself changed.
    pub fn define_observed(
        &self,
        owner_checkout_id: &str,
        mut new: NewPlanStep,
        observed: Vec<(RunTarget, TicketUpdate)>,
        actor_id: Option<String>,
        tx: &mut Transaction<'_>,
    ) -> Result<(PlanStep, bool), OrchestrationError> {
        valid_identifier(owner_checkout_id)?;
        for (target, update) in &observed {
            validate_update(target, update)?;
        }
        new.resources.sort();
        let now_ms = tx.clock.now_ms();
        let _lock = self.storage.lock()?;
        let mut file = self.read_file()?;
        let existing = file.steps.iter().position(|record| {
            record.step.owner_checkout_id == owner_checkout_id && record.step.target == new.target
        });
        let step = match existing {
            Some(index) => {
                let step = &mut file.steps[index].step;
                if step.ticket != new.ticket {
                    return Err(OrchestrationError::Conflict);
                }
                if step.prerequisites == new.prerequisites
                    && step.resources == new.resources
                    && step.tool == new.tool
                    && step.model == new.model
                {
                    let step = step.clone();
                    let mut seen = false;
                    for (target, update) in observed {
                        seen |= apply_observation(&mut file, owner_checkout_id, &target, update);
                    }
                    if seen {
                        self.write_file(&file)?;
                    }
                    return Ok((step, false));
                }
                step.prerequisites = new.prerequisites;
                step.resources = new.resources;
                step.tool = new.tool;
                step.model = new.model;
                step.revision = step.revision.saturating_add(1);
                step.updated_at_ms = now_ms;
                step.updated_by = actor_id;
                validate_step(step)?;
                step.clone()
            }
            None => {
                let planned = file
                    .steps
                    .iter()
                    .filter(|record| record.step.owner_checkout_id == owner_checkout_id)
                    .count();
                if planned >= MAX_PLAN_STEPS || file.steps.len() >= MAX_PLAN_STEPS_TOTAL {
                    return Err(OrchestrationError::Capacity);
                }
                let step = PlanStep {
                    step_id: tx.token(),
                    owner_checkout_id: owner_checkout_id.into(),
                    ticket: new.ticket,
                    target: new.target,
                    prerequisites: new.prerequisites,
                    resources: new.resources,
                    tool: new.tool,
                    model: new.model,
                    revision: 1,
                    created_at_ms: now_ms,
                    updated_at_ms: now_ms,
                    updated_by: actor_id,
                };
                validate_step(&step)?;
                file.steps.push(PlanRecord {
                    step: step.clone(),
                    published: None,
                });
                step
            }
        };
        prune_tickets(&mut file);
        for (target, update) in observed {
            apply_observation(&mut file, owner_checkout_id, &target, update);
        }
        self.write_file(&file)?;
        Ok((step, true))
    }

    /// Withdraw a step. Readings kept only for it are forgotten.
    pub fn remove(
        &self,
        owner_checkout_id: &str,
        step_id: &str,
    ) -> Result<PlanStep, OrchestrationError> {
        valid_identifier(owner_checkout_id)?;
        valid_token(step_id)?;
        let _lock = self.storage.lock()?;
        let mut file = self.read_file()?;
        let index = file
            .steps
            .iter()
            .position(|record| {
                record.step.owner_checkout_id == owner_checkout_id && record.step.step_id == step_id
            })
            .ok_or(OrchestrationError::Readiness(ReadinessError::StepNotFound))?;
        let removed = file.steps.remove(index).step;
        prune_tickets(&mut file);
        self.write_file(&file)?;
        Ok(removed)
    }

    /// The tickets read for the owner's plan.
    pub fn tickets(
        &self,
        owner_checkout_id: &str,
    ) -> Result<Vec<TicketRecord>, OrchestrationError> {
        valid_identifier(owner_checkout_id)?;
        Ok(self
            .read_file()?
            .tickets
            .into_iter()
            .filter(|record| record.owner_checkout_id == owner_checkout_id)
            .collect())
    }

    /// Record a read of one ticket the plan names, or that it could not be read. A
    /// read older than the stored one is ignored, and a ticket no step names is not
    /// kept. Returns whether anything changed.
    pub fn observe(
        &self,
        owner_checkout_id: &str,
        target: &RunTarget,
        update: TicketUpdate,
    ) -> Result<bool, OrchestrationError> {
        valid_identifier(owner_checkout_id)?;
        validate_update(target, &update)?;
        let _lock = self.storage.lock()?;
        let mut file = self.read_file()?;
        if !apply_observation(&mut file, owner_checkout_id, target, update) {
            return Ok(false);
        }
        self.write_file(&file)?;
        Ok(true)
    }

    /// Store the levels just computed and return the steps whose level differs from
    /// the one last published. Concurrent callers report each change once.
    pub fn publish(
        &self,
        owner_checkout_id: &str,
        levels: &[(String, ReadinessLevel)],
        now_ms: u64,
    ) -> Result<Vec<LevelChange>, OrchestrationError> {
        valid_identifier(owner_checkout_id)?;
        let _lock = self.storage.lock()?;
        let mut file = self.read_file()?;
        let mut changes = Vec::new();
        for record in &mut file.steps {
            if record.step.owner_checkout_id != owner_checkout_id {
                continue;
            }
            let Some((_, level)) = levels.iter().find(|(id, _)| *id == record.step.step_id) else {
                continue;
            };
            let from = record.published.as_ref().map(|published| published.level);
            if from == Some(*level) {
                continue;
            }
            let seq = record
                .published
                .as_ref()
                .map_or(1, |published| published.seq.saturating_add(1));
            record.published = Some(PublishedLevel {
                level: *level,
                since_ms: now_ms,
                seq,
            });
            changes.push(LevelChange {
                step_id: record.step.step_id.clone(),
                from,
                to: *level,
                seq,
            });
        }
        if !changes.is_empty() {
            self.write_file(&file)?;
        }
        Ok(changes)
    }
}

/// Every ticket the owner's steps work or require.
fn validate_update(target: &RunTarget, update: &TicketUpdate) -> Result<(), OrchestrationError> {
    validate_target(target)?;
    if let Some(seen) = &update.observed {
        validate_seen(seen)?;
    }
    if let Some(Some(down)) = &update.unreachable {
        valid_identifier(&down.reason)?;
    }
    Ok(())
}

/// Record one read in `file`: a read older than the stored one is ignored, and a
/// ticket no step of the owner names is not kept. Returns whether anything changed.
fn apply_observation(
    file: &mut PlanFile,
    owner_checkout_id: &str,
    target: &RunTarget,
    update: TicketUpdate,
) -> bool {
    let key = ticket_key(target);
    if !named(file, owner_checkout_id).contains(&key) {
        return false;
    }
    let index = match file.tickets.iter().position(|record| {
        record.owner_checkout_id == owner_checkout_id && record.target == *target
    }) {
        Some(index) => index,
        None => {
            file.tickets.push(TicketRecord {
                owner_checkout_id: owner_checkout_id.into(),
                target: target.clone(),
                facts: TicketFacts::default(),
            });
            file.tickets.len() - 1
        }
    };
    let facts = &mut file.tickets[index].facts;
    let before = facts.clone();
    if let Some(seen) = update.observed
        && facts
            .observed
            .as_ref()
            .is_none_or(|stored| seen.at_ms >= stored.at_ms)
    {
        facts.observed = Some(seen);
    }
    match update.unreachable {
        // An outage keeps when it began; reporting it again changes nothing.
        Some(Some(down)) if facts.unreachable.is_none() => facts.unreachable = Some(down),
        Some(None) => facts.unreachable = None,
        _ => {}
    }
    *facts != before
}

fn named(file: &PlanFile, owner_checkout_id: &str) -> BTreeSet<TicketKey> {
    file.steps
        .iter()
        .filter(|record| record.step.owner_checkout_id == owner_checkout_id)
        .flat_map(|record| {
            record
                .step
                .prerequisites
                .iter()
                .filter_map(Prerequisite::ticket_key)
                .chain([ticket_key(&record.step.target)])
        })
        .collect()
}

fn prune_tickets(file: &mut PlanFile) {
    let owners: BTreeSet<String> = file
        .tickets
        .iter()
        .map(|record| record.owner_checkout_id.clone())
        .collect();
    let keep: BTreeMap<String, BTreeSet<TicketKey>> = owners
        .into_iter()
        .map(|owner| {
            let named = named(file, &owner);
            (owner, named)
        })
        .collect();
    file.tickets.retain(|record| {
        keep.get(&record.owner_checkout_id)
            .is_some_and(|named| named.contains(&ticket_key(&record.target)))
    });
}

fn valid_native(value: &str) -> Result<(), OrchestrationError> {
    if value.is_empty() || value.len() > MAX_NATIVE_ID_BYTES || value.chars().any(char::is_control)
    {
        return Err(OrchestrationError::Invalid("bounded native id required"));
    }
    Ok(())
}

fn validate_target(target: &RunTarget) -> Result<(), OrchestrationError> {
    valid_token(&target.target_id)?;
    valid_identifier(&target.connection_id)?;
    valid_native(&target.native_id)
}

fn validate_seen(seen: &TicketSeen) -> Result<(), OrchestrationError> {
    if seen.exists {
        valid_identifier(&seen.status)?;
    }
    if let Some(priority) = &seen.priority {
        valid_identifier(priority)?;
    }
    Ok(())
}

fn validate_step(step: &PlanStep) -> Result<(), OrchestrationError> {
    valid_token(&step.step_id)?;
    valid_identifier(&step.owner_checkout_id)?;
    valid_identifier(&step.ticket.connection_id)?;
    valid_native(&step.ticket.native_id)?;
    validate_target(&step.target)?;
    if let Some(actor) = &step.updated_by {
        valid_identifier(actor)?;
    }
    validate_needs(&step.resources, MAX_STEP_RESOURCES)?;
    if let Some(tool) = &step.tool {
        valid_identifier(tool)?;
    }
    if let Some(model) = &step.model {
        super::actions::valid_model(model)?;
    }
    if step.prerequisites.len() > MAX_STEP_PLAN_PREREQUISITES {
        return Err(OrchestrationError::Invalid(
            "a step has at most 16 prerequisites",
        ));
    }
    for (index, prerequisite) in step.prerequisites.iter().enumerate() {
        match prerequisite {
            Prerequisite::Ticket {
                target_id,
                connection_id,
                native_id,
                commit,
                ..
            } => {
                valid_token(target_id)?;
                valid_identifier(connection_id)?;
                valid_native(native_id)?;
                if let Some(commit) = commit {
                    valid_commit(commit)?;
                }
                if prerequisite.ticket_key() == Some(ticket_key(&step.target)) {
                    return Err(OrchestrationError::Invalid(
                        "a step cannot require its own ticket",
                    ));
                }
            }
            Prerequisite::Validation { ticket, step_id } => {
                valid_identifier(&ticket.connection_id)?;
                valid_native(&ticket.native_id)?;
                valid_identifier(step_id)?;
            }
        }
        // One prerequisite per ticket or validation step, whatever else it says.
        let subject = |other: &Prerequisite| match other {
            Prerequisite::Ticket { .. } => (other.ticket_key(), None),
            Prerequisite::Validation { ticket, step_id } => {
                (None, Some((ticket.clone(), step_id.clone())))
            }
        };
        if step.prerequisites[..index]
            .iter()
            .any(|other| subject(other) == subject(prerequisite))
        {
            return Err(OrchestrationError::Invalid(
                "a prerequisite is named once per step",
            ));
        }
    }
    Ok(())
}
