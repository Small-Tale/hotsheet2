import { execFile } from 'node:child_process';
import { mkdir, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { promisify } from 'node:util';

import { expect, test } from '@playwright/test';

import { withoutGitRepositoryEnv } from '../scripts/repository-env.mjs';
import { type RealTicketFixtureContext, realTicketServer } from './real-ticket-server';

const execute = promisify(execFile);

test('HS2-NBDXPW saves presets and Custom through the real project API, previews overrides, and keeps live cadence visible', async ({
  page,
}) => {
  test.setTimeout(180_000);
  const server = await realTicketServer({
    seed: async ({ store, root, env, cli }) => {
      const cleanEnv = withoutGitRepositoryEnv(env),
        git = (...args: string[]) => execute('git', args, { cwd: root, env: cleanEnv }),
        pool = (...args: string[]) =>
          execute(cli, ['-C', store, 'coordination', '--project', root, ...args], { cwd: root, env: cleanEnv });
      await git('init', '-q', '-b', 'main');
      await git('commit', '-q', '--allow-empty', '-m', 'Synthetic base');
      await git('remote', 'add', 'origin', 'https://example.invalid/synthetic.git');
      await mkdir(join(root, '.github/workflows'), { recursive: true });
      await writeFile(join(root, '.github/workflows/ci.yml'), 'name: CI\non: push\n');
      await pool('setup');
      await pool('strategy-allow', '--strategy', 'ci-staged', '--reason', 'synthetic strategy editor');
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
        json: { id: server.checkoutId, root: server.root, name: 'Synthetic Strategy', stores: [server.store], apiPath },
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
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto('/');
    await page.getByRole('button', { name: 'Open Project' }).click();
    await page.getByRole('button', { name: 'Local Project' }).click();
    await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
    await page.getByLabel('Settings View').click();
    await page.getByRole('button', { name: 'Development strategy' }).click();
    const settings = page.locator('[data-component="development-strategy-settings"]');
    const choose = async (label: string, option: string) => {
      await settings.getByRole('combobox', { name: label }).click();
      await settings.getByRole('option', { name: option, exact: true }).click();
    };
    await expect(settings.getByRole('combobox', { name: 'Project Strategy' })).toBeVisible();
    await choose('Project Strategy', 'Steady Main');
    await settings.getByRole('button', { name: 'Save Strategy' }).click();
    await expect(settings.getByRole('status')).toContainText('saved');
    await expect
      .poll(
        async () =>
          (
            await server.request<{ effective: { preset: string } }>(
              `/checkouts/${server.checkoutId}/development-strategy`,
            )
          ).effective.preset,
      )
      .toBe('steady-main');
    await expect(settings.locator('[data-cadence-source]')).toContainText('Last green main promotion');
    await page.screenshot({ path: test.info().outputPath('development-strategy-light-wide.png') });
    await page.setViewportSize({ width: 390, height: 760 });
    const hideSettings = page.getByRole('button', { name: 'Hide Settings Sidebar' }),
      hideInspector = page.getByRole('button', { name: 'Hide Ticket Inspector' });
    if (await hideSettings.isVisible()) await hideSettings.click();
    if (await hideInspector.isVisible()) await hideInspector.click();
    await page.mouse.move(190, 520);
    await page.mouse.wheel(0, 4000);
    await page.waitForTimeout(250);
    await expect(settings.locator('[data-cadence-source]')).toBeInViewport();
    await page.screenshot({ path: test.info().outputPath('development-strategy-light-phone-cadence.png') });
    await page.setViewportSize({ width: 1440, height: 900 });

    await choose('Project Strategy', 'New Custom…');
    await settings.getByLabel('Custom Name').fill('reviewed-fast');
    await choose('Minimum Risk Tier', 'T2');
    await settings.getByLabel('Local Budget (Minutes, Optional)').fill('5');
    await settings.getByRole('button', { name: 'Save Strategy' }).click();
    await expect(settings.getByRole('alert')).toContainText('T2 minimum cannot have a local time budget');
    await expect(settings.getByLabel('Custom Name')).toHaveValue('reviewed-fast');
    await choose('Minimum Risk Tier', 'T1');
    await settings.getByRole('button', { name: 'Save Strategy' }).click();
    await expect(settings.getByRole('status')).toContainText('saved');
    await expect
      .poll(
        async () =>
          (
            await server.request<{ policy: { default_custom?: string } }>(
              `/checkouts/${server.checkoutId}/development-strategy`,
            )
          ).policy.default_custom,
      )
      .toBe('reviewed-fast');
    await settings.getByLabel('Session Override (Optional)').fill('steady-main');
    await settings.getByLabel('Ticket Override (Optional)').fill('risk-guided');
    await settings.getByRole('button', { name: 'Preview Effective Policy' }).click();
    await expect(settings).toContainText('Selection: risk-guided · ticket');
    await expect
      .poll(
        async () =>
          (
            await server.request<{ policy: { default_custom?: string } }>(
              `/checkouts/${server.checkoutId}/development-strategy`,
            )
          ).policy.default_custom,
      )
      .toBe('reviewed-fast');
    await page.reload();
    await page.getByLabel('Settings View').click();
    await page.getByRole('button', { name: 'Development strategy' }).click();
    await expect(settings.getByRole('combobox', { name: 'Project Strategy' })).toContainText('reviewed-fast');
    await expect(settings).toContainText('Selection: reviewed-fast · project');

    await page.setViewportSize({ width: 1024, height: 750 });
    await page.screenshot({ path: test.info().outputPath('development-strategy-light-medium.png') });
    await page.emulateMedia({ colorScheme: 'dark' });
    await page.setViewportSize({ width: 390, height: 760 });
    await page.waitForTimeout(500);
    await expect(settings).toBeVisible();
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
    await page.screenshot({ path: test.info().outputPath('development-strategy-dark-phone.png') });
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.waitForTimeout(500);
    await expect(settings).toBeVisible();
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
    await page.mouse.move(720, 500);
    await page.mouse.wheel(0, -4000);
    await page.waitForTimeout(250);
    await page.screenshot({ path: test.info().outputPath('development-strategy-dark-returned-wide.png') });
  } finally {
    await server.stop();
  }
});

test('HS2-NBDXPW project switching shows only the selected checkout policy and resets its draft', async ({ page }) => {
  test.setTimeout(120_000);
  const seedGit = async ({ root, store, cli, env }: RealTicketFixtureContext) => {
    const cleanEnv = withoutGitRepositoryEnv(env);
    await execute('git', ['init', '-q', '-b', 'main'], { cwd: root, env: cleanEnv });
    await execute('git', ['commit', '-q', '--allow-empty', '-m', 'Synthetic base'], { cwd: root, env: cleanEnv });
    await execute('git', ['remote', 'add', 'origin', 'https://example.invalid/synthetic.git'], {
      cwd: root,
      env: cleanEnv,
    });
    await mkdir(join(root, '.github/workflows'), { recursive: true });
    await writeFile(join(root, '.github/workflows/ci.yml'), 'name: CI\non: push\n');
    await execute(cli, ['-C', store, 'coordination', '--project', root, 'setup'], { cwd: root, env: cleanEnv });
    await execute(
      cli,
      [
        '-C',
        store,
        'coordination',
        '--project',
        root,
        'strategy-allow',
        '--strategy',
        'ci-staged',
        '--reason',
        'synthetic strategy switch',
      ],
      { cwd: root, env: cleanEnv },
    );
  };
  const first = await realTicketServer({ seed: seedGit }),
    second = await realTicketServer({ seed: seedGit });
  try {
    let opening = first;
    await page.routeWebSocket(/.*/, (route) => {
      void route.close();
    });
    await page.route('**/__hotsheet/folders/choose', (route) => route.fulfill({ json: { path: second.root } }));
    await page.route(/\/__hotsheet\/(?:remote\/)?projects\/open$/, (route) =>
      route.fulfill({
        status: 201,
        json: {
          id: opening.checkoutId,
          root: opening.root,
          name: opening === first ? 'Synthetic One' : 'Synthetic Two',
          stores: [opening.store],
          apiPath: `/__hotsheet/project-api/${opening.checkoutId}`,
        },
      }),
    );
    await page.route('**/__hotsheet/project-api/*/**', async (route) => {
      const incoming = new URL(route.request().url()),
        server = incoming.pathname.includes(first.checkoutId) ? first : second,
        prefix = `/__hotsheet/project-api/${server.checkoutId}`;
      try {
        const response = await route.fetch({
          url: `${server.url}${incoming.pathname.slice(prefix.length)}${incoming.search}`,
          headers: { ...route.request().headers(), 'X-Hotsheet-Secret': server.secret },
          timeout: 60_000,
        });
        await route.fulfill({ response });
      } catch {
        await route.abort().catch(() => undefined);
      }
    });
    await page.goto('/');
    await page.getByRole('button', { name: 'Open Project' }).click();
    await page.getByRole('button', { name: 'Local Project' }).click();
    await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
    await page.getByLabel('Settings View').click();
    await page.getByRole('button', { name: 'Development strategy' }).click();
    const settings = page.locator('[data-component="development-strategy-settings"]');
    await expect(settings.getByLabel('Green Main Goal (Minutes)')).toHaveValue('360');
    await settings.getByRole('combobox', { name: 'Project Strategy' }).click();
    await settings.getByRole('option', { name: 'Steady Main' }).click();
    await settings.getByLabel('Green Main Goal (Minutes)').fill('90');
    await settings.getByRole('button', { name: 'Save Strategy' }).click();
    await expect
      .poll(
        async () =>
          (
            await first.request<{ policy: { green_main_goal_minutes: number } }>(
              `/checkouts/${first.checkoutId}/development-strategy`,
            )
          ).policy.green_main_goal_minutes,
      )
      .toBe(90);
    await expect(settings.locator('[data-cadence-source]')).toHaveCount(1);
    const firstSource = await settings.locator('[data-cadence-source]').getAttribute('data-cadence-source');
    await settings.getByLabel('Green Main Goal (Minutes)').fill('120');
    await expect(settings.getByLabel('Green Main Goal (Minutes)')).toHaveValue('120');
    opening = second;
    await page.getByRole('button', { name: 'Add Project' }).click();
    await page.getByRole('button', { name: 'Local Project' }).click();
    await expect(settings.getByLabel('Green Main Goal (Minutes)')).toHaveValue('360');
    await expect(settings).toContainText('unconfigured project');
    await expect(settings.locator(`[data-cadence-source="${firstSource}"]`)).toHaveCount(0);
    await page.locator('[data-component="tab-bar-item"]').filter({ hasText: 'Synthetic One' }).click();
    await expect(settings.getByLabel('Green Main Goal (Minutes)')).toHaveValue('90');
    await expect(settings.locator(`[data-cadence-source="${firstSource}"]`)).toHaveCount(1);

    const stalePreview = await first.request<unknown>(
      `/checkouts/${first.checkoutId}/development-strategy?ticket=ticket-assured`,
    );
    let releasePreview: () => void = () => {},
      previewStarted: () => void = () => {},
      previewDelivered: () => void = () => {};
    const previewHold = new Promise<void>((resolve) => {
        releasePreview = resolve;
      }),
      previewRequest = new Promise<void>((resolve) => {
        previewStarted = resolve;
      }),
      previewResponse = new Promise<void>((resolve) => {
        previewDelivered = resolve;
      });
    await page.route(/\/development-strategy\?ticket=ticket-assured$/, async (route) => {
      previewStarted();
      await previewHold;
      await route.fulfill({ json: stalePreview });
      previewDelivered();
    });
    await settings.getByLabel('Ticket Override (Optional)').fill('ticket-assured');
    await settings.getByRole('button', { name: 'Preview Effective Policy' }).click();
    await previewRequest;
    await settings.getByLabel('Green Main Goal (Minutes)').fill('105');
    await settings.getByRole('button', { name: 'Save Strategy' }).click();
    await expect(settings.getByRole('status')).toContainText('saved');
    releasePreview();
    await previewResponse;
    await expect(settings).toContainText('· project');
    await expect(settings).not.toContainText('· ticket');
  } finally {
    await first.stop();
    await second.stop();
  }
});
