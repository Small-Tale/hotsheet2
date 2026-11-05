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
    runway:
      attention || scenario === 'quiet' ? { ready: 4, estimate_ms: 220 * MINUTE } : { ready: 0, estimate_ms: null },
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
        battery_percent: scenario === 'battery' ? 64 : 100,
        lid_closed: false,
        sleep_prevented: true,
        idle_sleep_minutes: null,
      },
      network: { ...NORMAL, reachable: true, interface: 'en0' },
      thermal: { ...NORMAL, throttled: false, cpu_speed_limit: 100 },
    },
  };
}
