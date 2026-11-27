//! HS2-4J6V46: shared CLI/server/MCP composition. Domain transitions remain in core.
//! Host injects its actual registry/config/secret owners; no store/path discovery,
//! provider account probe, implicit source creation or credential copying.
mod access;
mod assignment;
mod auto_replan;
// HS2-AMJNER: the work a host runs between requests (resuming waiting applies).
mod automation;
// HS2-H1CSSN: the one bounded backoff of the follower loops.
mod backoff;
mod cancel_hold;
mod capacity;
mod capacity_watch;
mod decisions;
mod gates;
mod home_outages;
mod hosted_audit;
mod hosted_deployments;
mod hosted_gates;
mod job_monitor;
mod joining;
// HS2-KB2Y51: pushing this machine's lines to the servers that host its targets.
mod line_push;
mod observer;
mod permissions;
mod plan_update;
mod planning;
mod policy;
mod projects;
mod provisional;
mod readiness;
mod readiness_timer;
mod reconcile_sources;
mod recovery;
mod request;
mod resources;
mod review_returns;
mod runs;
mod scheduler;
mod scheduler_apply;
mod scheduler_dispositions;
mod session_control;
mod session_executor;
mod session_worktrees;
mod sessions;
mod site_pool;
mod step_job;
mod step_process;
mod step_program;
mod summary;
mod target_lines;
// HS2-A4RVYA: reusable plan templates, applied through the planner.
mod templates;
// HS2-H1CSSN: the owner of the runtime's background threads.
mod tasks;
// HS2-KXKA7X: scheduled and event triggers.
mod ticket_list;
mod tickets;
mod timeline;
mod tool_capacity;
mod triggers;
mod validation;
mod validation_output;
mod validation_runner;
mod watch;
pub use access::{ActionRequest, Authorized};
pub use assignment::{Assignment, CLAIMED_FOR_SESSION, REVIEW_RETURNED};
pub use auto_replan::{AUTO_REPLAN_ACTION, AutoReplan};
pub use automation::{AutomationMemory, AutomationReport};
pub use backoff::{BACKOFF_FAST_STEP, BACKOFF_FIRST, BACKOFF_MAX, Backoff, is_fast_step};
pub use cancel_hold::CANCELLED_BY_PERSON;
pub use capacity::{
    CAPACITY_RESAMPLE_MS, CapacityProbe, LocalServerCapacity, MAX_CAPACITY_WAIT_MS, ReadCapacity,
    TargetAdmission, UnknownCapacity,
};
pub use capacity_watch::{CapacityFollow, MAX_CAPACITY_WATCHES};
pub use gates::{
    GATE_RELINK_ATTEMPTS, GateFollowState, GateFollowStep, GateProvider, GateView,
    GithubActionsGates, LinkGate, MAX_GATE_WAIT_MS, MarkReview, RequestDeployment, RunGates,
    SyncGates, TARGET_SCRIPT_PROVIDER,
};
pub use home_outages::{HomeNotices, HomeNotification, notification as home_notification};
pub use hosted_audit::{HostedAuditView, HostedJournalFailures};
pub use hosted_deployments::{
    DEPLOYMENT_WORKTREES_DIRECTORY, HostedDeploymentStart, MAX_DEPLOYMENT_SECONDS,
};
pub use hosted_gates::{HostedGate, HostedGateWatch, MAX_HOSTED_GATE_WAIT_MS};
pub use job_monitor::{DetachedJobMonitor, JOB_MONITOR_AUTOSTART_ENV, JobMonitor};
pub use joining::REPLACEMENTS_EXHAUSTED;
pub use line_push::{
    LINE_OBSERVATION_LEAF, LINE_PUSH_FIRST_BACKOFF_MS, LINE_PUSH_MAX_BACKOFF_MS,
    LINE_PUSH_MIN_GAP_MS, LINE_PUSH_TTL_MS, LinePushOutcome, LinePushReport, LinePushState,
    MAX_LINE_PUSH_DESTINATIONS,
};
pub use permissions::{
    AnswerPermission, NoPrompt, PERMISSION_SIGNAL_DIRECTORY, PermissionList, PermissionPrompt,
    PermissionSignalWaiter, PromptRequest, RememberPermission, Remembered, RetiredRemoved,
    RuleView, SignalWaiter, permission_signal_directory, permission_signal_token,
    permission_summary, signal_token,
};
pub use plan_update::{
    PlanQuery, PlanUpdateRef, PlanUpdateView, STEP_REPLANNED_RUN, UpdateOutcome, UpdatePlan,
};
pub use planning::{
    DEFAULT_GOAL_TAG, DEFAULT_GOAL_TITLE, GoalTicket, MAX_LISTED_GOALS, PlanProposal, PlanningView,
    ProposeOutcome, ProposePlan,
};
pub use policy::{
    AutomaticSet, DryRun, DryRunAction, DryRunTarget, PolicyProposal, PolicyValidation,
    RoleSessionsSet, SetAutomatic, SetRoleSessions,
};
pub use projects::{
    AddHostedProject, AddProject, CHECKOUT_NOT_OPEN, HostedProjectChoice, ProjectView,
    RemoveProject, RemovedProject,
};
pub use provisional::{
    ProvisionalGate, ProvisionalView, ProvisionalWork, ReopenProvisional, RepairFirst,
};
pub use readiness::{
    DefinePlanStep, MigratePlan, PlanStepRef, PlanView, PrerequisiteRequest, ReadyItem,
    RefreshReadiness, RefreshView, StepView,
};
pub use recovery::{CorrectRun, HandoffRun, PauseRun, ReconcileFailure, ReconcileSummary};
pub use request::{is_cleanup_request, is_machine_path, is_path, request_cleanup_now};
pub use resources::{
    ClaimSource, ClaimsUnreachable, MAX_CLAIM_WATCHES, MAX_RESOURCE_WAIT_MS, MachineClaims,
    RemoteRelease, ResourceWait,
};
pub use review_returns::{ReturnedHandoff, assigner_live, returned_handoffs};
pub use runs::{
    DEFAULT_LEASE_MINUTES, DispatchRun, HostedClaim, MAX_EVENT_WAIT_MS, MAX_LEASE_MINUTES,
    RunStatus, RunView,
};
/// HS2-KB2Y51: a follower that wakes with the scheduler watches this directory.
pub use scheduler::WAKE_DIRECTORY as SCHEDULER_WAKE_DIRECTORY;
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
    HAND_BACK_OWNER_REMOTE, HOSTED_SESSION_SIGNAL_DIRECTORY, HandBackSession, HostCall, HostLaunch,
    HostSessionState, HostedControlReply, HostedRoles, HostedSessionControl, HostedSessionList,
    HostedSessionStart, HostedSessionStop, HostedSessionView, HostedStartOutcome, HostedStartReply,
    HostedStopReply, LAUNCH_ANSWER, LAUNCH_FENCE_MARGIN, Launched, LocalServerSessions,
    NoSessionHost, PushAttempt, RolePrompt, SessionHost, SessionHostError, SessionSyncFailure,
    SessionSyncSummary, SessionView, StartSession, StopSession, TOOL_SETUP_FAILED, TakeoverSession,
    ToolSetup, classify_transport, launch_body, launch_outcome, planner_prompt_text,
    plugin_reports_start, plugin_role_paths, pool_joined, role_prompt_text, setup_store,
    target_tool_setup, tool_ready,
};
pub use target_lines::MAX_TARGET_LINE_OWNERS;
pub use tasks::{RuntimeTasks, StopHook, TasksShutdown};
pub use templates::{
    AcceptTemplate, AccessNeed, ApplyRef, ApplySummary, ApplyTemplate, MAX_LISTED_APPLIES,
    MAX_RESUME_BACKOFF_MS, PreviewTarget, PreviewTemplate, ProfilesCheck, ResumeMemory,
    ResumeReport, ResumedApply, SaveGoalTemplate, SetTemplates, TemplateAcceptView,
    TemplateApplyView, TemplateList, TemplatePreview, TemplateRef, TemplateValidation,
    ValidateTemplate,
};
pub use ticket_list::{
    DEFAULT_TARGET_TICKET_PAGE, ListTargetTickets, MAX_TARGET_TICKET_PAGE, TargetTicketPage,
    TargetTicketRow, TargetTicketSearch,
};
pub use tickets::{
    CreateTargetTicket, ExternalProviders, HostedCreate, HostedTicket, HostedUpdate, LinkOutcome,
    LinkTargetTicket, LinkView, ListLinks, OrchestrationTicket, ReadTargetTicket, TargetTicket,
    TargetTicketFields, TargetTicketResult, UnlinkTargetTicket, UpdateTargetTicket,
};
pub use timeline::{MAX_TIMELINE_WAIT_MS, PERMISSION_REQUIRED};
pub use tool_capacity::{
    SelectTool, SignalResult, ToolCapacityRow, ToolCapacityView, ToolSelection,
};
pub use triggers::{
    BranchFollowState, BranchStep, DeliveryView, FireTrigger, MAX_BRANCH_WAIT_MS, ReviewNeed,
    ReviewNext, ReviewPerDelivery, ReviewPermission, ReviewProject, ReviewScope, ReviewTrigger,
    ReviewWatch, SetTrigger, TRIGGER_NEXT_RUNS, TriggerList, TriggerNotification,
    TriggerPassReport, TriggerPassRequest, TriggerRef, TriggerReviewView, TriggerView,
};
pub use validation::{
    CancelValidation, DefineValidation, HostedExecutionState, HostedValidation,
    HostedValidationCancel, HostedValidationStart, ListValidations, MAX_VALIDATION_OUTPUT_BYTES,
    MAX_VALIDATION_WAIT_MS, RunValidation, STEP_EXECUTION_VARIABLE, TargetProfiles,
    ValidationRunView, ValidationStepRef, validation_environment,
};
pub use validation_output::{
    MAX_RETAINED_COPIES, MAX_RETAINED_STREAM_BYTES, MAX_RETAINED_TOTAL_BYTES, RETAINED_OUTPUT_TTL,
    RetainedOutput, VALIDATION_OUTPUT_DIRECTORY, read_retained,
};
pub use validation_runner::{VALIDATION_STEPS_DIRECTORY, run_validation_step};
pub use watch::{
    HostedSpeculation, MAX_TARGET_EVENT_WAIT_MS, TargetEventPage, TargetEvents, TargetSessionEvent,
    TargetTicketEvent, WatchState, WatchStep,
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
    /// HS2-XHQ1EH: where waiting permission requests are shown to people.
    prompt: Arc<dyn PermissionPrompt>,
    /// HS2-P5GTSW: how often a capacity wait samples again (tests shorten it).
    capacity_resample: std::time::Duration,
    /// HS2-507DGZ: the first delay before a gate follower links an ended CI watch again,
    /// doubling per attempt (tests shorten it).
    gate_relink_retry_ms: u64,
    /// HS2-J7N3PQ: how a deadline timer waits until its instant (tests skip the wait).
    /// `None` waits on the stop token of [`Self::tasks`] (HS2-H1CSSN).
    timer_sleep: Option<Arc<dyn Fn(std::time::Duration) + Send + Sync>>,
    /// HS2-H1CSSN: the owner of the background threads this runtime starts, shared by
    /// its clones and, in the server, by every runtime the server builds.
    tasks: Arc<RuntimeTasks>,
    /// HS2-M7EV18: hosted validation steps run in detached runner processes.
    detached_validation: bool,
    /// HS2-CRBR8X: who sent the request this runtime serves (a device certificate, a
    /// paired browser), for the hosting machine's session journal. `None` is this
    /// machine itself.
    requester: Option<String>,
    /// HS2-EX594V: where home-outage notifications go; `None` sends none.
    home_notices: Option<Arc<dyn HomeNotices>>,
    /// HS2-EX594V: how long a home must be unavailable before it is announced.
    home_notify_after_ms: u64,
    /// HS2-1CC1TQ: the first wait before a home on record as out is read again.
    home_recheck_base_ms: u64,
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
            gate_providers: vec![
                Arc::new(GithubActionsGates::new(&home)),
                // HS2-TPAHYA: GitLab CI pipelines, through the same watch Jobs.
                Arc::new(GithubActionsGates::gitlab(&home)),
                // HS2-17HVNP: Buildkite builds and CircleCI pipelines, likewise.
                Arc::new(GithubActionsGates::buildkite(&home)),
                Arc::new(GithubActionsGates::circleci(&home)),
                // HS2-7H0S8R: Jenkins builds, likewise.
                Arc::new(GithubActionsGates::jenkins(&home)),
                // HS2-JZ1DD2: Azure Pipelines builds, likewise.
                Arc::new(GithubActionsGates::azure_pipelines(&home)),
                // HS2-BWF72S: Bitbucket Pipelines, likewise.
                Arc::new(GithubActionsGates::bitbucket_pipelines(&home)),
            ],
            sessions: Arc::new(NoSessionHost),
            session_executor: Arc::new(ExecutorUnavailable),
            claims: Arc::new(MachineClaims::new(&home)),
            prompt: Arc::new(NoPrompt),
            capacity_resample: std::time::Duration::from_millis(capacity::CAPACITY_RESAMPLE_MS),
            gate_relink_retry_ms: gates::OBSERVER_RETRY_MS,
            timer_sleep: None,
            tasks: Arc::default(),
            detached_validation: false,
            requester: None,
            home_notices: None,
            home_notify_after_ms: home_outages::DEFAULT_NOTIFY_AFTER_MS,
            home_recheck_base_ms: home_outages::DEFAULT_RECHECK_BASE_MS,
            home,
            checkouts,
            providers,
            secrets,
            transport,
            server_id,
        })
    }

    /// HS2-M7EV18: run hosted validation steps in detached runners, so a step finishes
    /// and is collected after this process restarts. Only where a runner's liveness can
    /// be checked by its start time (Linux, macOS); elsewhere steps keep running in the
    /// hosting process.
    #[must_use]
    pub fn with_detached_validation(mut self) -> Self {
        self.detached_validation = cfg!(any(target_os = "linux", target_os = "macos"));
        self
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

    /// HS2-SHH7WW: every project with its sites and this machine's binding of each.
    pub fn sites(
        &self,
        owner_id: &str,
    ) -> Result<hotsheet_ticketing::orchestration::SitesView, OrchestrationError> {
        self.flow(&self.owner(owner_id)?)?.sites()
    }

    /// HS2-SHH7WW: which targets of the owner's manifest are sites of one project.
    pub(crate) fn project_sites(
        &self,
        owner_id: &str,
    ) -> Result<hotsheet_ticketing::orchestration::ProjectSites, OrchestrationError> {
        let sites = ManifestStore::new(Path::new(&self.owner(owner_id)?.root)).sites()?;
        Ok(self.with_bound_sources(owner_id, sites))
    }

    /// HS2-3JGPDR: `sites` with each site's sources joined to its home's by the source
    /// identity this machine's bindings last confirmed, so one ticket of the shared store
    /// is one ticket whichever machine's connection id names it. A grouping of one-site
    /// projects, or unreadable bindings, is returned as it is.
    pub(crate) fn with_bound_sources(
        &self,
        owner_id: &str,
        sites: hotsheet_ticketing::orchestration::ProjectSites,
    ) -> hotsheet_ticketing::orchestration::ProjectSites {
        if sites.is_empty() {
            return sites;
        }
        let Ok(bindings) = self.bindings(owner_id) else {
            return sites;
        };
        sites.with_sources(bindings.iter().map(|binding| {
            (
                binding.target.target_id.as_str(),
                binding
                    .last_confirmed_sources
                    .iter()
                    .map(|source| (source.connection_id.as_str(), source.identity.as_str()))
                    .collect(),
            )
        }))
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
                // HS2-M7EV18: a hosted validation step outlives this process.
                .with_detached_validation()
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
        OrchestrationError::Invalid(_) | OrchestrationError::UnknownField { .. } => 400,
        OrchestrationError::Conflict | OrchestrationError::Capacity => 409,
        OrchestrationError::NotFound => 404,
        OrchestrationError::Removed => 410,
        OrchestrationError::UnsupportedVersion { .. } => 426,
        OrchestrationError::Observation(_) => 502,
        OrchestrationError::Corrupt => 500,
        OrchestrationError::Storage(_) => 503,
        OrchestrationError::Denied(_)
        | OrchestrationError::Forbidden(_)
        | OrchestrationError::PermissionRequired { .. }
        | OrchestrationError::DecisionRequired { .. } => 403,
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
            hotsheet_ticketing::orchestration::ReadinessError::StepNotFound
            | hotsheet_ticketing::orchestration::ReadinessError::UpdateNotFound,
        ) => 404,
        OrchestrationError::Readiness(_) => 409,
        OrchestrationError::Gate(error) => {
            use hotsheet_ticketing::orchestration::GateError as E;
            match error {
                E::NotFound => 404,
                E::ProviderUnsupported => 422,
                E::ProviderUnavailable => 502,
                // HS2-YA5S82: not known yet; the same request succeeds or is refused later.
                E::RepositoryUnknown => 503,
                E::NotPassed
                | E::InvalidTransition
                | E::RepositoryMismatch
                | E::DeploymentActive
                | E::ArtifactMismatch
                | E::DeploymentScriptUntracked => 409,
            }
        }
        // HS2-8Z9VHH: a refused admission, with its reason and limit.
        OrchestrationError::AdmissionRefused(_) => 409,
        // HS2-A4RVYA: an unknown template or apply is absent; a definition or inputs
        // that do not fit are invalid; anything else conflicts with what is recorded.
        OrchestrationError::Template(error) => {
            use hotsheet_ticketing::orchestration::TemplateError as E;
            match error {
                E::NotFound | E::ApplyNotFound => 404,
                E::Invalid | E::InputsInvalid => 422,
                E::Retired
                | E::Conflict
                | E::TargetUnknown
                | E::ApplyConflict
                | E::GoalEmpty
                | E::NothingToAccept => 409,
            }
        }
        // HS2-KXKA7X: an unknown trigger is absent; a definition that does not fit is
        // invalid; a trigger that does not fire, or lost its template, conflicts.
        OrchestrationError::Trigger(error) => {
            use hotsheet_ticketing::orchestration::TriggerError as E;
            match error {
                E::NotFound => 404,
                E::Invalid => 422,
                E::NotFiring | E::TemplateMissing => 409,
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
                | E::RoleActive
                | E::RunHeld
                | E::Withdrawn
                | E::MidTurn => 409,
            }
        }
        // HS2-SHH7WW: a project the site names is absent; anything else conflicts with
        // the project's other sites or records.
        OrchestrationError::Site(hotsheet_ticketing::orchestration::SiteError::ProjectUnknown) => {
            404
        }
        OrchestrationError::Site(_) => 409,
        // HS2-061YTB: the target's machine is not what the plan step requires.
        OrchestrationError::Requirement { .. } => 409,
        OrchestrationError::TargetOperation(error) => {
            use hotsheet_ticketing::orchestration::TargetOpError as E;
            match error {
                E::NotFound => 404,
                E::Conflict | E::Disabled => 409,
                E::Invalid => 400,
                E::Unsupported | E::LinksUnsupported => 422,
                E::RateLimited => 429,
                E::Unauthorized
                | E::PeerUnsupported
                | E::Failed
                | E::PeerSessionExpired
                | E::PeerSessionRevoked
                | E::PeerReadOnly
                | E::PeerCsrfRequired
                | E::PeerAuthRefused => 502,
            }
        }
    }
}
