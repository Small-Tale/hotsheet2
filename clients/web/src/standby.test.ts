import { describe, expect, it, vi } from 'vitest';

import type { LiveStoreState } from './api';
import {
  createStandbyIdleTimer,
  formatStandbyAge,
  formatStandbyClock,
  parseStandbySettings,
  STANDBY_ALERT_WORDS,
  standbyFreshnessLabel,
  standbyModel,
  standbyWordLabel,
} from './standby';

const MINUTE = 60_000;
const NOW = 1_800_000_000_000; // a whole minute

/** A snapshot in the server's exact shape (`GET /live-state`). */
function liveFixture(overrides: Partial<LiveStoreState> = {}): LiveStoreState {
  return {
    store: 's1',
    root: '/work/demo.hs2',
    name: 'Hot Sheet 2',
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
        slug: 'HS2-R76MMW',
        title: 'Glance work',
        worker: 'w1',
        phase: 'working',
        claimed_at_ms: NOW - 10 * MINUTE,
        lease_expires_at_ms: NOW + 20 * MINUTE,
        eta_at_ms: null,
      },
    ],
    workers: { live: 2, expected: 2 },
    waiting: { count: 0, tickets: [] },
    runway: { ready: 3, estimate_ms: null },
    rates: {
      tokens: {
        window_minutes: 5,
        minutes: [
          { start_ms: NOW - 4 * MINUTE, tokens: 5_000 },
          { start_ms: NOW, tokens: 3_000 },
        ],
        peak_per_minute: 6_000,
      },
      tickets_updated: {
        window_minutes: 60,
        at_ms: [NOW - 70 * MINUTE, NOW - 30 * MINUTE, NOW - MINUTE],
        peak_per_hour: 20,
      },
    },
    ...overrides,
  };
}

const stalled = liveFixture({
  store: 's2',
  name: 'Small Tale Website',
  checkouts: ['site-cd34'],
  state: 'stalled',
  activity: {
    ...liveFixture().activity,
    word: 'stalled',
    last_progress_ms: NOW - 12 * MINUTE,
    stalled: [{ slug: 'STW-4K2P', worker: 'w2' }],
  },
  claims: [],
  workers: { live: 0, expected: 1 },
});
const waiting = liveFixture({
  store: 's3',
  name: 'UX Review',
  checkouts: ['ux-ef56'],
  state: 'waiting_on_you',
  waiting: {
    count: 2,
    tickets: [
      { ticket: 'a', slug: 'UX-1', title: 'One', reason: 'feedback' },
      { ticket: 'b', slug: 'UX-2', title: 'Two', reason: 'feedback' },
    ],
  },
});
const stopped = liveFixture({
  store: 's4',
  name: 'Kerf',
  checkouts: ['kerf-gh78'],
  state: 'stopped',
  claims: [],
  runway: { ready: 0, estimate_ms: null },
});

describe('standby settings contract (HS2-EQVZTZ)', () => {
  const view = {
    defaults: { enabled: true, idle_minutes: 15 },
    configured: { enabled: null, idle_minutes: 30 },
    effective: { enabled: true, idle_minutes: 30 },
  };

  it('accepts the server shape, with null configured values', () => {
    expect(parseStandbySettings(view)).toBe(view);
  });

  it('rejects drift in any group', () => {
    expect(() => parseStandbySettings({ ...view, effective: { enabled: 'yes', idle_minutes: 30 } })).toThrow(
      'standby settings',
    );
    expect(() => parseStandbySettings({ ...view, defaults: { enabled: true, idle_minutes: null } })).toThrow();
    expect(() => parseStandbySettings({ ...view, configured: { enabled: null, idle_minutes: 0 } })).toThrow();
    expect(() => parseStandbySettings(null)).toThrow();
  });
});

describe('standby display model', () => {
  it('sums the gauge and count over every project for All projects (m4)', () => {
    const model = standbyModel([stopped, liveFixture(), waiting], NOW);
    expect(model.scopeLabel).toBe('All projects');
    // 8,000 tokens over the 5-minute window per store; three stores share the fixture rates.
    expect(model.tokensPerMinute).toBe(3 * 1_600);
    expect(model.tokensPeakPerMinute).toBe(18_000);
    // The 70-minute-old update is outside the hour.
    expect(model.ticketsUpdated).toBe(6);
    expect(model.rows.map((row) => row.title)).toEqual(['Hot Sheet 2', 'Kerf', 'UX Review']);
    expect(model.rows[0]).toMatchObject({ word: 'active', detail: 'Active · 2m · HS2-R76MMW', alert: false });
    expect(model.rows[1]).toMatchObject({ word: 'stopped', detail: 'Stopped · queue empty', alert: false });
    expect(model.rows[2]).toMatchObject({ detail: 'Waiting on you · 2 feedback requests', alert: true });
  });

  it('lights the problem row and adds one amber callout per problem (m5)', () => {
    const quiet = standbyModel([liveFixture(), stopped], NOW);
    expect(quiet.alerts).toEqual([]);
    expect(quiet.rows.every((row) => !row.alert)).toBe(true);
    const loud = standbyModel([liveFixture(), stalled, stopped], NOW);
    expect(loud.alerts).toEqual([
      {
        key: 's2',
        word: 'stalled',
        title: 'Small Tale Website stalled',
        detail: 'STW-4K2P · lease expired · no progress for 12m',
      },
    ]);
    expect(loud.rows.filter((row) => row.alert).map((row) => row.key)).toEqual(['s2']);
    const halted = liveFixture({
      state: 'halted',
      activity: { ...liveFixture().activity, halted: [{ worker: 'w', message: 'usage limit reached', at_ms: NOW }] },
    });
    expect(standbyModel([halted], NOW).alerts[0]).toMatchObject({
      title: 'Hot Sheet 2 halted',
      detail: 'usage limit reached',
    });
    expect(standbyModel([waiting], NOW).alerts[0]?.title).toBe('UX Review is waiting on you');
    expect([...STANDBY_ALERT_WORDS].sort()).toEqual(['halted', 'stalled', 'waiting_on_you']);
  });

  it('always covers every project and counts only feedback on Started tickets (HS2-JGW031)', () => {
    const waitingFeedback = liveFixture({
      store: 's5',
      name: 'Kerf',
      checkouts: ['kerf-ef56'],
      state: 'waiting_on_you',
      waiting: { count: 2, tickets: [{ ticket: 'a', slug: 'KF-A', title: 'A', reason: 'feedback' }] },
    });
    const model = standbyModel([liveFixture(), waitingFeedback], NOW);
    expect(model.scopeLabel).toBe('All projects');
    expect(model.rows.map((row) => row.key)).toEqual(['s1', 's5'].sort((a, b) => a.localeCompare(b)));
    expect(model.rows.find((row) => row.key === 's5')?.detail).toBe('Waiting on you · 2 feedback requests');
    expect(standbyModel([], NOW)).toMatchObject({ tokensPerMinute: 0, ticketsUpdated: 0, rows: [], alerts: [] });
  });

  it('words every state and age', () => {
    expect(standbyWordLabel('may_be_stuck')).toBe('May be stuck');
    expect(formatStandbyAge(30_000)).toBe('now');
    expect(formatStandbyAge(4 * MINUTE)).toBe('4m');
    expect(formatStandbyAge(125 * MINUTE)).toBe('2h 5m');
    expect(formatStandbyAge(120 * MINUTE)).toBe('2h');
    expect(formatStandbyAge(50 * 60 * MINUTE)).toBe('2d');
    const noProgress = liveFixture({
      state: 'may_be_stuck',
      activity: { ...liveFixture().activity, last_progress_ms: null },
      claims: [],
    });
    expect(standbyModel([noProgress], NOW).rows[0]?.detail).toBe('May be stuck');
    const ready = liveFixture({ state: 'stopped', runway: { ready: 4, estimate_ms: null } });
    expect(standbyModel([ready], NOW).rows[0]?.detail).toBe('Stopped · 4 ready');
  });
});

describe('standby clock and freshness', () => {
  it('drops the day period from the clock face', () => {
    const at = new Date(2026, 10, 6, 14, 47, 12).getTime();
    expect(formatStandbyClock(at, 'en-US')).toBe('2:47');
    expect(formatStandbyClock(at, 'en-GB')).toBe('14:47');
  });

  it('never lets a dropped stream look healthy', () => {
    const at = new Date(2026, 10, 6, 2, 47, 12).getTime();
    expect(standbyFreshnessLabel({ live: true }, 'en-US')).toBe('live');
    expect(standbyFreshnessLabel({ live: true, updatedAtMs: at }, 'en-US')).toBe('live · updated 2:47:12');
    expect(standbyFreshnessLabel({ live: false, updatedAtMs: at }, 'en-US')).toBe(
      'connection lost · last update 2:47:12',
    );
    expect(standbyFreshnessLabel({ live: false }, 'en-US')).toBe('connection lost');
  });
});

describe('standby idle timer', () => {
  function harness(idle: number | undefined) {
    let clock = 0;
    const timers: { at: number; run: () => void; id: number }[] = [];
    let next = 0;
    const onIdle = vi.fn();
    const config = { idle };
    const timer = createStandbyIdleTimer({
      idleMs: () => config.idle,
      onIdle,
      now: () => clock,
      setTimer: (run, ms) => {
        timers.push({ at: clock + ms, run, id: ++next });
        return next;
      },
      clearTimer: (id) => {
        const index = timers.findIndex((item) => item.id === id);
        if (index >= 0) timers.splice(index, 1);
      },
    });
    const advance = (ms: number) => {
      clock += ms;
      for (;;) {
        const due = timers
          .filter((item) => item.at <= clock)
          .sort((a, b) => a.at - b.at)
          .at(0);
        if (!due) return;
        timers.splice(timers.indexOf(due), 1);
        due.run();
      }
    };
    return { timer, onIdle, advance, config, pending: () => timers.length };
  }

  it('enters once after the threshold with no activity, and activity restarts the window', () => {
    const { timer, onIdle, advance, pending } = harness(10 * MINUTE);
    timer.resume();
    advance(9 * MINUTE);
    timer.activity();
    timer.activity();
    // Activity only records a time: still one pending timer.
    expect(pending()).toBe(1);
    advance(2 * MINUTE);
    expect(onIdle).not.toHaveBeenCalled();
    advance(8 * MINUTE);
    expect(onIdle).toHaveBeenCalledTimes(1);
    // Paused after entering: more time and activity do nothing until resumed.
    timer.activity();
    advance(60 * MINUTE);
    expect(onIdle).toHaveBeenCalledTimes(1);
    timer.resume();
    advance(10 * MINUTE);
    expect(onIdle).toHaveBeenCalledTimes(2);
  });

  it('stays off while disabled and re-arms when the threshold changes', () => {
    const { timer, onIdle, advance, config, pending } = harness(undefined);
    timer.resume();
    advance(600 * MINUTE);
    expect(onIdle).not.toHaveBeenCalled();
    expect(pending()).toBe(0);
    config.idle = 5 * MINUTE;
    timer.reconfigure();
    advance(5 * MINUTE);
    expect(onIdle).toHaveBeenCalledTimes(1);
    timer.resume();
    config.idle = undefined;
    timer.reconfigure();
    advance(60 * MINUTE);
    expect(onIdle).toHaveBeenCalledTimes(1);
    timer.dispose();
    timer.activity();
    expect(pending()).toBe(0);
  });

  it('pausing cancels a pending entry (forced entry, then exit)', () => {
    const { timer, onIdle, advance } = harness(5 * MINUTE);
    timer.resume();
    advance(4 * MINUTE);
    timer.pause();
    advance(10 * MINUTE);
    expect(onIdle).not.toHaveBeenCalled();
    timer.resume();
    advance(4 * MINUTE);
    expect(onIdle).not.toHaveBeenCalled();
    advance(MINUTE);
    expect(onIdle).toHaveBeenCalledTimes(1);
  });
});
