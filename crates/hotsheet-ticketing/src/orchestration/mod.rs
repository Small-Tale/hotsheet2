//! HS2-4J6V46: shared target declarations and reviewed machine-local bindings.
//! Network, provider metadata, clock and randomness remain injected boundaries.

mod bindings;
pub(crate) mod contract;
mod manifest;
mod storage;
mod workflow;

pub use bindings::BindingRegistry;
pub use contract::*;
pub use manifest::ManifestStore;
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
mod binding_tests;
#[cfg(test)]
mod tests;
