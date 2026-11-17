import { describe, expect, it } from 'vitest';

import { parsePlanHealth, PLAN_FAILURE_CODES, type PlanHealth, planProblem } from './orchestration-plan-health';

const name = (id: string) => (id === 'S1' ? 'API-12' : `step ${id}`);
const health = (rest: Partial<PlanHealth>): PlanHealth => ({
  state: 'conflict',
  conflicts: [],
  local: [],
  discard_resolves: false,
  ...rest,
});

describe('a plan the view cannot read (HS2-F1BKQ0)', () => {
  it('reads readiness health leniently and knows the codes that mean the plan failed', () => {
    expect(parsePlanHealth(undefined)).toBeUndefined();
    expect(parsePlanHealth({ state: 'broken' })).toBeUndefined();
    expect(parsePlanHealth({ state: 'ok', discard_resolves: false })).toEqual({
      state: 'ok',
      file: undefined,
      reason: undefined,
      conflicts: [],
      local: [],
      discard_resolves: false,
    });
    expect(
      parsePlanHealth({ state: 'conflict', conflicts: ['S1', 2, 'S2'], local: ['S1'], discard_resolves: true }),
    ).toMatchObject({ conflicts: ['S1', 'S2'], local: ['S1'], discard_resolves: true });
    expect(parsePlanHealth({ state: 'damaged', file: 'elsewhere' })?.file).toBeUndefined();
    expect([...PLAN_FAILURE_CODES].sort()).toEqual(['corrupt_orchestration_data', 'plan_conflict', 'upgrade_required']);
  });

  it('names the steps, offers Keep the Checked-In Plan… only when dropping this machine’s copies resolves it', () => {
    expect(planProblem(health({ state: 'ok' }), name)).toBeUndefined();
    const resolvable = planProblem(health({ conflicts: ['S1', 'S2'], local: ['S1'], discard_resolves: true }), name)!;
    expect(resolvable.canDiscard).toBe(true);
    expect(resolvable.detail).toContain('This machine kept API-12');
    expect(resolvable.detail).toContain('steps involved: API-12, step S2');
    const several = planProblem(
      health({ conflicts: ['S1', 'S2'], local: ['S1', 'S2'], discard_resolves: true }),
      name,
    )!;
    expect(several.detail).toContain('has them differently');
    const stuck = planProblem(health({ conflicts: ['S1', 'S2'], local: ['S1'] }), name)!;
    expect(stuck.canDiscard).toBe(false);
    expect(stuck.detail).toContain('dropping this machine’s copies would not resolve it');
    expect(planProblem(health({ conflicts: ['S1', 'S2'] }), name)!.detail).toContain('in the checked-in plan');
  });

  it('tells a damaged checked-in file from damaged local records and a newer version', () => {
    const checkedIn = planProblem(health({ state: 'damaged', file: 'checked_in', reason: 'corrupt' }), name)!;
    expect(checkedIn.title).toBe('The Checked-In Plan Cannot Be Read');
    expect(checkedIn.detail).toContain('merge markers');
    expect(checkedIn.canDiscard).toBe(false);
    expect(
      planProblem(health({ state: 'damaged', file: 'checked_in', reason: 'unsupported_version' }), name)!.detail,
    ).toContain('newer version');
    const local = planProblem(health({ state: 'damaged', file: 'local', reason: 'corrupt' }), name)!;
    expect(local.title).toBe('This Machine’s Plan Records Are Damaged');
    expect(
      planProblem(health({ state: 'damaged', file: 'local', reason: 'unsupported_version' }), name)!.detail,
    ).toContain('Update Hot Sheet');
  });
});
