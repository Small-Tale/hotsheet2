//! HS2-4J6V46: shared CLI/server/MCP composition. Domain transitions remain in core.
//! Host injects its actual registry/config/secret owners; no store/path discovery,
//! provider account probe, implicit source creation or credential copying.
mod observer;
mod request;
pub use request::{is_cleanup_request, is_machine_path, is_path, request_cleanup_now};
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use hotsheet_ticketing::checkouts::{Checkout, CheckoutError, CheckoutRegistry};
use hotsheet_ticketing::orchestration::{
    AddTarget, BindTarget, BindingRegistry, BindingView, CheckoutProjection, ManifestStore,
    ManifestView, Orchestration, OrchestrationError, RemoveTarget, RenameTarget, ReplaceTargets,
    TargetFailure, TargetRef, TargetResolution, Transaction, UnbindTarget,
};
use hotsheet_ticketing::remote_servers::{RemoteCheckoutTransport, RemoteError, RemoteRegistry};
use hotsheet_ticketing::{ProviderConfigRegistry, ProviderConnection, SecretStore};

use observer::RuntimeObserver;

pub struct OrchestrationRuntime {
    home: PathBuf,
    checkouts: CheckoutRegistry,
    providers: ProviderConfigRegistry,
    remotes: RemoteRegistry,
    secrets: Arc<dyn SecretStore + Send + Sync>,
    transport: Arc<dyn RemoteCheckoutTransport + Send + Sync>,
    server_id: String,
}

impl OrchestrationRuntime {
    /// server_id comes from the existing durable machine identity owner, not a
    /// process/checkout/store id. Construction neither creates it nor opens secrets.
    pub fn new(
        home: PathBuf,
        checkouts: CheckoutRegistry,
        providers: ProviderConfigRegistry,
        secrets: Arc<dyn SecretStore + Send + Sync>,
        transport: Arc<dyn RemoteCheckoutTransport + Send + Sync>,
        server_id: String,
    ) -> Result<Self, OrchestrationError> {
        if server_id
            .parse::<ulid::Ulid>()
            .ok()
            .map(|id| id.to_string())
            .as_deref()
            != Some(server_id.as_str())
        {
            return Err(OrchestrationError::Invalid(
                "canonical server identity required",
            ));
        }
        Ok(Self {
            remotes: RemoteRegistry::new(home.join("remotes.json")),
            home,
            checkouts,
            providers,
            secrets,
            transport,
            server_id,
        })
    }

    fn owner(&self, id: &str) -> Result<Checkout, OrchestrationError> {
        let checkout = self.checkouts.describe(id).map_err(checkout_error)?;
        if checkout.id != id {
            return Err(OrchestrationError::Invalid(
                "registered checkout id required",
            ));
        }
        Ok(checkout)
    }

    fn flow(&self, owner: &Checkout) -> Result<Orchestration, OrchestrationError> {
        Orchestration::new(
            owner.id.clone(),
            ManifestStore::new(Path::new(&owner.root)),
            BindingRegistry::new(&self.home),
        )
    }

    /// Ordinary list paths never touch transport or the credential port.
    pub fn declarations(&self, owner_id: &str) -> Result<ManifestView, OrchestrationError> {
        self.flow(&self.owner(owner_id)?)?.declarations()
    }

    pub fn bindings(&self, owner_id: &str) -> Result<Vec<BindingView>, OrchestrationError> {
        let owner = self.owner(owner_id)?;
        let flow = self.flow(&owner)?;
        flow.declarations()?
            .targets
            .iter()
            .map(|target| {
                flow.binding(&TargetRef {
                    owner_checkout_id: owner.id.clone(),
                    target_id: target.target_id.clone(),
                })
            })
            .collect()
    }

    pub fn replace(
        &self,
        owner_id: &str,
        request: ReplaceTargets,
        tx: &mut Transaction<'_>,
    ) -> Result<ManifestView, OrchestrationError> {
        let owner = self.owner(owner_id)?;
        self.flow(&owner)?
            .replace(request, &RuntimeObserver::new(self, &owner), tx)
    }

    pub fn add(
        &self,
        owner_id: &str,
        request: AddTarget,
        tx: &mut Transaction<'_>,
    ) -> Result<ManifestView, OrchestrationError> {
        let owner = self.owner(owner_id)?;
        self.flow(&owner)?
            .add(request, &RuntimeObserver::new(self, &owner), tx)
    }

    pub fn rename(
        &self,
        target: &TargetRef,
        request: RenameTarget,
        tx: &mut Transaction<'_>,
    ) -> Result<ManifestView, OrchestrationError> {
        let owner = self.owner(&target.owner_checkout_id)?;
        self.flow(&owner)?
            .rename(target, request, &RuntimeObserver::new(self, &owner), tx)
    }

    pub fn remove(
        &self,
        target: &TargetRef,
        request: RemoveTarget,
        tx: &mut Transaction<'_>,
    ) -> Result<ManifestView, OrchestrationError> {
        let owner = self.owner(&target.owner_checkout_id)?;
        self.flow(&owner)?
            .remove(target, request, &RuntimeObserver::new(self, &owner), tx)
    }

    /// Reuse existing identity and native-secret owners; constructing the backend
    /// does not access a secret. Cleanup dispatch must precede this constructor.
    pub fn live(
        home: &Path,
        checkouts: CheckoutRegistry,
        providers: ProviderConfigRegistry,
    ) -> Result<Self, OrchestrationError> {
        let identity = hotsheet_ticketing::remote_servers::server_identity(home, || {
            ulid::Ulid::new().to_string()
        })
        .map_err(remote_error)?;
        Self::new(
            home.into(),
            checkouts,
            providers,
            Arc::new(hotsheet_ticketing::OsKeychain::at(home)),
            Arc::new(crate::remote_servers::MtlsRemoteTransport),
            identity.server_id,
        )
    }

    pub fn inspect(
        &self,
        owner_id: &str,
        request: hotsheet_ticketing::orchestration::InspectTarget,
    ) -> Result<hotsheet_ticketing::orchestration::TargetInspection, OrchestrationError> {
        let owner = self.owner(owner_id)?;
        RuntimeObserver::new(self, &owner).inspect(request)
    }

    pub fn bind(
        &self,
        request: BindTarget,
        tx: &mut Transaction<'_>,
    ) -> Result<TargetResolution, OrchestrationError> {
        let owner = self.owner(&request.target.owner_checkout_id)?;
        self.flow(&owner)?
            .bind(request, &RuntimeObserver::new(self, &owner), tx)
    }

    pub fn resolve(
        &self,
        target: &TargetRef,
        tx: &mut Transaction<'_>,
    ) -> Result<TargetResolution, OrchestrationError> {
        let owner = self.owner(&target.owner_checkout_id)?;
        self.flow(&owner)?
            .resolve(target, &RuntimeObserver::new(self, &owner), tx)
    }

    /// Read the machine key for orphan cleanup without source/manifest/identity dependencies.
    pub fn binding_at(home: &Path, target: &TargetRef) -> Result<BindingView, OrchestrationError> {
        BindingRegistry::new(home).get(target)
    }

    /// Orphan recovery deliberately needs no registered owner/root, shared manifest,
    /// server identity, provider configuration, transport or credential backend.
    pub fn unbind_at(
        home: &Path,
        request: UnbindTarget,
        tx: &mut Transaction<'_>,
    ) -> Result<BindingView, OrchestrationError> {
        BindingRegistry::new(home).unbind(request, tx)
    }

    /// Hosting-side path-free metadata, also reused for local destinations. Read
    /// configuration under held local guards; never contact a provider or secrets.
    pub fn checkout_projection(
        &self,
        checkout_id: &str,
    ) -> Result<CheckoutProjection, OrchestrationError> {
        let (checkout, links) = self
            .checkouts
            .observe_sources(checkout_id)
            .map_err(checkout_error)?;
        if checkout.id != checkout_id {
            return Err(OrchestrationError::Invalid(
                "registered checkout id required",
            ));
        }
        let (connections, configuration) = self
            .providers
            .observe_configuration()
            .map_err(|_| OrchestrationError::Corrupt)?;
        self.checkouts
            .with_source_observations(
                checkout_id,
                Path::new(&checkout.root),
                &[&links],
                |current| {
                    ProviderConfigRegistry::with_configuration_observations(
                        &[(&self.providers, &configuration)],
                        || {
                            links.resolution_projection(
                                &self.server_id,
                                &external_sources(current, &connections),
                            )
                        },
                    )
                    .map_err(|_| OrchestrationError::Conflict)?
                },
            )
            .map_err(|_| OrchestrationError::Conflict)?
    }
}

fn external_sources(
    checkout: &Checkout,
    connections: &[ProviderConnection],
) -> Vec<hotsheet_ticketing::orchestration::TargetSource> {
    checkout
        .sources
        .iter()
        .filter(|source| source.provider != "git")
        .filter_map(|source| {
            connections
                .iter()
                .find(|connection| connection.id == source.connection_id)
                .map(hotsheet_extsync::source_resolution)
        })
        .collect()
}

fn checkout_error(error: CheckoutError) -> OrchestrationError {
    match error {
        CheckoutError::NotFound(_) | CheckoutError::Missing(_) => {
            OrchestrationError::Observation(TargetFailure::SourceMissing)
        }
        CheckoutError::Io(error) => OrchestrationError::Storage(error),
        CheckoutError::UpgradeRequired { .. } => {
            OrchestrationError::Observation(TargetFailure::UnsupportedMetadata)
        }
        CheckoutError::Invalid(_)
        | CheckoutError::Ambiguous(_)
        | CheckoutError::Store(_)
        | CheckoutError::Endpoint(_) => OrchestrationError::Corrupt,
    }
}

fn remote_error(error: RemoteError) -> OrchestrationError {
    match error {
        RemoteError::Invalid(_) => {
            OrchestrationError::Invalid("invalid remote observation request")
        }
        RemoteError::UnsupportedVersion => {
            OrchestrationError::Observation(TargetFailure::UnsupportedMetadata)
        }
        RemoteError::Corrupt => OrchestrationError::Corrupt,
        RemoteError::Conflict => OrchestrationError::Conflict,
        RemoteError::NotFound => OrchestrationError::Observation(TargetFailure::SourceMissing),
        RemoteError::Revoked => OrchestrationError::Observation(TargetFailure::Revoked),
        RemoteError::Verification(failure) => OrchestrationError::Observation(failure.into()),
        RemoteError::Credentials => {
            OrchestrationError::Observation(TargetFailure::MissingCredentials)
        }
        RemoteError::Storage(error) => OrchestrationError::Storage(error),
    }
}

/// One status mapping for HTTP and MCP; messages use Display/code only, never source chains.
pub fn error_status(error: &OrchestrationError) -> u16 {
    match error {
        OrchestrationError::Invalid(_) => 400,
        OrchestrationError::Conflict | OrchestrationError::Capacity => 409,
        OrchestrationError::NotFound => 404,
        OrchestrationError::Removed => 410,
        OrchestrationError::UnsupportedVersion { .. } => 426,
        OrchestrationError::Observation(_) => 502,
        OrchestrationError::Corrupt => 500,
        OrchestrationError::Storage(_) => 503,
    }
}
