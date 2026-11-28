import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { LiveStoreState } from './api';
import {
  formatGlanceDuration,
  hostCallouts,
  lastProgressDescription,
  liveChips,
  liveCiClockRunning,
  liveStateDescription,
  projectLiveSummary,
  workersLive,
  workersLiveTitle,
} from './project-live-summary';
import { createTurnClock } from './turn-clock';
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

  it('derives the rates, workers, and last progress of one store, without queue runway (HS2-5A83MR)', () => {
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
    });
    expect(quiet).not.toHaveProperty('runway');
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
  });

  it('reports no progress before the idle detector saw any', () => {
    const fresh = summary({
      ...demoLiveState('stopped'),
      activity: { ...demoLiveState('stopped').activity, last_progress_ms: null },
    });
    expect(fresh.progressAgeMs).toBeUndefined();
    expect(lastProgressDescription(fresh)).toBe('no progress yet');
    expect(liveStateDescription({ ...fresh, workersLive: 1 })).toBe('Stopped · 1 worker live');
  });
});

describe('liveChips (HS2-F788GW, docs/34-glance-surfaces.md §34.4)', () => {
  it('shows no chips for a quiet, healthy project, even with Up Next work (HS2-5A83MR)', () => {
    // An older server still sends `runway`; it never becomes a chip.
    const older = { ...demoLiveState('quiet'), runway: { ready: 4, estimate_ms: 220 * MINUTE } };
    expect(liveChips(summary(older))).toEqual([]);
  });

  it('shows nothing at all for a stopped project', () => {
    expect(liveChips(summary(demoLiveState('stopped')))).toEqual([]);
  });

  it('puts Needs you, then the worst ETA overrun, in reading order', () => {
    expect(liveChips(summary(demoLiveState('attention')))).toEqual([
      { kind: 'waiting', text: '2', title: 'Needs you · 2 tickets waiting on you', tone: 'pop' },
      { kind: 'overrun', text: '+18m', title: 'HS2-R76MMW past ETA by 18m', tone: 'warning' },
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

  it('pairs the state word with the progress of the stores that show it (HS2-PPZQX9)', () => {
    const stalled = demoLiveState('stalled'),
      busy = { ...demoLiveState('quiet'), store: 'busy' };
    const stalledStore = {
      ...stalled,
      activity: { ...stalled.activity, last_progress_ms: NOW - 34 * MINUTE },
    };
    const combined = summary(stalledStore, { ...busy, activity: { ...busy.activity, last_progress_ms: NOW - 30_000 } });
    expect(combined.activity).toBe('stalled');
    // The project's freshest progress still names the Workers choice; the chip reads the stalled store's.
    expect(combined.progressAgeMs).toBe(30_000);
    expect(combined.activityProgressAgeMs).toBe(34 * MINUTE);
    expect(liveChips(combined)[0]?.title).toBe('Stalled · last progress 34m ago');
    expect(lastProgressDescription(combined)).toBe('last progress 34m ago');
    // A stalled store that never progressed says so, whatever the other stores did.
    const never = summary(
      { ...stalled, activity: { ...stalled.activity, last_progress_ms: null } },
      { ...busy, activity: { ...busy.activity, last_progress_ms: NOW - 30_000 } },
    );
    expect(liveChips(never)[0]?.title).toBe('Stalled · no progress yet');
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
      { signal: 'power', level: 'warning', text: 'On battery · 49%' },
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

describe('workersLive (HS2-6B9S7P)', () => {
  it('is absent without live state or at 0 workers', () => {
    expect(workersLive(undefined)).toBeUndefined();
    expect(workersLive(summary(demoLiveState('stopped')))).toBeUndefined();
  });

  it('carries the count and the freshest progress age into the Workers title', () => {
    const workers = workersLive(summary(demoLiveState('quiet')));
    expect(workers).toEqual({ live: 2, progressAgeMs: 2 * MINUTE });
    expect(workersLiveTitle(workers!)).toBe('2 workers live · last progress 2m ago');
    expect(workersLiveTitle({ live: 1 })).toBe('1 worker live · no progress yet');
    const fresh = summary({
      ...demoLiveState('quiet'),
      activity: { ...demoLiveState('quiet').activity, last_progress_ms: null },
    });
    expect(workersLive(fresh)).toEqual({ live: 2 });
  });
});

describe('main CI chip and row model (HS2-57BYEA)', () => {
  const run = (
    status: NonNullable<LiveStoreState['ci']>['status'],
    minutesAgo = 12,
  ): NonNullable<LiveStoreState['ci']> => ({
    status,
    checkout: 'demo-ab12',
    branch: 'main',
    sha: '9f8e7d6c5b4a',
    run_url: 'https://ci.example.test/runs/1',
    workflow: 'CI',
    started_at_ms: DEMO_LIVE_NOW_MS - (minutesAgo + 6) * MINUTE,
    finished_at_ms: status === 'in_progress' || status === 'queued' ? null : DEMO_LIVE_NOW_MS - minutesAgo * MINUTE,
    reported_at_ms: DEMO_LIVE_NOW_MS - minutesAgo * MINUTE,
  });
  const withCi = (ci?: NonNullable<LiveStoreState['ci']>) =>
    summary({ ...demoLiveState('quiet'), ...(ci ? { ci } : {}) });

  it('carries the newest reported run, or none for an older server', () => {
    expect(withCi().ci).toEqual({ status: 'unavailable', branch: 'main' });
    expect(withCi(run('failure')).ci).toMatchObject({ status: 'failure', shortSha: '9f8e7d6', ageMs: 12 * MINUTE });
  });

  it('adds a labelled CI chip only when the run fails, runs, or waits (HS2-505PHC)', () => {
    expect(liveChips(withCi(run('failure')))).toEqual([
      { kind: 'ci', text: 'CI failed', title: 'main CI failed · 9f8e7d6', tone: 'danger' },
    ]);
    // Started 18 minutes before the display clock.
    expect(liveChips(withCi(run('in_progress')))).toEqual([
      { kind: 'ci', text: 'CI · 18:00', title: 'main CI running for 18:00 · 9f8e7d6', tone: 'neutral' },
    ]);
    expect(liveChips(withCi(run('queued')))).toEqual([
      { kind: 'ci', text: 'CI · queued', title: 'main CI queued · 9f8e7d6', tone: 'neutral' },
    ]);
    for (const status of ['success', 'cancelled', 'unavailable'] as const)
      expect(liveChips(withCi(run(status)))).toEqual([]);
    expect(liveChips(withCi())).toEqual([]);
  });

  it('counts a running chip from the reported start, clamps clock skew, and words a missing start', () => {
    const at = (startedAtMs: number | null, nowMs: number) =>
      projectLiveSummary(
        [{ ...demoLiveState('quiet'), ci: { ...run('in_progress'), started_at_ms: startedAtMs } }],
        nowMs,
      );
    const chipAt = (startedAtMs: number | null, nowMs: number) => {
      const result = at(startedAtMs, nowMs);
      return result ? liveChips(result).find((chip) => chip.kind === 'ci') : undefined;
    };
    expect(chipAt(NOW - 5_000, NOW)?.text).toBe('CI · 0:05');
    expect(chipAt(NOW - 65_000, NOW)?.text).toBe('CI · 1:05');
    expect(chipAt(NOW - (3_600_000 + 123_000), NOW)?.text).toBe('CI · 1:02:03');
    // A start ahead of this machine's clock reads 0:00 rather than a negative time.
    expect(chipAt(NOW + 90_000, NOW)).toMatchObject({ text: 'CI · 0:00', title: 'main CI running for 0:00 · 9f8e7d6' });
    expect(chipAt(null, NOW)).toMatchObject({ text: 'CI · running', title: 'main CI running · 9f8e7d6' });
  });

  it('puts the CI chip after Needs you and the ETA overrun', () => {
    const chips = liveChips(summary({ ...demoLiveState('attention'), ci: run('failure') }));
    expect(chips.map((chip) => chip.kind)).toEqual(['waiting', 'overrun', 'ci']);
  });
});

describe('Live CI chip display clock (HS2-505PHC)', () => {
  const running = {
    ...demoLiveState('quiet'),
    ci: {
      status: 'in_progress' as const,
      checkout: 'demo-ab12',
      branch: 'main',
      sha: '9f8e7d6c5b4a',
      run_url: null,
      workflow: 'CI',
      started_at_ms: NOW - 65_000,
      finished_at_ms: null,
      reported_at_ms: NOW - 60_000,
    },
  };
  const shown = { visible: true, expanded: false };

  it('runs only while the page shows, the section is compact, and a run with a start is running', () => {
    expect(liveCiClockRunning([running], shown, NOW)).toBe(true);
    expect(liveCiClockRunning([running], { visible: false, expanded: false }, NOW)).toBe(false);
    expect(liveCiClockRunning([running], { visible: true, expanded: true }, NOW)).toBe(false);
    expect(liveCiClockRunning([{ ...running, ci: { ...running.ci, started_at_ms: null } }], shown, NOW)).toBe(false);
    for (const status of ['queued', 'failure', 'success', 'cancelled', 'unavailable'] as const)
      expect(liveCiClockRunning([{ ...running, ci: { ...running.ci, status } }], shown, NOW)).toBe(false);
    expect(liveCiClockRunning([demoLiveState('quiet')], shown, NOW)).toBe(false);
    expect(liveCiClockRunning([], shown, NOW)).toBe(false);
  });

  it('follows the newest reported run across the project stores', () => {
    const olderFailure = {
      ...running,
      store: 'older',
      ci: { ...running.ci, status: 'failure' as const, reported_at_ms: NOW - 600_000, finished_at_ms: NOW - 600_000 },
    };
    expect(liveCiClockRunning([olderFailure, running], shown, NOW)).toBe(true);
    const newerFailure = { ...olderFailure, ci: { ...olderFailure.ci, reported_at_ms: NOW } };
    expect(liveCiClockRunning([newerFailure, running], shown, NOW)).toBe(false);
  });

  describe('with fake timers', () => {
    beforeEach(() => {
      vi.useFakeTimers();
      vi.setSystemTime(NOW);
    });
    afterEach(() => {
      vi.useRealTimers();
    });

    it('advances the chip face each second from a render-only clock, and stops with the run', () => {
      let states: LiveStoreState[] = [running],
        clock = Date.now();
      const face = () => liveChips(projectLiveSummary(states, clock)!).find((chip) => chip.kind === 'ci')?.text;
      const timer = createTurnClock(
        () => liveCiClockRunning(states, shown, Date.now()),
        () => {
          clock = Date.now();
        },
        1_000,
      );
      timer.sync();
      expect(face()).toBe('CI · 1:05');
      vi.advanceTimersByTime(1_000);
      expect(face()).toBe('CI · 1:06');
      vi.advanceTimersByTime(54_000);
      expect(face()).toBe('CI · 2:00');
      // The run finishes: the next interval stops the clock and the chip says it failed.
      states = [{ ...running, ci: { ...running.ci, status: 'failure', finished_at_ms: Date.now() } }];
      vi.advanceTimersByTime(1_000);
      expect(timer.running()).toBe(false);
      expect(face()).toBe('CI failed');
      const frozen = clock;
      vi.advanceTimersByTime(10_000);
      expect(clock).toBe(frozen);
    });
  });
});

describe('stale main CI reports (HS2-CC7TCB)', () => {
  const HOUR = 60 * MINUTE;
  const reportedRun = (status: 'in_progress' | 'queued', reportedAgoMs: number, nowMs = NOW): LiveStoreState => ({
    ...demoLiveState('quiet'),
    ci: {
      status,
      checkout: 'demo-ab12',
      branch: 'main',
      sha: '8f6c8d81f0a1',
      run_url: 'https://github.com/acme/widgets/actions/runs/2983',
      workflow: 'CI',
      started_at_ms: nowMs - reportedAgoMs - 5 * MINUTE,
      finished_at_ms: null,
      reported_at_ms: nowMs - reportedAgoMs,
    },
  });
  const shown = { visible: true, expanded: false };
  const ciChip = (states: LiveStoreState[], nowMs = NOW) =>
    liveChips(projectLiveSummary(states, nowMs)!).find((chip) => chip.kind === 'ci');

  it('shows a run reported running over 2 h ago as unknown, with when it was last seen and no clock', () => {
    const stale = reportedRun('in_progress', 2 * HOUR + 5 * MINUTE);
    expect(summary(stale).ci).toMatchObject({
      status: 'unknown',
      lastSeen: 'running',
      shortSha: '8f6c8d8',
      ageMs: 2 * HOUR + 5 * MINUTE,
    });
    expect(ciChip([stale])).toEqual({
      kind: 'ci',
      text: 'CI · unknown',
      title: 'main CI status unknown · last seen running 2h 05m ago · 8f6c8d8',
      tone: 'neutral',
    });
    expect(liveCiClockRunning([stale], shown, NOW)).toBe(false);
    expect(ciChip([reportedRun('queued', 3 * HOUR)])?.title).toBe(
      'main CI status unknown · last seen queued 3h 00m ago · 8f6c8d8',
    );
  });

  it('moves fresh, stale, renewed by a new report, then finished', () => {
    // Fresh: reported 2 h ago exactly, the run still counts.
    expect(ciChip([reportedRun('in_progress', 2 * HOUR)])?.text).toBe('CI · 2:05:00');
    expect(liveCiClockRunning([reportedRun('in_progress', 2 * HOUR)], shown, NOW)).toBe(true);
    // Stale one millisecond later.
    expect(ciChip([reportedRun('in_progress', 2 * HOUR + 1)])?.text).toBe('CI · unknown');
    // A newer report of the same run renews it.
    expect(ciChip([reportedRun('in_progress', MINUTE)])?.text).toBe('CI · 6:00');
    // The run's end replaces it, and a finished run never goes stale.
    const failed = reportedRun('in_progress', 3 * HOUR);
    failed.ci = { ...failed.ci!, status: 'failure', finished_at_ms: NOW - 3 * HOUR };
    expect(ciChip([failed])).toMatchObject({ text: 'CI failed', tone: 'danger' });
  });

  describe('with fake timers', () => {
    beforeEach(() => {
      vi.useFakeTimers();
      vi.setSystemTime(NOW);
    });
    afterEach(() => {
      vi.useRealTimers();
    });

    it('stops the display clock when the running report goes stale, and starts it on a new report', () => {
      let states: LiveStoreState[] = [reportedRun('in_progress', 2 * HOUR - 2_000)],
        clock = Date.now();
      const timer = createTurnClock(
        () => liveCiClockRunning(states, shown, clock),
        () => {
          clock = Date.now();
        },
        1_000,
      );
      timer.sync();
      expect(timer.running()).toBe(true);
      vi.advanceTimersByTime(1_000);
      expect(ciChip(states, clock)?.text).toBe('CI · 2:04:59');
      // The tick that crosses the bound renders `unknown`; the next one stops the clock.
      vi.advanceTimersByTime(2_000);
      expect(ciChip(states, clock)?.text).toBe('CI · unknown');
      vi.advanceTimersByTime(1_000);
      expect(timer.running()).toBe(false);
      const frozen = clock;
      vi.advanceTimersByTime(10_000);
      expect(clock).toBe(frozen);
      // A new report of the run makes it believable again.
      states = [reportedRun('in_progress', 0, Date.now())];
      clock = Date.now();
      timer.sync();
      expect(timer.running()).toBe(true);
      vi.advanceTimersByTime(1_000);
      expect(ciChip(states, clock)?.text).toBe('CI · 5:01');
      timer.stop();
    });
  });
});
