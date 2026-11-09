import { type ChildProcess, execFile, spawn } from 'node:child_process';
import { once } from 'node:events';
import { promisify } from 'node:util';

import { expect, test } from '@playwright/test';

import { withoutGitRepositoryEnv } from '../scripts/repository-env.mjs';
import { realTicketServer } from './real-ticket-server';

const execute = promisify(execFile);

test('HS2-R0E39D crashed worker lease expires before a synthetic claim is taken over', async ({ page }) => {
  test.setTimeout(180_000);
  const first = 'synthetic-native-worker',
    second = 'synthetic-takeover-worker';
  let cli = '',
    root = '',
    store = '',
    env: NodeJS.ProcessEnv = {},
    slug = '';
  const server = await realTicketServer({
    seed: async (fixture) => {
      ({ cli, root, store } = fixture);
      env = withoutGitRepositoryEnv(fixture.env);
      const git = (...args: string[]) => execute('git', args, { cwd: root, env });
      const pool = (...args: string[]) =>
        execute(cli, ['-C', store, 'coordination', '--project', root, ...args], { cwd: root, env });
      await git('init', '-q', '-b', 'main');
      await git('commit', '-q', '--allow-empty', '-m', 'Synthetic base');
      const created = await execute(cli, ['-C', store, 'new', '--title', 'Synthetic worker claim'], { cwd: root, env });
      slug = /HS2-[A-Z0-9]+/.exec(created.stdout)?.[0] ?? '';
      expect(slug).toMatch(/^HS2-[A-Z0-9]+$/);
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
        '180',
      );
    },
  });
  let worker: ChildProcess | null = null;
  try {
    const pool = (...args: string[]) =>
      execute(cli, ['-C', store, 'coordination', '--project', root, ...args], { cwd: root, env });
    const ticket = (...args: string[]) => execute(cli, ['-C', store, ...args], { cwd: root, env });
    worker = spawn(
      cli,
      [
        '-C',
        store,
        'coordination',
        '--project',
        root,
        'next',
        '--role',
        'worker',
        '--id',
        first,
        '--session',
        'w1',
        '--route',
        'human',
        '--lease-seconds',
        '30',
      ],
      { cwd: root, env, stdio: ['ignore', 'pipe', 'pipe'] },
    );
    const ready = new Promise<void>((resolve, reject) => {
      const timer = setTimeout(() => {
        reject(new Error('synthetic worker did not register'));
      }, 15_000);
      const inspect = (chunk: Buffer) => {
        if (!chunk.toString().includes('Waiting on local assignment')) return;
        clearTimeout(timer);
        resolve();
      };
      worker?.stdout?.on('data', inspect);
      worker?.stderr?.on('data', inspect);
      worker?.once('exit', () => {
        clearTimeout(timer);
        reject(new Error('synthetic worker exited before its crash probe'));
      });
    });
    await ready;
    await ticket('claim', slug, '--worker', first, '--lease-minutes', '1', '--eta', '2m');

    const apiPath = `/__hotsheet/project-api/${server.checkoutId}`;
    await page.routeWebSocket(/.*/, (route) => {
      void route.close();
    });
    await page.route('**/__hotsheet/projects/open', (route) =>
      route.fulfill({
        status: 201,
        json: { id: server.checkoutId, root: server.root, name: 'Synthetic Workers', stores: [store], apiPath },
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
    const requests: string[] = [];
    page.on('request', (request) => {
      if (request.url().includes('/workers?')) requests.push(request.url());
    });
    const view = () =>
      server.request<{ workers: Array<{ id: string; availability: string }>; past_workers: number }>(
        `/checkouts/${server.checkoutId}/workers?wait_ms=0`,
      );
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto('/');
    await page.getByRole('button', { name: 'Open Project' }).click();
    await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
    await page.locator('[data-segment-value="workers"]').first().click();
    const firstCard = page.locator(`[data-worker-column="${first}"]`);
    await expect(firstCard).toContainText(slug);
    await expect(firstCard).toContainText('Availability unknown');
    expect((await view()).workers.find((item) => item.id === first)?.availability).toBe('live');
    const idleStart = requests.length;
    await page.waitForTimeout(1500);
    expect(requests.length - idleStart).toBeLessThanOrEqual(1);
    await page.screenshot({ path: test.info().outputPath('workers-native-wide-live.png') });

    const exited = once(worker, 'exit');
    worker.kill('SIGKILL');
    await exited;
    await expect
      .poll(async () => (await view()).workers.find((item) => item.id === first)?.availability, { timeout: 30_000 })
      .toBe('disconnected');
    await expect(firstCard).toContainText('Disconnected', { timeout: 30_000 });
    await page.setViewportSize({ width: 900, height: 750 });
    await page.waitForTimeout(500);
    await page.evaluate(() => {
      window.scrollTo(0, 0);
    });
    await firstCard.scrollIntoViewIfNeeded();
    await expect(firstCard).toBeInViewport();
    await page.screenshot({ path: test.info().outputPath('workers-native-medium-crashed.png') });

    await expect
      .poll(
        async () => {
          try {
            await ticket('claim', slug, '--worker', second, '--lease-minutes', '1', '--eta', '2m');
            return true;
          } catch {
            return false;
          }
        },
        { timeout: 75_000, intervals: [1000] },
      )
      .toBe(true);
    await pool(
      'register',
      '--role',
      'worker',
      '--id',
      second,
      '--session',
      'w2',
      '--route',
      'human',
      '--lease-seconds',
      '90',
    );
    const secondCard = page.locator(`[data-worker-column="${second}"]`);
    await expect(secondCard).toContainText(slug, { timeout: 30_000 });
    await expect(secondCard).toContainText('Availability unknown');
    await expect(firstCard).toHaveCount(0);
    const after = await view();
    expect(after.workers.find((item) => item.id === second)?.availability).toBe('live');
    expect(after.past_workers).toBeGreaterThan(0);
    await page.setViewportSize({ width: 390, height: 760 });
    await page.waitForTimeout(500);
    await page.evaluate(() => {
      window.scrollTo(0, 0);
    });
    await secondCard.scrollIntoViewIfNeeded();
    await expect(secondCard).toBeInViewport();
    await page.screenshot({ path: test.info().outputPath('workers-native-phone-takeover.png') });
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.waitForTimeout(500);
    await page.evaluate(() => {
      window.scrollTo(0, 0);
    });
    await page.screenshot({ path: test.info().outputPath('workers-native-returned-wide.png') });
    expect(requests.some((url) => new URL(url).searchParams.has('after'))).toBe(true);

    // A deliberate stop withdraws availability immediately while the worker's
    // still-live claim stays visible as an unresolved holder, not an available agent.
    await pool('leave', '--role', 'worker', '--id', second, '--session', 'w2');
    await expect
      .poll(async () => (await view()).workers.find((item) => item.id === second)?.availability)
      .toBe('unresolved_claim_holder');
    await expect(secondCard).toContainText('Unresolved claim holder');
    await expect(secondCard).not.toContainText('Available');
    await page.screenshot({ path: test.info().outputPath('workers-native-returned-wide-stopped.png') });
  } finally {
    if (worker?.pid && worker.exitCode === null && worker.signalCode === null) {
      const exited = once(worker, 'exit');
      worker.kill('SIGKILL');
      await exited;
    }
    await server.stop();
  }
});
