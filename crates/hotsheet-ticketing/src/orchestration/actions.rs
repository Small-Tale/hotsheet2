//! HS2-XHQ1EH: the vocabulary of one orchestrator action on a target project.
//!
//! Adding a project (declaring the target and binding it to a checkout) is the consent
//! to read and follow it. What acts on it is decided per action: observation never
//! asks, reducing activity never asks, and anything else asks a person through the
//! permission bridge unless a remembered rule allows it (`permissions.rs`). The grants
//! of HS2-SBQ3T8 are retired; their file is reported and never read as authority.

use serde::{Deserialize, Serialize};

use super::contract::{bounded_text, valid_identifier};
use super::*;

/// What an orchestration project may want to do in a target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetAction {
    TicketsRead,
    TicketsWrite,
    WorkStart,
    GatesObserve,
    ValidationRun,
    SessionsManage,
    /// HS2-5JBYPV: request a deployment of a gated commit through the target's script.
    DeploymentsRun,
}

impl TargetAction {
    pub const ALL: [Self; 7] = [
        Self::TicketsRead,
        Self::TicketsWrite,
        Self::WorkStart,
        Self::GatesObserve,
        Self::ValidationRun,
        Self::SessionsManage,
        Self::DeploymentsRun,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Self::TicketsRead => "tickets_read",
            Self::TicketsWrite => "tickets_write",
            Self::WorkStart => "work_start",
            Self::GatesObserve => "gates_observe",
            Self::ValidationRun => "validation_run",
            Self::SessionsManage => "sessions_manage",
            Self::DeploymentsRun => "deployments_run",
        }
    }

    /// Reading and following a target never asks: adding the project consented to it.
    pub fn observes(self) -> bool {
        matches!(self, Self::TicketsRead | Self::GatesObserve)
    }
}

/// The session a `sessions_manage` request concerns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionRequest {
    pub tool: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Orchestrator-owned sessions already running on the target. Kept for callers
    /// that state it; the policy's caps use [`SessionLoad`].
    #[serde(default)]
    pub running: u32,
    /// Starting acts on the target; stopping or winding down only reduces activity.
    #[serde(default)]
    pub starting: bool,
}

impl SessionRequest {
    pub fn validate(&self) -> Result<(), OrchestrationError> {
        valid_identifier(&self.tool)?;
        if let Some(model) = &self.model {
            valid_model(model)?;
        }
        Ok(())
    }
}

/// One action as a side-effect-free check or a dry run states it.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccessRequest {
    pub actor_id: String,
    pub action: TargetAction,
    #[serde(default)]
    pub session: Option<SessionRequest>,
    #[serde(default)]
    pub operation_id: Option<String>,
    /// HS2-YCJDEB: sessions already running, for the policy's concurrency caps.
    #[serde(default)]
    pub load: Option<SessionLoad>,
    /// The work would start on a prerequisite whose gate is still pending.
    #[serde(default)]
    pub provisional: bool,
    /// The session is a target's integrator or dispatcher rather than a worker.
    #[serde(default)]
    pub role_session: bool,
}

impl AccessRequest {
    pub fn new(actor_id: impl Into<String>, action: TargetAction) -> Self {
        Self {
            actor_id: actor_id.into(),
            action,
            session: None,
            operation_id: None,
            load: None,
            provisional: false,
            role_session: false,
        }
    }

    /// Whether the action only lowers activity on the target: stopping or winding a
    /// session down. Such an action never asks and is never denied by the policy.
    pub fn reduces(&self) -> bool {
        self.action == TargetAction::SessionsManage
            && self
                .session
                .as_ref()
                .is_some_and(|session| !session.starting)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DenialReason {
    /// The target could not be freshly resolved as usable.
    TargetUnavailable,
    /// The orchestration policy's limits do not permit the action (HS2-YCJDEB).
    PolicyDenied,
    /// A person has not answered the permission request for this operation yet.
    PermissionRequired,
    /// A person denied the permission request for this operation; that is final.
    PermissionDenied,
    /// A judgment call waits on a person's answer on the orchestration ticket.
    DecisionRequired,
}

impl DenialReason {
    pub fn code(self) -> &'static str {
        match self {
            Self::TargetUnavailable => "target_unavailable",
            Self::PolicyDenied => "policy_denied",
            Self::PermissionRequired => "permission_required",
            Self::PermissionDenied => "permission_denied",
            Self::DecisionRequired => "decision_required",
        }
    }
}

/// How the gate would answer, or answered, one action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessDecision {
    pub allowed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<DenialReason>,
    /// The specific policy limit behind a `policy_denied` answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Why no person was asked: `observation`, `reduces_activity`, `person`,
    /// `remembered_rule` or `allowed_once`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub basis: Option<String>,
    /// The permission request this answer concerns (pending, denied or allowed once).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    /// The remembered rule that allowed it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rule_id: Option<String>,
}

impl AccessDecision {
    pub fn denied(reason: DenialReason) -> Self {
        Self {
            allowed: false,
            reason: Some(reason),
            detail: None,
            basis: None,
            request_id: None,
            rule_id: None,
        }
    }

    pub fn allowed(basis: &str) -> Self {
        Self {
            allowed: true,
            reason: None,
            detail: None,
            basis: Some(basis.into()),
            request_id: None,
            rule_id: None,
        }
    }
}

pub(crate) fn valid_model(model: &str) -> Result<(), OrchestrationError> {
    if !bounded_text(model, 128) || model.chars().any(char::is_whitespace) {
        return Err(OrchestrationError::Invalid("bounded model name required"));
    }
    Ok(())
}
