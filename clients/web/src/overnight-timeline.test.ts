import { describe, expect, it } from 'vitest';

import type { LiveHistorySegment, LiveStoreState } from './api';
import {
  extendSegments,
  formatSegmentDuration,
  hourTicks,
  liveHistoryState,
  overnightFrom,
  timelineLane,
} from './overnight-timeline';
import { parseLiveHistoryList } from './server-response-parsers';

const HOUR = 3_600_000;

function live(state: LiveStoreState['state'], phases: (string | null)[] = []): LiveStoreState {
  return {
    store: 'stw',
    root: '/work/stw.hs2',
    name: 'Small Tale Website',
    checkouts: ['p1'],
    epoch_ms: 1,
    revision: 1,
    state,
    activity: {
      word: state,
      active_since_ms: null,
      stopped_at_ms: null,
      last_progress_ms: null,
      idle_deadline_ms: null,
      stalled: [],
      halted: [],
      causes: [],
    },
    claims: phases.map((phase, index) => ({
      ticket: `t${String(index)}`,
      slug: `STW-${String(index)}`,
      title: 'Synthetic',
      worker: 'w',
      phase,
      claimed_at_ms: null,
      lease_expires_at_ms: 1,
      eta_at_ms: null,
    })),
    workers: { live: phases.length, expected: 1 },
    waiting: { count: 0, tickets: [] },
    runway: { ready: 0, estimate_ms: null },
    rates: {
      tokens: { window_minutes: 5, minutes: [], peak_per_minute: 1 },
      tickets_updated: { window_minutes: 60, at_ms: [], peak_per_hour: 1 },
    },
  };
}

const seg = (state: LiveHistorySegment['state'], start_ms: number, end_ms: number): LiveHistorySegment => ({
  state,
  start_ms,
  end_ms,
});

describe('overnight timeline (HS2-9AN4DN)', () => {
  it('derives Waiting on CI only while every claim is final-testing, as the server records it', () => {
    expect(liveHistoryState(live('active', ['final_testing', 'final_testing']))).toBe('waiting_on_ci');
    expect(liveHistoryState(live('active', ['final_testing', 'working']))).toBe('active');
    expect(liveHistoryState(live('active'))).toBe('active');
    expect(liveHistoryState(live('stalled', ['final_testing']))).toBe('stalled');
  });

  it('extends the read segments live: the same state grows, a new state starts at the read', () => {
    const read = [seg('active', 0, 100)];
    // Nothing pushed yet: the open segment runs to now.
    expect(extendSegments(read, undefined, 100, 160)).toEqual([seg('active', 0, 160)]);
    // An older read never shrinks to an earlier now.
    expect(extendSegments(read, live('active'), 100, 90)).toEqual([seg('active', 0, 100)]);
    expect(extendSegments(read, live('active'), 100, 150)).toEqual([seg('active', 0, 150)]);
    expect(extendSegments(read, live('stalled'), 100, 150)).toEqual([seg('active', 0, 100), seg('stalled', 100, 150)]);
    // Empty history: the current state starts when it was read.
    expect(extendSegments([], live('stopped'), 100, 130)).toEqual([seg('stopped', 100, 130)]);
    expect(extendSegments([], undefined, 100, 130)).toEqual([]);
    // The tail is the server's present: a slightly earlier end (clock skew) still merges.
    expect(extendSegments([seg('active', 0, 98)], live('active'), 100, 130)).toEqual([seg('active', 0, 130)]);
    expect(extendSegments([seg('active', 0, 98)], live('stalled'), 100, 130)).toEqual([
      seg('active', 0, 98),
      seg('stalled', 98, 130),
    ]);
    // The input is not mutated.
    expect(read).toEqual([seg('active', 0, 100)]);
  });

  it('places segments and ticks as fractions of the window, clipped and filtered', () => {
    const lane = timelineLane(
      {
        store: 'stw',
        name: 'Small Tale Website',
        completions: [
          { slug: 'STW-1', at_ms: 50 },
          { slug: 'STW-0', at_ms: -10 },
        ],
      },
      [seg('active', -100, 25), seg('stalled', 25, 100), seg('stopped', 200, 300)],
      0,
      100,
    );
    expect(lane.segments).toEqual([
      { ...seg('active', -100, 25), left: 0, width: 0.25 },
      { ...seg('stalled', 25, 100), left: 0.25, width: 0.75 },
    ]);
    expect(lane.ticks).toEqual([{ slug: 'STW-1', at_ms: 50, left: 0.5 }]);
  });

  it('labels whole hours inside the window and thins them', () => {
    const start = new Date(2026, 10, 5, 21, 0).getTime();
    expect(hourTicks(start, start + 6 * HOUR)).toHaveLength(7);
    expect(hourTicks(start + 1, start + 2 * HOUR)).toEqual([start + HOUR, start + 2 * HOUR]);
    expect(hourTicks(start, start + 24 * HOUR).length).toBeLessThanOrEqual(13);
    expect(hourTicks(start, start)).toEqual([]);
  });

  it('chooses the window start and formats durations', () => {
    const now = 100 * HOUR;
    expect(overnightFrom('12h', now, undefined)).toBe(now - 12 * HOUR);
    expect(overnightFrom('24h', now, 5)).toBe(now - 24 * HOUR);
    expect(overnightFrom('standby', now, now - 3 * HOUR)).toBe(now - 3 * HOUR);
    // Standby never entered: the last 12 hours.
    expect(overnightFrom('standby', now, undefined)).toBe(now - 12 * HOUR);
    expect(formatSegmentDuration(30_000)).toBe('under a minute');
    expect(formatSegmentDuration(45 * 60_000)).toBe('45m');
    expect(formatSegmentDuration(125 * 60_000)).toBe('2h 05m');
  });

  it('parses the server shape and rejects unknown states', () => {
    const body = {
      since_ms: 0,
      until_ms: 10,
      stores: [
        {
          store: 's',
          name: 'S',
          checkouts: ['p'],
          segments: [{ state: 'waiting_on_ci', start_ms: 0, end_ms: 10 }],
          completions: [{ slug: 'S-1', at_ms: 5 }],
        },
      ],
    };
    expect(parseLiveHistoryList(body)).toEqual(body);
    expect(() =>
      parseLiveHistoryList({
        ...body,
        stores: [{ ...body.stores[0], segments: [{ state: 'asleep', start_ms: 0, end_ms: 1 }] }],
      }),
    ).toThrow();
  });
});
