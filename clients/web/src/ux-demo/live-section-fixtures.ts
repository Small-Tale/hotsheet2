import type { LiveStoreState, LiveWord } from '../api';
import type { HostHealthSnapshot } from '../host-health';

/**
 * Deterministic, synthetic live-state and host-health fixtures for the Live section demos (HS2-F788GW).
 * They follow the server's `GET /live-state` and `GET /host-health` shapes; no customer content.
 */
export const DEMO_LIVE_NOW_MS = Date.UTC(2026, 10, 6, 2, 30);
const MINUTE = 60_000;

export type DemoLiveScenario = 'quiet' | 'attention' | 'may_be_stuck' | 'stalled' | 'halted' | 'stopped';
export type DemoHostScenario = 'normal' | 'disk-warning' | 'disk-critical' | 'battery';

function minutes(tokensPerMinute: number) {
  return Array.from({ length: 5 }, (_, index) => ({
    start_ms: Math.floor(DEMO_LIVE_NOW_MS / MINUTE) * MINUTE - index * MINUTE,
    tokens: tokensPerMinute,
  }));
}

/** One store's synthetic snapshot for a scenario, at {@link DEMO_LIVE_NOW_MS}. */
export function demoLiveState(scenario: DemoLiveScenario): LiveStoreState {
  const word: LiveWord = scenario === 'quiet' || scenario === 'attention' ? 'active' : scenario;
  const attention = scenario === 'attention';
  const idle = scenario === 'stopped';
  return {
    store: 'demo-store',
    root: '<repo-root>.hs2',
    name: 'Hot Sheet 2',
    checkouts: ['hotsheet'],
    epoch_ms: DEMO_LIVE_NOW_MS - 3_600_000,
    revision: 1,
    state: attention ? 'waiting_on_you' : word,
    activity: {
      word,
      active_since_ms: idle ? null : DEMO_LIVE_NOW_MS - 90 * MINUTE,
      stopped_at_ms: idle ? DEMO_LIVE_NOW_MS - 40 * MINUTE : null,
      last_progress_ms: DEMO_LIVE_NOW_MS - (scenario === 'may_be_stuck' ? 34 : idle ? 41 : 2) * MINUTE,
      idle_deadline_ms: null,
      stalled: scenario === 'stalled' ? [{ slug: 'HS2-DEMO01', worker: 'worker-a' }] : [],
      halted:
        scenario === 'halted'
          ? [{ worker: 'worker-a', message: 'Usage limit reached', at_ms: DEMO_LIVE_NOW_MS - 5 * MINUTE }]
          : [],
      causes: [],
    },
    claims: idle
      ? []
      : [
          {
            ticket: '01DEMO01',
            slug: 'HS2-R76MMW',
            title: 'Build TicketList and TicketBoard around shared responsive TicketRow',
            worker: 'worker-a',
            phase: 'working',
            claimed_at_ms: DEMO_LIVE_NOW_MS - 80 * MINUTE,
            lease_expires_at_ms: DEMO_LIVE_NOW_MS + 20 * MINUTE,
            eta_at_ms: DEMO_LIVE_NOW_MS - (attention ? 18 : -30) * MINUTE,
          },
        ],
    workers: { live: idle ? 0 : 2, expected: 2 },
    waiting: attention
      ? {
          count: 2,
          tickets: [
            { ticket: '01DEMO02', slug: 'HS2-DEMO02', title: 'Document empty-state behavior', reason: 'feedback' },
            { ticket: '01DEMO03', slug: 'HS2-DEMO03', title: 'Review provider capability messaging', reason: 'review' },
          ],
        }
      : { count: 0, tickets: [] },
    rates: {
      tokens: { window_minutes: 5, minutes: minutes(idle ? 0 : 1600), peak_per_minute: 6200 },
      tickets_updated: {
        window_minutes: 60,
        at_ms: idle ? [] : Array.from({ length: 14 }, (_, index) => DEMO_LIVE_NOW_MS - (index + 1) * 3 * MINUTE),
        peak_per_hour: 22,
      },
    },
  };
}

/**
 * The CI runs a demo shows (HS2-505PHC): none reported, running, running before a start, queued, failed,
 * passed, running while the provider is unreachable for now, several runs in flight, and a run that cannot be
 * verified, which shows nothing (HS2-D3JJ49).
 */
export type DemoCiScenario =
  'none' | 'running' | 'running-no-start' | 'queued' | 'failed' | 'passed' | 'transient' | 'several' | 'unverifiable';

/** One synthetic reported run at {@link DEMO_LIVE_NOW_MS}. */
function demoRun(
  status: NonNullable<LiveStoreState['ci']>['status'],
  startedAgoMs: number | null,
  extra: Partial<NonNullable<LiveStoreState['ci']>> = {},
): NonNullable<LiveStoreState['ci']> {
  const finished = status === 'failure' || status === 'success';
  return {
    status,
    checkout: 'hotsheet',
    branch: 'main',
    sha: '9f8e7d6c5b4a',
    run_url: null,
    workflow: 'CI',
    started_at_ms: startedAgoMs === null ? null : DEMO_LIVE_NOW_MS - startedAgoMs,
    finished_at_ms: finished ? DEMO_LIVE_NOW_MS - 12 * MINUTE : null,
    reported_at_ms: DEMO_LIVE_NOW_MS - (finished ? 12 * MINUTE : 5_000),
    verification: 'verified',
    verify_by_ms: null,
    ...extra,
  };
}

/** The synthetic runs a scenario reports, newest first, as the server's `ci_runs` lists them. */
export function demoLiveCiRuns(scenario: DemoCiScenario): NonNullable<LiveStoreState['ci_runs']> {
  switch (scenario) {
    case 'none':
      return [];
    // Running 1:05, or queued 30 seconds ago; a finished run started 18 minutes ago and took six.
    case 'running':
      return [demoRun('in_progress', 65_000)];
    case 'running-no-start':
      return [demoRun('in_progress', null)];
    case 'queued':
      return [demoRun('queued', 30_000)];
    case 'failed':
      return [demoRun('failure', 18 * MINUTE)];
    case 'passed':
      return [demoRun('success', 18 * MINUTE)];
    // HS2-D3JJ49: the provider could not be read just now; the last known state stays shown.
    case 'transient':
      return [demoRun('in_progress', 65_000, { verification: 'transient' })];
    // HS2-D3JJ49: three workflows of one push in flight, each with its own chip.
    case 'several':
      return [
        demoRun('in_progress', 65_000),
        demoRun('in_progress', 3 * MINUTE + 10_000, {
          workflow: 'Platforms',
          reported_at_ms: DEMO_LIVE_NOW_MS - 6_000,
        }),
        demoRun('queued', 20_000, { workflow: 'Docs', reported_at_ms: DEMO_LIVE_NOW_MS - 7_000 }),
      ];
    // HS2-D3JJ49: nothing can verify it, so the server leaves it out and nothing shows.
    case 'unverifiable':
      return [];
    default:
      return scenario satisfies never;
  }
}

/** The newest synthetic run for a scenario, as the server's `ci`; `undefined` for none. */
export function demoLiveCi(scenario: DemoCiScenario): LiveStoreState['ci'] {
  return demoLiveCiRuns(scenario)[0];
}

const NORMAL = { status: 'normal' as const, reason: null, since: null };

/** The server host's synthetic health for a scenario. */
export function demoHostHealth(scenario: DemoHostScenario): HostHealthSnapshot {
  const disk = scenario === 'disk-warning' || scenario === 'disk-critical';
  return {
    host: { name: 'demo-host', os: 'macos', arch: 'aarch64' },
    sampled_at: new Date(DEMO_LIVE_NOW_MS).toISOString(),
    level: scenario === 'normal' ? 'normal' : scenario === 'disk-critical' ? 'critical' : 'warning',
    signals: {
      disk: {
        ...(disk
          ? { status: scenario === 'disk-critical' ? 'critical' : 'warning', reason: 'projected_full', since: null }
          : NORMAL),
        free_bytes: (disk ? (scenario === 'disk-critical' ? 4 : 18) : 240) * 1024 ** 3,
        total_bytes: 1000 * 1024 ** 3,
        consumption_bytes_per_hour: disk ? 2 * 1024 ** 3 : null,
        hours_to_full: disk ? (scenario === 'disk-critical' ? 2 : 9) : null,
      },
      memory: { ...NORMAL, pressure: 'normal', swap_used_bytes: 0, swap_growth_bytes_per_hour: null },
      cpu: { ...NORMAL, busy_percent: 22, busy_for_seconds: null, top_process: null, top_process_percent: null },
      power: {
        ...(scenario === 'battery' ? { status: 'warning', reason: 'on_battery', since: null } : NORMAL),
        on_battery: scenario === 'battery',
        battery_percent: scenario === 'battery' ? 49 : 100,
        lid_closed: false,
        sleep_prevented: true,
        idle_sleep_minutes: null,
      },
      network: { ...NORMAL, reachable: true, interface: 'en0' },
      thermal: { ...NORMAL, throttled: false, cpu_speed_limit: 100 },
    },
  };
}
