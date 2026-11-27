//! HS2-190RDX: Update Plan. A re-plan runs in the background while placement keeps
//! working the live plan. The planner writes a machine-local **draft** of the plan,
//! seeded from the live steps when the update opened; finishing the update swaps the
//! checked-in plan to the draft in one locked write. Nothing here starts a planner or
//! winds a session down: this module holds the update record, its draft files and the
//! pure three-way merge the swap applies.
//!
//! The record and the draft live in this machine's home, beside the plan's
//! observations, under `orchestration-plan-updates/<owner>.*.json`. One update per
//! orchestration project at a time; a finished one is kept as the latest outcome
//! until the next update opens.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::contract::valid_identifier;
use super::shared_plan::{SharedStep, merge_steps};
use super::storage::{Storage, decode};
use super::*;

/// The directory under the home that holds the update records and drafts.
pub const PLAN_UPDATE_DIR: &str = "orchestration-plan-updates";
const MAX_UPDATE_BYTES: usize = 4 * 1024 * 1024;

/// Where an update stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanUpdateState {
    /// The planner works on the draft; the live plan is unchanged.
    Drafting,
    /// The draft became the plan.
    Applied,
    /// The draft could not become the plan; the live plan is unchanged.
    Failed,
    /// Someone cancelled it; the live plan is unchanged.
    Cancelled,
}

impl PlanUpdateState {
    pub const ALL: [Self; 4] = [Self::Drafting, Self::Applied, Self::Failed, Self::Cancelled];

    pub fn code(self) -> &'static str {
        match self {
            Self::Drafting => "drafting",
            Self::Applied => "applied",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn is_open(self) -> bool {
        self == Self::Drafting
    }
}

/// Why an update failed. The live plan is unchanged in every case.
pub mod plan_update_reason {
    /// The draft and a change made to the live plan meanwhile disagree on a step.
    pub const CONFLICT: &str = "plan_update_conflict";
    /// The merged plan is not a valid plan (for example two steps for one ticket).
    pub const INVALID: &str = "plan_update_invalid";
    /// The planner ended without finishing the update.
    pub const PLANNER_ENDED: &str = "plan_update_planner_ended";
    /// The planner is still at work on another proposal.
    pub const PLANNER_BUSY: &str = "plan_update_planner_busy";
    /// Cancelled by the caller.
    pub const CANCELLED: &str = "plan_update_cancelled";
}

/// What a swap changed, by step id.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanSwap {
    /// Steps of the live plan that are in the new plan unchanged.
    pub kept: Vec<String>,
    pub added: Vec<String>,
    /// Steps whose definition the new plan changes. Their running work is withdrawn.
    pub changed: Vec<String>,
    /// Steps the new plan no longer has. Their running work is withdrawn.
    pub removed: Vec<String>,
}

impl PlanSwap {
    /// The steps whose running work the swap withdraws.
    pub fn withdrawn(&self) -> impl Iterator<Item = &String> {
        self.changed.iter().chain(&self.removed)
    }
}

/// One update of one orchestration project's plan on this machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanUpdate {
    pub update_id: String,
    pub state: PlanUpdateState,
    /// The planner session (its operation id) that drafts it, once one was asked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planner: Option<String>,
    /// The checked-in plan's revision when the update opened.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_revision: Option<String>,
    /// The live steps when the update opened, as checked in. The merge's base.
    #[serde(default)]
    pub base: Vec<SharedStep>,
    pub created_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_by: Option<String>,
    /// A failure or cancellation reason code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Runs of withdrawn steps a swap could not cancel (left for reconciliation).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unwithdrawn: Vec<String>,
    /// The step ids a failed merge disagreed on.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conflicts: Vec<String>,
    /// What an applied update changed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub swap: Option<PlanSwap>,
    /// The checked-in plan's revision the swap wrote.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub applied_revision: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpdateFile {
    schema_version: u32,
    update: PlanUpdate,
}

/// What a request to update the plan does, decided from the latest update alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateDecision {
    /// Open a new update under this id.
    Open(String),
    /// An update is already drafting: the request answers with it.
    Existing(String),
}

/// Decide a request to update the plan. Only one update drafts at a time: a second
/// request answers with the live one, whatever id it names. The same id as a
/// finished update answers with that update, so a repeated request never opens twice.
pub fn decide_update(
    latest: Option<&PlanUpdate>,
    requested: Option<&str>,
    fresh: impl FnOnce() -> String,
) -> UpdateDecision {
    match latest {
        Some(update) if update.state.is_open() => {
            UpdateDecision::Existing(update.update_id.clone())
        }
        Some(update) if requested == Some(update.update_id.as_str()) => {
            UpdateDecision::Existing(update.update_id.clone())
        }
        _ => UpdateDecision::Open(requested.map_or_else(fresh, str::to_owned)),
    }
}

/// Why a three-way merge refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeRefusal {
    pub reason: &'static str,
    pub conflicts: Vec<String>,
}

/// The three-way merge a swap applies, by step id. `base` is the live plan when the
/// update opened, `live` the live plan now and `draft` the planner's draft:
///
/// - a step the draft left as it was in the base takes the live side (a change made
///   to the live plan meanwhile, its removal included, is kept);
/// - a step the live plan left as it was takes the draft side (the planner's change,
///   addition or removal);
/// - both sides alike take either; both changed differently is a conflict.
///
/// The merged plan must be a valid plan (one step per target ticket), or the merge is
/// refused as invalid. Pure: the same inputs give the same answer in any order.
pub fn merge_update(
    base: &[SharedStep],
    live: &[SharedStep],
    draft: &[SharedStep],
) -> Result<(Vec<SharedStep>, PlanSwap), MergeRefusal> {
    let by_id = |steps: &[SharedStep]| -> BTreeMap<String, SharedStep> {
        steps
            .iter()
            .map(|step| (step.step_id.clone(), step.clone()))
            .collect()
    };
    let (base, live, draft) = (by_id(base), by_id(live), by_id(draft));
    let ids: BTreeSet<&String> = base.keys().chain(live.keys()).chain(draft.keys()).collect();
    let mut merged = Vec::new();
    let mut conflicts = Vec::new();
    for id in ids {
        let (b, l, d) = (base.get(id), live.get(id), draft.get(id));
        let chosen = if d == b {
            l
        } else if l == b || l == d {
            d
        } else {
            conflicts.push(id.clone());
            continue;
        };
        if let Some(step) = chosen {
            merged.push(step.clone());
        }
    }
    if !conflicts.is_empty() {
        return Err(MergeRefusal {
            reason: plan_update_reason::CONFLICT,
            conflicts,
        });
    }
    let merged = merge_steps(&merged, &[]).map_err(|conflicts| MergeRefusal {
        reason: plan_update_reason::INVALID,
        conflicts,
    })?;
    let mut swap = PlanSwap::default();
    for step in &merged {
        match live.get(&step.step_id) {
            None => swap.added.push(step.step_id.clone()),
            Some(before) if before == step => swap.kept.push(step.step_id.clone()),
            Some(_) => swap.changed.push(step.step_id.clone()),
        }
    }
    swap.removed = live
        .keys()
        .filter(|id| !merged.iter().any(|step| &step.step_id == *id))
        .cloned()
        .collect();
    Ok((merged, swap))
}

/// The update record and draft files of one orchestration project on this machine.
#[derive(Clone)]
pub struct PlanUpdateStore {
    record: Storage,
    home: std::path::PathBuf,
    owner: String,
}

impl PlanUpdateStore {
    pub fn new(home: &Path, owner_checkout_id: &str) -> Result<Self, OrchestrationError> {
        valid_identifier(owner_checkout_id)?;
        Ok(Self {
            record: Storage::machine_named(
                home,
                &format!("{PLAN_UPDATE_DIR}/{owner_checkout_id}.json"),
            ),
            home: home.into(),
            owner: owner_checkout_id.into(),
        })
    }

    /// The draft's checked-in-shaped plan document.
    pub(crate) fn draft_storage(home: &Path, owner_checkout_id: &str) -> Storage {
        Storage::machine_named(
            home,
            &format!("{PLAN_UPDATE_DIR}/{owner_checkout_id}.draft.json"),
        )
    }

    /// The draft's own ticket reads, so the live plan's reads and levels are untouched.
    pub(crate) fn observed_storage(home: &Path, owner_checkout_id: &str) -> Storage {
        Storage::machine_named(
            home,
            &format!("{PLAN_UPDATE_DIR}/{owner_checkout_id}.observed.json"),
        )
    }

    pub fn lock(&self) -> Result<crate::FileLock, OrchestrationError> {
        self.record.lock()
    }

    /// The latest update, open or finished. Never creates the file.
    pub fn latest(&self) -> Result<Option<PlanUpdate>, OrchestrationError> {
        let Some(bytes) = self.record.read(MAX_UPDATE_BYTES)? else {
            return Ok(None);
        };
        let file: UpdateFile = decode(&bytes)?;
        if file.schema_version != ORCHESTRATION_VERSION {
            return Err(OrchestrationError::UnsupportedVersion {
                found: u64::from(file.schema_version),
            });
        }
        valid_identifier(&file.update.update_id).map_err(|_| OrchestrationError::Corrupt)?;
        Ok(Some(file.update))
    }

    /// Replace the latest update. The caller holds [`Self::lock`].
    pub fn write(&self, update: &PlanUpdate) -> Result<(), OrchestrationError> {
        valid_identifier(&update.update_id)?;
        let bytes = serde_json::to_vec(&UpdateFile {
            schema_version: ORCHESTRATION_VERSION,
            update: update.clone(),
        })
        .map_err(|_| OrchestrationError::Corrupt)?;
        self.record.write(&bytes, MAX_UPDATE_BYTES)
    }

    /// Start a draft from `steps`, replacing any earlier draft and its reads.
    pub fn seed_draft(
        &self,
        steps: Vec<SharedStep>,
        tx: &mut Transaction<'_>,
    ) -> Result<(), OrchestrationError> {
        self.discard_draft()?;
        let store = SharedPlanStore::draft(&self.home, &self.owner);
        let _lock = store.lock()?;
        // An empty draft is still a draft: it is written so the planner's reads see it.
        let document = SharedPlan {
            schema_version: ORCHESTRATION_VERSION,
            revision: super::manifest::fresh_token(tx, None)?,
            steps: {
                let mut steps = steps;
                steps.sort_by(|a, b| a.step_id.cmp(&b.step_id));
                steps
            },
            holds: Vec::new(),
        };
        let mut bytes =
            serde_json::to_vec_pretty(&document).map_err(|_| OrchestrationError::Corrupt)?;
        bytes.push(b'\n');
        store.storage.write(&bytes, 4 * 1024 * 1024)
    }

    /// The draft's steps, as checked in.
    pub fn draft_steps(&self) -> Result<Vec<SharedStep>, OrchestrationError> {
        Ok(SharedPlanStore::draft(&self.home, &self.owner)
            .read()?
            .map(|document| document.steps)
            .unwrap_or_default())
    }

    /// Forget the draft and its reads. Absent files are fine.
    pub fn discard_draft(&self) -> Result<(), OrchestrationError> {
        for storage in [
            Self::draft_storage(&self.home, &self.owner),
            Self::observed_storage(&self.home, &self.owner),
        ] {
            match std::fs::remove_file(&storage.path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }
}
