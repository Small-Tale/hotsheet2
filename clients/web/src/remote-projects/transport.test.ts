import { once } from 'node:events';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';
import WebSocket, { WebSocketServer } from 'ws';

import { closeRemoteServerSessions, projectSessionRegistry, proxyProjectRequest } from '../project-bridge';
import { installProjectWebSocketBridge } from '../terminal-ws-bridge';
import { remoteProjectId } from './identity';

const first = '01ARZ3NDEKTSV4RRFFQ69G5FAV';
const second = '01ARZ3NDEKTSV4RRFFQ69G5FAW';
const originalHome = process.env.HOTSHEET_HOME;
const homes: string[] = [];
const servers: Array<ReturnType<typeof createServer>> = [];
const websockets: WebSocketServer[] = [];
const clients: WebSocket[] = [];

afterEach(async () => {
  projectSessionRegistry().clear();
  if (originalHome === undefined) delete process.env.HOTSHEET_HOME;
  else process.env.HOTSHEET_HOME = originalHome;
  for (const socket of clients.splice(0)) socket.terminate();
  for (const server of websockets.splice(0)) {
    for (const socket of server.clients) socket.terminate();
    await new Promise<void>((resolve) => {
      server.close(() => {
        resolve();
      });
    });
  }
  await Promise.all(
    servers.splice(0).map(
      (server) =>
        new Promise<void>((resolve) =>
          server.close(() => {
            resolve();
          }),
        ),
    ),
  );
  await Promise.all(homes.splice(0).map((home) => rm(home, { recursive: true, force: true })));
});

describe('HS2-BT4KN6 private two-server transport', () => {
  it('keeps same-named checkout paths and write proofs bound to their reviewed server', async () => {
    const observed: Array<{
      peer: string;
      path: string;
      cookie?: string;
      origin?: string;
      csrf?: string;
      forged?: string;
    }> = [];
    const start = async (peer: string) => {
      const server = createServer((request, response) => {
        observed.push({
          peer,
          path: request.url ?? '',
          cookie: request.headers.cookie,
          origin: request.headers.origin,
          csrf: request.headers['x-hotsheet-csrf'] as string | undefined,
          forged: request.headers['x-hotsheet-secret'] as string | undefined,
        });
        response.setHeader('content-type', 'application/json');
        response.end(JSON.stringify({ peer }));
      });
      servers.push(server);
      server.listen(0, '127.0.0.1');
      await once(server, 'listening');
      const address = server.address();
      if (!address || typeof address === 'string') throw new Error('fixture did not bind');
      return `http://127.0.0.1:${address.port}`;
    };
    const leftOrigin = await start('left');
    const rightOrigin = await start('right');
    const home = await mkdtemp(join(tmpdir(), 'hs2-bt4kn6-'));
    homes.push(home);
    process.env.HOTSHEET_HOME = home;
    await writeFile(
      join(home, 'remotes.json'),
      JSON.stringify({
        servers: [
          { id: first, revision: 1, revoked: false, origin: leftOrigin, identity: { server_id: first } },
          { id: second, revision: 1, revoked: false, origin: rightOrigin, identity: { server_id: second } },
        ],
      }),
    );
    for (const [id, origin, token, csrf] of [
      [first, leftOrigin, 'left-token', 'left-csrf'],
      [second, rightOrigin, 'right-token', 'right-csrf'],
    ]) {
      projectSessionRegistry().set(remoteProjectId(id, 'same'), {
        url: origin,
        secret: '',
        remote: {
          bindingId: id,
          bindingRevision: 1,
          checkoutId: 'same',
          connection: {
            binding: {
              id,
              label: id,
              revision: 1,
              revoked: false,
              origin,
              identity: { server_id: id, ca_fingerprint: null },
              last_failure: null,
              last_confirmed_checkouts: [],
              stale: false,
            },
            credentials: {
              bearer_secret: `session:${token}`,
              session_origin: 'http://localhost:4175',
              session_csrf: csrf,
            },
          },
        },
      });
    }
    for (const id of [first, second]) {
      const project = remoteProjectId(id, 'same');
      const path = `/checkouts/${encodeURIComponent(project)}/tickets`;
      const response = await proxyProjectRequest(
        project,
        path,
        new Request(`http://bridge.invalid${path}`, {
          method: 'POST',
          headers: { 'x-hotsheet-secret': 'forged', origin: 'http://attacker.invalid' },
          body: '{}',
        }),
      );
      expect(response.status).toBe(200);
    }
    expect(observed).toEqual([
      {
        peer: 'left',
        path: '/checkouts/same/tickets',
        cookie: 'hs2_remote_session=left-token',
        origin: 'http://localhost:4175',
        csrf: 'left-csrf',
        forged: undefined,
      },
      {
        peer: 'right',
        path: '/checkouts/same/tickets',
        cookie: 'hs2_remote_session=right-token',
        origin: 'http://localhost:4175',
        csrf: 'right-csrf',
        forged: undefined,
      },
    ]);
    const wrong = remoteProjectId(second, 'same');
    const denied = await proxyProjectRequest(
      remoteProjectId(first, 'same'),
      `/checkouts/${encodeURIComponent(wrong)}/tickets`,
      new Request('http://bridge.invalid'),
    );
    expect(denied.status).toBe(403);
    expect(observed).toHaveLength(2);
  });

  it('isolates two remote change sockets and closes only the revoked server', async () => {
    const seen: Array<{ peer: string; url: string; cookie?: string; csrf?: string }> = [];
    const start = async (peer: string) => {
      const server = createServer();
      const upstream = new WebSocketServer({ server });
      servers.push(server);
      websockets.push(upstream);
      upstream.on('connection', (_socket, request) => {
        seen.push({
          peer,
          url: request.url ?? '',
          cookie: request.headers.cookie,
          csrf: request.headers['x-hotsheet-csrf'] as string | undefined,
        });
      });
      server.listen(0, '127.0.0.1');
      await once(server, 'listening');
      const address = server.address();
      if (!address || typeof address === 'string') throw new Error('fixture did not bind');
      return `http://127.0.0.1:${address.port}`;
    };
    const leftOrigin = await start('left');
    const rightOrigin = await start('right');
    const home = await mkdtemp(join(tmpdir(), 'hs2-bt4kn6-ws-'));
    homes.push(home);
    process.env.HOTSHEET_HOME = home;
    await writeFile(
      join(home, 'remotes.json'),
      JSON.stringify({
        servers: [
          { id: first, revision: 1, revoked: false, origin: leftOrigin, identity: { server_id: first } },
          { id: second, revision: 1, revoked: false, origin: rightOrigin, identity: { server_id: second } },
        ],
      }),
    );
    for (const [id, origin, token, csrf] of [
      [first, leftOrigin, 'left-token', 'left-csrf'],
      [second, rightOrigin, 'right-token', 'right-csrf'],
    ]) {
      projectSessionRegistry().set(remoteProjectId(id, 'same'), {
        url: origin,
        secret: '',
        remote: {
          bindingId: id,
          bindingRevision: 1,
          checkoutId: 'same',
          connection: {
            binding: {
              id,
              label: id,
              revision: 1,
              revoked: false,
              origin,
              identity: { server_id: id, ca_fingerprint: null },
              last_failure: null,
              last_confirmed_checkouts: [],
              stale: false,
            },
            credentials: {
              bearer_secret: `session:${token}`,
              session_origin: 'http://localhost:4175',
              session_csrf: csrf,
            },
          },
        },
      });
    }
    const bridge = createServer();
    servers.push(bridge);
    installProjectWebSocketBridge({ httpServer: bridge });
    bridge.listen(0, '127.0.0.1');
    await once(bridge, 'listening');
    const address = bridge.address();
    if (!address || typeof address === 'string') throw new Error('bridge did not bind');
    const open = async (id: string) => {
      const socket = new WebSocket(
        `ws://127.0.0.1:${address.port}/__hotsheet/project-api/${encodeURIComponent(remoteProjectId(id, 'same'))}/ws/sync`,
      );
      clients.push(socket);
      await once(socket, 'open');
      return socket;
    };
    const left = await open(first);
    const right = await open(second);
    await new Promise<void>((resolve) => {
      const inspect = () => {
        if (seen.length === 2) resolve();
        else setTimeout(inspect, 5);
      };
      inspect();
    });
    expect(seen).toEqual([
      { peer: 'left', url: '/ws/sync?checkout=same', cookie: 'hs2_remote_session=left-token', csrf: 'left-csrf' },
      { peer: 'right', url: '/ws/sync?checkout=same', cookie: 'hs2_remote_session=right-token', csrf: 'right-csrf' },
    ]);
    closeRemoteServerSessions(first);
    await once(left, 'close');
    expect(right.readyState).toBe(WebSocket.OPEN);
  });
});
