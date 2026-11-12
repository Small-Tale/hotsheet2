//! The backend contract. Every call blocks; async callers run it on a blocking thread.

use std::time::Duration;

use crate::error::ClaimError;
use crate::model::{
    AcquireRequest, Capabilities, Claim, Grant, Namespace, ResourceRef, WaitOutcome,
};

/// One claim backend. Implementations must make `acquire` atomic (compare-and-swap) and
/// hand out a fencing token greater than every earlier one for that resource, or else
/// advertise [`crate::Guarantee::AdvisoryOnly`]. Expiry always uses the backend's clock.
pub trait ClaimBackend: Send + Sync {
    /// What this backend guarantees and how it waits.
    fn capabilities(&self) -> Capabilities;

    /// Take the resource now, or fail with [`ClaimError::HeldBy`].
    fn acquire(&self, target: &ResourceRef, request: &AcquireRequest) -> Result<Grant, ClaimError>;

    /// Take the resource, blocking up to `timeout` for a release or an expiry. Fails with
    /// [`ClaimError::WaitTimedOut`] naming the holder. Never polls on a fixed interval.
    fn acquire_wait(
        &self,
        target: &ResourceRef,
        request: &AcquireRequest,
        timeout: Duration,
    ) -> Result<Grant, ClaimError>;

    /// Extend the token's own claim to `duration` from the backend's now.
    fn renew(&self, renewal_token: &str, duration: Duration) -> Result<Claim, ClaimError>;

    /// End the token's own claim early and revoke the token. Returns the ended claim.
    fn release(&self, renewal_token: &str) -> Result<Claim, ClaimError>;

    /// The live claim on a resource, if any. Never acquires.
    fn get(&self, target: &ResourceRef) -> Result<Option<Claim>, ClaimError>;

    /// Every live claim in a namespace, by resource name.
    fn list(&self, namespace: &Namespace) -> Result<Vec<Claim>, ClaimError>;

    /// Block until the resource is free or `timeout` passes, without acquiring it.
    fn wait_free(&self, target: &ResourceRef, timeout: Duration)
    -> Result<WaitOutcome, ClaimError>;

    /// Check a fencing token a protected resource received. `Ok` only while it is the
    /// live claim's token; [`ClaimError::NotHolder`] once a later grant exists and
    /// [`ClaimError::Expired`] once the lease lapsed.
    fn verify_fencing(&self, target: &ResourceRef, fencing_token: u64) -> Result<(), ClaimError>;
}
