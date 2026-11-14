//! HS2-WHR4GS: fences of withdrawn session operations on their host.
//!
//! An orchestrator ends a start its host never confirmed once the host's stop says it
//! has no record of the operation, or released a reservation it never launched. A
//! start request still in transit could reach the host after that stop and launch a
//! terminal under an operation the orchestrator already ended. The stop therefore
//! leaves a bounded tombstone, and a start for a fenced operation is refused. A fence
//! lasts [`WITHDRAWN_FENCE_MS`], longer than any one request and than the
//! orchestrator's own five-minute rule for an unconfirmed start.
use serde::{Deserialize, Serialize};

use super::contract::valid_identifier;
use super::*;

/// How long a withdrawn operation stays fenced.
pub const WITHDRAWN_FENCE_MS: u64 = 15 * 60 * 1000;
/// The most fences a host keeps; the oldest goes first beyond it.
pub const MAX_WITHDRAWN_FENCES: usize = 1024;

/// One withdrawn operation on one checkout of this machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WithdrawnOperation {
    pub checkout_id: String,
    pub operation_id: String,
    pub withdrawn_at_ms: u64,
}

impl WithdrawnOperation {
    /// Still fenced at `now_ms`. A clock that went back keeps the fence.
    pub fn holds_at(&self, now_ms: u64) -> bool {
        now_ms.saturating_sub(self.withdrawn_at_ms) < WITHDRAWN_FENCE_MS
    }
}

/// The host's fences, oldest first. Pure: the registry reads and writes it under the
/// hosted sessions' lock.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WithdrawnFences {
    pub withdrawn: Vec<WithdrawnOperation>,
}

impl WithdrawnFences {
    /// Drop the fences that expired by `now_ms`. True when any was dropped.
    pub fn prune(&mut self, now_ms: u64) -> bool {
        let before = self.withdrawn.len();
        self.withdrawn.retain(|fence| fence.holds_at(now_ms));
        self.withdrawn.len() != before
    }

    pub fn is_fenced(&self, checkout_id: &str, operation_id: &str, now_ms: u64) -> bool {
        self.withdrawn.iter().any(|fence| {
            fence.checkout_id == checkout_id
                && fence.operation_id == operation_id
                && fence.holds_at(now_ms)
        })
    }

    /// Fence an operation from `now_ms`. Fencing it again restarts its time, so a
    /// repeated stop never shortens the fence. Beyond [`MAX_WITHDRAWN_FENCES`] the
    /// oldest fence is dropped.
    pub fn fence(
        &mut self,
        checkout_id: &str,
        operation_id: &str,
        now_ms: u64,
    ) -> Result<(), OrchestrationError> {
        valid_identifier(checkout_id)?;
        valid_identifier(operation_id)?;
        self.prune(now_ms);
        self.withdrawn
            .retain(|fence| fence.checkout_id != checkout_id || fence.operation_id != operation_id);
        while self.withdrawn.len() >= MAX_WITHDRAWN_FENCES {
            self.withdrawn.remove(0);
        }
        self.withdrawn.push(WithdrawnOperation {
            checkout_id: checkout_id.into(),
            operation_id: operation_id.into(),
            withdrawn_at_ms: now_ms,
        });
        Ok(())
    }

    pub(super) fn validate(&self) -> Result<(), OrchestrationError> {
        if self.withdrawn.len() > MAX_WITHDRAWN_FENCES {
            return Err(OrchestrationError::Corrupt);
        }
        for fence in &self.withdrawn {
            valid_identifier(&fence.checkout_id).map_err(|_| OrchestrationError::Corrupt)?;
            valid_identifier(&fence.operation_id).map_err(|_| OrchestrationError::Corrupt)?;
        }
        Ok(())
    }
}

/// What a stop did to an operation's record on its host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Withdrawal {
    /// The terminal was launched: nothing is fenced and the stop goes on as usual.
    Launched(Box<HostedSession>),
    /// A reservation that never launched was released and the operation fenced.
    Released,
    /// The host had no record; the operation is fenced so a late start cannot launch.
    Unknown,
}
