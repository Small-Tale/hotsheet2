import { execFile } from 'node:child_process';
import { promisify } from 'node:util';

import { expect, type Page, test } from '@playwright/test';

import { withoutGitRepositoryEnv } from '../scripts/repository-env.mjs';
import { proxyFetch, realTicketServer } from './real-ticket-server';

const execute = promisify(execFile);

/**
 * HS2-N8H97X: provisional assignments in the real Workers view, on the shared TicketRow orchestration
 * line. The pool and its worker are real; reserving provisional work needs a prerequisite's CI on a
 * provider, so the pool's speculation status and the worker's assignment are added to the real worker
 * answer in the shape `coordination workers` gives (HS2-2SJPCH). Synthetic tickets only.
 */
test('HS2-N8H97X a worker’s provisional and at-risk tickets say so on their cards', async ({ page }) => {
  test.setTimeout(150_000);
  const workerId = 'synthetic-provisional-worker';
  const server = await realTicketServer({
    seed: async ({ store, root, env, cli }) => {
      const cleanEnv = withoutGitRepositoryEnv(env);
      const git = (...args: string[]) => execute('git', args, { cwd: root, env: cleanEnv });
      const pool = (...args: string[]) =>
        execute(cli, ['-C', store, 'coordination', '--project', root, ...args], { cwd: root, env: cleanEnv });
      await git('init', '-q', '-b', 'main');
      await git('commit', '-q', '--allow-empty', '-m', 'Synthetic base');
      await pool('setup');
      await pool('register', '--role', 'worker', '--id', workerId, '--session', 'w1', '--route', 'human');
    },
  });
  try {
    type Ticket = { id: string; slug: string; connection_id: string };
    const base = await server.request<Ticket>('/tickets', 'POST', { title: 'Add the synthetic token endpoint' });
    const built = await server.request<Ticket>('/tickets', 'POST', { title: 'Add the synthetic login page' });
    const risky = await server.request<Ticket>('/tickets', 'POST', { title: 'Add the synthetic audit log' });
    // The Workers view shows a worker's started tickets.
    for (const ticket of [built, risky]) await server.request(`/tickets/${ticket.id}`, 'PATCH', { status: 'started' });
    const apiPath = `/__hotsheet/project-api/${server.checkoutId}`;
    await page.routeWebSocket(/.*/, (route) => {
      void route.close();
    });
    await page.route(/\/__hotsheet\/(?:remote\/)?projects\/open$/, (route) =>
      route.fulfill({
        status: 201,
        json: { id: server.checkoutId, root: server.root, name: 'Synthetic Workers', stores: [server.store], apiPath },
      }),
    );
    await page.route(`**${apiPath}/**`, async (route) => {
      const incoming = new URL(route.request().url());
      try {
        const response = await proxyFetch(route, {
          url: `${server.url}${incoming.pathname.slice(apiPath.length)}${incoming.search}`,
          headers: { ...route.request().headers(), 'X-Hotsheet-Secret': server.secret },
          timeout: 60_000,
        });
        await route.fulfill({ response });
      } catch {
        await route.abort().catch(() => undefined);
      }
    });
    const assignment = (ticket: Ticket, id: string) => ({
      ticket: { connection_id: ticket.connection_id, native_id: ticket.id },
      slug: ticket.slug,
      assignment_id: id,
      assignment_phase: 'accepted',
      claim_expires_at: null,
      started_phase: null,
      eta_at: null,
    });
    const gate = (result: string) => ({
      blocker: base.id,
      slug: base.slug,
      commit: 'a'.repeat(40),
      result,
      run: 'run-7',
    });
    const record = (ticket: Ticket, id: string, state: string, result: string) => ({
      assignment: id,
      ticket: { connection_id: ticket.connection_id, native_id: ticket.id },
      slug: ticket.slug,
      worker: workerId,
      base: 'b'.repeat(40),
      depth: 1,
      state,
      gates: [gate(result)],
      explain: `provisional on ${base.slug} CI run-7`,
    });
    const original = await server.request<{ workers: Array<Record<string, unknown>> }>(
      `/checkouts/${server.checkoutId}/workers?wait_ms=0`,
    );
    const snapshot = {
      ...original,
      workers: original.workers.map((worker) =>
        worker.id === workerId
          ? { ...worker, tickets: [assignment(built, 'assign-1'), assignment(risky, 'assign-2')] }
          : worker,
      ),
      speculation: {
        records: [record(risky, 'assign-2', 'at_risk', 'red'), record(built, 'assign-1', 'provisional', 'pending')],
        repair_first: [base.slug],
        reevaluate: [],
      },
    };
    await page.route(`**${apiPath}/checkouts/${server.checkoutId}/workers?*`, async (route) => {
      // A cursor read is the long poll: it answers when something changes, which nothing does here.
      if (new URL(route.request().url()).searchParams.has('after'))
        await new Promise((resolve) => setTimeout(resolve, 20_000));
      await route.fulfill({ json: snapshot }).catch(() => undefined);
    });
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto('/');
    await page.getByRole('button', { name: 'Open Project' }).click();
    await page.getByRole('button', { name: 'Local Project' }).click();
    await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
    await page.locator('[data-segment-value="workers"]').first().click();
    const column = page.locator(`[data-worker-column="${workerId}"]`);
    const line = (ticket: Ticket) =>
      column.locator(`[data-worker-ticket="${ticket.connection_id}:${ticket.id}"] [data-orchestration-part="state"]`);
    await expect(line(built)).toHaveText(`Provisional on ${base.slug} · CI running`);
    await expect(line(risky)).toHaveText(`At risk · repair ${base.slug} first`);
    const shot = async (name: string) => {
      // The phone layout replaces the desktop one after a resize; capture once it has, with nothing sliding.
      await page.waitForFunction(
        () =>
          document.documentElement.scrollWidth <= window.innerWidth + 1 &&
          document
            .getAnimations()
            .every(
              (animation) => animation.playState !== 'running' || animation.effect?.getTiming().iterations === Infinity,
            ),
      );
      await page.screenshot({ path: test.info().outputPath(`hs2-n8h97x-${name}.png`) });
    };
    const pass = async (narrow: (page: Page) => Promise<void>) => {
      for (const [width, height] of [
        [1440, 900],
        [1100, 820],
        [390, 844],
      ] as const) {
        await page.setViewportSize({ width, height });
        if (width === 390) await narrow(page);
        await expect(line(risky)).toBeVisible();
        await shot(`workers-${width}`);
      }
      await page.setViewportSize({ width: 1440, height: 900 });
      await expect(line(built)).toBeVisible();
      await shot('workers-1440-after-narrow');
      await page.emulateMedia({ colorScheme: 'dark' });
      await shot('workers-1440-dark');
      await page.setViewportSize({ width: 390, height: 844 });
      await narrow(page);
      await shot('workers-390-dark');
      await page.emulateMedia({ colorScheme: 'light' });
    };
    await pass(async () => {
      await expect(page.locator('[data-worker-column]').first()).toBeInViewport();
    });
  } finally {
    await server.stop(page);
  }
});
