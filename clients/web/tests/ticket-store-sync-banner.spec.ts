import { execFile } from 'node:child_process';
import { chmod, rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { promisify } from 'node:util';

import { expect, test } from '@playwright/test';
import WebSocket from 'ws';

import { withoutGitRepositoryEnv } from '../scripts/repository-env.mjs';
import { openMockedProject } from './open-mocked-project';
import { proxyFetch, realTicketServer, routeProjectChangeSocket } from './real-ticket-server';

const run = promisify(execFile);

// HS2-51XDAZ: the ticket-store sync strip (HS2-ADHFKZ) against a real server, a real Git store,
// and a real bare remote whose pre-receive hook refuses pushes while a flag file exists. A ticket
// write kicks the server's sync loop; the refused push is announced as a `ticket_store_sync`
// event, and only that event (never a timer) makes the shell re-read the store's sync health.
// Synthetic data only.
test('raises and clears the ticket store sync banner from real sync events (HS2-51XDAZ)', async ({ page }) => {
  test.setTimeout(180_000);
  let refuse = '';
  let syncNow: () => Promise<unknown> = () => Promise.resolve();
  const server = await realTicketServer({
    seed: async ({ store, root, env, cli }) => {
      const remote = join(root, 'remote.git'),
        hook = join(remote, 'hooks', 'pre-receive'),
        git = (args: string[]) => run('git', args, { env: withoutGitRepositoryEnv(env) });
      refuse = join(root, 'refuse-pushes');
      // The strip's next step: fix the cause, then run `hotsheet-cli sync`.
      syncNow = () => run(cli, ['-C', store, 'sync'], { env: withoutGitRepositoryEnv(env) });
      await git(['init', '--bare', '-q', remote]);
      await writeFile(
        hook,
        `#!/bin/sh\nif [ -e "${refuse}" ]; then echo "test remote refuses pushes" >&2; exit 1; fi\nexit 0\n`,
      );
      await chmod(hook, 0o755);
      await git(['-C', store, 'remote', 'add', 'origin', remote]);
    },
  });
  // Every health read the page makes, and every sync event the server pushes, in arrival order.
  const timeline: Array<'read' | 'event'> = [];
  const events = new WebSocket(
    `${server.url.replace(/^http/, 'ws')}/ws/sync?secret=${server.secret}&checkout=${server.checkoutId}`,
  );
  events.on('message', (data: Buffer) => {
    if (data.toString('utf8').includes('"ticket_store_sync"')) timeline.push('event');
  });
  await new Promise((resolve, reject) => {
    events.once('open', resolve);
    events.once('error', reject);
  });
  try {
    const apiPath = `/__hotsheet/project-api/${server.checkoutId}`;
    await page.route(/\/__hotsheet\/(?:remote\/)?projects\/open$/, (route) =>
      route.fulfill({
        status: 201,
        json: { id: server.checkoutId, root: server.root, name: 'sync', stores: [server.store], apiPath },
      }),
    );
    await page.route(`**${apiPath}/**`, async (route) => {
      const incoming = new URL(route.request().url()),
        path = incoming.pathname.slice(apiPath.length);
      if (path.endsWith('/repository/sync')) timeline.push('read');
      try {
        const response = await proxyFetch(route, {
          url: `${server.url}${path}${incoming.search}`,
          headers: { ...route.request().headers(), 'X-Hotsheet-Secret': server.secret },
          timeout: 60_000,
        });
        await route.fulfill({ response });
      } catch {
        await route.abort().catch(() => undefined);
      }
    });
    await routeProjectChangeSocket(page, server, server.checkoutId);
    await page.setViewportSize({ width: 1280, height: 844 });
    await page.goto('/');
    await openMockedProject(page);
    await expect.poll(() => timeline.includes('read'), { timeout: 30_000 }).toBe(true);
    const banner = page.locator('[data-component="ticket-store-sync-banner"]');
    await expect(banner).toHaveCount(0);
    // The first write publishes to the accepting remote.
    const state = async () =>
      (
        await server.request<{ stores: Array<{ health: { state: string } }> }>(
          `/checkouts/${server.checkoutId}/repository/sync`,
        )
      ).stores[0]?.health.state;
    await server.request('/tickets', 'POST', { title: 'Written while the remote accepts' });
    await expect.poll(state, { timeout: 60_000 }).toBe('synced');
    await expect(banner).toHaveCount(0);

    // The remote starts refusing; a ticket write kicks a sync pass, whose refused push raises the strip.
    await writeFile(refuse, '');
    await server.request('/tickets', 'POST', { title: 'Written while the remote refuses' });
    await expect(banner).toBeVisible({ timeout: 60_000 });
    await expect(banner).toContainText(/Ticket store \S+ is not publishing changes/);
    await expect(banner).toContainText('The remote rejected the push.');
    await expect(banner).toContainText('Tickets are saved on this computer, but');
    await expect(banner).toContainText('not published.');

    // Wide, medium, phone, then wide again: the strip stays visible and inside the viewport.
    for (const [pass, width] of [1280, 900, 390, 1280].entries()) {
      await page.setViewportSize({ width, height: 844 });
      await expect(banner).toBeVisible();
      await expect
        .poll(async () => {
          const box = await banner.boundingBox();
          return box !== null && box.width > 0 && box.x >= 0 && box.x + box.width <= width + 0.5;
        })
        .toBe(true);
      await banner.screenshot({
        path: `target/visual-captures/hs2-51xdaz-sync-banner-${pass === 3 ? 'return-' : ''}${width}.png`,
      });
    }

    // The remote accepts again. The refused push backed the server's loop off, so (as the strip
    // says) a person runs `hotsheet-cli sync`, which publishes, and the strip clears.
    await rm(refuse);
    await syncNow();
    await expect(banner).toHaveCount(0, { timeout: 20_000 });

    // No timer: after the first read, each health read follows a sync event received since the
    // previous read.
    const reads = timeline.flatMap((entry, index) => (entry === 'read' ? [index] : []));
    expect(reads.length).toBeGreaterThanOrEqual(3);
    for (const [position, index] of reads.entries()) {
      if (position === 0) continue;
      const previous = reads[position - 1] ?? 0;
      expect(timeline.slice(previous + 1, index), `read ${position} was not preceded by a sync event`).toContain(
        'event',
      );
    }
  } finally {
    events.close();
    await server.stop(page);
  }
});
