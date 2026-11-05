import { describe, expect, it } from 'vitest';

import {
  integrationStrategyLabel,
  mergeWorkerViews,
  parseWorkerView,
  qualifiedWorkerTicket,
  workerGroups,
  workerPreferenceChips,
  workerRequestStatus,
  type WorkerRow,
  type WorkerTicket,
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

  it('HS2-NEY1QH parses the integrator strategy and tolerates servers without it', () => {
    const parsed = parseWorkerView(
      view('s', [worker('i', { role: 'integrator', integration_strategy: 'serial' }), worker('w')]),
    );
    expect(parsed.workers.map((row) => row.integration_strategy)).toEqual(['serial', null]);
    expect(() =>
      parseWorkerView(view('s', [{ ...worker('i'), integration_strategy: 7 } as unknown as WorkerRow])),
    ).toThrow('invalid worker view');
    expect(integrationStrategyLabel('serial')).toBe('Serial integration');
    expect(integrationStrategyLabel('ci-staged')).toBe('CI-staged integration');
  });

  it('HS2-6WFCZR parses sparse preferences and matching and tolerates servers without them', () => {
    const raw = {
      ...worker('picky'),
      // The server skips empty filter lists and absent requests.
      preferences: {
        only: ['tag:glance'],
        request: 'HS2-A',
        refusal: { slug: 'HS2-A', reason: 'claimed', at: 't', by: {} },
      },
      matching: {
        worker: 'picky',
        candidates: [{ slug: 'HS2-B', preferred: true, priority: 'high', ticket: ref('B') }],
        idle_reason: null,
        request: { slug: 'HS2-A', ticket: null, reasons: ['claimed'], refusal: null },
      },
    } as unknown as WorkerRow;
    const [picky, plain] = parseWorkerView(view('s', [raw, worker('plain')])).workers;
    expect(picky.preferences).toEqual({
      prefer: [],
      only: ['tag:glance'],
      avoid: [],
      request: 'HS2-A',
      refusal: { slug: 'HS2-A', reason: 'claimed', detail: null },
    });
    expect(picky.matching).toEqual({
      candidates: [{ slug: 'HS2-B', preferred: true }],
      idle_reason: null,
      request: { slug: 'HS2-A', reasons: ['claimed'] },
    });
    expect([plain.preferences, plain.matching]).toEqual([null, null]);
    expect(() =>
      parseWorkerView(view('s', [{ ...worker('x'), preferences: { only: 'tag:x' } } as unknown as WorkerRow])),
    ).toThrow('invalid worker view');
    expect(() =>
      parseWorkerView(
        view('s', [{ ...worker('x'), matching: { candidates: [{ slug: 'a' }] } } as unknown as WorkerRow]),
      ),
    ).toThrow('invalid worker view');
    expect(workerPreferenceChips(picky.preferences).map((chip) => chip.label)).toEqual(['only: glance']);
    expect(workerPreferenceChips(null)).toEqual([]);
    expect(workerRequestStatus(picky)).toBe('Request HS2-A refused: claimed');
    expect(workerRequestStatus({ ...picky, preferences: { ...picky.preferences!, refusal: null } })).toBe(
      'Requested HS2-A',
    );
    // A refusal of an earlier request never labels a newer one.
    expect(workerRequestStatus({ ...picky, preferences: { ...picky.preferences!, request: 'HS2-NEW' } })).toBe(
      'Requested HS2-NEW',
    );
    expect(workerRequestStatus(plain)).toBeNull();
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

  it('HS2-8SZBJJ shows one card per qualified ticket in a worker column, choosing the most advanced record', () => {
    const op = (id: string | null, phase: WorkerTicket['assignment_phase'], claim: number | null = null) => ({
      ...ticket('same'),
      assignment_id: id,
      assignment_phase: phase,
      claim_expires_at: claim,
    });
    const items = (groups: ReturnType<typeof workerGroups>) =>
      groups.map((group) => [group.kind, group.tickets.map((item) => item.assignment?.assignment_id ?? null)]);
    // A review-returned blocked operation and its handoff correction on an expired worker.
    const expired = worker('A', {
      availability: 'disconnected',
      tickets: [op('same-1', 'blocked'), op('same-2', 'handoff')],
    });
    expect(items(workerGroups(expired, 100))).toEqual([['unresolved', ['same-2']]]);
    // A live claim wins over an offered or accepted operation for the same ticket.
    const claimed = worker('A', { tickets: [op('offer', 'offered'), op(null, null, 500)] });
    expect(items(workerGroups(claimed, 100))).toEqual([['working', [null]]]);
    // An expired claim falls below the operation; handoff outranks a live claim.
    expect(items(workerGroups(claimed, 600))).toEqual([['offered', ['offer']]]);
    expect(items(workerGroups(worker('A', { tickets: [op(null, null, 500), op('h', 'handoff')] }), 100))).toEqual([
      ['handoff', ['h']],
    ]);
    // Equal ranks keep the later assignment, numerically ordered.
    expect(items(workerGroups(worker('A', { tickets: [op('op-10', 'blocked'), op('op-9', 'blocked')] }), 100))).toEqual(
      [['assigned', ['op-10']]],
    );
    // Records merged from two source reads still collapse; another qualified ticket and plans stay.
    const [merged] = mergeWorkerViews([
      view('one', [worker('A', { tickets: [op('same-1', 'blocked')] })]),
      view('two', [
        worker('A', {
          tickets: [op('same-2', 'integrating'), { ...op('other', 'accepted'), ticket: ref('same', 'other') }],
        }),
      ]),
    ]);
    expect(
      workerGroups({ ...merged, role: 'integrator' }, 100).map((group) => [
        group.kind,
        group.tickets.map((item) => qualifiedWorkerTicket(item.ref)),
      ]),
    ).toEqual([
      ['integrating', ['git:same']],
      ['assigned', ['other:same']],
    ]);
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
