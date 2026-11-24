import { afterEach, describe, expect, it, vi } from 'vitest';

import { projectSessionRegistry, proxyProjectRequest } from './project-bridge';
import { remoteProjectId } from './remote-projects/identity';
import { remoteRequest } from './remote-projects/transport';

vi.mock('./remote-projects/host', async (importOriginal) => ({
  ...(await importOriginal<typeof import('./remote-projects/host')>()),
  requireCurrentRemoteBinding: vi.fn(),
}));
vi.mock('./remote-projects/transport', () => ({ remoteRequest: vi.fn() }));

const hostA = '01ARZ3NDEKTSV4RRFFQ69G5FAV';
const hostB = '01ARZ3NDEKTSV4RRFFQ69G5FAW';
const app = remoteProjectId(hostA, 'same');
const api = remoteProjectId(hostA, 'api');
const equalOtherHost = remoteProjectId(hostB, 'same');
const staleRevision = remoteProjectId(hostA, 'stale-revision');
const staleIdentity = remoteProjectId(hostA, 'stale-identity');
const registry = projectSessionRegistry();
const credentials = { bearer_secret: 'session:approved', session_origin: 'https://host.test', session_csrf: 'csrf' };

function register(
  id: string,
  bindingId: string,
  checkoutId: string,
  root: string,
  origin: string,
  revision = 1,
  serverId = bindingId,
): void {
  registry.set(id, {
    url: origin,
    secret: '',
    remote: {
      bindingId,
      bindingRevision: revision,
      checkoutId,
      root,
      stores: [`${root}.hs2`],
      connection: { credentials, binding: { identity: { server_id: serverId } } } as never,
    },
  });
}

async function request(id: string, path: string, method = 'GET', body?: object): Promise<Response> {
  return proxyProjectRequest(
    id,
    path,
    new Request(`http://bridge.invalid${path}`, {
      method,
      ...(body === undefined ? {} : { body: JSON.stringify(body) }),
    }),
  );
}

afterEach(() => {
  registry.delete(app);
  registry.delete(api);
  registry.delete(equalOtherHost);
  registry.delete(staleRevision);
  registry.delete(staleIdentity);
  vi.mocked(remoteRequest).mockReset();
});

describe('HS2-TCJQHX remote AI bridge', () => {
  it('translates create and scopes live and saved lists across two checkouts on one host', async () => {
    register(app, hostA, 'same', '/srv/app', 'https://a.test');
    register(api, hostA, 'api', '/srv/api', 'https://a.test');
    const seen: Array<{ path: string; body: unknown }> = [];
    vi.mocked(remoteRequest).mockImplementation(async (url, init) => {
      const path = new URL(url).pathname;
      seen.push({
        path,
        body: init.body ? JSON.parse(Buffer.from(init.body as ArrayBuffer).toString('utf8')) : undefined,
      });
      if (path === '/drive/connections') return Response.json({ id: 'new', project: '/srv/app', tool: 'claude' });
      if (path === '/connections')
        return Response.json([
          { id: 'one', project: '/srv/app', tool: 'claude' },
          { id: 'two', project: '/srv/api', tool: 'claude' },
          { id: 'store', project: '/srv/app.hs2', tool: 'claude' },
        ]);
      if (path === '/drive/sessions')
        return Response.json([
          { connection_id: 'one', project: '/srv/app', session_id: 's1' },
          { connection_id: 'two', project: '/srv/api', session_id: 's2' },
        ]);
      throw new Error(`Unexpected ${path}`);
    });

    const created = await request(app, '/drive/connections', 'POST', { checkout: app, tool: 'claude' });
    expect(created.status).toBe(200);
    expect(await created.json()).toEqual({ id: 'new', project: app, tool: 'claude' });
    expect(seen[0]).toEqual({ path: '/drive/connections', body: { checkout: 'same', tool: 'claude' } });
    expect(await (await request(app, '/connections')).json()).toEqual([
      { id: 'one', project: app, tool: 'claude' },
      { id: 'store', project: app, tool: 'claude' },
    ]);
    expect(await (await request(api, '/connections')).json()).toEqual([{ id: 'two', project: api, tool: 'claude' }]);
    expect(await (await request(app, '/drive/sessions')).json()).toEqual([
      { connection_id: 'one', project: app, session_id: 's1' },
    ]);
    const foreign = await request(app, '/drive/connections', 'POST', { checkout: api, tool: 'claude' });
    expect(foreign.status).toBe(400);
    vi.mocked(remoteRequest).mockResolvedValueOnce(Response.json({ id: 'wrong', project: '/srv/api', tool: 'claude' }));
    const wrongAnswer = await request(app, '/drive/connections', 'POST', { checkout: app, tool: 'claude' });
    expect(wrongAnswer.status).toBe(502);
  });

  it('keeps equal native checkout ids on different reviewed hosts separate', async () => {
    register(app, hostA, 'same', '/srv/shared', 'https://a.test');
    register(equalOtherHost, hostB, 'same', '/srv/shared', 'https://b.test');
    vi.mocked(remoteRequest).mockImplementation(async (url) =>
      Response.json([{ id: new URL(url).hostname, project: '/srv/shared', tool: 'claude' }]),
    );
    expect(await (await request(app, '/connections')).json()).toEqual([{ id: 'a.test', project: app, tool: 'claude' }]);
    expect(await (await request(equalOtherHost, '/connections')).json()).toEqual([
      { id: 'b.test', project: equalOtherHost, tool: 'claude' },
    ]);
  });

  it('does not consult tabs from an old binding revision or server identity', async () => {
    register(app, hostA, 'same', '/srv/app', 'https://a.test');
    register(staleRevision, hostA, 'old', '/srv/old', 'https://a.test', 0);
    register(staleIdentity, hostA, 'replaced', '/srv/replaced', 'https://a.test', 1, hostB);
    registry.get(staleRevision)!.remote!.root = undefined;
    registry.get(staleIdentity)!.remote!.root = undefined;
    vi.mocked(remoteRequest).mockImplementation(async (url) => {
      if (new URL(url).pathname !== '/connections') throw new Error('Stale peer was consulted');
      return Response.json([{ id: 'owned', project: '/srv/app', tool: 'claude' }]);
    });
    expect(await (await request(app, '/connections')).json()).toEqual([{ id: 'owned', project: app, tool: 'claude' }]);
    expect(remoteRequest).toHaveBeenCalledTimes(1);
  });
});
