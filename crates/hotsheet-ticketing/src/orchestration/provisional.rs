//! HS2-JSF985: whether a step at the `provisional` readiness level may be worked
//! ahead of its prerequisites' pending gates, across orchestration targets.
//!
//! The rule is the single-project one of HS2-2SJPCH, [`crate::provisional`]. This
//! module only projects a plan onto that rule's input and reads its answer:
//!
//! - a prerequisite that holds (§26.15) is a closed ticket;
//! - a prerequisite whose latest run links one standing CI gate is a ticket in
//!   `final_testing` that recorded that commit as its `CI gate commit`;
//! - the prerequisites of the step that works a ticket are its `blocked_by`;
//! - the [`GateProbe`] answers from this machine's gate records (§26.13).
//!
//! What the rule cannot know is decided here first: the orchestration policy and
//! each involved target's own policy must both allow speculation, an unknown answer
//! is a refusal, and the depth limit is the smallest any of them sets.
//!
//! The module also holds the machine-local file the runtime keeps its ledger in.
//! The ledger itself is `hotsheet_cli::coordination::speculation::SpeculationLedger`.

use std::collections::BTreeMap;
use std::path::Path;

use hotsheet_model::{Note, NoteKind, StartedPhase, Status, Ticket, Timestamp, Ulid};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use super::contract::valid_identifier;
use super::readiness::{Verdict, verdict};
use super::storage::{Storage, decode};
use super::{
    GateFacts, GateRequirement, GateState, ORCHESTRATION_VERSION, OrchestrationError, PlanFacts,
    PolicyBody, Prerequisite, ReadinessLevel, StepReadiness, TargetSpeculation, TicketKey,
    ticket_key,
};
use crate::provisional::{
    BlockerReadiness, GateObservation, GateProbe, GateResult, OPT_OUT_TAG, SpeculationPolicy,
    blocker_readiness,
};

const MAX_PROVISIONAL_BYTES: usize = 8 * 1024 * 1024;

/// A pending gate a provisional step would rest on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProvisionalBasis {
    /// The prerequisite ticket whose gate is undecided.
    pub target_id: String,
    pub connection_id: String,
    pub native_id: String,
    /// Its integrated commit.
    pub commit: String,
    /// The standing CI gates of that commit on this machine, oldest link first.
    pub gate_ids: Vec<String>,
    pub gate: GateResult,
    /// Unconfirmed gates stacked at this prerequisite, itself included.
    pub depth: u32,
    /// The prerequisite pins this commit, so a later run of its ticket does not
    /// replace it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pinned: bool,
}

/// The provisional rule's answer for one step at the `provisional` level.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProvisionalStanding {
    /// The step may be worked now, ahead of the gates in `bases`.
    pub allowed: bool,
    /// Why not, as a stable code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refusal: Option<String>,
    /// The prerequisite a refusal is about, when it is about one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refused_prerequisite: Option<Prerequisite>,
    /// The deepest stack of unconfirmed gates beneath the step; 0 when none is
    /// pending.
    pub depth: u32,
    /// The smallest depth limit among the policies involved.
    pub max_depth: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bases: Vec<ProvisionalBasis>,
}

impl ProvisionalStanding {
    fn refused(code: &str, prerequisite: Option<&Prerequisite>, max_depth: u32) -> Self {
        Self {
            refusal: Some(code.into()),
            refused_prerequisite: prerequisite.cloned(),
            max_depth,
            ..Self::default()
        }
    }
}

/// What the rule is decided on, beside the plan's own facts.
pub struct SpeculationInputs<'a> {
    pub policy: &'a PolicyBody,
    /// Each involved target's own setting as its machine last reported it. A target
    /// that is absent is unknown, which refuses.
    pub targets: &'a BTreeMap<String, TargetSpeculation>,
}

/// One ticket as one prerequisite sees it: the same ticket may hold for a step that
/// asks for no gate and not for one that asks for CI.
type Node = (TicketKey, GateRequirement, Option<String>);

struct Projection<'a> {
    facts: &'a PlanFacts,
    /// The prerequisites of the step that works each ticket.
    steps: BTreeMap<TicketKey, &'a [Prerequisite]>,
    ids: BTreeMap<Node, Ulid>,
    tickets: Vec<Ticket>,
    gates: BTreeMap<String, (Option<bool>, GateObservation)>,
}

fn gate_of<'a>(facts: &'a PlanFacts, node: &Node) -> Option<&'a GateFacts> {
    match &node.2 {
        Some(commit) => facts.commit_gates.get(&(node.0.0.clone(), commit.clone())),
        None => facts.gates.get(&node.0),
    }
}

/// The one commit a prerequisite's standing CI gates are about, when there is one.
fn gate_commit(facts: &PlanFacts, node: &Node) -> Option<String> {
    let gate = gate_of(facts, node)?;
    match (&node.2, gate.commits.as_slice()) {
        (Some(commit), _) => Some(commit.clone()),
        (None, [only]) => Some(only.clone()),
        _ => None,
    }
}

/// The rolled-up CI gates of one commit as the rule reads a gate: its result, the
/// first standing gate as the name of what is pending, and whether the reading
/// attests that the shipping branch contains the commit.
pub fn gate_observation(gate: &GateFacts) -> (Option<bool>, GateObservation) {
    let result = match gate.ci {
        GateState::Pending => GateResult::Pending,
        GateState::Passed => GateResult::Green,
        GateState::Failed => GateResult::Red,
        GateState::Superseded => GateResult::Superseded,
        GateState::Unknown if gate.ci_reason.as_deref() == Some("watch_cancelled") => {
            GateResult::Cancelled
        }
        GateState::Unknown | GateState::AwaitingApproval => GateResult::Unknown,
    };
    // A watched gate with a reading attests that the shipping branch contains the
    // commit (§26.13); an unknown one attests nothing.
    let reachable = matches!(
        result,
        GateResult::Pending | GateResult::Green | GateResult::Red
    )
    .then_some(true);
    (
        reachable,
        GateObservation {
            result,
            run: gate.gate_ids.first().cloned(),
        },
    )
}

fn marker(commit: &str) -> Note {
    let at = Timestamp::new("1970-01-01T00:00:00Z");
    Note {
        id: Ulid::from(0u128),
        kind: NoteKind::Regular,
        kind_explicit: false,
        created_at: at.clone(),
        edited_at: at,
        summary: None,
        confidence: None,
        feedback_for: None,
        feedback_request: false,
        response_to: None,
        ai_feedback: None,
        human_edited: false,
        actor: None,
        text: format!("CI gate commit: {commit}"),
    }
}

impl<'a> Projection<'a> {
    fn new(answers: &'a [StepReadiness], facts: &'a PlanFacts) -> Self {
        Self {
            facts,
            steps: answers
                .iter()
                .map(|answer| {
                    (
                        ticket_key(&answer.step.target),
                        answer.step.prerequisites.as_slice(),
                    )
                })
                .collect(),
            ids: BTreeMap::new(),
            tickets: Vec::new(),
            gates: BTreeMap::new(),
        }
    }

    fn opted_out(&self, key: &TicketKey) -> Option<bool> {
        self.facts
            .tickets
            .get(key)
            .and_then(|facts| facts.observed.as_ref())
            .and_then(|seen| seen.opted_out)
    }

    fn mint(&mut self) -> Ulid {
        // One per node, so every id is distinct; `u128::MAX` is the dependent's.
        Ulid::from(u128::try_from(self.ids.len() + 1).unwrap_or(u128::MAX - 1))
    }

    /// Project a prerequisite as the rule's ticket and return its id.
    fn ticket(&mut self, prerequisite: &Prerequisite) -> Option<Ulid> {
        let Prerequisite::Ticket { gates, commit, .. } = prerequisite else {
            return None;
        };
        let key = prerequisite.ticket_key()?;
        let node: Node = (key.clone(), *gates, commit.clone());
        if let Some(id) = self.ids.get(&node) {
            return Some(*id);
        }
        let id = self.mint();
        // Registered before its own prerequisites are walked, so a cycle ends here.
        self.ids.insert(node.clone(), id);
        let mut ticket = Ticket {
            id,
            slug: format!("{}/{}:{}", key.0, key.1, key.2),
            ..Ticket::default()
        };
        if self.opted_out(&key) == Some(true) {
            ticket.tags.push(OPT_OUT_TAG.into());
        }
        if matches!(verdict(prerequisite, self.facts), Verdict::Satisfied) {
            ticket.status = Status::Completed;
        } else {
            ticket.status = Status::Started;
            if *gates != GateRequirement::None
                && let Some(commit) = gate_commit(self.facts, &node)
                && let Some(gate) = gate_of(self.facts, &node)
            {
                ticket.started_phase = Some(StartedPhase::FinalTesting);
                ticket.notes.push(marker(&commit));
                self.gates
                    .entry(commit)
                    .or_insert_with(|| gate_observation(gate));
            }
            let beneath: &[Prerequisite] = self.steps.get(&key).copied().unwrap_or(&[]);
            let facts = self.facts;
            for other in beneath {
                if !matches!(verdict(other, facts), Verdict::Satisfied)
                    && let Some(id) = self.ticket(other)
                {
                    ticket.blocked_by.push(id);
                }
            }
        }
        self.tickets.push(ticket);
        Some(id)
    }
}

struct Probe<'a>(&'a BTreeMap<String, (Option<bool>, GateObservation)>);
impl GateProbe for Probe<'_> {
    fn reachable(&self, commit: &str) -> Option<bool> {
        self.0.get(commit).and_then(|(reachable, _)| *reachable)
    }
    fn gate(&self, commit: &str) -> GateObservation {
        self.0
            .get(commit)
            .map(|(_, gate)| gate.clone())
            .unwrap_or_default()
    }
}

/// The provisional rule's answer for every step at the `provisional` level, by step
/// id. Pure: the same answers, facts and inputs give the same result.
pub fn provisional_standings(
    answers: &[StepReadiness],
    facts: &PlanFacts,
    inputs: &SpeculationInputs<'_>,
) -> BTreeMap<String, ProvisionalStanding> {
    let policy = inputs.policy;
    let mut projection = Projection::new(answers, facts);
    let mut standings = BTreeMap::new();
    for answer in answers {
        if answer.level != ReadinessLevel::Provisional {
            continue;
        }
        let step = &answer.step;
        let own = ticket_key(&step.target);
        let limit = policy.speculation.max_depth;
        let refuse = |code: &str, prerequisite, max_depth| {
            ProvisionalStanding::refused(code, prerequisite, max_depth)
        };
        // Whose policy says no is named: the orchestration project's, or the target's.
        let permit = |target_id: &str, prerequisite: bool| -> Result<u32, &'static str> {
            if !policy.speculation_allowed(target_id) {
                return Err(if prerequisite {
                    "prerequisite_speculation_not_allowed"
                } else {
                    "policy_speculation_not_allowed"
                });
            }
            match inputs.targets.get(target_id) {
                None => Err("target_speculation_unknown"),
                Some(target) if !target.enabled => Err("target_speculation_disabled"),
                Some(target) => Ok(target.max_depth),
            }
        };
        let mut max_depth = match permit(&step.target.target_id, false) {
            Ok(depth) => limit.min(depth),
            Err(code) => {
                standings.insert(step.step_id.clone(), refuse(code, None, limit));
                continue;
            }
        };
        let mut dependent = Ticket {
            id: Ulid::from(u128::MAX),
            slug: step.step_id.clone(),
            status: Status::NotStarted,
            ..Ticket::default()
        };
        match projection.opted_out(&own) {
            Some(false) => {}
            Some(true) => dependent.tags.push(OPT_OUT_TAG.into()),
            None => {
                standings.insert(
                    step.step_id.clone(),
                    refuse("opt_out_unknown", None, max_depth),
                );
                continue;
            }
        }
        let mut refusal = None;
        let mut pending: Vec<(&Prerequisite, Ulid)> = Vec::new();
        for prerequisite in &step.prerequisites {
            let (Prerequisite::Ticket { gates, commit, .. }, Some(key)) =
                (prerequisite, prerequisite.ticket_key())
            else {
                continue;
            };
            let code = match verdict(prerequisite, facts) {
                Verdict::Satisfied => continue,
                Verdict::Provisional(code, _) => code,
                // The level says none is blocked or stale; refuse rather than assume.
                Verdict::Blocked(code) | Verdict::Stale(code) => {
                    refusal = Some((code, prerequisite));
                    break;
                }
            };
            match permit(&key.0, true) {
                Ok(depth) => max_depth = max_depth.min(depth),
                Err(code) => {
                    refusal = Some((code, prerequisite));
                    break;
                }
            }
            if projection.opted_out(&key).is_none() {
                refusal = Some(("opt_out_unknown", prerequisite));
                break;
            }
            // A deployment that has not passed is not speculated on (HS2-QBERH6).
            if *gates == GateRequirement::Deployment && code == "gate_pending" {
                refusal = Some(("deployment_gate_pending", prerequisite));
                break;
            }
            let node: Node = (key, *gates, commit.clone());
            if gate_commit(facts, &node).is_none() {
                refusal = Some(("gate_commit_ambiguous", prerequisite));
                break;
            }
            let Some(id) = projection.ticket(prerequisite) else {
                continue;
            };
            dependent.blocked_by.push(id);
            pending.push((prerequisite, id));
        }
        if let Some((code, prerequisite)) = refusal {
            standings.insert(
                step.step_id.clone(),
                refuse(code, Some(prerequisite), max_depth),
            );
            continue;
        }
        let rule = SpeculationPolicy {
            enabled: true,
            max_depth,
            ..SpeculationPolicy::default()
        };
        let mut standing = ProvisionalStanding {
            allowed: true,
            max_depth,
            ..ProvisionalStanding::default()
        };
        for (prerequisite, id) in pending {
            let probe = Probe(&projection.gates);
            match blocker_readiness(&rule, &dependent, &id, &projection.tickets, &probe) {
                BlockerReadiness::Confirmed { .. } => {}
                BlockerReadiness::Provisional(base) => {
                    let key = prerequisite.ticket_key().unwrap_or_default();
                    let node: Node = match prerequisite {
                        Prerequisite::Ticket { gates, commit, .. } => {
                            (key.clone(), *gates, commit.clone())
                        }
                        Prerequisite::Validation { .. } => continue,
                    };
                    standing.depth = standing.depth.max(base.depth);
                    standing.bases.push(ProvisionalBasis {
                        target_id: key.0,
                        connection_id: key.1,
                        native_id: key.2,
                        commit: base.base_commit,
                        gate_ids: gate_of(facts, &node)
                            .map(|gate| gate.gate_ids.clone())
                            .unwrap_or_default(),
                        gate: base.gate.result,
                        depth: base.depth,
                        pinned: node.2.is_some(),
                    });
                }
                BlockerReadiness::Blocked { reason } => {
                    standing = refuse(reason.code(), Some(prerequisite), max_depth);
                    break;
                }
            }
        }
        standings.insert(step.step_id.clone(), standing);
    }
    standings
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProvisionalFile {
    schema_version: u32,
    #[serde(default)]
    owners: BTreeMap<String, serde_json::Value>,
}

/// HS2-J3SMMT: a document that bounds itself. After every change the store asks it to
/// retire what it no longer needs, so the file stays within its limit.
pub trait Retiring {
    /// Retire settled entries beyond the document's bound. True when anything went.
    fn retire(&mut self) -> bool;
}

/// `${HOTSHEET_HOME}/orchestration-provisional.json`: one document per owning
/// checkout (private, locked, atomically replaced, version 1). A read never creates
/// it, and damaged or newer data is an error that is never overwritten. The document
/// is the runtime's: its ledger type lives beside the single-project one.
#[derive(Clone)]
pub struct ProvisionalStore {
    storage: Storage,
}

impl ProvisionalStore {
    pub fn new(home: &Path) -> Self {
        Self {
            storage: Storage::machine_named(home, "orchestration-provisional.json"),
        }
    }

    fn read_file(&self) -> Result<Option<ProvisionalFile>, OrchestrationError> {
        self.storage
            .read(MAX_PROVISIONAL_BYTES)?
            .map(|bytes| decode(&bytes))
            .transpose()
    }

    /// The owner's document, or `None` when nothing was ever recorded for it.
    pub fn read<T: DeserializeOwned>(
        &self,
        owner_checkout_id: &str,
    ) -> Result<Option<T>, OrchestrationError> {
        valid_identifier(owner_checkout_id)?;
        self.read_file()?
            .and_then(|mut file| file.owners.remove(owner_checkout_id))
            .map(|value| serde_json::from_value(value).map_err(|_| OrchestrationError::Corrupt))
            .transpose()
    }

    /// Change the owner's document under the file's lock. `change` answers whether it
    /// changed anything; an unchanged document is not rewritten, and a document that
    /// does not exist yet is created only by a change.
    pub fn update<T, R>(
        &self,
        owner_checkout_id: &str,
        change: impl FnOnce(&mut T) -> Result<(R, bool), OrchestrationError>,
    ) -> Result<R, OrchestrationError>
    where
        T: DeserializeOwned + Serialize + Default + Retiring,
    {
        valid_identifier(owner_checkout_id)?;
        let _lock = self.storage.lock()?;
        let mut file = self.read_file()?.unwrap_or(ProvisionalFile {
            schema_version: ORCHESTRATION_VERSION,
            owners: BTreeMap::new(),
        });
        let mut document: T = match file.owners.get(owner_checkout_id) {
            Some(value) => {
                serde_json::from_value(value.clone()).map_err(|_| OrchestrationError::Corrupt)?
            }
            None => T::default(),
        };
        let (answer, changed) = change(&mut document)?;
        if changed {
            // HS2-J3SMMT: settled entries beyond the document's bound go with the change.
            document.retire();
            file.owners.insert(
                owner_checkout_id.into(),
                serde_json::to_value(&document).map_err(|_| OrchestrationError::Corrupt)?,
            );
            let bytes = serde_json::to_vec(&file).map_err(|_| OrchestrationError::Corrupt)?;
            self.storage.write(&bytes, MAX_PROVISIONAL_BYTES)?;
        }
        Ok(answer)
    }
}
