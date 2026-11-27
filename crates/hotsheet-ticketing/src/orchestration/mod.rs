//! HS2-4J6V46: shared target declarations and reviewed machine-local bindings.
//! Network, provider metadata, clock and randomness remain injected boundaries.

mod actions;
mod assignment;
mod audit;
mod bindings;
mod capabilities;
mod capacity;
pub(crate) mod contract;
mod gates;
mod home_outage;
mod hosted_audit;
mod hosted_validations;
mod joining;
mod manifest;
mod permissions;
mod plan_update;
mod planning;
mod policy;
mod project_type;
mod provisional;
mod readiness;
// HS2-BQN0M5, HS2-G6F5BP: the automatic re-plan's machine-local memory.
mod reconcile;
mod reconcile_sources;
mod replan_memory;
mod resources;
mod role_delivery;
mod run_events;
mod runs;
mod scheduler;
mod session_fence;
mod session_idle;
mod sessions;
mod storage;
mod summary;
mod summary_use;
mod summary_wait;
mod target_lines;
// HS2-A4RVYA: reusable plan templates, applied through the planner.
mod templates;
// HS2-KXKA7X: scheduled and event triggers that apply templates or set placement.
mod triggers;
// HS2-YDY5HJ: the checked-in step definitions of a plan.
mod shared_plan;
// HS2-KB2Y51: lines another machine pushed for a checkout hosted here.
mod line_observations;
mod timeline;
mod tool_capacity;
mod validation;
mod workflow;

pub use actions::{
    AccessDecision, AccessRequest, DenialReason, SessionRequest, TargetAction, validate_model_name,
};
pub use assignment::{
    AssignmentDecision, AssignmentInput, AssignmentState, DEFAULT_DISPATCHER_WAIT_MINUTES,
    decide_assignment,
};
pub use audit::{
    AUDIT_COALESCE_MS, AuditEntry, AuditJournal, AuditOutcome, AuditPage, AuditRecord, Journal,
    JournalEntry, JournalPage, JournalRecord, MAX_AUDIT_PAGE, MAX_AUDIT_SEGMENT_BYTES, RepeatCheck,
    coalescible, continued_wait, repeats,
};
pub use bindings::BindingRegistry;
pub use capabilities::{
    DEFAULT_TOOLCHAINS, GPU_VENDORS, GpuCapability, HostCapabilities, MAX_MODEL_CHARS,
    MAX_REQUIREMENT_VALUES, MAX_TOOLCHAINS, MAX_VERSION_CHARS, OsFamily, RequirementMiss,
    RequirementState, StepRequirements, ToolVersion, ToolchainCapability, normalize_arch,
    requirement_misses, valid_toolchain, validate_toolchains,
};
pub use capacity::{
    AdmissionConfig, AdmissionDecision, AdmissionOutcome, AdmissionReason, AdmissionRefusal,
    AdmissionRequest, AdmissionStore, AdmissionView, CAPACITY_DISK_STEP_MB,
    CAPACITY_LOAD_STEP_PERCENT, CAPACITY_MEMORY_STEP_MB, CapacityField, CapacitySnapshot,
    CpuCapacity, DEFAULT_SESSION_FOOTPRINT_MB, DiskCapacity, FootprintSource, HeldClaim,
    HostObservation, MAX_CLAIM_NAMESPACES, MAX_CLAIM_PAGES, MAX_SESSION_FOOTPRINT_MB,
    MemoryCapacity, PoolState, SessionFootprint, TargetSpeculation, ToolAvailability,
    UnreadableNamespace, capacity_version, decide_admission, effective_limits, estimated_footprint,
};
pub use contract::*;
pub use gates::{
    ArtifactRef, CiScope, Coverage, DeploymentScope, Gate, GateError, GateEvidence, GateFilter,
    GateKind, GateOutcome, GateReading, GateRegistry, GateState, GateSummary, GateTransition,
    Linked, MAX_GATE_HISTORY, MAX_GATES, NewGate, REVIEW_APPROVED, REVIEW_CHANGES_REQUESTED,
    REVIEW_PROVIDER, REVIEW_REQUESTED, ReviewMark, WATCH_ENDED, WATCH_GONE, WATCH_LOST, summarize,
    valid_commit, valid_script,
};
pub use home_outage::{
    HOME_OUTAGE_DIRECTORY, HOME_OUTAGE_NOTIFY_AFTER_MS, HOME_RECHECK_BASE_MS, HOME_RECHECK_CAP_MS,
    HomeNotice, HomeNoticeKind, HomeObservation, HomeOutage, HomeOutageReason, HomeOutageStore,
    advance_outages, due_notices, next_notice_at, next_recheck_at, observe_homes,
};
pub use hosted_audit::{
    HostedSessionAuditEntry, HostedSessionAuditJournal, HostedSessionAuditPage,
    HostedSessionAuditRecord, HostedSessionEvent,
};
pub use hosted_validations::{
    HostedValidationKnown, HostedValidationRecord, HostedValidationRegistry,
    MAX_HOSTED_VALIDATION_RECORDS, StepProcess,
};
pub use joining::{
    DEFAULT_JOIN_MINUTES, JoinDecision, JoinInput, JoinState, MAX_JOIN_REPLACEMENTS, decide_join,
};
pub use line_observations::{
    DEFAULT_OBSERVATION_TTL_MS, LINE_OBSERVATION_DIRECTORY, LineObservations,
    MAX_OBSERVATION_BYTES, MAX_OBSERVATION_OWNERS, MAX_OBSERVATION_TTL_MS, MAX_OBSERVED_LINES,
    MAX_PUSHES_PER_MINUTE, MIN_OBSERVATION_TTL_MS, ObservationEntry, ObservationFile,
    ObservationOutcome, ObservedLines, ObservedOwner, display_line,
};
pub use manifest::ManifestStore;
pub use permissions::{
    Answered, Asked, DecisionAnswered, DecisionAsked, MAX_DECISIONS, MAX_PERMISSION_REQUESTS,
    MAX_PERMISSION_RULES, PERMISSION_ALLOWED_MS, PERMISSION_PENDING_MS, PermissionAction,
    PermissionAnswer, PermissionAsk, PermissionKey, PermissionLedger, PermissionRequest,
    PermissionRule, PermissionState, RETIRED_APPROVALS_FILE, RETIRED_GRANTS_FILE, RetiredAuthority,
    remove_retired_authority, retired_authority,
};
pub use plan_update::{
    MergeRefusal, PLAN_UPDATE_DIR, PlanSwap, PlanUpdate, PlanUpdateState, PlanUpdateStore,
    UpdateDecision, decide_update, merge_update, plan_update_reason,
};
pub use planning::{
    MAX_PLANNER_PROPOSALS, PlannerFacts, PlanningState, ProposeDecision, current_planner,
    decide_propose, planning_state,
};
pub use policy::{
    AUTO_TOOL_WEIGHT, AccountPolicy, IgnoredField, MAX_POLICY_ACCOUNTS, MAX_POLICY_MODELS,
    MAX_POLICY_SESSIONS, MAX_POLICY_TOOLS, MAX_SPECULATION_DEPTH, MachineLimits, ModelUnresolved,
    ORCHESTRATOR_ACTOR_PREFIX, PlacementPolicy, PlacementStartedStore, PolicyBody, PolicyDenial,
    PolicyProblem, PolicyQuestion, PolicyStore, PolicyVerdict, PolicyView, PolicyWarning,
    ROLE_SESSIONS_NOT_HOME, RoleSessionKind, SessionLoad, SetPolicy, Speculation, TargetPolicy,
    TargetRoleSessions, ToolPolicy, ToolScope, WindDown,
};
pub use project_type::{
    PROJECT_TYPE_SETTING, PROJECTS_RETAINED, ProjectType, ProjectTypeView, SetProjectType,
    TypeChange, TypeState, plan_type_change,
};
pub use provisional::{
    ProvisionalBasis, ProvisionalStanding, ProvisionalStore, Retiring, SpeculationInputs,
    gate_observation, provisional_standings,
};
pub use readiness::{
    GateFacts, GateRequirement, LevelChange, MAX_PLAN_STEPS, MAX_STEP_PLAN_PREREQUISITES,
    NewPlanStep, PlanFacts, PlanHealth, PlanMigration, PlanRecord, PlanRegistry, PlanStep,
    Prerequisite, PublishedLevel, ReadinessError, ReadinessLevel, ReadinessReason, StepReadiness,
    TicketFacts, TicketKey, TicketRecord, TicketSeen, TicketUpdate, evaluate_plan, ticket_key,
};
pub use reconcile::{
    Alignment, Assessment, Disposition, DispositionAction, ExternalSource, ExternalWork,
    MAX_FINDINGS, Observations, Observed, RunReconciliation, SOURCE_NOT_WIRED, SourceFinding,
    TargetFacts, Verification, align, assess, disposition_action,
};
pub use reconcile_sources::{
    AttemptAnswer, AttemptRecord, DEADLINE_PASSED, EXECUTION_LOST, EXECUTION_NOT_FOUND,
    EXECUTION_UNCONFIRMED, GateObservation, NOTE_AWAITING_APPROVAL, NOTE_DEPLOYMENT_NOT_EXECUTED,
    NOTE_NO_DEPLOYMENT, NOTE_NO_GATE, NOTE_NO_SESSION, NOTE_NO_VALIDATION, NOTE_SESSION_LOST,
    NOTE_SESSION_STOPPED, NOTE_START_UNCONFIRMED, NOTE_WAITING_ON_GATE, NOTE_WAITING_ON_VALIDATION,
    NOTE_WATCH_ADOPTED, SessionNeed, SourceWork, ci_work, combine_work, deployment_work,
    session_work, validation_attempt, validation_work,
};
pub use replan_memory::{
    CatchUp as ReplanCatchUp, MAX_REPLAN_HANDED, MAX_REPLAN_SEEN, REPLAN_MEMORY_DIR, ReplanKey,
    ReplanMemory, ReplanMemoryStore,
};
pub use resources::{
    MAX_STEP_RESOURCES, MAX_TARGET_RESOURCES, OfflinePolicy, ResourceContention, ResourceStanding,
    ResourceState, TargetResources, UnreadableClaims, UnreadableReason, resource_standings,
    validate_needs, wildcard_match,
};
pub use role_delivery::{
    MAX_ROLE_ATTEMPTS, PushAttempt, ROLE_PROMPT_WINDOW_MS, RoleDelivery, RolePrompt,
};
pub use run_events::{
    RunEvent, RunEventEntry, RunEventJournal, RunEventKind, RunEventPage, RunEventSource,
};
pub use runs::{
    DispatchMode, MAX_RUN_HISTORY, MAX_RUNS, NewRun, RUN_OPERATION_STRIPES, Run, RunControl,
    RunError, RunObservation, RunOperationGuard, RunPause, RunRegistry, RunState, RunTarget,
    RunTicket, RunTransition, RunUnreachable, RunUpdate,
};
pub use scheduler::{
    Blocker, DecisionEntry, DecisionJournal, DecisionKind, DecisionMode, DecisionPage,
    DecisionRecord, HeldResource, INTEGRATOR_CI_PENDING, INTEGRATOR_HANDOFF_PENDING,
    INTEGRATOR_IDLE, INTEGRATOR_POLICY_OPT_OUT, INTEGRATOR_QUEUE, INTEGRATOR_TARGET_DISABLED,
    INTEGRATOR_WORKER_RUN, MAX_SCHEDULE_SESSIONS, MAX_SCHEDULE_WORK, Placement, PlacementAction,
    PlacementInputs, PlacementMemory, PlacementOutcome, PlacementReason, REFUSAL_BACKOFF_BASE_MS,
    REFUSAL_BACKOFF_CAP_MS, Ranked, ResourceNeed, RoleBlocker, RoleUnplaced, SITE_NOT_CHOSEN,
    Schedule, ScheduleInput, ScheduledSession, SchedulerStore, ScoreParts, SessionActivity,
    SitePass, Standing, TargetState, TickInputs, Unplaced, WorkItem, WorkLevel, WorkSpeculation,
    is_capacity_refusal, lift_capacity_refusal, plan, refusal_backoff_ms, remember,
    scheduler_pass_lock, score,
};
pub use session_fence::{
    MAX_WITHDRAWN_FENCES, WITHDRAWN_FENCE_MS, Withdrawal, WithdrawnFences, WithdrawnOperation,
};
pub use session_idle::{SessionIdleStore, observe_idle};
pub use sessions::{
    HostedRef, HostedRequest, HostedSession, HostedSessionRegistry, MAX_HOSTED_SESSIONS,
    MAX_SESSION_HISTORY, MAX_SESSION_PROJECT_CHARS, MAX_SESSIONS, NewHostedSession, NewSession,
    OrchestratedSession, RecordedRequest, RequestKind, SafePoint, SafePointBlocker, SafePointFacts,
    SessionControl, SessionError, SessionOrigin, SessionOwner, SessionRegistry, SessionRole,
    SessionState, SessionTransition, SessionUpdate, StopRequest, decide_hand_back,
    decide_safe_point, decide_stop, run_holder, run_worked_elsewhere, session_label,
};
pub use shared_plan::{SHARED_PLAN_PATH, SharedPlan, SharedPlanStore, SharedStep, merge_steps};
pub use summary::{
    BlockedTime, CostAggregate, CostReading, DurationAggregate, Metric, OutcomeTally,
    PROGRESS_ATTRIBUTION, ProgressInput, ProgressRollup, ProgressSummary, RetryTotals, RunProgress,
    RunRetries, TargetProgress, UnknownReason, WorkItemKey, WorkItemProgress, summarize_progress,
};
pub use summary_use::{
    CapacityUse, LimitedTime, Utilization, UtilizationInput, summarize_utilization,
};
pub use summary_wait::{
    ExternalWait, ExternalWaitInput, ProvisionalTally, ProvisionalWork, SystemWait,
    summarize_external_wait,
};
pub use target_lines::{
    TargetLine, TargetLineSource, TargetLines, target_lines, target_lines_observed,
};
pub use templates::{
    ApplyDecision, ApplyGoal, ApplyState, Brief, BriefStep, InputKind, MAX_APPLY_OPERATION_ID,
    MAX_INPUT_TEXT, MAX_TEMPLATE_APPLIES, MAX_TEMPLATE_INPUTS, MAX_TEMPLATE_LINE,
    MAX_TEMPLATE_STEPS, MAX_TEMPLATE_TEXT, MAX_TEMPLATES, MAX_TEMPLATES_BYTES, PRIORITIES,
    STEP_GATES, StepAfter, StepTicket, TEMPLATE_APPLY_DIR, TEMPLATE_GOAL_TAG, TEMPLATES_PATH,
    Template, TemplateApply, TemplateApplyStore, TemplateError, TemplateGoal, TemplateInput,
    TemplateProblem, TemplateState, TemplateStep, TemplateStore, TemplatesFile, decide_apply,
    input_value, merge_templates, placeholders, reference, render_brief, resolve_inputs,
    substitute, validate_template,
};
pub use timeline::{
    DEFAULT_WAVE_MINUTES, DecisionWait, EstimateBasis, ItemState, KickOff, PermissionWait,
    PlacementHint, RunGateVerdicts, TargetLocation, TicketLabel, Timeline, TimelineInput,
    TimelineItem, TimelinePhase, TimelinePolicy, TimelineProvisional, TimelineProvisionalGate,
    TimelineRun, TimelineStep, TimelineTarget, WaitsOn, Wave, WaveKind, timeline,
};
pub use tool_capacity::{
    ActiveLimit, Applied, BACKOFF_BASE_MS, BACKOFF_CAP_MS, BACKOFF_FACTOR, Candidate,
    CapacityEntry, CapacityReason, CapacityState, CapacityStatus, DaySpend, Excluded,
    ExclusionReason, LimitSignal, MAX_CAPACITY_ENTRIES, MAX_RECENT_SIGNALS, MAX_RESET_AHEAD_MS,
    MAX_SIGNAL_COST_CENTS, MAX_SIGNAL_SKEW_MS, Selection, SelectionRequest, SessionCount,
    SignalKind, SignalOutcome, SignalSupport, ToolCapacityRegistry, apply_signal, backoff_ms,
    known_spend_cents, select, state_at, status_for,
};
pub use triggers::{
    ActiveRun, BranchWatch, Delivery, DeliveryDecision, DeliveryKind, DeliveryOutcome, EventField,
    MAX_DELIVERY_KEY, MAX_HANDLED_KEYS, MAX_TRIGGER_HISTORY, MAX_TRIGGER_INPUTS, MAX_TRIGGER_LINE,
    MAX_TRIGGERS, Overlap, PendingOn, TRIGGER_ACTOR_PREFIX, TRIGGER_DIR, TRIGGER_OPERATION_PREFIX,
    Trigger, TriggerAction, TriggerCursor, TriggerError, TriggerHistoryEntry, TriggerOp,
    TriggerPlacement, TriggerProblem, TriggerReview, TriggerSchedule, TriggerSource, TriggerSpec,
    TriggerState, TriggerStore, TriggerTemplate, TriggerWarning, WouldRun, decide_delivery,
    delivery_operation_id, event_value, keys as trigger_keys, spec_digest,
    transition as trigger_transition, validate_spec as validate_trigger,
};
pub use validation::{
    DEFAULT_STEP_TIMEOUT_SECONDS, ExecutionEnd, ExecutionOutcome, ExpectedEvidence,
    MAX_COMMAND_ARGS, MAX_COMMAND_BYTES, MAX_STEP_ATTEMPTS, MAX_STEP_PREREQUISITES,
    MAX_STEP_RESULTS, MAX_STEP_TIMEOUT_SECONDS, MAX_STEPS_PER_TICKET, MAX_VALIDATION_PROFILES,
    MAX_VALIDATION_STEPS, NewResult, ProfileCheck, ProfileProblem, ResultUpdate, RetryPolicy,
    StepDefinition, StreamRef, VALIDATION_DEADLINE_GRACE_MS, ValidationCheck, ValidationError,
    ValidationEvidence, ValidationProfile, ValidationProfileStore, ValidationRegistry,
    ValidationResult, ValidationState, ValidationStep, effective, evaluate, may_retry,
    next_lapse_ms,
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
    /// HS2-XHQ1EH: the target is unavailable, the policy's limits refuse the action,
    /// or a person denied it.
    #[error("orchestration action is not authorized: {}", .0.code())]
    Denied(DenialReason),
    /// HS2-XHQ1EH: a person has not answered the permission request yet. Nothing
    /// waits for the answer; the answer wakes the scheduler.
    #[error("orchestration action waits for permission request {request_id}")]
    PermissionRequired { request_id: String },
    /// HS2-XHQ1EH: a judgment call waits on a person's answer to the `FEEDBACK
    /// NEEDED` note on the orchestration ticket.
    #[error("orchestration action waits for a decision on note {note_id}")]
    DecisionRequired { note_id: String },
    /// HS2-SBQ3T8: the caller may never perform this operation, whatever is granted.
    #[error("orchestration operation is forbidden: {0}")]
    Forbidden(&'static str),
    /// HS2-AZ15GA: the target's own ticket source refused or could not perform an
    /// authorized operation. The target stays authoritative; nothing is retried elsewhere.
    #[error("{}", .0.message())]
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
    /// HS2-SHH7WW: a site of a project does not fit the project it names.
    #[error("orchestration site refused the request: {}", .0.code())]
    Site(SiteError),
    /// HS2-061YTB: a dispatch of a planned ticket whose target machine does not meet
    /// a requirement of its plan step (`unmet`) or did not report it (`unknown`).
    /// `requirement` names it, such as `os:windows`.
    #[error(
        "orchestration dispatch refused: the target's machine {} requirement `{requirement}`",
        if *.state == RequirementState::Unmet { "does not meet" } else { "did not report" }
    )]
    Requirement {
        state: RequirementState,
        requirement: String,
    },
    /// HS2-8Z9VHH: the session's machine refused admission. Its code stays
    /// `session_admission_refused`; the refusal names the reason, the limit and the
    /// observed value, so a person learns what to change.
    #[error("the machine refused the session: {}", .0.describe())]
    AdmissionRefused(Box<AdmissionRefusal>),
    /// HS2-A4RVYA: a plan template, its inputs or its apply refused the request.
    #[error("orchestration template refused the request: {}", .0.code())]
    Template(TemplateError),
    /// HS2-KXKA7X: a trigger refused the request.
    #[error("orchestration trigger refused the request: {}", .0.code())]
    Trigger(TriggerError),
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
            Self::PermissionRequired { .. } => DenialReason::PermissionRequired.code(),
            Self::DecisionRequired { .. } => DenialReason::DecisionRequired.code(),
            Self::Forbidden(_) => "forbidden",
            Self::TargetOperation(error) => error.code(),
            Self::Run(error) => error.code(),
            Self::Validation(error) => error.code(),
            Self::Gate(error) => error.code(),
            Self::Readiness(error) => error.code(),
            Self::Session(error) => error.code(),
            Self::Site(error) => error.code(),
            Self::Requirement { state, .. } => match state {
                RequirementState::Unmet => "requirement_unmet",
                RequirementState::Unknown => "requirement_unknown",
            },
            Self::AdmissionRefused(_) => SessionError::AdmissionRefused.code(),
            Self::Template(error) => error.code(),
            Self::Trigger(error) => error.code(),
        }
    }

    /// HS2-061YTB: a refusal's detail that is not a record to answer: the step
    /// requirement a dispatch's target machine misses, as `("requirement", name)`.
    pub fn refusal_detail(&self) -> Option<(&'static str, &str)> {
        match self {
            Self::Requirement { requirement, .. } => Some(("requirement", requirement)),
            _ => None,
        }
    }

    /// HS2-8Z9VHH: the admission refusal behind a `session_admission_refused` error.
    pub fn admission_refusal(&self) -> Option<&AdmissionRefusal> {
        match self {
            Self::AdmissionRefused(refusal) => Some(refusal),
            _ => None,
        }
    }

    /// HS2-XHQ1EH: the record a person answers to let a refused action go ahead:
    /// `("request_id", id)` for a permission request, `("note_id", id)` for a decision.
    pub fn answer_reference(&self) -> Option<(&'static str, &str)> {
        match self {
            Self::PermissionRequired { request_id } => Some(("request_id", request_id)),
            Self::DecisionRequired { note_id } => Some(("note_id", note_id)),
            _ => None,
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
    /// HS2-AW2AKF: the peer refused this machine's pairing itself, not a provider
    /// credential: its approval expired and needs renewal on the peer.
    PeerSessionExpired,
    /// HS2-AW2AKF: the peer revoked this machine's pairing; only a new pairing helps.
    PeerSessionRevoked,
    /// HS2-AW2AKF: the peer paired this machine read-only, so it refuses every write.
    PeerReadOnly,
    /// HS2-AW2AKF: the peer asked for browser write proof (Origin and CSRF): it predates
    /// bearer authentication for paired machines and needs updating.
    PeerCsrfRequired,
    /// HS2-AW2AKF: the peer refused this machine's credential for another reason, such as
    /// an unknown pairing or a denied certificate.
    PeerAuthRefused,
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
            Self::PeerSessionExpired => "target_peer_session_expired",
            Self::PeerSessionRevoked => "target_peer_session_revoked",
            Self::PeerReadOnly => "target_peer_read_only",
            Self::PeerCsrfRequired => "target_peer_csrf_required",
            Self::PeerAuthRefused => "target_peer_auth_refused",
        }
    }

    /// HS2-AW2AKF, HS2-8Z9VHH: a person-readable reason with the next step. Content-free
    /// like the code: it never carries the peer's or the provider's own text.
    pub fn message(self) -> &'static str {
        match self {
            Self::NotFound => "the target ticket or its ticket source does not exist on that site",
            Self::Unsupported => "the target's ticket provider does not support this operation",
            Self::Conflict => {
                "the target ticket changed or is held by someone else; refresh and retry"
            }
            Self::Unauthorized => {
                "the target's ticket provider refused its credentials; reconnect that ticket source on the target machine"
            }
            Self::RateLimited => {
                "the target's ticket provider is rate limiting requests; retry later"
            }
            Self::Disabled => {
                "the target's ticket source is disabled; enable it on the target machine"
            }
            Self::Invalid => "the target's ticket provider rejected the request as invalid",
            Self::PeerUnsupported => {
                "the remote site runs an older Hot Sheet without this operation; update it"
            }
            Self::LinksUnsupported => {
                "the orchestration ticket's source cannot store links; use a Git ticket source"
            }
            Self::Failed => "the target ticket operation failed",
            Self::PeerSessionExpired => {
                "this computer's approval on the remote site expired; renew it in Settings → Devices on that site"
            }
            Self::PeerSessionRevoked => {
                "the remote site revoked this computer's access; pair this computer with it again"
            }
            Self::PeerReadOnly => {
                "the remote site paired this computer read-only; give it read-write access in Settings → Devices on that site"
            }
            Self::PeerCsrfRequired => {
                "the remote site needs an update before a paired computer can change its tickets or sessions"
            }
            Self::PeerAuthRefused => {
                "the remote site refused this computer's credentials; check its pairing in Settings → Devices on that site"
            }
        }
    }

    /// HS2-AW2AKF: the refusal for a peer's own authentication code, or `None` when the
    /// code is not one. The peer answers these before any target route runs.
    pub fn from_peer_auth_code(code: &str) -> Option<Self> {
        Some(match code {
            "session_expired" => Self::PeerSessionExpired,
            "session_revoked" => Self::PeerSessionRevoked,
            "read_only" => Self::PeerReadOnly,
            "csrf_required" => Self::PeerCsrfRequired,
            "session_invalid" | "certificate_denied" | "host_access_required" => {
                Self::PeerAuthRefused
            }
            _ => return None,
        })
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
            Self::PeerSessionExpired,
            Self::PeerSessionRevoked,
            Self::PeerReadOnly,
            Self::PeerCsrfRequired,
            Self::PeerAuthRefused,
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
mod assignment_tests;
#[cfg(test)]
mod audit_tests;
#[cfg(test)]
mod binding_tests;
#[cfg(test)]
mod capacity_tests;
#[cfg(test)]
mod gate_tests;
#[cfg(test)]
mod home_outage_tests;
#[cfg(test)]
mod hosted_audit_tests;
#[cfg(test)]
mod hosted_validation_tests;
#[cfg(test)]
mod integrator_session_tests;
#[cfg(test)]
mod joining_tests;
#[cfg(test)]
mod line_observations_tests;
#[cfg(test)]
mod permission_tests;
#[cfg(test)]
mod plan_update_tests;
#[cfg(test)]
mod planning_tests;
#[cfg(test)]
mod policy_tests;
#[cfg(test)]
mod project_role_session_tests;
#[cfg(test)]
mod project_type_tests;
#[cfg(test)]
mod provisional_tests;
#[cfg(test)]
mod readiness_tests;
#[cfg(test)]
mod reconcile_source_tests;
#[cfg(test)]
mod reconcile_tests;
#[cfg(test)]
mod replan_memory_tests;
#[cfg(test)]
mod resource_tests;
#[cfg(test)]
mod role_delivery_tests;
#[cfg(test)]
mod role_session_tests;
#[cfg(test)]
mod run_event_tests;
#[cfg(test)]
mod run_tests;
#[cfg(test)]
mod scheduler_tests;
#[cfg(test)]
mod session_fence_tests;
#[cfg(test)]
mod session_idle_tests;
#[cfg(test)]
mod session_tests;
#[cfg(test)]
mod shared_plan_tests;
#[cfg(test)]
mod site_scheduling_tests;
#[cfg(test)]
mod sites_tests;
#[cfg(test)]
mod summary_tests;
#[cfg(test)]
mod summary_use_tests;
#[cfg(test)]
mod summary_wait_tests;
#[cfg(test)]
mod target_lines_tests;
#[cfg(test)]
mod template_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod timeline_tests;
#[cfg(test)]
mod tool_capacity_tests;
#[cfg(test)]
mod trigger_tests;
#[cfg(test)]
mod validation_tests;
