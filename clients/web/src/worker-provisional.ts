/**
 * HS2-N8H97X: provisional assignments in the Workers view (design HS2-2XJSJK). The worker view carries the
 * pool's speculation status (HS2-2SJPCH): each assignment built ahead of a prerequisite's CI, its state and
 * gates, and which prerequisites to repair first. This maps it onto the shared TicketRow orchestration line
 * ("Provisional on API-12 · CI running", "At risk · repair API-12 first"), with no new TicketRow prop. Pure;
 * a malformed member is left out rather than failing the worker view.
 */
import type { TicketRowOrchestration } from './components/ticket-row';

export interface WorkerSpeculationGate {
  slug: string;
  /** `pending`, `green`, `red`, `superseded`, `cancelled` or `unknown`. */
  result: string;
}

export interface WorkerSpeculationRecord {
  /** The provisional ticket, qualified as the Workers view keys tickets (`connection:native`). */
  qualified: string;
  slug: string;
  /** `provisional`, `confirmed`, `at_risk` or `reopened`. */
  state: string;
  gates: WorkerSpeculationGate[];
}

export interface WorkerSpeculation {
  records: WorkerSpeculationRecord[];
  /** Prerequisite slugs whose gate is red: their repair comes before new work. */
  repair_first: string[];
}

const text = (value: unknown) => (typeof value === 'string' ? value : undefined);

/** Read the worker view's optional `speculation`; undefined when absent or not an object. */
export function parseWorkerSpeculation(value: unknown): WorkerSpeculation | undefined {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return undefined;
  const status = value as Record<string, unknown>;
  const records = Array.isArray(status.records) ? (status.records as Array<Record<string, unknown>>) : [];
  return {
    records: records.flatMap((record) => {
      const ticket = (record.ticket ?? {}) as Record<string, unknown>,
        connection = text(ticket.connection_id),
        native = text(ticket.native_id),
        state = text(record.state);
      if (!connection || !native || !state) return [];
      const gates = Array.isArray(record.gates) ? (record.gates as Array<Record<string, unknown>>) : [];
      return [
        {
          qualified: `${connection}:${native}`,
          slug: text(record.slug) ?? native,
          state,
          gates: gates.flatMap((gate) => {
            const slug = text(gate.slug);
            return slug ? [{ slug, result: text(gate.result) ?? 'unknown' }] : [];
          }),
        },
      ];
    }),
    repair_first: Array.isArray(status.repair_first)
      ? status.repair_first.filter((slug): slug is string => typeof slug === 'string')
      : [],
  };
}

const RESULTS: Readonly<Record<string, string>> = {
  pending: 'CI running',
  green: 'CI passed',
  red: 'CI failed',
  superseded: 'CI superseded',
  cancelled: 'CI cancelled',
  unknown: 'CI result unknown',
};

/** The orchestration line of one provisional assignment; none for confirmed work, which needs no remark. */
export function provisionalLine(
  record: WorkerSpeculationRecord,
  repairFirst: readonly string[],
): TicketRowOrchestration | undefined {
  const bases = record.gates.map((gate) => gate.slug);
  switch (record.state) {
    case 'provisional': {
      const results = [...new Set(record.gates.map((gate) => RESULTS[gate.result] ?? gate.result))];
      return {
        state: `Provisional on ${bases.join(', ') || 'a prerequisite'}${results.length ? ` · ${results.join(', ')}` : ''}`,
        stateTone: 'info',
      };
    }
    case 'at_risk': {
      const repair = bases.filter((slug) => repairFirst.includes(slug));
      return {
        state: `At risk · repair ${(repair.length ? repair : bases).join(', ') || 'its prerequisite'} first`,
        stateTone: 'warning',
      };
    }
    case 'reopened':
      return { state: 'Reopened · the fix invalidated it', stateTone: 'warning' };
    default:
      return undefined;
  }
}

/** Every provisional assignment's line, by the qualified ticket the Workers view shows it on. */
export function provisionalLines(
  speculations: ReadonlyArray<WorkerSpeculation | undefined>,
): Readonly<Partial<Record<string, TicketRowOrchestration>>> {
  const lines: Partial<Record<string, TicketRowOrchestration>> = {};
  for (const speculation of speculations) {
    if (!speculation) continue;
    for (const record of speculation.records) {
      const line = provisionalLine(record, speculation.repair_first);
      if (line) lines[record.qualified] = line;
    }
  }
  return lines;
}
