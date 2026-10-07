import { chmod, mkdir, mkdtemp, readdir, readFile, realpath, rm, stat, symlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { resolve } from 'node:path';

import { describe, expect, it, vi } from 'vitest';

import { createDevApp } from './dev-server';
import {
  authenticatedServerUrl,
  authenticatedTerminalWebSocketUrl,
  chooseLocalFolder,
  connectGitTicketStoreRemote,
  createLocalGitTicketStore,
  createServerHealthProbe,
  deliverProjectSetupNotice,
  describeGitRemoteFailure,
  developmentRepositoryRoot,
  developmentSetupAssetsFingerprint,
  folderChooserCommand,
  forgetVerifiedServer,
  hasHs1Registrations,
  hs1ChannelSlug,
  hs1MigrationArgs,
  linkedTicketStore,
  listServerCheckouts,
  localStoreInitArgs,
  preserveHs1Entry,
  projectBootstrapArgs,
  projectScopedServerPath,
  projectServerPlan,
  projectSessionRegistry,
  reconcileHs1Registrations,
  recoverUnhealthyServer,
  refreshLocalProjectSetup,
  refreshProjectSetupOnOpen,
  removeHs1LiveData,
  requireCompatibleServer,
  requireCurrentSetupAssets,
  requireReportedCorruptPath,
  requireStoreSchemaCompatibility,
  revealCommand,
  runGitCommand,
  safelyRestartServer,
  setupRefreshNotices,
  storeNeedsServerUpgrade,
  superviseServer,
} from './project-bridge';

describe('linked migration destination', () => {
  it('prefers the current durable link, supports legacy links, and fails closed for a broken selected store', async () => {
    const root = await realpath(await mkdtemp(resolve(tmpdir(), 'linked-migration-')));
    try {
      expect(await linkedTicketStore(root)).toBeUndefined();
      await mkdir(resolve(root, '.hotsheet2'));
      await mkdir(resolve(root, '.hotsheet'));
      for (const store of ['custom.hs2', 'legacy.hs2']) {
        await mkdir(resolve(root, store));
        await writeFile(resolve(root, store, 'hotsheet-store.json'), '{}');
      }
      await writeFile(resolve(root, '.hotsheet/store'), `${root}/legacy.hs2\n`);
      expect(await linkedTicketStore(root)).toBe(resolve(root, 'legacy.hs2'));
      await writeFile(resolve(root, '.hotsheet2/store'), `${root}/custom.hs2\n`);
      expect(await linkedTicketStore(root)).toBe(resolve(root, 'custom.hs2'));
      await rm(resolve(root, 'custom.hs2/hotsheet-store.json'));
      await expect(linkedTicketStore(root)).rejects.toThrow(/linked ticket repository/);
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });
});

describe('projectSessionRegistry', () => {
  it('shares project sessions across separately evaluated Vite module graphs', async () => {
    const moduleUrl = new URL('./project-bridge.ts', import.meta.url).href;
    const configGraph = await import(`${moduleUrl}?graph=config`),
      ssrGraph = await import(`${moduleUrl}?graph=ssr`);
    configGraph.projectSessionRegistry().set('module-graph-checkout', { url: 'http://127.0.0.1:1', secret: 'private' });
    await expect(ssrGraph.projectTerminalWebSocketUrl('module-graph-checkout', 'terminal')).resolves.toBe(
      'ws://127.0.0.1:1/terminals/terminal/attach?secret=private',
    );
    await expect(ssrGraph.projectChangeWebSocketUrl('module-graph-checkout')).resolves.toBe(
      'ws://127.0.0.1:1/ws/sync?secret=private&checkout=module-graph-checkout',
    );
  });
});

describe('developmentRepositoryRoot', () => {
  it('uses the explicit original repository root inside a stable snapshot', () => {
    expect(
      developmentRepositoryRoot('/tmp/hotsheet-web-stable-123', {
        HOTSHEET_REPO_ROOT: '/work/hotsheet2',
      }),
    ).toBe('/work/hotsheet2');
  });

  it('retains the normal clients/web fallback for hot development', () => {
    expect(developmentRepositoryRoot('/work/hotsheet2/clients/web', {})).toBe('/work/hotsheet2');
  });
});

describe('project setup compatibility', () => {
  const cli = { generation: 'hs2', store_schema: { min: 1, max: 3, creates: 3 }, selected_store_schema: 3 };
  const oldServer = { generation: 'hs2', protocol: { min: 1, max: 1 }, store_schema: { min: 1, max: 2 } };
  it('rejects create and open before a newer store crosses an older server boundary', () => {
    expect(() => {
      requireStoreSchemaCompatibility(oldServer, cli, 'create');
    }).toThrow(/No ticket repository was created.*supports ticket-store schema through 2.*creates schema 3/i);
    expect(() => {
      requireStoreSchemaCompatibility(oldServer, cli, 'open');
    }).toThrow(/cannot be opened through that server.*found schema 3/i);
    expect(() => {
      requireStoreSchemaCompatibility({ ...oldServer, store_schema: { min: 1, max: 3 } }, cli, 'open');
    }).not.toThrow();
  });
  it('exposes the same setup refresh as a non-graphical CLI operation', async () => {
    const fingerprint = await developmentSetupAssetsFingerprint(),
      runner = vi
        .fn()
        .mockResolvedValueOnce(JSON.stringify({ ...cli, setup_assets_fingerprint: fingerprint }))
        .mockResolvedValueOnce(JSON.stringify({ set_up: [], removed: [] }));
    await refreshLocalProjectSetup('/work/code', '/work/tickets.hs2', runner);
    expect(runner).toHaveBeenNthCalledWith(
      1,
      expect.stringContaining('hotsheet-cli'),
      ['-C', '/work/tickets.hs2', 'compatibility', '--json'],
      expect.any(String),
    );
    expect(runner).toHaveBeenNthCalledWith(
      2,
      expect.stringContaining('hotsheet-cli'),
      ['-C', '/work/tickets.hs2', 'setup', '--refresh', '--project', '/work/code', '--json'],
      expect.any(String),
    );
  });
  it('opens a project even when its setup refresh is refused, reporting why (HS2-0TXM8S)', async () => {
    const refreshed: string[] = [];
    expect(
      await refreshProjectSetupOnOpen('/work/code', '/work/tickets.hs2', (root, store) => {
        refreshed.push(`${root}|${store}`);
        return Promise.resolve(undefined);
      }),
    ).toEqual({});
    expect(refreshed).toEqual(['/work/code|/work/tickets.hs2']);
    // The stale-CLI guard still refuses to write, but opening continues with its explanation.
    const staleRunner = vi.fn().mockResolvedValue(JSON.stringify(cli));
    const warning = await refreshProjectSetupOnOpen('/work/code', '/work/tickets.hs2', (root, store) =>
      refreshLocalProjectSetup(root, store, staleRunner),
    );
    expect(warning.warning).toMatch(/does not report.*cargo build -p hotsheet-cli.*No setup files were changed/i);
    expect(staleRunner).not.toHaveBeenCalledWith(
      expect.anything(),
      expect.arrayContaining(['setup']),
      expect.anything(),
    );
    expect(
      await refreshProjectSetupOnOpen('/work/code', '/work/tickets.hs2', () =>
        Promise.reject(new Error('plain failure')),
      ),
    ).toEqual({ warning: 'plain failure' });
  });
  it('carries changed-hook notices separately from failures and ignores no-op or unrelated notices (HS2-4AR09Z)', async () => {
    const changed = {
      set_up: [{ tool: 'Codex', notices: [{ kind: 'codex_hooks_changed', path: '.codex/hooks.json' }] }],
      removed: [],
    };
    expect(setupRefreshNotices(changed)).toEqual({ codexHooksChanged: '.codex/hooks.json' });
    expect(setupRefreshNotices({ set_up: [{ tool: 'Codex', wrote: ['.codex/hooks.json'] }] })).toEqual({});
    expect(
      setupRefreshNotices({
        set_up: [
          {
            notices: [
              { kind: 'future_notice', path: 'other' },
              { kind: 'codex_hooks_changed', path: '' },
            ],
          },
        ],
      }),
    ).toEqual({});
    expect(() => setupRefreshNotices(null)).toThrow('invalid setup report');
    const fingerprint = await developmentSetupAssetsFingerprint();
    const runner = vi
      .fn()
      .mockResolvedValueOnce(JSON.stringify({ ...cli, setup_assets_fingerprint: fingerprint }))
      .mockResolvedValueOnce(JSON.stringify(changed));
    expect(
      await refreshProjectSetupOnOpen('/work/code', '/work/tickets.hs2', (root, store) =>
        refreshLocalProjectSetup(root, store, runner),
      ),
    ).toEqual({ codexHooksChanged: '.codex/hooks.json' });
    expect(await refreshProjectSetupOnOpen('/work/code', '/work/tickets.hs2', async () => ({}))).toEqual({});
  });
  it('retains changed-hook review across failed opens, isolates roots, and clears after delivery (HS2-4AR09Z)', async () => {
    const pending = new Map<string, { warning?: string; codexHooksChanged?: string }>();
    const changed = { codexHooksChanged: '.codex/hooks.json' };
    await expect(
      deliverProjectSetupNotice(
        '/one',
        changed,
        async () => {
          throw new Error('server unavailable');
        },
        pending,
      ),
    ).rejects.toThrow('server unavailable');
    expect(await deliverProjectSetupNotice('/two', {}, async (result) => result, pending)).toEqual({});
    await expect(
      deliverProjectSetupNotice(
        '/one',
        {},
        async () => {
          throw new Error('still unavailable');
        },
        pending,
      ),
    ).rejects.toThrow('still unavailable');
    expect(
      await deliverProjectSetupNotice('/one', { warning: 'refresh refused' }, async (result) => result, pending),
    ).toEqual({ ...changed, warning: 'refresh refused' });
    expect(await deliverProjectSetupNotice('/one', {}, async (result) => result, pending)).toEqual({});
    expect(pending.size).toBe(0);
    const partial = { warning: 'Later setup failed. Run /hooks in Codex for this checkout.' };
    await expect(
      deliverProjectSetupNotice(
        '/one',
        partial,
        async () => {
          throw new Error('open failed');
        },
        pending,
      ),
    ).rejects.toThrow('open failed');
    expect(await deliverProjectSetupNotice('/one', {}, async (result) => result, pending)).toEqual(partial);
    expect(await deliverProjectSetupNotice('/one', {}, async (result) => result, pending)).toEqual({});
  });
  it('refuses missing or mismatched compiled setup assets with rebuild guidance before writing', async () => {
    const fingerprint = await developmentSetupAssetsFingerprint();
    expect(fingerprint).toMatch(/^[a-f0-9]{64}$/);
    expect(() => {
      requireCurrentSetupAssets(cli, fingerprint);
    }).toThrow(/does not report.*cargo build -p hotsheet-cli.*No setup files were changed/i);
    expect(() => {
      requireCurrentSetupAssets({ ...cli, setup_assets_fingerprint: '0'.repeat(64) }, fingerprint);
    }).toThrow(/does not match.*cargo build -p hotsheet-cli/i);
    expect(() => {
      requireCurrentSetupAssets({ ...cli, setup_assets_fingerprint: fingerprint }, fingerprint);
    }).not.toThrow();
    const staleRunner = vi.fn().mockResolvedValue(JSON.stringify(cli));
    await expect(refreshLocalProjectSetup('/work/code', '/work/tickets.hs2', staleRunner)).rejects.toThrow(
      /No setup files were changed/i,
    );
    expect(staleRunner).toHaveBeenCalledOnce();
  });
});

describe('authenticatedServerUrl', () => {
  it('uses legacy query authentication for loopback polling without changing browser URLs', () => {
    expect(authenticatedServerUrl('http://127.0.0.1:55560', '/ws/poll?timeout_ms=25000&since=7', 'old secret')).toBe(
      'http://127.0.0.1:55560/ws/poll?timeout_ms=25000&since=7&secret=old+secret',
    );
  });

  it('does not put secrets into ordinary upstream request URLs', () => {
    expect(authenticatedServerUrl('http://127.0.0.1:55560', '/tickets?text=one', 'secret')).toBe(
      'http://127.0.0.1:55560/tickets?text=one',
    );
  });
});

describe('projectScopedServerPath', () => {
  it('scopes project settings independently of ticket sources and leaves host APIs alone', () => {
    expect(projectScopedServerPath('project one', '/commands')).toBe('/checkouts/project%20one/commands');
    // Kept (possibly empty) command groups live beside the project's commands (HS2-EZ5KMC).
    expect(projectScopedServerPath('project one', '/command-groups')).toBe('/checkouts/project%20one/command-groups');
    // AI tool defaults belong to the project (HS2-SW5S13); the tool inventory stays machine-wide.
    expect(projectScopedServerPath('project one', '/ai-settings')).toBe('/checkouts/project%20one/ai-settings');
    // A project lists only its checkout's linked sources (HS2-3SCH1K) and reads, creates, edits, and
    // disables only the connections it owns (HS2-SM9PM8); accounts stay machine-wide.
    expect(projectScopedServerPath('project one', '/providers')).toBe('/checkouts/project%20one/providers');
    expect(projectScopedServerPath('project one', '/provider-connections')).toBe(
      '/checkouts/project%20one/provider-connections',
    );
    expect(projectScopedServerPath('project one', '/provider-connections/github-a')).toBe(
      '/checkouts/project%20one/provider-connections/github-a',
    );
    expect(projectScopedServerPath('project one', '/provider-connections/github-a/disabled')).toBe(
      '/checkouts/project%20one/provider-connections/github-a/disabled',
    );
    expect(projectScopedServerPath('project one', '/accounts')).toBe('/accounts');
    expect(projectScopedServerPath('project one', '/accounts/github-app-1/github-repositories')).toBe(
      '/accounts/github-app-1/github-repositories',
    );
    expect(projectScopedServerPath('project one', '/ai-tools?refresh=true')).toBe('/ai-tools?refresh=true');
    expect(projectScopedServerPath('project one', '/commands/review/run?confirm=true')).toBe(
      '/checkouts/project%20one/commands/review/run?confirm=true',
    );
    expect(projectScopedServerPath('project one', '/command-runs/run-1?after=3')).toBe(
      '/checkouts/project%20one/command-runs/run-1?after=3',
    );
    expect(projectScopedServerPath('project one', '/command-runs/run-1/cancel')).toBe(
      '/checkouts/project%20one/command-runs/run-1/cancel',
    );
    expect(projectScopedServerPath('project one', '/views')).toBe('/checkouts/project%20one/views');
    expect(projectScopedServerPath('project one', '/terminal-settings')).toBe(
      '/checkouts/project%20one/terminal-settings',
    );
    // A project's explicit close and its change stream carry the checkout (HS2-ARJ9J1).
    expect(projectScopedServerPath('project one', '/close?client=tab-1')).toBe(
      '/checkouts/project%20one/close?client=tab-1',
    );
    expect(projectScopedServerPath('project one', '/ws/poll?timeout_ms=0&client=tab-1')).toBe(
      '/ws/poll?timeout_ms=0&client=tab-1&checkout=project%20one',
    );
    expect(projectScopedServerPath('project one', '/ws/poll')).toBe('/ws/poll?checkout=project%20one');
    expect(projectScopedServerPath('project one', '/checkouts/project%20one/tickets')).toBe(
      '/checkouts/project%20one/tickets',
    );
  });
});

describe('authenticatedTerminalWebSocketUrl', () => {
  it('adds the secret only to the loopback upstream and escapes terminal identity', () => {
    expect(authenticatedTerminalWebSocketUrl('http://127.0.0.1:5511', 'codex/main', 'private value')).toBe(
      'ws://127.0.0.1:5511/terminals/codex%2Fmain/attach?secret=private+value',
    );
    expect(authenticatedTerminalWebSocketUrl('https://hs.test', 'term', 'secret')).toMatch(/^wss:/);
  });
});

describe('revealCommand', () => {
  it('uses argument arrays and the platform-native file location action without a shell', () => {
    expect(revealCommand('/tmp/broken ticket.md', 'darwin')).toEqual({
      command: 'open',
      args: ['-R', '/tmp/broken ticket.md'],
    });
    expect(revealCommand('C:\\work\\broken.md', 'win32')).toEqual({
      command: 'explorer.exe',
      args: ['/select,C:\\work\\broken.md'],
    });
    expect(revealCommand('/work/tickets/broken.md', 'linux')).toEqual({ command: 'xdg-open', args: ['/work/tickets'] });
  });

  it('allows only an exact path from current authenticated corrupt diagnostics', () => {
    const diagnostics = [{ path: '/work/store/tickets/broken.md' }];
    expect(() => {
      requireReportedCorruptPath(diagnostics, '/work/store/tickets/broken.md');
    }).not.toThrow();
    expect(() => {
      requireReportedCorruptPath(diagnostics, '/work/store/../secrets.txt');
    }).toThrow(/no longer present/);
  });

  it('routes a specific project and path through an injected launcher boundary', async () => {
    const reveal = vi.fn().mockResolvedValue(undefined);
    const response = await createDevApp(true, undefined, reveal).request(
      '/__hotsheet/projects/demo/corrupt-tickets/reveal',
      { method: 'POST', headers: { 'content-type': 'application/json' }, body: '{"path":"/work/broken.md"}' },
    );
    expect(response.status).toBe(200);
    expect(await response.json()).toEqual({ revealed: true });
    expect(reveal).toHaveBeenCalledWith('demo', '/work/broken.md');
    expect(
      (
        await createDevApp(false, undefined, reveal).request('/__hotsheet/projects/demo/corrupt-tickets/reveal', {
          method: 'POST',
        })
      ).status,
    ).toBe(404);
  });
});

describe('native folder chooser', () => {
  it('uses fixed argument arrays for each host platform and returns the selected path', async () => {
    expect(folderChooserCommand('darwin')).toMatchObject({
      command: 'osascript',
      args: ['-e', expect.stringContaining('choose folder')],
    });
    expect(folderChooserCommand('win32')).toMatchObject({ command: 'powershell.exe' });
    expect(folderChooserCommand('linux')).toEqual({
      command: 'zenity',
      args: ['--file-selection', '--directory', '--title=Choose a folder for Hot Sheet'],
    });
    const runner = vi.fn().mockResolvedValue('/work/selected');
    await expect(chooseLocalFolder(runner, 'darwin')).resolves.toBe('/work/selected');
    expect(runner).toHaveBeenCalledWith('osascript', expect.any(Array));
  });
  it('exposes selection and cancellation only from the local development bridge', async () => {
    const choose = vi.fn().mockResolvedValueOnce('/work/selected').mockResolvedValueOnce(undefined),
      app = createDevApp(true, undefined, undefined, choose);
    expect(await (await app.request('/__hotsheet/folders/choose', { method: 'POST' })).json()).toEqual({
      path: '/work/selected',
    });
    expect(await (await app.request('/__hotsheet/folders/choose', { method: 'POST' })).json()).toEqual({});
    expect((await createDevApp(false).request('/__hotsheet/folders/choose', { method: 'POST' })).status).toBe(404);
  });
  it('creates bootstrap and standalone stores through explicit CLI argument arrays', () => {
    expect(localStoreInitArgs('/tmp/bootstrap')).toEqual(['init', '-C', '/tmp/bootstrap', '--prefix', 'HS2']);
    expect(localStoreInitArgs('/work/demo.hs2', true)).toEqual([
      'init',
      '--standalone',
      '--at',
      '/work/demo.hs2',
      '--prefix',
      'HS2',
    ]);
    expect(projectBootstrapArgs('/work/demo', '/work/demo.hs2')).toEqual([
      'bootstrap',
      '--project',
      '/work/demo',
      '--store',
      '/work/demo.hs2',
      '--prefix',
      'HS2',
    ]);
    expect(projectBootstrapArgs('/work/demo', '/work/demo.hs2', 'git@example.com:tickets.git')).toEqual([
      'bootstrap',
      '--project',
      '/work/demo',
      '--store',
      '/work/demo.hs2',
      '--prefix',
      'HS2',
      '--remote',
      'git@example.com:tickets.git',
    ]);
    const first = projectServerPlan('/machine/bootstrap.hs2', '/work/one', '/work/one.hs2'),
      second = projectServerPlan('/machine/bootstrap.hs2', '/work/two', '/work/two.hs2');
    expect(first).toEqual({
      serverStore: '/machine/bootstrap.hs2',
      openBody: { root: '/work/one', stores: ['/work/one.hs2'] },
    });
    expect(second).toEqual({
      serverStore: first.serverStore,
      openBody: { root: '/work/two', stores: ['/work/two.hs2'] },
    });
    expect(projectServerPlan('/machine/bootstrap.hs2', '/work/empty').openBody).toEqual({ root: '/work/empty' });
  });

  it('has the graphical bridge invoke the same headless bootstrap workflow on every setup', async () => {
    const directory = await mkdtemp(resolve(tmpdir(), 'hotsheet-client-bootstrap-')),
      project = resolve(directory, 'project'),
      store = resolve(directory, 'tickets.hs2'),
      calls: Array<{ command: string; args: string[]; cwd: string }> = [];
    await mkdir(project);
    const fingerprint = await developmentSetupAssetsFingerprint();
    const runner = async (command: string, args: string[], cwd: string) => {
      calls.push({ command, args, cwd });
      if (args.includes('compatibility'))
        return JSON.stringify({
          generation: 'hs2',
          setup_assets_fingerprint: fingerprint,
          store_schema: { min: 1, max: 3, creates: 3 },
        });
      await mkdir(store, { recursive: true });
      return '';
    };
    try {
      const canonicalProject = await realpath(project);
      const first = await createLocalGitTicketStore(project, store, runner),
        canonicalStore = await realpath(store);
      expect(first).toBe(canonicalStore);
      await expect(createLocalGitTicketStore(project, store, runner)).resolves.toBe(canonicalStore);
      expect(calls).toHaveLength(4);
      const bootstraps = calls.filter((call) => call.args.includes('bootstrap'));
      expect(bootstraps).toHaveLength(2);
      expect(bootstraps[0].args).toEqual(projectBootstrapArgs(canonicalProject, store));
      expect(bootstraps[1]).toEqual(bootstraps[0]);
    } finally {
      await rm(directory, { recursive: true, force: true });
    }
  });
  it('preflights the active project server and leaves no store when its schema range is older', async () => {
    const directory = await mkdtemp(resolve(tmpdir(), 'hotsheet-client-schema-')),
      project = resolve(directory, 'project'),
      store = resolve(directory, 'tickets.hs2');
    await mkdir(project);
    const canonicalProject = await realpath(project),
      sessions = projectSessionRegistry(),
      fingerprint = await developmentSetupAssetsFingerprint(),
      runner = vi.fn().mockResolvedValue(
        JSON.stringify({
          generation: 'hs2',
          setup_assets_fingerprint: fingerprint,
          store_schema: { min: 1, max: 3, creates: 3 },
        }),
      );
    sessions.set('schema-preflight', { url: 'http://older-server.test', secret: 'private', root: canonicalProject });
    vi.stubGlobal(
      'fetch',
      vi
        .fn()
        .mockResolvedValue(
          new Response(
            JSON.stringify({ generation: 'hs2', protocol: { min: 1, max: 1 }, store_schema: { min: 1, max: 2 } }),
            { status: 200, headers: { 'content-type': 'application/json' } },
          ),
        ),
    );
    try {
      await expect(createLocalGitTicketStore(project, store, runner)).rejects.toThrow(
        /No ticket repository was created.*schema through 2/i,
      );
      expect(runner).toHaveBeenCalledOnce();
      expect(runner.mock.calls[0]?.[1]).toEqual(['compatibility', '--json']);
      await expect(realpath(store)).rejects.toThrow();
    } finally {
      sessions.delete('schema-preflight');
      vi.unstubAllGlobals();
      await rm(directory, { recursive: true, force: true });
    }
  });
  it('exposes git ticket-store setup only through the local development bridge', async () => {
    const setup = vi.fn().mockResolvedValueOnce('/work/demo.hs2').mockResolvedValueOnce('/chosen/tickets'),
      app = createDevApp(true, undefined, undefined, undefined, setup);
    const response = await app.request('/__hotsheet/projects/setup-git', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: '{"root":"/work/demo"}',
    });
    expect(response.status).toBe(201);
    expect(await response.json()).toEqual({ ticketStore: '/work/demo.hs2', connectionId: '719abfebc935ba14' });
    expect(setup).toHaveBeenCalledWith('/work/demo', undefined);
    const custom = await app.request('/__hotsheet/projects/setup-git', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: '{"root":"/work/demo","location":"/chosen/tickets"}',
    });
    expect(custom.status).toBe(201);
    expect(setup).toHaveBeenLastCalledWith('/work/demo', '/chosen/tickets');
    expect(await custom.json()).toMatchObject({
      ticketStore: '/chosen/tickets',
      connectionId: expect.stringMatching(/^[a-f0-9]{16}$/),
    });
    expect(
      (
        await createDevApp(false, undefined, undefined, undefined, setup).request('/__hotsheet/projects/setup-git', {
          method: 'POST',
        })
      ).status,
    ).toBe(404);
  });
});

describe('Hot Sheet 1 project import bridge', () => {
  it('derives the same normalized project identity stored by HS1 channels', () => {
    expect(hs1ChannelSlug('/work/Kerf Project/.hotsheet')).toBe('kerf-project');
  });
  it('uses argument arrays for the standalone migrator and preserves backups, snapshots, unknown files, and the HS2 link', () => {
    expect(hs1MigrationArgs('/work/demo', '/tickets/demo.hs2', '/app/migrator/export.mjs')).toEqual([
      '/work/demo/.hotsheet',
      '-C',
      '/tickets/demo.hs2',
      '--migrator',
      '/app/migrator/export.mjs',
    ]);
    expect(['db', 'attachments', 'settings.json'].filter(preserveHs1Entry)).toEqual([]);
    expect(['store', 'db.hs1-backup', 'Backup-2026', 'snapshot.tar.gz', 'notes.txt'].filter(preserveHs1Entry)).toEqual([
      'store',
      'db.hs1-backup',
      'Backup-2026',
      'snapshot.tar.gz',
      'notes.txt',
    ]);
  });
  it('removes only explicit live HS1 entries and keeps every backup and unknown file', async () => {
    const parent = await mkdtemp(resolve(tmpdir(), 'hotsheet-hs1-cleanup-')),
      directory = resolve(parent, '.hotsheet');
    try {
      await mkdir(resolve(directory, 'db'), { recursive: true });
      await writeFile(resolve(directory, 'db/PG_VERSION'), '17');
      await mkdir(resolve(directory, 'attachments'));
      await writeFile(resolve(directory, 'settings.json'), '{}');
      await writeFile(resolve(directory, 'worklist.md'), 'generated');
      await mkdir(resolve(directory, 'backups'));
      await writeFile(resolve(directory, 'backups/keep.tar.gz'), 'backup');
      await writeFile(resolve(directory, 'snapshot.tar.gz'), 'backup');
      await writeFile(resolve(directory, 'notes.txt'), 'unknown');
      await writeFile(resolve(directory, 'store'), '/tickets/demo.hs2');
      await expect(removeHs1LiveData(directory, () => false)).resolves.toEqual([
        'attachments',
        'db',
        'settings.json',
        'worklist.md',
      ]);
      expect((await readdir(directory)).sort()).toEqual(['backups', 'notes.txt', 'snapshot.tar.gz', 'store']);
      await expect(removeHs1LiveData(directory, () => false)).resolves.toEqual([]);
    } finally {
      await rm(parent, { recursive: true, force: true });
    }
  });
  it('preserves schema-marked HS2 project settings while removing unmarked HS1 settings', async () => {
    const parent = await mkdtemp(resolve(tmpdir(), 'hotsheet-hs2-settings-cleanup-')),
      directory = resolve(parent, '.hotsheet');
    try {
      await mkdir(resolve(directory, 'db'), { recursive: true });
      await writeFile(resolve(directory, 'db/PG_VERSION'), '17');
      await writeFile(resolve(directory, 'settings.json'), '{"$hotsheetSchema":1,"views":[]}');
      await writeFile(resolve(directory, 'settings.local.json'), '{"$hotsheetSchema":2,"commands":[]}');
      await expect(removeHs1LiveData(directory, () => false)).resolves.toEqual(['db']);
      expect((await readdir(directory)).sort()).toEqual(['settings.json', 'settings.local.json']);
      await mkdir(resolve(directory, 'db'), { recursive: true });
      await writeFile(resolve(directory, 'db/PG_VERSION'), '17');
      await writeFile(resolve(directory, 'settings.local.json'), '{"custom_commands":[]}');
      await expect(removeHs1LiveData(directory, () => false)).resolves.toEqual(['db', 'settings.local.json']);
      expect((await readdir(directory)).sort()).toEqual(['settings.json']);
    } finally {
      await rm(parent, { recursive: true, force: true });
    }
  });
  it('refuses partial cleanup while a registered HS1 channel is still live', async () => {
    const parent = await mkdtemp(resolve(tmpdir(), 'hotsheet-hs1-running-')),
      directory = resolve(parent, '.hotsheet');
    try {
      await mkdir(resolve(directory, 'db'), { recursive: true });
      await writeFile(resolve(directory, 'db/PG_VERSION'), '17');
      await writeFile(resolve(directory, 'settings.json'), '{}');
      await mkdir(resolve(directory, 'channel-ports.d'));
      await writeFile(
        resolve(directory, 'channel-ports.d/42.json'),
        JSON.stringify({ pid: 42, slug: hs1ChannelSlug(directory) }),
      );
      await expect(removeHs1LiveData(directory, (pid) => pid === 42)).rejects.toThrow(
        /still running.*42.*no files were removed/i,
      );
      expect((await readdir(directory)).sort()).toEqual(['channel-ports.d', 'db', 'settings.json']);
    } finally {
      await rm(parent, { recursive: true, force: true });
    }
  });
  it('keeps identity-less legacy channel registrations conservatively blocking', async () => {
    const parent = await mkdtemp(resolve(tmpdir(), 'hotsheet-hs1-legacy-running-')),
      directory = resolve(parent, '.hotsheet');
    try {
      await mkdir(resolve(directory, 'db'), { recursive: true });
      await writeFile(resolve(directory, 'db/PG_VERSION'), '17');
      await mkdir(resolve(directory, 'channel-ports.d'));
      await writeFile(resolve(directory, 'channel-ports.d/43.json'), '{"pid":43}');
      await expect(removeHs1LiveData(directory, (pid) => pid === 43)).rejects.toThrow(
        /still running.*43.*no files were removed/i,
      );
      expect((await readdir(directory)).sort()).toEqual(['channel-ports.d', 'db']);
    } finally {
      await rm(parent, { recursive: true, force: true });
    }
  });
  it('refuses cleanup while the HS1 project lock is live even without a channel registration', async () => {
    const parent = await mkdtemp(resolve(tmpdir(), 'hotsheet-hs1-lock-')),
      directory = resolve(parent, '.hotsheet');
    try {
      await mkdir(resolve(directory, 'db'), { recursive: true });
      await writeFile(resolve(directory, 'db/PG_VERSION'), '17');
      await writeFile(resolve(directory, 'hotsheet.lock'), '{"pid":73}');
      await expect(removeHs1LiveData(directory, (pid) => pid === 73)).rejects.toThrow(
        /still running.*73.*no files were removed/i,
      );
      expect((await readdir(directory)).sort()).toEqual(['db', 'hotsheet.lock']);
      await expect(removeHs1LiveData(directory, () => false)).resolves.toEqual(['db', 'hotsheet.lock']);
    } finally {
      await rm(parent, { recursive: true, force: true });
    }
  });
  it('preserves legacy data when its project lock cannot be verified', async () => {
    const parent = await mkdtemp(resolve(tmpdir(), 'hotsheet-hs1-invalid-lock-')),
      directory = resolve(parent, '.hotsheet');
    try {
      await mkdir(resolve(directory, 'db'), { recursive: true });
      await writeFile(resolve(directory, 'db/PG_VERSION'), '17');
      await writeFile(resolve(directory, 'hotsheet.lock'), '{invalid');
      await expect(removeHs1LiveData(directory, () => false)).rejects.toThrow(
        /Cannot verify.*lock.*no files were removed/i,
      );
      expect((await readdir(directory)).sort()).toEqual(['db', 'hotsheet.lock']);
    } finally {
      await rm(parent, { recursive: true, force: true });
    }
  });
  it('removes only matching saved projects and HS1 MCP channels, including on retry', async () => {
    const parent = await mkdtemp(resolve(tmpdir(), 'hotsheet-hs1-registry-')),
      root = resolve(parent, 'legacy'),
      other = resolve(parent, 'other'),
      home = resolve(parent, 'home');
    try {
      await mkdir(resolve(root, '.hotsheet'), { recursive: true });
      await mkdir(home);
      await writeFile(
        resolve(home, 'projects.json'),
        JSON.stringify([resolve(root, '.hotsheet'), resolve(other, '.hotsheet')]),
      );
      await writeFile(resolve(home, 'instance.json'), '{"pid":82}');
      await writeFile(
        resolve(root, '.mcp.json'),
        JSON.stringify({
          otherSetting: true,
          mcpServers: {
            'hotsheet-channel-legacy': { args: ['channel.js', '--data-dir', resolve(root, '.hotsheet')] },
            'hotsheet-channel-other': { args: ['channel.js', '--data-dir', resolve(other, '.hotsheet')] },
            hotsheet: { args: ['--path', resolve(root, 'tickets.hs2')] },
          },
        }),
      );
      if (process.platform !== 'win32') await chmod(resolve(root, '.mcp.json'), 0o600);
      expect(await hasHs1Registrations(root, home)).toBe(true);
      await expect(reconcileHs1Registrations(root, home, (pid) => pid === 82)).rejects.toThrow(
        /still running.*82.*no files were removed/i,
      );
      expect(JSON.parse(await readFile(resolve(home, 'projects.json'), 'utf8'))).toHaveLength(2);
      await reconcileHs1Registrations(root, home, () => false);
      await reconcileHs1Registrations(root, home, () => false);
      expect(await hasHs1Registrations(root, home)).toBe(false);
      expect(JSON.parse(await readFile(resolve(home, 'projects.json'), 'utf8'))).toEqual([resolve(other, '.hotsheet')]);
      expect(JSON.parse(await readFile(resolve(root, '.mcp.json'), 'utf8'))).toEqual({
        otherSetting: true,
        mcpServers: {
          'hotsheet-channel-other': { args: ['channel.js', '--data-dir', resolve(other, '.hotsheet')] },
          hotsheet: { args: ['--path', resolve(root, 'tickets.hs2')] },
        },
      });
      if (process.platform !== 'win32') expect((await stat(resolve(root, '.mcp.json'))).mode & 0o777).toBe(0o600);
    } finally {
      await rm(parent, { recursive: true, force: true });
    }
  });
  it('keeps registries untouched if either HS1 project list or MCP configuration is malformed', async () => {
    const parent = await mkdtemp(resolve(tmpdir(), 'hotsheet-hs1-registry-invalid-')),
      root = resolve(parent, 'legacy'),
      home = resolve(parent, 'home');
    try {
      await mkdir(resolve(root, '.hotsheet'), { recursive: true });
      await mkdir(home);
      const projects = JSON.stringify([resolve(root, '.hotsheet')]);
      await writeFile(resolve(home, 'projects.json'), projects);
      await writeFile(resolve(root, '.mcp.json'), '{invalid');
      await expect(hasHs1Registrations(root, home)).rejects.toThrow(/MCP configuration/);
      await expect(reconcileHs1Registrations(root, home, () => false)).rejects.toThrow(/MCP configuration/);
      expect(await readFile(resolve(home, 'projects.json'), 'utf8')).toBe(projects);
      await writeFile(resolve(root, '.mcp.json'), '{}');
      await writeFile(resolve(home, 'projects.json'), '{}');
      await expect(hasHs1Registrations(root, home)).rejects.toThrow(/saved project list/);
      await expect(reconcileHs1Registrations(root, home, () => false)).rejects.toThrow(/saved project list/);
      expect(await readFile(resolve(root, '.mcp.json'), 'utf8')).toBe('{}');
    } finally {
      await rm(parent, { recursive: true, force: true });
    }
  });
  it('detects saved-list and channel-only registrations after an older cleanup removed the database', async () => {
    const parent = await mkdtemp(resolve(tmpdir(), 'hotsheet-hs1-repair-detection-')),
      root = resolve(parent, 'legacy'),
      other = resolve(parent, 'other'),
      home = resolve(parent, 'home');
    try {
      await mkdir(resolve(root, '.hotsheet'), { recursive: true });
      await mkdir(home);
      await writeFile(resolve(home, 'projects.json'), JSON.stringify([resolve(other, '.hotsheet')]));
      expect(await hasHs1Registrations(root, home)).toBe(false);
      await writeFile(resolve(home, 'projects.json'), JSON.stringify([resolve(root, '.hotsheet')]));
      expect(await hasHs1Registrations(root, home)).toBe(true);
      await writeFile(resolve(home, 'projects.json'), '[]');
      await writeFile(
        resolve(root, '.mcp.json'),
        JSON.stringify({
          mcpServers: { 'hotsheet-channel-legacy': { args: ['--data-dir', resolve(root, '.hotsheet')] } },
        }),
      );
      expect(await hasHs1Registrations(root, home)).toBe(true);
      await reconcileHs1Registrations(root, home, () => false);
      expect(await hasHs1Registrations(root, home)).toBe(false);
    } finally {
      await rm(parent, { recursive: true, force: true });
    }
  });
  it.skipIf(process.platform === 'win32')(
    'removes symlink aliases and registrations refilled after cleanup',
    async () => {
      const parent = await mkdtemp(resolve(tmpdir(), 'hotsheet-hs1-registry-alias-')),
        root = resolve(parent, 'legacy'),
        alias = resolve(parent, 'shortcut'),
        home = resolve(parent, 'home');
      try {
        await mkdir(resolve(root, '.hotsheet'), { recursive: true });
        await mkdir(home);
        await symlink(root, alias, 'dir');
        const aliasData = resolve(alias, '.hotsheet'),
          projectsPath = resolve(home, 'projects.json'),
          mcpPath = resolve(root, '.mcp.json');
        for (let attempt = 0; attempt < 2; attempt++) {
          await writeFile(projectsPath, JSON.stringify([aliasData]));
          await writeFile(
            mcpPath,
            JSON.stringify({ mcpServers: { 'hotsheet-channel-legacy': { args: ['--data-dir', aliasData] } } }),
          );
          await reconcileHs1Registrations(root, home, () => false);
          expect(JSON.parse(await readFile(projectsPath, 'utf8'))).toEqual([]);
          expect(JSON.parse(await readFile(mcpPath, 'utf8'))).toEqual({ mcpServers: {} });
        }
      } finally {
        await rm(parent, { recursive: true, force: true });
      }
    },
  );
  it('ignores live channel registrations that explicitly belong to another HS1 project', async () => {
    const parent = await mkdtemp(resolve(tmpdir(), 'kerf-hs1-cleanup-')),
      directory = resolve(parent, '.hotsheet'),
      probe = vi.fn(() => true);
    try {
      await mkdir(resolve(directory, 'db'), { recursive: true });
      await writeFile(resolve(directory, 'db/PG_VERSION'), '17');
      await writeFile(resolve(directory, 'settings.json'), '{}');
      await mkdir(resolve(directory, 'channel-ports.d'));
      await writeFile(resolve(directory, 'channel-port'), '{"port":50312,"pid":31807,"slug":"hotsheet"}');
      await writeFile(
        resolve(directory, 'channel-ports.d/31807.json'),
        '{"port":50312,"pid":31807,"slug":"hotsheet","warm":true}',
      );
      await writeFile(
        resolve(directory, 'channel-ports.d/68568.json'),
        '{"port":64567,"pid":68568,"slug":"hotsheet","warm":true}',
      );
      expect(hs1ChannelSlug(directory)).not.toBe('hotsheet');
      await expect(removeHs1LiveData(directory, probe)).resolves.toEqual([
        'channel-port',
        'channel-ports.d',
        'db',
        'settings.json',
      ]);
      expect(probe).not.toHaveBeenCalled();
      expect(await readdir(directory)).toEqual([]);
    } finally {
      await rm(parent, { recursive: true, force: true });
    }
  });
  it('exposes explicit import and cleanup actions only through the local bridge', async () => {
    const migrate = vi.fn().mockResolvedValue({
        ticketStore: '/tickets/demo.hs2',
        connectionId: 'source',
        tickets: 12,
        attachments: 3,
        toolsConfigured: true,
      }),
      remove = vi.fn().mockResolvedValue(['db', 'settings.json']),
      app = createDevApp(true, undefined, undefined, undefined, undefined, undefined, migrate, remove);
    const imported = await app.request('/__hotsheet/projects/migrate-hs1', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: '{"root":"/work/demo","location":"/tickets/demo.hs2"}',
    });
    expect(imported.status).toBe(201);
    expect(await imported.json()).toMatchObject({ tickets: 12, attachments: 3 });
    expect(migrate).toHaveBeenCalledWith('/work/demo', '/tickets/demo.hs2');
    const cleaned = await app.request('/__hotsheet/projects/demo/hs1-data', { method: 'DELETE' });
    expect(await cleaned.json()).toEqual({ removed: ['db', 'settings.json'] });
    expect(remove).toHaveBeenCalledWith('demo');
    const disabled = createDevApp(false, undefined, undefined, undefined, undefined, undefined, migrate, remove);
    expect((await disabled.request('/__hotsheet/projects/migrate-hs1', { method: 'POST' })).status).toBe(404);
    expect((await disabled.request('/__hotsheet/projects/demo/hs1-data', { method: 'DELETE' })).status).toBe(404);
  });
});

describe('Git ticket-store remote setup', () => {
  it('captures the exact stderr from a failed Git subprocess', async () => {
    await expect(
      runGitCommand(process.execPath, ['-e', 'process.stderr.write("fatal: preserved details\\n");process.exit(1)']),
    ).rejects.toThrow('fatal: preserved details');
  });
  it('adds origin and performs the first push with argument arrays', async () => {
    const store = await realpath(await mkdtemp(resolve(tmpdir(), 'hotsheet-remote-'))),
      calls: Array<[string, string[]]> = [],
      runner = async (command: string, args: string[]) => {
        calls.push([command, args]);
      };
    try {
      await writeFile(resolve(store, 'hotsheet-store.json'), '{}');
      await connectGitTicketStoreRemote(store, 'git@example.com:team/tickets.git', runner, async () => {
        throw new Error('No origin');
      });
      expect(calls).toEqual([
        ['git', ['-C', store, 'remote', 'add', 'origin', 'git@example.com:team/tickets.git']],
        ['git', ['-C', store, 'push', '-u', 'origin', 'HEAD']],
      ]);
    } finally {
      await rm(store, { recursive: true, force: true });
    }
  });
  it('removes the just-added origin and preserves unknown push diagnostics for a retry', async () => {
    const store = await realpath(await mkdtemp(resolve(tmpdir(), 'hotsheet-remote-'))),
      calls: Array<[string, string[]]> = [];
    let origin: string | undefined;
    const runner = async (command: string, args: string[]) => {
      calls.push([command, args]);
      if (args.includes('add')) origin = 'git@example.com:team/tickets.git';
      if (args.includes('push')) throw new Error('remote helper reported an unfamiliar failure');
    };
    try {
      await writeFile(resolve(store, 'hotsheet-store.json'), '{}');
      await expect(
        connectGitTicketStoreRemote(store, 'git@example.com:team/tickets.git', runner, async () => {
          if (!origin) throw new Error('No origin');
          return origin;
        }),
      ).rejects.toThrow(/could not push.*Git details: remote helper reported an unfamiliar failure/i);
      expect(calls.at(-1)).toEqual(['git', ['-C', store, 'remote', 'remove', 'origin']]);
    } finally {
      await rm(store, { recursive: true, force: true });
    }
  });
  it('turns common remote failures into next steps without hiding Git stderr', () => {
    expect(describeGitRemoteFailure(new Error('error: remote origin already exists.'), 'add').message).toMatch(
      /already has an origin.*Git details: error: remote origin already exists/i,
    );
    expect(
      describeGitRemoteFailure(
        new Error('ERROR: Repository not found.\nfatal: Could not read from remote repository.'),
        'push',
      ).message,
    ).toMatch(/not found.*verify the clone URL.*Git details: ERROR: Repository not found/i);
    expect(
      describeGitRemoteFailure(new Error('git@github.com: Permission denied (publickey).'), 'push').message,
    ).toMatch(/authenticate.*SSH key.*Permission denied \(publickey\)/i);
    expect(
      describeGitRemoteFailure(new Error('fatal: unable to access: Could not resolve host: github.com'), 'push')
        .message,
    ).toMatch(/could not reach.*network.*Could not resolve host/i);
    expect(describeGitRemoteFailure(new Error('! [rejected] HEAD -> main (fetch first)'), 'push').message).toMatch(
      /already contains commits.*empty remote.*fetch first/i,
    );
  });
  it('rejects option-like and multiline remote values before running Git', async () => {
    const runner = vi.fn();
    await expect(
      connectGitTicketStoreRemote('/Users/westphal/Documents/hotsheet2.hs2', '--upload-pack=bad', runner),
    ).rejects.toThrow(/valid Git remote URL/);
    await expect(
      connectGitTicketStoreRemote('/Users/westphal/Documents/hotsheet2.hs2', 'good\nbad', runner),
    ).rejects.toThrow(/valid Git remote URL/);
    expect(runner).not.toHaveBeenCalled();
  });
  it('exposes remote connection only through the local bridge', async () => {
    const connect = vi.fn().mockResolvedValue(undefined),
      app = createDevApp(true, undefined, undefined, undefined, undefined, connect),
      request = {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: '{"store":"/tickets","remote":"git@example.com:team/tickets.git"}',
      };
    const response = await app.request('/__hotsheet/projects/setup-git-remote', request);
    expect(response.status).toBe(200);
    expect(connect).toHaveBeenCalledWith('/tickets', 'git@example.com:team/tickets.git');
    expect(
      (
        await createDevApp(false, undefined, undefined, undefined, undefined, connect).request(
          '/__hotsheet/projects/setup-git-remote',
          request,
        )
      ).status,
    ).toBe(404);
  });
  it('returns the complete actionable Git diagnostic through the local bridge', async () => {
    const message = 'The remote repository was not found. Git details: ERROR: Repository not found.',
      connect = vi.fn().mockRejectedValue(new Error(message)),
      app = createDevApp(true, undefined, undefined, undefined, undefined, connect),
      response = await app.request('/__hotsheet/projects/setup-git-remote', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: '{"store":"/tickets","remote":"git@example.com:missing.git"}',
      });
    expect(response.status).toBe(400);
    expect(await response.json()).toEqual({ error: message });
  });
});

describe('remote project checkouts (HS2-QMR41J, HS2-VFNCXG)', () => {
  it('serves the cross-device checkouts endpoint ungated by dev mode, so a remote client can load the list', async () => {
    const checkouts = [{ id: 'demo-checkout', root: '/work/demo', alias: 'demo', stores: ['/work/demo.hs2'] }];
    const list = vi.fn().mockResolvedValue(checkouts);
    // Unlike the local-filesystem endpoints (404 when !dev), this must work for a remote/production client.
    for (const dev of [true, false]) {
      const response = await createDevApp(
        dev,
        undefined,
        undefined,
        undefined,
        undefined,
        undefined,
        undefined,
        undefined,
        undefined,
        list,
      ).request('/__hotsheet/checkouts');
      expect(response.status).toBe(200);
      expect(await response.json()).toEqual(checkouts);
    }
    expect(list).toHaveBeenCalledTimes(2);
  });
  it('surfaces a listing failure as a 502 with a message rather than falling through to the SPA', async () => {
    const list = vi.fn().mockRejectedValue(new Error('server unreachable'));
    const response = await createDevApp(
      true,
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      list,
    ).request('/__hotsheet/checkouts');
    expect(response.status).toBe(502);
    expect(await response.json()).toEqual({ error: 'server unreachable' });
  });
  it('queries the bootstrap server GET /checkouts through its running instance', async () => {
    const root = await mkdtemp(resolve(tmpdir(), 'hs2-picker-'));
    const ensure = vi.fn().mockResolvedValue({ pid: 1, url: 'http://127.0.0.1:9', secret: 'shh' });
    const request = vi.fn().mockResolvedValue([
      { id: 'x', root, alias: 'x', stores: [] },
      { id: 'gone', root: resolve(root, 'deleted'), alias: 'gone', stores: [] },
    ]);
    const resolveStore = vi.fn().mockResolvedValue('/home/server-bootstrap.hs2');
    try {
      const result = await listServerCheckouts(ensure as never, request as never, resolveStore as never);
      expect(ensure).toHaveBeenCalledWith('/home/server-bootstrap.hs2');
      expect(request).toHaveBeenCalledWith(
        { url: 'http://127.0.0.1:9', secret: 'shh', serverStore: '/home/server-bootstrap.hs2' },
        '/checkouts',
      );
      expect(result).toEqual([{ id: 'x', root, alias: 'x', stores: [] }]);
      await writeFile(resolve(root, 'deleted'), 'not a project directory');
      expect(await listServerCheckouts(ensure as never, request as never, resolveStore as never)).toEqual(result);
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });
});

describe('requireCompatibleServer', () => {
  const base = { revisionMismatch: false, sourceStale: false, canRestartServer: false };

  it('blocks newer-server and older-server protocol boundaries with explicit upgrade guidance', () => {
    expect(() => {
      requireCompatibleServer({ ...base, kind: 'client_too_old', detail: 'Client protocol 1–1 is older.' });
    }).toThrow(/update required.*cannot be opened.*requires a newer HS2 client/i);
    expect(() => {
      requireCompatibleServer({ ...base, kind: 'server_too_old', detail: 'Server protocol 0–0 is older.' });
    }).toThrow(/server update required.*cannot open/i);
  });

  it('allows intersecting ranges and legacy servers with unknown metadata', () => {
    expect(() => {
      requireCompatibleServer({ ...base, kind: 'compatible' });
    }).not.toThrow();
    expect(() => {
      requireCompatibleServer({ ...base, kind: 'unknown' });
    }).not.toThrow();
  });
});

describe('dev bridge server health probe (HS2-TANE0V)', () => {
  const instance = { pid: 42, url: 'http://127.0.0.1:8787', secret: 'private', started_at: '2026-09-10T01:00:00Z' };
  const setup = (answers: Array<'ok' | 'down' | 'bad'>) => {
    let clock = 1_000;
    const timeouts: number[] = [],
      timeoutSpy = vi.spyOn(AbortSignal, 'timeout').mockImplementation((ms: number) => {
        timeouts.push(ms);
        return new AbortController().signal;
      }),
      request = vi.fn(() => {
        const answer = answers.shift() ?? 'ok';
        if (answer === 'down') return Promise.reject(new Error('connection refused'));
        return Promise.resolve(Response.json({ status: answer === 'ok' ? 'ok' : 'starting' }));
      }),
      verified = new Map<string, { instance: typeof instance; at: number }>(),
      probe = createServerHealthProbe('/store', {
        request: request as unknown as typeof fetch,
        now: () => clock,
        verified,
      });
    return {
      probe,
      request,
      verified,
      timeouts,
      restore: () => {
        timeoutSpy.mockRestore();
      },
      advance: (ms: number) => {
        clock += ms;
      },
    };
  };

  it('is patient on the first probe of a call and short on retries', async () => {
    const { probe, timeouts, restore } = setup(['down', 'down']);
    await expect(probe(instance, 0)).resolves.toBe(false);
    await expect(probe(instance, 1)).resolves.toBe(false);
    expect(timeouts).toEqual([3_000, 750]);
    restore();
  });

  it('reuses a fresh verification of the same instance, then re-probes after expiry, replacement, or forget', async () => {
    const { probe, request, verified, advance, restore } = setup(['ok', 'ok', 'ok', 'down', 'ok']);
    await expect(probe(instance, 0)).resolves.toBe(true);
    expect(request).toHaveBeenCalledTimes(1);
    // A burst of project opens within the window does not re-request /health.
    advance(9_000);
    await expect(probe(instance, 0)).resolves.toBe(true);
    await expect(probe(instance, 0)).resolves.toBe(true);
    expect(request).toHaveBeenCalledTimes(1);
    // Expired: probe again and refresh the verification.
    advance(1_500);
    await expect(probe(instance, 0)).resolves.toBe(true);
    expect(request).toHaveBeenCalledTimes(2);
    // A replacement process (new pid/started_at) is never trusted from the old verification.
    const replacement = { ...instance, pid: 43, started_at: '2026-09-10T02:00:00Z' };
    await expect(probe(replacement, 0)).resolves.toBe(true);
    expect(request).toHaveBeenCalledTimes(3);
    // A failed forward forgets the verification, so a dead instance is re-probed and reported.
    forgetVerifiedServer('/store', verified);
    await expect(probe(replacement, 0)).resolves.toBe(false);
    expect(verified.has('/store')).toBe(false);
    await expect(probe(replacement, 0)).resolves.toBe(true);
    expect(request).toHaveBeenCalledTimes(5);
    restore();
  });

  it('never caches an unhealthy answer', async () => {
    const { probe, request, verified, restore } = setup(['bad', 'ok']);
    await expect(probe(instance, 0)).resolves.toBe(false);
    expect(verified.size).toBe(0);
    await expect(probe(instance, 1)).resolves.toBe(true);
    expect(request).toHaveBeenCalledTimes(2);
    restore();
  });
});

describe('machine server supervision', () => {
  const running = { pid: 42, url: 'http://127.0.0.1:8787', secret: 'private', started_at: '2026-09-10T01:00:00Z' };
  const replacement = { ...running, pid: 43, started_at: '2026-09-10T02:00:00Z' };

  it('reuses a healthy discovered server without launching another process', async () => {
    const launch = vi.fn(),
      probe = vi.fn().mockResolvedValue(true);
    await expect(
      superviseServer({ discover: vi.fn().mockResolvedValue(running), probe, launch, wait: vi.fn() }, 1),
    ).resolves.toEqual(running);
    expect(probe).toHaveBeenCalledWith(running, 0);
    expect(launch).not.toHaveBeenCalled();
  });

  it('numbers probe attempts so only the first check of a supervision call is patient (HS2-TANE0V)', async () => {
    const probe = vi.fn().mockResolvedValueOnce(false).mockResolvedValueOnce(false).mockResolvedValueOnce(true);
    await expect(
      superviseServer({ discover: vi.fn().mockResolvedValue(running), probe, launch: vi.fn(), wait: vi.fn() }, 3),
    ).resolves.toEqual(running);
    expect(probe.mock.calls.map((call) => call[1])).toEqual([0, 1, 2]);
  });

  it('restarts after a crash but refuses to duplicate a live unhealthy process', async () => {
    let launched = false;
    const discover = vi.fn().mockImplementation(() => (launched ? replacement : undefined)),
      launch = vi.fn().mockImplementation(() => {
        launched = true;
      });
    await expect(
      superviseServer({ discover, probe: vi.fn().mockResolvedValue(true), launch, wait: vi.fn() }, 2),
    ).resolves.toEqual(replacement);
    expect(launch).toHaveBeenCalledOnce();

    const unsafeLaunch = vi.fn();
    await expect(
      superviseServer(
        {
          discover: vi.fn().mockResolvedValue(running),
          probe: vi.fn().mockResolvedValue(false),
          launch: unsafeLaunch,
          wait: vi.fn(),
        },
        2,
      ),
    ).rejects.toThrow(/registered but unhealthy.*preserved/i);
    expect(unsafeLaunch).not.toHaveBeenCalled();
  });

  it('waits for an accepted quiescent restart to relinquish discovery before supervising its replacement', async () => {
    const request = vi.fn(),
      supervise = vi.fn().mockResolvedValue(replacement),
      discover = vi.fn().mockResolvedValueOnce(running).mockResolvedValueOnce(undefined);
    await expect(safelyRestartServer(running, { request, discover, supervise, wait: vi.fn() }, 3)).resolves.toEqual(
      replacement,
    );
    expect(request).toHaveBeenCalledOnce();
    expect(supervise).toHaveBeenCalledOnce();
  });

  it('recovers only the expected unhealthy instance and escalates when it ignores graceful termination', async () => {
    let current: typeof running | typeof replacement | undefined = running;
    const signals: string[] = [],
      launch = vi.fn();
    const platform = {
      discover: vi.fn(async () => current),
      probe: vi.fn(async (instance) => instance === replacement),
      launch,
      wait: vi.fn(async () => undefined),
      terminate: vi.fn(async (_instance: typeof running, signal: 'SIGTERM' | 'SIGKILL') => {
        signals.push(signal);
        if (signal === 'SIGKILL') current = undefined;
      }),
    };
    platform.launch.mockImplementation(async () => {
      current = replacement;
    });
    await expect(
      recoverUnhealthyServer(
        { store: '/tickets', expected: { pid: running.pid, url: running.url, started_at: running.started_at } },
        platform,
        2,
        2,
      ),
    ).resolves.toEqual(replacement);
    expect(signals).toEqual(['SIGTERM', 'SIGKILL']);
    expect(launch).toHaveBeenCalledOnce();

    const protectedTerminate = vi.fn();
    await expect(
      recoverUnhealthyServer(
        { store: '/tickets', expected: { pid: running.pid, url: running.url, started_at: running.started_at } },
        {
          ...platform,
          discover: vi.fn().mockResolvedValue(replacement),
          probe: vi.fn().mockResolvedValue(true),
          terminate: protectedTerminate,
        },
        1,
        1,
      ),
    ).resolves.toEqual(replacement);
    expect(protectedTerminate).not.toHaveBeenCalled();
  });

  it('keeps unhealthy-server recovery behind the local bridge', async () => {
    const recover = vi.fn().mockResolvedValue(replacement),
      body = { store: '/tickets', expected: { pid: running.pid, url: running.url, started_at: running.started_at } },
      request = { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(body) };
    const response = await createDevApp(
      true,
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      undefined,
      recover,
    ).request('/__hotsheet/server/recover-unhealthy', request);
    expect(response.status).toBe(200);
    expect(await response.json()).toEqual({ recovered: true });
    expect(recover).toHaveBeenCalledWith(body);
    expect(
      (
        await createDevApp(
          false,
          undefined,
          undefined,
          undefined,
          undefined,
          undefined,
          undefined,
          undefined,
          recover,
        ).request('/__hotsheet/server/recover-unhealthy', request)
      ).status,
    ).toBe(404);
  });

  it('recognizes a hosted store schema that requires the current server build', () => {
    const server = { generation: 'hs2', protocol: { min: 1, max: 1 }, store_schema: { min: 1, max: 2 } };
    expect(
      storeNeedsServerUpgrade(server, {
        generation: 'hs2',
        store_schema: { min: 1, max: 3, creates: 3 },
        selected_store_schema: 3,
      }),
    ).toBe(true);
    expect(
      storeNeedsServerUpgrade(server, {
        generation: 'hs2',
        store_schema: { min: 1, max: 3, creates: 3 },
        selected_store_schema: 2,
      }),
    ).toBe(false);
  });
});
