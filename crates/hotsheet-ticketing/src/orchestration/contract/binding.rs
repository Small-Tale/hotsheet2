use serde::{Deserialize, Serialize};

use super::{
    DeclarationState, ResolutionState, TargetDeclaration, TargetDestination, TargetFailure,
    TargetRef, TargetSource,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BindingAction {
    Bind,
    Review,
    Unbind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BindingAudit {
    pub revision: u64,
    pub action: BindingAction,
    pub previous_destination: Option<TargetDestination>,
    pub destination: Option<TargetDestination>,
    pub at_ms: u64,
}

/// Last-confirmed identifiers only. A cached capability is never usable authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CachedTargetSource {
    pub connection_id: String,
    pub provider: String,
    pub identity: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BindingView {
    pub target: TargetRef,
    pub revision: u64,
    pub destination: Option<TargetDestination>,
    pub checked_at_ms: Option<u64>,
    pub confirmed_at_ms: Option<u64>,
    pub last_failure: Option<TargetFailure>,
    pub last_confirmed_sources: Vec<CachedTargetSource>,
    pub review_history: Vec<BindingAudit>,
    pub stale: bool,
}

impl BindingView {
    pub(crate) fn absent(target: TargetRef) -> Self {
        Self {
            target,
            revision: 0,
            destination: None,
            checked_at_ms: None,
            confirmed_at_ms: None,
            last_failure: None,
            last_confirmed_sources: Vec::new(),
            review_history: Vec::new(),
            stale: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetResolution {
    pub declaration: TargetDeclaration,
    pub binding: BindingView,
    pub state: ResolutionState,
    pub reason: Option<TargetFailure>,
    pub stale: bool,
    /// Fresh selection/identity only. Actions still need their own authorization/grants.
    pub usable: bool,
    /// Only the final guarded fresh result may return capabilities.
    pub sources: Option<Vec<TargetSource>>,
}

impl TargetResolution {
    pub(crate) fn inactive(declaration: TargetDeclaration, binding: BindingView) -> Self {
        let removed = declaration.state == DeclarationState::Removed;
        Self {
            declaration,
            binding,
            state: if removed {
                ResolutionState::Unreachable
            } else {
                ResolutionState::Unbound
            },
            reason: removed.then_some(TargetFailure::TargetRemoved),
            stale: true,
            usable: false,
            sources: None,
        }
    }
}
