import { spawnSync } from 'node:child_process';

import { expect, type Page, test } from '@playwright/test';

import { withoutGitRepositoryEnv } from '../scripts/repository-env.mjs';
import { realTicketServer } from './real-ticket-server';

type Server = Awaited<ReturnType<typeof realTicketServer>>;

async function openProject(page: Page, server: Server) {
  const apiPath = `/__hotsheet/project-api/${server.checkoutId}`;
  await page.route(/\/__hotsheet\/(?:remote\/)?projects\/open$/, (route) =>
    route.fulfill({
      status: 201,
      json: { id: server.checkoutId, root: server.root, name: 'synthetic', stores: [server.store], apiPath },
    }),
  );
  await page.route(`**${apiPath}/**`, async (route) => {
    const incoming = new URL(route.request().url());
    const response = await route.fetch({
      url: `${server.url}${incoming.pathname.slice(apiPath.length)}${incoming.search}`,
      headers: { ...route.request().headers(), 'X-Hotsheet-Secret': server.secret },
      timeout: 60_000,
    });
    await route.fulfill({ response });
  });
  await page.goto('/');
  await page.getByRole('button', { name: 'Open Project' }).click();
  await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
  await page.getByRole('button', { name: 'Workers' }).click();
}

test('HS2-D5KXBA removes completed worker cards and counts, fades them out (HS2-PM42ZM), then refills on Started', async ({
  page,
}) => {
  test.setTimeout(180_000);
  let fixtureCli = '';
  let fixtureEnv: NodeJS.ProcessEnv = {};
  const server = await realTicketServer({
    seed: async ({ root, env, cli }) => {
      fixtureCli = cli;
      fixtureEnv = env;
      const initialized = spawnSync('git', ['-C', root, 'init'], {
        env: withoutGitRepositoryEnv(env),
        encoding: 'utf8',
      });
      expect(initialized.status, initialized.stderr).toBe(0);
      const committed = spawnSync(
        'git',
        [
          '-C',
          root,
          '-c',
          'user.name=Fixture',
          '-c',
          'user.email=fixture@example.invalid',
          'commit',
          '--allow-empty',
          '-m',
          'Synthetic checkout',
        ],
        { env: withoutGitRepositoryEnv(env), encoding: 'utf8' },
      );
      expect(committed.status, committed.stderr).toBe(0);
    },
  });
  try {
    const run = (...args: string[]) => {
      const result = spawnSync(fixtureCli, ['-C', server.store, 'coordination', '--project', server.root, ...args], {
        cwd: server.root,
        env: fixtureEnv,
        encoding: 'utf8',
      });
      expect(result.status, `${args.join(' ')}: ${result.stderr}`).toBe(0);
    };
    run('setup');
    run(
      'register',
      '--role',
      'worker',
      '--id',
      'synthetic-worker',
      '--session',
      'one',
      '--route',
      'human',
      '--lease-seconds',
      '3600',
    );
    const ticket = await server.request<{ id: string; slug: string }>('/tickets', 'POST', {
      title: 'Synthetic active work',
      category: 'task',
    });
    await server.request(`/tickets/${ticket.id}/claim`, 'POST', { worker: 'synthetic-worker', lease_minutes: 30 });
    await page.routeWebSocket(/.*/, (route) => {
      void route.close();
    });
    await page.setViewportSize({ width: 1440, height: 844 });
    await openProject(page, server);
    const column = page.locator('[data-worker-column="synthetic-worker"]');
    await expect(column).toBeVisible({ timeout: 30_000 });
    await expect(column.locator('[data-worker-ticket]')).toHaveCount(1, { timeout: 30_000 });
    await expect(column).toContainText('Working on · 1');

    // HS2-PM42ZM: a card completed while the Workers view is visible fades out through the shared
    // ticket motion instead of vanishing; reduced motion removes it directly.
    await page.emulateMedia({ reducedMotion: 'no-preference' });
    await page.evaluate(() => {
      // eslint-disable-next-line @typescript-eslint/unbound-method -- HS2-PM42ZM records the real animate call
      const original = Element.prototype.animate;
      const records: Array<{ ghost: string; slug: string; fades: boolean }> = [];
      (window as typeof window & { __workerMotion?: typeof records }).__workerMotion = records;
      Element.prototype.animate = function (keyframes, options) {
        const animation = original.call(this, keyframes, options),
          element = this as HTMLElement;
        if (element.dataset.ticketMotionGhost)
          records.push({
            ghost: element.dataset.ticketMotionGhost,
            slug: element.dataset.ticketMotionSlug ?? '',
            fades: (animation.effect as KeyframeEffect).getKeyframes().some((frame) => Number(frame.opacity) === 0),
          });
        return animation;
      };
    });
    const motion = () =>
      page.evaluate(
        () =>
          (window as typeof window & { __workerMotion?: Array<{ ghost: string; slug: string; fades: boolean }> })
            .__workerMotion ?? [],
      );
    await server.request(`/tickets/${ticket.id}`, 'PATCH', { status: 'completed' });
    await expect(column.locator('[data-worker-ticket]')).toHaveCount(0, { timeout: 30_000 });
    await expect.poll(motion).toContainEqual({ ghost: 'outgoing', slug: ticket.slug, fades: true });
    await expect(page.locator('[data-ticket-motion-ghost]')).toHaveCount(0);
    await expect(column).toContainText('No assigned or planned tickets');
    await expect(column).not.toContainText('Working on · 1');

    await server.request(`/tickets/${ticket.id}`, 'PATCH', { status: 'started' });
    await server.request(`/tickets/${ticket.id}/claim`, 'POST', { worker: 'synthetic-worker', lease_minutes: 30 });
    await expect(column.locator('[data-worker-ticket]')).toHaveCount(1, { timeout: 30_000 });
    await expect(column).toContainText('Working on · 1');

    await page.emulateMedia({ reducedMotion: 'reduce' });
    await page.evaluate(() => {
      (window as typeof window & { __workerMotion?: unknown[] }).__workerMotion?.splice(0);
    });
    await server.request(`/tickets/${ticket.id}`, 'PATCH', { status: 'completed' });
    await expect(column.locator('[data-worker-ticket]')).toHaveCount(0, { timeout: 30_000 });
    expect(await motion()).toEqual([]);
    for (const scheme of ['light', 'dark'] as const) {
      await page.emulateMedia({ colorScheme: scheme, reducedMotion: 'reduce' });
      for (const [name, width] of [
        ['wide', 1440],
        ['medium', 900],
        ['phone', 390],
        ['returned-wide', 1440],
      ] as const) {
        await page.setViewportSize({ width, height: 844 });
        await column.scrollIntoViewIfNeeded();
        await expect(column).toBeInViewport();
        await expect(column).toContainText('No assigned or planned tickets');
        await page.screenshot({
          path: test.info().outputPath(`hs2-d5kxba-${scheme}-${name}.png`),
          animations: 'disabled',
        });
      }
    }
  } finally {
    await page.goto('about:blank').catch(() => undefined);
    await page.unrouteAll({ behavior: 'ignoreErrors' });
    await server.stop();
  }
});
