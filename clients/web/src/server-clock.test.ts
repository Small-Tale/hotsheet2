import { describe, expect, it, vi } from 'vitest';

import { conversationTurnProgress } from './ai-conversation';
import { Api } from './api';
import { type ClockSource, createServerClocks, responseDateMs, SERVER_CLOCK_DEAD_ZONE_MS } from './server-clock';

/** A browser whose wall clock is `skew` ms off the server's, with an independent monotonic clock. */
function device(serverStart = 1_800_000_000_000, skew = 0) {
  let monotonic = 50_000,
    wallCorrection = 0;
  const server = () => serverStart + monotonic;
  const source: ClockSource = {
    wallNow: () => server() - skew + wallCorrection,
    monotonicNow: () => monotonic,
  };
  return {
    source,
    server,
    advance(ms: number) {
      monotonic += ms;
    },
    /** The device's wall clock is corrected (NTP) without the monotonic clock moving. */
    correctWall(ms: number) {
      wallCorrection += ms;
    },
  };
}

describe('server clock estimate (HS2-GA5ASE)', () => {
  it('uses the browser clock until a stamped event arrives, and ignores unusable stamps', () => {
    const d = device(undefined, 180_000);
    const clocks = createServerClocks(d.source);
    expect(clocks.now('p')).toBe(d.source.wallNow());
    clocks.observe('p', undefined);
    clocks.observe('p', Number.NaN);
    clocks.observe('p', '1800000000000');
    expect(clocks.offset('p')).toBe(0);
  });

  it('follows a client clock three minutes behind the server, less the fastest delivery', () => {
    const d = device(undefined, 180_000);
    const clocks = createServerClocks(d.source);
    // Stamped 400 ms and then 30 ms before arrival: the faster delivery is the better sample.
    const slow = d.server();
    d.advance(400);
    clocks.observe('p', slow);
    const fast = d.server();
    d.advance(30);
    clocks.observe('p', fast);
    expect(clocks.offset('p')).toBe(180_000 - 30);
    d.advance(60_000);
    expect(Math.abs(clocks.now('p') - d.server())).toBeLessThanOrEqual(30);
  });

  it('follows a client clock ahead of the server too', () => {
    const d = device(undefined, -120_000);
    const clocks = createServerClocks(d.source);
    clocks.observe('p', d.server());
    expect(clocks.offset('p')).toBe(-120_000);
  });

  it('keeps an in-sync device on its own clock: delivery latency is not skew', () => {
    const d = device(undefined, 0);
    const clocks = createServerClocks(d.source);
    const stamped = d.server();
    d.advance(SERVER_CLOCK_DEAD_ZONE_MS - 1);
    clocks.observe('p', stamped);
    expect(clocks.offset('p')).toBe(0);
    expect(clocks.now('p')).toBe(d.source.wallNow());
  });

  it('never lets a replayed or delayed event drag the estimate', () => {
    const d = device(undefined, 180_000);
    const clocks = createServerClocks(d.source);
    clocks.observe('p', d.server());
    // A reconnect replays events stamped ten minutes ago.
    d.advance(600_000);
    clocks.observe('p', d.server() - 600_000);
    clocks.observe('p', d.server() - 590_000);
    expect(clocks.offset('p')).toBe(180_000);
  });

  it('errs toward later "may be stuck" when only replayed events have been seen', () => {
    const d = device(undefined, 180_000);
    const clocks = createServerClocks(d.source);
    clocks.observe('p', d.server() - 600_000);
    // The estimate reads the server as earlier than it is, never later.
    expect(clocks.now('p')).toBeLessThan(d.server());
    // A live event then corrects it.
    clocks.observe('p', d.server());
    expect(clocks.now('p')).toBe(d.server());
  });

  it('survives a wall-clock correction on the device', () => {
    const d = device(undefined, 180_000);
    const clocks = createServerClocks(d.source);
    clocks.observe('p', d.server());
    d.correctWall(180_000);
    d.advance(5_000);
    expect(clocks.now('p')).toBe(d.server());
    expect(clocks.offset('p')).toBe(0);
  });

  it('keeps each project on its own server', () => {
    const d = device(undefined, 0);
    const clocks = createServerClocks(d.source);
    clocks.observe('remote', d.server() + 90_000);
    expect(clocks.offset('remote')).toBe(90_000);
    expect(clocks.offset('local')).toBe(0);
  });

  it('times a skewed remote turn by the server clock: no early "may be stuck"', () => {
    const d = device(undefined, 180_000);
    const clocks = createServerClocks(d.source);
    const startedAt = d.server();
    clocks.observe('p', startedAt);
    d.advance(30_000);
    const connection = {
      busy: true,
      turn_id: 't1',
      turn_started_at_ms: startedAt,
      last_event_at_ms: startedAt,
    };
    // The device's own clock is three minutes behind: it reads the turn as not yet started.
    expect(conversationTurnProgress({ messages: [] }, connection, d.source.wallNow())?.label).toContain('Working · 0s');
    const progress = conversationTurnProgress({ messages: [] }, connection, clocks.now('p'));
    expect(progress?.state).toBe('working');
    expect(progress?.label).toBe('Working · 30s · last activity 30s ago');
    // A device three minutes ahead would have called it stuck; the server clock does not.
    const ahead = device(startedAt - 50_000, -180_000);
    ahead.advance(30_000);
    expect(conversationTurnProgress({ messages: [] }, connection, ahead.source.wallNow())?.state).toBe('stuck');
    const aheadClocks = createServerClocks(ahead.source);
    aheadClocks.observe('p', ahead.server());
    expect(conversationTurnProgress({ messages: [] }, connection, aheadClocks.now('p'))?.state).toBe('working');
  });
});

describe('server clock sample from a response Date header (HS2-FMZSNY)', () => {
  const dated = (date?: string) => new Response('[]', { headers: date === undefined ? {} : { date } });

  it('reads an HTTP date and yields no sample when the header is missing or unparseable', () => {
    expect(responseDateMs(dated('Thu, 15 Jan 2027 08:00:00 GMT'))).toBe(Date.UTC(2027, 0, 15, 8));
    expect(responseDateMs(dated())).toBeUndefined();
    expect(responseDateMs(dated(''))).toBeUndefined();
    expect(responseDateMs(dated('not a date'))).toBeUndefined();
  });

  it('times a silent turn on a skewed device from the snapshot alone, before any turn event', () => {
    // This device runs three minutes ahead of the server, whose Date header drops the milliseconds.
    const d = device(1_800_000_000_400, -180_000);
    const clocks = createServerClocks(d.source);
    const startedAt = d.server() - 90_000;
    clocks.observe('p', responseDateMs(dated(new Date(d.server()).toUTCString())));
    // Short by the sub-second truncation only: far inside the dead zone, in the safe direction.
    expect(clocks.offset('p')).toBeLessThanOrEqual(-180_000);
    expect(clocks.offset('p')).toBeGreaterThan(-181_000);
    const connection = {
      busy: true,
      turn_started_at_ms: startedAt,
      last_event_at_ms: startedAt,
      in_flight_tool: { name: 'Bash', started_at_ms: startedAt, count: 1 },
    };
    const progress = conversationTurnProgress({ messages: [] }, connection, clocks.now('p'));
    expect(progress?.state).toBe('tool');
    expect(progress?.label).toMatch(/^Running Bash · 1m (29|30)s/u);
    // The device clock alone would have overstated it by the three-minute skew.
    expect(conversationTurnProgress({ messages: [] }, connection, d.source.wallNow())?.label).toMatch(
      /^Running Bash · 4m 30s/u,
    );
    // A later, more precise turn stamp still wins, and an older header never pulls it back.
    clocks.observe('p', d.server());
    expect(clocks.offset('p')).toBe(-180_000);
    clocks.observe('p', responseDateMs(dated(new Date(d.server() - 5_000).toUTCString())));
    expect(clocks.offset('p')).toBe(-180_000);
  });

  it('passes the connections response to the caller before its body is parsed', async () => {
    const date = 'Thu, 15 Jan 2027 08:00:00 GMT';
    const fetchMock = vi.fn(async () => new Response('[]', { headers: { date, 'content-type': 'application/json' } }));
    vi.stubGlobal('fetch', fetchMock);
    try {
      const seen: (number | undefined)[] = [];
      await expect(
        new Api('/api', 'secret').activeToolConnections((response) => seen.push(responseDateMs(response))),
      ).resolves.toEqual([]);
      expect(seen).toEqual([Date.UTC(2027, 0, 15, 8)]);
      // Callers that do not sample keep working.
      await expect(new Api('/api', 'secret').activeToolConnections()).resolves.toEqual([]);
    } finally {
      vi.unstubAllGlobals();
    }
  });
});
