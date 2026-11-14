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

interface FakeTerminalHost {
  origin: string;
  requests: Array<{ method: string; path: string; body?: unknown; acceptEncoding?: string }>;
  upgrades: Array<{ url: string; cookie?: string; origin?: string; csrf?: string }>;
  terminals: Array<Record<string, unknown>>;
}

function webSocketText(data: WebSocket.RawData): string {
  if (Array.isArray(data)) return Buffer.concat(data).toString('utf8');
  if (Buffer.isBuffer(data)) return data.toString('utf8');
  return Buffer.from(data).toString('utf8');
}

/** A synthetic reviewed server: checkout roots, a terminal list, and a PTY attach socket. */
async function fakeTerminalHost(roots: Record<string, string>): Promise<FakeTerminalHost> {
  const host: FakeTerminalHost = { origin: '', requests: [], upgrades: [], terminals: [] };
  const server = createServer((request, response) => {
    const chunks: Buffer[] = [];
    request.on('data', (chunk: Buffer) => chunks.push(chunk));
    request.on('end', () => {
      const text = Buffer.concat(chunks).toString('utf8');
      const path = request.url ?? '';
      const encoding = request.headers['accept-encoding'];
      host.requests.push({
        method: request.method ?? 'GET',
        path,
        ...(text ? { body: JSON.parse(text) as unknown } : {}),
        ...(encoding ? { acceptEncoding: encoding } : {}),
      });
      response.setHeader('content-type', 'application/json');
      const checkout = /^\/checkouts\/([^/?]+)$/.exec(path);
      if (checkout) {
        const id = decodeURIComponent(checkout[1]);
        response.end(JSON.stringify({ id, root: roots[id], alias: id, stores: [] }));
      } else if (path === '/terminals' && request.method === 'POST') {
        const body = JSON.parse(text) as { cwd: string };
        const created = { id: `t${host.terminals.length + 1}`, alive: true, busy: false, launch_cwd: body.cwd };
        host.terminals.push(created);
        response.end(JSON.stringify(created));
      } else if (path === '/terminals') response.end(JSON.stringify(host.terminals));
      else {
        response.statusCode = 404;
        response.end('{}');
      }
    });
  });
  const upstream = new WebSocketServer({ server });
  upstream.on('connection', (socket, request) => {
    host.upgrades.push({
      url: request.url ?? '',
      cookie: request.headers.cookie,
      origin: request.headers.origin,
      csrf: request.headers['x-hotsheet-csrf'] as string | undefined,
    });
    socket.on('message', (data) => {
      socket.send(`echo:${webSocketText(data)}`);
    });
    socket.send('replay');
  });
  servers.push(server);
  websockets.push(upstream);
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  const address = server.address();
  if (!address || typeof address === 'string') throw new Error('fixture did not bind');
  host.origin = `http://127.0.0.1:${address.port}`;
  return host;
}

async function writeRemoteRegistry(entries: Array<{ id: string; origin: string; revoked?: boolean }>): Promise<void> {
  let home = process.env.HOTSHEET_HOME;
  if (!home || !homes.includes(home)) {
    home = await mkdtemp(join(tmpdir(), 'hs2-pphjmz-'));
    homes.push(home);
    process.env.HOTSHEET_HOME = home;
  }
  await writeFile(
    join(home, 'remotes.json'),
    JSON.stringify({
      servers: entries.map(({ id, origin, revoked }) => ({
        id,
        revision: 1,
        revoked: revoked ?? false,
        origin,
        identity: { server_id: id },
      })),
    }),
  );
}

function openRemoteSession(bindingId: string, origin: string, checkoutId: string, token: string): string {
  const project = remoteProjectId(bindingId, checkoutId);
  projectSessionRegistry().set(project, {
    url: origin,
    secret: '',
    remote: {
      bindingId,
      bindingRevision: 1,
      checkoutId,
      connection: {
        binding: {
          id: bindingId,
          label: bindingId,
          revision: 1,
          revoked: false,
          origin,
          identity: { server_id: bindingId, ca_fingerprint: null },
          last_failure: null,
          last_confirmed_checkouts: [],
          stale: false,
        },
        credentials: {
          bearer_secret: `session:${token}`,
          session_origin: 'http://localhost:4175',
          session_csrf: `${token}-csrf`,
        },
      },
    },
  });
  return project;
}

async function terminalBridge(): Promise<number> {
  const bridge = createServer();
  servers.push(bridge);
  installProjectWebSocketBridge({ httpServer: bridge });
  bridge.listen(0, '127.0.0.1');
  await once(bridge, 'listening');
  const address = bridge.address();
  if (!address || typeof address === 'string') throw new Error('bridge did not bind');
  return address.port;
}

function attachUrl(port: number, project: string, terminal: string): string {
  return `ws://127.0.0.1:${port}/__hotsheet/project-api/${encodeURIComponent(project)}/terminals/${terminal}/attach`;
}

async function attach(port: number, project: string, terminal: string): Promise<WebSocket> {
  const socket = new WebSocket(`${attachUrl(port, project, terminal)}?replay_protocol=2&secret=forged`);
  clients.push(socket);
  const [replay] = (await once(socket, 'message')) as [WebSocket.RawData];
  expect(webSocketText(replay)).toBe('replay');
  return socket;
}

async function refused(port: number, project: string, terminal: string): Promise<void> {
  const socket = new WebSocket(attachUrl(port, project, terminal));
  clients.push(socket);
  await new Promise<void>((resolve, reject) => {
    socket.once('open', () => {
      reject(new Error('a refused remote terminal opened'));
    });
    socket.once('error', () => {
      resolve();
    });
  });
}

async function proxied(project: string, path: string, init: RequestInit = {}): Promise<Response> {
  return proxyProjectRequest(project, path, new Request(`http://bridge.invalid${path}`, init));
}

describe('HS2-PPHJMZ remote terminal transport', () => {
  it('attaches each tab to its own server PTY with that server session, and stops at revocation', async () => {
    const left = await fakeTerminalHost({ same: '/srv/left' });
    const right = await fakeTerminalHost({ same: '/srv/right' });
    await writeRemoteRegistry([
      { id: first, origin: left.origin },
      { id: second, origin: right.origin },
    ]);
    const leftProject = openRemoteSession(first, left.origin, 'same', 'left-token');
    const rightProject = openRemoteSession(second, right.origin, 'same', 'right-token');
    const port = await terminalBridge();

    const leftSocket = await attach(port, leftProject, 't1');
    const rightSocket = await attach(port, rightProject, 't1');
    expect(left.upgrades).toEqual([
      {
        url: '/terminals/t1/attach?replay_protocol=2',
        cookie: 'hs2_remote_session=left-token',
        origin: 'http://localhost:4175',
        csrf: 'left-token-csrf',
      },
    ]);
    expect(right.upgrades).toEqual([
      {
        url: '/terminals/t1/attach?replay_protocol=2',
        cookie: 'hs2_remote_session=right-token',
        origin: 'http://localhost:4175',
        csrf: 'right-token-csrf',
      },
    ]);
    leftSocket.send('ls');
    const [echo] = (await once(leftSocket, 'message')) as [WebSocket.RawData];
    expect(webSocketText(echo)).toBe('echo:ls');

    // Reconnect: a dropped viewer re-resolves and attaches again with the same session.
    leftSocket.close();
    await once(leftSocket, 'close');
    const again = await attach(port, leftProject, 't1');
    expect(left.upgrades).toHaveLength(2);

    // Removing one binding mid-session ends only that server's terminal streams.
    closeRemoteServerSessions(first);
    await once(again, 'close');
    expect(rightSocket.readyState).toBe(WebSocket.OPEN);
    await refused(port, leftProject, 't1');
    expect(left.upgrades).toHaveLength(2);

    // A binding the registry no longer approves is refused before any upgrade reaches it.
    await writeRemoteRegistry([
      { id: first, origin: left.origin },
      { id: second, origin: right.origin, revoked: true },
    ]);
    await refused(port, rightProject, 't2');
    expect(right.upgrades).toHaveLength(1);
  });

  it('launches in the remote checkout and lists a terminal only in the tab that owns it', async () => {
    const left = await fakeTerminalHost({ app: '/srv/app', api: '/srv/api' });
    const right = await fakeTerminalHost({ app: '/srv/app' });
    await writeRemoteRegistry([
      { id: first, origin: left.origin },
      { id: second, origin: right.origin },
    ]);
    const app = openRemoteSession(first, left.origin, 'app', 'left-token');
    const api = openRemoteSession(first, left.origin, 'api', 'left-token');
    const twin = openRemoteSession(second, right.origin, 'app', 'right-token');
    const launch = (project: string, cwd: string) =>
      proxied(project, '/terminals', {
        method: 'POST',
        headers: { 'content-type': 'application/json', 'accept-encoding': 'gzip' },
        body: JSON.stringify({ cwd, connect: 'claude' }),
      });

    const created = await launch(app, app);
    expect(created.status).toBe(200);
    expect(await created.json()).toEqual({ id: 't1', alive: true, busy: false, launch_cwd: app });
    expect(left.requests.filter((request) => request.method === 'POST')).toEqual([
      { method: 'POST', path: '/terminals', body: { cwd: '/srv/app', connect: 'claude' } },
    ]);
    expect((await launch(api, `${api}/services`)).status).toBe(200);
    expect(left.terminals.map((terminal) => terminal.launch_cwd)).toEqual(['/srv/app', '/srv/api/services']);
    expect((await launch(twin, twin)).status).toBe(200);
    // Another tab's composite id never reaches the server as a directory.
    const denied = await launch(app, api);
    expect(denied.status).toBe(400);
    expect(left.terminals).toHaveLength(2);
    left.terminals.push({ id: 'stray', alive: true, busy: false, launch_cwd: '/home/someone' });

    const list = async (project: string) => (await (await proxied(project, '/terminals')).json()) as unknown;
    expect(await list(app)).toEqual([{ id: 't1', alive: true, busy: false, launch_cwd: app }]);
    expect(await list(api)).toEqual([{ id: 't2', alive: true, busy: false, launch_cwd: api }]);
    expect(await list(twin)).toEqual([{ id: 't1', alive: true, busy: false, launch_cwd: twin }]);
    // Each checkout root is read once per open tab, and terminal answers arrive uncompressed.
    expect(
      left.requests.filter((request) => request.path.startsWith('/checkouts/')).map((request) => request.path),
    ).toEqual(['/checkouts/app', '/checkouts/api']);
    expect(left.requests.every((request) => request.acceptEncoding === undefined)).toBe(true);

    // Closing a tab hands its terminals to no other tab.
    expect((await proxied(api, '/close', { method: 'POST' })).status).toBe(200);
    expect(await list(app)).toEqual([{ id: 't1', alive: true, busy: false, launch_cwd: app }]);
  });
});
