//! HS2-6H8ZY6: fixed GitHub endpoint decisions and provider-domain transactions.
//! Read proofs fence selected-source token/provider adapters; link/write adopters remain separate.

mod registry;
mod state;

pub use registry::{
    Compensation, EndpointRegistry, LinkObserver, Mutation, OrdinaryWrite, ReadProof, Recovery,
    SourceProof,
};
pub use state::{
    BundleOrigin, Decision, Endpoint, EndpointError, Expected, Identity, Intent, Link, Nullable,
    Observation, Operation, OperationState, Request, State, admit_read, guard_ordinary,
    validate_bundle_origin,
};

#[cfg(test)]
mod tests;
#[cfg(test)]
mod writer_tests;
