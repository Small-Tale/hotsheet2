import { describe, expect, it } from 'vitest';

import type { LiveStoreState } from './api';
import {
  formatGlanceDuration,
  hostCallouts,
  lastProgressDescription,
  liveChips,
  liveStateDescription,
  projectLiveSummary,
  runwayDescription,
  runwayLabel,
} from './project-live-summary';
import { DEMO_LIVE_NOW_MS, demoHostHealth, demoLiveState } from './ux-demo/live-section-fixtures';

const NOW = DEMO_LIVE_NOW_MS;
const MINUTE = 60_000;

function summary(...states: LiveStoreState[]) {
  const result = projectLiveSummary(states, NOW);
  if (!result) throw new Error('expected a summary');
  return result;
}

describe('formatGlanceDuration (HS2-F788GW)', () => {
  it('rounds down to whole minutes and drops zero parts', () => {
    expect(formatGlanceDuration(-5)).toBe('<1m');
    expect(formatGlanceDuration(59_999)).toBe('<1m');
    expect(formatGlanceDuration(18 * MINUTE)).toBe('18m');
    expect(formatGlanceDuration(60 * MINUTE)).toBe('1h');
    expect(formatGlanceDuration(220 * MINUTE)).toBe('3h 40m');
    expect(formatGlanceDuration(48 * 60 * MINUTE)).toBe('2d');
    expect(formatGlanceDuration(52 * 60 * MINUTE)).toBe('2d 4h');
  });
});

describe('projectLiveSummary (HS2-F788GW)', () => {
  it('is absent until one of the project stores has reported', () => {
    expect(projectLiveSummary([], NOW)).toBeUndefined();
  });

  it('derives the rates, workers, last progress, and runway of one store', () => {
    const quiet = summary(demoLiveState('quiet'));
    expect(quiet).toMatchObject({
      state: 'active',
      activity: 'active',
      tokensPerMinute: 1600,
      tokensPeakPerMinute: 6200,
      ticketsUpdatedPerHour: 14,
      ticketsPeakPerHour: 22,
      workersLive: 2,
      progressAgeMs: 2 * MINUTE,
      waiting: 0,
      overrunCount: 0,
      runway: { ready: 4, estimateMs: 220 * MINUTE },
    });
    expect(quiet.overrun).toBeUndefined();
    expect(liveStateDescription(quiet)).toBe('Active · 2 workers live');
    expect(lastProgressDescription(quiet)).toBe('last progress 2m ago');
  });

  it('sums linked stores, keeps the worst state, the freshest progress, and the worst overrun', () => {
    const attention = demoLiveState('attention'),
      stuck = { ...demoLiveState('may_be_stuck'), store: 'second' },
      later = {
        ...attention,
        store: 'third',
        claims: [{ ...attention.claims[0], slug: 'HS2-LATER1', eta_at_ms: NOW - 45 * MINUTE }],
        runway: { ready: 1, estimate_ms: null },
      };
    const combined = summary(attention, stuck, later);
    expect(combined.state).toBe('may_be_stuck');
    expect(combined.activity).toBe('may_be_stuck');
    // Waiting on you is the glance word, but the work-state row and chip read the activity word.
    const waiting = summary(attention);
    expect(waiting.state).toBe('waiting_on_you');
    expect(waiting.activity).toBe('active');
    expect(liveStateDescription(waiting)).toBe('Active · 2 workers live');
    expect(combined.tokensPerMinute).toBe(4800);
    expect(combined.workersLive).toBe(6);
    expect(combined.progressAgeMs).toBe(2 * MINUTE);
    expect(combined.overrun).toEqual({ slug: 'HS2-LATER1', title: attention.claims[0].title, ms: 45 * MINUTE });
    expect(combined.overrunCount).toBe(2);
    expect(combined.runway).toEqual({ ready: 5, estimateMs: 220 * MINUTE });
  });

  it('reports no progress before the idle detector saw any and no estimate without one', () => {
    const fresh = summary({
      ...demoLiveState('stopped'),
      activity: { ...demoLiveState('stopped').activity, last_progress_ms: null },
    });
    expect(fresh.progressAgeMs).toBeUndefined();
    expect(lastProgressDescription(fresh)).toBe('no progress yet');
    expect(liveStateDescription({ ...fresh, workersLive: 1 })).toBe('Stopped · 1 worker live');
    expect(runwayLabel(fresh.runway)).toBe('Queue runway');
    expect(runwayDescription(fresh.runway)).toBe('Queue runway · 0 ready');
  });
});

describe('liveChips (HS2-F788GW, docs/34-glance-surfaces.md §34.4)', () => {
  it('shows only the runway for a quiet, healthy project', () => {
    expect(liveChips(summary(demoLiveState('quiet')))).toEqual([
      { kind: 'runway', text: '4', title: 'Queue runway ~3h 40m · 4 ready', tone: 'neutral' },
    ]);
  });

  it('shows nothing at all for a stopped project with an empty queue', () => {
    expect(liveChips(summary(demoLiveState('stopped')))).toEqual([]);
  });

  it('puts Needs you, the worst ETA overrun, and the runway in reading order', () => {
    expect(liveChips(summary(demoLiveState('attention')))).toEqual([
      { kind: 'waiting', text: '2', title: 'Needs you · 2 tickets waiting on you', tone: 'pop' },
      { kind: 'overrun', text: '+18m', title: 'HS2-R76MMW past ETA by 18m', tone: 'warning' },
      { kind: 'runway', text: '4', title: 'Queue runway ~3h 40m · 4 ready', tone: 'neutral' },
    ]);
    const one = summary({ ...demoLiveState('attention'), waiting: { count: 1, tickets: [] } });
    expect(liveChips(one)[0]?.title).toBe('Needs you · 1 ticket waiting on you');
  });

  it('names how many claims are late when more than one is', () => {
    const attention = demoLiveState('attention');
    const twice = summary({ ...attention, claims: [...attention.claims, { ...attention.claims[0], slug: 'HS2-X' }] });
    expect(liveChips(twice).find((chip) => chip.kind === 'overrun')?.title).toBe(
      'HS2-R76MMW past ETA by 18m · 2 claims past ETA',
    );
  });

  it('brightens a problem state with a glyph-only chip whose title says why', () => {
    expect(liveChips(summary(demoLiveState('may_be_stuck')))[0]).toEqual({
      kind: 'state',
      text: '',
      title: 'May be stuck · last progress 34m ago',
      tone: 'warning',
    });
    expect(liveChips(summary(demoLiveState('stalled')))[0]?.tone).toBe('danger');
    expect(liveChips(summary(demoLiveState('halted')))[0]?.title).toBe('Halted · last progress 2m ago');
  });
});

describe('hostCallouts (HS2-F788GW, HS2-DTAVQA)', () => {
  it('is empty without a snapshot or while the machine is normal', () => {
    expect(hostCallouts(undefined)).toEqual([]);
    expect(hostCallouts(demoHostHealth('normal'))).toEqual([]);
  });

  it('describes a filling disk at its level', () => {
    expect(hostCallouts(demoHostHealth('disk-warning'))).toEqual([
      { signal: 'disk', level: 'warning', text: 'Disk 18 GB free · full in ~9h' },
    ]);
    expect(hostCallouts(demoHostHealth('disk-critical'))).toEqual([
      { signal: 'disk', level: 'critical', text: 'Disk 4 GB free · full in ~2h' },
    ]);
  });

  it('describes every signal, worst first, without guessing unknown values', () => {
    const snapshot = demoHostHealth('battery');
    const warn = { status: 'warning' as const, since: null };
    snapshot.signals.disk = {
      ...snapshot.signals.disk,
      ...warn,
      reason: 'low_free_space',
      free_bytes: null,
      hours_to_full: null,
    };
    snapshot.signals.memory = {
      ...snapshot.signals.memory,
      status: 'critical',
      reason: 'pressure',
      since: null,
      pressure: 'critical',
    };
    snapshot.signals.cpu = {
      ...snapshot.signals.cpu,
      ...warn,
      reason: 'sustained_cpu',
      busy_percent: 97.4,
      top_process: 'cargo',
    };
    snapshot.signals.network = {
      ...snapshot.signals.network,
      status: 'critical',
      reason: 'no_default_route',
      since: null,
    };
    snapshot.signals.thermal = { ...snapshot.signals.thermal, ...warn, reason: 'throttled' };
    expect(hostCallouts(snapshot)).toEqual([
      { signal: 'memory', level: 'critical', text: 'Memory pressure critical' },
      { signal: 'network', level: 'critical', text: 'No network route' },
      { signal: 'disk', level: 'warning', text: 'Disk low' },
      { signal: 'cpu', level: 'warning', text: 'CPU 97% busy · cargo' },
      { signal: 'power', level: 'warning', text: 'On battery · 64%' },
      { signal: 'thermal', level: 'warning', text: 'CPU throttled' },
    ]);
  });

  it('names swap growth, an unknown CPU reading, and the other power reasons', () => {
    const snapshot = demoHostHealth('normal');
    snapshot.signals.memory = {
      ...snapshot.signals.memory,
      status: 'warning',
      reason: 'swap_growth',
      swap_growth_bytes_per_hour: 512 * 1024 ** 2,
    };
    snapshot.signals.cpu = { ...snapshot.signals.cpu, status: 'warning', busy_percent: null, top_process: null };
    snapshot.signals.power = { ...snapshot.signals.power, status: 'critical', reason: 'lid_closed_on_battery' };
    expect(hostCallouts(snapshot).map((callout) => callout.text)).toEqual([
      'Lid closed on battery',
      'Swap growing 512 MB/h',
      'CPU busy',
    ]);
    snapshot.signals.power = { ...snapshot.signals.power, status: 'warning', reason: 'idle_sleep_allowed' };
    snapshot.signals.memory = { ...snapshot.signals.memory, reason: 'pressure', pressure: null };
    snapshot.signals.power.battery_percent = null;
    expect(hostCallouts(snapshot).map((callout) => callout.text)).toEqual([
      'Memory pressure warning',
      'CPU busy',
      'Idle sleep allowed',
    ]);
    snapshot.signals.power = { ...snapshot.signals.power, reason: 'on_battery' };
    expect(hostCallouts(snapshot).at(-1)?.text).toBe('On battery');
  });
});
