import { describe, expect, it } from 'vitest';

import { parseWorkerSpeculation, provisionalLine, provisionalLines } from './worker-provisional';
import { parseWorkerView } from './worker-view-model';

const record = (state: string, gates = [{ slug: 'API-12', result: 'pending' }]) => ({
  qualified: 'src:t1',
  slug: 'WEB-3',
  state,
  gates,
});

describe('provisional assignments in the Workers view (HS2-N8H97X)', () => {
  it('reads the worker view’s speculation leniently, keyed by the qualified ticket', () => {
    expect(parseWorkerSpeculation(undefined)).toBeUndefined();
    expect(parseWorkerSpeculation([])).toBeUndefined();
    expect(
      parseWorkerSpeculation({
        records: [
          {
            assignment: 'a1',
            ticket: { connection_id: 'src', native_id: 't1' },
            slug: 'WEB-3',
            worker: 'w',
            base: 'b'.repeat(40),
            depth: 1,
            state: 'at_risk',
            gates: [{ blocker: 'n', slug: 'API-12', commit: 'c'.repeat(40), result: 'red', run: 'r' }, { slug: 4 }],
            explain: 'provisional on API-12 CI r',
          },
          { ticket: { connection_id: 'src' }, state: 'provisional' },
          { ticket: { connection_id: 'src', native_id: 't2' }, state: 'provisional' },
        ],
        repair_first: ['API-12', 7],
        reevaluate: [],
      }),
    ).toEqual({
      records: [
        { qualified: 'src:t1', slug: 'WEB-3', state: 'at_risk', gates: [{ slug: 'API-12', result: 'red' }] },
        { qualified: 'src:t2', slug: 't2', state: 'provisional', gates: [] },
      ],
      repair_first: ['API-12'],
    });
    expect(parseWorkerSpeculation({})).toEqual({ records: [], repair_first: [] });
  });

  it('keeps the Workers view available whatever the speculation member holds', () => {
    const base = {
      cursor: 'c',
      project_id: 'p',
      source_id: 's',
      workers: [],
      uncovered_sources: [],
      connection_coverage: true,
    };
    expect(parseWorkerView(base).speculation).toBeUndefined();
    expect(parseWorkerView({ ...base, speculation: 'broken' }).speculation).toBeUndefined();
    expect(parseWorkerView({ ...base, speculation: { records: [] } }).speculation).toEqual({
      records: [],
      repair_first: [],
    });
  });

  it('words provisional, at-risk and reopened work, and leaves confirmed work alone', () => {
    expect(provisionalLine(record('provisional'), [])).toEqual({
      state: 'Provisional on API-12 · CI running',
      stateTone: 'info',
    });
    expect(
      provisionalLine(
        record('provisional', [
          { slug: 'API-12', result: 'pending' },
          { slug: 'API-13', result: 'superseded' },
        ]),
        [],
      )?.state,
    ).toBe('Provisional on API-12, API-13 · CI running, CI superseded');
    expect(provisionalLine(record('provisional', []), [])?.state).toBe('Provisional on a prerequisite');
    expect(provisionalLine(record('provisional', [{ slug: 'API-12', result: 'novel' }]), [])?.state).toBe(
      'Provisional on API-12 · novel',
    );
    expect(
      provisionalLine(
        record('at_risk', [
          { slug: 'API-12', result: 'red' },
          { slug: 'API-13', result: 'green' },
        ]),
        ['API-12'],
      ),
    ).toEqual({ state: 'At risk · repair API-12 first', stateTone: 'warning' });
    // Without repair_first naming a base, every base is named.
    expect(provisionalLine(record('at_risk'), [])?.state).toBe('At risk · repair API-12 first');
    expect(provisionalLine(record('at_risk', []), [])?.state).toBe('At risk · repair its prerequisite first');
    expect(provisionalLine(record('reopened'), [])).toEqual({
      state: 'Reopened · the fix invalidated it',
      stateTone: 'warning',
    });
    expect(provisionalLine(record('confirmed'), [])).toBeUndefined();
    expect(
      provisionalLines([
        undefined,
        { records: [record('provisional'), { ...record('confirmed'), qualified: 'x:y' }], repair_first: [] },
      ]),
    ).toEqual({ 'src:t1': { state: 'Provisional on API-12 · CI running', stateTone: 'info' } });
  });
});
