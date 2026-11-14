import { readdirSync, readFileSync } from 'node:fs';
import { createServer, type Server } from 'node:http';
import type { AddressInfo } from 'node:net';
import { join, resolve } from 'node:path';

import { Hono } from 'hono';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { WebSocket, WebSocketServer } from 'ws';

import { createLocalHostApp } from './local-host';
import {
  installPeerWebSocketGateway,
  isPeerGatewayPath,
  PEER_GATEWAY_PREFIXES,
  peerGateway,
  peerGatewayUrl,
} from './peer-gateway';

const SERVER = 'http://127.0.0.1:9';
const SERVER_SOURCE = resolve(__dirname, '../../../crates/hotsheet-server/src');
// Routes that only exist inside `#[cfg(test)]` modules of non-test source files.
const TEST_FIXTURE_ROUTES = new Set(['api', 'busy', 'fast']);

afterEach(() => vi.unstubAllEnvs());

function rustSources(directory: string): string[] {
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name);
    if (entry.isDirectory()) return rustSources(path);
    return entry.name.endsWith('.rs') && !/test/.test(entry.name) ? [path] : [];
  });
}

function host(forward: typeof fetch, resolveServer = async () => SERVER) {
  return createLocalHostApp('/nonexistent-synthetic-dist', new Hono(), undefined, peerGateway(resolveServer, forward));
}

describe('HS2-H4REBS peer gateway', () => {
  it('covers exactly the machine server router', () => {
    const routes = new Set<string>();
    for (const file of rustSources(SERVER_SOURCE))
      for (const match of readFileSync(file, 'utf8').matchAll(/\.route\(\s*"\/([^"/]*)/g))
        if (match[1] && !TEST_FIXTURE_ROUTES.has(match[1])) routes.add(match[1]);
    expect([...PEER_GATEWAY_PREFIXES].sort()).toEqual([...routes].sort());
  });

  it('recognizes only machine routes, never the application or the bridge', () => {
    for (const path of ['/remote-identity', '/remote-auth/pair', '/checkouts/a/tickets', '/ws/sync', '/tickets'])
      expect(isPeerGatewayPath(path), path).toBe(true);
    for (const path of ['/', '/index.html', '/assets/app.js', '/__hotsheet/remotes', '/ux-demo', '/remote-identityx'])
      expect(isPeerGatewayPath(path), path).toBe(false);
    expect(peerGatewayUrl(SERVER, '/ws/sync?secret=s&checkout=a&secret=t').toString()).toBe(
      `${SERVER}/ws/sync?checkout=a`,
    );
  });

  it('forwards a peer request on a remote-mode host without any machine secret', async () => {
    vi.stubEnv('HOTSHEET_BROWSER_AUTH_MODE', 'remote');
    const forward = vi.fn<typeof fetch>(
      async () =>
        new Response('{"server_id":"synthetic"}', {
          status: 201,
          headers: { 'content-type': 'application/json', 'set-cookie': 'hs2_remote_session=issued; HttpOnly' },
        }),
    );
    const response = await host(forward).request('https://peer.example.ts.net/remote-auth/redeem?secret=s&x=1', {
      method: 'POST',
      headers: {
        cookie: 'hs2_remote_session=synthetic',
        origin: 'http://127.0.0.1:4174',
        'x-hotsheet-csrf': 'synthetic-csrf',
        'x-hotsheet-secret': 'smuggled',
        authorization: 'Bearer smuggled',
        'content-type': 'application/json',
      },
      body: '{"id":"ABCD"}',
    });
    expect(response.status).toBe(201);
    expect(response.headers.get('set-cookie')).toBe('hs2_remote_session=issued; HttpOnly');
    expect(await response.json()).toEqual({ server_id: 'synthetic' });
    const [url, init] = forward.mock.calls[0] ?? [];
    expect(url instanceof URL ? url.href : url).toBe(`${SERVER}/remote-auth/redeem?x=1`);
    const headers = new Headers(init?.headers);
    expect(headers.get('x-hotsheet-secret')).toBeNull();
    expect(headers.get('authorization')).toBeNull();
    expect(headers.get('cookie')).toBe('hs2_remote_session=synthetic');
    expect(headers.get('origin')).toBe('http://127.0.0.1:4174');
    expect(headers.get('x-hotsheet-csrf')).toBe('synthetic-csrf');
    expect(new TextDecoder().decode(init?.body as ArrayBuffer)).toBe('{"id":"ABCD"}');
  });

  it('stays out of the way on a local-mode host and for application paths', async () => {
    const forward = vi.fn<typeof fetch>();
    expect((await host(forward).request('/remote-identity')).status).toBe(404);
    vi.stubEnv('HOTSHEET_BROWSER_AUTH_MODE', 'remote');
    for (const path of ['/', '/assets/missing.js', '/__hotsheet/remotes'])
      expect((await host(forward).request(path)).status, path).toBe(404);
    expect(forward).not.toHaveBeenCalled();
  });

  it('answers 503 when this host cannot reach its own machine server', async () => {
    vi.stubEnv('HOTSHEET_BROWSER_AUTH_MODE', 'remote');
    const unreachable = host(vi.fn<typeof fetch>(async () => Promise.reject(new Error('down'))));
    expect((await unreachable.request('/remote-identity')).status).toBe(503);
    const unresolved = host(vi.fn<typeof fetch>(), async () => Promise.reject(new Error('no server')));
    expect((await unresolved.request('/remote-identity')).status).toBe(503);
  });

  it('relays a peer change stream with its own session and never a secret', async () => {
    vi.stubEnv('HOTSHEET_BROWSER_AUTH_MODE', 'remote');
    const listen = (server: Server) =>
      new Promise<number>((done) => {
        server.listen(0, '127.0.0.1', () => {
          done((server.address() as AddressInfo).port);
        });
      });
    const machine = createServer();
    const upstream = new WebSocketServer({ server: machine });
    const seen = new Promise<{ url: string; cookie?: string; secret?: string }>((done) =>
      upstream.on('connection', (socket, request) => {
        socket.on('message', (data) => {
          socket.send(`echo:${(data as Buffer).toString()}`);
        });
        done({
          url: request.url ?? '',
          cookie: request.headers.cookie,
          secret: request.headers['x-hotsheet-secret'] as string | undefined,
        });
      }),
    );
    const machinePort = await listen(machine);
    const gatewayServer = createServer();
    installPeerWebSocketGateway(gatewayServer, async () => `http://127.0.0.1:${machinePort}`);
    const gatewayPort = await listen(gatewayServer);
    try {
      const caller = new WebSocket(`ws://127.0.0.1:${gatewayPort}/ws/sync?checkout=a&secret=smuggled`, {
        headers: { cookie: 'hs2_remote_session=synthetic', 'x-hotsheet-secret': 'smuggled' },
      });
      const reply = new Promise<string>((done) => {
        caller.on('message', (data) => {
          done((data as Buffer).toString());
        });
      });
      await new Promise((done) => caller.on('open', done));
      caller.send('hello');
      expect(await reply).toBe('echo:hello');
      expect(await seen).toEqual({
        url: '/ws/sync?checkout=a',
        cookie: 'hs2_remote_session=synthetic',
        secret: undefined,
      });
      caller.close();
    } finally {
      upstream.close();
      await new Promise((done) => gatewayServer.close(done));
      await new Promise((done) => machine.close(done));
    }
  });
});
