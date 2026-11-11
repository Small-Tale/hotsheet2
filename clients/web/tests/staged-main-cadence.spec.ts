import { execFile } from 'node:child_process';
import { promisify } from 'node:util';

import { expect, test } from '@playwright/test';

import { withoutGitRepositoryEnv } from '../scripts/repository-env.mjs';
import { realTicketServer } from './real-ticket-server';

const execute = promisify(execFile);

test('HS2-0BJ1A3 rebuilt Workers view shows the real source cadence across sizes and policy replay', async ({
  page,
}) => {
  test.setTimeout(120_000);
  let updateGoal: (() => Promise<void>) | undefined;
  const server = await realTicketServer({
    seed: async ({ store, root, env, cli }) => {
      const cleanEnv = withoutGitRepositoryEnv(env);
      const git = (...args: string[]) => execute('git', args, { cwd: root, env: cleanEnv });
      const pool = (...args: string[]) =>
        execute(cli, ['-C', store, 'coordination', '--project', root, ...args], { cwd: root, env: cleanEnv });
      await git('init', '-q', '-b', 'main');
      await git('commit', '-q', '--allow-empty', '-m', 'Synthetic base');
      await pool('setup');
      await pool('strategy-allow', '--strategy', 'ci-staged', '--reason', 'synthetic cadence');
      await pool('development-strategy-set', '--preset', 'steady-main');
      updateGoal = async () => {
        await pool('development-strategy-set', '--preset', 'steady-main', '--green-main-goal-minutes', '90');
      };
    },
  });
  try {
    const apiPath = `/__hotsheet/project-api/${server.checkoutId}`;
    await page.routeWebSocket(/.*/, (route) => {
      void route.close();
    });
    await page.route(/\/__hotsheet\/(?:remote\/)?projects\/open$/, (route) =>
      route.fulfill({
        status: 201,
        json: { id: server.checkoutId, root: server.root, name: 'Synthetic Cadence', stores: [server.store], apiPath },
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
    const view = () =>
      server.request<{ cadence: { goal_minutes: number; last_green_main_at: number | null } }>(
        `/checkouts/${server.checkoutId}/workers?wait_ms=0`,
      );
    expect((await view()).cadence).toMatchObject({ goal_minutes: 360, last_green_main_at: null });
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto('/');
    await page.getByRole('button', { name: 'Open Project' }).click();
    await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
    await page.locator('[data-segment-value="workers"]').first().click();
    const cadence = page.locator('[data-cadence-source]');
    await expect(cadence).toContainText('Green main goal · 360 min');
    await page.screenshot({ path: test.info().outputPath('cadence-light-wide.png') });
    await updateGoal!();
    await expect.poll(async () => (await view()).cadence.goal_minutes).toBe(90);
    await expect(cadence).toContainText('Green main goal · 90 min', { timeout: 30_000 });
    await page.setViewportSize({ width: 1024, height: 750 });
    await cadence.scrollIntoViewIfNeeded();
    const fits = await cadence.evaluate((root) => {
      const right = root.getBoundingClientRect().right;
      return [...root.children].every((child) => child.getBoundingClientRect().right <= right + 1);
    });
    expect(fits).toBe(true);
    await page.screenshot({ path: test.info().outputPath('cadence-light-medium.png') });
    await page.emulateMedia({ colorScheme: 'dark' });
    await page.setViewportSize({ width: 390, height: 760 });
    await page.locator('[data-segment-value="workers"]').first().click();
    await cadence.scrollIntoViewIfNeeded();
    await page.waitForTimeout(500);
    expect(
      await page.evaluate(() => {
        const rect = document.querySelector('[data-cadence-source]')?.getBoundingClientRect();
        return (
          rect &&
          window.scrollX === 0 &&
          document.documentElement.scrollWidth === window.innerWidth &&
          rect.left >= 0 &&
          rect.right <= window.innerWidth
        );
      }),
    ).toBe(true);
    await page.screenshot({ path: test.info().outputPath('cadence-dark-phone.png') });
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.locator('[data-segment-value="workers"]').first().click();
    await cadence.scrollIntoViewIfNeeded();
    await page.waitForTimeout(500);
    expect(
      await page.evaluate(() => {
        const rect = document.querySelector('[data-cadence-source]')?.getBoundingClientRect();
        return (
          rect &&
          window.scrollX === 0 &&
          document.documentElement.scrollWidth === window.innerWidth &&
          rect.left >= 0 &&
          rect.right <= window.innerWidth
        );
      }),
    ).toBe(true);
    await page.screenshot({ path: test.info().outputPath('cadence-dark-returned-wide.png') });
  } finally {
    await server.stop();
  }
});
