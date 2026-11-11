import { afterEach, describe, expect, it, vi } from 'vitest';

import type { Checkout } from './api';
import { createDevApp } from './dev-server';
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

  function bridge(openRemote: (root: string) => Promise<unknown>) {
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
    return createDevApp(true, ...defaults, openRemote);
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
