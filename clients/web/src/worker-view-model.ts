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
}

export interface WorkerView {
  cursor: string;
  project_id: string;
  source_id: string;
  workers: WorkerRow[];
  uncovered_sources: string[];
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

/** Group authoritative work without treating a reservation or tentative plan as a live claim. */
export function workerGroups(worker: WorkerRow, now = Date.now() / 1000): WorkerGroup[] {
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
  for (const ticket of worker.tickets) {
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
    if (!assigned.has(qualifiedWorkerTicket(plan.entry.ticket)))
      add('planned', { ref: plan.entry.ticket, slug: plan.entry.slug, plan });
  const order: WorkerGroupKind[] =
    worker.role === 'integrator'
      ? ['integrating', 'waiting', 'remote-ci', 'offered', 'working', 'assigned', 'handoff', 'unresolved', 'planned']
      : ['offered', 'working', 'assigned', 'handoff', 'remote-ci', 'unresolved', 'planned'];
  return order.flatMap((kind) => groups.get(kind) ?? []);
}

export type WorkerTicketDetails = Record<string, TicketRow | undefined>;
