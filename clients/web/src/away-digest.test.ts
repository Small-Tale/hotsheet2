import { describe, expect, it } from 'vitest';

import type { LiveStoreState } from './api';
import {
  awayEventRow,
  beginAwayRecord,
  completionsBetween,
  finishAwayDigest,
  formatAwayDuration,
  observeAwayHost,
  observeAwayStates,
} from './away-digest';
import type { HostHealthSnapshot } from './host-health';

const MINUTE = 60_000;
const T0 = 1_700_000_000_000;

/** One store's live snapshot in the server's shape; synthetic content only. */
function store(
  overrides: Partial<Omit<LiveStoreState, 'activity'>> & { activity?: Partial<LiveStoreState['activity']> } = {},
) {
  const base: LiveStoreState = {
    store: 'stw',
    root: '/work/stw.hs2',
    name: 'Small Tale Website',
    checkouts: ['demo-checkout'],
    epoch_ms: 1,
    revision: 1,
    state: 'active',
    activity: {
      word: 'active',
      active_since_ms: T0,
      stopped_at_ms: null,
      last_progress_ms: T0,
      idle_deadline_ms: null,
      stalled: [],
      halted: [],
      causes: [],
    },
    claims: [],
    workers: { live: 1, expected: 1 },
    waiting: { count: 0, tickets: [] },
    runway: { ready: 2, estimate_ms: null },
    rates: {
      tokens: { window_minutes: 5, minutes: [], peak_per_minute: 1_000 },
      tickets_updated: { window_minutes: 60, at_ms: [], peak_per_hour: 10 },
    },
  };
  return { ...base, ...overrides, activity: { ...base.activity, ...overrides.activity } };
}

const claim = (slug: string) => ({
  ticket: slug,
  slug,
  title: `Ticket ${slug}`,
  worker: 'worker-1',
  phase: 'working',
  claimed_at_ms: T0,
  lease_expires_at_ms: T0 + 30 * MINUTE,
  eta_at_ms: null,
});

function host(disk: 'normal' | 'warning' | 'critical', free = 12 * 1024 ** 3): HostHealthSnapshot {
  const header = { status: 'normal' as const, reason: null, since: null };
  return {
    host: { name: 'host', os: 'macos', arch: 'aarch64' },
    sampled_at: null,
    level: disk,
    signals: {
      disk: {
        ...header,
        status: disk,
        free_bytes: free,
        total_bytes: 500 * 1024 ** 3,
        consumption_bytes_per_hour: null,
        hours_to_full: null,
      },
      memory: { ...header, pressure: null, swap_used_bytes: null, swap_growth_bytes_per_hour: null },
      cpu: { ...header, busy_percent: null, busy_for_seconds: null, top_process: null, top_process_percent: null },
      power: {
        ...header,
        on_battery: null,
        battery_percent: null,
        lid_closed: null,
        sleep_prevented: null,
        idle_sleep_minutes: null,
      },
      network: { ...header, reachable: null, interface: null },
      thermal: { ...header, throttled: null, cpu_speed_limit: null },
    },
  };
}

const map = (...states: LiveStoreState[]) => new Map(states.map((state) => [state.store, state]));

describe('away digest (HS2-JJ9RYN)', () => {
  it('an uneventful standby exits silently', () => {
    const before = store();
    const record = beginAwayRecord([before], T0);
    observeAwayStates(record, map(before), [store({ revision: 2 })], ['demo-checkout'], T0 + MINUTE);
    expect(finishAwayDigest(record, [before], 0, T0 + 60 * MINUTE)).toBeUndefined();
  });

  it('records one row per new stall, wait, usage-limit pause, host crossing, and finished queue', () => {
    const before = store({ claims: [claim('STW-1')] });
    const record = beginAwayRecord([before], T0);
    // A stall, a ticket waiting on the user, and a usage-limit pause arrive in one snapshot.
    const stalled = store({
      revision: 2,
      claims: [claim('STW-1'), claim('STW-2')],
      waiting: { count: 1, tickets: [{ ticket: 'STW-3', slug: 'STW-3', title: 'Review copy', reason: 'review' }] },
      activity: {
        stalled: [{ slug: 'STW-2', worker: 'worker-1' }],
        causes: [
          {
            kind: 'usage_limit',
            worker: 'worker-1',
            terminal: null,
            since_ms: T0 + 10 * MINUTE,
            resets_at_ms: T0 + 50 * MINUTE,
            prompt: null,
          },
        ],
      },
    });
    observeAwayStates(record, map(before), [stalled], ['demo-checkout'], T0 + 10 * MINUTE);
    // The same snapshot again adds nothing.
    observeAwayStates(record, map(stalled), [stalled], ['demo-checkout'], T0 + 11 * MINUTE);
    // The pause clears, then the queue finishes.
    const resumed: LiveStoreState = { ...stalled, revision: 3, activity: { ...stalled.activity, causes: [] } };
    observeAwayStates(record, map(stalled), [resumed], ['demo-checkout'], T0 + 50 * MINUTE);
    const done: LiveStoreState = { ...resumed, revision: 4, state: 'stopped', runway: { ready: 0, estimate_ms: null } };
    observeAwayStates(record, map(resumed), [done], ['demo-checkout'], T0 + 90 * MINUTE);
    observeAwayHost(record, host('normal'), host('warning'), T0 + 20 * MINUTE);
    // Staying in warning is not a second crossing; escalating to critical is.
    observeAwayHost(record, host('warning'), host('warning'), T0 + 21 * MINUTE);
    observeAwayHost(record, host('warning'), host('critical', 5 * 1024 ** 3), T0 + 22 * MINUTE);

    const digest = finishAwayDigest(record, [done], 0, T0 + 100 * MINUTE)!;
    expect(digest.events.map((event) => event.kind)).toEqual([
      'stalled',
      'waiting',
      'usage_limit',
      'host',
      'host',
      'queue_finished',
    ]);
    expect(digest.events[0]).toMatchObject({ slug: 'STW-2', projectId: 'demo-checkout' });
    expect(digest.events[1]).toMatchObject({ slug: 'STW-3', projectId: 'demo-checkout' });
    expect(digest.events[2]).toMatchObject({ resetsAtMs: T0 + 50 * MINUTE, resumedAtMs: T0 + 50 * MINUTE });
    expect(digest.events[3]).toMatchObject({ signal: 'disk', level: 'warning' });
    expect(digest.events[4]).toMatchObject({ signal: 'disk', level: 'critical', detail: '5 GB free' });
    // Both tickets worked during the run count toward the finished queue.
    expect(digest.events[5]).toMatchObject({ tickets: 2 });
    expect(digest.needYou).toBe(1);
  });

  it('a store first seen during standby counts from its first snapshot', () => {
    const record = beginAwayRecord([], T0);
    const first = store({ activity: { stalled: [{ slug: 'STW-9', worker: 'w' }] } });
    observeAwayStates(record, new Map(), [first], [], T0 + MINUTE);
    expect(record.events).toEqual([]);
    const second = store({
      revision: 2,
      activity: {
        stalled: [
          { slug: 'STW-9', worker: 'w' },
          { slug: 'STW-10', worker: 'w' },
        ],
      },
    });
    observeAwayStates(record, map(first), [second], [], T0 + 2 * MINUTE);
    // Not an open project: no action target.
    expect(record.events).toEqual([
      { kind: 'stalled', store: 'stw', name: 'Small Tale Website', slug: 'STW-10', atMs: T0 + 2 * MINUTE },
    ]);
  });

  it('sums token minutes inside the window once, even when snapshots repeat them', () => {
    const before = store();
    const record = beginAwayRecord([before], T0);
    const minutes = (...pairs: [number, number][]) =>
      store({
        revision: 2,
        rates: {
          ...before.rates,
          tokens: {
            window_minutes: 5,
            minutes: pairs.map(([start, tokens]) => ({ start_ms: start, tokens })),
            peak_per_minute: 1_000,
          },
        },
      });
    // A minute from well before standby is ignored; a repeated minute keeps its larger value.
    observeAwayStates(record, map(before), [minutes([T0 - 10 * MINUTE, 999], [T0, 100])], [], T0 + MINUTE);
    observeAwayStates(record, map(before), [minutes([T0, 150], [T0 + MINUTE, 200])], [], T0 + 2 * MINUTE);
    const digest = finishAwayDigest(record, [before], 0, T0 + 3 * MINUTE)!;
    expect(digest.tokens).toBe(350);
    expect(digest.events).toEqual([]);
  });

  it('counts completions inside the window once across projects that share a store', () => {
    const report = (completed_at: string[]) => ({
      completions: completed_at.length,
      scored: 0,
      bands: [],
      events: completed_at.map((at, index) => ({ slug: `HS-${index}`, completed_at: at, outcome: 'pending' as const })),
    });
    const inside = new Date(T0 + MINUTE).toISOString(),
      outside = new Date(T0 - MINUTE).toISOString();
    expect(completionsBetween([report([inside, outside]), report([inside])], T0, T0 + 10 * MINUTE)).toBe(1);
    // Completions alone make the standby eventful.
    const record = beginAwayRecord([], T0);
    expect(finishAwayDigest(record, [], 1, T0 + 10 * MINUTE)).toMatchObject({ completed: 1, events: [] });
  });

  it('formats durations', () => {
    expect(formatAwayDuration(30_000)).toBe('under a minute');
    expect(formatAwayDuration(26 * MINUTE)).toBe('26m');
    expect(formatAwayDuration((7 * 60 + 26) * MINUTE)).toBe('7h 26m');
  });

  it('words each row with its one resolving action', () => {
    const clock = (ms: number) => `t${String((ms - T0) / MINUTE)}`;
    const base = { store: 'stw', name: 'Small Tale Website' };
    expect(
      awayEventRow({ kind: 'stalled', ...base, projectId: 'p', slug: 'STW-2', atMs: T0 + 5 * MINUTE }, clock),
    ).toMatchObject({
      subject: 'Small Tale Website',
      text: 'stalled at t5 · STW-2',
      action: { label: 'Open', projectId: 'p', slug: 'STW-2' },
      icon: { name: 'octagon-alert' },
    });
    expect(
      awayEventRow({ kind: 'waiting', ...base, projectId: 'p', slug: 'STW-3', title: 'x', fromMs: T0 }, clock),
    ).toMatchObject({ text: 'waited on you from t0 · STW-3', action: { label: 'Answer' }, icon: { name: 'hand' } });
    // Without an open project there is nothing to open.
    expect(awayEventRow({ kind: 'stalled', ...base, slug: 'STW-2', atMs: T0 }, clock).action).toBeUndefined();
    const limit = { kind: 'usage_limit' as const, ...base, worker: 'w', sinceMs: T0 };
    expect(awayEventRow({ ...limit, resetsAtMs: T0 + 40 * MINUTE }, clock).text).toBe(
      'paused at a usage limit at t0 (resumes t40)',
    );
    expect(awayEventRow({ ...limit, resetsAtMs: null }, clock).text).toBe('paused at a usage limit at t0');
    expect(awayEventRow({ ...limit, resetsAtMs: null, resumedAtMs: T0 + 40 * MINUTE }, clock)).toMatchObject({
      text: 'paused 40m at a usage limit (resumed t40)',
      icon: { name: 'zap-off' },
    });
    expect(
      awayEventRow({ kind: 'host', signal: 'disk', level: 'warning', atMs: T0, detail: '12 GB free' }, clock),
    ).toMatchObject({ text: 'Disk space ran low (12 GB free) at t0', icon: { name: 'hard-drive' } });
    expect(
      awayEventRow({ kind: 'host', signal: 'network', level: 'critical', atMs: T0 }, clock).subject,
    ).toBeUndefined();
    expect(awayEventRow({ kind: 'queue_finished', ...base, atMs: T0, tickets: 1 }, clock).text).toBe(
      'finished its queue at t0 · 1 ticket',
    );
    expect(awayEventRow({ kind: 'queue_finished', ...base, atMs: T0, tickets: 0 }, clock)).toMatchObject({
      text: 'finished its queue at t0',
      icon: { name: 'check' },
    });
  });
});
