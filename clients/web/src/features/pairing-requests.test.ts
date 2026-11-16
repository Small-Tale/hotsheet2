import { describe, expect, it, vi } from 'vitest';

import type { ChangeEvent } from '../api';
import type { RemotePendingDevice } from '../remote-devices-model';
import { createPairingRequests, PAIRING_REQUESTS_EVENT, pairingRequestNotice } from './pairing-requests';
import { REMOTE_DEVICES_PATH } from './remote-devices';

const NOW = 1_000_000;

const event = (cursor: number | undefined, kind = PAIRING_REQUESTS_EVENT): ChangeEvent => ({
  ...(cursor === undefined ? {} : { cursor }),
  store: '',
  kind,
  id: '',
  slug: '',
});

const pending = (id: string, overrides: Partial<RemotePendingDevice> = {}): RemotePendingDevice => ({
  id,
  label: `Synthetic ${id}`,
  origin: 'http://localhost:4175',
  created_at: NOW - 10,
  expires_at: NOW + 300,
  approved: false,
  ...overrides,
});

const deviceList = (items: RemotePendingDevice[]) =>
  Response.json({
    server_id: 'synthetic-server',
    revision: 1,
    pair_ttl_seconds: 300,
    manager: { local: true },
    devices: [],
    pending: items,
  });

/** A host whose device-list answers the test releases, so reads can overlap events. */
function scriptedHost() {
  const unanswered: ((response: Response) => void)[] = [];
  const waiters: (() => void)[] = [];
  const request = vi.fn<typeof fetch>(
    () =>
      new Promise<Response>((resolve) => {
        unanswered.push(resolve);
        waiters.splice(0).forEach((wake) => {
          wake();
        });
      }),
  );
  const answer = async (response: Response) => {
    while (unanswered.length === 0) await new Promise<void>((resolve) => waiters.push(resolve));
    unanswered.shift()?.(response);
  };
  return { request, answer, unanswered };
}

/** A manual clock and expiry scheduler: timers fire only when the test advances time. */
function manualClock() {
  let now = NOW;
  const timers: { at: number; callback: () => void; cancelled: boolean }[] = [];
  return {
    now: () => now,
    schedule: vi.fn((callback: () => void, ms: number) => {
      const timer = { at: now + ms / 1000, callback, cancelled: false };
      timers.push(timer);
      return () => {
        timer.cancelled = true;
      };
    }),
    advance(seconds: number) {
      now += seconds;
      for (const timer of timers.splice(0)) {
        if (timer.cancelled) continue;
        if (timer.at <= now) timer.callback();
        else timers.push(timer);
      }
    },
    live: () => timers.filter((timer) => !timer.cancelled).length,
  };
}

function setup(options: { canManage?: () => boolean } = {}) {
  const host = scriptedHost(),
    clock = manualClock(),
    arrivals: string[] = [],
    onForbidden = vi.fn();
  const requests = createPairingRequests({
    request: host.request,
    canManage: options.canManage ?? (() => true),
    onArrival: (item) => arrivals.push(item.id),
    onForbidden,
    now: clock.now,
    schedule: clock.schedule,
  });
  const ids = () => requests.waiting.value.map((item) => item.id);
  return { host, clock, arrivals, onForbidden, requests, ids };
}

describe('HS2-BZHMEV pairing requests reach device managers over the change channel', () => {
  it('the first resync reads the waiting requests silently; a later arrival is announced once', async () => {
    const { host, arrivals, requests, ids } = setup();
    const first = requests.resync();
    expect(host.request).toHaveBeenCalledWith(REMOTE_DEVICES_PATH, { credentials: 'same-origin' });
    await host.answer(deviceList([pending('a')]));
    await first;
    expect(ids()).toEqual(['a']);
    expect(arrivals).toEqual([]);

    const arrived = requests.applyEvents([event(7)]);
    await host.answer(deviceList([pending('a'), pending('b')]));
    await arrived;
    expect(ids()).toEqual(['a', 'b']);
    expect(arrivals).toEqual(['b']);

    // A reconnect that re-reads the same requests announces nothing new.
    const again = requests.resync();
    await host.answer(deviceList([pending('a'), pending('b')]));
    await again;
    expect(arrivals).toEqual(['b']);
    expect(host.request).toHaveBeenCalledTimes(3);
  });

  it('counts only requests still waiting: approved ones and expired ones drop out', async () => {
    const { host, requests, ids } = setup();
    const read = requests.resync();
    await host.answer(
      deviceList([pending('a'), pending('b', { approved: true }), pending('c', { expires_at: NOW - 1 })]),
    );
    await read;
    expect(ids()).toEqual(['a']);
  });

  it('a local display timer drops each request at its expiry without a request', async () => {
    const { host, clock, requests, ids } = setup();
    const read = requests.resync();
    await host.answer(deviceList([pending('a', { expires_at: NOW + 60 }), pending('b', { expires_at: NOW + 120 })]));
    await read;
    expect(ids()).toEqual(['a', 'b']);
    expect(clock.schedule).toHaveBeenLastCalledWith(expect.any(Function), 60_000);
    clock.advance(60);
    expect(ids()).toEqual(['b']);
    clock.advance(60);
    expect(ids()).toEqual([]);
    expect(clock.live()).toBe(0);
    expect(host.request).toHaveBeenCalledTimes(1);
  });

  it('ignores other kinds and repeated or older cursors; the same event through two streams reads once', async () => {
    const { host, requests } = setup();
    expect(requests.applyEvents([event(3, 'browser_grants'), event(4, 'ticket_changed')])).toBeUndefined();
    const read = requests.applyEvents([event(5)]);
    await host.answer(deviceList([]));
    await read;
    expect(requests.applyEvents([event(5)])).toBeUndefined();
    expect(requests.applyEvents([event(4)])).toBeUndefined();
    const next = requests.applyEvents([event(6), event(6)]);
    await host.answer(deviceList([]));
    await next;
    // An uncursored event always reads.
    const loose = requests.applyEvents([event(undefined)]);
    await host.answer(deviceList([]));
    await loose;
    expect(host.request).toHaveBeenCalledTimes(3);
  });

  it('a resync forgets the cursor, because a restarted server restarts its cursors', async () => {
    const { host, requests } = setup();
    const read = requests.applyEvents([event(40)]);
    await host.answer(deviceList([]));
    await read;
    const resync = requests.resync();
    await host.answer(deviceList([]));
    await resync;
    const restarted = requests.applyEvents([event(2)]);
    expect(restarted).toBeDefined();
    await host.answer(deviceList([]));
    await restarted;
    expect(host.request).toHaveBeenCalledTimes(3);
  });

  it('coalesces: events during a read schedule exactly one more read', async () => {
    const { host, requests, ids } = setup();
    const first = requests.applyEvents([event(1)]);
    void requests.applyEvents([event(2)]);
    void requests.applyEvents([event(3)]);
    expect(host.request).toHaveBeenCalledTimes(1);
    await host.answer(deviceList([pending('a')]));
    await host.answer(deviceList([pending('a'), pending('b')]));
    await first;
    expect(host.request).toHaveBeenCalledTimes(2);
    expect(ids()).toEqual(['a', 'b']);
  });

  it('a page that may not manage devices reads nothing and shows nothing', async () => {
    let manages = false;
    const { host, requests, ids } = setup({ canManage: () => manages });
    await requests.resync();
    await requests.applyEvents([event(9)]);
    requests.observe([pending('a')]);
    expect(host.request).not.toHaveBeenCalled();
    expect(ids()).toEqual([]);

    // Gaining the grant reads; losing it while a read is in flight drops the answer.
    manages = true;
    const gained = requests.refresh();
    await host.answer(deviceList([pending('a')]));
    await gained;
    expect(ids()).toEqual(['a']);
    const losing = requests.applyEvents([event(10)]);
    manages = false;
    await host.answer(deviceList([pending('a'), pending('b')]));
    await losing;
    expect(ids()).toEqual([]);
  });

  it('a refused read clears the requests and re-checks this page grants', async () => {
    const { host, onForbidden, requests, ids } = setup();
    const read = requests.resync();
    await host.answer(deviceList([pending('a')]));
    await read;
    for (const status of [401, 403]) {
      const refused = requests.applyEvents([event(status)]);
      await host.answer(Response.json({ error: 'administrator access required' }, { status }));
      await refused;
      expect(ids()).toEqual([]);
    }
    expect(onForbidden).toHaveBeenCalledTimes(2);
  });

  it('a host without remote authentication stops resync reads until a pairing event arrives', async () => {
    const { host, requests } = setup();
    const read = requests.resync();
    await host.answer(Response.json({ error: 'remote authentication is not configured' }, { status: 503 }));
    await read;
    await requests.resync();
    await requests.resync();
    expect(host.request).toHaveBeenCalledTimes(1);
    const woke = requests.applyEvents([event(1)]);
    await host.answer(deviceList([]));
    await woke;
    const resync = requests.resync();
    await host.answer(deviceList([]));
    await resync;
    expect(host.request).toHaveBeenCalledTimes(3);
  });

  it('a failed or invalid read keeps the last requests for the next resync', async () => {
    const { host, requests, ids } = setup();
    const read = requests.resync();
    await host.answer(deviceList([pending('a')]));
    await read;
    const failed = requests.applyEvents([event(1)]);
    await host.answer(Response.json({ error: 'remote auth storage failed' }, { status: 500 }));
    await failed;
    const invalid = requests.applyEvents([event(2)]);
    await host.answer(Response.json({ pending: 'not a list' }));
    await invalid;
    expect(ids()).toEqual(['a']);
  });

  it('the open Devices list feeds the requests without another read and announces nothing twice', async () => {
    const { host, arrivals, requests, ids } = setup();
    const read = requests.resync();
    await host.answer(deviceList([pending('a')]));
    await read;
    requests.observe([pending('a'), pending('b')]);
    expect(ids()).toEqual(['a', 'b']);
    expect(arrivals).toEqual(['b']);
    const event2 = requests.applyEvents([event(3)]);
    await host.answer(deviceList([pending('a'), pending('b')]));
    await event2;
    expect(arrivals).toEqual(['b']);
    requests.observe([pending('b', { approved: true })]);
    expect(ids()).toEqual([]);
    expect(host.request).toHaveBeenCalledTimes(2);
  });

  it('clear forgets what was announced, so regaining the grant reads silently', async () => {
    const { host, arrivals, requests, ids } = setup();
    const read = requests.resync();
    await host.answer(deviceList([pending('a')]));
    await read;
    requests.clear();
    expect(ids()).toEqual([]);
    const again = requests.refresh();
    await host.answer(deviceList([pending('a'), pending('b')]));
    await again;
    expect(ids()).toEqual(['a', 'b']);
    expect(arrivals).toEqual([]);
  });

  it('names the requesting device and the route to approve it', () => {
    expect(pairingRequestNotice({ label: 'Synthetic tablet' })).toBe(
      '“Synthetic tablet” asks to pair with this host. Open Settings → Devices to approve or reject it.',
    );
  });
});
