import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { resolve } from 'node:path';

import { Hono } from 'hono';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';

import { createLocalHostApp, releaseBinaryEnvironment } from './local-host';

describe('local production host (HS2-587N4D)', () => {
  let dist: string;
  beforeAll(async () => {
    dist = await mkdtemp(resolve(tmpdir(), 'hs-local-host-'));
    await mkdir(resolve(dist, 'assets'));
    await writeFile(resolve(dist, 'index.html'), '<!doctype html><title>Hot Sheet</title>');
    await writeFile(resolve(dist, 'assets', 'app-abc123.js'), 'console.log(1)');
    await writeFile(resolve(dist, 'manifest.webmanifest'), '{}');
  });
  afterAll(async () => {
    await rm(dist, { recursive: true, force: true });
  });
  const app = () => {
    const bridge = new Hono();
    bridge.get('/__hotsheet/checkouts', (context) => context.json([{ id: 'one' }]));
    bridge.post('/__hotsheet/dev-review/tickets', (context) => context.json({ leaked: true }));
    bridge.get('/ux-demo', (context) => context.text('catalog'));
    return createLocalHostApp(dist, bridge);
  };

  it('routes the bridge, immutable assets, other static files, and client routes to the document', async () => {
    const host = app();
    expect(await (await host.request('/__hotsheet/checkouts')).json()).toEqual([{ id: 'one' }]);
    const asset = await host.request('/assets/app-abc123.js');
    expect(asset.status).toBe(200);
    expect(asset.headers.get('cache-control')).toBe('public, max-age=31536000, immutable');
    expect((await host.request('/manifest.webmanifest')).status).toBe(200);
    for (const path of ['/', '/some/client/route']) {
      const document = await host.request(path);
      expect(document.status).toBe(200);
      expect(await document.text()).toContain('<title>Hot Sheet</title>');
    }
    expect((await host.request('/some/client/route')).headers.get('cache-control')).toBe('no-cache');
  });

  it('never serves development-only surfaces, even when the bridge defines them', async () => {
    const host = app();
    expect((await host.request('/__hotsheet/dev-review/tickets', { method: 'POST' })).status).toBe(404);
    expect((await host.request('/ux-demo')).status).toBe(404);
    expect((await host.request('/ux-demo?component=tag-chip')).status).toBe(404);
    expect((await host.request('/__hotsheet/demo-modified')).status).toBe(404);
    // A missing hashed asset is a 404, not the application document.
    expect((await host.request('/assets/missing.js')).status).not.toBe(200);
  });
});

describe('release binaries for the production host (HS2-D2JQ9A)', () => {
  const root = '/repo',
    release = (name: string) => `/repo/target/release/${name}`,
    built =
      (...names: string[]) =>
      (path: string) =>
        names.some((name) => path === release(name));

  it('switches every binary to its release build together once a release server exists', () => {
    expect(releaseBinaryEnvironment(root, {}, built('hotsheet-server', 'hotsheet-cli', 'hotsheet-migrate'))).toEqual({
      HOTSHEET_SERVER_BIN: release('hotsheet-server'),
      HOTSHEET_CLI_BIN: release('hotsheet-cli'),
      HOTSHEET_MIGRATE_BIN: release('hotsheet-migrate'),
    });
  });

  it('never pairs a stale release CLI with the debug server', () => {
    expect(releaseBinaryEnvironment(root, {}, built('hotsheet-cli', 'hotsheet-migrate'))).toEqual({});
  });

  it('keeps explicit overrides and skips release binaries that are not built', () => {
    expect(releaseBinaryEnvironment(root, { HOTSHEET_SERVER_BIN: '/custom/server' }, built('hotsheet-server'))).toEqual(
      {},
    );
    expect(
      releaseBinaryEnvironment(root, { HOTSHEET_CLI_BIN: '/custom/cli' }, built('hotsheet-server', 'hotsheet-cli')),
    ).toEqual({ HOTSHEET_SERVER_BIN: release('hotsheet-server') });
  });
});
