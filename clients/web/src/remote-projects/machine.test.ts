import { describe, expect, it, vi } from 'vitest';

import { remoteRouteKind } from '../dev-server';
import type { PrivateRemoteConnection, RemoteDiscoveryView } from './host';
import {
  createRemoteMachineBridge,
  type RemoteMachineBridgeDependencies,
  remoteMachineRoute,
  remoteMachineServerPath,
} from './machine';

/** HS2-CMXKK2: synthetic bindings, origins and session values only. */
const BINDING = '01ARZ3NDEKTSV4RRFFQ69G5FAV';

const view = (revision = 1, checkouts: RemoteDiscoveryView['checkouts'] = available, revoked = false) =>
  ({
    remote: {
      id: BINDING,
      label: 'Build',
      origin: 'https://build.example.test',
      identity: { server_id: 'server-1', ca_fingerprint: null },
      revision,
      revoked,
      last_failure: null,
      last_confirmed_checkouts: [],
      stale: false,
    },
    checkouts,
  }) satisfies RemoteDiscoveryView;
const available: RemoteDiscoveryView['checkouts'] = [
  { id: 'gone', name: 'Gone', state: 'inaccessible', sources: [] },
  { id: 'c1', name: 'API', state: 'available', sources: [] },
];
const connection = (revision = 1, bearer = 'session:synthetic-session'): PrivateRemoteConnection => ({
  binding: view(revision).remote,
  credentials: { bearer_secret: bearer, ca_pem: 'synthetic-ca' },
});

function harness(options: { discovery?: RemoteDiscoveryView; connection?: PrivateRemoteConnection } = {}) {
  const calls: { url: string; init: RequestInit }[] = [];
  const dependencies = {
    discover: vi.fn(() => Promise.resolve(options.discovery ?? view())),
    connection: vi.fn(() => Promise.resolve(options.connection ?? connection())),
    requireCurrent: vi.fn(() => Promise.resolve()),
    request: vi.fn<RemoteMachineBridgeDependencies['request']>((url, init) => {
      calls.push({ url: url.toString(), init });
      return Promise.resolve(
        Response.json(
          { ok: true },
          { headers: { 'content-type': 'application/json', 'set-cookie': 'hs2_remote_session=leak' } },
        ),
      );
    }),
  };
  return { bridge: createRemoteMachineBridge(dependencies), dependencies, calls };
}

describe('HS2-CMXKK2 remote machine routes', () => {
  it('allows only the three machine reads, keeping only their own query parameters', () => {
    expect(remoteMachineRoute('GET', '/host-health?secret=x')).toEqual({ route: '/host-health', search: '' });
    expect(remoteMachineRoute('GET', '/host-health/cpu?timeout_ms=10000&after=7&checkout=x')).toEqual({
      route: '/host-health/cpu',
      search: 'after=7&timeout_ms=10000',
    });
    expect(
      remoteMachineRoute('GET', '/checkouts/machine/target-capacity?after=ab&wait_ms=8000&claim_namespaces=vm&x=1'),
    ).toEqual({ route: '/target-capacity', search: 'after=ab&wait_ms=8000&claim_namespaces=vm' });
    for (const path of [
      '/checkouts/c1/target-capacity',
      '/checkouts/machine/target-admission',
      '/tickets',
      '/host-health/disks',
      '/host-health/thresholds',
      '/terminals',
      '/../host-health',
    ])
      expect(remoteMachineRoute('GET', path)).toMatchObject({ status: 404 });
    for (const method of ['POST', 'PUT', 'DELETE', 'PATCH'])
      expect(remoteMachineRoute(method, '/host-health')).toMatchObject({ status: 405 });
  });

  it('reaches capacity only through a readable checkout', () => {
    expect(remoteMachineServerPath('/host-health', '', undefined)).toBe('/host-health');
    expect(remoteMachineServerPath('/host-health/cpu', 'after=1', undefined)).toBe('/host-health/cpu?after=1');
    expect(remoteMachineServerPath('/target-capacity', 'after=v', 'c 1')).toBe(
      '/checkouts/c%201/target-capacity?after=v',
    );
    expect(remoteMachineServerPath('/target-capacity', '', undefined)).toBeUndefined();
  });

  it('needs the local grant on a remote-mode host, like a remote tab', () => {
    expect(remoteRouteKind(`/__hotsheet/remotes/${BINDING}/machine/host-health`)).toBe('host');
    expect(remoteRouteKind(`/__hotsheet/remotes/${BINDING}/machine/checkouts/machine/target-capacity`)).toBe('host');
  });
});

describe('HS2-CMXKK2 remote machine bridge', () => {
  it('forwards a read with the private session and TLS, through the first available checkout, and finds them once', async () => {
    const h = harness();
    const response = await h.bridge.proxy(BINDING, 'GET', '/checkouts/machine/target-capacity?after=v1&wait_ms=8000');
    expect(response.status).toBe(200);
    expect(await response.json()).toEqual({ ok: true });
    // Only the content type comes back: no remote cookie reaches the browser.
    expect(response.headers.get('set-cookie')).toBeNull();
    expect(h.calls[0].url).toBe('https://build.example.test/checkouts/c1/target-capacity?after=v1&wait_ms=8000');
    expect(h.calls[0].init.headers).toEqual({
      accept: 'application/json',
      cookie: 'hs2_remote_session=synthetic-session',
    });
    expect(h.dependencies.request.mock.calls[0][2]).toMatchObject({ ca_pem: 'synthetic-ca' });
    await h.bridge.proxy(BINDING, 'GET', '/host-health');
    await h.bridge.proxy(BINDING, 'GET', '/host-health/cpu?after=3');
    expect(h.calls.map((call) => new URL(call.url).pathname)).toEqual([
      '/checkouts/c1/target-capacity',
      '/host-health',
      '/host-health/cpu',
    ]);
    // Discovery and the private connection are found once; the binding is re-checked on every read.
    expect(h.dependencies.discover).toHaveBeenCalledTimes(1);
    expect(h.dependencies.connection).toHaveBeenCalledTimes(1);
    expect(h.dependencies.requireCurrent).toHaveBeenCalledTimes(3);
  });

  it('refuses before reaching the remote: a write, an unknown route, a binding that changed or was revoked', async () => {
    const h = harness();
    expect((await h.bridge.proxy(BINDING, 'POST', '/host-health')).status).toBe(405);
    expect((await h.bridge.proxy(BINDING, 'GET', '/tickets')).status).toBe(404);
    h.dependencies.requireCurrent.mockRejectedValueOnce(new Error('changed'));
    expect((await h.bridge.proxy(BINDING, 'GET', '/host-health')).status).toBe(503);
    // The changed binding was forgotten, so the next read finds it again.
    expect((await h.bridge.proxy(BINDING, 'GET', '/host-health')).status).toBe(200);
    expect(h.dependencies.discover).toHaveBeenCalledTimes(2);
    expect(h.calls).toHaveLength(1);
    const revoked = harness({ discovery: view(1, available, true) });
    expect((await revoked.bridge.proxy(BINDING, 'GET', '/host-health')).status).toBe(503);
    const moved = harness({ connection: connection(2) });
    expect((await moved.bridge.proxy(BINDING, 'GET', '/host-health')).status).toBe(503);
    const unapproved = harness({ connection: connection(1, 'bearer-without-session') });
    expect((await unapproved.bridge.proxy(BINDING, 'GET', '/host-health')).status).toBe(401);
    expect([revoked, moved, unapproved].flatMap((each) => each.calls)).toEqual([]);
  });

  it('says why capacity cannot be read without an available checkout, while host health still can', async () => {
    const h = harness({ discovery: view(1, [{ id: 'gone', name: 'Gone', state: 'inaccessible', sources: [] }]) });
    const capacity = await h.bridge.proxy(BINDING, 'GET', '/checkouts/machine/target-capacity');
    expect(capacity.status).toBe(409);
    expect(await capacity.json()).toEqual({
      error: 'This remote server has no project this computer can read its capacity through.',
    });
    expect((await h.bridge.proxy(BINDING, 'GET', '/host-health')).status).toBe(200);
    const none = harness({ discovery: view(1, null) });
    expect((await none.bridge.proxy(BINDING, 'GET', '/checkouts/machine/target-capacity')).status).toBe(409);
  });

  it('finds the connection again after the remote refuses the session or goes away, and after forget', async () => {
    const h = harness();
    h.dependencies.request.mockResolvedValueOnce(new Response('{}', { status: 401 }));
    expect((await h.bridge.proxy(BINDING, 'GET', '/host-health')).status).toBe(401);
    await h.bridge.proxy(BINDING, 'GET', '/host-health');
    expect(h.dependencies.discover).toHaveBeenCalledTimes(2);
    h.dependencies.request.mockRejectedValueOnce(new Error('connection reset'));
    expect((await h.bridge.proxy(BINDING, 'GET', '/host-health')).status).toBe(503);
    await h.bridge.proxy(BINDING, 'GET', '/host-health');
    expect(h.dependencies.discover).toHaveBeenCalledTimes(3);
    h.bridge.forget(BINDING);
    await h.bridge.proxy(BINDING, 'GET', '/host-health');
    expect(h.dependencies.discover).toHaveBeenCalledTimes(4);
    // A failed discovery is not kept either.
    h.bridge.forget(BINDING);
    h.dependencies.discover.mockRejectedValueOnce(new Error('unreachable'));
    expect((await h.bridge.proxy(BINDING, 'GET', '/host-health')).status).toBe(503);
    expect((await h.bridge.proxy(BINDING, 'GET', '/host-health')).status).toBe(200);
  });

  it('passes an abort through to the caller, so a stopped long poll is not a lost remote', async () => {
    const h = harness();
    const abort = new AbortController();
    h.dependencies.request.mockImplementationOnce((_url: URL, init: RequestInit) => {
      abort.abort();
      expect(init.signal).toBe(abort.signal);
      return Promise.reject(new DOMException('aborted', 'AbortError'));
    });
    await expect(h.bridge.proxy(BINDING, 'GET', '/host-health/cpu?after=1', abort.signal)).rejects.toThrow('aborted');
    await h.bridge.proxy(BINDING, 'GET', '/host-health');
    expect(h.dependencies.discover).toHaveBeenCalledTimes(1);
  });
});
