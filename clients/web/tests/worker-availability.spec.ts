import { execFile } from 'node:child_process';
import { promisify } from 'node:util';

import { expect, test } from '@playwright/test';

import { withoutGitRepositoryEnv } from '../scripts/repository-env.mjs';
import { realTicketServer } from './real-ticket-server';

const execute = promisify(execFile);

test('HS2-KX87JC replays a short worker lease into the rebuilt Workers view without idle polling', async ({ page }) => {
  test.setTimeout(120_000);
  const workerId = 'synthetic-availability-worker';
  const server = await realTicketServer({
    seed: async ({ store, root, env, cli }) => {
      const cleanEnv = withoutGitRepositoryEnv(env);
      const git = (...args: string[]) => execute('git', args, { cwd: root, env: cleanEnv });
      const pool = (...args: string[]) =>
        execute(cli, ['-C', store, 'coordination', '--project', root, ...args], { cwd: root, env: cleanEnv });
      await git('init', '-q', '-b', 'main');
      await git('commit', '-q', '--allow-empty', '-m', 'Synthetic base');
      await pool('setup');
      await pool(
        'register',
        '--role',
        'integrator',
        '--id',
        'synthetic-integrator',
        '--session',
        'i1',
        '--route',
        'human',
        '--lease-seconds',
        '60',
      );
      await pool(
        'register',
        '--role',
        'worker',
        '--id',
        workerId,
        '--session',
        'w1',
        '--route',
        'human',
        '--lease-seconds',
        '10',
      );
    },
  });
  try {
    const apiPath = `/__hotsheet/project-api/${server.checkoutId}`;
    await page.routeWebSocket(/.*/, (route) => {
      void route.close();
    });
    await page.route('**/__hotsheet/projects/open', (route) =>
      route.fulfill({
        status: 201,
        json: { id: server.checkoutId, root: server.root, name: 'Synthetic Workers', stores: [server.store], apiPath },
      }),
    );
    await page.route(`**${apiPath}/**`, async (route) => {
      const incoming = new URL(route.request().url());
      try {
        const response = await route.fetch({
          url: `${server.url}${incoming.pathname.slice(apiPath.length)}${incoming.search}`,
          headers: { ...route.request().headers(), 'X-Hotsheet-Secret': server.secret },
          timeout: 60_000,
        });
        await route.fulfill({ response });
      } catch {
        await route.abort().catch(() => undefined);
      }
    });
    const workerRequests: string[] = [];
    page.on('request', (request) => {
      if (request.url().includes('/workers?')) workerRequests.push(request.url());
    });
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto('/');
    await page.getByRole('button', { name: 'Open Project' }).click();
    await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
    await page.locator('[data-segment-value="workers"]').first().click();

    const column = page.locator(`[data-worker-column="${workerId}"]`);
    await expect(column).toBeVisible({ timeout: 30_000 });
    // A human return route has no observed terminal connection, so availability is unknown.
    await expect(column).toContainText('Availability unknown');
    const view = () =>
      server.request<{ workers: Array<{ id: string }>; past_workers: number }>(
        `/checkouts/${server.checkoutId}/workers?wait_ms=0`,
      );
    expect((await view()).workers.some((worker) => worker.id === workerId)).toBe(true);
    await page.screenshot({ path: test.info().outputPath('workers-wide-live.png') });
    const beforeIdle = workerRequests.length;
    await page.waitForTimeout(1500);
    expect(workerRequests.length - beforeIdle).toBeLessThanOrEqual(1);

    await expect.poll(async () => (await view()).past_workers, { timeout: 30_000 }).toBeGreaterThan(0);
    await expect(column).toHaveCount(0, { timeout: 35_000 });
    await page.setViewportSize({ width: 900, height: 750 });
    await page.waitForTimeout(500);
    await page.evaluate(() => {
      window.scrollTo(0, 0);
    });
    await page.screenshot({ path: test.info().outputPath('workers-medium-expired.png') });
    await page.setViewportSize({ width: 390, height: 760 });
    await page.waitForTimeout(500);
    await page.evaluate(() => {
      window.scrollTo(0, 0);
    });
    await page.screenshot({ path: test.info().outputPath('workers-phone-expired.png') });
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.waitForTimeout(500);
    await page.evaluate(() => {
      window.scrollTo(0, 0);
    });
    await page.screenshot({ path: test.info().outputPath('workers-returned-wide-expired.png') });
  } finally {
    await server.stop();
  }
});

test('HS2-KX87JC source failure must not leave a stale Available worker card', async ({ page }) => {
  test.setTimeout(90_000);
  const workerId = 'synthetic-availability-worker';
  const server = await realTicketServer({
    seed: async ({ store, root, env, cli }) => {
      const cleanEnv = withoutGitRepositoryEnv(env);
      const git = (...args: string[]) => execute('git', args, { cwd: root, env: cleanEnv });
      const pool = (...args: string[]) =>
        execute(cli, ['-C', store, 'coordination', '--project', root, ...args], { cwd: root, env: cleanEnv });
      await git('init', '-q', '-b', 'main');
      await git('commit', '-q', '--allow-empty', '-m', 'Synthetic base');
      await pool('setup');
      await pool(
        'register',
        '--role',
        'integrator',
        '--id',
        'synthetic-integrator',
        '--session',
        'i1',
        '--route',
        'human',
        '--lease-seconds',
        '3600',
      );
      await pool(
        'register',
        '--role',
        'worker',
        '--id',
        workerId,
        '--session',
        'w1',
        '--route',
        'human',
        '--lease-seconds',
        '3600',
      );
    },
  });
  try {
    const apiPath = `/__hotsheet/project-api/${server.checkoutId}`;
    await page.routeWebSocket(/.*/, (route) => {
      void route.close();
    });
    await page.route('**/__hotsheet/projects/open', (route) =>
      route.fulfill({
        status: 201,
        json: { id: server.checkoutId, root: server.root, name: 'Synthetic Workers', stores: [server.store], apiPath },
      }),
    );
    await page.route(`**${apiPath}/**`, async (route) => {
      const incoming = new URL(route.request().url());
      try {
        const response = await route.fetch({
          url: `${server.url}${incoming.pathname.slice(apiPath.length)}${incoming.search}`,
          headers: { ...route.request().headers(), 'X-Hotsheet-Secret': server.secret },
          timeout: 60_000,
        });
        await route.fulfill({ response });
      } catch {
        await route.abort().catch(() => undefined);
      }
    });
    const original = await server.request<{ workers: Array<Record<string, unknown>> }>(
      `/checkouts/${server.checkoutId}/workers?wait_ms=0`,
    );
    const snapshot = {
      ...original,
      workers: original.workers.map((worker) =>
        worker.id === workerId
          ? { ...worker, availability: 'live', connection_available: true, observed_alive: true, process_busy: false }
          : worker,
      ),
    };
    let failedAfter = false;
    let allowRecovery = false;
    await page.route(`**${apiPath}/checkouts/${server.checkoutId}/workers?*`, async (route) => {
      if (new URL(route.request().url()).searchParams.has('after')) {
        // Keep the initial live card observable, then fail the first cursor read.
        await new Promise((resolve) => setTimeout(resolve, failedAfter ? 3000 : 300));
        if (!failedAfter) {
          failedAfter = true;
          return route.fulfill({ status: 503, json: { error: 'synthetic source failure' } });
        }
      }
      while (failedAfter && !allowRecovery) await new Promise((resolve) => setTimeout(resolve, 50));
      return route.fulfill({ json: snapshot });
    });
    await page.goto('/');
    await page.getByRole('button', { name: 'Open Project' }).click();
    await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
    await page.locator('[data-segment-value="workers"]').first().click();
    const column = page.locator(`[data-worker-column="${workerId}"]`);
    await expect(column).toContainText('Available');
    await expect(page.locator('[data-worker-coverage-problems]')).toBeVisible();
    await expect(column).toHaveCount(0, { timeout: 2000 });
    await page.screenshot({ path: test.info().outputPath('workers-source-failure-without-stale-card.png') });
    // The backoff restarts without a cursor. A same-cursor snapshot must clear the warning.
    allowRecovery = true;
    await expect(column).toContainText('Available', { timeout: 10_000 });
    await expect(page.locator('[data-worker-coverage-problems]')).toHaveCount(0);
    await page.screenshot({ path: test.info().outputPath('workers-same-cursor-recovered.png') });
  } finally {
    await server.stop();
  }
});
