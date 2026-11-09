import type { TicketRow } from './api';
import { ServerResponseShapeError } from './server-response-parsers';

export interface WorkerTicketRef {
  connection_id: string;
  native_id: string;
}

export interface WorkerTicket {
  ticket: WorkerTicketRef;
  slug: string | null;
  assignment_id: string | null;
  assignment_phase:
    | 'offered'
    | 'accepted'
    | 'handoff'
    | 'verifying'
    | 'integrating'
    | 'suspended_integration'
    | 'ci'
    | 'done'
    | 'blocked'
    | null;
  claim_expires_at: number | null;
  started_phase: string | null;
  eta_at: string | null;
}

export interface PlannedWorkerTicket {
  entry: { operation: string; worker: string; generation: string; ticket: WorkerTicketRef; slug: string };
  reasons: string[];
}

/** HS2-6WFCZR: a dispatcher-recorded refusal of one exact worker request. */
export interface WorkerRequestRefusal {
  slug: string;
  reason: string;
  detail: string | null;
}

/** HS2-6WFCZR: a worker session's stated preferences; empty lists when none are stated. */
export interface WorkerPreferences {
  prefer: string[];
  only: string[];
  avoid: string[];
  request: string | null;
  refusal: WorkerRequestRefusal | null;
}

/** HS2-6WFCZR: the dispatcher-facing match of one worker's preferences against the queue. */
export interface WorkerMatching {
  candidates: Array<{ slug: string; preferred: boolean }>;
  idle_reason: string | null;
  request: { slug: string; reasons: string[] } | null;
}

export interface WorkerPreferenceChip {
  kind: 'only' | 'prefer' | 'avoid';
  filter: string;
  label: string;
}

export interface WorkerRow {
  id: string;
  role: 'integrator' | 'dispatcher' | 'worker' | null;
  generation: string | null;
  parent: string | null;
  label: string;
  tool: string | null;
  connection_kind: string | null;
  connection_id: string | null;
  connection_available: boolean | null;
  observed_alive: boolean | null;
  process_busy: boolean | null;
  availability: 'live' | 'disconnected' | 'unresolved_claim_holder' | 'unbound_connection';
  lease_expires_at: number | null;
  tickets: WorkerTicket[];
  planned: PlannedWorkerTicket[];
  /** HS2-NEY1QH: the recorded strategy of an integrator session; absent from older servers. */
  integration_strategy?: string | null;
  /** HS2-6WFCZR: absent from older servers and for workers that state none. */
  preferences?: WorkerPreferences | null;
  matching?: WorkerMatching | null;
}

/** HS2-83KFB5: why a ticket source's workers are not shown. */
export type WorkerCoverageReason = 'unsupported' | 'failed_read' | 'unobserved';

/** One uncovered ticket source, keyed by connection id (HS2-83KFB5). */
export interface WorkerCoverageGap {
  connection_id: string;
  name: string;
  provider: string;
  reason: WorkerCoverageReason;
  details: string[];
}

export interface WorkerView {
  cursor: string;
  project_id: string;
  source_id: string;
  workers: WorkerRow[];
  uncovered_sources: string[];
  /** Absent from servers older than HS2-83KFB5; `uncovered_sources` is the fallback. */
  coverage_gaps?: WorkerCoverageGap[];
  connection_coverage: boolean;
}

function record(value: unknown): Record<string, unknown> {
  if (value && typeof value === 'object' && !Array.isArray(value)) return value as Record<string, unknown>;
  throw new ServerResponseShapeError('worker view');
}
function string(value: unknown): string {
  if (typeof value === 'string') return value;
  throw new ServerResponseShapeError('worker view');
}
function nullableString(value: unknown): string | null {
  return value === null ? null : string(value);
}
function nullableNumber(value: unknown): number | null {
  if (value === null) return null;
  if (typeof value === 'number' && Number.isFinite(value)) return value;
  throw new ServerResponseShapeError('worker view');
}
function nullableBoolean(value: unknown): boolean | null {
  if (value === null || typeof value === 'boolean') return value;
  throw new ServerResponseShapeError('worker view');
}
function array<T>(value: unknown, parse: (item: unknown) => T): T[] {
  if (!Array.isArray(value)) throw new ServerResponseShapeError('worker view');
  return value.map(parse);
}
function ticketRef(value: unknown): WorkerTicketRef {
  const item = record(value);
  return { connection_id: string(item.connection_id), native_id: string(item.native_id) };
}
function workerTicket(value: unknown): WorkerTicket {
  const item = record(value);
  const phase = nullableString(item.assignment_phase);
  if (
    phase !== null &&
    ![
      'offered',
      'accepted',
      'handoff',
      'verifying',
      'integrating',
      'suspended_integration',
      'ci',
      'done',
      'blocked',
    ].includes(phase)
  )
    throw new ServerResponseShapeError('worker view');
  return {
    ticket: ticketRef(item.ticket),
    slug: nullableString(item.slug),
    assignment_id: nullableString(item.assignment_id),
    assignment_phase: phase as WorkerTicket['assignment_phase'],
    claim_expires_at: nullableNumber(item.claim_expires_at),
    started_phase: nullableString(item.started_phase),
    eta_at: nullableString(item.eta_at),
  };
}
function plannedTicket(value: unknown): PlannedWorkerTicket {
  const item = record(value),
    entry = record(item.entry);
  return {
    entry: {
      operation: string(entry.operation),
      worker: string(entry.worker),
      generation: string(entry.generation),
      ticket: ticketRef(entry.ticket),
      slug: string(entry.slug),
    },
    reasons: array(item.reasons, string),
  };
}
function optionalStrings(value: unknown): string[] {
  return value === undefined ? [] : array(value, string);
}
function preferences(value: unknown): WorkerPreferences | null {
  if (value === undefined || value === null) return null;
  const item = record(value),
    refusal = item.refusal === undefined || item.refusal === null ? null : record(item.refusal);
  return {
    prefer: optionalStrings(item.prefer),
    only: optionalStrings(item.only),
    avoid: optionalStrings(item.avoid),
    request: item.request === undefined ? null : nullableString(item.request),
    refusal: refusal && {
      slug: string(refusal.slug),
      reason: string(refusal.reason),
      detail: refusal.detail === undefined ? null : nullableString(refusal.detail),
    },
  };
}
function matching(value: unknown): WorkerMatching | null {
  if (value === undefined || value === null) return null;
  const item = record(value),
    request = item.request === undefined || item.request === null ? null : record(item.request);
  return {
    candidates: array(item.candidates, (candidate) => {
      const entry = record(candidate);
      if (typeof entry.preferred !== 'boolean') throw new ServerResponseShapeError('worker view');
      return { slug: string(entry.slug), preferred: entry.preferred };
    }),
    idle_reason: item.idle_reason === undefined ? null : nullableString(item.idle_reason),
    request: request && { slug: string(request.slug), reasons: array(request.reasons, string) },
  };
}
function worker(value: unknown): WorkerRow {
  const item = record(value),
    role = nullableString(item.role),
    availability = string(item.availability);
  if (role !== null && !['integrator', 'dispatcher', 'worker'].includes(role))
    throw new ServerResponseShapeError('worker view');
  if (!['live', 'disconnected', 'unresolved_claim_holder', 'unbound_connection'].includes(availability))
    throw new ServerResponseShapeError('worker view');
  return {
    id: string(item.id),
    role: role as WorkerRow['role'],
    generation: nullableString(item.generation),
    parent: nullableString(item.parent),
    label: string(item.label),
    tool: nullableString(item.tool),
    connection_kind: nullableString(item.connection_kind),
    connection_id: nullableString(item.connection_id),
    connection_available: nullableBoolean(item.connection_available),
    observed_alive: nullableBoolean(item.observed_alive),
    process_busy: nullableBoolean(item.process_busy),
    availability: availability as WorkerRow['availability'],
    lease_expires_at: nullableNumber(item.lease_expires_at),
    tickets: array(item.tickets, workerTicket),
    planned: array(item.planned, plannedTicket),
    integration_strategy: item.integration_strategy === undefined ? null : nullableString(item.integration_strategy),
    preferences: preferences(item.preferences),
    matching: matching(item.matching),
  };
}

/** HS2-6WFCZR: one chip per stated filter, strict filters first, e.g. `only: glance`. */
export function workerPreferenceChips(preferences: WorkerPreferences | null | undefined): WorkerPreferenceChip[] {
  if (!preferences) return [];
  const chips = (kind: WorkerPreferenceChip['kind'], filters: readonly string[]) =>
    filters.map((filter) => ({ kind, filter, label: `${kind}: ${filter.slice(filter.indexOf(':') + 1)}` }));
  return [
    ...chips('only', preferences.only),
    ...chips('avoid', preferences.avoid),
    ...chips('prefer', preferences.prefer),
  ];
}

/** HS2-6WFCZR: the pending request line, including a recorded refusal and its detail. */
export function workerRequestStatus(worker: WorkerRow): string | null {
  const request = worker.preferences?.request;
  if (!request) return null;
  const refusal = worker.preferences?.refusal;
  if (refusal && refusal.slug.toLowerCase() === request.toLowerCase())
    return `Request ${request} refused: ${refusal.reason.replace(/_/g, ' ')}${refusal.detail ? ` (${refusal.detail})` : ''}`;
  return `Requested ${request}`;
}

/** HS2-NEY1QH: display name for a registered integration strategy, e.g. `Serial integration`. */
export function integrationStrategyLabel(strategy: string): string {
  const name = strategy === 'ci-staged' ? 'CI-staged' : strategy.charAt(0).toUpperCase() + strategy.slice(1);
  return `${name} integration`;
}

const COVERAGE_REASONS: readonly WorkerCoverageReason[] = ['unsupported', 'failed_read', 'unobserved'];
function coverageGap(value: unknown): WorkerCoverageGap {
  const item = record(value),
    reason = string(item.reason);
  if (!(COVERAGE_REASONS as readonly string[]).includes(reason)) throw new ServerResponseShapeError('worker view');
  return {
    connection_id: string(item.connection_id),
    name: string(item.name),
    provider: string(item.provider),
    reason: reason as WorkerCoverageReason,
    details: array(item.details, string),
  };
}

/** A malformed 2xx response is an explicit unavailable state, never an empty worker list. */
export function parseWorkerView(value: unknown): WorkerView {
  const item = record(value);
  return {
    cursor: string(item.cursor),
    project_id: string(item.project_id),
    source_id: string(item.source_id),
    workers: array(item.workers, worker),
    uncovered_sources: array(item.uncovered_sources, string),
    ...(item.coverage_gaps === undefined ? {} : { coverage_gaps: array(item.coverage_gaps, coverageGap) }),
    connection_coverage:
      typeof item.connection_coverage === 'boolean'
        ? item.connection_coverage
        : (() => {
            throw new ServerResponseShapeError('worker view');
          })(),
  };
}

export function qualifiedWorkerTicket(ref: WorkerTicketRef): string {
  return `${ref.connection_id}:${ref.native_id}`;
}

/** One ticket can have separate durable assignment operations and a live claim. */
export function workerTicketOperationKey(ticket: WorkerTicket): string {
  const qualified = qualifiedWorkerTicket(ticket.ticket);
  return ticket.assignment_id ? `assignment:${qualified}:${ticket.assignment_id}` : `claim:${qualified}`;
}

/** Merge source-scoped reads into one stable worker identity; duplicate assignments win over plans. */
export function mergeWorkerViews(views: readonly WorkerView[]): WorkerRow[] {
  const rows = new Map<string, WorkerRow>();
  for (const view of views)
    for (const worker of view.workers) {
      const previous = rows.get(worker.id);
      if (!previous) {
        rows.set(worker.id, { ...worker, tickets: [...worker.tickets], planned: [...worker.planned] });
        continue;
      }
      const tickets = new Map(previous.tickets.map((item) => [workerTicketOperationKey(item), item]));
      for (const item of worker.tickets) tickets.set(workerTicketOperationKey(item), item);
      const assigned = new Set([...tickets.values()].map((item) => qualifiedWorkerTicket(item.ticket)));
      const plans = new Map(previous.planned.map((item) => [item.entry.operation, item]));
      for (const item of worker.planned) plans.set(item.entry.operation, item);
      rows.set(worker.id, {
        ...previous,
        ...worker,
        tickets: [...tickets.values()],
        planned: [...plans.values()].filter((item) => !assigned.has(qualifiedWorkerTicket(item.entry.ticket))),
      });
    }
  const ordered = [...rows.values()].sort((a, b) => {
    const rank = (item: WorkerRow) => (item.role === 'integrator' ? 0 : item.role === 'dispatcher' ? 1 : 2);
    return rank(a) - rank(b) || a.label.localeCompare(b.label) || a.id.localeCompare(b.id);
  });
  for (let index = 0; index < ordered.length; index += 1) {
    const worker = ordered[index];
    if (!worker.parent) continue;
    const parent = ordered.findIndex((item) => item.id === worker.parent);
    if (parent < 0 || parent === index - 1) continue;
    ordered.splice(index, 1);
    ordered.splice(parent < index ? parent + 1 : parent, 0, worker);
  }
  return ordered;
}

export type WorkerGroupKind =
  'integrating' | 'waiting' | 'remote-ci' | 'offered' | 'working' | 'assigned' | 'handoff' | 'unresolved' | 'planned';
export interface WorkerGroup {
  kind: WorkerGroupKind;
  label: string;
  tickets: Array<{ ref: WorkerTicketRef; slug: string; assignment?: WorkerTicket; plan?: PlannedWorkerTicket }>;
}

/** Lower is more advanced; a review-returned `blocked` operation is superseded by its correction. */
function workerTicketRank(ticket: WorkerTicket, now: number): number {
  switch (ticket.assignment_phase) {
    case 'integrating':
    case 'suspended_integration':
      return 0;
    case 'ci':
      return 1;
    case 'handoff':
    case 'verifying':
      return 2;
    case 'accepted':
      return 4;
    case 'offered':
      return 5;
    case 'done':
      return 6;
    case 'blocked':
      return 8;
    case null:
      return ticket.claim_expires_at !== null && ticket.claim_expires_at > now ? 3 : 7;
  }
}

/**
 * HS2-8SZBJJ: the server keeps one record per assignment operation and live claim, so one
 * qualified ticket can arrive several times for one worker. A Worker Column shows it once:
 * the most advanced record wins (integrating, remote CI, handoff, live claim, accepted,
 * offered, done, expired claim, then blocked), ties go to the later assignment id, and
 * that record decides the section. The same ticket under another worker is unaffected.
 */
function representativeWorkerTickets(tickets: readonly WorkerTicket[], now: number): WorkerTicket[] {
  const best = new Map<string, WorkerTicket>();
  for (const ticket of tickets) {
    const key = qualifiedWorkerTicket(ticket.ticket),
      previous = best.get(key);
    if (!previous) {
      best.set(key, ticket);
      continue;
    }
    const order =
      workerTicketRank(ticket, now) - workerTicketRank(previous, now) ||
      (previous.assignment_id ?? '').localeCompare(ticket.assignment_id ?? '', undefined, { numeric: true });
    if (order < 0) best.set(key, ticket);
  }
  return [...best.values()];
}

/** Group authoritative work without treating a reservation or tentative plan as a live claim. */
export function workerGroups(worker: WorkerRow, now = Date.now() / 1000, details?: WorkerTicketDetails): WorkerGroup[] {
  const groups = new Map<WorkerGroupKind, WorkerGroup>();
  const labels: Record<WorkerGroupKind, string> = {
    integrating: 'Integrating',
    waiting: 'Waiting to integrate',
    'remote-ci': 'Remote CI',
    offered: 'Offered',
    working: 'Working on',
    assigned: 'Assigned',
    handoff: 'Handed off',
    unresolved: 'Unresolved',
    planned: 'Planned next',
  };
  const add = (kind: WorkerGroupKind, item: WorkerGroup['tickets'][number]) => {
    const group = groups.get(kind) ?? { kind, label: labels[kind], tickets: [] };
    group.tickets.push(item);
    groups.set(kind, group);
  };
  for (const ticket of representativeWorkerTickets(worker.tickets, now)) {
    // HS2-D5KXBA: assignment history is not ticket status. The source-qualified detail wins.
    if (details && details[qualifiedWorkerTicket(ticket.ticket)]?.status !== 'started') continue;
    const phase = ticket.assignment_phase;
    const kind: WorkerGroupKind =
      worker.availability !== 'live'
        ? 'unresolved'
        : phase === 'ci'
          ? 'remote-ci'
          : phase === 'integrating' || phase === 'suspended_integration'
            ? 'integrating'
            : worker.role === 'integrator' && (phase === 'handoff' || phase === 'verifying')
              ? 'waiting'
              : phase === 'handoff' || phase === 'verifying'
                ? 'handoff'
                : ticket.claim_expires_at !== null && ticket.claim_expires_at > now
                  ? 'working'
                  : phase === 'offered'
                    ? 'offered'
                    : 'assigned';
    add(kind, { ref: ticket.ticket, slug: ticket.slug ?? ticket.ticket.native_id, assignment: ticket });
  }
  const assigned = new Set(worker.tickets.map((ticket) => qualifiedWorkerTicket(ticket.ticket)));
  for (const plan of worker.planned)
    if (
      !assigned.has(qualifiedWorkerTicket(plan.entry.ticket)) &&
      (!details || details[qualifiedWorkerTicket(plan.entry.ticket)]?.status === 'started')
    )
      add('planned', { ref: plan.entry.ticket, slug: plan.entry.slug, plan });
  const order: WorkerGroupKind[] =
    worker.role === 'integrator'
      ? ['integrating', 'waiting', 'remote-ci', 'offered', 'working', 'assigned', 'handoff', 'unresolved', 'planned']
      : ['offered', 'working', 'assigned', 'handoff', 'remote-ci', 'unresolved', 'planned'];
  return order.flatMap((kind) => groups.get(kind) ?? []);
}

export type WorkerTicketDetails = Record<string, TicketRow | undefined>;

/** A ticket source the Workers view knows about, from the project's provider descriptors. */
export interface WorkerCoverageSource {
  connectionId: string;
  name: string;
  provider: string;
}

/** A Git source whose worker view could not be loaded. */
export interface WorkerSourceError {
  connectionId: string;
  name: string;
  message: string;
}

/** One source in the Workers coverage summary, listed once by its human-readable name. */
export interface WorkerCoverageItem {
  connectionId: string;
  name: string;
  reason: WorkerCoverageReason | 'load_failed';
  message?: string;
}

export interface WorkerCoverage {
  /** Failed reads and failed loads: they may recover and deserve attention. */
  problems: WorkerCoverageItem[];
  /** Unsupported by design or not observed by this pool: expected, not alarming. */
  notes: WorkerCoverageItem[];
  /** The project's ticket sources are not loaded, so coverage cannot be judged. */
  sourcesUnknown: boolean;
  /** A view reported live connection observation as unknown. */
  connectionUnknown: boolean;
}

const COVERAGE_RANK: Record<WorkerCoverageItem['reason'], number> = {
  load_failed: 0,
  failed_read: 1,
  unobserved: 2,
  unsupported: 3,
};

/**
 * HS2-7WE7CQ: one entry per uncovered ticket source, by connection id and human-readable name. It
 * merges every view's `coverage_gaps`, the project's non-Git sources (known before any view loads)
 * and failed view loads; the most serious reason wins. An older server without `coverage_gaps`
 * falls back to `uncovered_sources`, so coverage is never shown as complete when it is not.
 */
export function workerCoverage(
  views: readonly WorkerView[],
  sources: readonly WorkerCoverageSource[] | undefined,
  errors: readonly WorkerSourceError[],
): WorkerCoverage {
  const byId = new Map<string, WorkerCoverageItem>(),
    nameOf = (id: string) => sources?.find((source) => source.connectionId === id)?.name ?? id,
    add = (item: WorkerCoverageItem) => {
      const previous = byId.get(item.connectionId);
      if (!previous || COVERAGE_RANK[item.reason] < COVERAGE_RANK[previous.reason]) byId.set(item.connectionId, item);
    };
  // Server gaps first, so their names win over the client's at equal rank.
  for (const view of views)
    if (view.coverage_gaps)
      for (const gap of view.coverage_gaps)
        add({ connectionId: gap.connection_id, name: gap.name, reason: gap.reason });
    else
      for (const id of view.uncovered_sources)
        add({
          connectionId: id,
          name: nameOf(id),
          reason: sources?.find((source) => source.connectionId === id && source.provider !== 'git')
            ? 'unsupported'
            : 'unobserved',
        });
  // Non-Git sources are known before any view loads (and when every view load fails).
  for (const source of sources ?? [])
    if (source.provider !== 'git') add({ connectionId: source.connectionId, name: source.name, reason: 'unsupported' });
  for (const error of errors)
    add({ connectionId: error.connectionId, name: error.name, reason: 'load_failed', message: error.message });
  const items = [...byId.values()];
  return {
    problems: items.filter((item) => item.reason === 'load_failed' || item.reason === 'failed_read'),
    notes: items.filter((item) => item.reason === 'unsupported' || item.reason === 'unobserved'),
    sourcesUnknown: sources === undefined,
    connectionUnknown: views.some((view) => !view.connection_coverage),
  };
}
