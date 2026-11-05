import { describe, expect, it } from 'vitest';

import {
  completedSinceUtcMidnight,
  formatDuration,
  rightNowRow,
  rightNowRows,
  type RightNowTargets,
  rightNowTiles,
  updatedSparkline,
} from './stats-right-now';
import { GLANCE_FIXTURE_NOW as NOW, glanceScenario, liveStateFixture } from './ux-demo/live-state-fixtures';

const MINUTE = 60_000;
const open: RightNowTargets = {
  projectFor: (state) => state.checkouts[0],
  terminalFor: (projectId) => `${projectId}:t1`,
};

describe('HS2-ERTNJ7 Right now tiles', () => {
  it('sums every store in scope and keeps the aggregate peak', () => {
    const tiles = rightNowTiles(glanceScenario(), NOW);
    expect(tiles.ticketsUpdated).toBe(14);
    expect(tiles.needsYou).toBe(2);
    expect(tiles.workersLive).toBe(3);
    expect(tiles.workersExpected).toBe(4);
    expect(tiles.tokensPeakPerMinute).toBe(4_100 + 2_400 + 6_200 + 3_000);
    expect(tiles.tokensPerMinute).toBeCloseTo((160 + 8_000) / 5);
    expect(tiles.updatedSparkline).toHaveLength(12);
    expect(tiles.updatedSparkline.reduce((sum, value) => sum + value, 0)).toBe(14);
  });

  it('scopes to one project by passing only its stores', () => {
    const hotSheet = glanceScenario().filter((state) => state.name === 'Hot Sheet 2');
    const tiles = rightNowTiles(hotSheet, NOW);
    expect(tiles.ticketsUpdated).toBe(13);
    expect(tiles.needsYou).toBe(0);
    expect(tiles.workersExpected).toBe(2);
  });

  it('is all zero for no stores', () => {
    expect(rightNowTiles([], NOW)).toMatchObject({
      ticketsUpdated: 0,
      needsYou: 0,
      workersLive: 0,
      tokensPerMinute: 0,
    });
  });

  it('buckets updates into 5-minute slices, oldest first, dropping the hour boundary and the future', () => {
    const state = liveStateFixture({
      rates: {
        tokens: { window_minutes: 5, minutes: [], peak_per_minute: 0 },
        tickets_updated: {
          window_minutes: 60,
          at_ms: [NOW - 60 * MINUTE, NOW - 59 * MINUTE, NOW - 2 * MINUTE, NOW, NOW + MINUTE],
          peak_per_hour: 3,
        },
      },
    });
    expect(updatedSparkline([state], NOW)).toEqual([1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2]);
  });

  it('decays as the clock moves without a new snapshot', () => {
    const states = glanceScenario();
    expect(rightNowTiles(states, NOW + 61 * MINUTE).ticketsUpdated).toBe(0);
  });
});

describe('HS2-ERTNJ7 Projects table rows', () => {
  it('sorts by need: problems first, then waiting, active, and stopped', () => {
    const rows = rightNowRows(glanceScenario(), NOW, open);
    expect(rows.map((row) => row.state)).toEqual(['may_be_stuck', 'waiting_on_you', 'active', 'stopped']);
  });

  it('breaks ties by the longest without progress, then by name', () => {
    const a = liveStateFixture({ store: 'a', name: 'Beta' });
    const b = liveStateFixture({
      store: 'b',
      name: 'Alpha',
      activity: { ...a.activity, last_progress_ms: NOW - 20 * MINUTE },
    });
    const c = liveStateFixture({ store: 'c', name: 'Aardvark' });
    expect(rightNowRows([a, b, c], NOW, open).map((row) => row.name)).toEqual(['Alpha', 'Aardvark', 'Beta']);
  });

  it('reports claims with phases, last progress, the largest ETA overrun, runway, and tokens', () => {
    const active = glanceScenario().find((state) => state.name === 'Hot Sheet 2')!;
    const row = rightNowRow(active, NOW, open);
    expect(row.work).toEqual([
      { slug: 'HS2-R76MMW', title: 'Coalesce reader refreshes', phase: 'initial_testing' },
      { slug: 'HS2-TG2ZV3', title: 'Pin the stats freshness line', phase: 'final_testing' },
    ]);
    expect(row.progressAgeMs).toBe(2 * MINUTE);
    expect(row.etaOverrunMs).toBe(18 * MINUTE);
    expect(row.runwayReady).toBe(6);
    expect(row.runwayEstimateMs).toBe(220 * MINUTE);
    expect(row.tokensPerMinute).toBe(1_600);
    expect(row.action).toBeUndefined();
  });

  it('omits overrun and progress when there are none', () => {
    const row = rightNowRow(
      liveStateFixture({ activity: { ...liveStateFixture().activity, last_progress_ms: null } }),
      NOW,
      open,
    );
    expect(row.progressAgeMs).toBeUndefined();
    expect(row.etaOverrunMs).toBeUndefined();
  });

  it('routes Waiting on you to Answer the first waiting ticket in the open project', () => {
    const waiting = glanceScenario().find((state) => state.state === 'waiting_on_you')!;
    expect(rightNowRow(waiting, NOW, open).action).toEqual({
      kind: 'answer',
      slug: 'UXR-2B7Q',
      projectId: 'review-e5f6',
    });
  });

  it.each(['may_be_stuck', 'stalled', 'halted'] as const)('routes %s to Open terminal', (state) => {
    const row = rightNowRow(liveStateFixture({ state, checkouts: ['p1'] }), NOW, open);
    expect(row.action).toEqual({ kind: 'open-terminal', terminalKey: 'p1:t1' });
  });

  it('offers no action without an open project, a terminal, a waiting ticket, or a problem', () => {
    const stuck = liveStateFixture({ state: 'may_be_stuck' });
    expect(rightNowRow(stuck, NOW, { ...open, projectFor: () => undefined }).action).toBeUndefined();
    expect(rightNowRow(stuck, NOW, { ...open, terminalFor: () => undefined }).action).toBeUndefined();
    expect(rightNowRow(liveStateFixture({ state: 'waiting_on_you' }), NOW, open).action).toBeUndefined();
    expect(rightNowRow(liveStateFixture({ state: 'stopped' }), NOW, open).action).toBeUndefined();
  });
});

describe('HS2-ERTNJ7 formatting helpers', () => {
  it.each([
    [0, '0s'],
    [45_000, '45s'],
    [34 * MINUTE, '34m'],
    [65 * MINUTE, '1h 05m'],
    [47 * 60 * MINUTE, '47h 00m'],
    [72 * 60 * MINUTE, '3d'],
    [-5, '0s'],
  ])('formats %d ms as %s', (ms, text) => {
    expect(formatDuration(ms)).toBe(text);
  });

  it("counts today's UTC bucket from its UTC midnight", () => {
    expect(completedSinceUtcMidnight({ '2026-11-06': 9, '2026-11-05': 4 }, NOW)).toEqual({
      count: 9,
      sinceMs: Date.parse('2026-11-06T00:00:00Z'),
    });
    expect(completedSinceUtcMidnight({}, NOW).count).toBe(0);
  });
});
