import type { LiveStoreState } from '../api';

/**
 * Deterministic live-state snapshots in the server's exact shape (`GET /live-state`, HS2-JTTQMY) for
 * the glance-surface demos and tests. Synthetic project names only.
 */

export const GLANCE_FIXTURE_NOW = Date.parse('2026-11-06T10:47:00Z');
const MINUTE = 60_000;
const HOUR = 60 * MINUTE;

export function liveStateFixture(overrides: Partial<LiveStoreState> = {}, now = GLANCE_FIXTURE_NOW): LiveStoreState {
  return {
    store: 'store-demo',
    root: '/work/demo.hs2',
    name: 'Demo',
    checkouts: ['demo-ab12'],
    epoch_ms: 1,
    revision: 1,
    state: 'active',
    activity: {
      word: 'active',
      active_since_ms: now - 40 * MINUTE,
      stopped_at_ms: null,
      last_progress_ms: now - 2 * MINUTE,
      idle_deadline_ms: now + 28 * MINUTE,
      stalled: [],
      halted: [],
      causes: [],
    },
    claims: [],
    workers: { live: 1, expected: 1 },
    waiting: { count: 0, tickets: [] },
    rates: {
      tokens: { window_minutes: 5, minutes: [], peak_per_minute: 0 },
      tickets_updated: { window_minutes: 60, at_ms: [], peak_per_hour: 0 },
    },
    ...overrides,
  };
}

/** The four projects of mock `m3-stats-right-now`: stuck, waiting, active, and stopped. */
export function glanceScenario(now = GLANCE_FIXTURE_NOW): LiveStoreState[] {
  const minutes = (perMinute: readonly number[]) =>
    perMinute.map((tokens, index) => ({ start_ms: now - (perMinute.length - 1 - index) * MINUTE, tokens }));
  // Twelve hourly buckets ending with the current hour (HS2-TRZGT3), given in thousands of tokens.
  const hours = (thousands: readonly number[]) =>
    thousands.map((tokens, index) => ({
      start_ms: Math.floor(now / HOUR) * HOUR - (thousands.length - 1 - index) * HOUR,
      tokens: tokens * 1_000,
    }));
  const base = (store: string, name: string, checkout: string) => ({
    store,
    name,
    checkouts: [checkout],
    root: `/work/${store}.hs2`,
  });
  return [
    liveStateFixture(
      {
        ...base('store-website', 'Small Tale Website', 'website-c3d4'),
        state: 'may_be_stuck',
        activity: {
          word: 'may_be_stuck',
          active_since_ms: now - 3 * 60 * MINUTE,
          stopped_at_ms: null,
          last_progress_ms: now - 34 * MINUTE,
          idle_deadline_ms: now - 4 * MINUTE,
          stalled: [],
          halted: [],
          causes: [],
        },
        claims: [
          {
            ticket: '01M6N3H10PF7JHA3VHWS72N5Z1',
            slug: 'STW-H8K2',
            title: 'Rebuild the pricing page',
            worker: 'worker-1',
            phase: 'working',
            claimed_at_ms: now - 90 * MINUTE,
            lease_expires_at_ms: now + 20 * MINUTE,
            eta_at_ms: now - 52 * MINUTE,
          },
        ],
        rates: {
          tokens: {
            window_minutes: 5,
            minutes: minutes([120, 0, 0, 40, 0]),
            peak_per_minute: 4_100,
            hours: hours([9, 21, 40, 32, 30, 18, 12, 6, 4, 3, 2, 1]),
          },
          tickets_updated: { window_minutes: 60, at_ms: [now - 50 * MINUTE], peak_per_hour: 6 },
        },
      },
      now,
    ),
    liveStateFixture(
      {
        ...base('store-review', 'UX Review', 'review-e5f6'),
        state: 'waiting_on_you',
        activity: {
          word: 'stopped',
          active_since_ms: null,
          stopped_at_ms: now - 60 * MINUTE,
          last_progress_ms: now - 65 * MINUTE,
          idle_deadline_ms: null,
          stalled: [],
          halted: [],
          causes: [],
        },
        workers: { live: 0, expected: 1 },
        waiting: {
          count: 2,
          tickets: [
            {
              ticket: '01M6N3H6FQS70A7RX41E6M0BF4',
              slug: 'UXR-2B7Q',
              title: 'Pick a header layout',
              reason: 'feedback',
            },
            {
              ticket: '01M6N3M9GVHQ03BKAJZFHBECST',
              slug: 'UXR-9C1D',
              title: 'Review the empty states',
              reason: 'review',
            },
          ],
        },
        rates: {
          tokens: {
            window_minutes: 5,
            minutes: [],
            peak_per_minute: 2_400,
            hours: hours([0, 0, 12, 30, 26, 8, 0, 0, 0, 0, 0, 0]),
          },
          tickets_updated: { window_minutes: 60, at_ms: [], peak_per_hour: 4 },
        },
      },
      now,
    ),
    liveStateFixture(
      {
        ...base('store-hotsheet', 'Hot Sheet 2', 'hotsheet2-a1b2'),
        claims: [
          {
            ticket: '01M6N3MNDPJEXTPVT48F0M3TGA',
            slug: 'HS2-R76MMW',
            title: 'Coalesce reader refreshes',
            worker: 'worker-2',
            phase: 'initial_testing',
            claimed_at_ms: now - 50 * MINUTE,
            lease_expires_at_ms: now + 25 * MINUTE,
            eta_at_ms: now - 18 * MINUTE,
          },
          {
            ticket: '01M6N3NRMDQZ3SBQT4Y1X2G5RQ',
            slug: 'HS2-TG2ZV3',
            title: 'Pin the stats freshness line',
            worker: 'worker-3',
            phase: 'final_testing',
            claimed_at_ms: now - 30 * MINUTE,
            lease_expires_at_ms: now + 25 * MINUTE,
            eta_at_ms: now + 10 * MINUTE,
          },
        ],
        workers: { live: 2, expected: 2 },
        rates: {
          tokens: {
            window_minutes: 5,
            minutes: minutes([1_400, 1_900, 1_500, 1_700, 1_500]),
            peak_per_minute: 6_200,
            hours: hours([20, 34, 42, 51, 60, 72, 66, 70, 81, 88, 90, 95]),
          },
          tickets_updated: {
            window_minutes: 60,
            at_ms: [55, 48, 41, 33, 29, 22, 18, 12, 9, 6, 4, 2, 1].map((ago) => now - ago * MINUTE),
            peak_per_hour: 22,
          },
        },
      },
      now,
    ),
    liveStateFixture(
      {
        ...base('store-kerf', 'Kerf', 'kerf-g7h8'),
        state: 'stopped',
        activity: {
          word: 'stopped',
          active_since_ms: null,
          stopped_at_ms: now - 120 * MINUTE,
          last_progress_ms: now - 121 * MINUTE,
          idle_deadline_ms: null,
          stalled: [],
          halted: [],
          causes: [],
        },
        workers: { live: 0, expected: 0 },
        rates: {
          tokens: {
            window_minutes: 5,
            minutes: [],
            peak_per_minute: 3_000,
            hours: hours([14, 18, 11, 0, 0, 0, 0, 0, 0, 0, 0, 0]),
          },
          tickets_updated: { window_minutes: 60, at_ms: [], peak_per_hour: 5 },
        },
      },
      now,
    ),
  ];
}
