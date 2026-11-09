import { once } from 'node:events';
import { existsSync, readFileSync, statSync } from 'node:fs';
import { chmod, mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, resolve } from 'node:path';

import { Hono } from 'hono';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import WebSocket from 'ws';

import { CHANGE_WS_MUX_PATH } from './change-ws-mux';
import { CLIENT_BUILD_META_NAME } from './client-build';
import {
  createLocalHostApp,
  mcpBinaryEnvironment,
  releaseBinaryEnvironment,
  snapshotLocalHostArtifacts,
  stampBrowserAuthMode,
  startLocalHost,
} from './local-host';
import { webSocketRawText } from './terminal-ws-bridge';

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
    // HS2-XXENE2: every path that returns the application document revalidates.
    for (const path of ['/', '/index.html', '/some/client/route']) {
      expect((await host.request(path)).headers.get('cache-control')).toBe('no-cache');
      expect((await host.request(path, { method: 'HEAD' })).headers.get('cache-control')).toBe('no-cache');
    }
    // Other static files keep their default caching; hashed assets stay immutable.
    expect((await host.request('/manifest.webmanifest')).headers.get('cache-control')).toBeNull();
  });

  it('never serves development-only surfaces, even when the bridge defines them', async () => {
    const host = app();
    expect((await host.request('/ux-demo')).status).toBe(404);
    expect((await host.request('/ux-demo?component=tag-chip')).status).toBe(404);
    // A missing hashed asset is a 404, not the application document.
    expect((await host.request('/assets/missing.js')).status).not.toBe(200);
  });
});

describe('production browser auth mode (HS2-7DBJMS)', () => {
  it('stamps the running host mode into the same built document', () => {
    const built = '<!doctype html><head><meta name="hotsheet-browser-auth" content="local"></head>';
    expect(stampBrowserAuthMode(built, true)).toContain('name="hotsheet-browser-auth" content="remote"');
    expect(stampBrowserAuthMode(built, false)).toContain('name="hotsheet-browser-auth" content="local"');
    expect(stampBrowserAuthMode(built, true).match(/name="hotsheet-browser-auth"/g)).toHaveLength(1);
    const backToLocal = stampBrowserAuthMode(stampBrowserAuthMode(built, true), false);
    expect(backToLocal).toBe(built);
  });
});

describe('local host client build announcement (HS2-KPJWBK)', () => {
  let dist: string, home: string;
  const previousHome = process.env.HOTSHEET_HOME;
  beforeAll(async () => {
    dist = await mkdtemp(resolve(tmpdir(), 'hs-local-host-build-'));
    // Client discovery writes under HOTSHEET_HOME; never touch the developer's real home.
    home = await mkdtemp(resolve(tmpdir(), 'hs-local-host-home-'));
    process.env.HOTSHEET_HOME = home;
  });
  afterAll(async () => {
    if (previousHome === undefined) delete process.env.HOTSHEET_HOME;
    else process.env.HOTSHEET_HOME = previousHome;
    await rm(dist, { recursive: true, force: true });
    await rm(home, { recursive: true, force: true });
  });
  const stamp = (id: string) =>
    writeFile(
      resolve(dist, 'index.html'),
      `<!doctype html><head><meta name="${CLIENT_BUILD_META_NAME}" content="${id}"></head>`,
    );
  /** Start a real host, subscribe once on its change bridge, and return the announced build. */
  const announcedAfterStart = async (): Promise<unknown> => {
    const server = await startLocalHost({ port: 0, distRoot: dist });
    try {
      const address = server.address();
      if (!address || typeof address === 'string') throw new Error('missing host address');
      const browser = new WebSocket(`ws://127.0.0.1:${address.port}${CHANGE_WS_MUX_PATH}`);
      await once(browser, 'open');
      const answer = once(browser, 'message');
      browser.send(JSON.stringify({ version: 2, type: 'subscribe', project: 'gone', client: 'tab', subscription: 1 }));
      const [data] = (await answer) as [WebSocket.RawData];
      browser.close();
      await once(browser, 'close');
      return (JSON.parse(webSocketRawText(data)) as { client_build?: unknown }).client_build;
    } finally {
      server.closeAllConnections();
      await new Promise((resolveClose) => server.close(resolveClose));
    }
  };

  it('announces the pinned dist build, and a restart onto a newer dist announces the new one', async () => {
    await stamp('bundle-first');
    expect(await announcedAfterStart()).toBe('bundle-first');
    // Restarting onto the same dist announces the same build: an open page raises nothing.
    expect(await announcedAfterStart()).toBe('bundle-first');
    await stamp('bundle-second');
    expect(await announcedAfterStart()).toBe('bundle-second');
    // An unstamped dist (an older build) announces nothing.
    await writeFile(resolve(dist, 'index.html'), '<!doctype html><title>Hot Sheet</title>');
    expect(await announcedAfterStart()).toBeUndefined();
  });
});

describe('release binaries for the production host (HS2-D2JQ9A)', () => {
  const root = resolve(tmpdir(), 'synthetic-repo'),
    executable = (name: string, platform = process.platform) => `${name}${platform === 'win32' ? '.exe' : ''}`,
    release = (name: string, platform = process.platform) =>
      resolve(root, 'target/release', executable(name, platform)),
    debug = (name: string, platform = process.platform) => resolve(root, 'target/debug', executable(name, platform)),
    built =
      (...names: string[]) =>
      (path: string) =>
        names.some((name) => path === release(name) || path === debug(name)),
    current = () => ({
      build_revision: 'source-sha256:current',
      source_revision: 'source-sha256:current',
      source_stale: false,
    });

  it('selects only matching Windows executables from one source revision (HS2-NRYAXT)', () => {
    const names = ['hotsheet-server', 'hotsheet-cli', 'hotsheet-migrate'];
    const windowsBinaries = new Set(names.map((name) => debug(name, 'win32')));
    const selected = releaseBinaryEnvironment(root, {}, (path) => windowsBinaries.has(path), current, 'win32');
    expect(selected).toEqual({
      HOTSHEET_SERVER_BIN: debug('hotsheet-server', 'win32'),
      HOTSHEET_CLI_BIN: debug('hotsheet-cli', 'win32'),
      HOTSHEET_MIGRATE_BIN: debug('hotsheet-migrate', 'win32'),
      HOT_SHEET_BUILD_REVISION: 'source-sha256:current',
    });
    expect(() =>
      releaseBinaryEnvironment(
        root,
        {},
        (path) => windowsBinaries.has(path),
        (path) =>
          path === debug('hotsheet-migrate', 'win32')
            ? { build_revision: 'source-sha256:other', source_revision: 'source-sha256:other', source_stale: false }
            : current(),
        'win32',
      ),
    ).toThrow('No current Hot Sheet hotsheet-migrate binary');
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
  it('HS2-FYXRRM signs the copied server and both CLI copies without touching build outputs', async () => {
    const root = await mkdtemp(resolve(tmpdir(), 'hs-signing-snapshot-')),
      dist = resolve(root, 'dist'),
      server = resolve(root, 'hotsheet-server'),
      cli = resolve(root, 'hotsheet-cli'),
      home = resolve(root, 'home');
    let snapshot: ReturnType<typeof snapshotLocalHostArtifacts> | undefined;
    try {
      await mkdir(dist);
      await writeFile(resolve(dist, 'index.html'), 'host');
      await writeFile(server, 'server build');
      await writeFile(cli, 'cli build');
      const calls: [string, string, string][] = [];
      snapshot = snapshotLocalHostArtifacts(
        dist,
        { HOTSHEET_SERVER_BIN: server, HOTSHEET_CLI_BIN: cli },
        home,
        (source, copy, name) => {
          calls.push([source, copy, name]);
        },
      );
      expect(calls.map(([source, , name]) => [source, name])).toEqual([
        [server, 'hotsheet-server'],
        [cli, 'hotsheet-cli'],
        [cli, 'hotsheet-cli'],
      ]);
      expect(calls[0]?.[1]).toBe(snapshot.environment.HOTSHEET_SERVER_BIN);
      expect(calls[1]?.[1]).toBe(snapshot.environment.HOTSHEET_CLI_BIN);
      expect(calls[2]?.[1]).not.toBe(cli);
      expect(readFileSync(server, 'utf8')).toBe('server build');
      expect(readFileSync(cli, 'utf8')).toBe('cli build');
    } finally {
      snapshot?.dispose();
      if (snapshot) await rm(resolve(snapshot.distRoot, '..'), { recursive: true, force: true });
      await rm(root, { recursive: true, force: true });
    }
  });

  it('keeps Windows executable names in pinned and stable copies (HS2-NRYAXT)', async () => {
    const root = await mkdtemp(resolve(tmpdir(), 'hs-windows-snapshot-')),
      dist = resolve(root, 'dist'),
      server = resolve(root, 'hotsheet-server.exe'),
      cli = resolve(root, 'hotsheet-cli.exe'),
      migrator = resolve(root, 'hotsheet-migrate.exe');
    let snapshot: ReturnType<typeof snapshotLocalHostArtifacts> | undefined;
    try {
      await mkdir(dist);
      await writeFile(resolve(dist, 'index.html'), 'host');
      for (const binary of [server, cli, migrator]) await writeFile(binary, 'current fixture');
      snapshot = snapshotLocalHostArtifacts(
        dist,
        { HOTSHEET_SERVER_BIN: server, HOTSHEET_CLI_BIN: cli, HOTSHEET_MIGRATE_BIN: migrator },
        resolve(root, 'home'),
        () => {},
        'win32',
      );
      for (const [variable, name] of [
        ['HOTSHEET_SERVER_BIN', 'hotsheet-server.exe'],
        ['HOTSHEET_CLI_BIN', 'hotsheet-cli.exe'],
        ['HOTSHEET_MIGRATE_BIN', 'hotsheet-migrate.exe'],
      ] as const) {
        expect(snapshot.environment[variable]).toBe(resolve(dirname(snapshot.distRoot), 'bin', name));
        expect(readFileSync(snapshot.environment[variable], 'utf8')).toBe('current fixture');
      }
      expect(snapshot.environment.HOTSHEET_HOOK_CLI_BIN).toBe(resolve(root, 'home/bin/hotsheet-cli.exe'));
    } finally {
      snapshot?.dispose();
      if (snapshot) await rm(dirname(snapshot.distRoot), { recursive: true, force: true });
      await rm(root, { recursive: true, force: true });
    }
  });

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
      expect(snapshot.environment.HOTSHEET_HOOK_CLI_BIN).toBe(
        resolve(root, 'home/bin', `hotsheet-cli${process.platform === 'win32' ? '.exe' : ''}`),
      );
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
  const root = resolve(tmpdir(), 'synthetic-repo'),
    release = resolve(root, 'target/release', `hotsheet-mcp${process.platform === 'win32' ? '.exe' : ''}`),
    debug = resolve(root, 'target/debug', `hotsheet-mcp${process.platform === 'win32' ? '.exe' : ''}`),
    server = (profile: 'release' | 'debug') =>
      resolve(root, 'target', profile, `hotsheet-server${process.platform === 'win32' ? '.exe' : ''}`),
    present =
      (...paths: string[]) =>
      (path: string) =>
        paths.includes(path);

  it('follows the selected server profile, then the other profile', () => {
    const both = present(release, debug);
    expect(mcpBinaryEnvironment(root, server('release'), {}, both)).toEqual({
      HOTSHEET_MCP_BIN: release,
    });
    expect(mcpBinaryEnvironment(root, server('debug'), {}, both)).toEqual({
      HOTSHEET_MCP_BIN: debug,
    });
    expect(mcpBinaryEnvironment(root, server('release'), {}, present(debug))).toEqual({
      HOTSHEET_MCP_BIN: debug,
    });
    expect(mcpBinaryEnvironment(root, undefined, {}, present(release))).toEqual({ HOTSHEET_MCP_BIN: release });
  });

  it('keeps an explicit override and selects nothing when no shim is built', () => {
    expect(mcpBinaryEnvironment(root, undefined, { HOTSHEET_MCP_BIN: '/custom/mcp' }, present(debug))).toEqual({});
    expect(mcpBinaryEnvironment(root, server('debug'), {}, present())).toEqual({});
  });

  it('keeps the native executable extension when a Windows server selects its MCP shim (HS2-NRYAXT)', () => {
    const releaseExe = resolve(root, 'target/release/hotsheet-mcp.exe');
    expect(
      mcpBinaryEnvironment(root, 'C:\\repo\\target\\release\\hotsheet-server.exe', {}, present(releaseExe), 'win32'),
    ).toEqual({ HOTSHEET_MCP_BIN: releaseExe });
  });
});
