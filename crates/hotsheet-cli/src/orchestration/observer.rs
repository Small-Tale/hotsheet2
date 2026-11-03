use hotsheet_ticketing::checkouts::CheckoutSourceProof;
use hotsheet_ticketing::orchestration::{
    InspectDestination, InspectTarget, ObservedTarget, TargetDestination, TargetInspection,
    TargetObserver,
};
use hotsheet_ticketing::provider::ProviderConfigurationProof;
use hotsheet_ticketing::remote_servers::RemoteCheckoutProof;

use super::*;

pub(super) struct RuntimeObserver<'a> {
    runtime: &'a OrchestrationRuntime,
    owner: &'a Checkout,
}

/// Trusted process-local bundle only; never serialize generations/configuration,
/// credentials, source locators or paths into an API request/response.
pub struct RuntimeProof {
    owner: CheckoutSourceProof,
    configuration: ProviderConfigurationProof,
    local_destination: Option<CheckoutSourceProof>,
    remote: Option<RemoteCheckoutProof>,
}

enum Selection<'a> {
    Local {
        server_id: &'a str,
        checkout_id: &'a str,
    },
    Remote {
        registration_id: &'a str,
        identity: &'a hotsheet_ticketing::remote_servers::ReviewedRemoteIdentity,
        checkout_id: &'a str,
    },
}

impl<'a> RuntimeObserver<'a> {
    pub(super) fn new(runtime: &'a OrchestrationRuntime, owner: &'a Checkout) -> Self {
        Self { runtime, owner }
    }
    pub(super) fn capture(
        &self,
        target: &TargetRef,
        destination: &TargetDestination,
    ) -> Result<(RuntimeProof, Result<CheckoutProjection, TargetFailure>), OrchestrationError> {
        if target.owner_checkout_id != self.owner.id {
            return Err(OrchestrationError::Invalid("checkout owner mismatch"));
        }
        let selection = match destination {
            TargetDestination::Local {
                server_id,
                checkout_id,
                ..
            } => Selection::Local {
                server_id,
                checkout_id,
            },
            TargetDestination::Remote {
                registration_id,
                identity,
                checkout_id,
                ..
            } => Selection::Remote {
                registration_id,
                identity,
                checkout_id,
            },
        };
        self.capture_selection(selection)
    }

    fn capture_selection(
        &self,
        selection: Selection<'_>,
    ) -> Result<(RuntimeProof, Result<CheckoutProjection, TargetFailure>), OrchestrationError> {
        let (_, owner) = self
            .runtime
            .checkouts
            .observe_sources(&self.owner.id)
            .map_err(checkout_error)?;
        let (connections, configuration) = self
            .runtime
            .providers
            .observe_configuration()
            .map_err(|_| OrchestrationError::Corrupt)?;
        let mut proof = RuntimeProof {
            owner,
            configuration,
            local_destination: None,
            remote: None,
        };
        let result = match selection {
            Selection::Local {
                server_id,
                checkout_id,
            } => {
                if server_id != self.runtime.server_id.as_str() {
                    Err(TargetFailure::IdentityChanged)
                } else {
                    match self.runtime.checkouts.observe_sources(checkout_id) {
                        Ok((checkout, links)) if checkout.id.as_str() == checkout_id => {
                            let projection = links.resolution_projection(
                                &self.runtime.server_id,
                                &external_sources(&checkout, &connections),
                            );
                            proof.local_destination = Some(links);
                            projection.map_err(|error| match error {
                                OrchestrationError::Capacity => TargetFailure::ResponseLimit,
                                OrchestrationError::Observation(failure) => failure,
                                _ => TargetFailure::InvalidResponse,
                            })
                        }
                        Ok(_) => Err(TargetFailure::IdentityChanged),
                        Err(_) => Err(TargetFailure::SourceMissing),
                    }
                }
            }
            Selection::Remote {
                registration_id,
                identity,
                checkout_id,
            } => {
                match self.runtime.remotes.observe_checkout(
                    registration_id,
                    identity,
                    checkout_id,
                    self.runtime.secrets.as_ref(),
                    self.runtime.transport.as_ref(),
                ) {
                    Ok((remote, result)) => {
                        proof.remote = Some(remote);
                        result
                    }
                    // No registration is never live authority. Its failure can be
                    // recorded without a remote proof; a fresh attempt can recover.
                    Err(RemoteError::NotFound) => Err(TargetFailure::SourceMissing),
                    Err(error) => return Err(remote_error(error)),
                }
            }
        };
        Ok((proof, result))
    }

    pub(super) fn inspect(
        &self,
        request: InspectTarget,
    ) -> Result<TargetInspection, OrchestrationError> {
        request.validate()?;
        self.with_source_guard(&self.owner.id, Path::new(&self.owner.root), None, || Ok(()))?;
        let mut remote_identity = None;
        let selection = match &request.destination {
            InspectDestination::Local { checkout_id } => Selection::Local {
                server_id: &self.runtime.server_id,
                checkout_id,
            },
            InspectDestination::Remote {
                registration_id,
                checkout_id,
            } => {
                remote_identity = Some(
                    self.runtime
                        .remotes
                        .list()
                        .map_err(remote_error)?
                        .into_iter()
                        .find(|remote| remote.id == *registration_id)
                        .ok_or(OrchestrationError::Observation(
                            TargetFailure::SourceMissing,
                        ))?
                        .identity,
                );
                Selection::Remote {
                    registration_id,
                    identity: remote_identity
                        .as_ref()
                        .ok_or(OrchestrationError::Corrupt)?,
                    checkout_id,
                }
            }
        };
        let (proof, projection) = self.capture_selection(selection)?;
        self.with_source_guard(
            &self.owner.id,
            Path::new(&self.owner.root),
            Some(&proof),
            || {
                self.with_remote_guard(&proof, || {
                    let projection = projection.map_err(OrchestrationError::Observation)?;
                    let destination = match request.destination {
                        InspectDestination::Local { .. } => TargetDestination::Local {
                            server_id: projection.server_id.clone(),
                            checkout_id: projection.checkout_id.clone(),
                            checkout_authority: projection.checkout_authority.clone(),
                        },
                        InspectDestination::Remote {
                            registration_id, ..
                        } => TargetDestination::Remote {
                            registration_id,
                            identity: remote_identity.ok_or(OrchestrationError::Corrupt)?,
                            checkout_id: projection.checkout_id.clone(),
                            checkout_authority: projection.checkout_authority.clone(),
                        },
                    };
                    TargetInspection::verified(destination, projection)
                })
            },
        )
    }
}

impl TargetObserver for RuntimeObserver<'_> {
    type Proof = RuntimeProof;

    fn observe(
        &self,
        target: &TargetRef,
        destination: &TargetDestination,
    ) -> Result<ObservedTarget<RuntimeProof>, OrchestrationError> {
        let (proof, result) = self.capture(target, destination)?;
        Ok(ObservedTarget::new(proof, result))
    }

    fn with_source_guard<T>(
        &self,
        owner_checkout_id: &str,
        checkout_root: &Path,
        proof: Option<&RuntimeProof>,
        commit: impl FnOnce() -> Result<T, OrchestrationError>,
    ) -> Result<T, OrchestrationError> {
        if owner_checkout_id != self.owner.id || checkout_root != Path::new(&self.owner.root) {
            return Err(OrchestrationError::Invalid("checkout owner mismatch"));
        }
        let links = proof
            .map(|proof| {
                std::iter::once(&proof.owner)
                    .chain(proof.local_destination.iter())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        self.runtime
            .checkouts
            .with_source_observations(owner_checkout_id, checkout_root, &links, |owner| {
                if owner.sources.is_empty() {
                    return Err(OrchestrationError::Observation(
                        TargetFailure::SourceMissing,
                    ));
                }
                match proof {
                    Some(proof) => ProviderConfigRegistry::with_configuration_observations(
                        &[(&self.runtime.providers, &proof.configuration)],
                        commit,
                    )
                    .map_err(|_| OrchestrationError::Conflict)?,
                    None => self
                        .runtime
                        .providers
                        .with_configuration_observation(None, |_, _| commit())
                        .map_err(|_| OrchestrationError::Conflict)?,
                }
            })
            .map_err(|_| OrchestrationError::Conflict)?
    }

    fn with_remote_guard<T>(
        &self,
        proof: &RuntimeProof,
        commit: impl FnOnce() -> Result<T, OrchestrationError>,
    ) -> Result<T, OrchestrationError> {
        match &proof.remote {
            Some(remote) => self
                .runtime
                .remotes
                .with_checkout_observation(remote, commit)
                .map_err(remote_error)?,
            None => commit(),
        }
    }
}
