import { mkdtemp, readdir, readFile, realpath, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterAll, beforeAll, describe, expect, it } from 'vitest';

import type { Checkout } from './api';
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
        const info = JSON.parse(await readFile(join(dir, name), 'utf8')) as { pid?: number };
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
});
