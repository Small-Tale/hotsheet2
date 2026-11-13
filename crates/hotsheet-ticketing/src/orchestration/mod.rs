//! HS2-4J6V46: shared target declarations and reviewed machine-local bindings.
//! Network, provider metadata, clock and randomness remain injected boundaries.

mod approvals;
mod audit;
mod bindings;
mod capacity;
pub(crate) mod contract;
mod gates;
mod grants;
mod manifest;
mod policy;
mod provisional;
mod readiness;
mod reconcile;
mod reconcile_sources;
mod resources;
mod role_delivery;
mod run_events;
mod runs;
mod scheduler;
mod session_idle;
mod sessions;
mod storage;
mod summary;
mod tool_capacity;
mod validation;
mod workflow;

pub use approvals::{
    APPROVAL_PENDING_MS, APPROVAL_VALID_MS, Approval, ApprovalRegistry, ApprovalState,
    MAX_APPROVALS,
};
pub use audit::{
    AuditEntry, AuditJournal, AuditOutcome, AuditPage, AuditRecord, Journal, JournalEntry,
    JournalPage, JournalRecord, MAX_AUDIT_PAGE, MAX_AUDIT_SEGMENT_BYTES,
};
pub use bindings::BindingRegistry;
pub use capacity::{
    AdmissionConfig, AdmissionDecision, AdmissionOutcome, AdmissionReason, AdmissionRequest,
    AdmissionStore, AdmissionView, CapacityField, CapacitySnapshot, CpuCapacity,
    DEFAULT_SESSION_FOOTPRINT_MB, DiskCapacity, FootprintSource, HeldClaim, HostObservation,
    MAX_CLAIM_NAMESPACES, MAX_SESSION_FOOTPRINT_MB, MemoryCapacity, SessionFootprint,
    TargetSpeculation, ToolAvailability, decide_admission, effective_limits, estimated_footprint,
};
pub use contract::*;
pub use gates::{
    ArtifactRef, CiScope, Coverage, DeploymentScope, Gate, GateError, GateEvidence, GateFilter,
    GateKind, GateOutcome, GateReading, GateRegistry, GateState, GateSummary, GateTransition,
    Linked, MAX_GATE_HISTORY, MAX_GATES, NewGate, summarize, valid_commit, valid_script,
};
pub use grants::{
    AccessDecision, AccessRequest, CreateGrant, DenialReason, Grant, GrantRegistry, GrantState,
    GrantView, MAX_GRANT_CONCURRENCY, MAX_GRANT_LIFETIME_MS, MAX_GRANT_MODELS, MAX_GRANT_TOOLS,
    MAX_GRANTS, MAX_GRANTS_PER_TARGET, SessionRequest, SessionScope, TargetAction, ToolScope,
    destination_key,
};
pub use manifest::ManifestStore;
pub use policy::{
    AccountPolicy, ApprovalGate, ApprovalPolicy, MAX_POLICY_ACCOUNTS, MAX_POLICY_SESSIONS,
    MAX_POLICY_TOOLS, MAX_SPECULATION_DEPTH, MachineLimits, PlacementPolicy, PolicyBody,
    PolicyDenial, PolicyProblem, PolicyQuestion, PolicyStore, PolicyVerdict, PolicyView,
    SessionLoad, SetPolicy, Speculation, TargetPolicy, ToolPolicy, WindDown,
};
pub use provisional::{
    ProvisionalBasis, ProvisionalStanding, ProvisionalStore, SpeculationInputs, gate_observation,
    provisional_standings,
};
pub use readiness::{
    GateFacts, GateRequirement, LevelChange, MAX_PLAN_STEPS, MAX_STEP_PLAN_PREREQUISITES,
    NewPlanStep, PlanFacts, PlanRecord, PlanRegistry, PlanStep, Prerequisite, PublishedLevel,
    ReadinessError, ReadinessLevel, ReadinessReason, StepReadiness, TicketFacts, TicketKey,
    TicketRecord, TicketSeen, TicketUpdate, evaluate_plan, ticket_key,
};
pub use reconcile::{
    Alignment, Assessment, Disposition, ExternalSource, ExternalWork, MAX_FINDINGS, Observations,
    Observed, RunReconciliation, SOURCE_NOT_WIRED, SourceFinding, TargetFacts, Verification, align,
    assess,
};
pub use reconcile_sources::{
    AttemptAnswer, AttemptRecord, DEADLINE_PASSED, EXECUTION_LOST, EXECUTION_NOT_FOUND,
    EXECUTION_UNCONFIRMED, GateObservation, NOTE_AWAITING_APPROVAL, NOTE_DEPLOYMENT_NOT_EXECUTED,
    NOTE_NO_DEPLOYMENT, NOTE_NO_GATE, NOTE_NO_SESSION, NOTE_NO_VALIDATION, NOTE_SESSION_LOST,
    NOTE_SESSION_STOPPED, NOTE_START_UNCONFIRMED, NOTE_WAITING_ON_GATE, NOTE_WAITING_ON_VALIDATION,
    NOTE_WATCH_ADOPTED, SessionNeed, SourceWork, ci_work, combine_work, deployment_work,
    session_work, validation_attempt, validation_work,
};
pub use resources::{
    MAX_STEP_RESOURCES, MAX_TARGET_RESOURCES, OfflinePolicy, ResourceContention, ResourceStanding,
    ResourceState, TargetResources, UnreadableClaims, UnreadableReason, resource_standings,
    validate_needs,
};
pub use role_delivery::{
    MAX_ROLE_ATTEMPTS, PushAttempt, ROLE_PROMPT_WINDOW_MS, RoleDelivery, RolePrompt,
};
pub use run_events::{
    RunEvent, RunEventEntry, RunEventJournal, RunEventKind, RunEventPage, RunEventSource,
};
pub use runs::{
    DispatchMode, MAX_RUN_HISTORY, MAX_RUNS, NewRun, Run, RunControl, RunError, RunObservation,
    RunPause, RunRegistry, RunState, RunTarget, RunTicket, RunTransition, RunUnreachable,
    RunUpdate,
};
pub use scheduler::{
    Blocker, DecisionEntry, DecisionJournal, DecisionKind, DecisionMode, DecisionPage,
    DecisionRecord, HeldResource, MAX_SCHEDULE_SESSIONS, MAX_SCHEDULE_WORK, Placement,
    PlacementAction, PlacementInputs, PlacementMemory, PlacementOutcome, PlacementReason,
    REFUSAL_BACKOFF_BASE_MS, REFUSAL_BACKOFF_CAP_MS, Ranked, ResourceNeed, Schedule, ScheduleInput,
    ScheduledSession, SchedulerStore, ScoreParts, SessionActivity, Standing, TargetState,
    TickInputs, Unplaced, WorkItem, WorkLevel, WorkSpeculation, plan, refusal_backoff_ms, remember,
    score,
};
pub use session_idle::{SessionIdleStore, observe_idle};
pub use sessions::{
    HostedRef, HostedSession, HostedSessionRegistry, MAX_HOSTED_SESSIONS, MAX_SESSION_HISTORY,
    MAX_SESSION_PROJECT_CHARS, MAX_SESSIONS, NewHostedSession, NewSession, OrchestratedSession,
    SafePoint, SafePointBlocker, SafePointFacts, SessionControl, SessionError, SessionOrigin,
    SessionOwner, SessionRegistry, SessionRole, SessionState, SessionTransition, SessionUpdate,
    StopRequest, decide_safe_point, decide_stop, run_holder, session_label,
};
pub use summary::{
    BlockedTime, CostAggregate, CostReading, DurationAggregate, Metric, OutcomeTally,
    PROGRESS_ATTRIBUTION, ProgressInput, ProgressRollup, ProgressSummary, RetryTotals, RunProgress,
    RunRetries, TargetProgress, UnknownReason, WorkItemKey, WorkItemProgress, summarize_progress,
};
pub use tool_capacity::{
    ActiveLimit, Applied, BACKOFF_BASE_MS, BACKOFF_CAP_MS, BACKOFF_FACTOR, Candidate,
    CapacityEntry, CapacityReason, CapacityState, CapacityStatus, DaySpend, Excluded,
    ExclusionReason, LimitSignal, MAX_CAPACITY_ENTRIES, MAX_RECENT_SIGNALS, MAX_RESET_AHEAD_MS,
    MAX_SIGNAL_COST_CENTS, MAX_SIGNAL_SKEW_MS, Selection, SelectionRequest, SessionCount,
    SignalKind, SignalOutcome, SignalSupport, ToolCapacityRegistry, apply_signal, backoff_ms,
    known_spend_cents, select, state_at, status_for,
};
pub use validation::{
    DEFAULT_STEP_TIMEOUT_SECONDS, ExecutionEnd, ExecutionOutcome, ExpectedEvidence,
    MAX_COMMAND_ARGS, MAX_COMMAND_BYTES, MAX_STEP_ATTEMPTS, MAX_STEP_PREREQUISITES,
    MAX_STEP_RESULTS, MAX_STEP_TIMEOUT_SECONDS, MAX_STEPS_PER_TICKET, MAX_VALIDATION_PROFILES,
    MAX_VALIDATION_STEPS, NewResult, ResultUpdate, RetryPolicy, StepDefinition, StreamRef,
    VALIDATION_DEADLINE_GRACE_MS, ValidationCheck, ValidationError, ValidationEvidence,
    ValidationProfile, ValidationProfileStore, ValidationRegistry, ValidationResult,
    ValidationState, ValidationStep, effective, evaluate, may_retry,
};
pub use workflow::{ObservedTarget, Orchestration, TargetObserver};

#[derive(Debug, thiserror::Error)]
pub enum OrchestrationError {
    #[error("invalid orchestration request: {0}")]
    Invalid(&'static str),
    #[error("orchestration state changed; read it and review again")]
    Conflict,
    #[error("orchestration target was not found")]
    NotFound,
    #[error("orchestration target was removed")]
    Removed,
    #[error("orchestration format version {found} is unsupported (supported: 1)")]
    UnsupportedVersion { found: u64 },
    #[error("orchestration data is malformed")]
    Corrupt,
    #[error("orchestration capacity reached")]
    Capacity,
    #[error("orchestration observation failed: {0}")]
    Observation(TargetFailure),
    #[error("orchestration storage is unavailable")]
    Storage(#[source] std::io::Error),
    /// HS2-SBQ3T8: no grant (or policy) authorizes this actor's action on the target.
    #[error("orchestration action is not authorized: {}", .0.code())]
    Denied(DenialReason),
    /// HS2-SBQ3T8: the caller may never perform this operation, whatever is granted.
    #[error("orchestration operation is forbidden: {0}")]
    Forbidden(&'static str),
    /// HS2-AZ15GA: the target's own ticket source refused or could not perform an
    /// authorized operation. The target stays authoritative; nothing is retried elsewhere.
    #[error("target ticket operation failed: {}", .0.code())]
    TargetOperation(TargetOpError),
    /// HS2-EBHDSB: the dispatch run does not allow this request in its current state.
    #[error("orchestration run refused the request: {}", .0.code())]
    Run(RunError),
    /// HS2-WN0MXZ: the validation step or result does not allow this request.
    #[error("orchestration validation refused the request: {}", .0.code())]
    Validation(ValidationError),
    /// HS2-5JBYPV: the gate, or its provider adapter, refused the request.
    #[error("orchestration gate refused the request: {}", .0.code())]
    Gate(GateError),
    /// HS2-42TARA: the plan step is unknown, or its work is not ready to dispatch.
    #[error("orchestration readiness refused the request: {}", .0.code())]
    Readiness(ReadinessError),
    /// HS2-GDJXGZ: the orchestrator-owned session does not allow this request.
    #[error("orchestration session refused the request: {}", .0.code())]
    Session(SessionError),
}

impl From<std::io::Error> for OrchestrationError {
    fn from(error: std::io::Error) -> Self {
        Self::Storage(error)
    }
}

impl OrchestrationError {
    /// Content-free adapter code; callers must not format Debug/source chains into DTOs.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Invalid(_) => "invalid_request",
            Self::Conflict => "conflict",
            Self::NotFound => "target_not_found",
            Self::Removed => "target_removed",
            Self::UnsupportedVersion { .. } => "upgrade_required",
            Self::Corrupt => "corrupt_orchestration_data",
            Self::Capacity => "capacity_reached",
            Self::Observation(failure) => failure.code(),
            Self::Storage(_) => "storage_unavailable",
            Self::Denied(reason) => reason.code(),
            Self::Forbidden(_) => "forbidden",
            Self::TargetOperation(error) => error.code(),
            Self::Run(error) => error.code(),
            Self::Validation(error) => error.code(),
            Self::Gate(error) => error.code(),
            Self::Readiness(error) => error.code(),
            Self::Session(error) => error.code(),
        }
    }
}

/// HS2-AZ15GA: why a target's ticket source did not perform an authorized operation.
/// Content-free: provider messages can hold ticket text and never cross this boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetOpError {
    /// The ticket, or the source connection, does not exist in the target.
    NotFound,
    /// The target's provider does not support the operation; no other source is tried.
    Unsupported,
    /// The provider reported a conflict, including an operation id reused for a
    /// different source ticket and a stale concurrency token.
    Conflict,
    /// The target's provider credentials were refused or are missing.
    Unauthorized,
    RateLimited,
    /// The target's source connection is disabled.
    Disabled,
    /// The request itself is malformed for this provider.
    Invalid,
    /// The peer does not offer orchestrated target operations.
    PeerUnsupported,
    /// The orchestration ticket's own source cannot store structured links.
    LinksUnsupported,
    Failed,
}

impl TargetOpError {
    pub fn code(self) -> &'static str {
        match self {
            Self::NotFound => "target_ticket_not_found",
            Self::Unsupported => "target_operation_unsupported",
            Self::Conflict => "target_conflict",
            Self::Unauthorized => "target_provider_unauthorized",
            Self::RateLimited => "target_rate_limited",
            Self::Disabled => "target_source_disabled",
            Self::Invalid => "target_request_invalid",
            Self::PeerUnsupported => "target_peer_unsupported",
            Self::LinksUnsupported => "orchestration_links_unsupported",
            Self::Failed => "target_operation_failed",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        [
            Self::NotFound,
            Self::Unsupported,
            Self::Conflict,
            Self::Unauthorized,
            Self::RateLimited,
            Self::Disabled,
            Self::Invalid,
            Self::PeerUnsupported,
            Self::LinksUnsupported,
            Self::Failed,
        ]
        .into_iter()
        .find(|error| error.code() == code)
    }
}

/// Runtime supplies the existing clock/RNG ports; no wall clock or global RNG in core.
pub struct Transaction<'a> {
    pub clock: &'a dyn crate::Clock,
    pub rng: &'a mut dyn crate::Rng,
}

impl Transaction<'_> {
    pub(crate) fn token(&mut self) -> String {
        crate::mint_ulid(self.clock, self.rng).to_string()
    }
}

#[cfg(test)]
mod approval_tests;
#[cfg(test)]
mod audit_tests;
#[cfg(test)]
mod binding_tests;
#[cfg(test)]
mod capacity_tests;
#[cfg(test)]
mod gate_tests;
#[cfg(test)]
mod grant_tests;
#[cfg(test)]
mod policy_tests;
#[cfg(test)]
mod provisional_tests;
#[cfg(test)]
mod readiness_tests;
#[cfg(test)]
mod reconcile_source_tests;
#[cfg(test)]
mod reconcile_tests;
#[cfg(test)]
mod resource_tests;
#[cfg(test)]
mod role_delivery_tests;
#[cfg(test)]
mod run_event_tests;
#[cfg(test)]
mod run_tests;
#[cfg(test)]
mod scheduler_tests;
#[cfg(test)]
mod session_idle_tests;
#[cfg(test)]
mod session_tests;
#[cfg(test)]
mod summary_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tool_capacity_tests;
#[cfg(test)]
mod validation_tests;
