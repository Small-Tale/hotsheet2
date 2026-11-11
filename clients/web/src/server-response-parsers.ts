import type {
  AdvisoryClaimStatus,
  AiToolDefaults,
  AiToolDescriptor,
  CalibrationReport,
  Checkout,
  CheckoutActivityEvent,
  CheckoutAnalytics,
  CheckoutSource,
  CheckoutTicketPage,
  CheckoutTicketRowsPage,
  ClientNotification,
  CodeReview,
  CodeReviewCommit,
  CommandDefinition,
  CommandRun,
  CompactionPlan,
  CompactionResult,
  CorruptTicket,
  CustomView,
  DevelopmentStrategyReport,
  DuplicateBacklinkResponse,
  FeedbackDelivery,
  FollowUpEvidence,
  FullTicket,
  GitHubAuthStart,
  GitHubAuthStatus,
  GitHubRepositoryAccess,
  GuidanceBatchReview,
  GuidanceInterviewReview,
  GuidanceOverview,
  GuidanceRecommendationReport,
  GuidanceRulePreview,
  GuidanceSourceReview,
  GuidanceSourceUpdate,
  LiveHistoryList,
  LiveStateList,
  LiveStoreState,
  MessageConnectionReceipt,
  NoteActor,
  NoteDoctorReport,
  NotificationChannel,
  NotificationChannelRemoval,
  NotificationConfig,
  NotificationDelivery,
  NotificationProjectTiming,
  NotificationQuietHours,
  NotificationRule,
  NotificationRuleFilter,
  NotifierDescriptor,
  NotifierList,
  NotifierSettingField,
  PermissionRequest,
  PhaseReport,
  PollResponse,
  ProjectTicketRef,
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
  TicketStoreSyncHealth,
  TicketStoreSyncReport,
  TicketUsage,
  ToolConnection,
  ToolSession,
  TrashPurgePreview,
  TrashSettings,
} from './api';
import type { CompatibilityRange, ServerCompatibility } from './compatibility';
import { isRecord } from './json-value';
import type { PermissionAutomation, PermissionRule, PermissionSecretSuggestion } from './permission-notifications';

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

/** Shared development policy with resolved provenance (HS2-NBDXPW). */
export const parseDevelopmentStrategyReport: ResponseParser<DevelopmentStrategyReport> = (value) => {
  const what = 'development strategy',
    body = record(value, what),
    policy = record(body.policy, what),
    effective = record(body.effective, what),
    presets = list(body.presets, what, (entry) => entry);
  const presetNames = ['ticket-assured', 'steady-main', 'risk-guided'],
    integrationNames = ['serial', 'batch', 'ci-staged'],
    localGateNames = ['full-local', 'targeted', 'risk-selected'];
  if (
    !Number.isInteger(policy.version) ||
    !presetNames.includes(String(policy.default)) ||
    !Number.isInteger(policy.green_main_goal_minutes) ||
    (policy.default_custom !== undefined && typeof policy.default_custom !== 'string') ||
    !presets.every((entry) => presetNames.includes(String(entry))) ||
    !isRecord(policy.custom ?? {}) ||
    typeof effective.preset !== 'string' ||
    !['unconfigured-project', 'project', 'session', 'ticket'].includes(String(effective.source)) ||
    !integrationNames.includes(String(effective.integration)) ||
    !localGateNames.includes(String(effective.local_gate)) ||
    typeof effective.require_green_main_sha !== 'boolean' ||
    typeof effective.attribute_red_ci_before_completion !== 'boolean' ||
    (effective.green_main_goal_minutes !== null &&
      effective.green_main_goal_minutes !== undefined &&
      !Number.isInteger(effective.green_main_goal_minutes))
  )
    invalid(what);
  const custom = (entry: unknown) => {
    const item = record(entry, what);
    if (
      !integrationNames.includes(String(item.integration)) ||
      !localGateNames.includes(String(item.local_gate)) ||
      !['T0', 'T1', 'T2'].includes(String(item.minimum_tier)) ||
      (item.budget_minutes !== undefined && !Number.isInteger(item.budget_minutes)) ||
      typeof item.browser !== 'boolean' ||
      typeof item.platform !== 'boolean' ||
      typeof item.perf !== 'boolean' ||
      !['before-work', 'before-handoff'].includes(String(item.test_plan_timing)) ||
      typeof item.ci_gate !== 'string' ||
      (item.staging_branch !== undefined && typeof item.staging_branch !== 'string') ||
      (item.green_main_goal_minutes !== undefined && !Number.isInteger(item.green_main_goal_minutes)) ||
      !['attribute-and-retry', 'fallback-to-serial'].includes(String(item.red_ci_recovery))
    )
      invalid(what);
    return item;
  };
  const profiles = Object.fromEntries(
    Object.entries(policy.custom ?? {}).map(([name, entry]) => [name, custom(entry)]),
  );
  if (effective.custom !== undefined) custom(effective.custom);
  return {
    ...body,
    policy: { ...policy, custom: profiles },
    effective,
    presets,
  } as unknown as DevelopmentStrategyReport;
};

export const parseGuidanceOverview: ResponseParser<GuidanceOverview> = (value) => {
  const what = 'guidance',
    body = record(value, what);
  if (body.owner !== 'builtin' && body.owner !== 'rulesync') invalid(what);
  if (body.pending_consolidation_review !== undefined && typeof body.pending_consolidation_review !== 'boolean')
    invalid(what);
  if (typeof body.explicit_selection !== 'boolean') invalid(what);
  requiredString(body, 'default_repository', what);
  if (body.local_rules !== undefined && !isStringArray(body.local_rules)) invalid(what);
  const sources = list(body.sources, what, (entry) => {
    const source = record(entry, what);
    for (const key of ['id', 'repository', 'requested_ref', 'commit']) requiredString(source, key, what);
    if (source.group_id !== undefined && source.group_id !== null && typeof source.group_id !== 'string') invalid(what);
    const rules = list(source.rules, what, (entry) => {
      const rule = record(entry, what);
      for (const key of ['name', 'description']) requiredString(rule, key, what);
      if (
        typeof rule.version !== 'number' ||
        (rule.installed_version !== null && typeof rule.installed_version !== 'number') ||
        (rule.owner !== null && typeof rule.owner !== 'string') ||
        !isStringArray(rule.tags) ||
        typeof rule.selected !== 'boolean' ||
        (rule.chosen_version !== undefined && typeof rule.chosen_version !== 'number') ||
        (rule.pinned_sha256 !== undefined && typeof rule.pinned_sha256 !== 'string') ||
        (rule.chosen_sha256 !== undefined && typeof rule.chosen_sha256 !== 'string') ||
        (rule.chosen_commit !== undefined && typeof rule.chosen_commit !== 'string') ||
        (rule.compatible !== undefined && typeof rule.compatible !== 'boolean') ||
        (rule.minimum_hotsheet_version !== undefined &&
          rule.minimum_hotsheet_version !== null &&
          typeof rule.minimum_hotsheet_version !== 'string')
      )
        invalid(what);
      if (rule.historical_versions !== undefined)
        list(rule.historical_versions, what, (entry) => {
          const revision = record(entry, what);
          if (
            typeof revision.version !== 'number' ||
            typeof revision.sha256 !== 'string' ||
            typeof revision.commit !== 'string' ||
            typeof revision.compatible !== 'boolean' ||
            (revision.minimum_hotsheet_version !== null && typeof revision.minimum_hotsheet_version !== 'string')
          )
            invalid(what);
          return revision;
        });
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

export const parseGuidanceBatchReview: ResponseParser<GuidanceBatchReview> = (value) => {
  const what = 'guidance batch review';
  const body = record(value, what);
  const requests = list(body.requests, what, (entry) => {
    const source = record(entry, what);
    for (const key of ['id', 'repository']) requiredString(source, key, what);
    if (!isStringArray(source.refs)) invalid(what);
    return source as unknown as GuidanceBatchReview['requests'][number];
  });
  const pins = list(body.pins, what, (entry) => {
    const pin = record(entry, what);
    for (const key of ['id', 'group_id', 'repository', 'requested_ref', 'commit']) requiredString(pin, key, what);
    if (typeof pin.rules !== 'number' || typeof pin.skills !== 'number') invalid(what);
    return pin as unknown as GuidanceBatchReview['pins'][number];
  });
  return { requests, pins };
};

export const parseGuidanceRulePreview: ResponseParser<GuidanceRulePreview> = (value) => {
  const what = 'guidance rule preview',
    body = record(value, what);
  if (body.owner !== 'builtin' && body.owner !== 'rulesync') invalid(what);
  if (!['add', 'replace', 'current', 'protected'].includes(String(body.outcome))) invalid(what);
  if (body.current !== null && typeof body.current !== 'string') invalid(what);
  if (body.after !== null && typeof body.after !== 'string') invalid(what);
  requiredString(body, 'pinned', what);
  return body as unknown as GuidanceRulePreview;
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
  list(body.other_refs, what, (entry) => {
    const ref = record(entry, what);
    requiredString(ref, 'reference', what);
    requiredString(ref, 'commit', what);
    return ref;
  });
  if (typeof body.other_refs_truncated !== 'boolean') invalid(what);
  if (body.other_refs_reason !== undefined && typeof body.other_refs_reason !== 'string') invalid(what);
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

/** `GET /checkouts/{id}/groups` (HS2-PDSHNN). */
export const parseGroupList: ResponseParser<{ groups: Array<{ name: string; count: number }> }> = (value) => {
  const body = record(value, 'group list');
  if (
    !Array.isArray(body.groups) ||
    !body.groups.every(
      (group: unknown) =>
        typeof group === 'object' &&
        group !== null &&
        typeof (group as { name?: unknown }).name === 'string' &&
        typeof (group as { count?: unknown }).count === 'number',
    )
  )
    invalid('group list');
  return body as { groups: Array<{ name: string; count: number }> };
};

/** `POST /checkouts/{id}/groups/rename` (HS2-8CDFKR). */
export const parseGroupRename: ResponseParser<{ name: string; updated: number }> = (value) => {
  const body = record(value, 'group rename');
  if (typeof body.name !== 'string' || typeof body.updated !== 'number') invalid('group rename');
  return body as { name: string; updated: number };
};

/** `POST /checkouts/{id}/groups/ungroup` (HS2-8CDFKR). */
export const parseGroupUngroup: ResponseParser<{ updated: number }> = (value) => {
  const body = record(value, 'group ungroup');
  if (typeof body.updated !== 'number') invalid('group ungroup');
  return body as { updated: number };
};

/* ---- Declarative DTO shapes (HS2-E0AG5E) -------------------------------------------------
 * Each shape names the fields callers rely on. Required fields must have the stated type;
 * optional fields may be absent or `null` (the Rust DTOs serialize `Option::None` as `null`
 * unless skipped) but must otherwise have the stated type; omitted fields are the skipped ones,
 * absent or the stated type but never `null`. Unlisted fields pass through. */

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

function shape<T>(
  what: string,
  required: Record<string, Check>,
  optional: Record<string, Check> = {},
  // HS2-FE0HBN: optional members the Rust DTO skips when unset (`skip_serializing_if`), so the
  // server never sends them as `null`: absent, or the stated type.
  omitted: Record<string, Check> = {},
): Shape<T> {
  const check: Check = (value) =>
    isRecord(value) &&
    Object.entries(required).every(([key, field]) => field(value[key])) &&
    Object.entries(optional).every(
      ([key, field]) => value[key] === undefined || value[key] === null || field(value[key]),
    ) &&
    Object.entries(omitted).every(([key, field]) => value[key] === undefined || field(value[key]));
  const parse = (value: unknown) => (check(value) ? (value as T) : invalid(what));
  const parseList = (value: unknown) => (listOf(check)(value) ? (value as T[]) : invalid(`${what} list`));
  return Object.assign(parse, { check, list: parseList });
}

const project = shape<{ id: string; alias: string }>('project', { id: isString, alias: isString });

export const parseProviderDescriptor = shape<ProviderDescriptor>(
  'provider',
  { connection_id: isString, provider: isString, display_name: isString, locator: isString, capabilities: isObject },
  { default: isBoolean, color: isString, ticket_prefix: isString },
);

/** `PUT /checkouts/{reference}/sources/{connection_id}/ticket-prefix` (HS2-8QJ6W3). */
export const parseTicketPrefix = shape<{ ticket_prefix: string }>('ticket prefix', { ticket_prefix: isString });

const advisoryClaim = shape<AdvisoryClaimStatus['projection']['claims'][number]>('advisory claim', {
  worker: isString,
  operation_id: isString,
  expires_unix_ms: isNumber,
});
const advisoryProjection = shape<AdvisoryClaimStatus['projection']>('advisory claim projection', {
  claims: listOf(advisoryClaim.check),
  conflicted: isBoolean,
  invalid_markers: isNumber,
});
const pendingAdvisoryOperation = shape<AdvisoryClaimStatus['pending_operations'][number]>(
  'pending advisory operation',
  {
    operation_id: isString,
    state: (value) => isString(value) && ['queued', 'confirmed', 'needs_attention', 'expired'].includes(value),
  },
);
export const parseAdvisoryClaimStatus = shape<AdvisoryClaimStatus>('advisory claim status', {
  projection: advisoryProjection.check,
  provisional: isBoolean,
  remote_verified: isBoolean,
  automatic_handoff_safe: isBoolean,
  pending_operation_ids: isStringArray,
  pending_operations: listOf(pendingAdvisoryOperation.check),
});

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

/** `GET /accounts`: the Rust `Account` always sends `host` and `managed` and omits unset `base_url`/`identity` (HS2-FE0HBN). */
export const parseProviderAccount = shape<ProviderAccount>(
  'account',
  {
    id: isString,
    provider: isString,
    host: isString,
    managed: isBoolean,
    sources: listOf(isObject),
    projects: listOf(project.check),
  },
  {},
  { base_url: isString, identity: isString },
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

const projectTicketRef = shape<ProjectTicketRef>('project ticket reference', {
  project_id: isString,
  connection_id: isString,
  native_id: isString,
});

/** A recorded actor such as a ticket's creator (HS2-9FPHWY): a role and an optional stable id. */
const actor = shape<NoteActor>('actor', { role: isString }, { id: isString });

const rowOptional: Record<string, Check> = {
  id: isString,
  title: isString,
  status: isString,
  up_next: isBoolean,
  feedback_needed: isBoolean,
  needs_review: isBoolean,
  tags: isStringArray,
  blocked_by: isStringArray,
  follow_ups: listOf(projectTicketRef.check),
  claim_count: isNumber,
  created_by: actor.check,
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
  follow_up_link_error: isString,
  warnings: isStringArray,
});

/**
 * A checkout-scoped full ticket, which names its source `store` when the route knows it; the
 * field is type-checked when present but not required, as before this parser existed.
 */
export const parseFeedbackDelivery: ResponseParser<FeedbackDelivery> = (value) => {
  if (!isRecord(value) || typeof value.note_id !== 'string') return invalid('feedback delivery');
  if ((value.status === 'sent' || value.status === 'queued') && typeof value.connection_id === 'string')
    return value as FeedbackDelivery;
  if (value.status === 'start' || value.status === 'not_shared') return value as FeedbackDelivery;
  if (
    value.status === 'choose' &&
    Array.isArray(value.sessions) &&
    value.sessions.every(
      (session) => isRecord(session) && typeof session.id === 'string' && typeof session.tool === 'string',
    )
  )
    return value as FeedbackDelivery;
  return invalid('feedback delivery');
};

export const parseStoredTicket = shape<FullTicket & { store: string; feedback_delivery?: FeedbackDelivery }>(
  'ticket',
  fullTicketRequired,
  {
    ...rowOptional,
    follow_up_link_error: isString,
    store: isString,
    warnings: isStringArray,
    feedback_delivery: (value) => {
      try {
        parseFeedbackDelivery(value);
        return true;
      } catch {
        return false;
      }
    },
  },
);

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

const numericMap: Check = (field) =>
  isObject(field) &&
  Object.values(field as Record<string, unknown>).every((value) => typeof value === 'number' && Number.isFinite(value));
const durationSummary = shape<CheckoutAnalytics['stores'][number]['ticket_flow']['cycle_time_seconds']>(
  'cycle-time summary',
  { samples: isNumber, average: isNumber, p50: isNumber, p95: isNumber },
);
const ticketFlowSummary = shape<CheckoutAnalytics['stores'][number]['ticket_flow']>('ticket-flow summary', {
  total: isNumber,
  open: isNumber,
  completed: isNumber,
  current_by_status: numericMap,
  current_by_category: numericMap,
  throughput_by_day: numericMap,
  cycle_time_seconds: accepts(durationSummary),
  historical_cumulative_flow_available: isBoolean,
});
const usageRollup = shape<CheckoutAnalytics['stores'][number]['usage']>('usage rollup', {
  events: isNumber,
  tokens_in: isNumber,
  tokens_out: isNumber,
  cost_usd: isNumber,
  by_model: numericMap,
  by_ticket: numericMap,
  by_day: numericMap,
});
const analyticsStore = shape<CheckoutAnalytics['stores'][number]>('analytics store', {
  store_id: isString,
  ticket_flow: accepts(ticketFlowSummary),
  usage: accepts(usageRollup),
});
const unavailableAnalyticsSource = shape<CheckoutAnalytics['unavailable_sources'][number]>(
  'unavailable analytics source',
  { connection_id: isString, provider: isString },
);
export const parseCheckoutAnalytics = shape<CheckoutAnalytics>('checkout analytics', {
  stores: listOf(analyticsStore.check),
  unavailable_sources: listOf(unavailableAnalyticsSource.check),
});

/* ---- Live state (HS2-JTTQMY) ----------------------------------------------------------- */
const liveWords = new Set(['active', 'stopped', 'may_be_stuck', 'stalled', 'halted', 'waiting_on_you']);
const isLiveWord: Check = (field) => typeof field === 'string' && liveWords.has(field);
const isNullableNumber: Check = (field) => field === null || isNumber(field);
const isNullableString: Check = (field) => field === null || isString(field);
const liveClaim = shape('live claim', {
  ticket: isString,
  slug: isString,
  title: isString,
  worker: isString,
  phase: isNullableString,
  claimed_at_ms: isNullableNumber,
  lease_expires_at_ms: isNumber,
  eta_at_ms: isNullableNumber,
});
const liveActivity = shape('live activity', {
  word: isLiveWord,
  active_since_ms: isNullableNumber,
  stopped_at_ms: isNullableNumber,
  last_progress_ms: isNullableNumber,
  idle_deadline_ms: isNullableNumber,
  stalled: listOf(shape('live stalled claim', { slug: isString, worker: isString }).check),
  halted: listOf(shape('live halt', { worker: isString, message: isString, at_ms: isNumber }).check),
  causes: listOf(
    shape('live cause', {
      kind: (field) => field === 'usage_limit' || field === 'awaiting_input',
      worker: isString,
      terminal: isNullableString,
      since_ms: isNumber,
      resets_at_ms: isNullableNumber,
      prompt: (field) => field === null || field === 'permission' || field === 'question',
    }).check,
  ),
});
const liveWaiting = shape('live waiting', {
  count: isNumber,
  tickets: listOf(
    shape('live waiting ticket', {
      ticket: isString,
      slug: isString,
      title: isString,
      reason: (field) => field === 'feedback' || field === 'review',
    }).check,
  ),
});
const liveRates = shape('live rates', {
  tokens: shape('live token rate', {
    window_minutes: isNumber,
    minutes: listOf(shape('live token minute', { start_ms: isNumber, tokens: isNumber }).check),
    peak_per_minute: isNumber,
  }).check,
  tickets_updated: shape('live updated rate', {
    window_minutes: isNumber,
    at_ms: listOf(isNumber),
    peak_per_hour: isNumber,
  }).check,
});
export const parseLiveStoreState = shape<LiveStoreState>('live state', {
  store: isString,
  root: isString,
  name: isString,
  checkouts: isStringArray,
  epoch_ms: isNumber,
  revision: isNumber,
  state: isLiveWord,
  activity: liveActivity.check,
  claims: listOf(liveClaim.check),
  workers: shape('live workers', { live: isNumber, expected: isNumber }).check,
  waiting: liveWaiting.check,
  // An older server still sends `runway` (removed by HS2-5A83MR); extra members are ignored.
  rates: liveRates.check,
});
export const parseLiveStateList = shape<LiveStateList>('live state list', {
  epoch_ms: isNumber,
  stores: listOf(parseLiveStoreState.check),
});
const isHistoryState: Check = (field) => field === 'waiting_on_ci' || isLiveWord(field);
export const parseLiveHistoryList = shape<LiveHistoryList>('live history', {
  since_ms: isNumber,
  until_ms: isNumber,
  stores: listOf(
    shape('live history store', {
      store: isString,
      name: isString,
      checkouts: isStringArray,
      segments: listOf(
        shape('live history segment', { state: isHistoryState, start_ms: isNumber, end_ms: isNumber }).check,
      ),
      completions: listOf(shape('live history completion', { slug: isString, at_ms: isNumber }).check),
    }).check,
  ),
});

export const parseCheckoutActivity: ResponseParser<CheckoutActivityEvent[]> = (value) =>
  list(value, 'checkout activity', (entry) => {
    const item = record(entry, 'checkout activity entry');
    requiredString(item, 'store_id', 'checkout activity entry');
    const event = record(item.event, 'activity event');
    for (const key of ['id', 'ts', 'tool', 'kind', 'summary']) requiredString(event, key, 'activity event');
    if (!['low', 'normal', 'high'].includes(String(event.importance))) invalid('activity event');
    for (const key of ['project', 'ticket', 'session']) optionalOf(event, key, 'activity event', isString);
    return entry as CheckoutActivityEvent;
  });

export const parseCurrentDiff: ResponseParser<{ diff: string; truncated: boolean }> = (value) => {
  const item = record(value, 'current diff');
  requiredString(item, 'diff', 'current diff');
  if (typeof item.truncated !== 'boolean') invalid('current diff');
  return item as { diff: string; truncated: boolean };
};

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

const TICKET_STORE_SYNC_STATES = new Set([
  'not_repository',
  'no_remote',
  'pending',
  'up_to_date',
  'synced',
  'offline',
  'rejected',
  'conflict',
  'rebase_in_progress',
  'history_recovery_required',
]);
const ticketStoreSyncHealth = shape<TicketStoreSyncHealth>(
  'ticket store sync health',
  {
    state: (field) => typeof field === 'string' && TICKET_STORE_SYNC_STATES.has(field),
    syncing: isBoolean,
    unpublished_commits: isNumber,
    incoming_commits: isNumber,
    consecutive_failures: isNumber,
  },
  {},
  {
    upstream: isString,
    last_attempt_at: isString,
    last_success_at: isString,
    reason: isString,
    oversized: listOf(shape('oversized object', { path: isString, bytes: isNumber, limit_bytes: isNumber }).check),
    next_action: isString,
  },
);
/** `GET /checkouts/{id}/repository/sync` (HS2-ADHFKZ). */
export const parseTicketStoreSyncReport = shape<TicketStoreSyncReport>('ticket store sync', {
  checkout_id: isString,
  stores: listOf(
    shape('ticket store sync entry', {
      connection_id: isString,
      label: isString,
      locator: isString,
      health: ticketStoreSyncHealth.check,
    }).check,
  ),
});

const compactionCandidate = shape<CompactionPlan['candidates'][number]>('compaction candidate', {
  ticket_id: isString,
  ticket_slug: isString,
  attachment_id: isString,
  filename: isString,
  path: isString,
  bytes: isNumber,
  sha256: isString,
  reason: (value) => value === 'duplicate' || value === 'oversized' || value === 'orphaned',
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
export const parseCompactionResult = shape<CompactionResult>('compaction result', {
  old_head: isString,
  new_head: isString,
  backup: isString,
  candidates_removed: isNumber,
  git_bytes_before: isNumber,
  git_bytes_after: isNumber,
  recovery: listOf(isString),
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

const noteDoctorFinding = shape<NoteDoctorReport['findings'][number]>('note doctor finding', {
  id: isString,
  note_id: isString,
  description: isString,
  proposed_operation: isString,
});
export const parseNoteDoctorReport = shape<NoteDoctorReport>('note doctor report', {
  ticket_id: isString,
  findings: listOf(noteDoctorFinding.check),
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

const followUpEvidence = shape<FollowUpEvidence>(
  'follow-up evidence',
  {
    target: projectTicketRef.check,
    state: (value) =>
      value === 'resolved' ||
      value === 'missing' ||
      value === 'deleted' ||
      value === 'unavailable' ||
      value === 'invalid',
  },
  { slug: isString, title: isString, native_url: isString },
);

export const parseCodeReview = shape<CodeReview>(
  'code review',
  { commits: listOf(commit.check), ranges: listOf(isObject) },
  {
    checkout_behind_main: isBoolean,
    difftool: isString,
    truncated: isBoolean,
    summary: isObject,
    files: listOf(isObject),
    follow_ups: listOf(followUpEvidence.check),
  },
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

export interface LegacyPermissionPreview {
  checkout: string;
  project_name: string;
  source: string;
  count: number;
}

export const parseLegacyPermissionPreview = shape<LegacyPermissionPreview>('legacy permission preview', {
  checkout: isString,
  project_name: isString,
  source: isString,
  count: isNumber,
});
export const parseLegacyPermissionDiscardOutcome = shape<{ discarded: number }>('legacy permission discard outcome', {
  discarded: isNumber,
});

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
  launch_cwd: isString,
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

/** Project-local automatic permission choice (HS2-QAGSJK). */
export const parsePermissionAutomationSettings = shape<PermissionAutomation & { configured: boolean }>(
  'permission automation settings',
  {
    configured: isBoolean,
    action: (value): value is PermissionAutomation['action'] =>
      value === 'off' || value === 'allow' || value === 'deny',
    delayMs: isNumber,
  },
);

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

/* ---- Notifier plugins (HS2-DPBJJA) ---------------------------------------------------- */

const isSettingType: Check = (field) =>
  typeof field === 'string' && ['string', 'list', 'number', 'bool', 'choice', 'secret'].includes(field);
const parseNotifierSettingField = shape<NotifierSettingField>(
  'notifier setting',
  { key: isString, label: isString, type: isSettingType, required: isBoolean },
  { choices: isStringArray, help: isString },
);
const parseNotifierDescriptor = shape<NotifierDescriptor>('notifier', {
  id: isString,
  display_name: isString,
  description: isString,
  capabilities: (value) => isRecord(value) && isStringArray(value.formats) && isNumber(value.max_body_bytes),
  settings: listOf(parseNotifierSettingField.check),
  source: (value) => isRecord(value) && (value.kind === 'built_in' || value.kind === 'external'),
});
export const parseNotifierList = shape<NotifierList>('notifier list', {
  notifiers: listOf(parseNotifierDescriptor.check),
  load_errors: isStringArray,
});
const parseQuietHours = shape<NotificationQuietHours>(
  'quiet hours',
  { start: isString, end: isString },
  { time_zone: isString, utc_offset: isString, allow_errors: isBoolean },
);
export const parseNotificationChannel = shape<NotificationChannel>(
  'notification channel',
  { id: isString, notifier: isString, enabled: isBoolean, settings: isObject },
  { label: isString, plugin_digest: isString, quiet_hours: parseQuietHours.check, max_per_hour: isNumber },
);
const isSeverity: Check = (field) =>
  field === 'info' || field === 'success' || field === 'warning' || field === 'error';
const viewRef = shape<{ checkout: string; view: string }>('saved view', { checkout: isString, view: isString });
const parseRuleFilter = shape<NotificationRuleFilter>(
  'notification rule filter',
  {},
  {
    projects: isStringArray,
    tags: isStringArray,
    statuses: isStringArray,
    categories: isStringArray,
    priorities: isStringArray,
    min_severity: isSeverity,
    text: isString,
    views: listOf(viewRef.check),
  },
);
export const parseNotificationRule = shape<NotificationRule>(
  'notification rule',
  { id: isString, events: isStringArray, channels: isStringArray, enabled: isBoolean, cooldown_seconds: isNumber },
  { filter: parseRuleFilter.check },
);
export const parseNotificationProjectTiming = shape<NotificationProjectTiming>(
  'notification project timing',
  {},
  { grace_seconds: isNumber, idle_minutes: isNumber },
);
export const parseNotificationConfig = shape<NotificationConfig>(
  'notification config',
  {
    version: isNumber,
    channels: listOf(parseNotificationChannel.check),
    rules: listOf(parseNotificationRule.check),
  },
  { link_base: isString, project_activity: parseNotificationProjectTiming.check },
);
export const parseClientNotification: ResponseParser<ClientNotification> = (value) => {
  const what = 'notification';
  const body = record(value, what);
  for (const key of ['id', 'message', 'severity']) requiredString(body, key, what);
  for (const key of ['checkout', 'store', 'ticket', 'recipient', 'dedupe_key'])
    if (body[key] !== null && typeof body[key] !== 'string') invalid(what);
  if (typeof body.created_at_ms !== 'number' || !Number.isFinite(body.created_at_ms)) invalid(what);
  if (typeof body.acknowledged !== 'boolean') invalid(what);
  return body as unknown as ClientNotification;
};
export const parseClientNotifications: ResponseParser<ClientNotification[]> = (value) =>
  list(value, 'notifications', parseClientNotification);
export const parseNotificationDelivery = shape<NotificationDelivery>(
  'notification delivery',
  {
    id: isString,
    at: isString,
    event: isString,
    channel: isString,
    notifier: isString,
    outcome: (field) => field === 'delivered' || field === 'failed' || field === 'suppressed',
    attempts: isNumber,
    truncated: isBoolean,
    omitted_attachments: isNumber,
  },
  { rule: isString, error: isString, provider_id: isString, project: isString, ticket: isString },
);
export const parseNotificationChannelRemoval = shape<NotificationChannelRemoval>('notification channel removal', {
  removed: isBoolean,
  removed_rules: isStringArray,
  deleted_secrets: isStringArray,
});

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

const parsePhaseStats = shape<PhaseReport['phases'][number]>('phase stats', {
  phase: isString,
  wall: (value) => isRecord(value) && isNumber(value.median_ms),
});

export const parsePhaseReport = shape<PhaseReport>('phase report', { phases: listOf(parsePhaseStats.check) });

const parseCommandLaunch = shape<NonNullable<CommandRun['launch']>>(
  'command launch',
  { kind: (value) => value === 'shell' || value === 'ai' },
  { ticket_id: isString, ticket_slug: isString, tool: isString, terminal_id: isString, terminal_label: isString },
);

export const parseCommandRun = shape<CommandRun>(
  'command run',
  { id: isString, command_id: isString, state: isString, output: listOf(isObject) },
  { exit_code: isNumber, started_at: isString, launch: parseCommandLaunch.check },
);

export const parsePollResponse = shape<PollResponse>('change poll', {
  cursor: isNumber,
  events: listOf((event) => isRecord(event) && isString(event.kind)),
  overflow: isBoolean,
});
