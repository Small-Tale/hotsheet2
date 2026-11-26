//! The resource-claim error model shared by every backend and surface.

use crate::model::Claim;

#[derive(Debug, thiserror::Error)]
pub enum ClaimError {
    /// Another claim holds the resource right now.
    #[error("resource is held by claim {} until {}", holder.claim_id, holder.expires_at)]
    HeldBy { holder: Box<Claim> },
    /// The wait ended while another claim still held the resource.
    #[error("timed out waiting; resource is held by claim {} until {}", holder.claim_id, holder.expires_at)]
    WaitTimedOut { holder: Box<Claim> },
    /// The claim's lease lapsed before this call.
    #[error("the claim has expired")]
    Expired,
    /// The token names a claim that another claim has since replaced.
    #[error("the resource is now held by a different claim")]
    NotHolder,
    /// The renewal token is malformed, unknown, revoked, or names another claim.
    #[error("the renewal token is not valid for this claim")]
    TokenInvalid,
    /// The backend could not be reached; nothing was changed.
    #[error("claim backend is unreachable: {0}")]
    BackendUnreachable(String),
    /// The backend or this build does not offer the requested operation.
    #[error("unsupported: {0}")]
    Unsupported(String),
    /// The request itself is malformed (a name, a duration, metadata).
    #[error("invalid request: {0}")]
    Invalid(String),
    /// A bounded store is full.
    #[error("capacity reached: {0}")]
    Capacity(String),
    /// Stored state is unreadable or from an unknown schema; it is never overwritten.
    #[error("claim state is corrupt or from an unsupported schema")]
    Corrupt,
    /// A stop signal ended a wait: a process signal under an `InterruptGuard`, or a
    /// `WaitCancel` (HS2-2W2PTX). The waiter's registration was removed.
    #[error("the wait was interrupted by a signal")]
    Interrupted,
    #[error("claim storage failed: {0}")]
    Storage(#[from] std::io::Error),
}

impl ClaimError {
    /// A stable machine-readable code for CLI, MCP and HTTP callers.
    pub fn code(&self) -> &'static str {
        match self {
            Self::HeldBy { .. } => "held_by",
            Self::WaitTimedOut { .. } => "wait_timeout",
            Self::Expired => "expired",
            Self::NotHolder => "not_holder",
            Self::TokenInvalid => "token_invalid",
            Self::BackendUnreachable(_) => "backend_unreachable",
            Self::Unsupported(_) => "unsupported",
            Self::Invalid(_) => "invalid",
            Self::Capacity(_) => "capacity",
            Self::Corrupt => "corrupt",
            Self::Interrupted => "interrupted",
            Self::Storage(_) => "storage",
        }
    }

    /// The live holder a conflict names, if this error is one.
    pub fn holder(&self) -> Option<&Claim> {
        match self {
            Self::HeldBy { holder } | Self::WaitTimedOut { holder } => Some(holder),
            _ => None,
        }
    }

    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid(message.into())
    }
}
