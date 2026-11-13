//! HS2-4J6V46: shared target declarations and reviewed machine-local bindings.
//! Network, provider metadata, clock and randomness remain injected boundaries.

mod approvals;
mod audit;
mod bindings;
pub(crate) mod contract;
mod grants;
mod manifest;
mod policy;
mod storage;
mod workflow;

pub use approvals::{
    APPROVAL_PENDING_MS, APPROVAL_VALID_MS, Approval, ApprovalRegistry, ApprovalState,
    MAX_APPROVALS,
};
pub use audit::{
    AuditEntry, AuditJournal, AuditOutcome, AuditPage, AuditRecord, MAX_AUDIT_PAGE,
    MAX_AUDIT_SEGMENT_BYTES,
};
pub use bindings::BindingRegistry;
pub use contract::*;
pub use grants::{
    AccessDecision, AccessRequest, CreateGrant, DenialReason, Grant, GrantRegistry, GrantState,
    GrantView, MAX_GRANT_CONCURRENCY, MAX_GRANT_LIFETIME_MS, MAX_GRANT_MODELS, MAX_GRANT_TOOLS,
    MAX_GRANTS, MAX_GRANTS_PER_TARGET, SessionRequest, SessionScope, TargetAction, ToolScope,
    destination_key,
};
pub use manifest::ManifestStore;
pub use policy::{
    AccountPolicy, ApprovalGate, ApprovalPolicy, MAX_POLICY_ACCOUNTS, MAX_POLICY_SESSIONS,
    MAX_POLICY_TOOLS, MAX_SPECULATION_DEPTH, MachineLimits, PolicyBody, PolicyDenial,
    PolicyProblem, PolicyQuestion, PolicyStore, PolicyVerdict, PolicyView, SessionLoad, SetPolicy,
    Speculation, TargetPolicy, ToolPolicy, WindDown,
};
pub use workflow::{ObservedTarget, Orchestration, TargetObserver};

#[derive(Debug, thiserror::Error)]
pub enum OrchestrationError {
    #[error("invalid orchestration request: {0}")]
    Invalid(&'static str),
    #[error("orchestration state changed; read it and review again")]
    Conflict,
    #[error("orchestration target was not found")]
    NotFound,
    #[error("orchestration target was removed")]
    Removed,
    #[error("orchestration format version {found} is unsupported (supported: 1)")]
    UnsupportedVersion { found: u64 },
    #[error("orchestration data is malformed")]
    Corrupt,
    #[error("orchestration capacity reached")]
    Capacity,
    #[error("orchestration observation failed: {0}")]
    Observation(TargetFailure),
    #[error("orchestration storage is unavailable")]
    Storage(#[source] std::io::Error),
    /// HS2-SBQ3T8: no grant (or policy) authorizes this actor's action on the target.
    #[error("orchestration action is not authorized: {}", .0.code())]
    Denied(DenialReason),
    /// HS2-SBQ3T8: the caller may never perform this operation, whatever is granted.
    #[error("orchestration operation is forbidden: {0}")]
    Forbidden(&'static str),
}

impl From<std::io::Error> for OrchestrationError {
    fn from(error: std::io::Error) -> Self {
        Self::Storage(error)
    }
}

impl OrchestrationError {
    /// Content-free adapter code; callers must not format Debug/source chains into DTOs.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Invalid(_) => "invalid_request",
            Self::Conflict => "conflict",
            Self::NotFound => "target_not_found",
            Self::Removed => "target_removed",
            Self::UnsupportedVersion { .. } => "upgrade_required",
            Self::Corrupt => "corrupt_orchestration_data",
            Self::Capacity => "capacity_reached",
            Self::Observation(failure) => failure.code(),
            Self::Storage(_) => "storage_unavailable",
            Self::Denied(reason) => reason.code(),
            Self::Forbidden(_) => "forbidden",
        }
    }
}

/// Runtime supplies the existing clock/RNG ports; no wall clock or global RNG in core.
pub struct Transaction<'a> {
    pub clock: &'a dyn crate::Clock,
    pub rng: &'a mut dyn crate::Rng,
}

impl Transaction<'_> {
    pub(crate) fn token(&mut self) -> String {
        crate::mint_ulid(self.clock, self.rng).to_string()
    }
}

#[cfg(test)]
mod approval_tests;
#[cfg(test)]
mod audit_tests;
#[cfg(test)]
mod binding_tests;
#[cfg(test)]
mod grant_tests;
#[cfg(test)]
mod policy_tests;
#[cfg(test)]
mod tests;
