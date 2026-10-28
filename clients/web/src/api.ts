import { browserRandomId } from './browser-id';
import { assertHostAction, type HostOnlyAction } from './host-only-actions';
import { errorCodeOf, errorMessageOf, responseJson, responseJsonBody } from './json-value';
import type { PermissionPattern, PermissionSecretSuggestion } from './permission-notifications';
import { prioritiesToWire } from './priority-wire';
import { observeRequest } from './request-observer';
import { beginServerRequest, describeServerRequest, endServerRequest } from './server-busy';
import {
  ignoreBody,
  parseAccountIdentity,
  parseAiToolDefaults,
  parseAiToolDescriptor,
  parseCalibrationReport,
  parseCheckout,
  parseCheckoutRelink,
  parseCheckoutTicketPage,
  parseCheckoutTicketRowsPage,
  parseCodeReview,
  parseCommandDefinition,
  parseCommandRun,
  parseCompactionPlan,
  parseCompactionResult,
  parseCorruptTicket,
  parseCustomView,
  parseDuplicateBacklinks,
  parseFilePath,
  parseFullTicket,
  parseGitHubAuthStart,
  parseGitHubAuthStatus,
  parseGitHubRepositoryAccess,
  parseGuidanceBatchReview,
  parseGuidanceInterviewReview,
  parseGuidanceOverview,
  parseGuidanceRecommendationReport,
  parseGuidanceRulePreview,
  parseGuidanceSourceReview,
  parseGuidanceSourceUpdates,
  parseLegacyPermissionDiscardOutcome,
  parseLegacyPermissionPreview,
  parseMessageConnectionReceipt,
  parsePermissionDecision,
  parsePermissionRequest,
  parsePermissionRule,
  parsePollResponse,
  parseProviderAccount,
  parseProviderConnection,
  parseProviderDescriptor,
  parseProviderOutboxOperation,
  parseQueuedProviderResult,
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
  parseTicket,
  parseTicketRow,
  parseTicketUsage,
  parseToolConnection,
  parseToolSession,
  parseTrashPurge,
  parseTrashPurgePreview,
  parseTrashSettings,
  type ResponseParser,
} from './server-response-parsers';
import type { TicketCloseReason } from './ticket-close';
import { completionDayStarts } from './ticket-completion-trend';

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
  /** Existing attachments can be renamed, deleted, regrouped, and annotated (HS2-HSA64D). Older servers omit it. */
  attachment_edit?: boolean;
  /** Existing attachments can be deleted even when they cannot be renamed (Jira, HS2-51ADSG). */
  attachment_delete?: boolean;
  attachment_crop?: boolean;
  /** Host may expose an opt-in durable provisional field-edit endpoint. */
  write_behind?: boolean;
  /** Structured, revisable ratings and the provider feedback query. */
  ai_feedback?: boolean;
};
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
  capabilities: Capabilities;
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
  blocked_reason?: string;
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
  legacy_number?: string;
  claim_count: number;
  /** Derived completion confidence of a completed/verified ticket (HS2-RD4M29). */
  latest_confidence?: number;
  claim_history?: ClaimEvent[];
  created_at?: string;
  updated_at?: string;
  completed_at?: string;
  closed_at?: string;
  close_reason?: TicketCloseReason;
  duplicate_of?: string;
  details?: string;
  notes?: Note[];
}
export interface ClaimEvent {
  id: string;
  kind: 'claim' | 'renew' | 'release';
  worker: string;
  at: string;
  lease_expires_at?: string;
  /** The holder's estimate as of this claim or renewal (HS2-DQQ0AX). */
  eta_at?: string;
  worker_label?: string;
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
  busy: boolean;
  cwd?: string;
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
export interface TrashSettings {
  trash_cleanup_days: number;
}
/** How many Trash tickets a retention of `trash_cleanup_days` would purge right now (HS2-KPD942). */
export interface TrashPurgePreview {
  trash_cleanup_days: number;
  purge_count: number;
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
  icon?: string;
  color?: string;
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
  if (method === 'POST') return /\/tickets\/[^/]+\/close$/u.test(pathname) || pathname.endsWith('/batch');
  return false;
}

/**
 * Ticket mutations from this client are a person acting (HS2-XF81CJ): add
 * `actor: {"role":"human"}` to the JSON object bodies of the actor-aware routes so the server records human
 * authorship and applies human (never AI-only) rules. A body that already names its actor,
 * including an explicit `null` for unknown authorship, is left unchanged.
 */
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

export class ApiHttpError extends Error {
  constructor(
    message: string,
    readonly status: number,
    /** The failure body's stable `code`, for callers that branch on the kind of failure (HS2-9S8CV1). */
    readonly code?: string,
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
   * available: it can still be pasted into a terminal that runs on that machine.
   */
  private hostAction(action: 'open' | 'reveal' | 'path' | HostOnlyAction): { parsed: HostParsedRequest } {
    const kind = action === 'open' ? 'open-file' : action === 'reveal' ? 'reveal-file' : action;
    return {
      parsed: async (parse, path, requested) => {
        if (kind !== 'path') assertHostAction(kind);
        return this.parsed(parse, path, requested);
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
        throw new ApiHttpError(errorMessageOf(failure) ?? `${response.status}`, response.status, errorCodeOf(failure));
      }
      onResponse?.(response);
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
  tickets = (id: string) => this.parsed(parseTicket.list, `/providers/${encodeURIComponent(id)}/tickets`);
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
  createConnection = (value: ProviderConnection, makeDefault = false) =>
    this.parsed(parseProviderConnection, '/provider-connections', {
      method: 'POST',
      body: JSON.stringify({ ...value, make_default: makeDefault }),
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
  addCheckoutSource = (checkout: string, connection: ProviderConnection, makeDefault = false) =>
    this.parsed(
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
  relinkCheckoutGitSource = (checkout: string, connectionId: string, path: string, reviewUnverifiedRecovery = false) =>
    this.parsed(
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
  /** Git object and attachment sizes with GitHub-limit levels (HS2-QF44T7). */
  checkoutRepositorySize = (checkout: string) =>
    this.parsed(parseRepositorySizeReport, `/checkouts/${encodeURIComponent(checkout)}/repository/size`);

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
  checkoutTicketDuplicateBacklinks = (checkout: string, id: string) =>
    this.parsed(
      parseDuplicateBacklinks,
      `/checkouts/${encodeURIComponent(checkout)}/tickets/${encodeURIComponent(id)}/duplicate-backlinks`,
    );
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
      body: JSON.stringify(prioritiesToWire(value)),
    }).then((ticket) => ({ store: ticket.store, ticket }));
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
  createToolConnection = (value: {
    tool: string;
    checkout: string;
    source?: string;
    connection_id?: string;
    session_id?: string;
    model?: string;
    effort?: string;
    permission_mode?: string;
  }) => this.parsed(parseToolConnection, '/drive/connections', { method: 'POST', body: JSON.stringify(value) });
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
  terminals = () => this.parsed(parseTerminalInfo.list, '/terminals');
  terminalSettings = () => this.parsed(parseTerminalSettings, '/terminal-settings');
  saveTerminalSettings = (value: TerminalSettings) =>
    this.parsed(parseTerminalSettings, '/terminal-settings', { method: 'PUT', body: JSON.stringify(value) });
  terminalLimits = () => this.parsed(parseTerminalLimits, '/terminal-limits');
  saveTerminalLimits = (value: TerminalLimits['configured']) =>
    this.parsed(parseTerminalLimits, '/terminal-limits', { method: 'PUT', body: JSON.stringify(value) });
  trashSettings = (checkoutId: string) =>
    this.parsed(parseTrashSettings, `/checkouts/${encodeURIComponent(checkoutId)}/trash-settings`);
  trashPurgePreview = (checkoutId: string, days: number) =>
    this.parsed(
      parseTrashPurgePreview,
      `/checkouts/${encodeURIComponent(checkoutId)}/trash/purge-preview?days=${encodeURIComponent(days)}`,
    );
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
  ) => this.parsed(parseTerminalInfo, '/terminals', { method: 'POST', body: JSON.stringify(value) });
  deleteTerminal = (id: string) =>
    this.parsed(ignoreBody, `/terminals/${encodeURIComponent(id)}`, { method: 'DELETE' });
  /** Clear only the stopped episode the user saw; a newer failure remains visible. */
  clearTerminalHalt = (id: string, at: string) =>
    this.parsed(ignoreBody, `/terminals/${encodeURIComponent(id)}/halt?at=${encodeURIComponent(at)}`, {
      method: 'DELETE',
    });
  /** Save (or, with `null`, clear) a terminal's shared tab name (HS2-89FPV1). */
  renameTerminal = (id: string, name: string | null) =>
    this.parsed(parseTerminalName, `/terminals/${encodeURIComponent(id)}/name`, {
      method: 'PUT',
      body: JSON.stringify({ name }),
    });
  commands = () => this.parsed(parseCommandDefinition.list, '/commands');
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
