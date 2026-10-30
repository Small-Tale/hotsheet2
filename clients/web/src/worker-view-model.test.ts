import { describe, expect, it } from 'vitest';

import {
  mergeWorkerViews,
  parseWorkerView,
  workerGroups,
  type WorkerRow,
  workerTicketOperationKey,
  type WorkerView,
} from './worker-view-model';

const ref = (native_id: string, connection_id = 'git') => ({ connection_id, native_id });
const ticket = (
  native_id: string,
  phase: 'accepted' | 'handoff' = 'accepted',
  claim_expires_at: number | null = 500,
) => ({
  ticket: ref(native_id),
  slug: `HS2-${native_id}`,
  assignment_id: native_id,
  assignment_phase: phase,
  claim_expires_at,
  started_phase: 'working',
  eta_at: null,
});
const worker = (id: string, overrides: Partial<WorkerRow> = {}): WorkerRow => ({
  id,
  role: 'worker',
  generation: 'g1',
  parent: null,
  label: id,
  tool: 'Codex',
  connection_kind: 'terminal',
  connection_id: id,
  connection_available: true,
  observed_alive: true,
  process_busy: true,
  availability: 'live',
  lease_expires_at: 900,
  tickets: [],
  planned: [],
  ...overrides,
});
const view = (source_id: string, workers: WorkerRow[]): WorkerView => ({
  cursor: source_id,
  project_id: 'demo',
  source_id,
  workers,
  uncovered_sources: [],
  connection_coverage: true,
});

describe('HS2-E9SR7R worker view projection', () => {
  it('rejects malformed successful responses instead of showing a false empty state', () => {
    expect(() =>
      parseWorkerView({ cursor: 'c', project_id: 'p', source_id: 's', workers: [], uncovered_sources: [] }),
    ).toThrow('invalid worker view');
    expect(() => parseWorkerView({ ...view('s', []), workers: [{ id: 'a' }] })).toThrow('invalid worker view');
    expect(parseWorkerView(view('s', [worker('a')])).workers[0]?.id).toBe('a');
  });

  it('deduplicates one worker across sources, keeps the integrator first and its background child adjacent', () => {
    const one = worker('Codex B', {
      tickets: [ticket('one')],
      planned: [
        {
          entry: {
            operation: 'plan-1',
            worker: 'Codex B',
            generation: 'g1',
            ticket: ref('one'),
            slug: 'HS2-one',
          },
          reasons: [],
        },
      ],
    });
    const two = worker('Codex B', { tickets: [ticket('two')] });
    const rows = mergeWorkerViews([
      view('git-one', [one, worker('Integrator', { role: 'integrator' }), worker('Child', { parent: 'Codex B' })]),
      view('git-two', [two]),
    ]);
    expect(rows.map((row) => row.id)).toEqual(['Integrator', 'Codex B', 'Child']);
    expect(rows[1]?.tickets.map((item) => item.ticket.native_id)).toEqual(['one', 'two']);
    expect(rows[1]?.planned).toEqual([]);
  });

  it('keeps claims, reservations, handoffs and plans distinct after disconnect', () => {
    const planned = {
      entry: {
        operation: 'p',
        worker: 'A',
        generation: 'g1',
        ticket: ref('future'),
        slug: 'HS2-future',
      },
      reasons: ['dependency open'],
    };
    const live = worker('A', { tickets: [ticket('claim'), ticket('handoff', 'handoff', null)], planned: [planned] });
    expect(workerGroups(live, 100).map((group) => group.kind)).toEqual(['working', 'handoff', 'planned']);
    expect(workerGroups({ ...live, availability: 'disconnected' }, 100).map((group) => group.kind)).toEqual([
      'unresolved',
      'planned',
    ]);
    expect(workerGroups(worker('idle'), 100)).toEqual([]);
  });

  it('retains qualified identities for duplicate native ids in different sources', () => {
    const rows = mergeWorkerViews([
      view('one', [worker('A', { tickets: [ticket('same', 'accepted', 500)] })]),
      view('two', [worker('A', { tickets: [{ ...ticket('same'), ticket: ref('same', 'other') }] })]),
    ]);
    expect(rows[0]?.tickets.map((item) => item.ticket.connection_id)).toEqual(['git', 'other']);
  });

  it('retains distinct blocked and offered operations plus a claim for one qualified ticket', () => {
    const offered = { ...ticket('same'), assignment_id: 'offered-op', assignment_phase: 'offered' as const };
    const blocked = { ...ticket('same'), assignment_id: 'blocked-op', assignment_phase: 'blocked' as const };
    const claim = { ...ticket('same'), assignment_id: null, assignment_phase: null, claim_expires_at: 700 };
    const rows = mergeWorkerViews([
      view('one', [worker('A', { tickets: [offered] })]),
      view('two', [worker('A', { tickets: [blocked, claim] })]),
    ]);
    expect(rows[0]?.tickets.map(workerTicketOperationKey)).toEqual([
      'assignment:git:same:offered-op',
      'assignment:git:same:blocked-op',
      'claim:git:same',
    ]);
  });
});
