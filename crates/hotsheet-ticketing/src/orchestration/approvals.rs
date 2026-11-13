//! HS2-YCJDEB: a person's answer to a decision the policy will not take alone.
//!
//! One machine-local record per exact `(target, actor, gate, operation id)`. A request
//! waits as `pending`; a person approves or denies it; an approval is consumed by the
//! one action it was given for and never reused.

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::contract::{valid_identifier, valid_token};
use super::storage::{Storage, decode};
use super::*;

pub const MAX_APPROVALS: usize = 256;
/// An unanswered request lapses after a day.
pub const APPROVAL_PENDING_MS: u64 = 24 * 60 * 60 * 1000;
/// An approval must be used within an hour of being given.
pub const APPROVAL_VALID_MS: u64 = 60 * 60 * 1000;
const MAX_APPROVAL_BYTES: usize = 512 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalState {
    Pending,
    Approved,
    Denied,
    /// The approved action ran; the approval cannot authorize another.
    Consumed,
    Expired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Approval {
    pub approval_id: String,
    pub target: TargetRef,
    pub actor_id: String,
    pub gate: ApprovalGate,
    pub operation_id: String,
    pub state: ApprovalState,
    pub requested_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_by: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApprovalFile {
    schema_version: u32,
    approvals: Vec<Approval>,
}

#[derive(Clone)]
pub struct ApprovalRegistry {
    storage: Storage,
}

impl ApprovalRegistry {
    pub fn new(home: &Path) -> Self {
        Self {
            storage: Storage::machine_named(home, "orchestration-approvals.json"),
        }
    }

    fn read_file(&self) -> Result<ApprovalFile, OrchestrationError> {
        let Some(bytes) = self.storage.read(MAX_APPROVAL_BYTES)? else {
            return Ok(ApprovalFile {
                schema_version: ORCHESTRATION_VERSION,
                approvals: Vec::new(),
            });
        };
        let file: ApprovalFile = decode(&bytes)?;
        for approval in &file.approvals {
            validate(approval).map_err(|_| OrchestrationError::Corrupt)?;
        }
        Ok(file)
    }

    fn write_file(&self, file: &ApprovalFile) -> Result<(), OrchestrationError> {
        let bytes = serde_json::to_vec(file).map_err(|_| OrchestrationError::Corrupt)?;
        self.storage.write(&bytes, MAX_APPROVAL_BYTES)
    }

    /// Requests for one owning checkout, lapsed ones shown as `expired`.
    pub fn list(
        &self,
        owner_checkout_id: &str,
        now_ms: u64,
    ) -> Result<Vec<Approval>, OrchestrationError> {
        valid_identifier(owner_checkout_id)?;
        Ok(self
            .read_file()?
            .approvals
            .into_iter()
            .filter(|approval| approval.target.owner_checkout_id == owner_checkout_id)
            .map(|approval| current(approval, now_ms))
            .collect())
    }

    /// The gate's answer for one exact operation. An approval is consumed here, so it
    /// allows exactly one call; anything else leaves (or files) a pending request and
    /// returns false.
    pub fn consume_or_request(
        &self,
        target: &TargetRef,
        actor_id: &str,
        gate: ApprovalGate,
        operation_id: &str,
        tx: &mut Transaction<'_>,
    ) -> Result<bool, OrchestrationError> {
        target.validate()?;
        valid_identifier(actor_id)?;
        valid_identifier(operation_id)?;
        let now_ms = tx.clock.now_ms();
        let _lock = self.storage.lock()?;
        let mut file = self.read_file()?;
        let matching = |approval: &Approval| {
            &approval.target == target
                && approval.actor_id == actor_id
                && approval.gate == gate
                && approval.operation_id == operation_id
        };
        // The newest record for this exact operation decides.
        if let Some(approval) = file.approvals.iter_mut().rev().find(|a| matching(a)) {
            match current(approval.clone(), now_ms).state {
                ApprovalState::Approved => {
                    approval.state = ApprovalState::Consumed;
                    self.write_file(&file)?;
                    return Ok(true);
                }
                // Still waiting, or answered no: asking again changes nothing.
                ApprovalState::Pending | ApprovalState::Denied => return Ok(false),
                // Used or lapsed: a new request is needed, filed below.
                ApprovalState::Consumed | ApprovalState::Expired => {}
            }
        }
        prune(&mut file, now_ms);
        if file.approvals.len() >= MAX_APPROVALS {
            return Err(OrchestrationError::Capacity);
        }
        file.approvals.push(Approval {
            approval_id: tx.token(),
            target: target.clone(),
            actor_id: actor_id.into(),
            gate,
            operation_id: operation_id.into(),
            state: ApprovalState::Pending,
            requested_at_ms: now_ms,
            decided_at_ms: None,
            decided_by: None,
        });
        self.write_file(&file)?;
        Ok(false)
    }

    /// Whether an unused approval exists for this exact operation, without consuming it.
    pub fn approved(
        &self,
        target: &TargetRef,
        actor_id: &str,
        gate: ApprovalGate,
        operation_id: &str,
        now_ms: u64,
    ) -> Result<bool, OrchestrationError> {
        Ok(self
            .read_file()?
            .approvals
            .into_iter()
            .rev()
            .find(|approval| {
                &approval.target == target
                    && approval.actor_id == actor_id
                    && approval.gate == gate
                    && approval.operation_id == operation_id
            })
            .is_some_and(|approval| current(approval, now_ms).state == ApprovalState::Approved))
    }

    /// A person's answer. Repeating the same answer is a no-op; a request that is no
    /// longer pending cannot be answered differently.
    pub fn decide(
        &self,
        owner_checkout_id: &str,
        approval_id: &str,
        approve: bool,
        decided_by: Option<String>,
        tx: &mut Transaction<'_>,
    ) -> Result<(Approval, bool), OrchestrationError> {
        valid_identifier(owner_checkout_id)?;
        valid_token(approval_id)?;
        if let Some(actor) = &decided_by {
            valid_identifier(actor)?;
        }
        let now_ms = tx.clock.now_ms();
        let wanted = if approve {
            ApprovalState::Approved
        } else {
            ApprovalState::Denied
        };
        let _lock = self.storage.lock()?;
        let mut file = self.read_file()?;
        let approval = file
            .approvals
            .iter_mut()
            .find(|approval| {
                approval.approval_id == approval_id
                    && approval.target.owner_checkout_id == owner_checkout_id
            })
            .ok_or(OrchestrationError::NotFound)?;
        match current(approval.clone(), now_ms).state {
            ApprovalState::Pending => {}
            state if state == wanted => return Ok((approval.clone(), false)),
            _ => return Err(OrchestrationError::Conflict),
        }
        approval.state = wanted;
        approval.decided_at_ms = Some(now_ms);
        approval.decided_by = decided_by;
        let decided = approval.clone();
        self.write_file(&file)?;
        Ok((decided, true))
    }
}

/// Apply lapse rules without writing: stored state stays the last explicit event.
fn current(mut approval: Approval, now_ms: u64) -> Approval {
    let lapsed = match approval.state {
        ApprovalState::Pending => {
            now_ms.saturating_sub(approval.requested_at_ms) >= APPROVAL_PENDING_MS
        }
        ApprovalState::Approved => approval
            .decided_at_ms
            .is_none_or(|at| now_ms.saturating_sub(at) >= APPROVAL_VALID_MS),
        _ => false,
    };
    if lapsed {
        approval.state = ApprovalState::Expired;
    }
    approval
}

/// At capacity, drop the oldest finished records; waiting or usable ones stay.
fn prune(file: &mut ApprovalFile, now_ms: u64) {
    while file.approvals.len() >= MAX_APPROVALS {
        let Some(index) = file.approvals.iter().position(|approval| {
            !matches!(
                current(approval.clone(), now_ms).state,
                ApprovalState::Pending | ApprovalState::Approved
            )
        }) else {
            return;
        };
        file.approvals.remove(index);
    }
}

fn validate(approval: &Approval) -> Result<(), OrchestrationError> {
    valid_token(&approval.approval_id)?;
    approval.target.validate()?;
    valid_identifier(&approval.actor_id)?;
    valid_identifier(&approval.operation_id)?;
    if let Some(actor) = &approval.decided_by {
        valid_identifier(actor)?;
    }
    Ok(())
}
