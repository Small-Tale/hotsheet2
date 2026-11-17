import { execFile } from 'node:child_process';
import { promisify } from 'node:util';

import { expect, type Page, test } from '@playwright/test';

import { withoutGitRepositoryEnv } from '../scripts/repository-env.mjs';
import { fakeGithubActions } from './fake-github-actions';
import { openMockedProject } from './open-mocked-project';
import { proxyFetch, realTicketServer } from './real-ticket-server';

const execute = promisify(execFile);
const REPO = 'acme/synthetic-widgets';

/**
 * HS2-N8H97X, HS2-8KV14Y: provisional work in the real Workers view, on the shared TicketRow orchestration
 * line, end to end. The pool, its integrator and worker, the provisional reservation and the CI gate are all
 * real: the prerequisite's commit is watched by `job watch ci` through the real GitHub Actions client
 * against a loopback fake provider, its run fails, and `coordination speculation-sync` records the red
 * result (§5.9.1). Nothing is added to the worker answer. Synthetic tickets and commits only.
 */
test('HS2-N8H97X a worker’s provisional ticket turns at risk when its base gate goes red', async ({ page }) => {
  test.setTimeout(180_000);
  const workerId = 'synthetic-provisional-worker';
  const provider = await fakeGithubActions();
  let commit = '';
  const server = await realTicketServer({
    env: provider.env(),
    seed: async ({ store, root, env: inherited, cli }) => {
      const env = withoutGitRepositoryEnv(inherited);
      const git = (...args: string[]) => execute('git', args, { cwd: root, env });
      const pool = (...args: string[]) =>
        execute(cli, ['-C', store, 'coordination', '--project', root, ...args], { cwd: root, env });
      await git('init', '-q', '-b', 'main');
      await git('commit', '-q', '--allow-empty', '-m', 'Synthetic base');
      commit = (await git('rev-parse', 'HEAD')).stdout.trim();
      await pool('setup');
      for (const [role, id, session] of [
        ['integrator', 'integrator', 'root'],
        ['worker', workerId, 'w1'],
      ])
        await pool('register', '--role', role, '--id', id, '--session', session, '--lease-seconds', '600');
      await pool('speculation-policy', '--enable', '--base-branch', 'main');
    },
  });
  const cli = (...args: string[]) =>
    execute(server.cli, ['-C', server.store, ...args], { cwd: server.root, env: server.env });
  const pool = (...args: string[]) => cli('coordination', '--project', server.root, ...args);
  const watch = ['--commit', commit, '--repo', REPO, '--checkout', server.root];
  try {
    type Ticket = { id: string; slug: string; connection_id: string };
    const base = await server.request<Ticket>('/tickets', 'POST', { title: 'Add the synthetic token endpoint' });
    // The prerequisite is pushed and waits on CI (`final_testing` with its `CI gate commit`).
    await server.request(`/tickets/${base.id}`, 'PATCH', {
      status: 'started',
      started_phase: 'final_testing',
      note: `Pushed; waiting on CI.\n\nCI gate commit: ${commit}\n`,
    });
    const built = await server.request<Ticket>('/tickets', 'POST', {
      title: 'Add the synthetic login page',
      up_next: true,
      blocked_by: [base.id],
    });
    // The integrator reserves the dependent ahead of the pending gate: a real provisional record.
    await pool(
      'assign',
      '--id',
      'integrator',
      '--session',
      'root',
      '--worker',
      workerId,
      '--worker-session',
      'w1',
      '--operation',
      'op-built',
      '--connection',
      built.connection_id,
      '--native-id',
      built.id,
      '--slug',
      built.slug,
      '--base',
      commit,
    );
    // The worker claims the offered ticket and accepts the assignment, as its role skill does.
    await cli('claim', built.slug, '--worker', workerId, '--eta', '1h');
    await pool('worker', '--id', workerId, '--session', 'w1', '--assignment', 'op-built', '--phase', 'accepted');
    // The prerequisite's CI run starts; the real watch reads it as pending.
    provider.run(REPO, { id: 7, sha: commit, status: 'in_progress' });
    await cli('job', 'watch', 'ci', ...watch, '--poll-ms', '100');
    const gate = async (wait: string) =>
      JSON.parse((await cli('job', 'watch', 'gate', ...watch, '--wait-seconds', wait)).stdout) as { state: string };
    await expect.poll(async () => (await gate('5')).state, { timeout: 30_000 }).toBe('pending');
    // The integrator's sync reads the watch Job: the record's gate goes from unknown to pending.
    await pool('speculation-sync');

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
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto('/');
    await openMockedProject(page);
    await page.locator('[data-segment-value="workers"]').first().click();
    const column = page.locator(`[data-worker-column="${workerId}"]`);
    const line = column.locator(
      `[data-worker-ticket="${built.connection_id}:${built.id}"] [data-orchestration-part="state"]`,
    );
    await expect(line).toHaveText(`Provisional on ${base.slug} · CI running`);

    // The run fails. The watch records the red verdict, the sync applies it to the ledger, and the open
    // Workers view follows the pool's cursor to the at-risk line, with no reload.
    provider.run(REPO, { id: 7, sha: commit, status: 'completed', conclusion: 'failure' });
    await expect.poll(async () => (await gate('20')).state, { timeout: 60_000 }).toBe('red');
    const synced = JSON.parse((await pool('speculation-sync')).stdout) as {
      status: { repair_first: string[] };
    };
    expect(synced.status.repair_first).toContain(base.slug);
    await expect(line).toHaveText(`At risk · repair ${base.slug} first`, { timeout: 30_000 });
    // The watch only read the provider.
    expect(provider.requests.every((request) => request.startsWith('GET '))).toBe(true);

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
      await page.screenshot({ path: test.info().outputPath(`hs2-8kv14y-${name}.png`) });
    };
    const pass = async (narrow: (page: Page) => Promise<void>) => {
      for (const [width, height] of [
        [1440, 900],
        [1100, 820],
        [390, 844],
      ] as const) {
        await page.setViewportSize({ width, height });
        if (width === 390) await narrow(page);
        await expect(line).toBeVisible();
        await shot(`workers-at-risk-${String(width)}`);
      }
      await page.setViewportSize({ width: 1440, height: 900 });
      await expect(line).toBeVisible();
      await shot('workers-at-risk-1440-after-narrow');
      await page.emulateMedia({ colorScheme: 'dark' });
      await shot('workers-at-risk-1440-dark');
      await page.emulateMedia({ colorScheme: 'light' });
    };
    // A phone pages the worker columns; bring the worker's column, and so its card, on screen.
    await pass(async () => {
      await line.scrollIntoViewIfNeeded();
      await expect(line).toBeInViewport();
    });
  } finally {
    await cli('job', 'watch', 'stop').catch(() => undefined);
    await server.stop(page);
    await provider.close();
  }
});
