import { describe, expect, it } from 'vitest';

import { advanceDismissalThrash, advanceRenderStorm, hasDismissalThrash, isUnexpectedQuickDismiss } from './ui-stability-diagnostics';

describe('UI stability diagnostics', () => {
  it('distinguishes an unexpected fast dismissal from a direct user action', () => {
    expect(isUnexpectedQuickDismiss(1_000, 1_700, 1_100)).toBe(true);
    expect(isUnexpectedQuickDismiss(1_000, 1_100, 1_050)).toBe(false);
    expect(isUnexpectedQuickDismiss(1_000, 2_500, 1_100)).toBe(false);
  });

  it('requires three quick dismissals inside one bounded window', () => {
    expect(hasDismissalThrash([1_000, 5_000], 9_000)).toBe(false);
    expect(hasDismissalThrash([1_000, 5_000, 9_000], 9_000)).toBe(true);
    expect(hasDismissalThrash([1_000, 5_000, 16_000], 16_000)).toBe(false);
  });

  it('reports one continuous dismissal episode and rearms after its window clears', () => {
    let state = { dismissals: [] as number[], reported: false };
    const reports: number[] = [];
    for (const at of [1_000, 2_000, 3_000, 4_000, 5_000]) {
      const next = advanceDismissalThrash(state, at);
      state = next;
      if (next.shouldReport) reports.push(at);
    }
    state = advanceDismissalThrash(state, 16_000);
    for (const at of [17_000, 18_000]) {
      const next = advanceDismissalThrash(state, at);
      state = next;
      if (next.shouldReport) reports.push(at);
    }
    expect(reports).toEqual([3_000, 18_000]);
  });

  it('reports a continuous render storm once and rearms after a quiet window', () => {
    let state = { passes: [] as number[], reported: false };
    const reports: number[] = [];
    for (let at = 0; at < 30_000; at += 100) {
      const next = advanceRenderStorm(state, at);
      state = next;
      if (next.shouldReport) reports.push(at);
    }
    expect(reports).toEqual([1_100]);

    state = advanceRenderStorm(state, 33_000);
    expect(state.reported).toBe(false);
    for (let at = 33_100; at <= 34_100; at += 100) {
      const next = advanceRenderStorm(state, at);
      state = next;
      if (next.shouldReport) reports.push(at);
    }
    expect(reports).toEqual([1_100, 34_100]);
  });

  it('does not carry intentional foreground rendering into a later storm', () => {
    let state = { passes: [] as number[], reported: false };
    for (let at = 0; at < 8_000; at += 100) state = advanceRenderStorm(state, at, true);
    expect(state).toMatchObject({ passes: [], reported: false, shouldReport: false });

    const reports: number[] = [];
    for (let at = 8_000; at < 9_200; at += 100) {
      const next = advanceRenderStorm(state, at);
      state = next;
      if (next.shouldReport) reports.push(at);
    }
    expect(reports).toEqual([9_100]);
  });
});
