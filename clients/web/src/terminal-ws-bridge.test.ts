import { EventEmitter, once } from 'node:events';
import { createServer } from 'node:http';
import { PassThrough } from 'node:stream';

import { afterEach, describe, expect, it, vi } from 'vitest';
import WebSocket, { WebSocketServer } from 'ws';

import {
  browserProjectWebSocketTarget,
  browserTerminalAttachTarget,
  installProjectWebSocketBridge,
  installTerminalWebSocketBridge,
  isProjectWebSocketPath,
} from './terminal-ws-bridge';

const servers: Array<{ close(callback: () => void): void }> = [];
afterEach(async () => {
  await Promise.all(
    servers.splice(0).map(
      (server) =>
        new Promise<void>((resolve) => {
          server.close(() => {
            resolve();
          });
        }),
    ),
  );
});
const listen = async (server: ReturnType<typeof createServer>) => {
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  const address = server.address();
  if (!address || typeof address === 'string') throw new Error('missing test address');
  return address.port;
};

describe('terminal WebSocket bridge', () => {
  it('parses only credential-free project terminal attach paths', async () => {
    const resolve = (project: string, terminal: string) => `${project}:${terminal}`;
    await expect(
      browserTerminalAttachTarget('/__hotsheet/project-api/project%20one/terminals/codex%2Fmain/attach', resolve),
    ).resolves.toBe('project one:codex/main');
    await expect(
      browserTerminalAttachTarget('/__hotsheet/project-api/project/terminals', resolve),
    ).resolves.toBeUndefined();
  });

  it('awaits asynchronous supervision before resolving a reconnect target', async () => {
    const resolve = async (project: string, terminal: string) => {
      await Promise.resolve();
      return `${project}:${terminal}`;
    };
    await expect(
      browserTerminalAttachTarget('/__hotsheet/project-api/project/terminals/main/attach', resolve),
    ).resolves.toBe('project:main');
  });

  it('routes the credential-free project sync path separately from terminal attaches', async () => {
    const terminal = vi.fn(async (project: string, id: string) => `terminal:${project}:${id}`),
      sync = vi.fn(async (project: string) => `sync:${project}`);
    await expect(
      browserProjectWebSocketTarget('/__hotsheet/project-api/project%20one/ws/sync', terminal, sync),
    ).resolves.toBe('sync:project one');
    await expect(
      browserProjectWebSocketTarget('/__hotsheet/project-api/project%20one/terminals/main/attach', terminal, sync),
    ).resolves.toBe('terminal:project one:main');
    expect(sync).toHaveBeenCalledOnce();
    // The browser tab's client id passes through so the server can track which projects are open.
    await expect(
      browserProjectWebSocketTarget('/__hotsheet/project-api/project%20one/ws/sync?client=tab-1', terminal, sync),
    ).resolves.toBe('sync:project one');
    expect(sync).toHaveBeenLastCalledWith('project one', 'tab-1');
  });

  it('forwards text and binary frames across an actual WebSocket upgrade', async () => {
    const upstreamHttp = createServer(),
      upstream = new WebSocketServer({ server: upstreamHttp });
    servers.push(upstream, upstreamHttp);
    upstream.on('connection', (socket) => {
      socket.on('message', (data, binary) => {
        socket.send(data, { binary });
      });
    });
    const upstreamPort = await listen(upstreamHttp),
      bridgeHttp = createServer();
    servers.push(bridgeHttp);
    installTerminalWebSocketBridge(
      { httpServer: bridgeHttp },
      () => `ws://127.0.0.1:${upstreamPort}/terminals/test/attach?secret=server-only`,
    );
    const bridgePort = await listen(bridgeHttp);
    const client = new WebSocket(`ws://127.0.0.1:${bridgePort}/__hotsheet/project-api/project/terminals/test/attach`);
    await once(client, 'open');
    client.send('hello');
    const [text] = await once(client, 'message');
    expect(text.toString()).toBe('hello');
    client.send(Uint8Array.from([1, 2, 3]));
    const [binary] = await once(client, 'message');
    expect([...(binary as Buffer)]).toEqual([1, 2, 3]);
    client.close();
    await once(client, 'close');
  });

  it('refuses an unresolvable project socket at once and leaves other upgrade paths alone (HS2-CNWHW3)', async () => {
    const bridgeHttp = createServer();
    servers.push(bridgeHttp);
    installProjectWebSocketBridge(
      { httpServer: bridgeHttp },
      () => undefined,
      () => undefined,
    );
    const port = await listen(bridgeHttp);
    for (const path of [
      '/__hotsheet/project-api/closed/ws/sync',
      '/__hotsheet/project-api/closed/terminals/t1/attach',
    ]) {
      const started = Date.now(),
        client = new WebSocket(`ws://127.0.0.1:${port}${path}`);
      client.on('error', () => undefined);
      const [, response] = (await once(client, 'unexpected-response')) as [unknown, { statusCode?: number }];
      expect(response.statusCode).toBe(404);
      // Well inside the browser transport's 2 s handshake timeout.
      expect(Date.now() - started).toBeLessThan(2_000);
      client.terminate();
    }
    // A path the bridge does not own is left to its own handler (Vite HMR), untouched.
    const foreign: Array<{ destroyed: boolean; destroy(): void }> = [];
    bridgeHttp.on('upgrade', (request: { url?: string }, socket: { destroyed: boolean; destroy(): void }) => {
      if (request.url === '/vite-hmr') foreign.push(socket);
    });
    let settled = false;
    const other = new WebSocket(`ws://127.0.0.1:${port}/vite-hmr`);
    other.on('unexpected-response', () => (settled = true));
    other.on('error', () => (settled = true));
    await vi.waitFor(() => {
      expect(foreign).toHaveLength(1);
    });
    await new Promise((resolve) => setTimeout(resolve, 150));
    expect(settled).toBe(false);
    expect(foreign[0].destroyed).toBe(false);
    foreign[0].destroy();
    other.terminate();
    expect(isProjectWebSocketPath('/__hotsheet/project-api/p/ws/sync?client=c')).toBe(true);
    expect(isProjectWebSocketPath('/vite-hmr')).toBe(false);
    expect(isProjectWebSocketPath(undefined)).toBe(false);
  });

  it('survives a browser resetting a project socket mid-resolve and leaves foreign sockets alone (HS2-D95TXJ)', async () => {
    const httpServer = new EventEmitter();
    let finishResolve: (target: string | undefined) => void = () => undefined;
    installProjectWebSocketBridge(
      { httpServer },
      () => undefined,
      () =>
        new Promise<string | undefined>((resolve) => {
          finishResolve = resolve;
        }),
    );
    const projectSocket = new PassThrough(),
      foreignSocket = new PassThrough();
    httpServer.emit('upgrade', { url: '/__hotsheet/project-api/p/ws/sync' }, projectSocket, Buffer.alloc(0));
    httpServer.emit('upgrade', { url: '/vite-hmr' }, foreignSocket, Buffer.alloc(0));
    // Node's HTTP server leaves upgraded sockets without an error listener; a reset while the target
    // is still resolving used to throw out of the dev server's event loop.
    expect(() =>
      projectSocket.emit('error', Object.assign(new Error('read ECONNRESET'), { code: 'ECONNRESET' })),
    ).not.toThrow();
    expect(projectSocket.destroyed).toBe(true);
    // A resolve that settles after the reset neither writes to nor upgrades the dead socket.
    finishResolve('ws://127.0.0.1:1/ws/sync');
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(foreignSocket.listenerCount('error')).toBe(0);
    foreignSocket.destroy();
  });

  it('forwards live project change frames without exposing the upstream secret', async () => {
    const upstreamHttp = createServer(),
      upstream = new WebSocketServer({ server: upstreamHttp });
    servers.push(upstream, upstreamHttp);
    let upstreamUrl = '';
    upstream.on('connection', (socket, request) => {
      upstreamUrl = request.url ?? '';
      socket.send(JSON.stringify({ cursor: 1, store: 'local', kind: 'created', id: '01', slug: 'HS2-ONE' }));
    });
    const upstreamPort = await listen(upstreamHttp),
      bridgeHttp = createServer();
    servers.push(bridgeHttp);
    installProjectWebSocketBridge(
      { httpServer: bridgeHttp },
      async () => undefined,
      () => `ws://127.0.0.1:${upstreamPort}/ws/sync?secret=server-only`,
    );
    const bridgePort = await listen(bridgeHttp),
      browserUrl = `ws://127.0.0.1:${bridgePort}/__hotsheet/project-api/project/ws/sync`;
    expect(browserUrl).not.toContain('secret');
    const client = new WebSocket(browserUrl);
    const [frame] = await once(client, 'message');
    expect(JSON.parse(frame.toString())).toMatchObject({ cursor: 1, kind: 'created' });
    expect(upstreamUrl).toBe('/ws/sync?secret=server-only');
    client.close();
    await once(client, 'close');
  });
});
