import { afterEach, describe, expect, it, vi } from 'vitest';

import type { Checkout } from './api';
import { changePollSessionHeaders, createDevApp, remoteRouteKind, remoteSessionGrants } from './dev-server';
import { createLocalHostApp } from './local-host';
import type { MigrationJobApi } from './migration-job-bridge';
import { openRegisteredProject, type ProjectSession, UnregisteredRemoteProjectError } from './project-bridge';

afterEach(() => vi.unstubAllEnvs());

describe('HS2-7DBJMS remote browser boundary', () => {
  it('denies unauthenticated project access and every local-only bridge route', async () => {
    vi.stubEnv('HOTSHEET_BROWSER_AUTH_MODE', 'remote');
    const app = createDevApp();
    expect((await app.request('/__hotsheet/checkouts')).status).toBe(401);
    expect((await app.request('/__hotsheet/project-api/synthetic/tickets')).status).toBe(401);
    expect((await app.request('/__hotsheet/folders/choose', { method: 'POST' })).status).toBe(404);
    // Device management needs a session first, then its admin grant (HS2-DXM0YN).
    expect((await app.request('/__hotsheet/remote-auth/owner')).status).toBe(401);
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
    // A live session without the local grant (HS2-85W6BE).
    const app = bridge(openRemote, async () =>
      Response.json({ origin: 'http://localhost', admin: true, local: false }),
    );
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

describe('HS2-9FWETT remote HS1 migration status', () => {
  const registered: Checkout[] = [
    { id: 'synthetic-a1b2', root: '/synthetic/registered', alias: 'registered', stores: [] },
  ];
  function app(remote: boolean) {
    if (remote) vi.stubEnv('HOTSHEET_BROWSER_AUTH_MODE', 'remote');
    const watch = vi.fn<MigrationJobApi['watch']>(async () => undefined);
    const jobs: MigrationJobApi = { start: vi.fn(), watch };
    const check = vi.fn<(headers: Headers) => Promise<Response>>(async () =>
      Response.json({ origin: 'https://synthetic-host.example.ts.net' }),
    );
    const before = Array.from({ length: 7 }, () => undefined) as [
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
    ];
    return {
      watch,
      app: createDevApp(true, ...before, async () => registered, jobs, undefined, undefined, undefined, check),
    };
  }
  const get = (target: ReturnType<typeof createDevApp>, root: string, headers: Record<string, string> = {}) =>
    target.request(`/__hotsheet/remote/projects/migration-jobs?root=${encodeURIComponent(root)}&after=2`, { headers });

  it('reads status only for a registered checkout', async () => {
    const { app: target, watch } = app(false);
    const read = await get(target, '/synthetic/registered');
    expect(read.status).toBe(200);
    expect(watch).toHaveBeenCalledWith('/synthetic/registered', 2, expect.any(AbortSignal));
    expect((await get(target, '/synthetic/elsewhere')).status).toBe(403);
    expect(watch).toHaveBeenCalledTimes(1);
  });

  it('is on the remote allowlist for a live session while the local status and start routes are not', async () => {
    const { app: target, watch } = app(true);
    const cookie = 'hs2_remote=synthetic-session';
    expect((await get(target, '/synthetic/registered')).status).toBe(401);
    expect((await get(target, '/synthetic/registered', { cookie })).status).toBe(200);
    expect(
      (
        await target.request('/__hotsheet/projects/migration-jobs?root=%2Fsynthetic%2Fregistered', {
          headers: { cookie },
        })
      ).status,
    ).toBe(404);
    expect(
      (await target.request('/__hotsheet/projects/migration-jobs', { method: 'POST', headers: { cookie } })).status,
    ).toBe(404);
    expect(watch).toHaveBeenCalledTimes(1);
  });
});

describe('HS2-85W6BE / HS2-DXM0YN session grants on a remote-mode bridge', () => {
  const ORIGIN = 'http://127.0.0.1:4175';
  const cookie = 'hs2_remote_session=synthetic-session; hs2_remote_csrf=synthetic-csrf';
  const grants = (device: Record<string, unknown>, ok = true) =>
    vi.fn<(headers: Headers) => Promise<Response>>(async () =>
      ok ? Response.json({ origin: ORIGIN, ...device }) : Response.json({ error: 'no' }, { status: 401 }),
    );
  function app(check: (headers: Headers) => Promise<Response>) {
    vi.stubEnv('HOTSHEET_BROWSER_AUTH_MODE', 'remote');
    const chooseFolder = vi.fn<() => Promise<string | undefined>>(async () => '/synthetic/chosen');
    // `chooseFolder` is the third injectable; the nine after it and `openRemote` keep their defaults.
    const defaults = Array.from({ length: 9 }, () => undefined) as [
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
    return { chooseFolder, app: createDevApp(true, undefined, chooseFolder, ...defaults, undefined, check) };
  }
  const choose = (target: ReturnType<typeof createDevApp>, headers: Record<string, string>) =>
    target.request(`${ORIGIN}/__hotsheet/folders/choose`, { method: 'POST', headers });

  it('classifies routes by the grant they need', () => {
    expect(remoteRouteKind('/__hotsheet/remote-auth/owner')).toBe('management');
    expect(remoteRouteKind('/__hotsheet/remote-auth/owner/changes')).toBe('management');
    for (const path of [
      '/__hotsheet/checkouts',
      '/__hotsheet/remote/projects/open',
      '/__hotsheet/project-api/synthetic/tickets',
      '/__hotsheet/remote-auth/me',
    ])
      expect(remoteRouteKind(path), path).toBe('session');
    for (const path of ['/__hotsheet/folders/choose', '/__hotsheet/projects/open', '/__hotsheet/projects/setup-git'])
      expect(remoteRouteKind(path), path).toBe('host');
  });

  it('reads only literal true grants from the session answer', () => {
    expect(remoteSessionGrants({ origin: ORIGIN, admin: true, local: true })).toEqual({
      origin: ORIGIN,
      admin: true,
      local: true,
    });
    expect(remoteSessionGrants({ admin: 'true', local: 1 })).toEqual({ admin: false, local: false });
    expect(remoteSessionGrants(undefined)).toEqual({ admin: false, local: false });
  });

  it('opens the folder chooser only for a session with the local grant, whatever the hostname', async () => {
    const headers = { cookie, origin: ORIGIN };
    for (const [reason, check, status] of [
      ['no session', grants({ local: true }, false), 404],
      ['plain session', grants({}), 404],
      ['admin without local', grants({ admin: true }), 404],
      ['local grant', grants({ local: true }), 200],
    ] as const) {
      const { app: target, chooseFolder } = app(check);
      const response = await choose(target, headers);
      expect(response.status, reason).toBe(status);
      expect(chooseFolder, reason).toHaveBeenCalledTimes(status === 200 ? 1 : 0);
    }
    // A host-trusted session still proves its paired Origin and CSRF token on every change.
    for (const [reason, headers] of [
      ['no origin', { cookie }],
      ['foreign origin', { cookie, origin: 'https://foreign.example' }],
      ['no CSRF cookie', { cookie: 'hs2_remote_session=synthetic-session', origin: ORIGIN }],
    ] as const) {
      const { app: target, chooseFolder } = app(grants({ local: true }));
      expect((await choose(target, headers)).status, reason).toBe(403);
      expect(chooseFolder, reason).not.toHaveBeenCalled();
    }
    // Without a cookie, a host route stays hidden and the session check never runs.
    const check = grants({ local: true });
    const { app: target } = app(check);
    expect((await choose(target, { origin: ORIGIN })).status).toBe(404);
    expect(check).not.toHaveBeenCalled();
  });

  it('HS2-HBM3JN admits only a local-grant session to the cross-server desktop registry', async () => {
    for (const path of ['/__hotsheet/remotes', '/__hotsheet/remotes/pair', '/__hotsheet/remotes/synthetic/checkouts'])
      expect(remoteRouteKind(path), path).toBe('host');
    // An address without a port fails before any network or CLI call, so the bridge answers with
    // its own readable JSON error. Before HS2-HBM3JN every remotes route was a plain-text 404 here.
    const pair = (target: ReturnType<typeof createDevApp>, headers: Record<string, string>) =>
      target.request(`${ORIGIN}/__hotsheet/remotes/pair`, {
        method: 'POST',
        headers: { 'content-type': 'application/json', ...headers },
        body: JSON.stringify({ address: 'https://synthetic.example', label: 'Synthetic' }),
      });
    const local = await pair(app(grants({ local: true })).app, { cookie, origin: ORIGIN });
    expect(local.status).toBe(400);
    expect(await local.json()).toEqual({
      error: 'Use an explicit protocol, host, and port without a path or credentials.',
    });
    // Without the local grant (even with admin), without a session, or without a valid one: hidden.
    for (const [reason, device, headers, ok] of [
      ['plain session', {}, { cookie, origin: ORIGIN }, true],
      ['admin only', { admin: true }, { cookie, origin: ORIGIN }, true],
      ['no cookie', { local: true }, { origin: ORIGIN }, true],
      ['rejected session', { local: true }, { cookie, origin: ORIGIN }, false],
    ] as const) {
      const response = await pair(app(grants(device, ok)).app, headers);
      expect(response.status, reason).toBe(404);
    }
    // A local session's change still proves its paired Origin.
    expect((await pair(app(grants({ local: true })).app, { cookie })).status).toBe(403);
    expect((await pair(app(grants({ local: true })).app, { cookie, origin: 'https://foreign.example' })).status).toBe(
      403,
    );
  });

  it('lets only an admin session reach device management', async () => {
    // An unknown management path passes the grant check and then 404s, without reaching the server.
    const path = `${ORIGIN}/__hotsheet/remote-auth/owner/unknown`;
    const plain = await app(grants({ local: true })).app.request(path, { headers: { cookie } });
    expect(plain.status).toBe(403);
    expect(await plain.json()).toEqual({ error: 'Administrator access required.' });
    expect((await app(grants({}, false)).app.request(path, { headers: { cookie } })).status).toBe(401);
    expect((await app(grants({ admin: true })).app.request(path, { headers: { cookie } })).status).toBe(404);
    const write = await app(grants({ admin: true })).app.request(path, {
      method: 'POST',
      headers: { cookie, origin: 'https://foreign.example' },
    });
    expect(write.status).toBe(403);
  });
});
