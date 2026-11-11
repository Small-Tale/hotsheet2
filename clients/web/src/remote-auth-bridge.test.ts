import { afterEach, describe, expect, it, vi } from 'vitest';

import type { Checkout } from './api';
import { changePollSessionHeaders, createDevApp } from './dev-server';
import { createLocalHostApp } from './local-host';
import { openRegisteredProject, type ProjectSession, UnregisteredRemoteProjectError } from './project-bridge';

afterEach(() => vi.unstubAllEnvs());

describe('HS2-7DBJMS remote browser boundary', () => {
  it('denies unauthenticated project access and every local-only bridge route', async () => {
    vi.stubEnv('HOTSHEET_BROWSER_AUTH_MODE', 'remote');
    const app = createDevApp();
    expect((await app.request('/__hotsheet/checkouts')).status).toBe(401);
    expect((await app.request('/__hotsheet/project-api/synthetic/tickets')).status).toBe(401);
    expect((await app.request('/__hotsheet/folders/choose', { method: 'POST' })).status).toBe(404);
    expect((await app.request('/__hotsheet/remote-auth/owner')).status).toBe(403);
    expect(
      (
        await app.request('/__hotsheet/project-api/synthetic/tickets', {
          headers: { 'x-hotsheet-secret': 'spoofed' },
        })
      ).status,
    ).toBe(401);
  });

  it('keeps production desktop storage out of remote mode', async () => {
    vi.stubEnv('HOTSHEET_BROWSER_AUTH_MODE', 'remote');
    const app = createLocalHostApp('/nonexistent-synthetic-dist', createDevApp(), '/nonexistent-synthetic-storage');
    expect((await app.request('/__hotsheet/desktop-storage')).status).toBe(404);
  });
});

describe('HS2-3HC9HT remote project open', () => {
  const checkouts: Checkout[] = [
    { id: 'synthetic-a1b2', root: '/synthetic/registered', alias: 'registered', stores: ['/synthetic/store'] },
  ];
  const session = (root: string) => ({ id: 'synthetic-a1b2', root }) as unknown as ProjectSession;
  function opener() {
    const open = vi.fn<(root: string, store: string | undefined) => Promise<ProjectSession>>(async (root) =>
      session(root),
    );
    const linked = vi.fn<(root: string) => Promise<string | undefined>>(async () => '/synthetic/linked-store');
    return {
      open,
      linked,
      run: (root: string) => openRegisteredProject(root, async () => checkouts, open, linked),
    };
  }

  it('opens a registered checkout by root or id with its own linked store', async () => {
    for (const wanted of ['/synthetic/registered', ' synthetic-a1b2 ']) {
      const { open, linked, run } = opener();
      expect(await run(wanted)).toEqual(session('/synthetic/registered'));
      expect(linked).toHaveBeenCalledWith('/synthetic/registered');
      expect(open).toHaveBeenCalledWith('/synthetic/registered', '/synthetic/linked-store');
    }
  });

  it('refuses an unregistered or empty root without opening anything', async () => {
    for (const wanted of ['/synthetic/elsewhere', '/synthetic', '', '   ']) {
      const { open, run } = opener();
      await expect(run(wanted)).rejects.toBeInstanceOf(UnregisteredRemoteProjectError);
      expect(open).not.toHaveBeenCalled();
    }
  });

  function bridge(
    openRemote: (root: string) => Promise<unknown>,
    checkRemoteSession?: (headers: Headers) => Promise<Response>,
  ) {
    // Every injectable between `dev` and `openRemote` keeps its default.
    const defaults = Array.from({ length: 11 }, () => undefined) as [
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
    ];
    return createDevApp(true, ...defaults, openRemote, checkRemoteSession);
  }
  const post = (app: ReturnType<typeof createDevApp>, body: string) =>
    app.request('/__hotsheet/remote/projects/open', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body,
    });

  it('answers 201 for a registered root, 403 for an unregistered one, and 400 for a malformed body', async () => {
    const app = bridge(async (root) => {
      if (root !== '/synthetic/registered') throw new UnregisteredRemoteProjectError();
      return { id: 'synthetic-a1b2', root };
    });
    const opened = await post(app, JSON.stringify({ root: '/synthetic/registered' }));
    expect(opened.status).toBe(201);
    expect(await opened.json()).toEqual({ id: 'synthetic-a1b2', root: '/synthetic/registered' });
    const refused = await post(app, JSON.stringify({ root: '/synthetic/elsewhere' }));
    expect(refused.status).toBe(403);
    expect(await refused.json()).toEqual({ error: new UnregisteredRemoteProjectError().message });
    for (const body of ['not json', '{}', JSON.stringify({ root: 7 }), JSON.stringify({ root: ' ' })])
      expect((await post(app, body)).status).toBe(400);
  });

  it('requires a remote session and keeps the local open route off the remote allowlist', async () => {
    vi.stubEnv('HOTSHEET_BROWSER_AUTH_MODE', 'remote');
    const openRemote = vi.fn(async () => ({}));
    const app = bridge(openRemote);
    expect((await post(app, JSON.stringify({ root: '/synthetic/registered' }))).status).toBe(401);
    const local = await app.request('/__hotsheet/projects/open', {
      method: 'POST',
      headers: { 'content-type': 'application/json', cookie: 'hs2_remote=synthetic' },
      body: JSON.stringify({ root: '/synthetic/registered' }),
    });
    expect(local.status).toBe(404);
    expect(openRemote).not.toHaveBeenCalled();
  });
});

describe('HS2-CD8YNK remote writes behind a TLS-terminating proxy', () => {
  const PAIRED = 'https://synthetic-host.example.ts.net';
  // Tailscale Serve hands the host a plain-HTTP request for the browser's https page.
  const PROXIED = 'http://synthetic-host.example.ts.net/__hotsheet/remote/projects/open';
  const session = (device: unknown, ok = true) =>
    vi.fn<(headers: Headers) => Promise<Response>>(async () =>
      ok ? Response.json(device) : Response.json({ error: 'no' }, { status: 401 }),
    );
  function app(check: (headers: Headers) => Promise<Response>) {
    vi.stubEnv('HOTSHEET_BROWSER_AUTH_MODE', 'remote');
    const openRemote = vi.fn<(root: string) => Promise<unknown>>(async () => ({ id: 'synthetic' }));
    const defaults = Array.from({ length: 11 }, () => undefined) as [
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
    ];
    return { openRemote, app: createDevApp(true, ...defaults, openRemote, check) };
  }
  const post = (target: ReturnType<typeof createDevApp>, headers: Record<string, string>) =>
    target.request(PROXIED, {
      method: 'POST',
      headers: { 'content-type': 'application/json', ...headers },
      body: JSON.stringify({ root: '/synthetic/registered' }),
    });
  const cookie = 'hs2_remote=synthetic-session; hs2_remote_csrf=synthetic-csrf';

  it('accepts the paired https origin although the proxied request URL is http', async () => {
    const { app: target, openRemote } = app(session({ id: 'device', origin: PAIRED }));
    const response = await post(target, { cookie, origin: PAIRED });
    expect(response.status).toBe(201);
    expect(openRemote).toHaveBeenCalledWith('/synthetic/registered');
  });

  it('refuses a write whose origin, CSRF cookie, or paired origin is missing or different', async () => {
    for (const [reason, check, headers] of [
      ['foreign origin', session({ origin: PAIRED }), { cookie, origin: 'https://foreign.example' }],
      ['request-URL origin', session({ origin: PAIRED }), { cookie, origin: 'http://synthetic-host.example.ts.net' }],
      ['no origin', session({ origin: PAIRED }), { cookie }],
      ['no CSRF cookie', session({ origin: PAIRED }), { cookie: 'hs2_remote=synthetic-session', origin: PAIRED }],
      ['no paired origin', session({ id: 'device' }), { cookie, origin: PAIRED }],
    ] as const) {
      const { app: target, openRemote } = app(check);
      expect((await post(target, headers)).status, reason).toBe(403);
      expect(openRemote, reason).not.toHaveBeenCalled();
    }
  });

  it('requires a live session before any origin check', async () => {
    const { app: target, openRemote } = app(session({}, false));
    expect((await post(target, { cookie, origin: PAIRED })).status).toBe(401);
    expect(openRemote).not.toHaveBeenCalled();
  });
});

describe('HS2-ZVSMTZ remote change-poll multiplexer', () => {
  const PAIRED = 'https://synthetic-host.example.ts.net';
  const cookie = 'hs2_remote=synthetic-session; hs2_remote_csrf=synthetic-csrf';
  function app(ok = true) {
    vi.stubEnv('HOTSHEET_BROWSER_AUTH_MODE', 'remote');
    const forward = vi.fn<(project: string, path: string, signal: AbortSignal, browser: Headers) => Promise<Response>>(
      async () => Response.json({ events: [], cursor: 3 }),
    );
    const check = vi.fn<(headers: Headers) => Promise<Response>>(async () =>
      ok ? Response.json({ origin: PAIRED }) : Response.json({ error: 'no' }, { status: 401 }),
    );
    const defaults = Array.from({ length: 10 }, () => undefined) as [
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
    ];
    return { forward, app: createDevApp(true, ...defaults, forward, undefined, check) };
  }
  const poll = (target: ReturnType<typeof createDevApp>, headers: Record<string, string>) =>
    target.request('http://synthetic-host.example.ts.net/__hotsheet/change-mux/poll', {
      method: 'POST',
      headers: { 'content-type': 'application/json', ...headers },
      body: JSON.stringify({
        version: 1,
        client: 'synthetic-client',
        polls: [{ project: 'synthetic-a1b2', timeout_ms: 0 }],
      }),
    });

  it('forwards each upstream poll with the browser session cookie only', async () => {
    const { app: target, forward } = app();
    const response = await poll(target, { cookie, origin: PAIRED, 'x-hotsheet-secret': 'spoofed' });
    expect(response.status).toBe(200);
    expect(await response.json()).toMatchObject({ results: [{ project: 'synthetic-a1b2', status: 200 }] });
    expect(forward).toHaveBeenCalledTimes(1);
    const browser = forward.mock.calls[0][3];
    expect(changePollSessionHeaders(browser).get('cookie')).toBe(cookie);
    expect([...changePollSessionHeaders(browser).keys()]).toEqual(['cookie']);
  });

  it('refuses an unauthenticated, revoked, or cross-origin multiplexed poll', async () => {
    for (const [reason, ok, headers, status] of [
      ['no cookie', true, { origin: PAIRED }, 401],
      ['revoked session', false, { cookie, origin: PAIRED }, 401],
      ['foreign origin', true, { cookie, origin: 'https://foreign.example' }, 403],
    ] as const) {
      const { app: target, forward } = app(ok);
      expect((await poll(target, headers)).status, reason).toBe(status);
      expect(forward, reason).not.toHaveBeenCalled();
    }
  });

  it('builds no session headers for a browser without a cookie', () => {
    expect([...changePollSessionHeaders(new Headers({ origin: PAIRED })).keys()]).toEqual([]);
  });
});
