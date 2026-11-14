//! Provisional readiness of a `blocked_by` prerequisite that is pushed and waiting on
//! remote CI (HS2-2SJPCH).
//!
//! Confirmed readiness is the default: a prerequisite satisfies a dependent once it is
//! closed. A project may opt in, through its committed [`SpeculationPolicy`], to treating
//! a prerequisite in `final_testing` as provisionally satisfied when its integrated
//! commit is recorded (`CI gate commit: <sha>`) and reachable from the shipping branch.
//! The dependent then builds on that commit, and the result of the pending gate decides
//! whether the work is confirmed, at risk, or reopened.
//!
//! This module is pure: reachability and gate state come from an injected [`GateProbe`].
//! The single-project dispatcher (`hotsheet-cli` coordination) and the cross-target
//! scheduler (HS2-JSF985) both call [`blocker_readiness`].

use std::collections::BTreeSet;

use hotsheet_model::{StartedPhase, Ticket, Ulid};
use serde::{Deserialize, Serialize};

use crate::{Scope, Settings, ops};

/// Shared project settings key holding the [`SpeculationPolicy`].
pub const SPECULATION_POLICY_SETTING: &str = "speculation";
/// A ticket carrying this tag never takes part in provisional readiness: it is not
/// started on an unconfirmed prerequisite, and no dependent is started on it.
pub const OPT_OUT_TAG: &str = "speculate:never";
/// Unconfirmed gates that may stack when the policy does not say otherwise.
pub const DEFAULT_MAX_DEPTH: u32 = 1;
/// The deepest stack a policy may allow.
pub const MAX_DEPTH_LIMIT: u32 = 8;
const MARKER: &str = "ci gate commit:";

/// Where a dependent commit may go while its base gate is pending.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IntegrationRule {
    /// The dependent may land on the shipping branch once its own local gates pass; the
    /// covering CI run then gates it with its base.
    #[default]
    PushPerTicket,
    /// Branch or review projects: the dependent stays on its branch until the base gate
    /// is green.
    Parked,
}

/// The project's opt-in to provisional readiness. An absent setting is the disabled
/// default, under which every caller behaves exactly as before this policy existed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpeculationPolicy {
    #[serde(default)]
    pub enabled: bool,
    /// How many unconfirmed gates may stack beneath one dependent.
    #[serde(default = "default_max_depth")]
    pub max_depth: u32,
    #[serde(default)]
    pub integration: IntegrationRule,
    /// The branch a recorded gate commit must be reachable from. `None` means the
    /// shipping checkout's upstream branch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_branch: Option<String>,
    /// HS2-X6FB9V: the CI workflow whose push runs judge a gate commit, as the provider
    /// names it. `None` means [`DEFAULT_CI_WORKFLOW`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ci_workflow: Option<String>,
}

/// The workflow a gate is watched in when the policy names none (HS2-X6FB9V), the same
/// default as `job watch ci`.
pub const DEFAULT_CI_WORKFLOW: &str = "CI";

fn default_max_depth() -> u32 {
    DEFAULT_MAX_DEPTH
}

impl Default for SpeculationPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            max_depth: DEFAULT_MAX_DEPTH,
            integration: IntegrationRule::default(),
            base_branch: None,
            ci_workflow: None,
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SpeculationPolicyError {
    #[error("max_depth must be between 1 and {MAX_DEPTH_LIMIT}")]
    Depth,
    #[error("base_branch must be a plain branch name")]
    Branch,
    #[error("ci_workflow must be a workflow name of 1 to 200 printable characters")]
    Workflow,
    #[error("invalid {SPECULATION_POLICY_SETTING} setting: {0}")]
    Invalid(String),
    #[error("settings: {0}")]
    Settings(String),
}

impl SpeculationPolicy {
    pub fn validate(&self) -> Result<(), SpeculationPolicyError> {
        if !(1..=MAX_DEPTH_LIMIT).contains(&self.max_depth) {
            return Err(SpeculationPolicyError::Depth);
        }
        if let Some(branch) = &self.base_branch {
            let plain = !branch.is_empty()
                && branch.len() <= 200
                && !branch.starts_with(['-', '/', '.'])
                && !branch.ends_with(['/', '.'])
                && !branch.contains("..")
                && !branch.contains("//")
                && branch
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '/' | '.'));
            if !plain {
                return Err(SpeculationPolicyError::Branch);
            }
        }
        if let Some(workflow) = &self.ci_workflow {
            if workflow.trim().is_empty()
                || workflow.len() > 200
                || workflow.chars().any(char::is_control)
            {
                return Err(SpeculationPolicyError::Workflow);
            }
        }
        Ok(())
    }

    /// The workflow a gate commit is watched in.
    pub fn workflow(&self) -> &str {
        self.ci_workflow.as_deref().unwrap_or(DEFAULT_CI_WORKFLOW)
    }

    /// Read the committed project policy. Speculation is a project decision, so a
    /// machine-local or global value is never consulted.
    pub fn read(settings: &Settings) -> Result<Self, SpeculationPolicyError> {
        let value = settings
            .get(SPECULATION_POLICY_SETTING, Scope::Shared)
            .map_err(|error| SpeculationPolicyError::Settings(error.to_string()))?;
        let Some(value) = value else {
            return Ok(Self::default());
        };
        let policy: Self = serde_json::from_value(value)
            .map_err(|error| SpeculationPolicyError::Invalid(error.to_string()))?;
        policy.validate()?;
        Ok(policy)
    }

    /// Validate and replace the committed project policy.
    pub fn write(&self, settings: &Settings) -> Result<(), SpeculationPolicyError> {
        self.validate()?;
        let value = serde_json::to_value(self)
            .map_err(|error| SpeculationPolicyError::Invalid(error.to_string()))?;
        settings
            .set(SPECULATION_POLICY_SETTING, value, Scope::Shared)
            .map_err(|error| SpeculationPolicyError::Settings(error.to_string()))
    }
}

/// The CI result of one gate commit, as the HS2-3R9V0R watch reports it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateResult {
    /// Watched, no verdict yet.
    Pending,
    Green,
    Red,
    /// The branch moved to a line that does not contain the commit.
    Superseded,
    /// The covering run was cancelled, or watching was stopped.
    Cancelled,
    /// Not watched, or the watch was lost.
    #[default]
    Unknown,
}

impl GateResult {
    pub const ALL: [Self; 6] = [
        Self::Pending,
        Self::Green,
        Self::Red,
        Self::Superseded,
        Self::Cancelled,
        Self::Unknown,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Green => "green",
            Self::Red => "red",
            Self::Superseded => "superseded",
            Self::Cancelled => "cancelled",
            Self::Unknown => "unknown",
        }
    }
}

/// One reading of a gate: its result and the run it rests on, when known.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateObservation {
    pub result: GateResult,
    /// Provider run identifier (`<run id>.<attempt>`); never run output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
}

impl GateObservation {
    pub fn of(result: GateResult) -> Self {
        Self { result, run: None }
    }

    pub fn with_run(result: GateResult, run: impl Into<String>) -> Self {
        Self {
            result,
            run: Some(run.into()),
        }
    }
}

/// The facts the rule cannot compute from tickets alone.
pub trait GateProbe {
    /// Whether `commit` is reachable from the branch the policy names. `None` means it
    /// could not be determined, which never counts as reachable.
    fn reachable(&self, commit: &str) -> Option<bool>;
    /// The newest reading of the CI gate of `commit`.
    fn gate(&self, commit: &str) -> GateObservation;
}

/// Why a prerequisite does not satisfy a dependent now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockReason {
    /// The policy is off: only a closed prerequisite satisfies.
    PolicyDisabled,
    /// The prerequisite is not in this source, so nothing is known about it.
    UnknownPrerequisite,
    /// The dependent or the prerequisite carries [`OPT_OUT_TAG`].
    OptedOut,
    /// The prerequisite is open and not in `final_testing`.
    NotAwaitingCi,
    /// `final_testing` without a recorded `CI gate commit`.
    NoGateCommit,
    /// The recorded commit is not reachable from the shipping branch.
    NotPushed,
    /// Reachability could not be determined.
    ReachabilityUnknown,
    GateRed,
    GateSuperseded,
    GateCancelled,
    /// More unconfirmed gates would stack than the policy allows.
    DepthExceeded,
}

impl BlockReason {
    pub const fn code(self) -> &'static str {
        match self {
            Self::PolicyDisabled => "speculation_disabled",
            Self::UnknownPrerequisite => "prerequisite_unknown",
            Self::OptedOut => "speculation_opted_out",
            Self::NotAwaitingCi => "prerequisite_not_awaiting_ci",
            Self::NoGateCommit => "gate_commit_unrecorded",
            Self::NotPushed => "gate_commit_unreachable",
            Self::ReachabilityUnknown => "gate_commit_reachability_unknown",
            Self::GateRed => "gate_red",
            Self::GateSuperseded => "gate_superseded",
            Self::GateCancelled => "gate_cancelled",
            Self::DepthExceeded => "speculation_depth_exceeded",
        }
    }
}

/// What confirmed a prerequisite.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Confirmation {
    /// The prerequisite ticket is closed.
    Closed,
    /// The prerequisite waits in `final_testing` and its gate is already green.
    GateGreen,
}

/// The base a dependent builds on while its prerequisite's gate is undecided.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProvisionalBase {
    /// The prerequisite ticket.
    pub blocker: Ulid,
    pub slug: String,
    /// The prerequisite's integrated commit; the dependent's base must contain it.
    pub base_commit: String,
    /// The pending gate of that commit.
    pub gate: GateObservation,
    /// Unconfirmed gates stacked at this prerequisite, itself included (1 or more).
    pub depth: u32,
}

/// How one `blocked_by` prerequisite stands for one dependent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "level", rename_all = "snake_case")]
pub enum BlockerReadiness {
    Confirmed { by: Confirmation },
    Provisional(ProvisionalBase),
    Blocked { reason: BlockReason },
}

impl BlockerReadiness {
    pub fn satisfied(&self) -> bool {
        !matches!(self, Self::Blocked { .. })
    }
}

/// The overall readiness of a dependent across all its prerequisites.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessLevel {
    Confirmed,
    Provisional,
    Blocked,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrerequisiteReadiness {
    pub blocker: Ulid,
    /// Empty when the prerequisite is not in this source.
    pub slug: String,
    pub readiness: BlockerReadiness,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TicketReadiness {
    pub level: ReadinessLevel,
    /// The deepest stack of unconfirmed gates beneath the dependent; 0 when confirmed.
    pub depth: u32,
    pub prerequisites: Vec<PrerequisiteReadiness>,
}

impl TicketReadiness {
    /// Every provisional base, in `blocked_by` order.
    pub fn provisional_bases(&self) -> Vec<&ProvisionalBase> {
        self.prerequisites
            .iter()
            .filter_map(|entry| match &entry.readiness {
                BlockerReadiness::Provisional(base) => Some(base),
                _ => None,
            })
            .collect()
    }

    /// `provisional on <slug> CI <run>` for each unconfirmed base, or the level name.
    pub fn explain(&self) -> String {
        match self.level {
            ReadinessLevel::Confirmed => "confirmed".to_owned(),
            ReadinessLevel::Provisional => self
                .provisional_bases()
                .iter()
                .map(|base| explain_base(&base.slug, &base.gate))
                .collect::<Vec<_>>()
                .join("; "),
            ReadinessLevel::Blocked => self
                .prerequisites
                .iter()
                .filter_map(|entry| match &entry.readiness {
                    BlockerReadiness::Blocked { reason } => Some(format!(
                        "blocked by {} ({})",
                        if entry.slug.is_empty() {
                            entry.blocker.to_string()
                        } else {
                            entry.slug.clone()
                        },
                        reason.code()
                    )),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("; "),
        }
    }
}

/// `provisional on <slug> CI <run>`; a gate without a run names its state instead.
pub fn explain_base(slug: &str, gate: &GateObservation) -> String {
    match &gate.run {
        Some(run) => format!("provisional on {slug} CI {run}"),
        None => format!("provisional on {slug} CI {}", gate.result.name()),
    }
}

/// The integrated commit a ticket recorded for its CI gate: the newest note line of the
/// form `CI gate commit: <full 40-character SHA>`, lowercased. Quoted lines are ignored.
pub fn ci_gate_commit(ticket: &Ticket) -> Option<String> {
    ticket.notes.iter().rev().find_map(|note| {
        note.text.lines().rev().find_map(|line| {
            let line = line.trim();
            let head = line.get(..MARKER.len())?;
            if !head.eq_ignore_ascii_case(MARKER) {
                return None;
            }
            let sha = line[MARKER.len()..].trim().trim_matches('`');
            (sha.len() == 40 && sha.bytes().all(|byte| byte.is_ascii_hexdigit()))
                .then(|| sha.to_ascii_lowercase())
        })
    })
}

pub fn opted_out(ticket: &Ticket) -> bool {
    ticket
        .tags
        .iter()
        .any(|tag| tag.trim().eq_ignore_ascii_case(OPT_OUT_TAG))
}

/// Unconfirmed gates stacked at `ticket`, itself included: 0 when it is closed or its
/// gate is green, otherwise 1 plus the deepest stack beneath its own prerequisites.
fn stacked(
    ticket: &Ticket,
    tickets: &[Ticket],
    probe: &dyn GateProbe,
    visiting: &mut BTreeSet<Ulid>,
) -> u32 {
    if !ops::is_open(ticket) {
        return 0;
    }
    if ticket.started_phase == Some(StartedPhase::FinalTesting)
        && ci_gate_commit(ticket)
            .is_some_and(|commit| probe.gate(&commit).result == GateResult::Green)
    {
        return 0;
    }
    if !visiting.insert(ticket.id) {
        // A dependency cycle never confirms; count it beyond any allowed depth.
        return MAX_DEPTH_LIMIT + 1;
    }
    let beneath = ticket
        .blocked_by
        .iter()
        .map(
            |dependency| match tickets.iter().find(|t| t.id == *dependency) {
                Some(found) => stacked(found, tickets, probe, visiting),
                None => 0,
            },
        )
        .max()
        .unwrap_or(0);
    visiting.remove(&ticket.id);
    beneath.saturating_add(1)
}

/// Whether one prerequisite of `dependent` is satisfied, and how.
///
/// `Confirmed` needs no policy. `Provisional` requires an enabled policy, no opt-out on
/// either ticket, the prerequisite in `final_testing` with a recorded gate commit that
/// the probe reports reachable, an undecided gate, and a stack no deeper than
/// `policy.max_depth`. The returned [`ProvisionalBase`] names the commit to build on,
/// the pending gate and the depth.
pub fn blocker_readiness(
    policy: &SpeculationPolicy,
    dependent: &Ticket,
    blocker: &Ulid,
    tickets: &[Ticket],
    probe: &dyn GateProbe,
) -> BlockerReadiness {
    let blocked = |reason| BlockerReadiness::Blocked { reason };
    let Some(found) = tickets.iter().find(|ticket| ticket.id == *blocker) else {
        return blocked(BlockReason::UnknownPrerequisite);
    };
    if !ops::is_open(found) {
        return BlockerReadiness::Confirmed {
            by: Confirmation::Closed,
        };
    }
    if !policy.enabled {
        return blocked(BlockReason::PolicyDisabled);
    }
    if opted_out(dependent) || opted_out(found) {
        return blocked(BlockReason::OptedOut);
    }
    if found.started_phase != Some(StartedPhase::FinalTesting) {
        return blocked(BlockReason::NotAwaitingCi);
    }
    let Some(base_commit) = ci_gate_commit(found) else {
        return blocked(BlockReason::NoGateCommit);
    };
    match probe.reachable(&base_commit) {
        Some(true) => {}
        Some(false) => return blocked(BlockReason::NotPushed),
        None => return blocked(BlockReason::ReachabilityUnknown),
    }
    let gate = probe.gate(&base_commit);
    match gate.result {
        GateResult::Green => {
            return BlockerReadiness::Confirmed {
                by: Confirmation::GateGreen,
            };
        }
        GateResult::Red => return blocked(BlockReason::GateRed),
        GateResult::Superseded => return blocked(BlockReason::GateSuperseded),
        GateResult::Cancelled => return blocked(BlockReason::GateCancelled),
        GateResult::Pending | GateResult::Unknown => {}
    }
    let depth = stacked(found, tickets, probe, &mut BTreeSet::new());
    if depth > policy.max_depth {
        return blocked(BlockReason::DepthExceeded);
    }
    BlockerReadiness::Provisional(ProvisionalBase {
        blocker: found.id,
        slug: found.slug.clone(),
        base_commit,
        gate,
        depth,
    })
}

/// The readiness of `dependent` across every `blocked_by` prerequisite.
pub fn ticket_readiness(
    policy: &SpeculationPolicy,
    dependent: &Ticket,
    tickets: &[Ticket],
    probe: &dyn GateProbe,
) -> TicketReadiness {
    let prerequisites: Vec<PrerequisiteReadiness> = dependent
        .blocked_by
        .iter()
        .map(|blocker| PrerequisiteReadiness {
            blocker: *blocker,
            slug: tickets
                .iter()
                .find(|ticket| ticket.id == *blocker)
                .map(|ticket| ticket.slug.clone())
                .unwrap_or_default(),
            readiness: blocker_readiness(policy, dependent, blocker, tickets, probe),
        })
        .collect();
    let level = if prerequisites
        .iter()
        .any(|entry| !entry.readiness.satisfied())
    {
        ReadinessLevel::Blocked
    } else if prerequisites
        .iter()
        .any(|entry| matches!(entry.readiness, BlockerReadiness::Provisional(_)))
    {
        ReadinessLevel::Provisional
    } else {
        ReadinessLevel::Confirmed
    };
    let depth = if level == ReadinessLevel::Provisional {
        prerequisites
            .iter()
            .filter_map(|entry| match &entry.readiness {
                BlockerReadiness::Provisional(base) => Some(base.depth),
                _ => None,
            })
            .max()
            .unwrap_or(0)
    } else {
        0
    };
    TicketReadiness {
        level,
        depth,
        prerequisites,
    }
}

#[cfg(test)]
#[path = "provisional_tests.rs"]
mod provisional_tests;
