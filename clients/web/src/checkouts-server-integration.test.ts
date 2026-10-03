import { mkdtemp, readdir, readFile, realpath, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterAll, beforeAll, describe, expect, it } from 'vitest';

import type { Checkout, ProviderAccount, ProviderConnection, SourceDetach } from './api';
import { createDevApp } from './dev-server';
import { createLocalGitTicketStore, listServerCheckouts, openLocalProject } from './project-bridge';

/**
 * Opt-in real-server integration coverage for the cross-device remote project picker
 * (GET /__hotsheet/checkouts), split from HS2-QMR41J / HS2-MTS80S.
 *
 * HS2-VFNCXG shipped the picker with only a *mocked* client E2E, so the bridge endpoint
 * was never actually wired to a running server and the feature failed against a real
 * server until HS2-QMR41J. Per CLAUDE.md ("Mock the exact transport contract" /
 * "for each newly composed real API surface, run at least one integration or opt-in
 * local-browser flow against the actual server"), this drives the whole boundary with
 * zero transport mocking: a real hotsheet-server is booted through the production
 * onboarding path, a checkout is registered, and the list is read back both through the
 * `listServerCheckouts` bridge helper and through the real Hono `/__hotsheet/checkouts`
 * route that a remote client actually hits.
 *
 * It is opt-in (spawns the real `target/debug/hotsheet-server` + `hotsheet-cli` binaries,
 * binds a loopback port, and writes to an isolated `HOTSHEET_HOME`), so it is skipped in
 * the fast unit tier. Run it with the built binaries present:
 *   HOTSHEET_LIVE_SERVER=1 npx vitest run src/checkouts-server-integration.test.ts
 */
const live = process.env.HOTSHEET_LIVE_SERVER === '1';

describe.skipIf(!live)('remote project picker against a real server (HS2-MTS80S)', () => {
  let home = '';
  let workspace = '';
  let previousHome: string | undefined;

  beforeAll(async () => {
    // Isolate every machine-local file the Node bridge and the Rust server touch
    // (bootstrap store, server instance registry, checkouts.json) into a temp home so
    // the test never reads or mutates the developer's real ~/.hotsheet2.
    previousHome = process.env.HOTSHEET_HOME;
    home = await mkdtemp(join(tmpdir(), 'hs2-checkouts-home-'));
    workspace = await mkdtemp(join(tmpdir(), 'hs2-checkouts-work-'));
    process.env.HOTSHEET_HOME = home;
  }, 120_000);

  afterAll(async () => {
    // Stop the detached server the onboarding path spawned (superviseServer records each
    // instance under $HOTSHEET_HOME/instances/<id>.json), then remove the temp trees.
    try {
      const dir = join(home, 'instances');
      for (const name of await readdir(dir).catch(() => [] as string[])) {
        if (!name.endsWith('.json')) continue;
        // A stopping server may remove its record while this loop runs.
        const text = await readFile(join(dir, name), 'utf8').catch(() => '{}'),
          info = JSON.parse(text) as { pid?: number };
        if (typeof info.pid === 'number') {
          try {
            process.kill(info.pid, 'SIGTERM');
          } catch {
            // Already gone — nothing to stop.
          }
        }
      }
    } finally {
      if (previousHome === undefined) delete process.env.HOTSHEET_HOME;
      else process.env.HOTSHEET_HOME = previousHome;
      await rm(home, { recursive: true, force: true });
      await rm(workspace, { recursive: true, force: true });
    }
  }, 120_000);

  it('registers a checkout and lists it through the bridge helper and the real HTTP route', async () => {
    const projectRoot = await mkdtemp(join(workspace, 'project-'));
    // Real onboarding: bootstrap a git-backed ticket store, then open the project, which
    // boots a real server and registers the checkout via POST /projects/open.
    const store = await createLocalGitTicketStore(projectRoot);
    const session = await openLocalProject(projectRoot, store);
    // The server canonicalizes roots (on macOS /var/... resolves to /private/var/...),
    // so compare against the real path rather than the raw temp path.
    const expectedRoot = await realpath(projectRoot);

    // The bridge helper reads the running server's real GET /checkouts (no mocked request).
    const listed = await listServerCheckouts();
    const mine = listed.find((checkout) => checkout.id === session.id);
    expect(mine, `checkout ${session.id} missing from ${JSON.stringify(listed)}`).toBeDefined();
    expect(await realpath(mine!.root)).toBe(expectedRoot);
    expect(mine!.stores).toContain(store);

    // The real Hono route a remote client hits, end to end, with no transport stub.
    const response = await createDevApp().request('/__hotsheet/checkouts');
    expect(response.status).toBe(200);
    const payload = (await response.json()) as Checkout[];
    expect(payload.some((checkout) => checkout.id === session.id)).toBe(true);
    // The route must forward the server's real wire shape, not a reshaped convenience body.
    expect(payload).toEqual(listed);
  }, 120_000);

  it('keeps ticket sources with their project and lists machine-wide accounts (HS2-SM9PM8)', async () => {
    const app = createDevApp(),
      roots = [await mkdtemp(join(workspace, 'owner-')), await mkdtemp(join(workspace, 'other-'))],
      [owner, other] = await Promise.all(
        roots.map(async (root) => openLocalProject(root, await createLocalGitTicketStore(root))),
      ),
      api = (project: string, path: string, init?: RequestInit) =>
        app.request(`/__hotsheet/project-api/${encodeURIComponent(project)}${path}`, init);
    // The bridge scopes the project's create to its checkout: record and link in one request.
    const created = await api(owner.id, '/provider-connections', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({
        id: '',
        provider: 'github',
        locator: 'acme/procurement',
        name: 'Procurement issues',
        default: false,
        settings: { credential: { secret: 'github-app-01live' } },
        make_default: true,
      }),
    });
    expect(created.status).toBe(201);
    const connection = (await created.json()) as ProviderConnection;
    expect(connection.id).toBe('github-acme-procurement');
    expect(connection.projects?.map((project) => project.id)).toEqual([owner.id]);
    // Only the owner sees it; the other project gets neither the record nor a catalog to attach from.
    const own = (await (await api(owner.id, '/provider-connections')).json()) as ProviderConnection[];
    expect(own.map((item) => item.id)).toEqual([connection.id]);
    expect(await (await api(other.id, '/provider-connections')).json()).toEqual([]);
    const hijack = await api(other.id, `/provider-connections/${connection.id}/disabled`, {
      method: 'PUT',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ disabled: true }),
    });
    expect(hijack.status).toBe(404);
    // Accounts are machine-wide and name the owning project.
    const accounts = (await (await api(other.id, '/accounts')).json()) as ProviderAccount[];
    expect(accounts).toEqual([
      expect.objectContaining({
        id: 'github-app-01live',
        host: 'github.com',
        projects: [expect.objectContaining({ id: owner.id })],
      }),
    ]);
    const refused = await api(other.id, '/accounts/github-app-01live', { method: 'DELETE' });
    expect(refused.status).toBe(409);
    // Removing it from its only project deletes the connection; the account outlives it.
    const removed = await api(owner.id, `/checkouts/${owner.id}/sources/${connection.id}`, { method: 'DELETE' });
    expect(removed.status).toBe(200);
    expect(((await removed.json()) as SourceDetach).removed_connection).toBe(true);
    expect(await (await api(owner.id, '/provider-connections')).json()).toEqual([]);
    expect(await (await api(owner.id, '/accounts')).json()).toEqual([]);
  }, 120_000);

  it('reports the host of an unused Enterprise sign-in and seeds a reusing source (HS2-16MYXN)', async () => {
    const app = createDevApp(),
      root = await mkdtemp(join(workspace, 'enterprise-')),
      opened = await openLocalProject(root, await createLocalGitTicketStore(root)),
      api = (path: string, init?: RequestInit) =>
        app.request(`/__hotsheet/project-api/${encodeURIComponent(opened.id)}${path}`, init);
    // What the device flow records beside the keychain entry: the name and its non-secret site.
    await writeFile(
      join(home, 'keys.json'),
      JSON.stringify({
        'github-app-01ghe': {
          provider: 'github-app-01ghe',
          env: 'HOTSHEET_API_KEY_GITHUB_APP_01GHE',
          site: 'https://ghe.corp.test',
        },
      }),
    );
    const accounts = (await (await api('/accounts')).json()) as ProviderAccount[];
    expect(accounts).toEqual([
      expect.objectContaining({
        id: 'github-app-01ghe',
        host: 'ghe.corp.test',
        base_url: 'https://ghe.corp.test/api/v3',
        sources: [],
      }),
    ]);
    // An older client that omits api_base still gets the Enterprise server's.
    const created = await api('/provider-connections', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({
        id: '',
        provider: 'github',
        locator: 'corp/app',
        name: 'Corp issues',
        default: false,
        settings: { credential: { secret: 'github-app-01ghe' } },
      }),
    });
    expect(created.status).toBe(201);
    expect(((await created.json()) as ProviderConnection).settings.api_base).toBe('https://ghe.corp.test/api/v3');
    await writeFile(join(home, 'keys.json'), '{}');
  }, 120_000);

  it('reports what a reusing GitLab or Jira source needs (HS2-F5HNJN)', async () => {
    const app = createDevApp(),
      root = await mkdtemp(join(workspace, 'jira-')),
      opened = await openLocalProject(root, await createLocalGitTicketStore(root)),
      api = (path: string, init?: RequestInit) =>
        app.request(`/__hotsheet/project-api/${encodeURIComponent(opened.id)}${path}`, init),
      create = (connection: Record<string, unknown>) =>
        api('/provider-connections', {
          method: 'POST',
          headers: { 'content-type': 'application/json' },
          body: JSON.stringify({ id: '', name: null, default: false, ...connection }),
        });
    expect(
      (
        await create({
          provider: 'jira',
          locator: 'ENG',
          settings: {
            credential: { secret: 'jira-live-token' },
            email: 'dev@acme.test',
            base_url: 'https://acme.atlassian.net',
          },
        })
      ).status,
    ).toBe(201);
    expect(
      (
        await create({
          provider: 'gitlab',
          locator: 'team/app',
          settings: { credential: { secret: 'gitlab-live-token' }, api_base: 'https://gitlab.corp.test/api/v4' },
        })
      ).status,
    ).toBe(201);
    const accounts = (await (await api('/accounts')).json()) as ProviderAccount[];
    expect(accounts).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          id: 'gitlab-live-token',
          provider: 'gitlab',
          host: 'gitlab.corp.test',
          base_url: 'https://gitlab.corp.test/api/v4',
        }),
        expect.objectContaining({
          id: 'jira-live-token',
          provider: 'jira',
          host: 'acme.atlassian.net',
          base_url: 'https://acme.atlassian.net',
          identity: 'dev@acme.test',
        }),
      ]),
    );
  }, 120_000);
});
