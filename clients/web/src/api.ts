import { browserRandomId } from './browser-id';
import { isTicketCollectionRead, responseChangeCursor, ticketReadCursors } from './change-read-gap';
import { parseSchedulePreview, parseScheduleView, parseScheduleViews, type ScheduleInput } from './command-schedules';
import { type FeatureSettingsChange, parseFeatureSettings } from './feature-areas';
import { parseHostCpuFrame } from './host-cpu-feed';
import { type HostHealthConfiguredThresholds, parseHostHealth, parseHostHealthThresholds } from './host-health';
import {
  assertHostAction,
  HOST_ACCESS_REQUIRED_CODE,
  type HostOnlyAction,
  HostOnlyActionError,
} from './host-only-actions';
import { errorCodeOf, errorMessageOf, responseJson, responseJsonBody } from './json-value';
import {
  parseOrchestrationList,
  parseOrchestrationRecord,
  parseOrchestrationTargetLines,
  parseOrchestrationTimeline,
} from './orchestration-timeline';
import type {
  OrchestratorAsk,
  PermissionAutomation,
  PermissionPattern,
  PermissionSecretSuggestion,
} from './permission-notifications';
import { prioritiesToWire } from './priority-wire';
import { observeRequest } from './request-observer';
import { beginServerRequest, describeServerRequest, endServerRequest } from './server-busy';
import {
  ignoreBody,
  parseAccountIdentity,
  parseAdvisoryClaimStatus,
  parseAiCommandRun,
  parseAiToolDefaults,
  parseAiToolDescriptor,
  parseCalibrationReport,
  parseCheckout,
  parseCheckoutActivity,
  parseCheckoutAnalytics,
  parseCheckoutRelink,
  parseCheckoutTicketPage,
  parseCheckoutTicketRowsPage,
  parseClientNotification,
  parseClientNotifications,
  parseCodeReview,
  parseCommandDefinition,
  parseCommandRun,
  parseCompactionPlan,
  parseCompactionResult,
  parseCorruptTicket,
  parseCurrentDiff,
  parseCustomView,
  parseDevelopmentStrategyReport,
  parseDuplicateBacklinks,
  parseFeedbackDelivery,
  parseFilePath,
  parseFullTicket,
  parseGitHubAuthStart,
  parseGitHubAuthStatus,
  parseGitHubRepositoryAccess,
  parseGroupCreate,
  parseGroupDelete,
  parseGroupList,
  parseGroupRename,
  parseGroupUngroup,
  parseGuidanceBatchReview,
  parseGuidanceInterviewReview,
  parseGuidanceOverview,
  parseGuidanceRecommendationReport,
  parseGuidanceRulePreview,
  parseGuidanceSourceReview,
  parseGuidanceSourceUpdates,
  parseHostedHandBackReply,
  parseHostedOrchestrationProjectChoice,
  parseHostedStop,
  parseLegacyPermissionDiscardOutcome,
  parseLegacyPermissionPreview,
  parseLiveHistoryList,
  parseLiveStateList,
  parseLiveStoreState,
  parseLiveTokenTotal,
  parseMessageConnectionReceipt,
  parseNoteDoctorReport,
  parseNotificationChannel,
  parseNotificationChannelRemoval,
  parseNotificationConfig,
  parseNotificationDelivery,
  parseNotificationProjectTiming,
  parseNotificationRule,
  parseNotifierList,
  parseOrchestrationAdmission,
  parseOrchestrationAutomaticSet,
  parseOrchestrationBinding,
  parseOrchestrationCapacity,
  parseOrchestrationInspection,
  parseOrchestrationManifest,
  parseOrchestrationPolicyView,
  parseOrchestrationProjectView,
  parseOrchestrationResolution,
  parseOrchestrationSession,
  parseOrchestrationSites,
  parseOrchestrationToolCapacity,
  parsePermissionAutomationSettings,
  parsePermissionDecision,
  parsePermissionRequest,
  parsePermissionRule,
  parsePhaseReport,
  parsePollResponse,
  parseProjectTypeView,
  parseProviderAccount,
  parseProviderConnection,
  parseProviderDescriptor,
  parseProviderOutboxOperation,
  parseQueuedProviderResult,
  parseRemovedOrchestrationProject,
  parseRepositoryCommitPage,
  parseRepositoryFilePage,
  parseRepositorySizeReport,
  parseRepositoryStatus,
  parseRulesyncUpdateReport,
  parseServerCompatibility,
  parseSourceDetach,
  parseStoredTicket,
  parseStringList,
  parseTerminalInfo,
  parseTerminalLimits,
  parseTerminalName,
  parseTerminalRead,
  parseTerminalSettings,
  parseTicketCreationSettings,
  parseTicketPrefix,
  parseTicketRow,
  parseTicketStoreSyncReport,
  parseTicketUsage,
  parseToolConnection,
  parseToolSession,
  parseTrashPurge,
  parseTrashPurgePreview,
  parseTrashSettings,
  parseTtsVoices,
  type ResponseParser,
} from './server-response-parsers';
import { parseStandbySettings, type StandbySettingsView } from './standby';
import type { TicketCloseReason } from './ticket-close';
import { completionDayStarts } from './ticket-completion-trend';
import { parseWorkerView } from './worker-view-model';

export type DevelopmentPreset = 'ticket-assured' | 'steady-main' | 'risk-guided' | 'ci-first';
export type IntegrationStrategy = 'serial' | 'batch' | 'ci-staged';
export type LocalGate = 'full-local' | 'targeted' | 'risk-selected' | 'best-effort';
/** A custom strategy cannot select CI First's best-effort gate (HS2-ZGS7JP). */
export type CustomLocalGate = Exclude<LocalGate, 'best-effort'>;
export interface CustomDevelopmentStrategy {
  integration: IntegrationStrategy;
  local_gate: CustomLocalGate;
  minimum_tier: 'T0' | 'T1' | 'T2';
  budget_minutes?: number;
  browser: boolean;
  platform: boolean;
  perf: boolean;
  test_plan_timing: 'before-work' | 'before-handoff';
  ci_gate: string;
  staging_branch?: string;
  green_main_goal_minutes?: number;
  red_ci_recovery: 'attribute-and-retry' | 'fallback-to-serial';
}
export interface DevelopmentStrategyPolicy {
  version: number;
  default: DevelopmentPreset;
  green_main_goal_minutes: number;
  /** CI First's best-effort local budget; absent means 10 minutes (HS2-ZGS7JP). */
  best_effort_budget_minutes?: number;
  default_custom?: string;
  custom: Partial<Record<string, CustomDevelopmentStrategy>>;
}
export interface DevelopmentStrategyReport {
  policy: DevelopmentStrategyPolicy;
  effective: {
    preset: string;
    source: 'unconfigured-project' | 'project' | 'session' | 'ticket';
    integration: IntegrationStrategy;
    local_gate: LocalGate;
    custom?: CustomDevelopmentStrategy;
    require_green_main_sha: boolean;
    attribute_red_ci_before_completion: boolean;
    green_main_goal_minutes: number | null;
    /** Present only for CI First's best-effort local gate. */
    local_budget_minutes?: number;
    /** HS2-ZGS7JP: true only for CI First, whose covering main CI run is the gate. */
    push_without_receipt: boolean;
  };
  presets: DevelopmentPreset[];
}

export interface GuidanceOverview {
  owner: 'builtin' | 'rulesync';
  pending_consolidation_review?: boolean;
  explicit_selection: boolean;
  default_repository: string;
  local_rules?: string[];
  sources: Array<{
    id: string;
    group_id?: string | null;
    repository: string;
    requested_ref: string;
    commit: string;
    rules: Array<{
      name: string;
      version: number;
      pinned_sha256?: string;
      installed_version: number | null;
      description: string;
      owner: string | null;
      tags: string[];
      selected: boolean;
      chosen_version?: number;
      chosen_sha256?: string;
      chosen_commit?: string;
      compatible?: boolean;
      minimum_hotsheet_version?: string | null;
      historical_versions?: Array<{
        version: number;
        sha256: string;
        commit: string;
        minimum_hotsheet_version: string | null;
        compatible: boolean;
      }>;
    }>;
    skills: Array<{ name: string; selected: boolean }>;
  }>;
}
export interface GuidanceRulePreview {
  owner: 'builtin' | 'rulesync';
  current: string | null;
  pinned: string;
  after: string | null;
  outcome: 'add' | 'replace' | 'current' | 'protected';
}
export interface GuidanceSourceUpdate {
  id: string;
  requested_ref: string;
  pinned_commit: string;
  observed_commit: string | null;
  state: 'current' | 'revision_changed' | 'pinned' | 'tag_moved' | 'check_failed';
  reason?: string;
  other_refs: Array<{ reference: string; commit: string }>;
  other_refs_truncated: boolean;
  other_refs_reason?: string;
}
export interface GuidanceSourceReview {
  update: GuidanceSourceUpdate;
  changes: Array<{
    name: string;
    kind: 'rule' | 'skill';
    change: 'added' | 'removed' | 'updated';
    selected: boolean;
    before_version: number | null;
    after_version: number | null;
  }>;
}
export interface GuidanceBatchSource {
  id: string;
  repository: string;
  refs: string[];
}
export interface GuidanceBatchReview {
  requests: GuidanceBatchSource[];
  pins: Array<{
    id: string;
    group_id: string;
    repository: string;
    requested_ref: string;
    commit: string;
    rules: number;
    skills: number;
  }>;
}
export interface GuidanceRecommendationReport {
  fingerprint: string;
  signals: Array<{ key: string; evidence: string }>;
  candidates: Array<{
    source: string;
    kind: 'rule' | 'skill';
    name: string;
    version: number;
    description: string;
    pinned_commit: string;
    evidence: string[];
    fingerprint: string;
    selected: boolean;
    installed_version: number | null;
    state: 'new' | 'accepted' | 'dismissed' | 'selected';
  }>;
  changes: Array<{ source: string; kind: 'rule' | 'skill'; name: string; change: 'new' | 'changed' | 'withdrawn' }>;
  source_registration_suggested: boolean;
  first_accept_replaces_bundled_default: boolean;
}
export interface GuidanceRecommendationDecision {
  source: string;
  kind: 'rule' | 'skill';
  name: string;
  fingerprint: string;
  decision: 'accepted' | 'dismissed';
}
export interface GuidanceInterviewReview {
  state: 'not_started' | 'awaiting_answer' | 'answered';
  preview: {
    fingerprint: string;
    context: {
      signals: string[];
      candidates: Array<{
        id: string;
        source: string;
        kind: 'rule' | 'skill';
        name: string;
        version: number;
        pinned_commit: string;
        selected: boolean;
        evidence: string[];
      }>;
    };
    questions: Array<{ id: string; prompt: string; choices: string[] }>;
    outbound_prompt: string | null;
  };
  answer: {
    advice: Array<{ candidate_id: string; stance: 'consider' | 'avoid' | 'uncertain'; explanation: string }>;
  } | null;
}
export interface RulesyncUpdateReport {
  state: 'current' | 'outdated' | 'unlocked' | 'unknown';
  executable_version: string | null;
  reason?: string;
  sources: Array<{
    source: string;
    state: RulesyncUpdateReport['state'];
    requested_ref: string | null;
    locked_ref: string | null;
    latest_ref: string | null;
  }>;
}
export type GuidanceAction =
  | { action: 'register'; id: string; repository: string; ref: string; expected_commit?: string }
  | { action: 'register_batch'; review: GuidanceBatchReview }
  | { action: 'select'; source: string; rules: string[]; skills: string[] }
  | { action: 'cache_rule_version'; source: string; rule: string; requested_ref: string; expected_commit: string }
  | { action: 'choose_rule_version'; source: string; rule: string; sha256: string | null }
  | { action: 'owner'; owner: 'builtin' | 'rulesync' }
  | { action: 'apply' };

export type Capabilities = Record<
  | 'create'
  | 'update'
  | 'close'
  | 'notes'
  | 'note_edit'
  | 'note_delete'
  | 'attachments'
  | 'assignment'
  | 'review_requests'
  | 'dependencies'
  | 'up_next'
  | 'close_reasons'
  | 'claims'
  | 'atomic_batch'
  | 'not_working_report'
  | 'offline_mutation'
  | 'history'
  | 'watch'
  | 'provider_idempotency',
  boolean
> & {
  query_fields: string[];
  /** Atomic note-kind correction; omitted older/provider capability is false. */
  note_kind_edit?: boolean;
  /** Link an existing note as a feedback response (HS2-J4G9GB); omitted older/provider capability is false. */
  note_response_link?: boolean;
  /** Append or correct a note's completion confidence; omitted older capability is false. */
  note_confidence?: boolean;
  /** Existing attachments can be renamed, deleted, regrouped, and annotated (HS2-HSA64D). Older servers omit it. */
  attachment_edit?: boolean;
  /** Existing attachments can be deleted even when they cannot be renamed (Jira, HS2-51ADSG). */
  attachment_delete?: boolean;
  attachment_crop?: boolean;
  /** Host may expose an opt-in durable provisional field-edit endpoint. */
  write_behind?: boolean;
  /** Provider-native advisory process markers; separate from exclusive claims. */
  advisory_claims?: boolean;
  /** Tickets can name a group (HS2-PDSHNN). Older servers and external providers omit it. */
  groups?: boolean;
  /** Structured, revisable ratings and the provider feedback query. */
  ai_feedback?: boolean;
};
/** One voice a server speech provider offers (`GET /tts/voices`, HS2-CEZVSE). */
export interface TtsVoice {
  /** The value the provider accepts as a synthesis `voice`. */
  name: string;
  /** A BCP 47 language tag such as `en-US`. */
  lang: string;
}
export interface ProviderDescriptor {
  connection_id: string;
  provider: string;
  display_name: string;
  locator: string;
  default: boolean;
  /** Project-local icon background from the shared customization palette. */
  color?: string;
  /** A reviewed legacy recovery whose former store identity could not be proved. */
  unverified_recovery?: boolean;
  /** A legacy Git link must be reviewed before its tickets can be used. */
  identity_review_required?: boolean;
  /** The linked path holds a different Git store than the recorded source. */
  identity_mismatch?: boolean;
  /** The linked Git store path is missing or its metadata cannot be read. */
  store_unavailable?: boolean;
  /** A Git source's display prefix for new ticket slugs (HS2-8QJ6W3); other providers omit it. */
  ticket_prefix?: string;
  capabilities: Capabilities;
}

/** Best-effort provider marker; this never proves an exclusive lease (HS2-2MZWZT). */
export interface AdvisoryClaimStatus {
  projection: {
    claims: Array<{ worker: string; operation_id: string; expires_unix_ms: number }>;
    conflicted: boolean;
    invalid_markers: number;
  };
  provisional: boolean;
  remote_verified: boolean;
  automatic_handoff_safe: boolean;
  pending_operation_ids: string[];
  pending_operations: Array<{ operation_id: string; state: 'queued' | 'confirmed' | 'needs_attention' | 'expired' }>;
}

export type AdvisoryClaimEventKind = { type: 'claim' | 'renew'; expires_unix_ms: number } | { type: 'release' };

export interface AdvisoryClaimEventInput {
  worker: string;
  operation_id: string;
  kind: AdvisoryClaimEventKind;
  provisional?: boolean;
}
export interface ProviderConnection {
  id: string;
  provider: string;
  locator: string;
  name: string | null;
  default: boolean;
  settings: Record<string, unknown>;
  /** Temporarily switched off; absent when enabled (HS2-SF6W34). */
  disabled?: boolean;
  /**
   * Every project that owns this source, as `GET /checkouts/{id}/provider-connections` reports it
   * (HS2-SM9PM8). Absent on a create/update response.
   */
  projects?: AccountProject[];
}
/** A project (checkout) that uses a ticket source (HS2-SM9PM8). */
export interface AccountProject {
  id: string;
  alias: string;
}
/** What removing a source from one project did (HS2-SM9PM8). */
export interface SourceDetach {
  checkout_id: string;
  connection_id: string;
  unlinked: boolean;
  /** No other project used it, so the connection itself was deleted. */
  removed_connection: boolean;
  still_used_by: string[];
}
/** A machine-wide provider sign-in and the ticket sources and projects using it (HS2-SM9PM8). */
export interface ProviderAccount {
  /** The credential reference (OS-keychain entry name); never the secret. */
  id: string;
  provider: string;
  /**
   * The host signed in to. An unused Hot Sheet GitHub sign-in reports the site it signed in to
   * (HS2-16MYXN); empty only for a sign-in whose site is unknown.
   */
  host: string;
  /**
   * The endpoint a new source reusing this account needs: GitHub Enterprise or self-managed GitLab
   * `api_base`, or the Jira site URL. Absent for github.com and gitlab.com.
   */
  base_url?: string;
  identity?: string;
  /** Minted by Hot Sheet's GitHub sign-in rather than registered with `hotsheet key set`. */
  managed: boolean;
  sources: Array<{
    connection_id: string;
    name: string;
    locator: string;
    disabled: boolean;
    projects: AccountProject[];
  }>;
  projects: AccountProject[];
}
/** Repositories the signed-in user can pick, and what each app installation grants (HS2-27T5WT). */
export interface GitHubRepositoryAccess {
  repositories: string[];
  installations?: Array<{
    account: string; /** `all` or `selected`. */
    selection: string;
    settings_url: string | null;
  }>;
  install_url?: string | null;
}
export interface GitHubAuthStart {
  session_id: string;
  user_code: string;
  verification_uri: string;
  expires_in: number;
}
export type GitHubAuthStatus =
  | { state: 'pending' }
  | { state: 'authorized'; credential_reference: string }
  | { state: 'denied' | 'expired' | 'cancelled' }
  | { state: 'error'; message: string };
export interface Note {
  id: string;
  /** Provider comment's stable Hot Sheet marker when it differs from the native id. */
  hotsheet_note_id?: string;
  kind: 'regular' | 'activity' | 'feedback_needed' | 'feedback_draft' | 'status';
  /** Explicit author intent; absent on legacy notes (HS2-CV5NMC). */
  kind_explicit?: boolean;
  created_at: string;
  edited_at: string;
  summary?: string;
  /** AI completion confidence (0-100) recorded on this note (HS2-DWTJ43). */
  confidence?: number;
  /** Persisted source note id of AI thumbs feedback; older/provider notes use their text prefix. */
  feedback_for?: string;
  /** New request tracked independently from legacy sequential exchanges. */
  feedback_request?: boolean;
  /** Request note id answered by this note; separate from AI thumbs feedback_for. */
  response_to?: string;
  /** Current rating; null is a withdrawal. Exact legacy prose is normalized on read. */
  ai_feedback?: { target: string; rating: 'helpful' | 'not_helpful' | null; explanation?: string };
  /** True once a human changed AI-authored text, including after a later revert. */
  human_edited?: boolean;
  /** Who wrote the note, when recorded (HS2-32QDZ3). */
  actor?: NoteActor;
  text: string;
}
export interface NoteDoctorReport {
  ticket_id: string;
  findings: Array<{ id: string; note_id: string; description: string; proposed_operation: string }>;
}
/** What happened after one completion (HS2-Q1WCCY). */
export type CalibrationOutcome = 'verified' | 'reopened' | 'pending';
/** One completion event of the calibration report: the server's `Completion` shape. */
export interface CalibrationEvent {
  slug: string;
  completed_at: string;
  confidence?: number;
  outcome: CalibrationOutcome;
}
/** Totals for one rubric band (or `unscored`); optional rates are omitted until known. */
export interface CalibrationBand {
  band: 'verified' | 'assumed' | 'partial' | 'unverified' | 'unscored';
  range: string;
  completions: number;
  verified: number;
  reopened: number;
  pending: number;
  reopen_rate?: number;
  mean_confidence?: number;
}
/** `GET /checkouts/{id}/confidence-report` (HS2-Q1WCCY). */
export interface CalibrationReport {
  completions: number;
  scored: number;
  bands: CalibrationBand[];
  events: CalibrationEvent[];
}
/** Checkout-scoped analytics keep each Git store distinct so shared stores are counted once. */
export interface TicketFlowSummary {
  total: number;
  open: number;
  completed: number;
  current_by_status: Record<string, number>;
  current_by_category: Record<string, number>;
  throughput_by_day: Record<string, number>;
  cycle_time_seconds: { samples: number; average: number; p50: number; p95: number };
  historical_cumulative_flow_available: boolean;
}
export interface UsageRollup {
  events: number;
  tokens_in: number;
  tokens_out: number;
  cost_usd: number;
  by_model: Record<string, number>;
  by_ticket: Record<string, number>;
  by_day: Record<string, number>;
}
export interface CheckoutAnalytics {
  stores: Array<{ store_id: string; ticket_flow: TicketFlowSummary; usage: UsageRollup }>;
  unavailable_sources: Array<{ connection_id: string; provider: string }>;
}
/** How close a repository is to GitHub's size limits; computed by the server (HS2-QF44T7). */
export type RepositorySizeLevel = 'ok' | 'warning' | 'critical';
/** Whether one repository could be measured. */
export type RepositorySizeStatus = 'measured' | 'not_repository' | 'unsupported' | 'unavailable';
/** One entry of `GET /checkouts/{id}/repository/size`: the code repository or one ticket source. */
export interface RepositorySizeEntry {
  kind: 'code' | 'ticket_store';
  connection_id?: string;
  provider?: string;
  label: string;
  locator: string;
  status: RepositorySizeStatus;
  /** `git count-objects -v` in bytes: loose plus packed is what a push carries. */
  git?: {
    loose_objects: number;
    loose_bytes: number;
    packed_objects: number;
    packs: number;
    packed_bytes: number;
    total_bytes: number;
  };
  /** A ticket store's on-disk `attachments/` payload; `truncated` makes `bytes` a lower bound. */
  attachments?: { bytes: number; files: number; truncated: boolean };
  shared_with_code: boolean;
  level: RepositorySizeLevel;
  detail?: string;
}
/** `GET /checkouts/{id}/repository/size` (HS2-QF44T7). */
export interface RepositorySizeReport {
  checkout_id: string;
  thresholds: { warning_bytes: number; critical_bytes: number };
  level: RepositorySizeLevel;
  entries: RepositorySizeEntry[];
}
/** A ticket store's published sync state (HS2-ADHFKZ, docs/02 §2.12.2). */
export type TicketStoreSyncState =
  | 'not_repository'
  | 'no_remote'
  | 'pending'
  | 'up_to_date'
  | 'synced'
  | 'offline'
  | 'push_raced'
  | 'rejected'
  | 'conflict'
  | 'blocked'
  | 'rebase_in_progress'
  | 'history_recovery_required';
/** An unpublished Git blob larger than the remote accepts. */
export interface TicketStoreOversizedObject {
  path: string;
  bytes: number;
  limit_bytes: number;
}
/** How Hot Sheet asked an AI to repair a stall sync cannot finish alone (HS2-QR1K8R). */
export interface TicketStoreAiHelp {
  outcome: 'session_asked' | 'started' | 'finished' | 'failed' | 'ticket_only';
  mode: 'off' | 'session' | 'start';
  tool?: string;
  ticket?: string;
  asked_at: string;
  finished_at?: string;
  reason?: string;
}
/** One ticket store's durable sync health; computed by the server without contacting the remote. */
export interface TicketStoreSyncHealth {
  state: TicketStoreSyncState;
  syncing: boolean;
  upstream?: string;
  unpublished_commits: number;
  incoming_commits: number;
  last_attempt_at?: string;
  last_success_at?: string;
  consecutive_failures: number;
  reason?: string;
  oversized?: TicketStoreOversizedObject[];
  next_action?: string;
  /** Whether ticket merges go through the semantic merge driver in this checkout (HS2-9TV6A2). */
  merge_driver?: 'registered' | 'repaired' | 'missing';
  merge_driver_repaired_at?: string;
  /** Present while an AI was asked to repair the stall and the store has not published since. */
  ai_help?: TicketStoreAiHelp;
  /** Writes an interrupted recovery saved but never wrote back, by backup ref (HS2-RDPT34). */
  stranded_saved_writes?: Array<{ backup_ref: string; paths: string[] }>;
}
/** `GET /checkouts/{id}/repository/sync` (HS2-ADHFKZ): every linked Git ticket store. */
export interface TicketStoreSyncReport {
  checkout_id: string;
  stores: Array<{ connection_id: string; label: string; locator: string; health: TicketStoreSyncHealth }>;
}
/** Read-only preview of old attached payloads and orphaned Git history. */
export interface CompactionPlan {
  head: string;
  branch: string;
  min_age_days: number;
  oversized_bytes: number;
  candidates: Array<{
    ticket_id: string;
    ticket_slug: string;
    attachment_id: string;
    filename: string;
    path: string;
    bytes: number;
    sha256: string;
    reason: 'duplicate' | 'oversized' | 'orphaned';
    historical_blobs?: Array<{ oid: string; bytes: number }>;
    removed_in_commit?: string;
  }>;
  reclaimable_payload_bytes: number;
  fingerprint: string;
}
export interface CompactionResult {
  old_head: string;
  new_head: string;
  backup: string;
  candidates_removed: number;
  git_bytes_before: number;
  git_bytes_after: number;
  recovery: string[];
}
/** A note's recorded author: the acting role and an optional stable id (HS2-32QDZ3). */
export interface NoteActor {
  role: 'human' | 'ai' | 'system' | 'unknown';
  id?: string;
}
export interface MediaAnnotation {
  id: string;
  x: number;
  y: number;
  width: number;
  height: number;
  start_ms?: number;
  end_ms?: number;
  text: string;
  shape?:
    | { type: 'rect' | 'strike' }
    | { type: 'freehand'; points: { x: number; y: number }[]; closed?: boolean }
    | {
        type: 'arrow';
        points: { x: number; y: number }[];
        startHead?: AnnotationArrowHead;
        endHead?: AnnotationArrowHead;
      }
    | { type: 'insertion'; point: { x: number; y: number } };
  intents?: string[];
}
export type AnnotationArrowHead = 'none' | 'open' | 'closed' | 'flat' | 'openCircle' | 'closedCircle';
export type AttachmentActorRole = 'human' | 'ai' | 'system' | 'unknown';
export type AttachmentPurpose = 'problem_evidence' | 'correctness_evidence' | 'reference' | 'other';
export interface AttachmentActor {
  identity?: string;
  display_name?: string;
  role: AttachmentActorRole;
}
export interface AttachmentMetadata {
  batch_id?: string;
  batch_label?: string;
  actor?: AttachmentActor;
  purpose?: AttachmentPurpose;
}
export interface Attachment extends AttachmentMetadata {
  id: string;
  filename: string;
  created_at: string;
  annotations?: MediaAnnotation[];
  /** Integer pixels in the immutable original image. */
  crop?: { x: number; y: number; width: number; height: number };
  /** Provider marker revision for optimistic attachment markup updates. */
  revision?: string;
}
export interface Ticket {
  qualified_id: string;
  /** Local provider intents awaiting remote acknowledgement. */
  pending_operation_ids?: string[];
  native_id: string;
  native_url?: string;
  title: string;
  status: string;
  started_phase?: StartedPhase;
  connection_id: string;
  notes?: Note[];
  attachments?: Attachment[];
}

export type ProviderOutboxState = 'queued' | 'sending' | 'rate_limited' | 'needs_attention' | 'confirmed' | 'discarded';

export interface ProviderOutboxOperation {
  operation_id: string;
  connection_id: string;
  native_id: string;
  state: ProviderOutboxState;
  attempts: number;
  next_attempt_at: number;
  last_error?: string;
  conflict?: Record<string, unknown>;
}

export interface QueuedProviderResult {
  operation_id: string;
  state: ProviderOutboxState;
  ticket: FullTicket;
}
export interface CheckoutSource {
  connection_id: string;
  provider: string;
  locator: string;
}
export interface Checkout {
  id: string;
  root: string;
  alias: string;
  repository?: string;
  stores: string[];
  sources?: CheckoutSource[];
  /** Current Git source id to former path for a reviewed, unverified legacy recovery. */
  unverified_store_sources?: Record<string, string>;
  default_source?: string;
}
export type { TicketCloseReason } from './ticket-close';
export type StartedPhase = 'analyzing' | 'planning' | 'working' | 'initial_testing' | 'integrating' | 'final_testing';
export interface TicketRow {
  connection_id: string;
  /** Durable external-provider intents still projected onto this ticket. */
  pending_operation_ids?: string[];
  native_id: string;
  qualified_id: string;
  id: string;
  slug: string;
  title: string;
  category?: string;
  priority?: string;
  status?: string;
  started_phase?: StartedPhase;
  up_next: boolean;
  feedback_needed: boolean;
  /** User-facing Needs Review state: feedback or an explicit review request. */
  needs_review?: boolean;
  tags: string[];
  blocked_by: string[];
  follow_ups?: ProjectTicketRef[];
  /** Links to tickets in declared orchestration targets (HS2-AZ15GA). */
  orchestration_links?: OrchestrationLink[];
  blocked_reason?: string;
  /** The ticket's group name (HS2-PDSHNN); absent or null when ungrouped. */
  group?: string | null;
  claimed_by?: string;
  claim_lease_expires_at?: string;
  /** The live claim holder's estimated completion time (HS2-DQQ0AX). */
  claim_eta_at?: string;
  /** When the live claim began, derived from its history (HS2-XQMDQB). */
  claim_started_at?: string;
  /** When the current Started phase began, derived server-side from phase history (HS2-YDGM04). */
  started_phase_since?: string;
  /** Median duration of this store's past spans of the current testing phase (HS2-YDGM04). */
  started_phase_estimate_ms?: number;
  worker_label?: string;
  /** Whether a live claim's worker belongs to this Hot Sheet server or a separate one seen through
   * the synced store (HS2-DQJPW0); absent without a claim or when the claim predates host records. */
  claim_origin?: 'local' | 'remote';
  /**
   * HS2-ZR8B4N: how this server's store sees the live claim beside it: provisional (taken offline, not confirmed,
   * never exclusive) or lost to another machine while the losing session has not been told. Absent otherwise.
   */
  claim_standing?: ClaimStanding;
  legacy_number?: string;
  claim_count: number;
  /** Derived completion confidence of a completed/verified ticket (HS2-RD4M29). */
  latest_confidence?: number;
  claim_history?: ClaimEvent[];
  created_at?: string;
  /** Who created the ticket (HS2-9FPHWY); absent when unknown (older tickets, external providers). */
  created_by?: NoteActor;
  updated_at?: string;
  completed_at?: string;
  closed_at?: string;
  close_reason?: TicketCloseReason;
  duplicate_of?: string;
  details?: string;
  notes?: Note[];
}
/** HS2-ZR8B4N: a ticket's claim standing (`ClaimStanding` in the contract). */
export type ClaimStanding =
  { kind: 'provisional' } | { kind: 'lost'; worker: string; holder: string; detected_at: string };
export interface ClaimEvent {
  id: string;
  kind: 'claim' | 'renew' | 'release';
  worker: string;
  at: string;
  lease_expires_at?: string;
  /** The holder's estimate as of this claim or renewal (HS2-DQQ0AX). */
  eta_at?: string;
  worker_label?: string;
  /** The Hot Sheet host that wrote this event (HS2-DQJPW0). */
  host?: string;
}
export interface CheckoutTicketQuery {
  /** A local-first checkout read that avoids external credential access. */
  source?: 'git';
  text?: string;
  status?: string;
  /** `all` is every collection except Trash, for the All Tickets search (HS2-4J6764). */
  collection?: 'queue' | 'archive' | 'trash' | 'all';
  open?: boolean;
  up_next?: boolean;
  needs_review?: boolean;
  ai_created?: boolean;
  blocked?: boolean;
  close_reason?: TicketCloseReason;
  compact?: boolean;
  tags?: string;
  has_attachment?: boolean;
  has_media_annotation?: boolean;
  has_commit?: boolean;
  attachment?: string;
  created_after?: string;
  created_before?: string;
  updated_after?: string;
  updated_before?: string;
  completed_after?: string;
  completed_before?: string;
  verified_after?: string;
  verified_before?: string;
  sort?: 'updated' | 'priority' | 'title' | 'status';
  direction?: 'ascending' | 'descending';
  /** Comma-separated row projection; `slug` and the source `store` are always kept. */
  fields?: string;
  /** Unpaged reads only: explicit cap (at most 500) that accepts a truncated result. */
  limit?: number;
}
export interface CheckoutTicketCounts {
  total: number;
  queued: number;
  backlog: number;
  archive: number;
  trash?: number;
  open: number;
  up_next: number;
  active: number;
  started: number;
  verified?: number;
  completed_today: number;
  completion_trend?: number[];
}
export interface CheckoutTicketPage {
  items: TicketRow[];
  next_cursor?: string;
  counts: CheckoutTicketCounts;
  source_errors?: string[];
}
/** Unpaged checkout rows retain the server's partial/truncated response headers. */
export interface CheckoutTicketRows {
  items: TicketRow[];
  partial: boolean;
  truncated: boolean;
}
/** A checkout page requested with `counts=false`: the server returns `counts: null`. */
export interface CheckoutTicketRowsPage {
  items: TicketRow[];
  next_cursor?: string;
  counts: null;
  source_errors?: string[];
}

function checkoutPageParams(
  pageSize: number,
  cursor: string | undefined,
  query: CheckoutTicketQuery,
  withCounts: boolean,
): URLSearchParams {
  const params = new URLSearchParams({ page_size: String(pageSize) });
  if (withCounts) params.set('summary_days', completionDayStarts().join(','));
  else params.set('counts', 'false');
  if (cursor) params.set('cursor', cursor);
  for (const [key, value] of Object.entries(query)) {
    if (value === undefined || value === '' || (key === 'text' && typeof value === 'string' && !value.trim())) continue;
    params.set(key, key === 'text' && typeof value === 'string' ? value.trim() : String(value));
  }
  return params;
}
export interface CorruptTicket {
  store: string;
  store_path: string;
  path: string;
  id?: string;
  slug?: string;
  error: string;
  error_code?: 'invalid_ticket' | 'upgrade_required';
}
/** Result of notifying a session after a human note on a Started ticket (HS2-25TP3N). */
export type FeedbackDelivery =
  | { status: 'sent' | 'queued'; note_id: string; connection_id: string }
  | {
      status: 'choose';
      note_id: string;
      /** `worker` is the session's claim worker id, when it has one (HS2-5MCAKY). */
      sessions: Array<{ id: string; tool: string; worker?: string }>;
    }
  | { status: 'start'; note_id: string }
  /** Best-effort delivery failed; the saved note reaches the AI when it next reads the ticket (HS2-H9Z9KZ). */
  | { status: 'not_shared'; note_id: string };

export interface FullTicket extends TicketRow {
  details: string;
  /** Target was created, but its source link needs an idempotent repair. */
  follow_up_link_error?: string;
  blocked_reason?: string;
  notes: Note[];
  /** Derived confidence of a completed/verified ticket: newest scored note of the current cycle. */
  latest_confidence?: number;
  attachments: Attachment[];
  concurrency_token?: string;
  warnings?: string[];
}
/** One AI usage event attributed to a ticket (`docs/14` §14.2, HS2-ZA7W6E). */
export interface TicketUsageEvent {
  ts: string;
  tool: string;
  model?: string;
  tokens_in: number;
  tokens_out: number;
  cost_usd?: number;
  session?: string;
}
/** A ticket's usage history: raw attributed events plus the settled rollup's cost for it. */
export interface TicketUsage {
  events: TicketUsageEvent[];
  settled_cost_usd: number;
  settled_through: string;
}
export interface DuplicateBacklink {
  reference: string;
  project_id: string;
  project_name: string;
  connection_id: string;
  native_id: string;
  qualified_id: string;
  slug: string;
  title: string;
}
export interface DuplicateBacklinkProject {
  project_id: string;
  project_name: string;
}
export interface DuplicateBacklinkResponse {
  backlinks: DuplicateBacklink[];
  inaccessible_projects: DuplicateBacklinkProject[];
}
export type RepositoryFileChange =
  'added' | 'copied' | 'deleted' | 'modified' | 'renamed' | 'type_changed' | 'unmerged' | 'untracked';
export interface RepositoryFile {
  path: string;
  original_path?: string;
  staged?: RepositoryFileChange;
  unstaged?: RepositoryFileChange;
  untracked: boolean;
  conflicted: boolean;
}
export type RepositoryPlatform = 'macos' | 'windows' | 'linux';
export interface RepositoryStatus {
  initialized?: boolean;
  branch?: string;
  upstream?: string;
  ahead: number;
  behind: number;
  staged: number;
  unstaged: number;
  untracked: number;
  conflicted: number;
  clean?: boolean;
  files?: RepositoryFile[];
  root?: string;
  platform?: RepositoryPlatform;
  commit_count?: number;
  commits?: CodeReviewCommit[];
  ranges?: CodeReviewRange[];
  difftool?: string;
  truncated?: boolean;
}
export interface RepositoryPage<T> {
  items: T[];
  next_cursor?: number | null;
}
export type CommitRefKind = 'head' | 'branch' | 'remote' | 'tag';
export interface CommitRef {
  label: string;
  kind: CommitRefKind;
}
export interface CodeReviewCommit {
  sha: string;
  short_sha: string;
  subject: string;
  body?: string;
  committed_at: string;
  refs?: CommitRef[];
  /** Present when the commit is not on this checkout's branch yet (HS2-0WZRK5). */
  unmerged?: UnmergedSource;
}
/** Where an unmerged ticket commit lives: a local branch and/or another worktree. */
export interface UnmergedSource {
  branch?: string;
  worktree?: string;
}
export interface CodeReviewRange {
  from: string;
  to: string;
  count: number;
}
export interface CodeReviewSummary {
  files: { total: number; docs: number; tests: number; source: number; other: number };
  tests_added: number;
  tests_modified: number;
}
export interface CodeReviewFile {
  path: string;
  original_path?: string;
  change: Exclude<RepositoryFileChange, 'unmerged' | 'untracked'>;
  category: 'docs' | 'tests' | 'source' | 'other';
}
/** A project's type (HS2-W27N1D): `software` by default, or an orchestration project. */
export type ProjectType = 'software' | 'orchestration';
/** `GET/PUT /checkouts/{owner}/orchestration/project-type` (HS2-W27N1D). */
export interface ProjectTypeView {
  project_type: ProjectType;
  /** The target declaration exists. */
  declared: boolean;
  /** Active projects in the declaration, whatever the type. */
  active_projects: number;
  changed: boolean;
}
/** One declared target of an orchestration project (docs/26 §26.2). */
export interface OrchestrationTargetDeclaration {
  target_id: string;
  state: 'active' | 'removed';
  label: string;
  expected_repository_identity?: string;
  /** HS2-SHH7WW: this target is another site of the project declared by that target. */
  site_of?: string;
}
/** `GET /checkouts/{owner}/orchestration/targets`. */
export interface OrchestrationManifest {
  enabled: boolean;
  revision: string | null;
  targets: OrchestrationTargetDeclaration[];
}
/** Where a target is bound on this machine: a local checkout or one on a reviewed remote server. */
export type OrchestrationDestination =
  | { kind: 'local'; server_id: string; checkout_id: string; checkout_authority: string }
  | {
      kind: 'remote';
      registration_id: string;
      identity: { server_id: string; ca_fingerprint?: string | null };
      checkout_id: string;
      checkout_authority: string;
    };
/** `GET /checkouts/{owner}/orchestration/targets/{target}/binding`: this machine's cached binding. */
export interface OrchestrationBinding {
  revision: number;
  destination: OrchestrationDestination | null;
  checked_at_ms: number | null;
  confirmed_at_ms: number | null;
  /** A stable failure code (`unreachable`, `revoked`, ...). */
  last_failure: string | null;
  stale: boolean;
}
/** `GET .../targets/{target}/resolve`: a fresh check of the binding. */
export interface OrchestrationResolution {
  declaration: OrchestrationTargetDeclaration;
  binding: OrchestrationBinding;
  state:
    'unbound' | 'connecting' | 'reachable' | 'unreachable' | 'unauthorized' | 'source_missing' | 'identity_changed';
  reason: string | null;
  stale: boolean;
  usable: boolean;
}
/**
 * HS2-E9VVQF: `POST /checkouts/{owner}/orchestration/inspect`: a fresh, machine-local check of a
 * candidate checkout. It binds nothing; its `checkout_authority` names the ticket store it reaches.
 */
export interface OrchestrationInspection {
  destination: OrchestrationDestination;
  stale: boolean;
  usable: boolean;
}
/** `POST /checkouts/{owner}/orchestration/projects` (HS2-W27N1D). */
export interface OrchestrationProjectView {
  target_id: string;
  label: string;
  added: boolean;
  resolution: OrchestrationResolution;
}
/** `GET /checkouts/{owner}/orchestration/hosted-projects` (HS2-92FJXM): one checkout the serving machine has open. */
export interface HostedOrchestrationProjectChoice {
  checkout_id: string;
  name: string;
  added: boolean;
  target_id?: string;
  /** HS2-89YNGN: the ticket store it reaches (non-secret digest), absent when the server could not read it. */
  checkout_authority?: string;
}
/** `DELETE /checkouts/{owner}/orchestration/projects/{target}` (HS2-W27N1D). */
export interface RemovedOrchestrationProject {
  target_id: string;
  unbound: boolean;
  removed: boolean;
}
/** `GET .../targets/{target}/capacity` (HS2-J8PRTV): measurements are null when unknown. */
export interface OrchestrationCapacity {
  sampled_at_ms: number | null;
  memory: { free_mb: number | null; total_mb: number | null };
  cpu: { load_percent: number | null; logical_cpus: number | null };
  disk: { free_mb: number | null; total_mb: number | null };
  tools: { tool: string; installed: boolean; configured: boolean }[] | null;
  sessions: { session_id: string; tool: string | null; busy: boolean }[] | null;
  /** Live claims in the namespaces asked for; null when none were asked or they could not be read. */
  claims: { namespace: string; resource: string; expires_at_ms: number }[] | null;
  claim_namespaces: string[];
  /** HS2-ZY4Z4G: what the machine is; absent from an older host or when it could not tell. */
  capabilities?: OrchestrationCapabilities;
  /** HS2-P5GTSW: changes when the room on the machine changes; absent from an older host. */
  version?: string;
  /** HS2-5H8R0N: the machine owner's own placement limits; absent from an older host, each unset one absent. */
  owner_limits?: OrchestrationOwnerLimits;
}
/** HS2-5H8R0N: limits a machine's owner set for sessions started there (`orchestration admission set`). */
export interface OrchestrationOwnerLimits {
  max_sessions?: number;
  min_free_ram_mb?: number;
  max_cpu_percent?: number;
  min_free_disk_mb?: number;
}
/** HS2-ZY4Z4G: a target machine's OS, architecture, CPU, GPU, AI tool versions and toolchains. */
export interface OrchestrationCapabilities {
  os: 'macos' | 'linux' | 'windows' | 'other';
  os_version?: string;
  os_distribution?: string;
  arch: string;
  cpu_model?: string;
  gpu?: { present: boolean; vendors?: string[] };
  /** HS2-S1FEYY: `signed_in` is absent when the machine could not tell (unknown, never signed out). */
  ai_tools?: { tool: string; version?: string; signed_in?: boolean }[];
  toolchains?: { name: string; installed: boolean; version?: string }[];
}
/**
 * HS2-SHH7WW, HS2-D4G81Q: one site of a project as this machine knows it, from
 * `GET /checkouts/{owner}/orchestration/sites?capabilities=true`.
 */
export interface OrchestrationSiteView {
  target_id: string;
  label: string;
  /** The project's own target, its home site. */
  project: boolean;
  destination: OrchestrationDestination | null;
  confirmed_at_ms: number | null;
  last_failure: string | null;
  stale: boolean;
  /** HS2-D4G81Q: what the site's machine is; absent when it was not read or not reported. */
  capabilities?: OrchestrationCapabilities;
  /** HS2-D4G81Q: the error code of a capabilities read that failed. */
  capabilities_error?: string;
}
/** HS2-SHH7WW: every active project with its active sites, the project's own target first. */
export interface OrchestrationSitesView {
  revision: string | null;
  projects: { project_id: string; label: string; sites: OrchestrationSiteView[] }[];
}
/** `POST .../targets/{target}/admission` (HS2-J8PRTV). */
export interface OrchestrationAdmission {
  outcome: 'accepted' | 'reduced' | 'refused';
  tool: string;
  requested: number;
  admitted: number;
  reason: string | null;
}
/** One row of `GET /checkouts/{owner}/orchestration/tool-capacity` (HS2-AHG7HW). */
export interface OrchestrationToolCapacityRow {
  tool: string;
  account?: string;
  state: 'available' | 'limited' | 'degraded' | 'unknown';
  until_ms?: number;
  verified: boolean;
  max_sessions: number;
  weight: number;
  budget_cents_per_day?: number;
  spent_cents_today?: number;
}
export interface OrchestrationToolCapacity {
  enabled: boolean;
  now_ms: number;
  tools: OrchestrationToolCapacityRow[];
}
/** One orchestrator-owned session (HS2-GDJXGZ), `GET /checkouts/{owner}/orchestration/sessions`. */
export interface OrchestrationSession {
  session_id: string;
  target_id: string;
  role: 'worker' | 'dispatcher' | 'integrator';
  tool: string;
  model?: string;
  run_id?: string;
  state: 'requested' | 'running' | 'stopped' | 'failed' | 'lost';
  owner: 'orchestrator' | 'human';
  paused: boolean;
  /** The host's id: its terminal in that server's drawer. */
  hosted?: { server_id: string; session_id: string; worker_id: string };
  label?: string;
  created_at_ms: number;
  updated_at_ms: number;
}
/** The checked-in throughput policy (HS2-YCJDEB). The settings form edits limits only; every other
 * member round-trips unchanged. */
/** One project's entry in the policy (docs/26 §26.8); members the client does not read round-trip. */
export interface OrchestrationTargetPolicy {
  target_id: string;
  enabled?: boolean;
  /** HS2-N5BDE4: absent or empty is Auto, any tool the policy does not turn off. */
  tools?: { tool: string; models?: string[] }[];
  /** HS2-N5BDE4: absent (or `"auto"`) is Auto, the machine's sessions limit; 0 starts none. */
  max_sessions?: number | 'auto' | null;
  machine?: { max_sessions?: number; min_free_ram_mb?: number; max_cpu_percent?: number; min_free_disk_mb?: number };
  speculation?: boolean;
  [key: string]: unknown;
}
/** HS2-TK9ADC: a role the scheduler may start a session in by itself. */
export type OrchestrationRoleSession = 'dispatcher' | 'integrator';
export interface OrchestrationPolicyBody {
  machine?: { max_sessions?: number; min_free_ram_mb?: number; max_cpu_percent?: number; min_free_disk_mb?: number };
  /** HS2-N3HWP9: an absent (or `"auto"`) `max_sessions` or `weight` is Auto; 0 turns the tool off. */
  tools?: {
    tool: string;
    max_sessions?: number | 'auto' | null;
    weight?: number | 'auto' | null;
    accounts?: unknown[];
  }[];
  targets?: OrchestrationTargetPolicy[];
  wind_down?: { idle_minutes?: number; keep_warm?: number };
  speculation?: {
    allowed?: boolean;
    max_depth?: number;
    risk_weight?: number;
    opt_out_tag?: string;
    /** HS2-NCMATS: redo reopened work by cancelling its run, with permission. */
    redo_reopened?: boolean;
  };
  placement?: {
    automatic?: boolean;
    actor?: string;
    resources?: { target_id: string; resources: { namespace: string; resource: string }[] }[];
    /** HS2-TK9ADC: targets whose dispatcher or integrator the scheduler starts by itself. */
    role_sessions?: { target_id: string; roles: OrchestrationRoleSession[] }[];
    [key: string]: unknown;
  };
  [key: string]: unknown;
}
export interface OrchestrationPolicyView {
  enabled: boolean;
  revision: string | null;
  policy: OrchestrationPolicyBody | null;
}
/** HS2-TZJYKH: the policy after Start or Pause; `saved_default` when no policy was saved before. */
export interface OrchestrationAutomaticSet extends OrchestrationPolicyView {
  saved_default: boolean;
}
/** One target ticket an orchestration ticket refers to (HS2-AZ15GA). */
export interface OrchestrationLink {
  target_id: string;
  connection_id: string;
  /** Absent only while a planned create has not produced its ticket. */
  native_id?: string;
  relation: 'planned' | 'observed' | 'created' | 'validated';
  operation_id?: string;
}
export interface ProjectTicketRef {
  project_id: string;
  connection_id: string;
  native_id: string;
}
export interface FollowUpEvidence {
  target: ProjectTicketRef;
  state: 'resolved' | 'missing' | 'deleted' | 'unavailable' | 'invalid';
  slug?: string;
  title?: string;
  native_url?: string;
}
export interface CodeReview {
  commits: CodeReviewCommit[];
  ranges: CodeReviewRange[];
  /** Recorded shipping commits are on remote main while this checkout is behind (HS2-39D5YS). */
  checkout_behind_main?: boolean;
  difftool?: string;
  truncated: boolean;
  summary?: CodeReviewSummary;
  files?: CodeReviewFile[];
  follow_ups?: FollowUpEvidence[];
}
export type CodeReviewTarget =
  | { mode: 'commit'; commit: string }
  | { mode: 'range'; from: string; to: string }
  | { mode: 'compare'; from: string; to: string }
  /** One or more ticket-changed files; several share one `git difftool -- <paths>` launch (HS2-J7HQ5E, HS2-TRJ9P7). */
  | { mode: 'ticket_file'; paths: string[] }
  | { mode: 'worktree_file'; paths: string[]; area: 'staged' | 'unstaged' };
export interface PermissionRequest {
  id: number;
  project?: string;
  connection: string;
  tool: string;
  action: string;
  agent?: string;
  always_allow_supported?: boolean;
  suggestions?: PermissionSecretSuggestion[];
  /** HS2-XHQ1EH: `orchestrator` for an orchestration project's request. */
  requester?: 'orchestrator';
  orchestrator?: OrchestratorAsk;
}
export interface AiModelDescriptor {
  id: string;
  label: string;
  effort_levels?: string[];
}
/** One plugin-declared permission mode a chat can run under (HS2-NHRDMW). */
export interface AiPermissionMode {
  id: string;
  label: string;
  description?: string;
  /** Present for a mode that removes a safety net; shown before and while it is active. */
  warning?: string;
}
export interface AiToolDescriptor {
  id: string;
  display_name: string;
  models: AiModelDescriptor[];
  default_model?: string;
  default_effort?: string;
  actions?: Array<'change_model' | 'change_effort' | 'change_permission_mode'>;
  permission_modes?: AiPermissionMode[];
  default_permission_mode?: string;
}
/** One provider's default model, effort, and permission mode (HS2-EK24KF, HS2-NHRDMW). */
export interface AiProviderDefaults {
  model?: string;
  effort?: string;
  permission_mode?: string;
}
/**
 * A project's AI defaults: the default provider with its model and effort, plus every provider's
 * own default model and effort keyed by tool id (HS2-EK24KF).
 */
export interface AiToolDefaults {
  tool: string;
  /** Automatic work tries these providers after `tool`, in order (HS2-RWA8V6). */
  fallback_tools?: string[];
  model?: string;
  effort?: string;
  permission_mode?: string;
  providers?: Record<string, AiProviderDefaults>;
}
export interface ToolConnection {
  id: string;
  tool: string;
  project: string;
  source?: string;
  role: 'main' | 'worker' | 'drivespawned';
  busy: boolean;
  actions?: Array<'send_turn' | 'interrupt' | 'close' | 'message'>;
  session_id?: string;
  last_error?: string;
  model?: string;
  effort?: string;
  /** The chat's active permission mode (HS2-NHRDMW). */
  permission_mode?: string;
  /** The running turn's correlation id while busy (HS2-14X4AF). */
  turn_id?: string;
  /** When the running turn began, in Unix milliseconds, while busy (HS2-XV1VCR). */
  turn_started_at_ms?: number;
  /** When the running turn last produced a stream event, while busy. */
  last_event_at_ms?: number;
  /** The longest-running open tool call of the running turn, while busy. */
  in_flight_tool?: InFlightTool;
}
export interface MessageConnectionReceipt {
  delivered: boolean;
  mechanism: string;
  session_id?: string;
  confirmed: boolean;
}
/** A running turn's oldest open tool call and the number of open calls (HS2-XV1VCR). */
export interface InFlightTool {
  name: string;
  started_at_ms: number;
  count: number;
}
export interface ToolSession {
  connection_id: string;
  tool: string;
  project: string;
  session_id: string;
  updated_at_ms: number;
  /**
   * The chat's last permission mode (HS2-Z5QVXW). The server restores it when this session's
   * drive is recreated, so recovery sends no mode of its own.
   */
  permission_mode?: string;
}
export interface TerminalInfo {
  id: string;
  /** Creation kind; older servers omit it and are treated as shell terminals. */
  kind?: 'shell' | 'ai';
  /** The AI tool an `ai` terminal launched (for example `claude`); names its tab (HS2-HZK0NK). */
  tool?: string;
  alive: boolean;
  /** Nonzero AI startup exit status, when known (HS2-JRQG1T). */
  exit_code?: number;
  busy: boolean;
  cwd?: string;
  /** Directory where this terminal was opened; stays fixed as the shell changes directory. */
  launch_cwd?: string;
  link?: string;
  progress?: number;
  /** The saved tab name shared by every client of this server (HS2-89FPV1); absent until renamed. */
  name?: string;
  /** The terminal's AI session stopped on an API error and waits for the user (HS2-HJ4D1H). */
  halt?: TerminalHalt;
  /** An interactive AI question waiting for an answer in this terminal. */
  question?: TerminalQuestion;
  /** An AI session in the terminal started with Hot Sheet's hooks active, so its permission
   * prompts come to Hot Sheet (HS2-EV1XK3); absent when none has reported in. */
  ai_connection?: TerminalAiConnection;
  /** Last trusted hook report for this live terminal, retained after SessionEnd. */
  last_hook_report?: TerminalAiConnection;
  /** A live `hotsheet-mcp` shim launched from this terminal holds a presence lease (HS2-HNXE38).
   * Separate from `ai_connection`: MCP reachability does not prove hooks route permission prompts. */
  mcp_connected?: boolean;
  /** This live terminal's AI session is its project's primary: proactive feedback delivery goes
   * here when no claim names a session (HS2-DB2FQ2). Absent otherwise. */
  feedback_primary?: boolean;
  /** HS2-H3S7YQ: the primary role is held because a person chose this terminal. Absent otherwise. */
  feedback_primary_chosen?: boolean;
  /** HS2-H3S7YQ: this live terminal's AI session accepts pushed messages, so it may be chosen as
   * the project's feedback-primary session. Absent otherwise. */
  feedback_eligible?: boolean;
  /** HS2-6MNMVB: an orchestrator started this terminal's session on this machine. */
  orchestrator?: TerminalOrchestrator;
  /**
   * HS2-G2X7RT: for an orchestrator's live session whose tool reports its session start, whether it has;
   * `false` while it may wait on a first-run question a person answers in this terminal. Absent otherwise.
   */
  tool_ready?: boolean;
}
/** HS2-6MNMVB: the hosted orchestrator session in a terminal, which a person may take over. */
export interface TerminalOrchestrator {
  checkout_id: string;
  operation_id: string;
  /** The orchestration project's label. */
  project: string;
  /** `human` once a person took it over; the orchestrator then leaves it alone. */
  owner: 'orchestrator' | 'human';
  paused: boolean;
}
/** An AI session's `SessionStart` hook report (`POST /terminals/{id}/ai-connection`). */
export interface TerminalAiConnection {
  agent?: string;
  /** RFC 3339 time the session reported in. */
  at: string;
  source?: 'session_start' | 'permission_request';
}
/** Why a terminal's AI session halted, as its hook reported it (`POST /terminals/{id}/halt`). */
export interface TerminalHalt {
  /** The tool's error category, for example Claude Code's `overloaded` or `rate_limit`. */
  error_type: string;
  message: string;
  agent?: string;
  /** RFC 3339 time the halt was reported. */
  at: string;
}
export interface TerminalQuestion {
  question: string;
  tool_use_id: string;
  at: string;
  questions?: Array<{
    question: string;
    header?: string;
    options?: Array<{ label: string; description?: string }>;
    multiSelect?: boolean;
  }>;
  /** `codex` for a question read from a Codex rollout: an in-app answer is queued to the session
   * as user input rather than returned to a waiting hook (HS2-D0QTGB). Absent for Claude. */
  agent?: string;
}
/** `PUT /terminals/{id}/name` response; `name` is absent after a clear. */
export interface TerminalName {
  id: string;
  name?: string;
}
export interface TerminalSettings {
  inherit_global_shell_history: boolean;
}
/** Terminal memory limits (`/terminal-limits`, HS2-Y5DYC0). */
export interface TerminalLimitValues {
  history_lines: number;
  replay_bytes: number;
  warm_terminals: number;
}
/** Machine-wide terminal limits: memory-derived defaults, explicit settings (`null` = default), effective values. */
export interface TerminalLimits {
  physical_memory_bytes: number | null;
  defaults: TerminalLimitValues;
  configured: { history_lines: number | null; replay_bytes: number | null; warm_terminals: number | null };
  effective: TerminalLimitValues;
}
/** Notifier plugins (`/notifiers`, docs/32, HS2-DPBJJA). */
export type NotifierSettingType = 'string' | 'list' | 'number' | 'bool' | 'choice' | 'secret';
export interface NotifierSettingField {
  key: string;
  label: string;
  type: NotifierSettingType;
  choices?: string[];
  required: boolean;
  default?: unknown;
  help?: string;
}
export interface NotifierDescriptor {
  id: string;
  display_name: string;
  description: string;
  capabilities: {
    formats: string[];
    max_title_bytes: number;
    max_body_bytes: number;
    attachments: { max_count: number; max_bytes: number; max_total_bytes: number; images_only: boolean };
    priority: boolean;
    click_url: boolean;
    audio: boolean;
  };
  settings: NotifierSettingField[];
  source: { kind: 'built_in' | 'external'; dir?: string };
}
export interface NotifierList {
  notifiers: NotifierDescriptor[];
  load_errors: string[];
}
export interface NotificationQuietHours {
  start: string;
  end: string;
  /** An IANA zone such as `America/New_York`; wins over `utc_offset` (HS2-9T27WQ). */
  time_zone?: string;
  utc_offset?: string;
  allow_errors?: boolean;
}
/** A configured channel; a secret setting holds only `{ secret: "<key name>" }`. */
export interface NotificationChannel {
  id: string;
  notifier: string;
  label?: string;
  enabled: boolean;
  settings: Record<string, unknown>;
  plugin_digest?: string;
  quiet_hours?: NotificationQuietHours;
  max_per_hour?: number;
}
export interface NotificationRuleFilter {
  projects?: string[];
  tags?: string[];
  statuses?: string[];
  categories?: string[];
  priorities?: string[];
  min_severity?: 'info' | 'success' | 'warning' | 'error';
  text?: string;
  /** Saved views the ticket must be in, any one (HS2-G8NXNC). */
  views?: NotificationViewRef[];
}
export interface NotificationViewRef {
  checkout: string;
  view: string;
}
export interface NotificationRule {
  id: string;
  events: string[];
  channels: string[];
  enabled: boolean;
  filter?: NotificationRuleFilter;
  cooldown_seconds: number;
}
export interface NotificationProjectTiming {
  grace_seconds?: number;
  idle_minutes?: number;
}
export interface NotificationConfig {
  version: number;
  channels: NotificationChannel[];
  rules: NotificationRule[];
  link_base?: string;
  project_activity?: NotificationProjectTiming;
}
export interface NotificationDelivery {
  id: string;
  at: string;
  event: string;
  channel: string;
  notifier: string;
  rule?: string;
  outcome: 'delivered' | 'failed' | 'suppressed';
  attempts: number;
  error?: string;
  provider_id?: string;
  project?: string;
  ticket?: string;
  truncated: boolean;
  omitted_attachments: number;
  /** Held by quiet hours or the hourly limit and reported in a later summary (HS2-CFMSP2). */
  held?: boolean;
}
export interface NotificationChannelRemoval {
  removed: boolean;
  removed_rules: string[];
  deleted_secrets: string[];
}
export interface TrashSettings {
  trash_cleanup_days: number;
}
/** Project Settings › General: group each new ticket with the default AI tool (HS2-FN018Q). */
export interface TicketCreationSettings {
  auto_group_new_tickets: boolean;
}
/** How many Trash tickets a retention of `trash_cleanup_days` would purge right now (HS2-KPD942). */
export interface TrashPurgePreview {
  trash_cleanup_days: number;
  purge_count: number;
}
/** A Trash ticket an external provider kept on Empty Trash (GH-5). */
export interface RetainedTrashTicket {
  qualified_id: string;
  slug: string;
  message: string;
}
/** An external provider connection whose Trash could not be emptied (GH-5). */
export interface UnavailableTrashSource {
  connection_id: string;
  message: string;
}
/** `POST /checkouts/{id}/trash/empty`. Older servers omit `retained` and `unavailable`. */
export interface TrashEmptyResult {
  purged: number;
  tickets: string[];
  retained?: RetainedTrashTicket[];
  unavailable?: UnavailableTrashSource[];
}
export interface TerminalRead extends TerminalInfo {
  scrollback: string;
}
export interface CommandDefinition {
  id: string;
  title: string;
  kind?: 'program' | 'shell' | 'ai';
  program?: string;
  args?: string[];
  cwd?: string;
  group?: string;
  confirmation?: string;
  command?: string;
  prompt?: string;
  tool?: string;
  model?: string;
  effort?: string;
  /** An AI command's run action; absent keeps the legacy queue-and-signal click (HS2-17Y2EC). */
  action?: 'create_ticket' | 'new_ai_session' | 'reuse_or_new_ai_session';
  icon?: string;
  color?: string;
  /** Keep an AI command's sidebar button while it has an active schedule; absent hides it (HS2-MA31ZN). */
  show_when_scheduled?: boolean;
  /** Archive or trash the command's tickets once completed or verified; absent keeps them (HS2-27C161). */
  cleanup?: 'none' | 'archive' | 'delete';
  /** When cleanup was turned on (RFC 3339); the settings writer stamps it (HS2-27C161). */
  cleanup_since?: string;
}
export interface CustomView {
  id: string;
  name: string;
  query: string;
}
export interface CommandOutputLine {
  seq: number;
  stream: string;
  text: string;
}
/** Where a shell command's terminal launch or an AI command's queued ticket went (HS2-PWNPM2). */
export interface CommandLaunch {
  kind: 'shell' | 'ai';
  ticket_id?: string;
  ticket_slug?: string;
  tool?: string;
  terminal_id?: string;
  terminal_label?: string;
}
export interface CommandRun {
  id: string;
  command_id: string;
  state: 'running' | 'completed' | 'failed' | 'cancelled';
  exit_code?: number;
  output: CommandOutputLine[];
  started_at?: string;
  launch?: CommandLaunch;
}
/** An AI command run through its configured action (HS2-17Y2EC). */
export interface AiCommandRun {
  action: NonNullable<CommandDefinition['action']>;
  /** `queued` for Create Ticket Only, `started` once a session claimed the ticket, `failed` otherwise. */
  delivery: 'queued' | 'started' | 'failed';
  reason?: string;
  ticket: FullTicket;
  run: CommandRun;
}
/** The phase report fields the client reads: each phase's median wall time (HS2-ST6RN1). */
export interface PhaseReport {
  phases: { phase: string; wall: { median_ms: number } }[];
}
export interface ActivityEvent {
  id: string;
  ts: string;
  tool: string;
  project?: string;
  ticket?: string;
  session?: string;
  kind: string;
  summary: string;
  detail?: unknown;
  importance: 'low' | 'normal' | 'high';
}
export interface CheckoutActivityEvent {
  store_id: string;
  event: ActivityEvent;
}
export interface ClientFileReference {
  id: string;
  filename: string;
  mime_type: string;
  kind: 'attachment' | 'media';
  url: string;
}
export type ClientTurnEvent =
  | { type: 'output'; content: string; truncated: boolean; files?: ClientFileReference[] }
  | { type: 'permission_asked'; tool: string; summary: string }
  | { type: 'usage'; model?: string; tokens_in: number; tokens_out: number; cost_usd?: number }
  | { type: 'native_activity'; source: string; payload: unknown }
  | { type: 'coalesced'; total: number; kinds: Record<string, number> }
  | { type: 'done'; reason: 'completed' | 'failed' | 'interrupted'; exit_code?: number; message?: string }
  | { type: string; [key: string]: unknown };
export interface TurnStreamEnvelope {
  connection_id: string;
  ticket?: string;
  /** The originating turn (HS2-14X4AF); absent from older servers. */
  turn_id?: string;
  /** When the server observed the event (HS2-XV1VCR); absent from older servers. */
  at_ms?: number;
  /** The in-flight tool after this event; with `at_ms` present, absence means none is open. */
  in_flight_tool?: InFlightTool;
  event: ClientTurnEvent;
}
export interface ChangeEvent {
  cursor?: number;
  store: string;
  kind: string;
  id: string;
  slug: string;
  message?: string;
  activity?: ActivityEvent;
  turn?: TurnStreamEnvelope;
  /** For `kind === 'live_state'`: the store's complete live snapshot (HS2-JTTQMY). */
  live?: LiveStoreState;
}
/**
 * One store's glance word (HS2-JTTQMY, docs/32-notifier-plugins.md §32.8.3): the notifier's
 * project-activity words plus Waiting on you.
 */
export type LiveWord = 'active' | 'stopped' | 'may_be_stuck' | 'stalled' | 'halted' | 'waiting_on_you';
/** A live claim; times are Unix milliseconds and `null` when unknown. */
export interface LiveClaim {
  ticket: string;
  slug: string;
  title: string;
  worker: string;
  phase: string | null;
  claimed_at_ms: number | null;
  lease_expires_at_ms: number;
  eta_at_ms: number | null;
}
/**
 * A stuck cause of a worker holding a live claim (HS2-EGKKA0, docs/32-notifier-plugins.md
 * §32.8.4): a usage-limit pause, or a terminal prompt waiting for the user.
 */
export interface LiveCause {
  kind: 'usage_limit' | 'awaiting_input';
  worker: string;
  /** The Hot Sheet terminal to open; `null` when unknown. */
  terminal: string | null;
  since_ms: number;
  /** When the usage limit resets; `null` when the tool did not state it (unknown). */
  resets_at_ms: number | null;
  /** Which prompt an `awaiting_input` cause waits at; `null` for `usage_limit`. */
  prompt: 'permission' | 'question' | null;
}
/**
 * A CI run's state (HS2-M8HHEA, docs/32-notifier-plugins.md §32.8.5). `unavailable` means no
 * linked checkout has reported one.
 */
export type LiveCiStatus = 'unavailable' | 'queued' | 'in_progress' | 'success' | 'failure' | 'cancelled';
/**
 * The newest CI run a linked checkout reported through `POST /checkouts/{reference}/ci-status`
 * or `hotsheet-cli ci report`; every member but `status` is `null` while `unavailable`.
 */
export interface LiveCi {
  status: LiveCiStatus;
  /** The checkout id that reported the run. */
  checkout: string | null;
  branch: string | null;
  sha: string | null;
  run_url: string | null;
  workflow: string | null;
  started_at_ms: number | null;
  finished_at_ms: number | null;
  reported_at_ms: number | null;
}
/** A hosted store's server-pushed live snapshot; clients derive ages and decaying rates locally. */
/** `GET /stores/{store_id}/live-tokens` (HS2-ZTRF4Q): recorded tokens in a window; `complete` is false
 * when the window starts before the server's retained series. */
export interface LiveTokenTotal {
  store: string;
  since_ms: number;
  until_ms: number;
  tokens: number;
  complete: boolean;
  /** The recorded cost of priced usage in the window, USD (HS2-9V4CGN); absent from an older server. */
  cost_usd?: number;
  /** Tokens in the window recorded without a cost because their model has no price. */
  unpriced_tokens?: number;
}
export interface LiveStoreState {
  store: string;
  root: string;
  name: string;
  checkouts: string[];
  epoch_ms: number;
  revision: number;
  state: LiveWord;
  activity: {
    word: LiveWord;
    active_since_ms: number | null;
    stopped_at_ms: number | null;
    last_progress_ms: number | null;
    idle_deadline_ms: number | null;
    stalled: { slug: string; worker: string }[];
    halted: { worker: string; message: string; at_ms: number }[];
    /** Why held claims are not moving, as the tools report it (HS2-EGKKA0). */
    causes: LiveCause[];
  };
  claims: LiveClaim[];
  workers: { live: number; expected: number };
  waiting: { count: number; tickets: { ticket: string; slug: string; title: string; reason: 'feedback' | 'review' }[] };
  rates: {
    tokens: {
      window_minutes: number;
      minutes: { start_ms: number; tokens: number }[];
      peak_per_minute: number;
      /** Tokens per whole UTC hour for the last 12 hours, the current one last (HS2-TRZGT3); absent from an older server. */
      hours?: { start_ms: number; tokens: number }[];
    };
    tickets_updated: { window_minutes: number; at_ms: number[]; peak_per_hour: number };
  };
  /** Main CI status (HS2-M8HHEA); absent from an older server. */
  ci?: LiveCi;
}
export interface LiveStateList {
  epoch_ms: number;
  stores: LiveStoreState[];
}
/** A store's state on the Stats overnight timeline (HS2-9AN4DN): a live word, or Waiting on CI. */
export type HistoryState = LiveWord | 'waiting_on_ci';
/** One recorded stretch in a single state, clipped to the window; Unix milliseconds. */
export interface LiveHistorySegment {
  state: HistoryState;
  start_ms: number;
  end_ms: number;
}
/** One store's lane of `GET /live-history`. */
export interface LiveHistoryStore {
  store: string;
  name: string;
  checkouts: string[];
  segments: LiveHistorySegment[];
  completions: { slug: string; at_ms: number }[];
}
/** `GET /live-history?since_ms&until_ms` (HS2-9AN4DN). */
export interface LiveHistoryList {
  since_ms: number;
  until_ms: number;
  stores: LiveHistoryStore[];
}
/** A notification in the local server's attention inbox (HS2-ZP869N). */
export interface ClientNotification {
  id: string;
  message: string;
  severity: string;
  checkout: string | null;
  store: string | null;
  ticket: string | null;
  recipient: string | null;
  dedupe_key: string | null;
  created_at_ms: number;
  acknowledged: boolean;
}
export interface PollResponse {
  cursor: number;
  events: ChangeEvent[];
  overflow: boolean;
}
export const turnStreamEvents = (response: PollResponse): TurnStreamEnvelope[] =>
  response.events.flatMap((event) => (event.kind === 'turn_event' && event.turn ? [event.turn] : []));
export class TurnStreamReplayGuard {
  private readonly seen = new Set<string>();
  private readonly order: string[] = [];
  constructor(private readonly limit = 2_048) {}
  events(response: PollResponse): TurnStreamEnvelope[] {
    return response.events.flatMap((event) => {
      if (event.kind !== 'turn_event' || !event.turn) return [];
      if (event.cursor === undefined) return [event.turn];
      const key = `${event.store}\0${event.cursor}`;
      if (this.seen.has(key)) return [];
      this.seen.add(key);
      this.order.push(key);
      while (this.order.length > this.limit) this.seen.delete(this.order.shift()!);
      return [event.turn];
    });
  }
}
export const encodeAttachmentFilename = (filename: string) => encodeURIComponent(filename);
export interface ApiOptions {
  /**
   * Whether this client's requests drive the global server-busy indicator (default true). Silent
   * background revalidation of an already-painted project (a warm project tab switch or a background
   * tab refresh) passes false so it never flashes the busy bars or loading-activity label (HS2-AZZ9TF).
   */
  trackBusy?: boolean;
}
/** Identifies this browser tab's change-stream subscriptions to the server, so it knows which
 * projects some client has open and which it may stop hosting (HS2-ARJ9J1). */
export const CHANGE_STREAM_CLIENT_ID =
  typeof crypto !== 'undefined' && 'randomUUID' in crypto
    ? crypto.randomUUID()
    : `tab-${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;

/**
 * Whether a request is one of the ticket mutations whose server route reads an `actor`
 * (HS2-RD4M29): a ticket update, a close, or a batch update.
 */
export function isActorAwareRequest(method: string, path: string): boolean {
  const pathname = path.split('?')[0];
  if (method === 'PATCH') return /\/tickets\/[^/]+$/u.test(pathname);
  // A ticket create records its actor as the ticket's creator (HS2-9FPHWY).
  if (method === 'POST')
    return (
      pathname.endsWith('/tickets') ||
      /\/tickets\/[^/]+\/close$/u.test(pathname) ||
      pathname.endsWith('/batch') ||
      /\/groups\/(?:rename|ungroup|merge)$/u.test(pathname)
    );
  return false;
}

/**
 * Ticket mutations from this client are a person acting (HS2-XF81CJ): add
 * `actor: {"role":"human"}` to the JSON object bodies of the actor-aware routes so the server records human
 * authorship and applies human (never AI-only) rules. A body that already names its actor,
 * including an explicit `null` for unknown authorship, is left unchanged.
 */
/** The `request_id` member of a refusal body, when it has one. */
function requestIdOf(failure: unknown): string | undefined {
  const id = (failure as { request_id?: unknown } | null | undefined)?.request_id;
  return typeof id === 'string' ? id : undefined;
}

export function withHumanActor(path: string, init: RequestInit): RequestInit {
  const method = (init.method ?? 'GET').toUpperCase();
  if (typeof init.body !== 'string' || !isActorAwareRequest(method, path)) return init;
  let body: unknown;
  try {
    body = JSON.parse(init.body);
  } catch {
    return init;
  }
  if (!body || typeof body !== 'object' || Array.isArray(body) || 'actor' in body) return init;
  return { ...init, body: JSON.stringify({ ...body, actor: { role: 'human' } }) };
}

type HostParsedRequest = <T>(parse: ResponseParser<T>, path: string, requested?: RequestInit) => Promise<T>;

/**
 * HS2-W27N1D: the actor id the app gives the person at this machine for orchestration actions that
 * need an identified actor (a target capacity read, a session stop or takeover). Authority still comes
 * from what the orchestration project allows; this only names who asked.
 */
export const ORCHESTRATION_APP_ACTOR = 'hotsheet-app';

/** HS2-8Z9VHH: an admission refusal's values, from the server's `admission` detail. */
export interface AdmissionRefusalDetail {
  reason: string;
  setting?: string;
  limit?: number;
  observed?: number;
  tool?: string;
}

/** Reads the `admission` object of a failure body; anything malformed is ignored. */
export function parseAdmissionRefusal(value: unknown): AdmissionRefusalDetail | undefined {
  if (!value || typeof value !== 'object') return undefined;
  const record = value as Record<string, unknown>;
  if (typeof record.reason !== 'string' || !record.reason) return undefined;
  const number = (key: string) => (typeof record[key] === 'number' ? record[key] : undefined);
  const text = (key: string) => (typeof record[key] === 'string' ? record[key] : undefined);
  return {
    reason: record.reason,
    setting: text('setting'),
    limit: number('limit'),
    observed: number('observed'),
    tool: text('tool'),
  };
}

/** The longest the hosted capacity long poll holds (the server caps it at eight seconds, HS2-P5GTSW). */
export const MACHINE_CAPACITY_WAIT_MS = 8_000;

export class ApiHttpError extends Error {
  constructor(
    message: string,
    readonly status: number,
    /** The failure body's stable `code`, for callers that branch on the kind of failure (HS2-9S8CV1). */
    readonly code?: string,
    /** HS2-9XJEZC: the orchestrator permission request a `permission_required` refusal waits on. */
    readonly requestId?: string,
    /** HS2-8Z9VHH: a `session_admission_refused` failure's reason, limit and observed value. */
    readonly admission?: AdmissionRefusalDetail,
  ) {
    super(message);
  }
}

export class Api {
  private readonly trackBusy: boolean;
  constructor(
    private origin = '',
    private secret = '',
    options: ApiOptions = {},
  ) {
    this.trackBusy = options.trackBusy ?? true;
  }
  // `trackBusy` defaults to true so ordinary loads and mutations drive the server-busy indicator.
  // Idle long-poll streams (e.g. pollEvents) pass false: they sit pending by design and must not
  // read as the server being busy (HS2-MW1V3M).
  /**
   * Request an endpoint and validate its 2xx body with the endpoint's response parser
   * (HS2-34P6XY, HS2-E0AG5E). Every endpoint goes through a parser; there is no unchecked cast.
   */
  private async parsed<T>(
    parse: ResponseParser<T>,
    path: string,
    requested: RequestInit = {},
    trackRequest = true,
    onResponse?: (response: Response) => void,
  ): Promise<T> {
    return parse(await this.requestBody(path, requested, trackRequest, onResponse));
  }
  /**
   * Requests that launch something on the server's machine reject with an explanation on a remote
   * client instead of acting on a screen nobody is looking at (HS2-F78B1S). A host file path stays
   * available: it can still be pasted into a terminal that runs on that machine. A server refusal for a
   * session without host access (HS2-S8R3PD) reads as the same explanation (HS2-CGXGWY).
   */
  private hostAction(action: 'open' | 'reveal' | 'path' | HostOnlyAction): { parsed: HostParsedRequest } {
    const kind = action === 'open' ? 'open-file' : action === 'reveal' ? 'reveal-file' : action;
    return {
      parsed: async (parse, path, requested) => {
        if (kind === 'path') return this.parsed(parse, path, requested);
        assertHostAction(kind);
        try {
          return await this.parsed(parse, path, requested);
        } catch (reason) {
          if (reason instanceof ApiHttpError && reason.code === HOST_ACCESS_REQUIRED_CODE)
            throw new HostOnlyActionError(kind);
          throw reason;
        }
      },
    };
  }
  private async requestBody(
    path: string,
    requested: RequestInit = {},
    trackRequest = true,
    onResponse?: (response: Response) => void,
  ): Promise<unknown> {
    const init = withHumanActor(path, requested);
    const headers = new Headers(init.headers);
    headers.set('X-Hotsheet-Secret', this.secret);
    if (!(init.body instanceof FormData) && !headers.has('Content-Type'))
      headers.set('Content-Type', 'application/json');
    const trackBusy = trackRequest && this.trackBusy;
    if (trackBusy) beginServerRequest(describeServerRequest(init.method ?? 'GET', path));
    const observed = trackRequest ? observeRequest(init.method ?? 'GET', path) : undefined;
    const dispatchedAt = observed ? performance.now() : undefined;
    let response: Response | undefined;
    let headersAt: number | undefined;
    try {
      response = await fetch(`${this.origin}${path}`, { ...init, headers });
      if (observed) headersAt = performance.now();
      if (!response.ok) {
        const failure = await responseJson(response);
        throw new ApiHttpError(
          errorMessageOf(failure) ?? `${response.status}`,
          response.status,
          errorCodeOf(failure),
          requestIdOf(failure),
          parseAdmissionRefusal((failure as { admission?: unknown } | null | undefined)?.admission),
        );
      }
      onResponse?.(response);
      // The change cursor this read was served at bounds what the stream must replay (HS2-SQKJYG).
      if (isTicketCollectionRead(init.method ?? 'GET', path)) {
        const cursor = responseChangeCursor(response.headers);
        if (cursor !== undefined) ticketReadCursors.note(this.origin, cursor);
      }
      return response.status === 204 ? undefined : await responseJsonBody(response);
    } finally {
      // After the body is read, so the timing covers the whole response (HS2-ZGBQ5Q).
      observed?.(response, {
        ...(dispatchedAt === undefined || headersAt === undefined ? {} : { headers_ms: headersAt - dispatchedAt }),
        ...(headersAt === undefined ? {} : { body_ms: performance.now() - headersAt }),
      });
      if (trackBusy) endServerRequest();
    }
  }
  compatibility = () => this.parsed(parseServerCompatibility, '/compatibility');
  providers = () => this.parsed(parseProviderDescriptor.list, '/providers');
  /** This project's own ticket-source connections; the bridge scopes the path to the checkout (HS2-SM9PM8). */
  connections = () => this.parsed(parseProviderConnection.list, '/provider-connections');
  queueProviderUpdates = (
    connection: string,
    operations: Array<{ operation_id: string; native_id: string; patch: Record<string, unknown> }>,
  ) =>
    this.parsed(parseQueuedProviderResult.list, `/providers/${encodeURIComponent(connection)}/tickets/queued`, {
      method: 'POST',
      body: JSON.stringify({
        operations: operations.map((item) => ({ ...item, patch: prioritiesToWire(item.patch) })),
      }),
    });
  providerOutbox = (connection: string) =>
    this.parsed(parseProviderOutboxOperation.list, `/providers/${encodeURIComponent(connection)}/outbox`);
  retryProviderOutbox = (connection: string, operation: string) =>
    this.parsed(
      parseProviderOutboxOperation,
      `/providers/${encodeURIComponent(connection)}/outbox/${encodeURIComponent(operation)}`,
      {
        method: 'POST',
      },
    );
  discardProviderOutbox = (connection: string, operation: string) =>
    this.parsed(
      parseProviderOutboxOperation,
      `/providers/${encodeURIComponent(connection)}/outbox/${encodeURIComponent(operation)}`,
      {
        method: 'DELETE',
      },
    );
  /** Create a ticket source owned by this project: the record and its checkout link in one request. */
  createConnection = (value: ProviderConnection, makeDefault = false, inferSite = false) =>
    this.parsed(parseProviderConnection, '/provider-connections', {
      method: 'POST',
      body: JSON.stringify({ ...value, make_default: makeDefault, infer_site: inferSite }),
    });
  /** Machine-wide sign-ins with the sources and projects using each (HS2-SM9PM8). */
  accounts = () => this.parsed(parseProviderAccount.list, '/accounts');
  identifyAccount = (id: string) =>
    this.parsed(parseAccountIdentity, `/accounts/${encodeURIComponent(id)}/identity`, { method: 'POST' });
  signOutAccount = (id: string) => this.parsed(ignoreBody, `/accounts/${encodeURIComponent(id)}`, { method: 'DELETE' });
  /** Remove an unlinked source belonging to this account without signing out. */
  removeUnusedAccountSource = (account: string, source: string) =>
    this.parsed(ignoreBody, `/accounts/${encodeURIComponent(account)}/sources/${encodeURIComponent(source)}`, {
      method: 'DELETE',
    });
  accountGithubRepositories = (id: string) =>
    this.parsed(parseGitHubRepositoryAccess, `/accounts/${encodeURIComponent(id)}/github-repositories`);
  updateConnection = (id: string, value: ProviderConnection) =>
    this.parsed(parseProviderConnection, `/provider-connections/${encodeURIComponent(id)}`, {
      method: 'PATCH',
      body: JSON.stringify(value),
    });
  setConnectionDisabled = (id: string, disabled: boolean) =>
    this.parsed(parseProviderConnection, `/provider-connections/${encodeURIComponent(id)}/disabled`, {
      method: 'PUT',
      body: JSON.stringify({ disabled }),
    });
  startGitHubAuth = (web_base = 'https://github.com') =>
    this.parsed(parseGitHubAuthStart, '/github-auth/device', { method: 'POST', body: JSON.stringify({ web_base }) });
  waitGitHubAuth = (session: string) =>
    this.parsed(parseGitHubAuthStatus, `/github-auth/device/${encodeURIComponent(session)}`);
  githubAuthRepositories = (session: string) =>
    this.parsed(parseGitHubRepositoryAccess, `/github-auth/device/${encodeURIComponent(session)}/repositories`);
  cancelGitHubAuth = (session: string) =>
    this.parsed(ignoreBody, `/github-auth/device/${encodeURIComponent(session)}`, { method: 'DELETE' });
  /** Linking a Git source names a host path, so it is a host-only action (HS2-S8R3PD, HS2-CGXGWY). */
  addCheckoutSource = (checkout: string, connection: ProviderConnection, makeDefault = false) =>
    (connection.provider === 'git' ? this.hostAction('git-source-link').parsed : this.parsed.bind(this))(
      parseCheckout,
      `/checkouts/${encodeURIComponent(checkout)}/sources/${encodeURIComponent(connection.id)}`,
      {
        method: 'PUT',
        body: JSON.stringify({ provider: connection.provider, locator: connection.locator, make_default: makeDefault }),
      },
    );
  /**
   * Remove a ticket source from one project. A connection no other project uses is deleted with it;
   * its account stays signed in (HS2-SM9PM8).
   */
  removeCheckoutSource = (checkout: string, connectionId: string) =>
    this.parsed(
      parseSourceDetach,
      `/checkouts/${encodeURIComponent(checkout)}/sources/${encodeURIComponent(connectionId)}`,
      { method: 'DELETE' },
    );
  setCheckoutDefaultSource = (checkout: string, connectionId: string | null) =>
    this.parsed(parseCheckout, `/checkouts/${encodeURIComponent(checkout)}/default-source`, {
      method: 'PUT',
      body: JSON.stringify({ connection_id: connectionId }),
    });
  setCheckoutSourceColor = (checkout: string, connectionId: string, color: string) =>
    this.parsed(
      parseCheckout,
      `/checkouts/${encodeURIComponent(checkout)}/sources/${encodeURIComponent(connectionId)}/color`,
      { method: 'PATCH', body: JSON.stringify({ color }) },
    );
  /** Change the prefix new tickets in a Git source use; existing tickets keep their slugs (HS2-8QJ6W3). */
  setCheckoutSourceTicketPrefix = (checkout: string, connectionId: string, ticketPrefix: string) =>
    this.parsed(
      parseTicketPrefix,
      `/checkouts/${encodeURIComponent(checkout)}/sources/${encodeURIComponent(connectionId)}/ticket-prefix`,
      { method: 'PUT', body: JSON.stringify({ ticket_prefix: ticketPrefix }) },
    );
  relinkCheckoutGitSource = (checkout: string, connectionId: string, path: string, reviewUnverifiedRecovery = false) =>
    this.hostAction('git-source-relink').parsed(
      parseCheckoutRelink,
      `/checkouts/${encodeURIComponent(checkout)}/sources/${encodeURIComponent(connectionId)}/relink`,
      { method: 'PATCH', body: JSON.stringify({ path, review_unverified_recovery: reviewUnverifiedRecovery }) },
    );
  transfer = (kind: 'copy' | 'move', source: Ticket, destination_connection: string) =>
    this.parsed(ignoreBody, `/provider-transfers/${kind}`, {
      method: 'POST',
      body: JSON.stringify({
        source: { connection_id: source.connection_id, native_id: source.native_id },
        destination_connection,
        operation_id: browserRandomId(),
        confirm: kind === 'move',
      }),
    });
  copyAttachment = (
    source: { connection_id: string; native_id: string; attachment_id: string },
    destination: { connection_id: string; native_id: string },
  ) =>
    this.parsed(parseFullTicket, '/provider-attachments/copy', {
      method: 'POST',
      body: JSON.stringify({ source, destination }),
    });
  reportNotWorking = (connection: string, id: string, note: string, files: readonly File[], expectedToken?: string) => {
    const body = new FormData();
    if (note.trim()) body.append('note', note.trim());
    if (expectedToken) body.append('expected_token', expectedToken);
    for (const file of files) body.append('evidence', file, file.name);
    return this.parsed(
      parseFullTicket,
      `/providers/${encodeURIComponent(connection)}/tickets/${encodeURIComponent(id)}/not-working`,
      { method: 'POST', body },
    );
  };
  checkoutTickets = async (checkout: string, query?: string | CheckoutTicketQuery): Promise<CheckoutTicketRows> => {
    const options = typeof query === 'string' ? { text: query } : (query ?? {}),
      params = new URLSearchParams();
    for (const [key, value] of Object.entries(options)) {
      if (value === undefined || value === '' || (key === 'text' && typeof value === 'string' && !value.trim()))
        continue;
      params.set(key, key === 'text' && typeof value === 'string' ? value.trim() : String(value));
    }
    const suffix = params.size ? `?${params}` : '';
    let partial = false,
      truncated = false;
    const items = await this.parsed(
      parseTicketRow.list,
      `/checkouts/${encodeURIComponent(checkout)}/tickets${suffix}`,
      {},
      true,
      (response) => {
        partial = response.headers.get('x-hotsheet-partial') === 'true';
        truncated = response.headers.get('x-hotsheet-truncated') === 'true';
      },
    );
    return { items, partial, truncated };
  };
  /** Authenticated, source-scoped worker snapshot; `after` is a cursor long poll with no idle busy state. */
  checkoutWorkers = (checkout: string, connectionId: string, after?: string, signal?: AbortSignal) => {
    const query = new URLSearchParams({ connection_id: connectionId, wait_ms: after ? '25000' : '0' });
    if (after) query.set('after', after);
    return this.parsed(
      parseWorkerView,
      `/checkouts/${encodeURIComponent(checkout)}/workers?${query}`,
      { signal },
      false,
    );
  };
  /**
   * HS2-J89JC7: the orchestration timeline of an owning checkout. With `after` (a version) it is a genuine
   * long poll that answers when what the timeline shows changes, with no idle busy state.
   */
  orchestrationTimeline = (checkout: string, after?: string, signal?: AbortSignal) => {
    const query = after ? `?${new URLSearchParams({ after, wait_ms: '20000' })}` : '';
    return this.parsed(
      parseOrchestrationTimeline,
      `/checkouts/${encodeURIComponent(checkout)}/orchestration/timeline${query}`,
      { signal },
      false,
    );
  };
  /**
   * HS2-S3FKJ7: from a target project's side, the orchestration line of each of its tickets an orchestration
   * project on this machine plans. With `after` (a version) it is a genuine long poll like the timeline.
   */
  orchestrationTargetLines = (checkout: string, after?: string, signal?: AbortSignal) => {
    const query = after ? `?${new URLSearchParams({ after, wait_ms: '20000' })}` : '';
    return this.parsed(
      parseOrchestrationTargetLines,
      `/checkouts/${encodeURIComponent(checkout)}/orchestration/target-lines${query}`,
      { signal },
      false,
    );
  };
  /**
   * HS2-3D8D6V: the same lines for a checkout on the remote server registered here as `registration`, answered
   * by this machine's server from the orchestration projects that bind it remotely. Never asks the remote server.
   */
  orchestrationRemoteTargetLines = (registration: string, checkout: string, after?: string, signal?: AbortSignal) => {
    const query = after ? `?${new URLSearchParams({ after, wait_ms: '20000' })}` : '';
    return this.parsed(
      parseOrchestrationTargetLines,
      `/remotes/${encodeURIComponent(registration)}/checkouts/${encodeURIComponent(checkout)}/orchestration/target-lines${query}`,
      { signal },
      false,
    );
  };
  /**
   * HS2-HCW7MD: start the orchestration project's planning AI session (Propose Plan) in the tool the person
   * chose, or answer with the live one. The same `operation_id` never launches twice.
   */
  proposeOrchestrationPlan = (
    checkout: string,
    request: { tool: string; model?: string; effort?: string; operation_id: string },
    actorId: string,
  ) =>
    this.parsed(parseOrchestrationRecord, `/checkouts/${encodeURIComponent(checkout)}/orchestration/planning/propose`, {
      method: 'POST',
      body: JSON.stringify({ ...request, actor: { role: 'human', id: actorId } }),
    });
  /**
   * HS2-190RDX: Update Plan. Re-plan in the background without pausing: the server opens a draft beside
   * the live plan and asks the planner to draft it. A second request answers with the drafting update.
   */
  updateOrchestrationPlan = (
    checkout: string,
    request: { tool: string; model?: string; effort?: string; operation_id: string },
    actorId: string,
  ) =>
    this.parsed(parseOrchestrationRecord, `/checkouts/${encodeURIComponent(checkout)}/orchestration/planning/update`, {
      method: 'POST',
      body: JSON.stringify({ ...request, actor: { role: 'human', id: actorId } }),
    });
  /** HS2-190RDX: cancel the drafting plan update; the live plan is unchanged. */
  cancelOrchestrationPlanUpdate = (checkout: string, actorId: string) =>
    this.parsed(
      parseOrchestrationRecord,
      `/checkouts/${encodeURIComponent(checkout)}/orchestration/planning/update_cancel`,
      { method: 'POST', body: JSON.stringify({ actor: { role: 'human', id: actorId } }) },
    );
  /**
   * HS2-A4RVYA: plan templates. `list`, `preview` and `apply_show` read and change nothing; `apply` opens a
   * new goal ticket and hands it to the planner, idempotent by `operation_id`; `save_goal` saves a goal's plan
   * steps as a template. A person's apply is their own act. HS2-36XDDX: `accept` is a person's accept of the
   * plan proposed for an apply's goal, which releases its held steps to placement; HS2-0YHA2S: with `all` in
   * place of `operation_id`, every template plan held now.
   */
  orchestrationTemplates = (
    checkout: string,
    operation: 'list' | 'preview' | 'apply' | 'apply_show' | 'save_goal' | 'validate' | 'set' | 'retire' | 'accept',
    body: Record<string, unknown>,
    actorId: string,
  ) =>
    this.parsed(
      parseOrchestrationRecord,
      `/checkouts/${encodeURIComponent(checkout)}/orchestration/templates/${operation}`,
      { method: 'POST', body: JSON.stringify({ ...body, actor: { role: 'human', id: actorId } }) },
      operation !== 'list' && operation !== 'preview' && operation !== 'apply_show' && operation !== 'validate',
    );
  /**
   * HS2-KXKA7X: this machine's scheduled and event triggers. `list`, `show` and `review` read and change nothing;
   * `set` saves a definition (new triggers start as Dry Run or Paused), `on` is the reviewed turn On, `dry_run`,
   * `pause` and `resume` change its state, `fire` delivers one manual delivery and `remove` forgets it.
   */
  orchestrationTriggers = (
    checkout: string,
    operation: 'list' | 'show' | 'review' | 'set' | 'on' | 'dry_run' | 'pause' | 'resume' | 'fire' | 'remove',
    body: Record<string, unknown>,
    actorId: string,
  ) =>
    this.parsed(
      parseOrchestrationRecord,
      `/checkouts/${encodeURIComponent(checkout)}/orchestration/triggers/${operation}`,
      { method: 'POST', body: JSON.stringify({ ...body, actor: { role: 'human', id: actorId } }) },
      operation !== 'list' && operation !== 'show' && operation !== 'review',
    );
  /** HS2-HCW7MD: where planning stands (`idle`, `planning` or `ended`); starts nothing. */
  orchestrationPlanning = (checkout: string) =>
    this.parsed(
      parseOrchestrationRecord,
      `/checkouts/${encodeURIComponent(checkout)}/orchestration/planning/show`,
      { method: 'POST', body: '{}' },
      false,
    );
  /** A person's run control (HS2-WBREDM, HS2-EBHDSB): pause, resume, handoff, correct or cancel. */
  orchestrationRunControl = (
    checkout: string,
    run: string,
    operation: 'pause' | 'resume' | 'handoff' | 'correct' | 'cancel',
    actorId: string,
    body: Record<string, unknown> = {},
  ) =>
    this.parsed(
      parseOrchestrationRecord,
      `/checkouts/${encodeURIComponent(checkout)}/orchestration/runs/${encodeURIComponent(run)}/${operation}`,
      { method: 'POST', body: JSON.stringify({ ...body, actor: { role: 'human', id: actorId } }) },
    );
  /**
   * HS2-HDHT5E: read one target ticket through the orchestration project's gate (§26.3), for a remote
   * target's label on the timeline. Adding the project was the consent to read it, so this never asks;
   * it runs in the background and never reads as the server being busy.
   */
  orchestrationTicketRead = (
    checkout: string,
    target: { target_id: string; connection_id: string; native_id: string },
    actorId: string,
    signal?: AbortSignal,
  ) =>
    this.parsed(
      parseOrchestrationRecord,
      `/checkouts/${encodeURIComponent(checkout)}/orchestration/tickets/read`,
      { method: 'POST', body: JSON.stringify({ ...target, actor: { role: 'human', id: actorId } }), signal },
      false,
    );
  /** The standing gates of one run (HS2-5JBYPV), from this machine's records. */
  orchestrationRunGates = (checkout: string, run: string) =>
    this.parsed(
      parseOrchestrationRecord,
      `/checkouts/${encodeURIComponent(checkout)}/orchestration/gates?${new URLSearchParams({ run })}`,
      {},
      false,
    );
  /**
   * HS2-A7YCB2: link a run's CI gate (HS2-5JBYPV), as Link Again does for a gate whose watch ended: the
   * same commit and scope return the same gate and watch it again.
   */
  orchestrationGateLink = (checkout: string, run: string, request: Record<string, unknown>, actorId: string) =>
    this.parsed(
      parseOrchestrationRecord,
      `/checkouts/${encodeURIComponent(checkout)}/orchestration/runs/${encodeURIComponent(run)}/gates`,
      { method: 'POST', body: JSON.stringify({ ...request, actor: { role: 'human', id: actorId } }) },
    );
  /** One run record (HS2-EBHDSB) with its last reconciliation (HS2-WBREDM), from this machine's records. */
  orchestrationRun = (checkout: string, run: string) =>
    this.parsed(
      parseOrchestrationRecord,
      `/checkouts/${encodeURIComponent(checkout)}/orchestration/runs/${encodeURIComponent(run)}`,
      {},
      false,
    );
  /**
   * HS2-9XJEZC: one run's events after a cursor, as a genuine long poll: with a cursor the server answers
   * as soon as an event of that run is recorded, or at the 20-second deadline with the same cursor.
   */
  orchestrationRunEventsAfter = (checkout: string, run: string, after: string | undefined, signal?: AbortSignal) =>
    this.parsed(
      parseOrchestrationRecord,
      `/checkouts/${encodeURIComponent(checkout)}/orchestration/run-events?${new URLSearchParams({
        run,
        after: after ?? '0',
        limit: '200',
        wait_ms: after === undefined ? '0' : '20000',
      })}`,
      { signal },
      false,
    );
  /** One run's events (HS2-7QHWHJ), oldest first. */
  orchestrationRunEvents = (checkout: string, run: string) =>
    this.parsed(
      parseOrchestrationRecord,
      `/checkouts/${encodeURIComponent(checkout)}/orchestration/run-events?${new URLSearchParams({ run, after: '0', limit: '200' })}`,
      {},
      false,
    );
  /**
   * HS2-9XJEZC: ask the orchestration project's orchestrator to run one validation step (HS2-WN0MXZ). The
   * same operation id is the same request: after a person answers its permission request, sending it
   * again runs the step once. A step the orchestrator may not run without a person refuses 403
   * `permission_required` with the request id.
   */
  orchestrationValidationRun = (
    checkout: string,
    request: {
      ticket: { connection_id: string; native_id: string };
      step_id: string;
      operation_id: string;
      run_id?: string;
      /** How long the server waits for the attempt to finish, at most 8000 ms. */
      wait_ms?: number;
      /** HS2-ZYM6ZT: the person who asked, recorded in the audit beside the orchestrator. */
      requested_by?: string;
    },
  ) =>
    this.parsed(
      parseOrchestrationRecord,
      `/checkouts/${encodeURIComponent(checkout)}/orchestration/validations/run`,
      {
        method: 'POST',
        body: JSON.stringify({ wait_ms: 0, ...request, actor: { role: 'system', id: `orchestrator.${checkout}` } }),
      },
      false,
    );
  /** HS2-F1BKQ0: the plan's state where every plan read fails; reads only. */
  orchestrationPlanHealth = (checkout: string) =>
    this.parsed(
      parseOrchestrationRecord,
      `/checkouts/${encodeURIComponent(checkout)}/orchestration/readiness/health`,
      { method: 'POST', body: '{}' },
      false,
    );
  /** HS2-F1BKQ0: move this machine's pre-sharing plan definitions in; `discardLocal` is a person's choice. */
  orchestrationPlanMigrate = (checkout: string, discardLocal: boolean, actorId: string) =>
    this.parsed(
      parseOrchestrationRecord,
      `/checkouts/${encodeURIComponent(checkout)}/orchestration/readiness/migrate`,
      {
        method: 'POST',
        body: JSON.stringify({ discard_local: discardLocal, actor: { role: 'human', id: actorId } }),
      },
    );
  /** HS2-0D60T8: one plan step's readiness with its provisional work (HS2-42TARA, HS2-JSF985); reads only. */
  orchestrationReadiness = (checkout: string, step: string) =>
    this.parsed(
      parseOrchestrationRecord,
      `/checkouts/${encodeURIComponent(checkout)}/orchestration/readiness/show`,
      { method: 'POST', body: JSON.stringify({ step_id: step }) },
      false,
    );
  /** HS2-0D60T8: a person reopens provisional work the fix invalidated, with a reason code (HS2-JSF985). */
  orchestrationReopen = (checkout: string, step: string, reason: string, actorId: string) =>
    this.parsed(parseOrchestrationRecord, `/checkouts/${encodeURIComponent(checkout)}/orchestration/readiness/reopen`, {
      method: 'POST',
      body: JSON.stringify({ step_id: step, reason, actor: { role: 'human', id: actorId } }),
    });
  /** HS2-77SRHC: why one plan step is not running, read as this browser's person (observation never asks). */
  orchestrationSchedulerExplain = (checkout: string, step: string, actorId: string) =>
    this.parsed(
      parseOrchestrationRecord,
      `/checkouts/${encodeURIComponent(checkout)}/orchestration/scheduler/explain`,
      { method: 'POST', body: JSON.stringify({ step_id: step, actor: { role: 'human', id: actorId } }) },
      false,
    );
  /** HS2-XJEH08: progress and cost of the project's runs (HS2-S13719); from this machine's records, asks no one. */
  orchestrationSummary = (checkout: string) =>
    this.parsed(
      parseOrchestrationRecord,
      `/checkouts/${encodeURIComponent(checkout)}/orchestration/summary`,
      {},
      false,
    );
  /** HS2-5GRXFN: the schedule a pass would compute now, as this browser's person; a dry run that logs nothing. */
  orchestrationSchedulerPlan = (checkout: string, actorId: string) =>
    this.parsed(
      parseOrchestrationRecord,
      `/checkouts/${encodeURIComponent(checkout)}/orchestration/scheduler/plan`,
      { method: 'POST', body: JSON.stringify({ actor: { role: 'human', id: actorId } }) },
      false,
    );
  /** HS2-77SRHC: the newest scheduler decisions first, before `before` when given (paging back). */
  orchestrationDecisions = (checkout: string, page: { limit: number; before?: number }) =>
    this.parsed(
      parseOrchestrationRecord,
      `/checkouts/${encodeURIComponent(checkout)}/orchestration/scheduler/decisions`,
      { method: 'POST', body: JSON.stringify({ newest: true, ...page }) },
      false,
    );
  /** The validation steps of one orchestration ticket with their latest evidence (HS2-WN0MXZ). */
  orchestrationValidations = (checkout: string, ticket: { connection_id: string; native_id: string }) =>
    this.parsed(
      parseOrchestrationList,
      `/checkouts/${encodeURIComponent(checkout)}/orchestration/validations/list`,
      { method: 'POST', body: JSON.stringify({ ticket }) },
      false,
    );
  developmentStrategy = (checkout: string, overrides: { session?: string; ticket?: string } = {}) => {
    const params = new URLSearchParams();
    if (overrides.session) params.set('session', overrides.session);
    if (overrides.ticket) params.set('ticket', overrides.ticket);
    const suffix = params.size ? `?${params}` : '';
    return this.parsed(
      parseDevelopmentStrategyReport,
      `/checkouts/${encodeURIComponent(checkout)}/development-strategy${suffix}`,
    );
  };
  saveDevelopmentStrategy = (checkout: string, policy: DevelopmentStrategyPolicy) =>
    this.parsed(parseDevelopmentStrategyReport, `/checkouts/${encodeURIComponent(checkout)}/development-strategy`, {
      method: 'PUT',
      body: JSON.stringify(policy),
    });
  checkoutTicketPage = (checkout: string, pageSize = 200, cursor?: string, query: CheckoutTicketQuery = {}) =>
    this.parsed(
      parseCheckoutTicketPage,
      `/checkouts/${encodeURIComponent(checkout)}/tickets?${checkoutPageParams(pageSize, cursor, query, true)}`,
    );
  /** One checkout page without counts (`counts=false`, HS2-VPEAM4), for callers that already hold them. */
  checkoutTicketRowsPage = (checkout: string, pageSize: number, cursor?: string, query: CheckoutTicketQuery = {}) =>
    this.parsed(
      parseCheckoutTicketRowsPage,
      `/checkouts/${encodeURIComponent(checkout)}/tickets?${checkoutPageParams(pageSize, cursor, query, false)}`,
    );
  /**
   * Every matching checkout row, read through bounded cursor pages rather than one unpaged
   * response, which the server caps at 500 rows (HS2-CYXS0N). Pages pass `counts=false`
   * because the walk ignores counts, which would otherwise cost a full provider summary
   * read per page (HS2-VPEAM4).
   */
  checkoutTicketRowsPaged = async (checkout: string, query: CheckoutTicketQuery = {}) => {
    const rows: TicketRow[] = [],
      seen = new Set<string>(),
      sourceErrors = new Set<string>();
    let cursor: string | undefined;
    do {
      const page = await this.checkoutTicketRowsPage(checkout, 500, cursor, query);
      rows.push(...page.items);
      page.source_errors?.forEach((message) => sourceErrors.add(message));
      cursor = page.next_cursor;
      if (cursor !== undefined) {
        if (seen.has(cursor)) throw new Error('Checkout pagination returned a repeated cursor.');
        seen.add(cursor);
      }
    } while (cursor !== undefined);
    return { items: rows, sourceErrors: [...sourceErrors] };
  };
  /** Completion-confidence calibration across the checkout's git stores (HS2-Q1WCCY). */
  checkoutConfidenceReport = (checkout: string) =>
    this.parsed(parseCalibrationReport, `/checkouts/${encodeURIComponent(checkout)}/confidence-report`);
  /** Every hosted store's live snapshot (HS2-JTTQMY); read on stream resync only, never on a timer. */
  liveStates = () => this.parsed(parseLiveStateList, '/live-state');
  /** Recorded per-store state segments and completion ticks since `sinceMs` (HS2-9AN4DN). Read on demand only. */
  liveHistory = (sinceMs: number) =>
    this.parsed(parseLiveHistoryList, `/live-history?since_ms=${String(Math.floor(sinceMs))}`);
  /** One store's recorded tokens between `sinceMs` and now, read once when standby exits (HS2-ZTRF4Q). */
  storeLiveTokens = (store: string, sinceMs: number, untilMs?: number) =>
    this.parsed(
      parseLiveTokenTotal,
      `/stores/${encodeURIComponent(store)}/live-tokens?since_ms=${String(Math.floor(sinceMs))}${
        untilMs === undefined ? '' : `&until_ms=${String(Math.floor(untilMs))}`
      }`,
    );
  /** One hosted store's live snapshot (HS2-JTTQMY). */
  storeLiveState = (store: string) =>
    this.parsed(parseLiveStoreState, `/stores/${encodeURIComponent(store)}/live-state`);
  /** Ticket flow and usage for each linked Git store, with unsupported sources identified. */
  checkoutAnalytics = (checkout: string) =>
    this.parsed(parseCheckoutAnalytics, `/checkouts/${encodeURIComponent(checkout)}/analytics`);
  checkoutActivity = (
    checkout: string,
    options: { limit?: number; minImportance?: 'low' | 'normal' | 'high' } = {},
  ) => {
    const params = new URLSearchParams();
    if (options.limit !== undefined) params.set('limit', String(options.limit));
    if (options.minImportance) params.set('min_importance', options.minImportance);
    return this.parsed(
      parseCheckoutActivity,
      `/checkouts/${encodeURIComponent(checkout)}/activity${params.size ? `?${params}` : ''}`,
    );
  };
  /** Bounded current working-tree diff; explicitly requested and never retained as activity. */
  checkoutCurrentDiff = (checkout: string, path: string) =>
    this.parsed(parseCurrentDiff, `/checkouts/${encodeURIComponent(checkout)}/activity/current-diff`, {
      method: 'POST',
      body: JSON.stringify({ path }),
    });
  notifications = (checkout: string) =>
    this.parsed(parseClientNotifications, `/notifications?checkout=${encodeURIComponent(checkout)}`);
  acknowledgeNotification = (id: string) =>
    this.parsed(parseClientNotification, `/notifications/${encodeURIComponent(id)}/ack`, { method: 'POST' });
  /** Configured server speech adapters; browser speech is a separate local choice (HS2-5PSQJQ). */
  ttsProviders = () => this.parsed(parseStringList, '/tts/providers');
  /** One server speech provider's voices, for the announcer's Voice menu (HS2-CEZVSE). */
  ttsVoices = (provider: string) => this.parsed(parseTtsVoices, `/tts/voices?provider=${encodeURIComponent(provider)}`);
  synthesizeSpeech = async (text: string, provider: string, voice?: string): Promise<Blob> => {
    const headers = new Headers({ 'Content-Type': 'application/json', 'X-Hotsheet-Secret': this.secret });
    const response = await fetch(`${this.origin}/tts/synthesize`, {
      method: 'POST',
      headers,
      body: JSON.stringify({ text, provider, voice }),
    });
    if (!response.ok) {
      const failure = await responseJson(response);
      throw new ApiHttpError(
        errorMessageOf(failure) ?? `${response.status}`,
        response.status,
        errorCodeOf(failure),
        requestIdOf(failure),
      );
    }
    return response.blob();
  };
  /** Git object and attachment sizes with GitHub-limit levels (HS2-QF44T7). */
  checkoutRepositorySize = (checkout: string) =>
    this.parsed(parseRepositorySizeReport, `/checkouts/${encodeURIComponent(checkout)}/repository/size`);
  /** Each linked Git ticket store's durable sync health (HS2-ADHFKZ). */
  checkoutRepositorySync = (checkout: string) =>
    this.parsed(parseTicketStoreSyncReport, `/checkouts/${encodeURIComponent(checkout)}/repository/sync`);

  checkoutRepositoryCompaction = (checkout: string, source: string, minAgeDays = 90, oversizedBytes = 10_000_000) =>
    this.parsed(
      parseCompactionPlan,
      `/checkouts/${encodeURIComponent(checkout)}/repository/compaction?${new URLSearchParams({ source, min_age_days: String(minAgeDays), oversized_bytes: String(oversizedBytes) })}`,
    );
  executeCheckoutRepositoryCompaction = (
    checkout: string,
    source: string,
    plan: CompactionPlan,
    backup: string,
    confirm: string,
  ) =>
    this.parsed(parseCompactionResult, `/checkouts/${encodeURIComponent(checkout)}/repository/compaction`, {
      method: 'POST',
      body: JSON.stringify({
        source,
        head: plan.head,
        branch: plan.branch,
        fingerprint: plan.fingerprint,
        min_age_days: plan.min_age_days,
        oversized_bytes: plan.oversized_bytes,
        backup,
        confirm,
      }),
    });
  checkoutGuidance = (checkout: string) =>
    this.parsed(parseGuidanceOverview, `/checkouts/${encodeURIComponent(checkout)}/guidance`);
  reviewCheckoutGuidanceBatch = (checkout: string, requests: GuidanceBatchSource[]) =>
    this.parsed(parseGuidanceBatchReview, `/checkouts/${encodeURIComponent(checkout)}/guidance/batch-review`, {
      method: 'POST',
      body: JSON.stringify(requests),
    });
  checkoutGuidanceRulePreview = (checkout: string, source: string, rule: string) =>
    this.parsed(
      parseGuidanceRulePreview,
      `/checkouts/${encodeURIComponent(checkout)}/guidance/rules/${encodeURIComponent(source)}/${encodeURIComponent(rule)}/preview`,
    );
  checkoutGuidanceRecommendations = (checkout: string) =>
    this.parsed(
      parseGuidanceRecommendationReport,
      `/checkouts/${encodeURIComponent(checkout)}/guidance/recommendations`,
    );
  checkoutGuidanceInterview = (checkout: string) =>
    this.parsed(
      parseGuidanceInterviewReview,
      `/checkouts/${encodeURIComponent(checkout)}/guidance/recommendations/interview`,
    );
  openCheckoutGuidanceInterview = (checkout: string, fingerprint: string) =>
    this.parsed(
      parseGuidanceInterviewReview,
      `/checkouts/${encodeURIComponent(checkout)}/guidance/recommendations/interview/open`,
      {
        method: 'POST',
        body: JSON.stringify({ fingerprint, consent: true }),
      },
    );
  submitCheckoutGuidanceInterview = (
    checkout: string,
    fingerprint: string,
    answer: GuidanceInterviewReview['answer'],
  ) =>
    this.parsed(
      parseGuidanceInterviewReview,
      `/checkouts/${encodeURIComponent(checkout)}/guidance/recommendations/interview/answer`,
      {
        method: 'POST',
        body: JSON.stringify({ fingerprint, answer }),
      },
    );
  decideCheckoutGuidanceRecommendation = (checkout: string, decision: GuidanceRecommendationDecision) =>
    this.parsed(
      parseGuidanceRecommendationReport,
      `/checkouts/${encodeURIComponent(checkout)}/guidance/recommendations`,
      {
        method: 'POST',
        body: JSON.stringify(decision),
      },
    );
  checkoutGuidanceUpdates = (checkout: string) =>
    this.parsed(parseGuidanceSourceUpdates, `/checkouts/${encodeURIComponent(checkout)}/guidance/updates`);
  checkoutGuidanceReview = (checkout: string, source: string) =>
    this.parsed(
      parseGuidanceSourceReview,
      `/checkouts/${encodeURIComponent(checkout)}/guidance/updates/${encodeURIComponent(source)}/review`,
    );
  checkoutRulesyncUpdates = (checkout: string) =>
    this.parsed(parseRulesyncUpdateReport, `/checkouts/${encodeURIComponent(checkout)}/guidance/rulesync-updates`);
  updateCheckoutGuidance = (checkout: string, action: GuidanceAction) =>
    this.parsed(parseGuidanceOverview, `/checkouts/${encodeURIComponent(checkout)}/guidance`, {
      method: 'POST',
      body: JSON.stringify(action),
    });
  checkoutCorruptTickets = (checkout: string) =>
    this.parsed(parseCorruptTicket.list, `/checkouts/${encodeURIComponent(checkout)}/corrupt-tickets`);
  createCorruptTicketRepair = (checkout: string, path: string) =>
    this.parsed(parseFullTicket, `/checkouts/${encodeURIComponent(checkout)}/corrupt-tickets/repair`, {
      method: 'POST',
      body: JSON.stringify({ path }),
    });
  checkoutTicket = (checkout: string, id: string) =>
    this.parsed(parseStoredTicket, `/checkouts/${encodeURIComponent(checkout)}/tickets/${encodeURIComponent(id)}`).then(
      (ticket) => ({ store: ticket.store, ticket }),
    );
  checkoutTicketNoteDoctor = (checkout: string, id: string, noteId: string) =>
    this.parsed(
      parseNoteDoctorReport,
      `/checkouts/${encodeURIComponent(checkout)}/tickets/${encodeURIComponent(id)}/note-doctor?note=${encodeURIComponent(noteId)}`,
    );
  advisoryClaimStatus = (checkout: string, connection: string, nativeId: string) =>
    this.parsed(parseAdvisoryClaimStatus, this.advisoryClaimPath(checkout, connection, nativeId));
  appendAdvisoryClaimEvent = (checkout: string, connection: string, nativeId: string, event: AdvisoryClaimEventInput) =>
    this.parsed(parseAdvisoryClaimStatus, this.advisoryClaimPath(checkout, connection, nativeId), {
      method: 'POST',
      body: JSON.stringify(event),
    });
  private advisoryClaimPath = (checkout: string, connection: string, nativeId: string) =>
    `/checkouts/${encodeURIComponent(checkout)}/providers/${encodeURIComponent(connection)}/tickets/${encodeURIComponent(nativeId)}/advisory-claim`;
  checkoutTicketDuplicateBacklinks = (checkout: string, id: string) =>
    this.parsed(
      parseDuplicateBacklinks,
      `/checkouts/${encodeURIComponent(checkout)}/tickets/${encodeURIComponent(id)}/duplicate-backlinks`,
    );
  /** Each phase's median wall time over the checkout's completed tickets, for timeline estimates (HS2-ST6RN1). */
  checkoutPhaseReport = (checkout: string) =>
    this.parsed(parsePhaseReport, `/checkouts/${encodeURIComponent(checkout)}/phase-report`);
  checkoutTicketUsage = (checkout: string, id: string) =>
    this.parsed(parseTicketUsage, `/checkouts/${encodeURIComponent(checkout)}/tickets/${encodeURIComponent(id)}/usage`);
  checkoutLegacyPermissionPreview = (checkout: string, source: string) =>
    this.parsed(
      parseLegacyPermissionPreview,
      `/checkouts/${encodeURIComponent(checkout)}/permissions/legacy?source=${encodeURIComponent(source)}`,
    );
  discardCheckoutLegacyPermissions = (checkout: string, source: string) =>
    this.parsed(parseLegacyPermissionDiscardOutcome, `/checkouts/${encodeURIComponent(checkout)}/permissions/legacy`, {
      method: 'DELETE',
      body: JSON.stringify({ source, confirm: true }),
    });
  createCheckoutTicket = (
    checkout: string,
    value: {
      title: string;
      details?: string;
      category: string;
      priority?: string;
      status?: string;
      up_next?: boolean;
      tags?: string[];
      follow_up_to?: string;
    },
    /** Target ticket source (connection id); omitted to use the checkout's default source. */
    source?: string,
  ) =>
    this.parsed(
      parseFullTicket,
      `/checkouts/${encodeURIComponent(checkout)}/tickets${source ? `?source=${encodeURIComponent(source)}` : ''}`,
      {
        method: 'POST',
        body: JSON.stringify(prioritiesToWire(value)),
      },
    );
  linkCheckoutFollowUp = (checkout: string, source: string, target: string) =>
    this.parsed(ignoreBody, `/checkouts/${encodeURIComponent(checkout)}/follow-up-links`, {
      method: 'POST',
      body: JSON.stringify({ source, target }),
    });
  updateCheckoutTicket = (checkout: string, id: string, value: Record<string, unknown>) =>
    this.parsed(parseStoredTicket, `/checkouts/${encodeURIComponent(checkout)}/tickets/${encodeURIComponent(id)}`, {
      method: 'PATCH',
      body: JSON.stringify(
        prioritiesToWire(
          typeof value.note === 'string' && !Object.hasOwn(value, 'actor')
            ? { ...value, actor: { role: 'human' } }
            : value,
        ),
      ),
    }).then((ticket) => ({ store: ticket.store, ticket, feedbackDelivery: ticket.feedback_delivery }));
  notifyTicketNote = (checkout: string, id: string, noteId: string, connectionId: string) =>
    this.parsed(
      parseFeedbackDelivery,
      `/checkouts/${encodeURIComponent(checkout)}/tickets/${encodeURIComponent(id)}/notes/${encodeURIComponent(noteId)}/notify`,
      { method: 'POST', body: JSON.stringify({ connection_id: connectionId }) },
    );
  closeCheckoutTicket = (
    checkout: string,
    id: string,
    reason: TicketCloseReason,
    duplicateOf?: string | { project_id: string; connection_id: string; native_id: string },
  ) =>
    this.parsed(
      parseStoredTicket,
      `/checkouts/${encodeURIComponent(checkout)}/tickets/${encodeURIComponent(id)}/close`,
      { method: 'POST', body: JSON.stringify({ reason, ...(duplicateOf ? { duplicate_of: duplicateOf } : {}) }) },
    ).then((ticket) => ({ store: ticket.store, ticket }));
  /** Return a Trash ticket to the status it held before deletion (git provider only). */
  restoreCheckoutTicket = (checkout: string, id: string) =>
    this.parsed(
      parseStoredTicket,
      `/checkouts/${encodeURIComponent(checkout)}/tickets/${encodeURIComponent(id)}/restore`,
      { method: 'POST' },
    ).then((ticket) => ({ store: ticket.store, ticket }));
  /** Permanently remove every Trash ticket from the checkout's git-backed stores. */
  emptyCheckoutTrash = (checkout: string) =>
    this.parsed(parseTrashPurge, `/checkouts/${encodeURIComponent(checkout)}/trash/empty`, {
      method: 'POST',
    });
  batchUpdateCheckoutTickets = (checkout: string, updates: Array<{ id: string; patch: Record<string, unknown> }>) =>
    this.parsed(parseStoredTicket.list, `/checkouts/${encodeURIComponent(checkout)}/batch`, {
      method: 'POST',
      body: JSON.stringify({ updates: updates.map(({ id, patch }) => ({ id, ...prioritiesToWire(patch) })) }),
    });
  /** Every group in the project with its display name and member count (HS2-PDSHNN, HS2-9KZSD4). */
  // HS2-9KZSD4: a background read of registry names that follows project switches and ticket
  // changes, so it never drives the server-busy indicator (HS2-AZZ9TF).
  checkoutGroups = (checkout: string) =>
    this.parsed(parseGroupList, `/checkouts/${encodeURIComponent(checkout)}/groups`, {}, false);
  /** Every registry group, empty ones included, for Project Settings › Ticket groups (HS2-GXKX1Q). */
  allCheckoutGroups = (checkout: string) =>
    this.parsed(parseGroupList, `/checkouts/${encodeURIComponent(checkout)}/groups?all=true`, {}, false);
  /** Unassign every member of a group, then delete its registry entry (HS2-GXKX1Q). */
  deleteCheckoutGroup = (checkout: string, group: string) =>
    this.parsed(parseGroupDelete, `/checkouts/${encodeURIComponent(checkout)}/groups/delete`, {
      method: 'POST',
      body: JSON.stringify({ group }),
    });
  /** The registry id for a group name, created when no group has that name yet (HS2-9KZSD4). */
  createCheckoutGroup = (checkout: string, name: string) =>
    this.parsed(parseGroupCreate, `/checkouts/${encodeURIComponent(checkout)}/groups`, {
      method: 'POST',
      body: JSON.stringify({ name }),
    });
  /** Rename a group by its stored value (HS2-8CDFKR, HS2-9KZSD4). */
  renameCheckoutGroup = (checkout: string, group: string, to: string) =>
    this.parsed(parseGroupRename, `/checkouts/${encodeURIComponent(checkout)}/groups/rename`, {
      method: 'POST',
      body: JSON.stringify({ group, to }),
    });
  /** Clear a group from every member in one atomic write (HS2-8CDFKR). */
  ungroupCheckoutGroup = (checkout: string, group: string) =>
    this.parsed(parseGroupUngroup, `/checkouts/${encodeURIComponent(checkout)}/groups/ungroup`, {
      method: 'POST',
      body: JSON.stringify({ group }),
    });
  /** Move every member of `from` into `into`, as when merging duplicate groups (HS2-9KZSD4). */
  mergeCheckoutGroups = (checkout: string, from: string, into: string, removeSource = false) =>
    this.parsed(parseGroupRename, `/checkouts/${encodeURIComponent(checkout)}/groups/merge`, {
      method: 'POST',
      // Merge into… also deletes the source group's registry entry (HS2-GXKX1Q).
      body: JSON.stringify(removeSource ? { from, into, remove_source: true } : { from, into }),
    });
  deleteCheckoutNote = (checkout: string, id: string, noteId: string) =>
    this.parsed(
      parseStoredTicket,
      `/checkouts/${encodeURIComponent(checkout)}/tickets/${encodeURIComponent(id)}/notes/${encodeURIComponent(noteId)}`,
      { method: 'DELETE' },
    ).then((ticket) => ({ store: ticket.store, ticket }));
  addCheckoutAttachment = (checkout: string, id: string, file: File, metadata: AttachmentMetadata = {}) => {
    const headers: Record<string, string> = {
      'Content-Type': file.type || 'application/octet-stream',
      'X-Hotsheet-Filename': encodeAttachmentFilename(file.name),
      'X-Hotsheet-Filename-Encoding': 'percent',
      'X-Hotsheet-Metadata-Encoding': 'percent',
    };
    if (metadata.batch_id) headers['X-Hotsheet-Attachment-Batch'] = encodeURIComponent(metadata.batch_id);
    if (metadata.batch_label) headers['X-Hotsheet-Attachment-Batch-Label'] = encodeURIComponent(metadata.batch_label);
    if (metadata.actor?.role) headers['X-Hotsheet-Actor-Role'] = metadata.actor.role;
    if (metadata.actor?.identity) headers['X-Hotsheet-Actor-Identity'] = encodeURIComponent(metadata.actor.identity);
    if (metadata.actor?.display_name)
      headers['X-Hotsheet-Actor-Name'] = encodeURIComponent(metadata.actor.display_name);
    if (metadata.purpose) headers['X-Hotsheet-Attachment-Purpose'] = metadata.purpose;
    return this.parsed(
      parseStoredTicket,
      `/checkouts/${encodeURIComponent(checkout)}/tickets/${encodeURIComponent(id)}/attachments`,
      { method: 'POST', headers, body: file },
    ).then((ticket) => ({ store: ticket.store, ticket }));
  };
  updateCheckoutAttachmentMetadata = (
    checkout: string,
    id: string,
    attachment_ids: string[],
    metadata: AttachmentMetadata,
  ) =>
    this.parsed(
      parseStoredTicket,
      `/checkouts/${encodeURIComponent(checkout)}/tickets/${encodeURIComponent(id)}/attachments`,
      { method: 'PATCH', body: JSON.stringify({ attachment_ids, ...metadata }) },
    ).then((ticket) => ({ store: ticket.store, ticket }));
  renameCheckoutAttachment = (checkout: string, id: string, attachmentId: string, filename: string) =>
    this.parsed(parseStoredTicket, this.checkoutAttachmentUrl(checkout, id, attachmentId).slice(this.origin.length), {
      method: 'PATCH',
      body: JSON.stringify({ filename }),
    }).then((ticket) => ({ store: ticket.store, ticket }));
  checkoutAttachmentUrl = (checkout: string, id: string, attachmentId: string) =>
    `${this.origin}/checkouts/${encodeURIComponent(checkout)}/tickets/${encodeURIComponent(id)}/attachments/${encodeURIComponent(attachmentId)}`;
  checkoutAttachmentThumbnailUrl = (checkout: string, id: string, attachmentId: string) =>
    `${this.checkoutAttachmentUrl(checkout, id, attachmentId)}/thumbnail`;
  checkoutAttachmentByNameUrl = (checkout: string, id: string, filename: string) =>
    `${this.origin}/checkouts/${encodeURIComponent(checkout)}/tickets/${encodeURIComponent(id)}/attachments/by-name/${encodeURIComponent(filename)}`;
  checkoutAttachmentAction = (checkout: string, id: string, attachmentId: string, action: 'open' | 'reveal' | 'path') =>
    this.hostAction(action).parsed(
      parseFilePath,
      `/checkouts/${encodeURIComponent(checkout)}/tickets/${encodeURIComponent(id)}/attachments/${encodeURIComponent(attachmentId)}/action`,
      { method: 'POST', body: JSON.stringify({ action }) },
    );
  checkoutAttachmentByNameAction = (
    checkout: string,
    id: string,
    filename: string,
    action: 'open' | 'reveal' | 'path',
  ) =>
    this.hostAction(action).parsed(
      parseFilePath,
      `/checkouts/${encodeURIComponent(checkout)}/tickets/${encodeURIComponent(id)}/attachments/by-name/${encodeURIComponent(filename)}/action`,
      { method: 'POST', body: JSON.stringify({ action }) },
    );
  deleteCheckoutAttachment = (checkout: string, id: string, attachmentId: string) =>
    this.parsed(parseStoredTicket, this.checkoutAttachmentUrl(checkout, id, attachmentId).slice(this.origin.length), {
      method: 'DELETE',
    }).then((ticket) => ({ store: ticket.store, ticket }));
  updateCheckoutAttachmentAnnotations = (
    checkout: string,
    id: string,
    attachmentId: string,
    annotations: MediaAnnotation[],
  ) =>
    this.parsed(parseStoredTicket, this.checkoutAttachmentUrl(checkout, id, attachmentId).slice(this.origin.length), {
      method: 'PUT',
      body: JSON.stringify({ annotations }),
    }).then((ticket) => ({ store: ticket.store, ticket }));
  updateCheckoutAttachmentMarkup = (
    checkout: string,
    id: string,
    attachmentId: string,
    annotations: MediaAnnotation[],
    crop: Attachment['crop'],
    expectedRevision?: string,
  ) =>
    this.parsed(
      parseStoredTicket,
      `${this.checkoutAttachmentUrl(checkout, id, attachmentId).slice(this.origin.length)}/markup`,
      { method: 'PUT', body: JSON.stringify({ annotations, crop: crop ?? null, expected_revision: expectedRevision }) },
    ).then((ticket) => ({ store: ticket.store, ticket }));
  checkoutAttachmentOriginalUrl = (checkout: string, id: string, attachmentId: string) =>
    `${this.checkoutAttachmentUrl(checkout, id, attachmentId)}/original`;
  repositoryStatus = (checkout: string) =>
    this.parsed(parseRepositoryStatus, `/checkouts/${encodeURIComponent(checkout)}/repository/status`);
  initializeRepository = (checkout: string) =>
    this.parsed(parseRepositoryStatus, `/checkouts/${encodeURIComponent(checkout)}/repository/init`, {
      method: 'POST',
    });
  configureRepositoryRemote = (checkout: string, remote: string) =>
    this.parsed(parseRepositoryStatus, `/checkouts/${encodeURIComponent(checkout)}/repository/remote`, {
      method: 'POST',
      body: JSON.stringify({ remote }),
    });
  repositoryFiles = (
    checkout: string,
    view: 'staged' | 'unstaged' | 'untracked' | 'conflicted',
    cursor = 0,
    limit = 50,
  ) =>
    this.parsed(
      parseRepositoryFilePage,
      `/checkouts/${encodeURIComponent(checkout)}/repository/files?view=${view}&cursor=${cursor}&limit=${limit}`,
    );
  repositoryCommits = (checkout: string, cursor = 0, limit = 50) =>
    this.parsed(
      parseRepositoryCommitPage,
      `/checkouts/${encodeURIComponent(checkout)}/repository/commits?cursor=${cursor}&limit=${limit}`,
    );
  repositoryFileAction = (checkout: string, path: string, action: 'open' | 'reveal') =>
    this.hostAction(action).parsed(ignoreBody, `/checkouts/${encodeURIComponent(checkout)}/repository/files/action`, {
      method: 'POST',
      body: JSON.stringify({ path, action }),
    });
  openRepositoryReview = (checkout: string, target: CodeReviewTarget) =>
    this.hostAction('diff-tool').parsed(ignoreBody, `/checkouts/${encodeURIComponent(checkout)}/repository/review`, {
      method: 'POST',
      body: JSON.stringify(target),
    });
  codeReview = (checkout: string, id: string) =>
    this.parsed(
      parseCodeReview,
      `/checkouts/${encodeURIComponent(checkout)}/tickets/${encodeURIComponent(id)}/code-review`,
    );
  openCodeReview = (checkout: string, id: string, target: CodeReviewTarget) =>
    this.hostAction('diff-tool').parsed(
      ignoreBody,
      `/checkouts/${encodeURIComponent(checkout)}/tickets/${encodeURIComponent(id)}/code-review`,
      {
        method: 'POST',
        body: JSON.stringify(target),
      },
    );
  permissions = () => this.parsed(parsePermissionRequest.list, '/permissions');
  permissionAutomation = () => this.parsed(parsePermissionAutomationSettings, '/permission-automation');
  savePermissionAutomation = (value: PermissionAutomation) =>
    this.parsed(parsePermissionAutomationSettings, '/permission-automation', {
      method: 'PUT',
      body: JSON.stringify(value),
    });
  migratePermissionAutomation = (value: PermissionAutomation) =>
    this.parsed(parsePermissionAutomationSettings, '/permission-automation/migrate', {
      method: 'POST',
      body: JSON.stringify(value),
    });
  /** `onResponse` sees the response before its body is parsed (its `Date` samples the server clock). */
  activeToolConnections = (onResponse?: (response: Response) => void) =>
    this.parsed(parseToolConnection.list, '/connections', {}, true, onResponse);
  messageToolConnection = (id: string, text: string) =>
    this.parsed(parseMessageConnectionReceipt, `/connections/${encodeURIComponent(id)}/message`, {
      method: 'POST',
      body: JSON.stringify({ text }),
    });
  aiTools = (refresh = false) => this.parsed(parseAiToolDescriptor.list, `/ai-tools${refresh ? '?refresh=true' : ''}`);
  aiSettings = () => this.parsed(parseAiToolDefaults, '/ai-settings');
  saveAiSettings = (value: AiToolDefaults) =>
    this.parsed(parseAiToolDefaults, '/ai-settings', { method: 'PUT', body: JSON.stringify(value) });
  toolSessions = () => this.parsed(parseToolSession.list, '/drive/sessions');
  createToolConnection = (
    value: {
      tool: string;
      checkout: string;
      source?: string;
      connection_id?: string;
      session_id?: string;
      model?: string;
      effort?: string;
      permission_mode?: string;
    },
    signal?: AbortSignal,
  ) => this.parsed(parseToolConnection, '/drive/connections', { method: 'POST', body: JSON.stringify(value), signal });
  sendToolTurn = (
    id: string,
    content: string,
    session_id?: string,
    selection?: { model?: string; effort?: string; permission_mode?: string },
    turn_id?: string,
  ) =>
    this.parsed(parseToolConnection, `/drive/connections/${encodeURIComponent(id)}/turns`, {
      method: 'POST',
      body: JSON.stringify({
        content,
        ...(session_id ? { session_id } : {}),
        ...selection,
        ...(turn_id ? { turn_id } : {}),
      }),
    });
  interruptToolTurn = (id: string) =>
    this.parsed(parseToolConnection, `/drive/connections/${encodeURIComponent(id)}/interrupt`, { method: 'POST' });
  deleteToolConnection = (checkout: string, id: string) =>
    this.parsed(ignoreBody, `/checkouts/${encodeURIComponent(checkout)}/drive/connections/${encodeURIComponent(id)}`, {
      method: 'DELETE',
    });
  terminals = (signal?: AbortSignal) => this.parsed(parseTerminalInfo.list, '/terminals', { signal });
  terminalSettings = () => this.parsed(parseTerminalSettings, '/terminal-settings');
  saveTerminalSettings = (value: TerminalSettings) =>
    this.parsed(parseTerminalSettings, '/terminal-settings', { method: 'PUT', body: JSON.stringify(value) });
  terminalLimits = () => this.parsed(parseTerminalLimits, '/terminal-limits');
  saveTerminalLimits = (value: TerminalLimits['configured']) =>
    this.parsed(parseTerminalLimits, '/terminal-limits', { method: 'PUT', body: JSON.stringify(value) });
  // The server host's health and its machine-wide thresholds (HS2-DTAVQA).
  hostHealth = () => this.parsed(parseHostHealth, '/host-health');
  /** The live CPU long poll (HS2-CPFBWP): answers when the server has a frame newer than `after`. */
  hostCpu = (after: number | undefined, signal: AbortSignal) =>
    this.parsed(
      parseHostCpuFrame,
      `/host-health/cpu?timeout_ms=10000${after === undefined ? '' : `&after=${after}`}`,
      { signal },
      false,
    );
  hostHealthThresholds = () => this.parsed(parseHostHealthThresholds, '/host-health/thresholds');
  /** Include or ignore one disk in host health (HS2-NHK8V8); answers with the updated snapshot. */
  setHostHealthDisk = (id: string, monitor: boolean) =>
    this.parsed(parseHostHealth, '/host-health/disks', { method: 'PUT', body: JSON.stringify({ id, monitor }) });
  /** Monitor the volume holding a path on the server's machine (HS2-KBVZV1). */
  addHostHealthDisk = (path: string) =>
    this.parsed(parseHostHealth, '/host-health/disks', { method: 'POST', body: JSON.stringify({ path }) });
  /** Forget a saved disk choice (HS2-KBVZV1). */
  forgetHostHealthDisk = (id: string) =>
    this.parsed(parseHostHealth, `/host-health/disks?id=${encodeURIComponent(id)}`, { method: 'DELETE' });
  saveHostHealthThresholds = (value: HostHealthConfiguredThresholds) =>
    this.parsed(parseHostHealthThresholds, '/host-health/thresholds', { method: 'PUT', body: JSON.stringify(value) });
  /** Machine-wide standby settings (HS2-EQVZTZ). */
  standbySettings = () => this.parsed(parseStandbySettings, '/standby-settings');
  saveStandbySettings = (value: StandbySettingsView['configured']) =>
    this.parsed(parseStandbySettings, '/standby-settings', { method: 'PUT', body: JSON.stringify(value) });
  /** Machine-wide feature-area choices (HS2-3GJZGG). */
  featureSettings = () => this.parsed(parseFeatureSettings, '/feature-settings');
  /** Merge feature choices; `null` clears one to its default. */
  saveFeatureSettings = (change: FeatureSettingsChange) =>
    this.parsed(parseFeatureSettings, '/feature-settings', { method: 'PUT', body: JSON.stringify(change) });
  // Notifier plugins (HS2-DPBJJA): machine-wide channels, rules, and delivery log.
  notifiers = () => this.parsed(parseNotifierList, '/notifiers');
  notificationConfig = () => this.parsed(parseNotificationConfig, '/notification-config');
  saveNotificationChannel = (id: string, body: Omit<NotificationChannel, 'id' | 'plugin_digest'>) =>
    this.parsed(parseNotificationChannel, `/notification-channels/${encodeURIComponent(id)}`, {
      method: 'PUT',
      body: JSON.stringify(body),
    });
  removeNotificationChannel = (id: string) =>
    this.parsed(parseNotificationChannelRemoval, `/notification-channels/${encodeURIComponent(id)}`, {
      method: 'DELETE',
    });
  saveNotificationChannelSecret = (id: string, field: string, value: string) =>
    this.parsed(ignoreBody, `/notification-channels/${encodeURIComponent(id)}/secrets/${encodeURIComponent(field)}`, {
      method: 'PUT',
      body: JSON.stringify({ value }),
    });
  testNotificationChannel = (id: string) =>
    this.parsed(parseNotificationDelivery, `/notification-channels/${encodeURIComponent(id)}/test`, {
      method: 'POST',
    });
  trustNotificationChannel = (id: string) =>
    this.parsed(parseNotificationChannel, `/notification-channels/${encodeURIComponent(id)}/trust`, {
      method: 'POST',
    });
  saveNotificationRule = (id: string, rule: Omit<NotificationRule, 'id'>) =>
    this.parsed(parseNotificationRule, `/notification-rules/${encodeURIComponent(id)}`, {
      method: 'PUT',
      body: JSON.stringify(rule),
    });
  removeNotificationRule = (id: string) =>
    this.parsed(ignoreBody, `/notification-rules/${encodeURIComponent(id)}`, { method: 'DELETE' });
  notificationDeliveries = (limit = 20) =>
    this.parsed(parseNotificationDelivery.list, `/notification-deliveries?limit=${limit}`);
  saveNotificationLinkBase = (url: string | null) =>
    this.parsed(ignoreBody, '/notification-link-base', { method: 'PUT', body: JSON.stringify({ url }) });
  saveNotificationProjectTiming = (timing: NotificationProjectTiming) =>
    this.parsed(parseNotificationProjectTiming, '/notification-project-timing', {
      method: 'PUT',
      body: JSON.stringify(timing),
    });
  /** HS2-W27N1D: Project Settings → General → Project Type and Settings → Orchestration. Every write
   * names a person as its actor; adding a project is that person's consent. */
  projectType = (owner: string) =>
    this.parsed(parseProjectTypeView, `/checkouts/${encodeURIComponent(owner)}/orchestration/project-type`);
  setProjectType = (owner: string, projectType: ProjectType, keepProjects = false) =>
    this.parsed(parseProjectTypeView, `/checkouts/${encodeURIComponent(owner)}/orchestration/project-type`, {
      method: 'PUT',
      body: JSON.stringify({ project_type: projectType, keep_projects: keepProjects, actor: { role: 'human' } }),
    });
  orchestrationTargets = (owner: string) =>
    this.parsed(parseOrchestrationManifest, `/checkouts/${encodeURIComponent(owner)}/orchestration/targets`);
  orchestrationBinding = (owner: string, target: string) =>
    this.parsed(
      parseOrchestrationBinding,
      `/checkouts/${encodeURIComponent(owner)}/orchestration/targets/${encodeURIComponent(target)}/binding`,
    );
  orchestrationResolve = (owner: string, target: string) =>
    this.parsed(
      parseOrchestrationResolution,
      `/checkouts/${encodeURIComponent(owner)}/orchestration/targets/${encodeURIComponent(target)}/resolve`,
    );
  addOrchestrationProject = (
    owner: string,
    request: {
      destination:
        { kind: 'local'; checkout_id: string } | { kind: 'remote'; registration_id: string; checkout_id: string };
      label: string;
      target_id: string;
      /** HS2-E9VVQF: add it as another site of this added project target. */
      site_of?: string;
    },
  ) =>
    this.parsed(parseOrchestrationProjectView, `/checkouts/${encodeURIComponent(owner)}/orchestration/projects`, {
      method: 'POST',
      body: JSON.stringify({ ...request, actor: { role: 'human' } }),
    });
  /** HS2-E9VVQF: which ticket store an open project reaches, before it is added. Binds nothing. */
  inspectOrchestrationTarget = (
    owner: string,
    destination:
      { kind: 'local'; checkout_id: string } | { kind: 'remote'; registration_id: string; checkout_id: string },
  ) =>
    this.parsed(parseOrchestrationInspection, `/checkouts/${encodeURIComponent(owner)}/orchestration/inspect`, {
      method: 'POST',
      body: JSON.stringify({ destination }),
    });
  /** HS2-E9VVQF: Separate from <project>: the site becomes a project of its own again. */
  separateOrchestrationSite = (owner: string, target: string, expectedRevision: string, label: string) =>
    this.parsed(
      parseOrchestrationManifest,
      `/checkouts/${encodeURIComponent(owner)}/orchestration/targets/${encodeURIComponent(target)}`,
      {
        method: 'PATCH',
        body: JSON.stringify({ expected_revision: expectedRevision, label, clear_site_of: true }),
      },
    );
  /** HS2-NAA3HV: make a site the home of its project; the old home and the other sites become its sites. */
  makeOrchestrationHomeSite = (owner: string, target: string, expectedRevision: string) =>
    this.parsed(
      parseOrchestrationManifest,
      `/checkouts/${encodeURIComponent(owner)}/orchestration/targets/${encodeURIComponent(target)}/make-home`,
      {
        method: 'POST',
        body: JSON.stringify({ expected_revision: expectedRevision, actor: { role: 'human' } }),
      },
    );
  /** HS2-92FJXM: the serving machine's open checkouts, which a client on another machine may add. */
  hostedOrchestrationProjects = (owner: string) =>
    this.parsed(
      parseHostedOrchestrationProjectChoice.list,
      `/checkouts/${encodeURIComponent(owner)}/orchestration/hosted-projects`,
    );
  /** HS2-92FJXM: add one of them; the serving machine binds it, and a repeat is the same project. */
  addHostedOrchestrationProject = (
    owner: string,
    // HS2-89YNGN: `site_of` adds it as another site of that project.
    request: { checkout_id: string; label: string; target_id: string; site_of?: string },
  ) =>
    this.parsed(
      parseOrchestrationProjectView,
      `/checkouts/${encodeURIComponent(owner)}/orchestration/hosted-projects`,
      { method: 'POST', body: JSON.stringify({ ...request, actor: { role: 'human' } }) },
    );
  /** HS2-0G80WD: stop orchestrating one project from another machine; a person only. */
  removeHostedOrchestrationProject = (owner: string, target: string) =>
    this.parsed(
      parseRemovedOrchestrationProject,
      `/checkouts/${encodeURIComponent(owner)}/orchestration/hosted-projects/${encodeURIComponent(target)}`,
      { method: 'DELETE', body: JSON.stringify({ actor: { role: 'human' } }) },
    );
  removeOrchestrationProject = (owner: string, target: string) =>
    this.parsed(
      parseRemovedOrchestrationProject,
      `/checkouts/${encodeURIComponent(owner)}/orchestration/projects/${encodeURIComponent(target)}`,
      { method: 'DELETE', body: JSON.stringify({ actor: { role: 'human' } }) },
    );
  /** A read that passes the authorization gate names the app's person (`ORCHESTRATION_APP_ACTOR`). */
  orchestrationCapacity = (owner: string, target: string, claimNamespaces: readonly string[]) => {
    const query = new URLSearchParams({ actor_role: 'human', actor_id: ORCHESTRATION_APP_ACTOR });
    if (claimNamespaces.length) query.set('claim_namespaces', claimNamespaces.join(','));
    return this.parsed(
      parseOrchestrationCapacity,
      `/checkouts/${encodeURIComponent(owner)}/orchestration/targets/${encodeURIComponent(target)}/capacity?${query}`,
    );
  };
  /**
   * HS2-D4G81Q: each project with its sites, and what each bound site's machine is. Each machine is
   * read through the authorization gate as the capacity read is, so it names the app's person.
   */
  orchestrationSites = (owner: string) => {
    const query = new URLSearchParams({ capabilities: 'true', actor_role: 'human', actor_id: ORCHESTRATION_APP_ACTOR });
    return this.parsed(parseOrchestrationSites, `/checkouts/${encodeURIComponent(owner)}/orchestration/sites?${query}`);
  };
  /**
   * HS2-DR0ZSQ: the machine that serves this project, as the Systems screen shows it: the hosted
   * capacity snapshot (HS2-J8PRTV). With `after` it is a long poll (HS2-P5GTSW) that answers when the
   * snapshot's version differs, or after {@link MACHINE_CAPACITY_WAIT_MS}. Not a busy-indicator read.
   */
  machineCapacity = (checkout: string, after: string | undefined, signal?: AbortSignal) =>
    this.parsed(
      parseOrchestrationCapacity,
      `/checkouts/${encodeURIComponent(checkout)}/target-capacity${
        after === undefined ? '' : `?after=${encodeURIComponent(after)}&wait_ms=${String(MACHINE_CAPACITY_WAIT_MS)}`
      }`,
      signal ? { signal } : {},
      false,
    );
  orchestrationAdmission = (owner: string, target: string, tool: string) =>
    this.parsed(
      parseOrchestrationAdmission,
      `/checkouts/${encodeURIComponent(owner)}/orchestration/targets/${encodeURIComponent(target)}/admission`,
      {
        method: 'POST',
        body: JSON.stringify({ tool, actor: { role: 'human', id: ORCHESTRATION_APP_ACTOR } }),
      },
    );
  orchestrationToolCapacity = (owner: string) =>
    this.parsed(parseOrchestrationToolCapacity, `/checkouts/${encodeURIComponent(owner)}/orchestration/tool-capacity`);
  orchestrationSessions = (owner: string) =>
    this.parsed(
      parseOrchestrationSession.list,
      `/checkouts/${encodeURIComponent(owner)}/orchestration/sessions?active=true`,
    );
  stopOrchestrationSession = (owner: string, session: string, force: boolean) =>
    this.parsed(
      parseOrchestrationSession,
      `/checkouts/${encodeURIComponent(owner)}/orchestration/sessions/${encodeURIComponent(session)}/stop`,
      { method: 'POST', body: JSON.stringify({ force, actor: { role: 'human', id: ORCHESTRATION_APP_ACTOR } }) },
    );
  /** HS2-HVKS7M: a person at the hosting machine stops a hosted session; `force` skips the safe point. */
  stopHostedSession = (checkout: string, operationId: string, force: boolean) =>
    this.parsed(parseHostedStop, `/checkouts/${encodeURIComponent(checkout)}/target-sessions`, {
      method: 'DELETE',
      body: JSON.stringify({
        operation_id: operationId,
        force,
        actor: { role: 'human', id: ORCHESTRATION_APP_ACTOR },
      }),
    });
  /** HS2-6MNMVB: a person at the hosting machine takes over (or pauses) a hosted session. */
  takeOverHostedSession = (checkout: string, operationId: string, pause: boolean) =>
    this.parsed(ignoreBody, `/checkouts/${encodeURIComponent(checkout)}/target-sessions`, {
      method: 'PATCH',
      body: JSON.stringify({
        operation_id: operationId,
        pause,
        actor: { role: 'human', id: ORCHESTRATION_APP_ACTOR },
      }),
    });
  /**
   * HS2-KR28FS: a person at the hosting machine hands a session back to its orchestrator. The
   * orchestrating checkout must be on this machine; `handBackOperationId` names the permission
   * request to continue (a new one asks again).
   */
  handBackHostedSession = (checkout: string, operationId: string, handBackOperationId: string) =>
    this.parsed(parseHostedHandBackReply, `/checkouts/${encodeURIComponent(checkout)}/target-sessions`, {
      method: 'PATCH',
      body: JSON.stringify({
        operation_id: operationId,
        hand_back: true,
        hand_back_operation_id: handBackOperationId,
        actor: { role: 'human', id: ORCHESTRATION_APP_ACTOR },
      }),
    });
  takeOverOrchestrationSession = (owner: string, session: string) =>
    this.parsed(
      parseOrchestrationSession,
      `/checkouts/${encodeURIComponent(owner)}/orchestration/sessions/${encodeURIComponent(session)}/takeover`,
      { method: 'POST', body: JSON.stringify({ pause: false, actor: { role: 'human', id: ORCHESTRATION_APP_ACTOR } }) },
    );
  orchestrationPolicy = (owner: string) =>
    this.parsed(parseOrchestrationPolicyView, `/checkouts/${encodeURIComponent(owner)}/orchestration/policy`);
  setOrchestrationPolicy = (owner: string, expectedRevision: string | null, policy: OrchestrationPolicyBody) =>
    this.parsed(parseOrchestrationPolicyView, `/checkouts/${encodeURIComponent(owner)}/orchestration/policy`, {
      method: 'PUT',
      body: JSON.stringify({ expected_revision: expectedRevision, policy, actor: { role: 'human' } }),
    });
  /**
   * HS2-TZJYKH: Start (`true`) or Pause (`false`) as one runtime edit of the stored policy at its revision; with
   * none saved the server saves the default policy with it (HS2-D9KP86).
   */
  setOrchestrationAutomatic = (owner: string, automatic: boolean) =>
    this.parsed(
      parseOrchestrationAutomaticSet,
      `/checkouts/${encodeURIComponent(owner)}/orchestration/policy/automatic`,
      { method: 'PUT', body: JSON.stringify({ automatic, actor: { role: 'human' } }) },
    );
  trashSettings = (checkoutId: string) =>
    this.parsed(parseTrashSettings, `/checkouts/${encodeURIComponent(checkoutId)}/trash-settings`);
  trashPurgePreview = (checkoutId: string, days: number) =>
    this.parsed(
      parseTrashPurgePreview,
      `/checkouts/${encodeURIComponent(checkoutId)}/trash/purge-preview?days=${encodeURIComponent(days)}`,
    );
  ticketCreationSettings = (checkoutId: string) =>
    this.parsed(parseTicketCreationSettings, `/checkouts/${encodeURIComponent(checkoutId)}/ticket-creation-settings`);
  saveTicketCreationSettings = (checkoutId: string, value: TicketCreationSettings) =>
    this.parsed(parseTicketCreationSettings, `/checkouts/${encodeURIComponent(checkoutId)}/ticket-creation-settings`, {
      method: 'PUT',
      body: JSON.stringify(value),
    });
  saveTrashSettings = (checkoutId: string, value: TrashSettings) =>
    this.parsed(parseTrashSettings, `/checkouts/${encodeURIComponent(checkoutId)}/trash-settings`, {
      method: 'PUT',
      body: JSON.stringify(value),
    });
  terminal = (id: string) => this.parsed(parseTerminalRead, `/terminals/${encodeURIComponent(id)}`);
  answerTerminalQuestion = (id: string, question: TerminalQuestion, answers?: Record<string, string>) =>
    this.parsed(ignoreBody, `/terminals/${encodeURIComponent(id)}/question/answer`, {
      method: 'POST',
      body: JSON.stringify({
        tool_use_id: question.tool_use_id,
        at: question.at,
        ...(answers ? { answers } : { native: true }),
      }),
    });
  createTerminal = (
    value: {
      id?: string;
      command?: string;
      shell_command?: string;
      args?: string[];
      cwd?: string;
      connect?: string;
      model?: string;
      effort?: string;
    } = {},
    signal?: AbortSignal,
  ) => this.parsed(parseTerminalInfo, '/terminals', { method: 'POST', body: JSON.stringify(value), signal });
  deleteTerminal = (id: string) =>
    this.parsed(ignoreBody, `/terminals/${encodeURIComponent(id)}`, { method: 'DELETE' });
  /** Clear only the stopped episode the user saw; a newer failure remains visible. */
  clearTerminalHalt = (id: string, at: string) =>
    this.parsed(ignoreBody, `/terminals/${encodeURIComponent(id)}/halt?at=${encodeURIComponent(at)}`, {
      method: 'DELETE',
    });
  /**
   * Clear a terminal for every viewer (HS2-HA0Z3A): the server's replay keeps only the cursor's line,
   * and attached viewports receive a replacement. Rejects with `501` when a detached broker predates it.
   */
  clearTerminal = (id: string) =>
    this.parsed(ignoreBody, `/terminals/${encodeURIComponent(id)}/clear`, { method: 'POST' });
  /**
   * Choose a live AI terminal as its project's feedback-primary session (HS2-H3S7YQ). Rejects with
   * `409 feedback_primary_ineligible` when its session cannot receive pushed messages.
   */
  chooseFeedbackPrimary = (id: string) =>
    this.parsed(ignoreBody, `/terminals/${encodeURIComponent(id)}/feedback-primary`, { method: 'PUT' });
  /** Forget the person's feedback-primary choice, so the oldest session receives feedback again (HS2-H3S7YQ). */
  clearFeedbackPrimary = (id: string) =>
    this.parsed(ignoreBody, `/terminals/${encodeURIComponent(id)}/feedback-primary`, { method: 'DELETE' });
  /** Save (or, with `null`, clear) a terminal's shared tab name (HS2-89FPV1). */
  renameTerminal = (id: string, name: string | null) =>
    this.parsed(parseTerminalName, `/terminals/${encodeURIComponent(id)}/name`, {
      method: 'PUT',
      body: JSON.stringify({ name }),
    });
  commands = () => this.parsed(parseCommandDefinition.list, '/commands');
  commandSchedules = (checkout: string) =>
    this.parsed(parseScheduleViews, `/checkouts/${encodeURIComponent(checkout)}/command-schedules`);
  previewCommandSchedule = (checkout: string, input: ScheduleInput) =>
    this.parsed(parseSchedulePreview, `/checkouts/${encodeURIComponent(checkout)}/command-schedules/preview`, {
      method: 'POST',
      body: JSON.stringify(input),
    });
  saveCommandSchedule = (checkout: string, commandId: string, input: ScheduleInput) =>
    this.parsed(
      parseScheduleView,
      `/checkouts/${encodeURIComponent(checkout)}/command-schedules/${encodeURIComponent(commandId)}`,
      { method: 'PUT', body: JSON.stringify(input) },
    );
  /** Run an AI command's configured action: queue its ticket or hand it to a session (HS2-17Y2EC). */
  runAiCommand = (checkout: string, commandId: string) =>
    this.parsed(
      parseAiCommandRun,
      `/checkouts/${encodeURIComponent(checkout)}/commands/${encodeURIComponent(commandId)}/ai-run`,
      { method: 'POST' },
    );
  deleteCommandSchedule = (checkout: string, commandId: string) =>
    this.parsed(
      ignoreBody,
      `/checkouts/${encodeURIComponent(checkout)}/command-schedules/${encodeURIComponent(commandId)}`,
      { method: 'DELETE' },
    );
  commandsWithRevision = async (): Promise<{ definitions: CommandDefinition[]; revision: string }> => {
    let revision = '*';
    const definitions = await this.parsed(parseCommandDefinition.list, '/commands', {}, true, (response) => {
      revision = response.headers.get('ETag') ?? '*';
    });
    return { definitions, revision };
  };
  saveCommands = (
    definitions: CommandDefinition[],
    options: { keepalive?: boolean; writeOrder?: { session: string; sequence: number }; expectedRevision: string },
  ) =>
    this.parsed(parseCommandDefinition.list, '/commands', {
      method: 'PUT',
      body: JSON.stringify(definitions),
      ...(options.keepalive ? { keepalive: true } : {}),
      headers: {
        'If-Match': options.expectedRevision,
        ...(options.writeOrder
          ? {
              'X-Hotsheet-Command-Write-Session': options.writeOrder.session,
              'X-Hotsheet-Command-Write-Sequence': String(options.writeOrder.sequence),
            }
          : {}),
      },
    });
  saveCommandsWithRevision = async (
    definitions: CommandDefinition[],
    options: { keepalive?: boolean; writeOrder?: { session: string; sequence: number }; expectedRevision: string },
  ): Promise<{ definitions: CommandDefinition[]; revision: string }> => {
    let revision = options.expectedRevision;
    const saved = await this.parsed(
      parseCommandDefinition.list,
      '/commands',
      {
        method: 'PUT',
        body: JSON.stringify(definitions),
        ...(options.keepalive ? { keepalive: true } : {}),
        headers: {
          'If-Match': options.expectedRevision,
          ...(options.writeOrder
            ? {
                'X-Hotsheet-Command-Write-Session': options.writeOrder.session,
                'X-Hotsheet-Command-Write-Sequence': String(options.writeOrder.sequence),
              }
            : {}),
        },
      },
      true,
      (response) => {
        revision = response.headers.get('ETag') ?? revision;
      },
    );
    return { definitions: saved, revision };
  };
  /** Command groups kept even while empty, stored beside `commands` (HS2-EZ5KMC). */
  commandGroups = () => this.parsed(parseStringList, '/command-groups');
  commandGroupsWithRevision = async (): Promise<{ groups: string[]; revision: string }> => {
    let revision = '*';
    const groups = await this.parsed(parseStringList, '/command-groups', {}, true, (response) => {
      revision = response.headers.get('ETag') ?? '*';
    });
    return { groups, revision };
  };
  saveCommandGroups = (groups: string[], expectedRevision: string) =>
    this.parsed(parseStringList, '/command-groups', {
      method: 'PUT',
      body: JSON.stringify(groups),
      headers: { 'If-Match': expectedRevision },
    });
  saveCommandGroupsWithRevision = async (
    groups: string[],
    expectedRevision: string,
  ): Promise<{ groups: string[]; revision: string }> => {
    let revision = expectedRevision;
    const saved = await this.parsed(
      parseStringList,
      '/command-groups',
      { method: 'PUT', body: JSON.stringify(groups), headers: { 'If-Match': expectedRevision } },
      true,
      (response) => {
        revision = response.headers.get('ETag') ?? revision;
      },
    );
    return { groups: saved, revision };
  };
  customViews = () => this.parsed(parseCustomView.list, '/views');
  saveCustomViews = (views: CustomView[]) =>
    this.parsed(parseCustomView.list, '/views', { method: 'PUT', body: JSON.stringify(views) });
  commandRuns = () => this.parsed(parseCommandRun.list, '/command-runs');
  runCommand = (id: string) =>
    this.parsed(parseCommandRun, `/commands/${encodeURIComponent(id)}/run`, { method: 'POST' });
  /** Record a shell or AI command launch in the command's run history (HS2-PWNPM2). */
  recordCommandLaunch = (id: string, launch: CommandLaunch) =>
    this.parsed(parseCommandRun, `/commands/${encodeURIComponent(id)}/launches`, {
      method: 'POST',
      body: JSON.stringify(launch),
    });
  commandRun = (id: string, after = 0) =>
    this.parsed(parseCommandRun, `/command-runs/${encodeURIComponent(id)}?after=${after}`);
  cancelCommandRun = (id: string) =>
    this.parsed(parseCommandRun, `/command-runs/${encodeURIComponent(id)}/cancel`, { method: 'POST' });
  changeWebSocketUrl = () => {
    const url = new URL(`${this.origin}/ws/sync`, typeof location === 'undefined' ? 'http://localhost' : location.href);
    url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:';
    url.searchParams.set('client', CHANGE_STREAM_CLIENT_ID);
    return url.toString();
  };
  pollEvents = (since?: number, signal?: AbortSignal, timeoutMs = 25_000) =>
    this.parsed(
      parsePollResponse,
      `/ws/poll?timeout_ms=${timeoutMs}${since === undefined ? '' : `&since=${since}`}&client=${encodeURIComponent(CHANGE_STREAM_CLIENT_ID)}`,
      { signal },
      false,
    );
  /** Tell the server this tab closed the project, so it can stop hosting what nothing else needs. */
  closeProjectSession = () =>
    this.parsed(ignoreBody, `/close?client=${encodeURIComponent(CHANGE_STREAM_CLIENT_ID)}`, { method: 'POST' });
  resolvePermission = (
    id: number,
    decision: 'allow' | 'deny',
    scope: 'once' | 'chat' | 'always',
    description?: string,
    pattern?: PermissionPattern,
    continueReview = false,
  ) =>
    this.parsed(parsePermissionDecision, `/permissions/${id}`, {
      method: 'POST',
      body: JSON.stringify({
        decision,
        scope,
        ...(description ? { description } : {}),
        ...(pattern ? { pattern } : {}),
        ...(continueReview ? { continue_review: true } : {}),
      }),
    });
  rememberPermissionAfterAllow = (id: number, description?: string, pattern?: PermissionPattern) =>
    this.parsed(ignoreBody, `/permissions/${id}/remember`, {
      method: 'POST',
      body: JSON.stringify({ ...(description ? { description } : {}), ...(pattern ? { pattern } : {}) }),
    });
  /** Remembered permission answers: chat, session, and durable rules (HS2-JNHAYB). */
  permissionRules = () => this.parsed(parsePermissionRule.list, '/permissions/rules');
  /**
   * HS2-4K43DK: the orchestrator's remembered rules alone. They live outside the AI permission
   * vault, so they are listed while the full list answers 503 for a locked or loading vault.
   */
  orchestratorPermissionRules = () => this.parsed(parsePermissionRule.list, '/permissions/rules?source=orchestrator');
  updatePermissionRule = (
    rule: { project: string; connection?: string; tool: string; action: string },
    decision: 'allow' | 'deny',
  ) =>
    this.parsed(ignoreBody, '/permissions/rules', {
      method: 'PATCH',
      body: JSON.stringify({
        project: rule.project,
        ...(rule.connection ? { connection: rule.connection } : {}),
        tool: rule.tool,
        action: rule.action,
        decision,
      }),
    });
  updatePermissionRulePattern = (
    rule: { project: string; connection?: string; tool: string; action: string },
    pattern: PermissionPattern | 'exact',
  ) =>
    this.parsed(ignoreBody, '/permissions/rules', {
      method: 'PATCH',
      body: JSON.stringify({
        project: rule.project,
        connection: rule.connection,
        tool: rule.tool,
        action: rule.action,
        pattern,
      }),
    });
  updatePermissionRuleScope = (
    rule: { project: string; connection?: string; tool: string; action: string },
    scope: 'session' | 'always',
  ) =>
    this.parsed(ignoreBody, '/permissions/rules', {
      method: 'PATCH',
      body: JSON.stringify({
        project: rule.project,
        ...(rule.connection ? { connection: rule.connection } : {}),
        tool: rule.tool,
        action: rule.action,
        scope,
      }),
    });
  revokePermissionRule = (rule: { project: string; connection?: string; tool: string; action: string }) =>
    this.parsed(ignoreBody, '/permissions/rules', {
      method: 'DELETE',
      body: JSON.stringify({
        project: rule.project,
        ...(rule.connection ? { connection: rule.connection } : {}),
        tool: rule.tool,
        action: rule.action,
      }),
    });
}

export async function revealCorruptTicketFile(project: string, path: string): Promise<void> {
  assertHostAction('reveal-file');
  const response = await fetch(`/__hotsheet/projects/${encodeURIComponent(project)}/corrupt-tickets/reveal`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ path }),
  });
  if (!response.ok) throw new Error(errorMessageOf(await responseJson(response)) ?? `${response.status}`);
}
