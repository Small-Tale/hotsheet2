//! HS2-4J6V46: shared CLI/server/MCP composition. Domain transitions remain in core.
//! Host injects its actual registry/config/secret owners; no store/path discovery,
//! provider account probe, implicit source creation or credential copying.
mod access;
mod capacity;
mod gates;
mod job_monitor;
mod observer;
mod policy;
mod provisional;
mod readiness;
mod recovery;
mod request;
mod resources;
mod runs;
mod scheduler;
mod scheduler_apply;
mod session_control;
mod session_executor;
mod sessions;
mod summary;
mod tickets;
mod tool_capacity;
mod validation;
mod watch;
pub use access::{ActionRequest, Authorized};
pub use capacity::{
    CapacityProbe, LocalServerCapacity, ReadCapacity, TargetAdmission, UnknownCapacity,
};
pub use gates::{
    GateFollowState, GateFollowStep, GateProvider, GateView, GithubActionsGates, LinkGate,
    MAX_GATE_WAIT_MS, RequestDeployment, RunGates, SyncGates, TARGET_SCRIPT_PROVIDER,
};
pub use job_monitor::{DetachedJobMonitor, JOB_MONITOR_AUTOSTART_ENV, JobMonitor};
pub use policy::{DryRun, DryRunAction, DryRunTarget, PolicyProposal, PolicyValidation};
pub use provisional::{
    ProvisionalGate, ProvisionalView, ProvisionalWork, ReopenProvisional, RepairFirst,
};
pub use readiness::{
    DefinePlanStep, PlanStepRef, PlanView, PrerequisiteRequest, ReadyItem, RefreshReadiness,
    RefreshView, StepView,
};
pub use recovery::{CorrectRun, HandoffRun, PauseRun, ReconcileFailure, ReconcileSummary};
pub use request::{is_cleanup_request, is_machine_path, is_path, request_cleanup_now};
pub use resources::{ClaimSource, ClaimsUnreachable, MAX_CLAIM_WATCHES, MachineClaims};
pub use runs::{
    DEFAULT_LEASE_MINUTES, DispatchRun, HostedClaim, MAX_EVENT_WAIT_MS, MAX_LEASE_MINUTES,
    RunStatus, RunView,
};
pub use scheduler::{
    DecisionOutcome, DecisionsRequest, ExplainRequest, Explanation, MAX_SCHEDULER_WAIT_MS,
    PlanRequest, RequiredResource, ScheduleView, SchedulerFollowState, SchedulerStep, TickRequest,
    TickView,
};
pub use session_control::{
    AssignOutcome, ExecutorUnavailable, PlacementAssign, PlacementStart, RecordingSessions,
    SessionCall, SessionExecutor, SessionExecutorError, StartOutcome, WindDownOutcome,
    WindDownSession,
};
pub use session_executor::{OwnedSessions, UNCONFIRMED_START_MS};
pub use sessions::{
    HostLaunch, HostSessionState, HostedControlReply, HostedRoles, HostedSessionControl,
    HostedSessionList, HostedSessionStart, HostedSessionStop, HostedSessionView,
    HostedStartOutcome, HostedStartReply, HostedStopReply, Launched, LocalServerSessions,
    NoSessionHost, PushAttempt, RolePrompt, SessionHost, SessionHostError, SessionSyncFailure,
    SessionSyncSummary, SessionView, StartSession, StopSession, TOOL_SETUP_FAILED, TakeoverSession,
    ToolSetup, plugin_role_paths, pool_joined, role_prompt_text, setup_store, target_tool_setup,
};
pub use tickets::{
    CreateTargetTicket, ExternalProviders, HostedCreate, HostedTicket, HostedUpdate, LinkOutcome,
    LinkTargetTicket, LinkView, ListLinks, OrchestrationTicket, ReadTargetTicket, TargetTicket,
    TargetTicketFields, TargetTicketResult, UnlinkTargetTicket, UpdateTargetTicket,
};
pub use tool_capacity::{
    SelectTool, SignalResult, ToolCapacityRow, ToolCapacityView, ToolSelection,
};
pub use validation::{
    DefineValidation, HostedExecutionState, HostedValidation, HostedValidationStart,
    ListValidations, MAX_VALIDATION_OUTPUT_BYTES, MAX_VALIDATION_WAIT_MS, RunValidation,
    ValidationRunView, ValidationStepRef,
};
pub use watch::{
    MAX_TARGET_EVENT_WAIT_MS, TargetEventPage, TargetEvents, TargetTicketEvent, WatchState,
    WatchStep,
};
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

/// Cloning shares every port; a host clones to attach its own capacity probe.
#[derive(Clone)]
pub struct OrchestrationRuntime {
    home: PathBuf,
    checkouts: CheckoutRegistry,
    providers: ProviderConfigRegistry,
    remotes: RemoteRegistry,
    secrets: Arc<dyn SecretStore + Send + Sync>,
    transport: Arc<dyn RemoteCheckoutTransport + Send + Sync>,
    server_id: String,
    /// HS2-AZ15GA: opens a hosted checkout's external ticket sources.
    external: Arc<dyn ExternalProviders>,
    /// HS2-7QHWHJ: the host's live ticket changes, when this process has a change stream.
    target_events: Option<Arc<dyn TargetEvents>>,
    /// HS2-J8PRTV: the session host's own measurements for the hosted half.
    capacity: Arc<dyn CapacityProbe>,
    /// HS2-5JBYPV: the adapters gate evidence comes from.
    gate_providers: Vec<Arc<dyn GateProvider>>,
    /// HS2-GDJXGZ: the process that owns AI terminals, for the hosted half.
    sessions: Arc<dyn SessionHost>,
    /// HS2-ECWG50: who starts and winds down the sessions the scheduler places.
    session_executor: Arc<dyn SessionExecutor>,
    /// HS2-2FG91X: this machine's live resource claims, for the hosted half.
    claims: Arc<dyn ClaimSource>,
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
            external: Arc::new(tickets::LiveExternalProviders { home: home.clone() }),
            target_events: None,
            capacity: Arc::new(UnknownCapacity),
            gate_providers: vec![Arc::new(GithubActionsGates::new(&home))],
            sessions: Arc::new(NoSessionHost),
            session_executor: Arc::new(ExecutorUnavailable),
            claims: Arc::new(MachineClaims::new(&home)),
            home,
            checkouts,
            providers,
            secrets,
            transport,
            server_id,
        })
    }

    /// HS2-AZ15GA: replace how external ticket sources are opened (tests, embedders).
    #[must_use]
    pub fn with_external_providers(mut self, external: Arc<dyn ExternalProviders>) -> Self {
        self.external = external;
        self
    }

    /// HS2-7QHWHJ: let a remote or local orchestration project follow this host's
    /// ticket changes instead of polling for them.
    #[must_use]
    pub fn with_target_events(mut self, events: Arc<dyn TargetEvents>) -> Self {
        self.target_events = Some(events);
        self
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
        // HS2-J8PRTV: a headless process asks the server running on this machine.
        .map(|runtime| {
            runtime
                .with_capacity_probe(Arc::new(LocalServerCapacity::new(home)))
                // HS2-GDJXGZ: and uses that server's terminals.
                .with_session_host(Arc::new(LocalServerSessions::new(home)))
                // HS2-95SWQ4: the scheduler places the sessions this runtime owns.
                .with_session_executor(Arc::new(OwnedSessions))
        })
        // HS2-VVHVF0: a watch this runtime creates is observed without a CLI call.
        .map(|runtime| match DetachedJobMonitor::automatic() {
            Some(monitor) => runtime.with_job_monitor(Arc::new(monitor)),
            None => runtime,
        })
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
        OrchestrationError::Denied(_) | OrchestrationError::Forbidden(_) => 403,
        OrchestrationError::Run(hotsheet_ticketing::orchestration::RunError::NotFound) => 404,
        OrchestrationError::Run(_) => 409,
        OrchestrationError::Validation(
            hotsheet_ticketing::orchestration::ValidationError::StepNotFound
            | hotsheet_ticketing::orchestration::ValidationError::ResultNotFound,
        ) => 404,
        OrchestrationError::Validation(_) => 409,
        // HS2-42TARA: an unknown step is absent; work that is not ready is a state
        // conflict a person can resolve.
        OrchestrationError::Readiness(
            hotsheet_ticketing::orchestration::ReadinessError::StepNotFound,
        ) => 404,
        OrchestrationError::Readiness(_) => 409,
        OrchestrationError::Gate(error) => {
            use hotsheet_ticketing::orchestration::GateError as E;
            match error {
                E::NotFound => 404,
                E::ProviderUnsupported => 422,
                E::ProviderUnavailable => 502,
                E::NotPassed
                | E::InvalidTransition
                | E::RepositoryMismatch
                | E::DeploymentActive => 409,
            }
        }
        OrchestrationError::Session(error) => {
            use hotsheet_ticketing::orchestration::SessionError as E;
            match error {
                E::NotFound => 404,
                E::ForceNotPermitted => 403,
                E::HostUnavailable | E::StartFailed => 502,
                E::NotActive
                | E::InvalidTransition
                | E::HumanOwned
                | E::NotSafe
                | E::AdmissionRefused
                | E::RoleActive => 409,
            }
        }
        OrchestrationError::TargetOperation(error) => {
            use hotsheet_ticketing::orchestration::TargetOpError as E;
            match error {
                E::NotFound => 404,
                E::Conflict | E::Disabled => 409,
                E::Invalid => 400,
                E::Unsupported | E::LinksUnsupported => 422,
                E::RateLimited => 429,
                E::Unauthorized | E::PeerUnsupported | E::Failed => 502,
            }
        }
    }
}
