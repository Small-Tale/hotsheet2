import { describe, expect, it, vi } from 'vitest';

import type { ChangeEvent, LiveCause, LiveStoreState } from './api';
import {
  aggregateLiveStates,
  awaitingInputCauses,
  createLiveStateStore,
  etaOverrunMs,
  gaugeFraction,
  isNewerLiveState,
  pausedUntilMs,
  progressAgeMs,
  ticketsUpdatedPerHour,
  TOKEN_HOURS,
  tokenHours,
  tokensPerMinute,
  worstLiveWord,
} from './live-state';
import { parseLiveStateList, parseLiveStoreState } from './server-response-parsers';

const MINUTE = 60_000;
const NOW = 1_800_000_000_000; // a whole minute

/** A snapshot in the server's exact shape (`GET /stores/{store_id}/live-state`). */
function liveFixture(overrides: Partial<LiveStoreState> = {}): LiveStoreState {
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
      idle_deadline_ms: NOW + 28 * MINUTE,
      stalled: [],
      halted: [],
      causes: [],
    },
    claims: [
      {
        ticket: '01M6N3H10PF7JHA3VHWS72N5Z1',
        slug: 'HS2-A',
        title: 'Glance work',
        worker: 'w1',
        phase: 'working',
        claimed_at_ms: NOW - 10 * MINUTE,
        lease_expires_at_ms: NOW + 20 * MINUTE,
        eta_at_ms: NOW - MINUTE,
      },
    ],
    workers: { live: 1, expected: 1 },
    waiting: { count: 0, tickets: [] },
    rates: {
      tokens: {
        window_minutes: 5,
        minutes: [
          { start_ms: NOW - 5 * MINUTE, tokens: 9_000 },
          { start_ms: NOW - 4 * MINUTE, tokens: 1_000 },
          { start_ms: NOW, tokens: 4_000 },
        ],
        peak_per_minute: 2_000,
      },
      tickets_updated: { window_minutes: 60, at_ms: [NOW - 59 * MINUTE, NOW - MINUTE], peak_per_hour: 4 },
    },
    ...overrides,
  };
}

const liveEvent = (live: unknown): ChangeEvent =>
  ({ kind: 'live_state', store: 's1', id: 's1', slug: '', live }) as ChangeEvent;

describe('live state helpers (HS2-JTTQMY)', () => {
  it('the parsers accept the server shape and reject a wrong word or missing field', () => {
    expect(parseLiveStoreState(liveFixture())).toEqual(liveFixture());
    expect(parseLiveStateList({ epoch_ms: 1, stores: [liveFixture()] }).stores).toHaveLength(1);
    expect(() => parseLiveStoreState({ ...liveFixture(), state: 'busy' })).toThrow('live state');
    const missing: Partial<LiveStoreState> = liveFixture();
    delete missing.rates;
    expect(() => parseLiveStoreState(missing)).toThrow('live state');
  });

  it('rates decay with the local clock from the pushed timestamps', () => {
    const state = liveFixture();
    // The current minute and the four before it: 1,000 + 4,000 over 5 minutes.
    expect(tokensPerMinute(state, NOW)).toBe(1_000);
    expect(tokensPerMinute(state, NOW + 30_000)).toBe(1_000);
    // A minute later the oldest bucket in the window leaves it.
    expect(tokensPerMinute(state, NOW + MINUTE)).toBe(800);
    expect(tokensPerMinute(state, NOW + 10 * MINUTE)).toBe(0);
    expect(
      tokensPerMinute(
        { ...state, rates: { ...state.rates, tokens: { ...state.rates.tokens, window_minutes: 0 } } },
        NOW,
      ),
    ).toBe(0);
    expect(ticketsUpdatedPerHour(state, NOW)).toBe(2);
    expect(ticketsUpdatedPerHour(state, NOW + 1.5 * MINUTE)).toBe(1);
    expect(ticketsUpdatedPerHour(state, NOW + 61 * MINUTE)).toBe(0);
  });

  it('ages, ETA overruns, and gauge positions', () => {
    const state = liveFixture();
    expect(progressAgeMs(state, NOW)).toBe(2 * MINUTE);
    expect(progressAgeMs({ ...state, activity: { ...state.activity, last_progress_ms: null } }, NOW)).toBeUndefined();
    expect(progressAgeMs(state, NOW - 3 * MINUTE)).toBe(0);
    const claim = state.claims[0];
    expect(etaOverrunMs(claim, NOW)).toBe(MINUTE);
    expect(etaOverrunMs(claim, NOW - 2 * MINUTE)).toBeUndefined();
    expect(etaOverrunMs({ ...claim, eta_at_ms: null }, NOW)).toBeUndefined();
    expect(gaugeFraction(500, 2_000)).toBe(0.25);
    expect(gaugeFraction(5_000, 2_000)).toBe(1);
    expect(gaugeFraction(-1, 2_000)).toBe(0);
    expect(gaugeFraction(0, 0)).toBe(0);
    expect(gaugeFraction(3, 0)).toBe(1);
  });

  it('the aggregate sums stores, scales to the summed peaks, and shows the worst word', () => {
    const a = liveFixture();
    const b = liveFixture({
      store: 's2',
      state: 'waiting_on_you',
      waiting: { count: 3, tickets: [] },
      workers: { live: 0, expected: 2 },
    });
    const total = aggregateLiveStates([a, b], NOW);
    expect(total).toMatchObject({
      stores: 2,
      state: 'waiting_on_you',
      tokensPerMinute: 2_000,
      tokensPeakPerMinute: 4_000,
      ticketsUpdatedPerHour: 4,
      ticketsPeakPerHour: 8,
      waiting: 3,
      workersLive: 1,
      workersExpected: 3,
    });
    expect(aggregateLiveStates([], NOW)).toMatchObject({ stores: 0, state: 'stopped', tokensPerMinute: 0 });
    expect(worstLiveWord([a, { ...b, state: 'halted' }, { ...a, state: 'stalled' }])).toBe('halted');
    expect(worstLiveWord([{ ...a, state: 'may_be_stuck' }, b])).toBe('may_be_stuck');
  });

  it('orders snapshots by server start, then revision', () => {
    expect(isNewerLiveState(undefined, liveFixture())).toBe(true);
    expect(isNewerLiveState(liveFixture({ revision: 4 }), liveFixture({ revision: 5 }))).toBe(true);
    expect(isNewerLiveState(liveFixture({ revision: 5 }), liveFixture({ revision: 5 }))).toBe(false);
    expect(isNewerLiveState(liveFixture({ revision: 5 }), liveFixture({ revision: 4 }))).toBe(false);
    // A restarted server counts revisions from 1 again.
    expect(isNewerLiveState(liveFixture({ revision: 90 }), liveFixture({ epoch_ms: 2, revision: 1 }))).toBe(true);
    expect(isNewerLiveState(liveFixture({ epoch_ms: 2, revision: 1 }), liveFixture({ revision: 90 }))).toBe(false);
  });
});

describe('hourly token buckets (HS2-TRZGT3)', () => {
  const HOUR = 60 * MINUTE;
  const current = Math.floor(NOW / HOUR) * HOUR;
  const withHours = (hours: { start_ms: number; tokens: number }[]) =>
    liveFixture({
      rates: { ...liveFixture().rates, tokens: { ...liveFixture().rates.tokens, hours } },
    });

  it('the parser accepts hourly buckets, their absence from an older server, and rejects a bad bucket', () => {
    const state = withHours([{ start_ms: current, tokens: 5 }]);
    expect(parseLiveStoreState(state).rates.tokens.hours).toEqual([{ start_ms: current, tokens: 5 }]);
    expect(parseLiveStoreState(liveFixture()).rates.tokens.hours).toBeUndefined();
    expect(() => parseLiveStoreState(withHours([{ start_ms: current, tokens: 'many' as unknown as number }]))).toThrow(
      'live state',
    );
  });

  it('aligns twelve hours to the render clock, oldest first, with missing hours as zero', () => {
    const state = withHours([
      { start_ms: current - 11 * HOUR, tokens: 1 },
      { start_ms: current - HOUR, tokens: 2 },
      { start_ms: current, tokens: 3 },
    ]);
    const hours = tokenHours(state, NOW);
    expect(hours).toHaveLength(TOKEN_HOURS);
    expect(hours).toEqual([1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 3]);
    // Once the hour turns before the next snapshot, the oldest bucket leaves and the new hour is 0.
    expect(tokenHours(state, current + HOUR)).toEqual([0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 3, 0]);
    // An older server without buckets reads twelve zeros.
    expect(tokenHours(liveFixture(), NOW)).toEqual(Array.from({ length: 12 }, () => 0));
  });
});

describe('stuck causes (HS2-EGKKA0)', () => {
  const pause = (worker: string, resets: number | null): LiveCause => ({
    kind: 'usage_limit',
    worker,
    terminal: `t-${worker}`,
    since_ms: NOW - MINUTE,
    resets_at_ms: resets,
    prompt: null,
  });
  const prompt = (worker: string, which: 'permission' | 'question'): LiveCause => ({
    kind: 'awaiting_input',
    worker,
    terminal: `t-${worker}`,
    since_ms: NOW - 2 * MINUTE,
    resets_at_ms: null,
    prompt: which,
  });
  const withCauses = (causes: LiveCause[]) => {
    const state = liveFixture();
    return { ...state, activity: { ...state.activity, causes } };
  };

  it('the parser accepts both causes and rejects an unknown kind or prompt', () => {
    const state = withCauses([pause('w1', NOW + 60 * MINUTE), prompt('w2', 'question')]);
    expect(parseLiveStoreState(state)).toEqual(state);
    expect(() => parseLiveStoreState(withCauses([{ ...pause('w1', null), kind: 'idle' } as never]))).toThrow(
      'live state',
    );
    expect(() => parseLiveStoreState(withCauses([{ ...prompt('w1', 'question'), prompt: 'shell' } as never]))).toThrow(
      'live state',
    );
    const missing = liveFixture();
    delete (missing.activity as Partial<LiveStoreState['activity']>).causes;
    expect(() => parseLiveStoreState(missing)).toThrow('live state');
  });

  it('pausedUntilMs: none, unknown when any reset is unstated, else the latest reset', () => {
    expect(pausedUntilMs(withCauses([]))).toBeUndefined();
    expect(pausedUntilMs(withCauses([prompt('w1', 'permission')]))).toBeUndefined();
    expect(pausedUntilMs(withCauses([pause('w1', NOW + MINUTE)]))).toBe(NOW + MINUTE);
    expect(pausedUntilMs(withCauses([pause('w1', NOW + MINUTE), pause('w2', NOW + 9 * MINUTE)]))).toBe(
      NOW + 9 * MINUTE,
    );
    expect(pausedUntilMs(withCauses([pause('w1', NOW + MINUTE), pause('w2', null)]))).toBeNull();
    // A reset in the past is still reported: the session waits for a new prompt.
    expect(pausedUntilMs(withCauses([pause('w1', NOW - MINUTE)]))).toBe(NOW - MINUTE);
  });

  it('awaitingInputCauses lists only terminal prompts', () => {
    const causes = [pause('w1', null), prompt('w2', 'permission'), prompt('w3', 'question')];
    expect(awaitingInputCauses(withCauses(causes)).map((cause) => cause.worker)).toEqual(['w2', 'w3']);
    expect(awaitingInputCauses(withCauses([]))).toEqual([]);
  });

  it('a pushed snapshot replaces the causes, and a later one without them clears them', () => {
    const store = createLiveStateStore();
    store.applyEvents([liveEvent(withCauses([prompt('w1', 'question')]))]);
    expect(awaitingInputCauses(store.states.value.get('s1')!)).toHaveLength(1);
    store.applyEvents([liveEvent({ ...withCauses([]), revision: 2 })]);
    expect(awaitingInputCauses(store.states.value.get('s1')!)).toEqual([]);
  });
});

describe('live state store (HS2-JTTQMY)', () => {
  it('applies events in order and ignores replays, stale revisions, other kinds, and bad payloads', () => {
    const store = createLiveStateStore();
    const seen: number[] = [];
    store.states.subscribe((map) => seen.push(map.get('s1')?.revision ?? 0));
    expect(store.applyEvents([liveEvent(liveFixture({ revision: 2 }))])).toBe(1);
    // A cursor replay repeats revision 2, and an out-of-order revision 1 arrives late.
    expect(store.applyEvents([liveEvent(liveFixture({ revision: 2 })), liveEvent(liveFixture({ revision: 1 }))])).toBe(
      0,
    );
    expect(
      store.applyEvents([
        { kind: 'updated', store: 's1', id: 'x', slug: 'HS2-X' },
        liveEvent({ store: 's1', revision: 9 }),
        { kind: 'live_state', store: 's1', id: 's1', slug: '' },
      ]),
    ).toBe(0);
    expect(store.applyEvents([liveEvent(liveFixture({ revision: 3, state: 'stalled' }))])).toBe(1);
    expect(store.states.value.get('s1')?.state).toBe('stalled');
    expect(seen.at(-1)).toBe(3);
    expect(store.forProject('demo-ab12').map((state) => state.store)).toEqual(['s1']);
    expect(store.forProject('other')).toEqual([]);
    // HS2-JGW031: only stores linked to an open project, each once.
    expect(store.forProjects(['other', 'demo-ab12', 'demo-ab12']).map((state) => state.store)).toEqual(['s1']);
    expect(store.forProjects([])).toEqual([]);
    store.reset();
    expect(store.states.value.size).toBe(0);
    // Empty then refill.
    expect(store.apply(liveFixture({ revision: 1 }))).toBe(true);
    expect(store.states.value.size).toBe(1);
  });

  it('resync replaces older snapshots, keeps newer pushed ones, and drops stores the server unhosted', async () => {
    const store = createLiveStateStore();
    store.apply(liveFixture({ store: 'pushed', revision: 7 }));
    store.apply(liveFixture({ store: 'gone', revision: 1 }));
    store.apply(liveFixture({ store: 'elsewhere', epoch_ms: 99, revision: 1 }));
    store.apply(liveFixture({ store: 'old', revision: 1 }));
    const transport = {
      liveStates: vi.fn(async () => ({
        epoch_ms: 1,
        stores: [liveFixture({ store: 'pushed', revision: 6 }), liveFixture({ store: 'old', revision: 2 })],
      })),
    };
    await expect(store.resync(transport)).resolves.toBe(true);
    const map = store.states.value;
    expect(map.get('pushed')?.revision).toBe(7);
    expect(map.get('old')?.revision).toBe(2);
    expect(map.has('gone')).toBe(false);
    expect(map.has('elsewhere')).toBe(true);
    // A failed read keeps what the store holds.
    const failing = { liveStates: vi.fn(async () => Promise.reject(new Error('offline'))) };
    await expect(store.resync(failing)).resolves.toBe(false);
    expect(store.states.value).toBe(map);
    store.remove(['pushed', 'missing']);
    expect(store.states.value.has('pushed')).toBe(false);
    const before = store.states.value;
    store.remove(['missing']);
    expect(store.states.value).toBe(before);
  });

  it('issues no request on its own: idle time reads nothing, each resync reads once', async () => {
    vi.useFakeTimers();
    try {
      const store = createLiveStateStore();
      const transport = { liveStates: vi.fn(async () => ({ epoch_ms: 1, stores: [liveFixture()] })) };
      await store.resync(transport);
      await vi.advanceTimersByTimeAsync(60 * MINUTE);
      expect(transport.liveStates).toHaveBeenCalledTimes(1);
      await store.resync(transport);
      expect(transport.liveStates).toHaveBeenCalledTimes(2);
    } finally {
      vi.useRealTimers();
    }
  });
});
