import { describe, expect, it } from 'vitest';

import {
  atRiskDetail,
  builtAheadRows,
  type BuiltAheadView,
  canReopen,
  gateTicketLabel,
  parseBuiltAhead,
  REOPEN_REASONS,
} from './orchestration-built-ahead';
import { demoTimeline } from './ux-demo/orchestration-fixtures';

const timeline = demoTimeline('running');
const gate = (rest: Partial<BuiltAheadView['gates'][number]> = {}) => ({
  target_id: 'web',
  connection_id: 'web-tickets',
  native_id: 'ticket-WEB-33',
  commit: '3f9c2a1d00ffee',
  result: 'pending',
  ...rest,
});
const view = (rest: Partial<BuiltAheadView> = {}): BuiltAheadView => ({
  state: 'provisional',
  depth: 1,
  gates: [gate()],
  held: false,
  ...rest,
});

describe('work built ahead of a pending gate (HS2-0D60T8)', () => {
  it('reads readiness show’s provisional record with the rule’s depth limit, and nothing without one', () => {
    expect(parseBuiltAhead(undefined)).toBeUndefined();
    expect(parseBuiltAhead({ step_id: 's' })).toBeUndefined();
    expect(
      parseBuiltAhead({
        provisional: {
          state: 'at_risk',
          depth: 2,
          held: true,
          recovery: 'reverify_by_covering_run',
          gates: [{ ...gate({ result: 'red' }), gate_ids: ['g'], run: 'r' }, {}],
        },
        speculation: { allowed: true, depth: 2, max_depth: 3, bases: [] },
      }),
    ).toEqual({
      state: 'at_risk',
      depth: 2,
      max_depth: 3,
      held: true,
      recovery: 'reverify_by_covering_run',
      reopen_reason: undefined,
      gates: [
        gate({ result: 'red' }),
        { target_id: '', connection_id: '', native_id: '', commit: '', result: 'unknown' },
      ],
    });
    expect(parseBuiltAhead({ provisional: { state: 'provisional' } })).toMatchObject({
      depth: 0,
      gates: [],
      held: false,
    });
  });

  it('names a gate’s ticket by its slug, through what waits on it, or its project', () => {
    expect(gateTicketLabel(gate(), timeline)).toBe('WEB-33');
    // API-15 is only known as a prerequisite of WEB-35 when its own card is gone.
    const pruned = demoTimeline('running');
    for (const wave of pruned.waves) wave.items = wave.items.filter((item) => item.slug !== 'API-12');
    expect(
      gateTicketLabel(gate({ target_id: 'api', connection_id: 'api-tickets', native_id: 'ticket-API-12' }), pruned),
    ).toBe('API-12');
    expect(gateTicketLabel(gate({ target_id: 'docs', native_id: 'other' }), timeline)).toBe('a Docs ticket');
    expect(gateTicketLabel(gate({ target_id: 'gone', native_id: 'other' }), timeline)).toBe('a prerequisite');
  });

  it('lists each gate, the state, the depth against its limit, and whether it is held', () => {
    expect(builtAheadRows(view({ max_depth: 2 }), timeline)).toEqual([
      { label: 'Rests on', value: 'WEB-33 · CI running on 3f9c2a1' },
      { label: 'State', value: 'Provisional' },
      { label: 'Depth', value: '1 of 2 allowed' },
    ]);
    expect(
      builtAheadRows(
        view({ gates: [], state: 'reopened', reopen_reason: 'fix_changed_contract', held: true }),
        timeline,
      ),
    ).toEqual([
      { label: 'Rests on', value: 'No pending gate' },
      { label: 'State', value: 'Reopened (fix changed contract)' },
      { label: 'Depth', value: '1' },
      { label: 'Shipping', value: 'Held off the shipping branch for now' },
    ]);
    expect(builtAheadRows(view({ gates: [gate({ result: 'novel' })], state: 'novel_state' }), timeline)[0].value).toBe(
      'WEB-33 · novel on 3f9c2a1',
    );
  });

  it('names what to repair first and how the work recovers', () => {
    expect(atRiskDetail(view({ state: 'at_risk', gates: [gate({ result: 'red' })] }), timeline)).toBe(
      'WEB-33’s CI failed. Repair WEB-33 first; after that this work is rebased onto the fix and checked again.',
    );
    expect(
      atRiskDetail(
        view({ state: 'at_risk', recovery: 'reverify_by_covering_run', gates: [gate({ result: 'pending' })] }),
        timeline,
      ),
    ).toBe(
      'A prerequisite’s CI failed. Repair the prerequisite first; after that the next covering CI run checks this work again.',
    );
  });

  it('lets a person reopen provisional or at-risk work only, with stable reason codes', () => {
    expect(['provisional', 'at_risk', 'confirmed', 'reopened'].map((state) => canReopen(view({ state })))).toEqual([
      true,
      true,
      false,
      false,
    ]);
    for (const reason of REOPEN_REASONS) {
      expect(reason.code).toMatch(/^[a-z_]+$/);
      expect(reason.label.endsWith('…')).toBe(true);
    }
  });
});
