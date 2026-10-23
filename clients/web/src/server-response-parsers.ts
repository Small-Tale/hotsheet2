import type {
  AiToolDefaults,
  AiToolDescriptor,
  CalibrationReport,
  Checkout,
  CheckoutSource,
  CheckoutTicketPage,
  CheckoutTicketRowsPage,
  CodeReview,
  CodeReviewCommit,
  CommandDefinition,
  CommandRun,
  CompactionPlan,
  CorruptTicket,
  CustomView,
  DuplicateBacklinkResponse,
  FullTicket,
  GitHubAuthStart,
  GitHubAuthStatus,
  GitHubRepositoryAccess,
  GuidanceInterviewReview,
  GuidanceOverview,
  GuidanceRecommendationReport,
  GuidanceSourceReview,
  GuidanceSourceUpdate,
  MessageConnectionReceipt,
  PermissionRequest,
  PollResponse,
  ProviderAccount,
  ProviderConnection,
  ProviderDescriptor,
  ProviderOutboxOperation,
  QueuedProviderResult,
  RepositoryFile,
  RepositoryPage,
  RepositorySizeEntry,
  RepositorySizeReport,
  RepositoryStatus,
  RulesyncUpdateReport,
  SourceDetach,
  TerminalInfo,
  TerminalLimits,
  TerminalName,
  TerminalRead,
  TerminalSettings,
  Ticket,
  TicketRow,
  TicketUsage,
  ToolConnection,
  ToolSession,
  TrashPurgePreview,
  TrashSettings,
} from './api';
import type { CompatibilityRange, ServerCompatibility } from './compatibility';
import { isRecord } from './json-value';
import type { PermissionPattern, PermissionRule, PermissionSecretSuggestion } from './permission-notifications';

/**
 * Response parsers for successful Hot Sheet server bodies (HS2-34P6XY). `Api` and the
 * project bridge's `serverRequest` read a 2xx body as `unknown` and pass it through one of
 * these before callers see it, so a body in an unexpected shape fails at the boundary with
 * a {@link ServerResponseShapeError} instead of at a later property access. Parsers check
 * every field callers rely on and keep the rest of the object as the server sent it.
 */

export type ResponseParser<T> = (value: unknown) => T;

/** A 2xx body that did not have the documented shape for its endpoint. */
export class ServerResponseShapeError extends Error {
  constructor(readonly what: string) {
    super(`The server returned an invalid ${what} response.`);
    this.name = 'ServerResponseShapeError';
  }
}

function invalid(what: string): never {
  throw new ServerResponseShapeError(what);
}

function record(value: unknown, what: string): Record<string, unknown> {
  return isRecord(value) ? value : invalid(what);
}

function requiredString(value: Record<string, unknown>, key: string, what: string): string {
  const field = value[key];
  return typeof field === 'string' ? field : invalid(what);
}

function optionalOf(
  value: Record<string, unknown>,
  key: string,
  what: string,
  check: (field: unknown) => boolean,
  nullable = false,
) {
  const field = value[key];
  if (field === undefined || (nullable && field === null)) return;
  if (!check(field)) invalid(what);
}

const isString = (field: unknown) => typeof field === 'string';
const isBoolean = (field: unknown) => typeof field === 'boolean';
const isStringArray = (field: unknown) => Array.isArray(field) && field.every(isString);

function list<T>(value: unknown, what: string, item: (entry: unknown) => T): T[] {
  return Array.isArray(value) ? value.map(item) : invalid(what);
}

/** A body the caller ignores (`DELETE`, `POST` acknowledgements): any value is accepted. */
export const ignoreBody: ResponseParser<undefined> = () => undefined;

export const parseStringList: ResponseParser<string[]> = (value) =>
  isStringArray(value) ? value : invalid('string list');

export const parseGuidanceOverview: ResponseParser<GuidanceOverview> = (value) => {
  const what = 'guidance',
    body = record(value, what);
  if (body.owner !== 'builtin' && body.owner !== 'rulesync') invalid(what);
  if (typeof body.explicit_selection !== 'boolean') invalid(what);
  requiredString(body, 'default_repository', what);
  if (body.local_rules !== undefined && !isStringArray(body.local_rules)) invalid(what);
  const sources = list(body.sources, what, (entry) => {
    const source = record(entry, what);
    for (const key of ['id', 'repository', 'requested_ref', 'commit']) requiredString(source, key, what);
    const rules = list(source.rules, what, (entry) => {
      const rule = record(entry, what);
      for (const key of ['name', 'description']) requiredString(rule, key, what);
      if (
        typeof rule.version !== 'number' ||
        (rule.installed_version !== null && typeof rule.installed_version !== 'number') ||
        (rule.owner !== null && typeof rule.owner !== 'string') ||
        !isStringArray(rule.tags) ||
        typeof rule.selected !== 'boolean'
      )
        invalid(what);
      return rule as unknown as GuidanceOverview['sources'][number]['rules'][number];
    });
    const skills = list(source.skills, what, (entry) => {
      const skill = record(entry, what);
      requiredString(skill, 'name', what);
      if (typeof skill.selected !== 'boolean') invalid(what);
      return skill as unknown as GuidanceOverview['sources'][number]['skills'][number];
    });
    return { ...source, rules, skills } as GuidanceOverview['sources'][number];
  });
  return { ...body, sources } as GuidanceOverview;
};

export const parseGuidanceRecommendationReport: ResponseParser<GuidanceRecommendationReport> = (value) => {
  const what = 'guidance recommendations';
  const body = record(value, what);
  requiredString(body, 'fingerprint', what);
  if (
    typeof body.source_registration_suggested !== 'boolean' ||
    typeof body.first_accept_replaces_bundled_default !== 'boolean'
  )
    invalid(what);
  const signals = list(body.signals, what, (entry) => {
    const item = record(entry, what);
    requiredString(item, 'key', what);
    requiredString(item, 'evidence', what);
    return item as GuidanceRecommendationReport['signals'][number];
  });
  const candidates = list(body.candidates, what, (entry) => {
    const item = record(entry, what);
    for (const key of ['source', 'name', 'description', 'pinned_commit', 'fingerprint'])
      requiredString(item, key, what);
    if (
      !['rule', 'skill'].includes(String(item.kind)) ||
      !['new', 'accepted', 'dismissed', 'selected'].includes(String(item.state)) ||
      typeof item.version !== 'number' ||
      typeof item.selected !== 'boolean' ||
      (item.installed_version !== null && typeof item.installed_version !== 'number') ||
      !isStringArray(item.evidence)
    )
      invalid(what);
    return item as unknown as GuidanceRecommendationReport['candidates'][number];
  });
  const changes = list(body.changes, what, (entry) => {
    const item = record(entry, what);
    for (const key of ['source', 'name']) requiredString(item, key, what);
    if (
      !['rule', 'skill'].includes(String(item.kind)) ||
      !['new', 'changed', 'withdrawn'].includes(String(item.change))
    )
      invalid(what);
    return item as unknown as GuidanceRecommendationReport['changes'][number];
  });
  return { ...body, signals, candidates, changes } as GuidanceRecommendationReport;
};

export const parseGuidanceInterviewReview: ResponseParser<GuidanceInterviewReview> = (value) => {
  const what = 'guidance interview';
  const body = record(value, what);
  if (!['not_started', 'awaiting_answer', 'answered'].includes(String(body.state))) invalid(what);
  const preview = record(body.preview, what);
  requiredString(preview, 'fingerprint', what);
  if (preview.outbound_prompt !== null && typeof preview.outbound_prompt !== 'string') invalid(what);
  const context = record(preview.context, what);
  if (!isStringArray(context.signals)) invalid(what);
  list(context.candidates, what, (entry) => {
    const candidate = record(entry, what);
    for (const key of ['id', 'source', 'name', 'pinned_commit']) requiredString(candidate, key, what);
    if (
      !['rule', 'skill'].includes(String(candidate.kind)) ||
      typeof candidate.version !== 'number' ||
      typeof candidate.selected !== 'boolean' ||
      !isStringArray(candidate.evidence)
    )
      invalid(what);
    return candidate;
  });
  list(preview.questions, what, (entry) => {
    const question = record(entry, what);
    requiredString(question, 'id', what);
    requiredString(question, 'prompt', what);
    if (!isStringArray(question.choices)) invalid(what);
    return question;
  });
  if (body.answer !== null) {
    const answer = record(body.answer, what);
    list(answer.advice, what, (entry) => {
      const advice = record(entry, what);
      requiredString(advice, 'candidate_id', what);
      requiredString(advice, 'explanation', what);
      if (!['consider', 'avoid', 'uncertain'].includes(String(advice.stance))) invalid(what);
      return advice;
    });
  }
  return body as unknown as GuidanceInterviewReview;
};

const parseGuidanceSourceUpdate: ResponseParser<GuidanceSourceUpdate> = (value) => {
  const what = 'guidance source update';
  const body = record(value, what);
  for (const key of ['id', 'requested_ref', 'pinned_commit']) requiredString(body, key, what);
  if (body.observed_commit !== null && typeof body.observed_commit !== 'string') invalid(what);
  if (!['current', 'revision_changed', 'pinned', 'tag_moved', 'check_failed'].includes(String(body.state)))
    invalid(what);
  if (body.reason !== undefined && typeof body.reason !== 'string') invalid(what);
  return body as unknown as GuidanceSourceUpdate;
};

export const parseGuidanceSourceUpdates: ResponseParser<GuidanceSourceUpdate[]> = (value) =>
  list(value, 'guidance source updates', parseGuidanceSourceUpdate);

export const parseGuidanceSourceReview: ResponseParser<GuidanceSourceReview> = (value) => {
  const what = 'guidance source review';
  const body = record(value, what);
  const update = parseGuidanceSourceUpdate(body.update);
  const changes = list(body.changes, what, (entry) => {
    const change = record(entry, what);
    requiredString(change, 'name', what);
    if (
      !['rule', 'skill'].includes(String(change.kind)) ||
      !['added', 'removed', 'updated'].includes(String(change.change))
    )
      invalid(what);
    if (typeof change.selected !== 'boolean') invalid(what);
    for (const key of ['before_version', 'after_version'])
      if (change[key] !== null && typeof change[key] !== 'number') invalid(what);
    return change as unknown as GuidanceSourceReview['changes'][number];
  });
  return { update, changes };
};

export const parseRulesyncUpdateReport: ResponseParser<RulesyncUpdateReport | null> = (value) => {
  if (value === null) return null;
  const what = 'Rulesync update status';
  const body = record(value, what);
  const states = ['current', 'outdated', 'unlocked', 'unknown'];
  if (!states.includes(String(body.state))) invalid(what);
  if (body.executable_version !== null && typeof body.executable_version !== 'string') invalid(what);
  if (body.reason !== undefined && typeof body.reason !== 'string') invalid(what);
  const sources = list(body.sources, what, (entry) => {
    const source = record(entry, what);
    requiredString(source, 'source', what);
    if (!states.includes(String(source.state))) invalid(what);
    for (const key of ['requested_ref', 'locked_ref', 'latest_ref'])
      if (source[key] !== null && typeof source[key] !== 'string') invalid(what);
    return source as unknown as RulesyncUpdateReport['sources'][number];
  });
  return { ...body, sources } as RulesyncUpdateReport;
};

function isRange(field: unknown): field is CompatibilityRange {
  return isRecord(field) && typeof field.min === 'number' && typeof field.max === 'number';
}

/** `GET /compatibility`. */
export const parseServerCompatibility: ResponseParser<ServerCompatibility> = (value) => {
  const what = 'compatibility',
    body = record(value, what);
  requiredString(body, 'generation', what);
  optionalOf(body, 'application_version', what, isString);
  optionalOf(body, 'build_revision', what, isString, true);
  optionalOf(body, 'source_revision', what, isString, true);
  optionalOf(body, 'source_stale', what, isBoolean);
  optionalOf(body, 'protocol', what, isRange);
  optionalOf(body, 'store_schema', what, isRange);
  optionalOf(body, 'started_at', what, isString, true);
  optionalOf(body, 'launcher', what, isString, true);
  optionalOf(
    body,
    'capabilities',
    what,
    (field) =>
      isRecord(field) &&
      [field.lifecycle_restart, field.lifecycle_quiescence].every((flag) => flag === undefined || isBoolean(flag)),
  );
  return body as unknown as ServerCompatibility;
};

function parseCheckoutSource(value: unknown): CheckoutSource {
  const what = 'checkout source',
    body = record(value, what);
  for (const key of ['connection_id', 'provider', 'locator']) requiredString(body, key, what);
  return body as unknown as CheckoutSource;
}

/** A checkout (`GET /checkouts` entries, source and default-source updates). */
export const parseCheckout: ResponseParser<Checkout> = (value) => {
  const what = 'checkout',
    body = record(value, what);
  for (const key of ['id', 'root', 'alias']) requiredString(body, key, what);
  if (!isStringArray(body.stores)) invalid(what);
  optionalOf(body, 'repository', what, isString);
  optionalOf(body, 'default_source', what, isString);
  optionalOf(
    body,
    'unverified_store_sources',
    what,
    (field) => isRecord(field) && Object.values(field).every(isString),
  );
  if (body.sources !== undefined) list(body.sources, what, parseCheckoutSource);
  return body as unknown as Checkout;
};

export const parseCheckouts: ResponseParser<Checkout[]> = (value) => list(value, 'checkout list', parseCheckout);

/** `POST /projects/open`: the opened checkout with its (possibly empty) ticket sources. */
export interface OpenedCheckout {
  checkout: { id: string; root: string; alias: string; stores: string[]; sources: unknown[] };
}
export const parseOpenedCheckout: ResponseParser<OpenedCheckout> = (value) => {
  const what = 'project open',
    checkout = record(record(value, what).checkout, what);
  for (const key of ['id', 'root', 'alias']) requiredString(checkout, key, what);
  if (!isStringArray(checkout.stores) || !Array.isArray(checkout.sources)) invalid(what);
  return value as OpenedCheckout;
};

/** `GET /checkouts/{id}/corrupt-tickets` reduced to the paths the bridge authorizes reveals for. */
export const parseCorruptTicketPaths: ResponseParser<Array<{ path: string }>> = (value) =>
  list(value, 'corrupt ticket list', (entry) => {
    const body = record(entry, 'corrupt ticket');
    requiredString(body, 'path', 'corrupt ticket');
    return body as { path: string };
  });

/** `POST /accounts/{id}/identity`. */
export const parseAccountIdentity: ResponseParser<{ identity: string }> = (value) => {
  const body = record(value, 'account identity');
  requiredString(body, 'identity', 'account identity');
  return body as { identity: string };
};

/** `POST /checkouts/{id}/trash/empty`. */
export const parseTrashPurge: ResponseParser<{ purged: number; tickets: string[] }> = (value) => {
  const body = record(value, 'trash purge');
  if (typeof body.purged !== 'number' || !isStringArray(body.tickets)) invalid('trash purge');
  return body as { purged: number; tickets: string[] };
};

/* ---- Declarative DTO shapes (HS2-E0AG5E) -------------------------------------------------
 * Each shape names the fields callers rely on. Required fields must have the stated type;
 * optional fields may be absent or `null` (the Rust DTOs serialize `Option::None` as `null`
 * unless skipped) but must otherwise have the stated type. Unlisted fields pass through. */

type Check = (field: unknown) => boolean;

const isNumber: Check = (field) => typeof field === 'number' && Number.isFinite(field);
const isObject: Check = (field) => isRecord(field);
const listOf =
  (check: Check): Check =>
  (field) =>
    Array.isArray(field) && field.every(check);
const valuesOf =
  (check: Check): Check =>
  (field) =>
    isRecord(field) && Object.values(field).every(check);
const accepts =
  <T>(parse: ResponseParser<T>): Check =>
  (field) => {
    try {
      parse(field);
      return true;
    } catch {
      return false;
    }
  };

/** A parser for one DTO, with its boolean `check` for nesting and a `list` parser for arrays of it. */
export interface Shape<T> extends ResponseParser<T> {
  check: Check;
  list: ResponseParser<T[]>;
}

function shape<T>(what: string, required: Record<string, Check>, optional: Record<string, Check> = {}): Shape<T> {
  const check: Check = (value) =>
    isRecord(value) &&
    Object.entries(required).every(([key, field]) => field(value[key])) &&
    Object.entries(optional).every(
      ([key, field]) => value[key] === undefined || value[key] === null || field(value[key]),
    );
  const parse = (value: unknown) => (check(value) ? (value as T) : invalid(what));
  const parseList = (value: unknown) => (listOf(check)(value) ? (value as T[]) : invalid(`${what} list`));
  return Object.assign(parse, { check, list: parseList });
}

const project = shape<{ id: string; alias: string }>('project', { id: isString, alias: isString });

export const parseProviderDescriptor = shape<ProviderDescriptor>(
  'provider',
  { connection_id: isString, provider: isString, display_name: isString, locator: isString, capabilities: isObject },
  { default: isBoolean, color: isString },
);

export const parseProviderConnection = shape<ProviderConnection>(
  'provider connection',
  { id: isString, provider: isString, locator: isString },
  { name: isString, default: isBoolean, settings: isObject, disabled: isBoolean, projects: listOf(project.check) },
);

export const parseSourceDetach = shape<SourceDetach>(
  'source removal',
  { checkout_id: isString, connection_id: isString },
  { unlinked: isBoolean, removed_connection: isBoolean, still_used_by: isStringArray },
);

export const parseProviderAccount = shape<ProviderAccount>(
  'account',
  { id: isString, provider: isString, sources: listOf(isObject), projects: listOf(project.check) },
  { host: isString, base_url: isString, identity: isString, managed: isBoolean },
);

export const parseGitHubRepositoryAccess = shape<GitHubRepositoryAccess>(
  'GitHub repositories',
  { repositories: isStringArray },
  { installations: listOf(isObject), install_url: isString },
);

export const parseGitHubAuthStart = shape<GitHubAuthStart>('GitHub sign-in', {
  session_id: isString,
  user_code: isString,
  verification_uri: isString,
  expires_in: isNumber,
});

export const parseGitHubAuthStatus = shape<GitHubAuthStatus>(
  'GitHub sign-in status',
  { state: isString },
  { credential_reference: isString, message: isString },
);

const note = shape('note', { id: isString, kind: isString, text: isString }, { created_at: isString });
const attachment = shape('attachment', { id: isString, filename: isString }, { annotations: listOf(isObject) });

export const parseTicket = shape<Ticket>(
  'ticket',
  { qualified_id: isString, native_id: isString, connection_id: isString, title: isString },
  { status: isString, notes: listOf(note.check), attachments: listOf(attachment.check) },
);

const rowOptional: Record<string, Check> = {
  id: isString,
  title: isString,
  status: isString,
  up_next: isBoolean,
  feedback_needed: isBoolean,
  needs_review: isBoolean,
  tags: isStringArray,
  blocked_by: isStringArray,
  claim_count: isNumber,
  notes: listOf(note.check),
};
const fullTicketRequired: Record<string, Check> = {
  id: isString,
  slug: isString,
  title: isString,
  notes: listOf(note.check),
  attachments: listOf(attachment.check),
};

/** A checkout ticket row: a `fields=` projection keeps only `slug` (plus `store`), so only it is required. */
export const parseTicketRow = shape<TicketRow>('ticket row', { slug: isString }, rowOptional);

export const parseFullTicket = shape<FullTicket>('ticket', fullTicketRequired, {
  ...rowOptional,
  warnings: isStringArray,
});

/**
 * A checkout-scoped full ticket, which names its source `store` when the route knows it; the
 * field is type-checked when present but not required, as before this parser existed.
 */
export const parseStoredTicket = shape<FullTicket & { store: string }>('ticket', fullTicketRequired, {
  ...rowOptional,
  store: isString,
  warnings: isStringArray,
});

export const parseProviderOutboxOperation = shape<ProviderOutboxOperation>(
  'provider outbox operation',
  { operation_id: isString, connection_id: isString, native_id: isString, state: isString },
  { attempts: isNumber, next_attempt_at: isNumber, last_error: isString, conflict: isObject },
);

export const parseQueuedProviderResult = shape<QueuedProviderResult>('queued provider update', {
  operation_id: isString,
  state: isString,
  ticket: parseFullTicket.check,
});

export const parseCheckoutRelink = shape<{ checkouts: Checkout[]; connection_id: string }>('source relink', {
  checkouts: accepts(parseCheckouts),
  connection_id: isString,
});

const ticketPage = shape<CheckoutTicketPage>(
  'ticket page',
  { items: listOf(parseTicketRow.check), counts: isObject },
  { next_cursor: isString, source_errors: isStringArray },
);
const ticketRowsPage = shape<CheckoutTicketRowsPage>(
  'ticket page',
  { items: listOf(parseTicketRow.check) },
  { next_cursor: isString, source_errors: isStringArray },
);
/** `page_size` requests always receive the page envelope; a bare row list is rejected (HS2-5T7QHE). */
export const parseCheckoutTicketPage = ticketPage;
export const parseCheckoutTicketRowsPage = ticketRowsPage;

export const parseCalibrationReport = shape<CalibrationReport>('confidence report', {
  completions: isNumber,
  scored: isNumber,
  bands: listOf(isObject),
  events: listOf(isObject),
});

const isSizeLevel: Check = (field) => field === 'ok' || field === 'warning' || field === 'critical';
const repositorySizeEntry = shape<RepositorySizeEntry>(
  'repository size entry',
  { kind: isString, label: isString, locator: isString, status: isString, level: isSizeLevel },
  {
    connection_id: isString,
    provider: isString,
    git: shape('git object size', { total_bytes: isNumber, packed_bytes: isNumber, loose_bytes: isNumber }).check,
    attachments: shape('attachment size', { bytes: isNumber, files: isNumber, truncated: isBoolean }).check,
    shared_with_code: isBoolean,
    detail: isString,
  },
);
/** `GET /checkouts/{id}/repository/size` (HS2-QF44T7). */
export const parseRepositorySizeReport = shape<RepositorySizeReport>('repository size', {
  checkout_id: isString,
  thresholds: shape('size thresholds', { warning_bytes: isNumber, critical_bytes: isNumber }).check,
  level: isSizeLevel,
  entries: listOf(repositorySizeEntry.check),
});

const compactionCandidate = shape<CompactionPlan['candidates'][number]>('compaction candidate', {
  ticket_id: isString,
  ticket_slug: isString,
  attachment_id: isString,
  filename: isString,
  path: isString,
  bytes: isNumber,
  sha256: isString,
  reason: (value) => value === 'duplicate' || value === 'oversized',
});
export const parseCompactionPlan = shape<CompactionPlan>('compaction plan', {
  head: isString,
  branch: isString,
  min_age_days: isNumber,
  oversized_bytes: isNumber,
  candidates: listOf(compactionCandidate.check),
  reclaimable_payload_bytes: isNumber,
  fingerprint: isString,
});

export const parseCorruptTicket = shape<CorruptTicket>(
  'corrupt ticket',
  { store: isString, path: isString, error: isString },
  { store_path: isString, id: isString, slug: isString, error_code: isString },
);

const ticketUsageEvent = shape<TicketUsage['events'][number]>(
  'ticket usage event',
  { ts: isString, tool: isString, tokens_in: isNumber, tokens_out: isNumber },
  { model: isString, cost_usd: isNumber, session: isString },
);
export const parseTicketUsage = shape<TicketUsage>('ticket usage', {
  events: listOf(ticketUsageEvent.check),
  settled_cost_usd: isNumber,
  settled_through: isString,
});

export const parseDuplicateBacklinks = shape<DuplicateBacklinkResponse>('duplicate backlinks', {
  backlinks: listOf(isObject),
  inaccessible_projects: listOf(isObject),
});

export const parseFilePath = shape<{ path: string }>('file path', { path: isString });

const repositoryFile = shape<RepositoryFile>(
  'repository file',
  { path: isString },
  { original_path: isString, staged: isString, unstaged: isString, untracked: isBoolean, conflicted: isBoolean },
);
const commit = shape<CodeReviewCommit>(
  'commit',
  { sha: isString, short_sha: isString, subject: isString },
  { body: isString, committed_at: isString, refs: listOf(isObject), unmerged: isObject },
);

/** `repository/status|init|remote`: an uninitialized repository omits the counts, so every field is optional. */
export const parseRepositoryStatus = shape<RepositoryStatus>(
  'repository status',
  {},
  {
    initialized: isBoolean,
    branch: isString,
    ahead: isNumber,
    behind: isNumber,
    staged: isNumber,
    unstaged: isNumber,
    untracked: isNumber,
    conflicted: isNumber,
    files: listOf(repositoryFile.check),
    commits: listOf(commit.check),
    ranges: listOf(isObject),
  },
);

export const parseRepositoryFilePage = shape<RepositoryPage<RepositoryFile>>(
  'repository files',
  { items: listOf(repositoryFile.check) },
  { next_cursor: isNumber },
);

export const parseRepositoryCommitPage = shape<RepositoryPage<CodeReviewCommit>>(
  'repository commits',
  { items: listOf(commit.check) },
  { next_cursor: isNumber },
);

export const parseCodeReview = shape<CodeReview>(
  'code review',
  { commits: listOf(commit.check), ranges: listOf(isObject) },
  { difftool: isString, truncated: isBoolean, summary: isObject, files: listOf(isObject) },
);

const parsePermissionSecretSuggestion = shape<PermissionSecretSuggestion>('permission secret suggestion', {
  start: isNumber,
  end: isNumber,
  label: isString,
  placeholder: (field) => field === 'token' || field === 'quoted',
});

export const parsePermissionRequest = shape<PermissionRequest>(
  'permission request',
  { id: isNumber, connection: isString, tool: isString, action: isString },
  {
    project: isString,
    agent: isString,
    always_allow_supported: isBoolean,
    suggestions: listOf(parsePermissionSecretSuggestion.check),
  },
);

export const parsePermissionDecision = shape<{
  connection: string;
  decision: 'allow' | 'deny';
  persisted: boolean;
  also_resolved?: number[];
}>(
  'permission decision',
  { connection: isString, decision: (field) => field === 'allow' || field === 'deny' },
  { persisted: isBoolean, also_resolved: listOf(isNumber) },
);

/** A remembered permission answer (HS2-JNHAYB). */
export const parsePermissionRule = shape<PermissionRule>(
  'permission rule',
  {
    project: isString,
    tool: isString,
    action: isString,
    decision: (field) => field === 'allow' || field === 'deny',
    scope: (field) => field === 'chat' || field === 'session' || field === 'always',
  },
  {
    connection: isString,
    description: isString,
    exact_action: isString,
    match_type: (field) => field === 'exact' || field === 'template' || field === 'regex',
    pattern: (field) =>
      typeof field === 'object' &&
      field !== null &&
      !Array.isArray(field) &&
      (field as Record<string, unknown>).version === 1 &&
      ((field as Record<string, unknown>).kind === 'template' || (field as Record<string, unknown>).kind === 'regex') &&
      typeof (field as Record<string, unknown>).expression === 'string',
  },
);

export interface PermissionMigrationCandidate {
  id: string;
  tool: string;
  action: string;
  pattern?: PermissionPattern;
  description?: string;
  decision: 'allow' | 'deny';
}

export interface PermissionMigrationPreview {
  checkout: string;
  project_name: string;
  source: string;
  rules: PermissionMigrationCandidate[];
}

const parsePermissionMigrationCandidate = shape<PermissionMigrationCandidate>(
  'permission migration candidate',
  { id: isString, tool: isString, action: isString, decision: (field) => field === 'allow' || field === 'deny' },
  {
    description: isString,
    pattern: (field) =>
      typeof field === 'object' &&
      field !== null &&
      !Array.isArray(field) &&
      (field as Record<string, unknown>).version === 1 &&
      ((field as Record<string, unknown>).kind === 'template' || (field as Record<string, unknown>).kind === 'regex') &&
      typeof (field as Record<string, unknown>).expression === 'string',
  },
);

export const parsePermissionMigrationPreview = shape<PermissionMigrationPreview>('permission migration preview', {
  checkout: isString,
  project_name: isString,
  source: isString,
  rules: listOf(parsePermissionMigrationCandidate.check),
});
export const parsePermissionMigrationOutcome = shape<{ added: number; existing: number }>(
  'permission migration outcome',
  { added: isNumber, existing: isNumber },
);

export const parseToolConnection = shape<ToolConnection>(
  'tool connection',
  { id: isString, tool: isString, project: isString, role: isString, busy: isBoolean },
  {
    source: isString,
    actions: isStringArray,
    session_id: isString,
    last_error: isString,
    turn_id: isString,
    model: isString,
    effort: isString,
    permission_mode: isString,
    turn_started_at_ms: isNumber,
    last_event_at_ms: isNumber,
    in_flight_tool: isObject,
  },
);

export const parseMessageConnectionReceipt = shape<MessageConnectionReceipt>(
  'session message receipt',
  { delivered: isBoolean, mechanism: isString, confirmed: isBoolean },
  { session_id: isString },
);

export const parseAiToolDescriptor = shape<AiToolDescriptor>(
  'AI tool',
  { id: isString, display_name: isString, models: listOf(isObject) },
  {
    default_model: isString,
    default_effort: isString,
    actions: isStringArray,
    permission_modes: listOf(isObject),
    default_permission_mode: isString,
  },
);

export const parseAiToolDefaults = shape<AiToolDefaults>(
  'AI settings',
  { tool: isString },
  { model: isString, effort: isString, permission_mode: isString, providers: valuesOf(isObject) },
);

export const parseToolSession = shape<ToolSession>('tool session', {
  connection_id: isString,
  tool: isString,
  project: isString,
  session_id: isString,
});

const terminalOptional: Record<string, Check> = {
  kind: isString,
  tool: isString,
  cwd: isString,
  name: isString,
  progress: isNumber,
  halt: isObject,
  question: isObject,
  ai_connection: isObject,
  last_hook_report: isObject,
  mcp_connected: isBoolean,
};

export const parseTerminalInfo = shape<TerminalInfo>(
  'terminal',
  { id: isString, alive: isBoolean, busy: isBoolean },
  terminalOptional,
);

export const parseTerminalRead = shape<TerminalRead>(
  'terminal',
  { id: isString, alive: isBoolean, busy: isBoolean, scrollback: isString },
  terminalOptional,
);

export const parseTerminalName = shape<TerminalName>('terminal name', { id: isString }, { name: isString });

export const parseTerminalSettings = shape<TerminalSettings>('terminal settings', {
  inherit_global_shell_history: isBoolean,
});

/** `GET`/`PUT /terminal-limits` (HS2-Y5DYC0). */
export const parseTerminalLimits: ResponseParser<TerminalLimits> = (value) => {
  const what = 'terminal limits',
    body = record(value, what),
    keys = ['history_lines', 'replay_bytes', 'warm_terminals'] as const;
  optionalOf(body, 'physical_memory_bytes', what, isNumber, true);
  for (const group of ['defaults', 'effective', 'configured'] as const) {
    const values = record(body[group], what);
    for (const key of keys)
      if (group === 'configured') optionalOf(values, key, what, isNumber, true);
      else if (!isNumber(values[key])) invalid(what);
  }
  return body as unknown as TerminalLimits;
};

export const parseTrashSettings = shape<TrashSettings>('trash settings', { trash_cleanup_days: isNumber });
export const parseTrashPurgePreview = shape<TrashPurgePreview>('trash purge preview', {
  trash_cleanup_days: isNumber,
  purge_count: isNumber,
});

export const parseCommandDefinition = shape<CommandDefinition>(
  'command',
  { id: isString, title: isString },
  { kind: isString, program: isString, args: isStringArray, group: isString, command: isString, prompt: isString },
);

export const parseCustomView = shape<CustomView>('view', { id: isString, name: isString, query: isString });

export const parseCommandRun = shape<CommandRun>(
  'command run',
  { id: isString, command_id: isString, state: isString, output: listOf(isObject) },
  { exit_code: isNumber },
);

export const parsePollResponse = shape<PollResponse>('change poll', {
  cursor: isNumber,
  events: listOf((event) => isRecord(event) && isString(event.kind)),
  overflow: isBoolean,
});
