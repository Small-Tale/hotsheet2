import { execFile } from 'node:child_process';
import { promisify } from 'node:util';

import { expect, type Page, test } from '@playwright/test';

import { openDemoProject, project } from './mobile-project-fixture';
import { openMockedProject } from './open-mocked-project';
import { proxyFetch, realTicketServer } from './real-ticket-server';
import { printThenEcho } from './terminal-fixture-command';

const execute = promisify(execFile);

/**
 * HS2-7DWS3S: the open Restore Project menu keeps the popup inset, so its heading and rows (and the
 * current-row highlight) sit inside the border instead of running edge to edge.
 */
async function expectRestoreMenuInset(page: Page) {
  const menu = page.locator('[data-minimized-projects="true"] [role="menu"]');
  await expect(menu).toBeVisible();
  const inset = await menu.evaluate((element) => {
    const style = getComputedStyle(element),
      box = element.getBoundingClientRect(),
      item = element.querySelector('[role="menuitem"]')!.getBoundingClientRect();
    return {
      paddingTop: parseFloat(style.paddingTop),
      paddingInline: Math.min(parseFloat(style.paddingLeft), parseFloat(style.paddingRight)),
      itemStart: item.left - box.left,
      itemEnd: box.right - item.right,
    };
  });
  expect(inset.paddingTop).toBeGreaterThanOrEqual(4);
  expect(inset.paddingInline).toBeGreaterThanOrEqual(4);
  expect(inset.itemStart).toBeGreaterThanOrEqual(4);
  expect(inset.itemEnd).toBeGreaterThanOrEqual(4);
}

test('HS2-4Q9DVS minimizes selected and background desktop tabs, then restores in place', async ({
  page,
}, testInfo) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await openDemoProject(page);
  const strip = page.locator('.project-tab-bar:not(.project-tab-bar--mobile)');
  await expect(strip.locator('[data-tab-kind="project"]')).toHaveCount(1);

  await page.route('**/__hotsheet/folders/choose', (route) => route.fulfill({ json: { path: '/work/second' } }));
  await page.route('**/__hotsheet/projects/open', (route) =>
    route.fulfill({
      status: 201,
      json: { ...project('second', '/work/second'), apiPath: '/__hotsheet/project-api/demo-checkout' },
    }),
  );
  await strip.getByRole('button', { name: 'Add Project' }).click();
  await page.getByRole('button', { name: 'Local Project' }).click();
  await expect(strip.locator('[data-tab-kind="project"]')).toHaveCount(2);
  await page.screenshot({ path: testInfo.outputPath('HS2-4Q9DVS-open-wide.png'), animations: 'disabled' });

  await strip.locator('[data-project-id="demo-checkout"]').first().click({ button: 'right' });
  await page.getByRole('menuitem', { name: 'Minimize Tab' }).click();
  await expect(strip.locator('[data-tab-kind="project"]')).toHaveCount(1);
  await expect(strip.locator('[data-project-id="second"]')).toHaveCount(1);
  await strip.getByRole('button', { name: 'Minimized Projects' }).click();
  await expect(page.getByRole('menuitem', { name: 'demo' })).toBeVisible();
  await expectRestoreMenuInset(page);
  await page.screenshot({ path: testInfo.outputPath('HS2-4Q9DVS-restore-menu-wide.png'), animations: 'disabled' });
  await page.getByRole('menuitem', { name: 'demo' }).click();
  await expect(strip.locator('[data-tab-kind="project"]')).toHaveCount(2);
  await expect(strip.getByRole('button', { name: 'Minimized Projects' })).toHaveCount(0);

  await strip.locator('[data-project-id="second"]').first().click({ button: 'right' });
  await page.getByRole('menuitem', { name: 'Minimize Tab' }).click();
  await expect(strip.locator('[data-tab-kind="project"]')).toHaveCount(1);
  await strip.locator('[data-project-id="demo-checkout"]').first().click({ button: 'right' });
  await page.getByRole('menuitem', { name: 'Minimize Tab' }).click();
  await expect(page.getByRole('button', { name: 'Minimized Projects' })).toBeVisible();
  await expect(page.getByRole('button', { name: 'Add Project' })).toBeVisible();
  await page.screenshot({ path: testInfo.outputPath('HS2-4Q9DVS-all-minimized-wide.png'), animations: 'disabled' });
  await page.setViewportSize({ width: 960, height: 900 });
  await expect(page.getByRole('button', { name: 'Minimized Projects' })).toBeVisible();
  await page.screenshot({ path: testInfo.outputPath('HS2-4Q9DVS-all-minimized-medium.png'), animations: 'disabled' });
  await page.setViewportSize({ width: 390, height: 844 });
  await expect(page.getByRole('button', { name: 'Minimized Projects' })).toBeVisible();
  await page.screenshot({ path: testInfo.outputPath('HS2-4Q9DVS-all-minimized-narrow.png'), animations: 'disabled' });
  await page.setViewportSize({ width: 1440, height: 900 });
  await expect(page.getByRole('button', { name: 'Minimized Projects' })).toBeVisible();
  await page.screenshot({
    path: testInfo.outputPath('HS2-4Q9DVS-all-minimized-wide-return.png'),
    animations: 'disabled',
  });
  await page.emulateMedia({ colorScheme: 'dark' });
  await page.screenshot({
    path: testInfo.outputPath('HS2-4Q9DVS-all-minimized-dark-wide.png'),
    animations: 'disabled',
  });
});

test('HS2-4Q9DVS phone actions and reload preserve a minimized project', async ({ page }, testInfo) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await openDemoProject(page);
  const phone = page.locator('.project-tab-bar--mobile');
  await phone.getByRole('button', { name: 'demo Project Actions' }).click();
  await page.screenshot({ path: testInfo.outputPath('HS2-4Q9DVS-phone-actions-narrow.png'), animations: 'disabled' });
  await phone.getByRole('menuitem', { name: 'Minimize Tab' }).click();
  await expect(phone.getByRole('button', { name: 'Minimized Projects' })).toBeVisible();
  await page.reload();
  await expect(phone.getByRole('button', { name: 'Minimized Projects' })).toBeVisible();
  await phone.getByRole('button', { name: 'Minimized Projects' }).click();
  await page.screenshot({ path: testInfo.outputPath('HS2-4Q9DVS-phone-restore-narrow.png'), animations: 'disabled' });
  await page.getByRole('menuitem', { name: 'demo' }).click();
  await expect(phone.getByRole('button', { name: 'Minimized Projects' })).toHaveCount(0);
  await expect(phone.getByRole('button', { name: 'demo Project Actions' })).toBeVisible();
});

test('HS2-4Q9DVS keeps an isolated project terminal and CLI available while its tab is minimized', async ({ page }) => {
  test.setTimeout(120_000);
  const server = await realTicketServer();
  const apiPath = `/__hotsheet/project-api/${server.checkoutId}`;
  try {
    await server.request('/terminals', 'POST', {
      id: 'continuity',
      ...printThenEcho('CONTINUITY READY\\r\\n'),
      cwd: server.root,
    });
    await page.routeWebSocket(/.*/, (route) => {
      void route.close();
    });
    await page.route('**/__hotsheet/folders/choose', (route) => route.fulfill({ json: { path: server.root } }));
    await page.route(/\/__hotsheet\/(?:remote\/)?projects\/open$/, (route) =>
      route.fulfill({
        status: 201,
        json: { id: server.checkoutId, root: server.root, name: 'isolated', stores: [server.store], apiPath },
      }),
    );
    await page.route(`**${apiPath}/**`, async (route) => {
      const incoming = new URL(route.request().url());
      const path = incoming.pathname.slice(apiPath.length);
      const response = await proxyFetch(route, {
        url: `${server.url}${path}${incoming.search}`,
        headers: { ...route.request().headers(), 'X-Hotsheet-Secret': server.secret },
        timeout: 60_000,
      });
      await route.fulfill({ response });
    });
    await page.goto('/');
    await openMockedProject(page);
    const strip = page.locator('.project-tab-bar:not(.project-tab-bar--mobile)');
    await expect(strip.locator(`[data-project-id="${server.checkoutId}"]`)).toBeVisible();
    await expect.poll(async () => (await server.request<{ alive: boolean }>('/terminals/continuity')).alive).toBe(true);

    await strip.locator(`[data-project-id="${server.checkoutId}"]`).click({ button: 'right' });
    await page.getByRole('menuitem', { name: 'Minimize Tab' }).click();
    await expect(strip.locator('[data-tab-kind="project"]')).toHaveCount(0);
    await expect(strip.getByRole('button', { name: 'Minimized Projects' })).toBeVisible();

    const created = await execute(
      server.cli,
      ['-C', server.store, 'new', '--title', 'CLI continuity', '--category', 'task'],
      {
        cwd: server.root,
        env: server.env,
      },
    );
    const slug = created.stdout.match(/HS2-[A-Z0-9]+/u)?.[0];
    expect(slug).toBeDefined();
    await expect.poll(async () => (await server.request<{ alive: boolean }>('/terminals/continuity')).alive).toBe(true);
    await expect
      .poll(async () => (await server.request<Array<{ slug: string }>>('/tickets')).some((item) => item.slug === slug))
      .toBe(true);

    await strip.getByRole('button', { name: 'Minimized Projects' }).click();
    await page.getByRole('menuitem', { name: 'isolated' }).click();
    await expect(strip.locator(`[data-project-id="${server.checkoutId}"]`)).toBeVisible();
    await page.reload();
    await expect(page.getByText('CLI continuity')).toBeVisible();
    await expect.poll(async () => (await server.request<{ alive: boolean }>('/terminals/continuity')).alive).toBe(true);
  } finally {
    await server.stop(page);
  }
});
