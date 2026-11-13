import { describe, expect, it, vi } from 'vitest';

import { createRemoteDevices, REMOTE_DEVICES_BACKOFF, REMOTE_DEVICES_PATH } from './remote-devices';

const list = (revision: number, extra: Record<string, unknown> = {}) => ({
  server_id: 'synthetic-server',
  revision,
  pair_ttl_seconds: 900,
  manager: { local: true },
  devices: [],
  pending: [],
  ...extra,
});

/** A scripted host: each GET waits for the test to answer it, as a real long poll waits for a change. */
function scriptedHost() {
  const calls: { path: string; method: string; body?: unknown; answer: (response: Response) => void }[] = [];
  const waiters: (() => void)[] = [];
  const request = vi.fn<typeof fetch>(
    (input, init) =>
      new Promise<Response>((resolve, reject) => {
        const path = typeof input === 'string' ? input : input instanceof URL ? input.href : input.url;
        const method = init?.method ?? 'GET';
        init?.signal?.addEventListener(
          'abort',
          () => {
            reject(new DOMException('aborted', 'AbortError'));
          },
          {
            once: true,
          },
        );
        calls.push({
          path,
          method,
          body: typeof init?.body === 'string' ? JSON.parse(init.body) : undefined,
          answer: resolve,
        });
        waiters.splice(0).forEach((wake) => {
          wake();
        });
      }),
  );
  let consumed = 0;
  /** The next request the controller sent, in order, waiting for it when it has not been sent yet. */
  const next = async () => {
    while (calls.length <= consumed) await new Promise<void>((resolve) => waiters.push(resolve));
    return calls[consumed++];
  };
  return { request, calls, next };
}

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

describe('HS2-DXM0YN remote devices controller', () => {
  it('loads, then follows the list with a cursor long poll and no fixed-interval request', async () => {
    const host = scriptedHost();
    const devices = createRemoteDevices({ request: host.request, canManage: () => true });
    devices.start();
    expect(devices.state.value).toBe('loading');
    const first = await host.next();
    expect(first.path).toBe(REMOTE_DEVICES_PATH);
    first.answer(Response.json(list(41)));
    const poll = await host.next();
    expect(poll.path).toBe(`${REMOTE_DEVICES_PATH}/changes?after=41`);
    expect(devices.state.value).toBe('ready');
    expect(devices.list.value?.revision).toBe(41);
    // While the long poll is unanswered the controller makes no other request.
    await settle();
    expect(host.calls).toHaveLength(2);
    poll.answer(
      Response.json(
        list(42, {
          pending: [
            { id: 'P1', label: 'Safari on iPhone', origin: 'https://h', created_at: 1, expires_at: 2, approved: false },
          ],
        }),
      ),
    );
    const again = await host.next();
    expect(again.path).toBe(`${REMOTE_DEVICES_PATH}/changes?after=42`);
    expect(devices.list.value?.pending[0]?.label).toBe('Safari on iPhone');
    // A repeated start keeps the one running poll.
    devices.start();
    await settle();
    expect(host.calls).toHaveLength(3);
    devices.stop();
    await settle();
    expect(host.calls).toHaveLength(3);
    expect(devices.state.value).toBe('ready');
  });

  it('reopens from the last cursor after stop and start', async () => {
    const host = scriptedHost();
    const devices = createRemoteDevices({ request: host.request, canManage: () => true });
    devices.start();
    (await host.next()).answer(Response.json(list(7)));
    await host.next();
    devices.stop();
    devices.start();
    expect(devices.state.value).toBe('ready');
    expect((await host.next()).path).toBe(`${REMOTE_DEVICES_PATH}/changes?after=7`);
    devices.stop();
  });

  it('backs off after failures, bounded at the maximum, and resets after an answer', async () => {
    const host = scriptedHost();
    const waits: number[] = [];
    const devices = createRemoteDevices({
      request: host.request,
      canManage: () => true,
      wait: async (ms) => {
        waits.push(ms);
      },
    });
    devices.start();
    for (let attempt = 0; attempt < 7; attempt += 1)
      (await host.next()).answer(Response.json({ error: 'server unavailable' }, { status: 503 }));
    expect(devices.message.value).toBe('Could not load the device list. server unavailable');
    (await host.next()).answer(Response.json(list(3)));
    (await host.next()).answer(new Response('not json', { status: 502 }));
    await host.next();
    expect(waits).toEqual([1_000, 2_000, 4_000, 8_000, 16_000, 30_000, 30_000, REMOTE_DEVICES_BACKOFF.initialMs]);
    devices.stop();
  });

  it('clears a load failure once the list arrives', async () => {
    const host = scriptedHost();
    const devices = createRemoteDevices({ request: host.request, canManage: () => true, wait: async () => {} });
    devices.start();
    (await host.next()).answer(Response.json({ unexpected: true }));
    const retry = await host.next();
    expect(devices.message.value).toMatch(/^Could not load the device list\. The host returned an invalid/);
    retry.answer(Response.json(list(1)));
    await host.next();
    expect(devices.message.value).toBe('');
    devices.stop();
  });

  it.each([401, 403])('stops and explains on %i instead of retrying', async (status) => {
    const host = scriptedHost();
    const devices = createRemoteDevices({ request: host.request, canManage: () => true });
    devices.start();
    (await host.next()).answer(Response.json(list(1)));
    (await host.next()).answer(Response.json({ error: 'administrator access required' }, { status }));
    await settle();
    expect(devices.state.value).toBe('forbidden');
    expect(devices.list.value).toBeUndefined();
    expect(host.calls).toHaveLength(2);
    devices.stop();
    expect(devices.state.value).toBe('idle');
  });

  it('never requests the list for a session without the admin grant', () => {
    const host = scriptedHost();
    const devices = createRemoteDevices({ request: host.request, canManage: () => false });
    devices.start();
    expect(devices.state.value).toBe('forbidden');
    expect(host.request).not.toHaveBeenCalled();
  });

  it('sends each change once, marks it busy, and reports a refusal in place', async () => {
    const host = scriptedHost();
    const devices = createRemoteDevices({ request: host.request, canManage: () => true });
    const approving = devices.approve('P1', 'read_write');
    expect(devices.busy.value).toEqual(['P1']);
    expect(await devices.approve('P1', 'read_only')).toBe(false);
    const approve = await host.next();
    expect([approve.method, approve.path, approve.body]).toEqual([
      'POST',
      `${REMOTE_DEVICES_PATH}/approve`,
      { id: 'P1', role: 'read_write' },
    ]);
    approve.answer(new Response(null, { status: 204 }));
    expect(await approving).toBe(true);
    expect(devices.busy.value).toEqual([]);

    const rejecting = devices.reject('P2');
    const reject = await host.next();
    expect([reject.method, reject.path, reject.body]).toEqual(['POST', `${REMOTE_DEVICES_PATH}/reject`, { id: 'P2' }]);
    reject.answer(Response.json({ error: 'pairing request not found or expired' }, { status: 404 }));
    expect(await rejecting).toBe(false);
    expect(devices.message.value).toBe('pairing request not found or expired');

    const updating = devices.update('D 1', { admin: true });
    const update = await host.next();
    expect([update.method, update.path, update.body]).toEqual([
      'PATCH',
      `${REMOTE_DEVICES_PATH}/devices/D%201`,
      { admin: true },
    ]);
    expect(devices.message.value).toBe('');
    update.answer(
      Response.json({ error: 'only the host or a host-trusted device can change host access' }, { status: 403 }),
    );
    expect(await updating).toBe(false);

    const revoking = devices.revoke('D1');
    const revoke = await host.next();
    expect([revoke.method, revoke.path, revoke.body]).toEqual([
      'DELETE',
      `${REMOTE_DEVICES_PATH}/devices/D1`,
      undefined,
    ]);
    revoke.answer(new Response('', { status: 500 }));
    expect(await revoking).toBe(false);
    expect(devices.message.value).toBe('The host refused the change.');
  });
});
