import { describe, expect, it } from 'vitest';

import type { LiveStoreState, LiveWord } from './api';
import {
  formatLiveDuration,
  type GridSidebarLive,
  gridSidebarLive,
  LIVE_WORD_LABELS,
  liveNeed,
  liveStateTitle,
  liveWordColor,
  liveWorkSummary,
  triageOrder,
} from './grid-sidebar-live';

const MINUTE = 60_000;
const NOW = 1_800_000_000_000;

/** A snapshot in the server's exact shape (HS2-JTTQMY); synthetic names only. */
function store(overrides: Partial<LiveStoreState> = {}): LiveStoreState {
  return {
    store: 's1',
    root: '/work/demo.hs2',
    name: 'demo',
    checkouts: ['demo-ab12'],
    epoch_ms: 1,
    revision: 1,
    state: 'active',
    activity: {
      word: 'active',
      active_since_ms: NOW - 10 * MINUTE,
      stopped_at_ms: null,
      last_progress_ms: NOW - 2 * MINUTE,
      idle_deadline_ms: null,
      stalled: [],
      halted: [],
    },
    claims: [
      {
        ticket: 't1',
        slug: 'HS2-A',
        title: 'Work',
        worker: 'w1',
        phase: 'initial_testing',
        claimed_at_ms: NOW - 10 * MINUTE,
        lease_expires_at_ms: NOW + 20 * MINUTE,
        eta_at_ms: null,
      },
    ],
    workers: { live: 1, expected: 2 },
    waiting: { count: 0, tickets: [] },
    runway: { ready: 0, estimate_ms: null },
    rates: {
      tokens: { window_minutes: 1, minutes: [{ start_ms: NOW, tokens: 600 }], peak_per_minute: 1000 },
      tickets_updated: { window_minutes: 60, at_ms: [NOW - MINUTE, NOW - 2 * MINUTE], peak_per_hour: 10 },
    },
    ...overrides,
  };
}

function reading(state: LiveWord, overrides: Partial<GridSidebarLive> = {}): GridSidebarLive {
  return {
    state,
    tokensPerMinute: 0,
    tokensPeakPerMinute: 0,
    ticketsUpdatedPerHour: 0,
    ticketsPeakPerHour: 0,
    waiting: 0,
    workersLive: 0,
    workersExpected: 0,
    ...overrides,
  };
}

describe('gridSidebarLive (HS2-E1AQE8)', () => {
  it('is undefined without a store', () => {
    expect(gridSidebarLive([], NOW)).toBeUndefined();
  });

  it('combines rates, peaks, and counts, and keeps the latest progress and claim', () => {
    const second = store({
      store: 's2',
      state: 'may_be_stuck',
      activity: { ...store().activity, word: 'may_be_stuck', last_progress_ms: NOW - 40 * MINUTE },
      claims: [{ ...store().claims[0], slug: 'HS2-B', phase: 'working', claimed_at_ms: NOW - MINUTE }],
      waiting: { count: 1, tickets: [{ ticket: 't9', slug: 'HS2-W', title: 'Q', reason: 'feedback' }] },
    });
    const live = gridSidebarLive([store(), second], NOW)!;
    expect(live).toMatchObject({
      state: 'may_be_stuck',
      tokensPerMinute: 1200,
      tokensPeakPerMinute: 2000,
      ticketsUpdatedPerHour: 4,
      ticketsPeakPerHour: 20,
      waiting: 1,
      workersLive: 2,
      workersExpected: 4,
      progressAgeMs: 2 * MINUTE,
      claim: { slug: 'HS2-B', phase: 'working' },
      waitingSlug: 'HS2-W',
    });
    expect(live.stoppedAgeMs).toBeUndefined();
  });

  it('records the stop age, stalled ticket, and halt message only when present', () => {
    const stopped = gridSidebarLive(
      [
        store({
          state: 'stopped',
          claims: [],
          activity: { ...store().activity, word: 'stopped', last_progress_ms: null, stopped_at_ms: NOW - 90 * MINUTE },
        }),
      ],
      NOW,
    )!;
    expect(stopped).toMatchObject({ state: 'stopped', stoppedAgeMs: 90 * MINUTE });
    expect(stopped.progressAgeMs).toBeUndefined();
    expect(stopped.claim).toBeUndefined();
    const halted = gridSidebarLive(
      [
        store({
          state: 'halted',
          activity: {
            ...store().activity,
            stalled: [{ slug: 'HS2-S', worker: 'w1' }],
            halted: [{ worker: 'w1', message: 'Usage limit', at_ms: NOW }],
          },
        }),
      ],
      NOW,
    )!;
    expect(halted).toMatchObject({ stalledSlug: 'HS2-S', haltedMessage: 'Usage limit' });
  });
});

describe('grid sidebar presentation helpers (HS2-E1AQE8)', () => {
  it('names every state word', () => {
    expect(Object.keys(LIVE_WORD_LABELS).sort()).toEqual(
      ['active', 'halted', 'may_be_stuck', 'stalled', 'stopped', 'waiting_on_you'].sort(),
    );
  });

  it('keeps normal states quiet and lets only problems take warning or danger tones (dark cockpit)', () => {
    expect(String(liveWordColor('stopped'))).toContain('text-quiet');
    expect(String(liveWordColor('active'))).toContain('success-on-quiet');
    expect(String(liveWordColor('waiting_on_you'))).toContain('pop-on-quiet');
    expect(String(liveWordColor('may_be_stuck'))).toContain('warning-on-quiet');
    expect(String(liveWordColor('stalled'))).toContain('danger-on-quiet');
    expect(String(liveWordColor('halted'))).toContain('danger-on-quiet');
  });

  it('formats title durations', () => {
    expect(formatLiveDuration(20_000)).toBe('less than a minute');
    expect(formatLiveDuration(MINUTE)).toBe('1 minute');
    expect(formatLiveDuration(34 * MINUTE)).toBe('34 minutes');
    expect(formatLiveDuration(60 * MINUTE)).toBe('1 hour');
    expect(formatLiveDuration(47 * 60 * MINUTE)).toBe('47 hours');
    expect(formatLiveDuration(72 * 60 * MINUTE)).toBe('3 days');
  });

  it('puts durations and causes in the title, never on the face (compact-surface rule)', () => {
    expect(liveStateTitle('Web', reading('may_be_stuck', { progressAgeMs: 34 * MINUTE }))).toBe(
      'Web: May be stuck, no progress for 34 minutes',
    );
    expect(
      liveStateTitle('Web', reading('active', { progressAgeMs: 2 * MINUTE, workersLive: 2, workersExpected: 2 })),
    ).toBe('Web: Active, last progress 2 minutes ago, 2 of 2 workers live');
    expect(liveStateTitle('Web', reading('stalled', { stalledSlug: 'HS2-S' }))).toBe(
      'Web: Stalled, the lease on HS2-S expired',
    );
    expect(liveStateTitle('Web', reading('halted', { haltedMessage: 'Usage limit' }))).toBe('Web: Halted, Usage limit');
    expect(liveStateTitle('Web', reading('waiting_on_you', { waiting: 2 }))).toBe(
      'Web: Waiting on you, 2 waiting on you',
    );
    expect(liveStateTitle('Web', reading('stopped', { stoppedAgeMs: 120 * MINUTE }))).toBe('Web: Stopped, for 2 hours');
    expect(liveStateTitle('Web', reading('stopped'))).toBe('Web: Stopped');
    expect(liveStateTitle('Web', reading('active'))).toBe('Web: Active');
    expect(liveStateTitle('Web', reading('may_be_stuck'))).toBe('Web: May be stuck');
    expect(liveStateTitle('Web', reading('stalled'))).toBe('Web: Stalled');
    expect(liveStateTitle('Web', reading('halted'))).toBe('Web: Halted');
  });

  it('summarises the current work for the row face', () => {
    expect(liveWorkSummary(reading('active', { claim: { slug: 'HS2-A', phase: 'initial_testing' } }))).toBe(
      'HS2-A · initial testing',
    );
    expect(liveWorkSummary(reading('active', { claim: { slug: 'HS2-A', phase: null } }))).toBe('HS2-A');
    expect(
      liveWorkSummary(reading('waiting_on_you', { waitingSlug: 'HS2-W', claim: { slug: 'HS2-A', phase: null } })),
    ).toBe('HS2-W');
    expect(liveWorkSummary(reading('stalled', { stalledSlug: 'HS2-S' }))).toBe('HS2-S');
    expect(liveWorkSummary(reading('stopped'))).toBeUndefined();
  });

  it('sorts triage by need and keeps project order for equal need and no live state', () => {
    const groups = [
      { id: 'a', live: reading('stopped') },
      { id: 'b' },
      { id: 'c', live: reading('active') },
      { id: 'd', live: reading('halted') },
      { id: 'e', live: reading('active') },
      { id: 'f', live: reading('waiting_on_you') },
      { id: 'g', live: reading('may_be_stuck') },
      { id: 'h', live: reading('stalled') },
    ];
    expect(triageOrder(groups).map((group) => group.id)).toEqual(['d', 'h', 'g', 'f', 'c', 'e', 'a', 'b']);
    expect(liveNeed(undefined)).toBeLessThan(liveNeed('stopped'));
  });
});
