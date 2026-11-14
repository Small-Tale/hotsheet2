import { once } from 'node:events';
import { createServer, type Server } from 'node:http';
import type { AddressInfo } from 'node:net';

import { afterEach, describe, expect, it, vi } from 'vitest';
import WebSocket from 'ws';

import { CHANGE_WS_MUX_PATH } from './change-ws-mux';
import { hideRemoteProjects, installProjectWebSocketBridge } from './terminal-ws-bridge';

const grants = vi.hoisted(() => ({ local: false }));

vi.mock('./project-bridge', async (importOriginal) => ({
  ...(await importOriginal<typeof import('./project-bridge')>()),
  // The real session check asks the machine server; here it answers with the test's grants.
  browserAuthRequest: vi.fn(async () => Response.json({ origin: 'http://synthetic.invalid', ...grants })),
}));

const REMOTE = encodeURIComponent('remote:01ARZ3NDEKTSV4RRFFQ69G5FAV:synthetic');
const servers: Server[] = [];

afterEach(async () => {
  vi.unstubAllEnvs();
  await Promise.all(servers.splice(0).map((server) => new Promise((done) => server.close(done))));
});

async function bridge(resolveSync: (project: string) => string | undefined) {
  vi.stubEnv('HOTSHEET_BROWSER_AUTH_MODE', 'remote');
  const http = createServer();
  servers.push(http);
  installProjectWebSocketBridge(
    { httpServer: http },
    () => undefined,
    resolveSync,
    () => undefined,
  );
  await new Promise<void>((done) => {
    http.listen(0, '127.0.0.1', () => {
      done();
    });
  });
  const port = (http.address() as AddressInfo).port;
  const open = (path: string) => {
    const client = new WebSocket(`ws://127.0.0.1:${port}${path}`, {
      headers: { cookie: 'hs2_remote_session=synthetic', origin: `http://127.0.0.1:${port}` },
    });
    client.on('error', () => undefined);
    return client;
  };
  return { open };
}

describe('HS2-F04J42 cross-server tabs on a remote-mode host need the local grant', () => {
  it('hides only remote project ids from a resolver', async () => {
    const resolve = vi.fn((project: string) => `ws://upstream.invalid/${project}`);
    const hidden = hideRemoteProjects(resolve);
    expect(await hidden(REMOTE)).toBeUndefined();
    expect(await hidden(decodeURIComponent(REMOTE))).toBeUndefined();
    expect(await hidden('local-checkout')).toBe('ws://upstream.invalid/local-checkout');
    expect(resolve).toHaveBeenCalledTimes(1);
  });

  it('refuses a remote project socket to a plain session without resolving it', async () => {
    grants.local = false;
    const resolveSync = vi.fn<(project: string) => string | undefined>(() => undefined);
    const { open } = await bridge(resolveSync);
    const client = open(`/__hotsheet/project-api/${REMOTE}/ws/sync`);
    const [, response] = (await once(client, 'unexpected-response')) as [unknown, { statusCode?: number }];
    expect(response.statusCode).toBe(404);
    expect(resolveSync).not.toHaveBeenCalled();
    client.terminate();
    // The same session still resolves this host's own projects.
    const local = open('/__hotsheet/project-api/local-checkout/ws/sync');
    await once(local, 'unexpected-response');
    expect(resolveSync).toHaveBeenCalledWith('local-checkout', undefined);
    local.terminate();
  });

  it('resolves a remote project socket for a local-grant session', async () => {
    grants.local = true;
    const resolveSync = vi.fn<(project: string) => string | undefined>(() => undefined);
    const { open } = await bridge(resolveSync);
    const client = open(`/__hotsheet/project-api/${REMOTE}/ws/sync`);
    await once(client, 'unexpected-response');
    expect(resolveSync).toHaveBeenCalledWith(decodeURIComponent(REMOTE), undefined);
    client.terminate();
  });

  it('closes a change-mux subscription to a remote project for a plain session', async () => {
    grants.local = false;
    const resolveSync = vi.fn<(project: string) => string | undefined>(() => undefined);
    const { open } = await bridge(resolveSync);
    const client = open(CHANGE_WS_MUX_PATH);
    await once(client, 'open');
    const answer = new Promise<unknown>((done) => {
      client.on('message', (data) => {
        done(JSON.parse((data as Buffer).toString()));
      });
    });
    const project = decodeURIComponent(REMOTE);
    client.send(JSON.stringify({ version: 2, type: 'subscribe', project, client: 'tab', subscription: 1 }));
    expect(await answer).toMatchObject({ type: 'closed', project, subscription: 1 });
    expect(resolveSync).not.toHaveBeenCalled();
    client.close();
  });
});
