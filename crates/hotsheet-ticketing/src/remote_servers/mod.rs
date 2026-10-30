//! HS2-1Y45MZ: machine-local reviewed remote bindings; transport and secrets are ports.

mod contract;
mod registry;
mod workflow;

pub use contract::{
    MAX_REMOTE_CHECKOUTS, MAX_REMOTE_RESPONSE_BYTES, REMOTE_PROTOCOL_VERSION, RegisterRemote,
    RemoteCheckout, RemoteCheckoutState, RemoteCredentials, RemoteDiscovery, RemoteFailure,
    RemoteIdentity, RemoteReview, RemoteSnapshot, RemoteSource, RemoteTransport, RemoteView,
    ReviewRemote, ReviewedRemoteIdentity, RevokeRemote, normalize_remote_origin,
    remote_registry_origin_is_loopback, validate_remote_checkouts,
};
pub use registry::{RemoteRegistry, server_identity};

#[derive(Debug, thiserror::Error)]
pub enum RemoteError {
    #[error("invalid remote registration: {0}")]
    Invalid(&'static str),
    #[error("remote registry has an unsupported version")]
    UnsupportedVersion,
    #[error("remote registry is malformed")]
    Corrupt,
    #[error("remote registration was changed; read it and review again")]
    Conflict,
    #[error("remote registration was not found")]
    NotFound,
    #[error("remote registration is revoked")]
    Revoked,
    #[error("remote verification failed: {0}")]
    Verification(RemoteFailure),
    // Never include a credential backend or peer response's text: it can contain secrets.
    #[error("remote credential storage is unavailable")]
    Credentials,
    #[error("remote registry storage failed")]
    Storage(#[source] std::io::Error),
}

impl From<std::io::Error> for RemoteError {
    fn from(error: std::io::Error) -> Self {
        Self::Storage(error)
    }
}

#[cfg(test)]
mod tests;
