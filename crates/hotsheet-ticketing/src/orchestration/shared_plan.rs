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

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::contract::valid_token;
use super::manifest::fresh_token;
use super::readiness::{PlanStep, Prerequisite, ticket_key};
use super::storage::{Storage, decode};
use super::*;

/// The checked-in plan, relative to the orchestration project's checkout.
pub const SHARED_PLAN_PATH: &str = ".hotsheet2/orchestration-plan.json";
const MAX_SHARED_PLAN_BYTES: usize = 4 * 1024 * 1024;

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
            revision: self.revision,
            created_at_ms: self.created_at_ms,
            updated_at_ms: self.updated_at_ms,
            updated_by: self.updated_by.clone(),
        }
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

/// The checked-in plan of one orchestration project's checkout.
#[derive(Clone)]
pub struct SharedPlanStore {
    pub(crate) storage: Storage,
}

impl SharedPlanStore {
    pub fn new(checkout_root: &Path) -> Self {
        Self {
            storage: Storage::shared_named(checkout_root, SHARED_PLAN_PATH),
        }
    }

    /// The document, or `None` when nothing was checked in yet. Never creates it; a
    /// damaged document (for example one left with merge markers) is an error.
    pub fn read(&self) -> Result<Option<SharedPlan>, OrchestrationError> {
        let Some(bytes) = self.storage.read(MAX_SHARED_PLAN_BYTES)? else {
            return Ok(None);
        };
        let document: SharedPlan = decode(&bytes)?;
        if document.schema_version != ORCHESTRATION_VERSION {
            return Err(OrchestrationError::UnsupportedVersion {
                found: u64::from(document.schema_version),
            });
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

    /// Replace the steps under the caller's lock. The same steps again change nothing
    /// and keep the revision; otherwise a fresh revision is written.
    pub(crate) fn write_locked(
        &self,
        previous: Option<&SharedPlan>,
        mut steps: Vec<SharedStep>,
        tx: &mut Transaction<'_>,
    ) -> Result<Option<SharedPlan>, OrchestrationError> {
        steps.sort_by(|a, b| a.step_id.cmp(&b.step_id));
        if previous.is_some_and(|document| document.steps == steps)
            || (previous.is_none() && steps.is_empty())
        {
            return Ok(previous.cloned());
        }
        if merge_steps(&steps, &[]).is_err() {
            return Err(OrchestrationError::Conflict);
        }
        let document = SharedPlan {
            schema_version: ORCHESTRATION_VERSION,
            revision: fresh_token(tx, previous.map(|document| document.revision.as_str()))?,
            steps,
        };
        let mut bytes =
            serde_json::to_vec_pretty(&document).map_err(|_| OrchestrationError::Corrupt)?;
        bytes.push(b'\n');
        self.storage.write(&bytes, MAX_SHARED_PLAN_BYTES)?;
        Ok(Some(document))
    }
}
