import { existsSync, readFileSync, statSync } from 'node:fs';
import { chmod, mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, resolve } from 'node:path';

import { Hono } from 'hono';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';

import {
  createLocalHostApp,
  mcpBinaryEnvironment,
  releaseBinaryEnvironment,
  snapshotLocalHostArtifacts,
} from './local-host';

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
    expect((await host.request('/ux-demo')).status).toBe(404);
    expect((await host.request('/ux-demo?component=tag-chip')).status).toBe(404);
    // A missing hashed asset is a 404, not the application document.
    expect((await host.request('/assets/missing.js')).status).not.toBe(200);
  });
});

describe('release binaries for the production host (HS2-D2JQ9A)', () => {
  const root = '/repo',
    release = (name: string) => `/repo/target/release/${name}`,
    debug = (name: string) => `/repo/target/debug/${name}`,
    built =
      (...names: string[]) =>
      (path: string) =>
        names.some((name) => path === release(name) || path === debug(name)),
    current = () => ({
      build_revision: 'source-sha256:current',
      source_revision: 'source-sha256:current',
      source_stale: false,
    });

  it('prefers current release binaries', () => {
    expect(
      releaseBinaryEnvironment(root, {}, built('hotsheet-server', 'hotsheet-cli', 'hotsheet-migrate'), current),
    ).toEqual({
      HOTSHEET_SERVER_BIN: release('hotsheet-server'),
      HOTSHEET_CLI_BIN: release('hotsheet-cli'),
      HOTSHEET_MIGRATE_BIN: release('hotsheet-migrate'),
      HOT_SHEET_BUILD_REVISION: 'source-sha256:current',
    });
  });

  it('falls back to current debug companions when release generations are stale', () => {
    expect(
      releaseBinaryEnvironment(root, {}, built('hotsheet-server', 'hotsheet-cli', 'hotsheet-migrate'), (path) =>
        path === release('hotsheet-cli') || path === release('hotsheet-migrate')
          ? { build_revision: 'old', source_revision: 'current', source_stale: true }
          : current(),
      ),
    ).toEqual({
      HOTSHEET_SERVER_BIN: release('hotsheet-server'),
      HOTSHEET_CLI_BIN: debug('hotsheet-cli'),
      HOTSHEET_MIGRATE_BIN: debug('hotsheet-migrate'),
      HOT_SHEET_BUILD_REVISION: 'source-sha256:current',
    });
  });

  it('uses current debug binaries when the release server is stale', () => {
    expect(
      releaseBinaryEnvironment(root, {}, built('hotsheet-server', 'hotsheet-cli', 'hotsheet-migrate'), (path) =>
        path === release('hotsheet-server')
          ? { build_revision: 'old', source_revision: 'current', source_stale: true }
          : current(),
      ),
    ).toEqual({
      HOTSHEET_SERVER_BIN: debug('hotsheet-server'),
      HOTSHEET_CLI_BIN: release('hotsheet-cli'),
      HOTSHEET_MIGRATE_BIN: release('hotsheet-migrate'),
      HOT_SHEET_BUILD_REVISION: 'source-sha256:current',
    });
  });

  it('rejects stale release and debug servers before opening a project', () => {
    expect(() =>
      releaseBinaryEnvironment(root, {}, built('hotsheet-server'), () => ({
        build_revision: 'old',
        source_revision: 'current',
        source_stale: true,
      })),
    ).toThrow('No current Hot Sheet hotsheet-server binary');
  });

  it('rejects a stale companion even when its server is current', () => {
    expect(() =>
      releaseBinaryEnvironment(root, {}, built('hotsheet-server', 'hotsheet-cli', 'hotsheet-migrate'), (path) =>
        path.includes('hotsheet-migrate') ? undefined : current(),
      ),
    ).toThrow('No current Hot Sheet hotsheet-migrate binary');
  });

  it('rejects a companion from a different current source snapshot', () => {
    expect(() =>
      releaseBinaryEnvironment(root, {}, built('hotsheet-server', 'hotsheet-cli', 'hotsheet-migrate'), (path) =>
        path.includes('hotsheet-cli')
          ? { build_revision: 'source-sha256:other', source_revision: 'source-sha256:other', source_stale: false }
          : current(),
      ),
    ).toThrow('No current Hot Sheet hotsheet-cli binary');
  });

  it('keeps explicit overrides and skips binaries that are not built', () => {
    expect(
      releaseBinaryEnvironment(
        root,
        { HOTSHEET_SERVER_BIN: '/custom/server' },
        built('hotsheet-cli', 'hotsheet-migrate'),
        current,
      ),
    ).toEqual({ HOTSHEET_CLI_BIN: release('hotsheet-cli'), HOTSHEET_MIGRATE_BIN: release('hotsheet-migrate') });
    expect(
      releaseBinaryEnvironment(
        root,
        { HOTSHEET_CLI_BIN: '/custom/cli' },
        built('hotsheet-server', 'hotsheet-migrate'),
        current,
      ),
    ).toEqual({
      HOTSHEET_SERVER_BIN: release('hotsheet-server'),
      HOTSHEET_MIGRATE_BIN: release('hotsheet-migrate'),
      HOT_SHEET_BUILD_REVISION: 'source-sha256:current',
    });
  });
});

describe('pinned production artifacts (HS2-6VQ5RV)', () => {
  it('serves and launches the selected build after working outputs change or disappear', async () => {
    const root = await mkdtemp(resolve(tmpdir(), 'hs-local-host-source-'));
    const dist = resolve(root, 'dist');
    const server = resolve(root, 'hotsheet-server');
    const cli = resolve(root, 'hotsheet-cli');
    let snapshot: ReturnType<typeof snapshotLocalHostArtifacts> | undefined;
    try {
      await mkdir(resolve(dist, 'assets'), { recursive: true });
      await writeFile(resolve(dist, 'index.html'), '<title>selected</title>');
      await writeFile(resolve(dist, 'assets', 'app.js'), 'selected build');
      await writeFile(server, 'selected server');
      await chmod(server, 0o755);
      await writeFile(cli, 'stable hook cli');
      await chmod(cli, 0o755);
      snapshot = snapshotLocalHostArtifacts(
        dist,
        {
          HOTSHEET_SERVER_BIN: server,
          HOTSHEET_CLI_BIN: cli,
          HOT_SHEET_BUILD_REVISION: 'selected',
        },
        resolve(root, 'home'),
      );
      const pinnedServer = snapshot.environment.HOTSHEET_SERVER_BIN;
      expect(snapshot.environment.HOTSHEET_CLI_BIN).not.toBe(cli);
      expect(snapshot.environment.HOTSHEET_HOOK_CLI_BIN).toBe(resolve(root, 'home/bin/hotsheet-cli'));
      expect(readFileSync(resolve(dirname(pinnedServer), '.stable-hook-cli'), 'utf8')).toBe(
        snapshot.environment.HOTSHEET_HOOK_CLI_BIN,
      );
      const host = createLocalHostApp(snapshot.distRoot, new Hono());

      await writeFile(resolve(dist, 'index.html'), '<title>new build</title>');
      await writeFile(resolve(dist, 'assets', 'app.js'), 'new build');
      await writeFile(server, 'new server');
      expect(await (await host.request('/')).text()).toContain('selected');
      expect(await (await host.request('/assets/app.js')).text()).toBe('selected build');
      expect(readFileSync(pinnedServer, 'utf8')).toBe('selected server');
      expect(statSync(pinnedServer).mode & 0o111).toBeTruthy();
      expect(snapshot.environment.HOT_SHEET_BUILD_REVISION).toBe('selected');
      expect(snapshot.environment.HOTSHEET_LOCAL_HOST_SNAPSHOT).toBe('1');

      await rm(dist, { recursive: true });
      await rm(server);
      await rm(cli);
      expect(await (await host.request('/')).text()).toContain('selected');
      expect(await (await host.request('/assets/app.js')).text()).toBe('selected build');
      expect(readFileSync(pinnedServer, 'utf8')).toBe('selected server');
      expect(readFileSync(snapshot.environment.HOTSHEET_HOOK_CLI_BIN, 'utf8')).toBe('stable hook cli');
    } finally {
      const pinnedRoot = snapshot?.distRoot,
        pinnedCli = snapshot?.environment.HOTSHEET_CLI_BIN;
      snapshot?.dispose();
      if (pinnedRoot) expect(existsSync(pinnedRoot)).toBe(false);
      if (pinnedCli) expect(readFileSync(pinnedCli, 'utf8')).toBe('stable hook cli');
      if (snapshot) expect(readFileSync(snapshot.environment.HOTSHEET_HOOK_CLI_BIN, 'utf8')).toBe('stable hook cli');
      if (pinnedRoot) await rm(resolve(pinnedRoot, '..'), { recursive: true, force: true });
      await rm(root, { recursive: true, force: true });
    }
  });

  it('keeps an old Codex hook command usable through a host restart', async () => {
    const root = await mkdtemp(resolve(tmpdir(), 'hs-hook-restart-')),
      dist = resolve(root, 'dist'),
      cli = resolve(root, 'hotsheet-cli'),
      home = resolve(root, 'home');
    let first: ReturnType<typeof snapshotLocalHostArtifacts> | undefined,
      second: ReturnType<typeof snapshotLocalHostArtifacts> | undefined;
    try {
      await mkdir(dist);
      await writeFile(resolve(dist, 'index.html'), 'host');
      await writeFile(cli, 'first cli');
      first = snapshotLocalHostArtifacts(dist, { HOTSHEET_CLI_BIN: cli }, home);
      const oldCommand = first.environment.HOTSHEET_CLI_BIN;
      first.dispose();
      expect(readFileSync(oldCommand, 'utf8')).toBe('first cli');

      await writeFile(cli, 'second cli');
      second = snapshotLocalHostArtifacts(dist, { HOTSHEET_CLI_BIN: cli }, home);
      expect(second.environment.HOTSHEET_HOOK_CLI_BIN).toBe(first.environment.HOTSHEET_HOOK_CLI_BIN);
      expect(readFileSync(oldCommand, 'utf8')).toBe('second cli');
    } finally {
      second?.dispose();
      for (const snapshot of [first, second]) {
        if (snapshot) await rm(resolve(snapshot.distRoot, '..'), { recursive: true, force: true });
      }
      await rm(root, { recursive: true, force: true });
    }
  });
});

describe('stable MCP shim for AI-tool configs (HS2-HNXE38)', () => {
  const root = '/repo',
    release = '/repo/target/release/hotsheet-mcp',
    debug = '/repo/target/debug/hotsheet-mcp',
    present =
      (...paths: string[]) =>
      (path: string) =>
        paths.includes(path);

  it('follows the selected server profile, then the other profile', () => {
    const both = present(release, debug);
    expect(mcpBinaryEnvironment(root, '/repo/target/release/hotsheet-server', {}, both)).toEqual({
      HOTSHEET_MCP_BIN: release,
    });
    expect(mcpBinaryEnvironment(root, '/repo/target/debug/hotsheet-server', {}, both)).toEqual({
      HOTSHEET_MCP_BIN: debug,
    });
    expect(mcpBinaryEnvironment(root, '/repo/target/release/hotsheet-server', {}, present(debug))).toEqual({
      HOTSHEET_MCP_BIN: debug,
    });
    expect(mcpBinaryEnvironment(root, undefined, {}, present(release))).toEqual({ HOTSHEET_MCP_BIN: release });
  });

  it('keeps an explicit override and selects nothing when no shim is built', () => {
    expect(mcpBinaryEnvironment(root, undefined, { HOTSHEET_MCP_BIN: '/custom/mcp' }, present(debug))).toEqual({});
    expect(mcpBinaryEnvironment(root, '/repo/target/debug/hotsheet-server', {}, present())).toEqual({});
  });
});
