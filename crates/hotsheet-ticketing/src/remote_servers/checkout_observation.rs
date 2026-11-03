//! HS2-4J6V46: authenticated bounded checkout observation and opaque local proof.
use std::path::PathBuf;

use super::registry::Binding;
use super::*;
use crate::SecretStore;
use crate::orchestration::{CheckoutProjection, ORCHESTRATION_VERSION, TargetFailure};

/// The trusted transport also owns response byte/time bounds, authenticated TLS,
/// no redirects/downgrade and literal origin pinning, exactly as the discovery port.
/// Metadata is an advertisement as-of this response, never a delegated grant.
pub trait RemoteCheckoutTransport: RemoteTransport {
    fn checkout_projection(
        &self,
        origin: &str,
        credentials: &RemoteCredentials,
        checkout_id: &str,
    ) -> Result<CheckoutProjection, TargetFailure>;
}

/// Process-local, registry-scoped proof. No serde/Debug/field access: no credential
/// reference, origin, private generation or local path can become a public DTO.
pub struct RemoteCheckoutProof {
    registry: PathBuf,
    binding: Binding,
}

impl RemoteRegistry {
    /// Capture reviewed authority BEFORE transport. Do not substitute a last-confirmed
    /// checkout or capture a later registry generation for an earlier peer response.
    /// No lock is held across credential access or either transport call.
    pub fn observe_checkout<T: RemoteCheckoutTransport + ?Sized>(
        &self,
        registration_id: &str,
        identity: &ReviewedRemoteIdentity,
        checkout_id: &str,
        secrets: &dyn SecretStore,
        transport: &T,
    ) -> Result<
        (
            RemoteCheckoutProof,
            Result<CheckoutProjection, TargetFailure>,
        ),
        RemoteError,
    > {
        let file = self.read()?;
        let binding = file
            .servers
            .iter()
            .find(|binding| binding.id == registration_id)
            .ok_or(RemoteError::NotFound)?
            .clone();
        let proof = RemoteCheckoutProof {
            registry: self.path.clone(),
            binding,
        };
        let result =
            self.observe_checkout_with(&proof.binding, identity, checkout_id, secrets, transport);
        Ok((proof, result))
    }

    fn observe_checkout_with<T: RemoteCheckoutTransport + ?Sized>(
        &self,
        binding: &Binding,
        identity: &ReviewedRemoteIdentity,
        checkout_id: &str,
        secrets: &dyn SecretStore,
        transport: &T,
    ) -> Result<CheckoutProjection, TargetFailure> {
        if binding.revoked {
            return Err(TargetFailure::Revoked);
        }
        if binding.identity != *identity {
            return Err(TargetFailure::IdentityChanged);
        }
        crate::orchestration::contract::valid_identifier(checkout_id)
            .map_err(|_| TargetFailure::InvalidResponse)?;
        let credentials =
            super::workflow::read_credentials(binding, secrets).map_err(TargetFailure::from)?;
        if credentials.trust_fingerprint(&binding.origin).ok().as_ref()
            != Some(&binding.identity.ca_fingerprint)
        {
            return Err(TargetFailure::TrustFailed);
        }
        let snapshot =
            super::workflow::verified_probe(transport, &binding.origin, &credentials, identity)
                .map_err(TargetFailure::from)?;
        let checkout = snapshot
            .checkouts
            .iter()
            .find(|checkout| checkout.id == checkout_id)
            .ok_or(TargetFailure::SourceMissing)?;
        match checkout.state {
            RemoteCheckoutState::Available => {}
            RemoteCheckoutState::Inaccessible => return Err(TargetFailure::SourceMissing),
            RemoteCheckoutState::IdentityChanged => return Err(TargetFailure::IdentityChanged),
        }
        if checkout.resolution_metadata_version != Some(ORCHESTRATION_VERSION) {
            return Err(TargetFailure::UnsupportedMetadata);
        }
        let projection =
            transport.checkout_projection(&binding.origin, &credentials, checkout_id)?;
        projection.validate().map_err(|error| match error {
            crate::orchestration::OrchestrationError::Observation(failure) => failure,
            crate::orchestration::OrchestrationError::Capacity => TargetFailure::ResponseLimit,
            _ => TargetFailure::InvalidResponse,
        })?;
        if projection.server_id != identity.server_id || projection.checkout_id != checkout_id {
            return Err(TargetFailure::IdentityChanged);
        }
        Ok(projection)
    }

    /// Nest last (checkout -> provider -> manifest -> bindings -> remote), retaining
    /// this guard until the caller's durable local commit/result completes. This
    /// fences LOCAL review/revoke/discovery, not changes on the remote peer.
    pub fn with_checkout_observation<T>(
        &self,
        proof: &RemoteCheckoutProof,
        commit: impl FnOnce() -> T,
    ) -> Result<T, RemoteError> {
        if proof.registry != self.path {
            return Err(RemoteError::Conflict);
        }
        let _guard = self.lock()?;
        let file = self.read()?;
        let current = file
            .servers
            .iter()
            .find(|binding| binding.id == proof.binding.id)
            .ok_or(RemoteError::Conflict)?;
        if current.revision != proof.binding.revision
            || current.observation_generation != proof.binding.observation_generation
            || current.identity != proof.binding.identity
            || current.origin != proof.binding.origin
            || current.credential_ref != proof.binding.credential_ref
            || current.revoked != proof.binding.revoked
        {
            return Err(RemoteError::Conflict);
        }
        Ok(commit())
    }
}
