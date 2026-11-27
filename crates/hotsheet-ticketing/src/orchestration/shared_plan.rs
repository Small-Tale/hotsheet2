//! HS2-YDY5HJ: the checked-in plan of an orchestration project.
//!
//! The step definitions (intent: the orchestration ticket, the exact target ticket,
//! prerequisites, resources, preferred tool and model) are shared the way the target
//! manifest and the policy are: a committed `.hotsheet2/orchestration-plan.json`
//! beside them, replaced under a lock with a fresh document revision on every change.
//! Steps merge by step id: a step on one side only is kept, the same step on both
//! sides must be identical, and anything else is a conflict that is refused, never
//! guessed. What a machine observed (ticket reads), the levels it published, and its
//! runs and gates stay machine-local, so readiness is computed only from what the
//! computing machine confirmed itself.
//!
//! A checkout id is a per-machine path identity, so a shared step carries no owner:
//! each machine reads the steps as its own checkout's.
//!
//! HS2-38MFXT: the plan also carries each template apply's Wait for Accept hold
//! ([`PlanHold`]), so every machine that reads these steps holds the same ones until a
//! person accepts, and an accept on any machine releases them everywhere. The file is
//! written as version [`PLAN_HOLD_VERSION`] only while it holds one, so an older Hot
//! Sheet answers `upgrade_required` (and places nothing) instead of reading the steps
//! without their hold.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::contract::{valid_identifier, valid_token};
use super::manifest::fresh_token;
use super::readiness::{PlanStep, Prerequisite, ticket_key};
use super::storage::Storage;
use super::*;

/// The checked-in plan, relative to the orchestration project's checkout.
pub const SHARED_PLAN_PATH: &str = ".hotsheet2/orchestration-plan.json";
const MAX_SHARED_PLAN_BYTES: usize = 4 * 1024 * 1024;
/// HS2-38MFXT: the plan's version while it holds a template plan for an accept.
/// Version 1 never carries holds, so an older binary keeps reading a plan without one
/// and refuses one with a hold as `upgrade_required` before it could place a held step
/// or overwrite the hold.
pub const PLAN_HOLD_VERSION: u32 = 2;
/// HS2-38MFXT: at most this many holds are kept; the oldest with no live step goes first.
pub const MAX_PLAN_HOLDS: usize = 256;

/// One step as checked in: the definition without the machine-local owner id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SharedStep {
    pub step_id: String,
    pub ticket: RunTicket,
    pub target: RunTarget,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prerequisites: Vec<Prerequisite>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resources: Vec<ResourceNeed>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// HS2-ZY4Z4G: what the target's machine must be.
    #[serde(default, skip_serializing_if = "StepRequirements::is_empty")]
    pub requirements: StepRequirements,
    pub revision: u32,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_by: Option<String>,
}

impl SharedStep {
    pub fn of(step: &PlanStep) -> Self {
        Self {
            step_id: step.step_id.clone(),
            ticket: step.ticket.clone(),
            target: step.target.clone(),
            prerequisites: step.prerequisites.clone(),
            resources: step.resources.clone(),
            tool: step.tool.clone(),
            model: step.model.clone(),
            requirements: step.requirements.clone(),
            revision: step.revision,
            created_at_ms: step.created_at_ms,
            updated_at_ms: step.updated_at_ms,
            updated_by: step.updated_by.clone(),
        }
    }

    /// The step as the checkout `owner_checkout_id` on this machine plans it.
    pub fn step(&self, owner_checkout_id: &str) -> PlanStep {
        PlanStep {
            step_id: self.step_id.clone(),
            owner_checkout_id: owner_checkout_id.into(),
            ticket: self.ticket.clone(),
            target: self.target.clone(),
            prerequisites: self.prerequisites.clone(),
            resources: self.resources.clone(),
            tool: self.tool.clone(),
            model: self.model.clone(),
            requirements: self.requirements.clone(),
            revision: self.revision,
            created_at_ms: self.created_at_ms,
            updated_at_ms: self.updated_at_ms,
            updated_by: self.updated_by.clone(),
        }
    }
}

/// HS2-38MFXT: one template apply's Wait for Accept, checked in with the plan. While
/// it is kept, a step of its goal that `accepted_steps` does not list is held on every
/// machine that reads the plan, until a person accepts the proposed plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanHold {
    /// The apply's operation id: what `templates/accept` names.
    pub operation_id: String,
    pub template_id: String,
    pub template_name: String,
    /// The goal ticket the apply opened. Steps match it by `native_id` (its ULID), since
    /// a Git source's connection id differs per machine.
    pub goal: RunTicket,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal_slug: Option<String>,
    /// Ordered step ids a person accepted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub accepted_steps: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepted_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepted_by: Option<String>,
    pub created_at_ms: u64,
}

impl PlanHold {
    /// Whether `step` is held by this hold: it plans the hold's goal and no person
    /// accepted it.
    pub fn holds(&self, step: &SharedStep) -> bool {
        self.names_goal(&step.ticket) && !self.accepted_steps.contains(&step.step_id)
    }

    /// Whether `ticket` is the hold's goal.
    pub fn names_goal(&self, ticket: &RunTicket) -> bool {
        ticket.native_id == self.goal.native_id
    }

    /// Record a person's accept of `steps`, added to those accepted before. Answers
    /// whether anything was new; a repeat changes nothing, not even the time.
    pub fn accept(&mut self, steps: &[String], by: Option<&str>, now_ms: u64) -> bool {
        let mut changed = false;
        for step in steps {
            if !self.accepted_steps.contains(step) {
                self.accepted_steps.push(step.clone());
                changed = true;
            }
        }
        if changed {
            self.accepted_steps.sort();
            self.accepted_at_ms = Some(now_ms);
            self.accepted_by = by.map(Into::into);
        }
        changed
    }

    fn valid(&self) -> bool {
        valid_identifier(&self.operation_id).is_ok()
            && valid_token(&self.template_id).is_ok()
            && !self.template_name.is_empty()
            && self.template_name.len() <= 1024
            && valid_token(&self.goal.native_id).is_ok()
            && self.accepted_steps.windows(2).all(|pair| pair[0] < pair[1])
            && self
                .accepted_steps
                .iter()
                .all(|step| valid_token(step).is_ok())
    }
}

/// HS2-38MFXT: the steps held for an accept, by step id, with the hold of each. Pure.
pub fn held_plan_steps<'a>(
    holds: &'a [PlanHold],
    steps: &[SharedStep],
) -> BTreeMap<String, &'a PlanHold> {
    if holds.is_empty() {
        return BTreeMap::new();
    }
    steps
        .iter()
        .filter_map(|step| {
            holds
                .iter()
                .find(|hold| hold.holds(step))
                .map(|hold| (step.step_id.clone(), hold))
        })
        .collect()
}

/// HS2-38MFXT: version 1 without a hold, [`PLAN_HOLD_VERSION`] with one.
pub fn plan_version(holds: &[PlanHold]) -> u32 {
    if holds.is_empty() {
        ORCHESTRATION_VERSION
    } else {
        PLAN_HOLD_VERSION
    }
}

/// HS2-38MFXT: keep at most [`MAX_PLAN_HOLDS`], forgetting the oldest hold that holds
/// no live step first (forgetting one that does would release it), then the oldest.
fn bound_holds(holds: &mut Vec<PlanHold>, steps: &[SharedStep]) {
    while holds.len() > MAX_PLAN_HOLDS {
        let idle = |hold: &PlanHold| !steps.iter().any(|step| hold.holds(step));
        let oldest = |candidates: Vec<usize>| {
            candidates.into_iter().min_by_key(|&index| {
                (
                    holds[index].created_at_ms,
                    holds[index].operation_id.clone(),
                )
            })
        };
        let pick = oldest(
            (0..holds.len())
                .filter(|&index| idle(&holds[index]))
                .collect(),
        )
        .or_else(|| oldest((0..holds.len()).collect()));
        let Some(index) = pick else { return };
        holds.remove(index);
    }
}

/// The checked-in document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SharedPlan {
    pub schema_version: u32,
    /// Changes with every write; a reader that saw it can tell nothing changed since.
    pub revision: String,
    /// Ordered by step id.
    pub steps: Vec<SharedStep>,
    /// HS2-38MFXT: the template applies whose planned steps wait for an accept, ordered
    /// by operation id. Present only in a version-2 plan.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub holds: Vec<PlanHold>,
}

/// The version is read before the strict field set, as every orchestration file does.
fn decode_plan(bytes: &[u8]) -> Result<SharedPlan, OrchestrationError> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| OrchestrationError::Corrupt)?;
    let found = value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .ok_or(OrchestrationError::Corrupt)?;
    if found != u64::from(ORCHESTRATION_VERSION) && found != u64::from(PLAN_HOLD_VERSION) {
        return Err(OrchestrationError::UnsupportedVersion { found });
    }
    serde_json::from_value(value).map_err(|_| OrchestrationError::Corrupt)
}

/// Merge two sets of step definitions by step id. A step on one side only is kept;
/// the same step on both sides must be identical. The same step id with different
/// definitions, or one target ticket under two step ids, is a conflict: the
/// conflicting step ids are returned and nothing is chosen.
pub fn merge_steps(
    ours: &[SharedStep],
    theirs: &[SharedStep],
) -> Result<Vec<SharedStep>, Vec<String>> {
    let mut merged: BTreeMap<String, SharedStep> = BTreeMap::new();
    let mut conflicts: Vec<String> = Vec::new();
    for step in ours.iter().chain(theirs) {
        match merged.get(&step.step_id) {
            Some(existing) if existing != step => conflicts.push(step.step_id.clone()),
            Some(_) => {}
            None => {
                merged.insert(step.step_id.clone(), step.clone());
            }
        }
    }
    // One step per target ticket.
    let mut by_target: BTreeMap<TicketKey, &str> = BTreeMap::new();
    for step in merged.values() {
        if let Some(other) = by_target.insert(ticket_key(&step.target), &step.step_id) {
            conflicts.push(other.to_owned());
            conflicts.push(step.step_id.clone());
        }
    }
    if conflicts.is_empty() {
        Ok(merged.into_values().collect())
    } else {
        conflicts.sort();
        conflicts.dedup();
        Err(conflicts)
    }
}

/// HS2-38MFXT: where a machine notes, per owner, the revision of the checked-in plan it
/// wrote last (under its home). A scheduler that sees another revision knows the plan
/// was written elsewhere (another machine's write, pulled in): a write made here
/// publishes its own events and wake marker.
pub const PLAN_WRITTEN_DIR: &str = "orchestration-plan-written";

/// HS2-38MFXT: the revision of the owner's checked-in plan this machine wrote last.
pub fn plan_written_here(home: &Path, owner_checkout_id: &str) -> Option<String> {
    std::fs::read_to_string(home.join(PLAN_WRITTEN_DIR).join(owner_checkout_id))
        .ok()
        .map(|text| text.trim().to_owned())
}

/// The checked-in plan of one orchestration project's checkout.
#[derive(Clone)]
pub struct SharedPlanStore {
    pub(crate) storage: Storage,
    /// HS2-38MFXT: the file under a machine home that notes each revision written here.
    written_here: Option<std::path::PathBuf>,
}

impl SharedPlanStore {
    pub fn new(checkout_root: &Path) -> Self {
        Self {
            storage: Storage::shared_named(checkout_root, SHARED_PLAN_PATH),
            written_here: None,
        }
    }

    /// HS2-38MFXT: the same, noting each revision it writes in `home` for the owner.
    pub(crate) fn noting(mut self, home: &Path, owner_checkout_id: &str) -> Self {
        self.written_here = Some(home.join(PLAN_WRITTEN_DIR).join(owner_checkout_id));
        self
    }

    /// HS2-190RDX: the machine-local draft of a plan update, in the same shape.
    pub(crate) fn draft(home: &Path, owner_checkout_id: &str) -> Self {
        Self {
            storage: super::plan_update::PlanUpdateStore::draft_storage(home, owner_checkout_id),
            written_here: None,
        }
    }

    /// The document, or `None` when nothing was checked in yet. Never creates it; a
    /// damaged document (for example one left with merge markers) is an error.
    pub fn read(&self) -> Result<Option<SharedPlan>, OrchestrationError> {
        let Some(bytes) = self.storage.read(MAX_SHARED_PLAN_BYTES)? else {
            return Ok(None);
        };
        let document = decode_plan(&bytes)?;
        // HS2-38MFXT: version 1 never holds, so an older reader never ignores a hold.
        if document.schema_version != plan_version(&document.holds)
            || document.holds.len() > MAX_PLAN_HOLDS
            || !document.holds.iter().all(PlanHold::valid)
            || document
                .holds
                .windows(2)
                .any(|pair| pair[0].operation_id >= pair[1].operation_id)
        {
            return Err(OrchestrationError::Corrupt);
        }
        valid_token(&document.revision).map_err(|_| OrchestrationError::Corrupt)?;
        if merge_steps(&document.steps, &[]).is_err()
            || document
                .steps
                .windows(2)
                .any(|pair| pair[0].step_id >= pair[1].step_id)
        {
            return Err(OrchestrationError::Corrupt);
        }
        Ok(Some(document))
    }

    pub(crate) fn lock(&self) -> Result<crate::FileLock, OrchestrationError> {
        self.storage.lock()
    }

    /// Replace the steps under the caller's lock, keeping the holds (HS2-38MFXT). The
    /// same steps again change nothing and keep the revision; otherwise a fresh
    /// revision is written.
    pub(crate) fn write_locked(
        &self,
        previous: Option<&SharedPlan>,
        steps: Vec<SharedStep>,
        tx: &mut Transaction<'_>,
    ) -> Result<Option<SharedPlan>, OrchestrationError> {
        let holds = previous
            .map(|document| document.holds.clone())
            .unwrap_or_default();
        self.write_all_locked(previous, steps, holds, tx)
    }

    /// HS2-38MFXT: replace the holds under the caller's lock, keeping the steps.
    pub(crate) fn write_holds_locked(
        &self,
        previous: Option<&SharedPlan>,
        holds: Vec<PlanHold>,
        tx: &mut Transaction<'_>,
    ) -> Result<Option<SharedPlan>, OrchestrationError> {
        let steps = previous
            .map(|document| document.steps.clone())
            .unwrap_or_default();
        self.write_all_locked(previous, steps, holds, tx)
    }

    fn write_all_locked(
        &self,
        previous: Option<&SharedPlan>,
        mut steps: Vec<SharedStep>,
        mut holds: Vec<PlanHold>,
        tx: &mut Transaction<'_>,
    ) -> Result<Option<SharedPlan>, OrchestrationError> {
        steps.sort_by(|a, b| a.step_id.cmp(&b.step_id));
        holds.sort_by(|a, b| a.operation_id.cmp(&b.operation_id));
        holds.dedup_by(|a, b| a.operation_id == b.operation_id);
        bound_holds(&mut holds, &steps);
        if previous.is_some_and(|document| document.steps == steps && document.holds == holds)
            || (previous.is_none() && steps.is_empty() && holds.is_empty())
        {
            return Ok(previous.cloned());
        }
        if merge_steps(&steps, &[]).is_err() || !holds.iter().all(PlanHold::valid) {
            return Err(OrchestrationError::Conflict);
        }
        let document = SharedPlan {
            schema_version: plan_version(&holds),
            revision: fresh_token(tx, previous.map(|document| document.revision.as_str()))?,
            steps,
            holds,
        };
        let mut bytes =
            serde_json::to_vec_pretty(&document).map_err(|_| OrchestrationError::Corrupt)?;
        bytes.push(b'\n');
        // HS2-38MFXT: noted before the plan changes, so a scheduler woken by the write
        // already knows it was made here. Best effort: a missing note costs one pass.
        if let Some(note) = &self.written_here
            && let Some(parent) = note.parent()
            && std::fs::create_dir_all(parent).is_ok()
        {
            let _ = std::fs::write(note, document.revision.as_bytes());
        }
        self.storage.write(&bytes, MAX_SHARED_PLAN_BYTES)?;
        Ok(Some(document))
    }
}
