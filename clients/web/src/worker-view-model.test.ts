import { describe, expect, it } from 'vitest';

import type { TicketRow } from './api';
import {
  integrationStrategyLabel,
  mergeWorkerViews,
  parseWorkerView,
  qualifiedWorkerTicket,
  workerCoverage,
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
  it('HS2-0BJ1A3 parses optional cadence and refuses a malformed goal', () => {
    const status = {
      goal_minutes: 360,
      last_green_main_at: 1_800_000_000,
      next_target_at: 1_800_021_600,
      oldest_staging_at: null,
      staging_age_seconds: null,
      ci_failures_since_promotion: 0,
      correction_wall_seconds: null,
      tickets_promoted_in_goal_window: 2,
      ready_batch: null,
      action: 'no_ready_work',
      missed_goal_reason: null,
    };
    expect(parseWorkerView({ ...view('s', []), cadence: status }).cadence).toEqual(status);
    expect(parseWorkerView(view('s', [])).cadence).toBeUndefined();
    expect(() => parseWorkerView({ ...view('s', []), cadence: { ...status, goal_minutes: -1 } })).toThrow();
  });
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

  it('HS2-D5KXBA keeps only authoritative Started tickets through completion, empty, and refill', () => {
    const assigned = worker('A', {
      availability: 'disconnected',
      tickets: [ticket('one'), ticket('two'), { ...ticket('two'), assignment_id: 'older' }],
      planned: [
        {
          entry: { operation: 'plan', worker: 'A', generation: 'g1', ticket: ref('future'), slug: 'HS2-future' },
          reasons: [],
        },
      ],
    });
    const details = {
      'git:one': { status: 'started' } as TicketRow,
      'git:two': { status: 'completed' } as TicketRow,
      'git:future': { status: 'not_started' } as TicketRow,
    };
    const ids = () =>
      workerGroups(assigned, 100, details).flatMap((group) => group.tickets.map((item) => item.ref.native_id));
    expect(workerGroups(assigned, 100, details).map((group) => [group.kind, group.tickets.length])).toEqual([
      ['unresolved', 1],
    ]);
    expect(ids()).toEqual(['one']);
    details['git:one'].status = 'completed';
    expect(workerGroups(assigned, 100, details)).toEqual([]);
    details['git:two'].status = 'started';
    details['git:future'].status = 'started';
    expect(ids()).toEqual(['two', 'future']);
    delete (details as Record<string, TicketRow | undefined>)['git:two'];
    expect(ids()).toEqual(['future']);
  });
});

describe('HS2-7WE7CQ worker coverage', () => {
  const gap = (connection_id: string, reason: 'unsupported' | 'failed_read' | 'unobserved', name = connection_id) => ({
    connection_id,
    name,
    provider: reason === 'unsupported' ? 'github' : 'git',
    reason,
    details: [],
  });
  const withGaps = (source_id: string, gaps: ReturnType<typeof gap>[]): WorkerView => ({
    ...view(source_id, []),
    uncovered_sources: gaps.map((item) => item.connection_id),
    coverage_gaps: gaps,
  });

  it('parses coverage gaps, tolerates older servers, and refuses an unknown reason', () => {
    const parsed = parseWorkerView(withGaps('s', [gap('github-a', 'unsupported', 'GitHub a/b')]));
    expect(parsed.coverage_gaps).toEqual([gap('github-a', 'unsupported', 'GitHub a/b')]);
    expect(parseWorkerView(view('s', [])).coverage_gaps).toBeUndefined();
    expect(() =>
      parseWorkerView({ ...view('s', []), coverage_gaps: [{ ...gap('x', 'unobserved'), reason: 'missing' }] }),
    ).toThrow('invalid worker view');
  });

  it('lists each source once by name, the most serious reason winning across views', () => {
    const coverage = workerCoverage(
      [
        withGaps('git-a', [gap('github-a', 'unsupported', 'GitHub a/b'), gap('git-b', 'unobserved', 'B tickets')]),
        withGaps('git-c', [gap('git-b', 'failed_read', 'B tickets'), gap('github-a', 'unsupported', 'GitHub a/b')]),
      ],
      [
        { connectionId: 'git-a', name: 'A tickets', provider: 'git' },
        { connectionId: 'github-a', name: 'Client name for a/b', provider: 'github' },
      ],
      [{ connectionId: 'git-d', name: 'D tickets', message: 'Server unavailable' }],
    );
    expect(coverage.problems).toEqual([
      { connectionId: 'git-b', name: 'B tickets', reason: 'failed_read' },
      { connectionId: 'git-d', name: 'D tickets', reason: 'load_failed', message: 'Server unavailable' },
    ]);
    // The server's name wins over the client's at equal rank; no id-only duplicate.
    expect(coverage.notes).toEqual([{ connectionId: 'github-a', name: 'GitHub a/b', reason: 'unsupported' }]);
    expect(coverage).toMatchObject({ sourcesUnknown: false, connectionUnknown: false });
  });

  it('a load error outranks a gap for the same source, in either order', () => {
    const error = { connectionId: 'git-b', name: 'B tickets', message: 'boom' };
    const coverage = workerCoverage([withGaps('git-a', [gap('git-b', 'unobserved', 'B tickets')])], [], [error]);
    expect(coverage.problems).toEqual([{ ...error, reason: 'load_failed' }]);
    expect(coverage.notes).toEqual([]);
  });

  it('reports non-Git sources before any view loads, and falls back to uncovered ids on older servers', () => {
    const sources = [
      { connectionId: 'git-a', name: 'A tickets', provider: 'git' },
      { connectionId: 'jira-eng', name: 'Jira ENG', provider: 'jira' },
    ];
    expect(workerCoverage([], sources, []).notes).toEqual([
      { connectionId: 'jira-eng', name: 'Jira ENG', reason: 'unsupported' },
    ]);
    const legacy = { ...view('git-a', []), uncovered_sources: ['jira-eng', 'git-z'] };
    expect(workerCoverage([legacy], sources, []).notes).toEqual([
      { connectionId: 'jira-eng', name: 'Jira ENG', reason: 'unsupported' },
      { connectionId: 'git-z', name: 'git-z', reason: 'unobserved' },
    ]);
  });

  it('keeps unloaded sources and unknown connection coverage explicit', () => {
    expect(workerCoverage([], undefined, [])).toEqual({
      problems: [],
      notes: [],
      sourcesUnknown: true,
      connectionUnknown: false,
    });
    expect(workerCoverage([{ ...view('git-a', []), connection_coverage: false }], [], []).connectionUnknown).toBe(true);
  });
});
