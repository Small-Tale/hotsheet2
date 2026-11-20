import { mkdirSync } from 'node:fs';
import { join } from 'node:path';

import { expect, type Locator, type Page, test } from '@playwright/test';

import type { FullTicket } from '../src/api';
import { openMockedProject } from './open-mocked-project';
import { proxyFetch, realTicketServer } from './real-ticket-server';

/**
 * HS2-1MSTXP: settings toggles that save the moment they change are Kerf switches, in the real app
 * against a private real server. App Settings › General (Show loading activity, Standby, host health
 * disks), Project Settings › General (Group new tickets automatically, Hide Verified column) and
 * Project Settings › Terminals (Use my global shell history) each expose `role="switch"` and save at
 * once. Synthetic tickets only. The captures are the ticket's after images, named by page, width and
 * color scheme, across wide, medium, narrow and back to wide.
 */
type Server = Awaited<ReturnType<typeof realTicketServer>>;
const captures = join('target', 'visual-captures', 'hs2-1mstxp');
mkdirSync(captures, { recursive: true });

async function openProject(page: Page, server: Server) {
  const apiPath = `/__hotsheet/project-api/${server.checkoutId}`;
  await page.routeWebSocket(/.*/, (route) => {
    void route.close();
  });
  await page.route(/\/__hotsheet\/(?:remote\/)?projects\/open$/, (route) =>
    route.fulfill({
      status: 201,
      json: { id: server.checkoutId, root: server.root, name: 'Launchpad', stores: [server.store], apiPath },
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
  await page.goto('/');
  await openMockedProject(page);
  await expect(page.locator('[data-component="ticket-list"] [data-ticket-slug]').first()).toBeVisible();
  await expect(page.locator('#app-loading-indicator')).toHaveCount(0);
}

/** Wait until finite layout animations (a rail settling after a resize) have finished. */
async function settle(page: Page): Promise<void> {
  await expect
    .poll(() =>
      page.evaluate(async () => {
        await new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
        return document
          .getAnimations()
          .filter(
            (animation) =>
              animation.playState === 'running' && animation.effect?.getComputedTiming().iterations !== Infinity,
          ).length;
      }),
    )
    .toBe(0);
}

/** Wide, medium, narrow and back to wide; each width keeps the section unclipped and captured. */
async function acrossWidths(page: Page, section: Locator, name: string, scheme: string, engine: string) {
  for (const [step, width] of [
    ['wide', 1440],
    ['medium', 1100],
    ['narrow', 390],
    ['wide-again', 1440],
  ] as const) {
    await page.setViewportSize({ width, height: width === 390 ? 844 : 1000 });
    await section.scrollIntoViewIfNeeded();
    await expect(section).toBeVisible();
    expect(await section.evaluate((node) => node.scrollWidth - node.clientWidth)).toBeLessThanOrEqual(1);
    await settle(page);
    // The host name is the test machine's own; mask it so a capture names no developer machine.
    await page.screenshot({
      path: join(captures, `hs2-1mstxp-${name}-${step}-${width}-${scheme}-${engine}.png`),
      mask: [page.locator('[data-component="host-disk-settings"] p').first()],
    });
  }
  await page.setViewportSize({ width: 1440, height: 1000 });
}

for (const scheme of ['light', 'dark'] as const)
  test(`HS2-1MSTXP instant-apply settings are switches that save at once (${scheme})`, async ({
    page,
    browserName,
  }) => {
    test.setTimeout(240_000);
    await page.emulateMedia({ colorScheme: scheme });
    await page.setViewportSize({ width: 1440, height: 1000 });
    const server = await realTicketServer();
    try {
      await server.request<FullTicket>(`/checkouts/${server.checkoutId}/tickets`, 'POST', {
        title: 'Persist column widths per view',
        category: 'task',
        tags: [],
      });
      await openProject(page, server);
      await page.getByLabel('Settings View').click();

      // App Settings › General.
      await page
        .getByRole('navigation', { name: 'App Settings' })
        .getByRole('button', { name: 'General', exact: true })
        .click();
      const general = page.locator('[data-settings-category="general"]');
      const loading = general.getByRole('switch', { name: 'Show loading activity' });
      await expect(loading).toBeVisible();
      await expect(general.getByRole('checkbox')).toHaveCount(0);
      const standby = general.getByRole('switch', { name: 'Enter standby when idle' });
      await expect(standby).toBeEnabled();
      const standbyBefore = await standby.isChecked();
      await standby.setChecked(!standbyBefore);
      await expect
        .poll(
          async () =>
            (await server.request<{ effective: { enabled: boolean } }>('/standby-settings')).effective.enabled,
        )
        .toBe(!standbyBefore);
      await standby.setChecked(standbyBefore);
      await expect
        .poll(
          async () =>
            (await server.request<{ effective: { enabled: boolean } }>('/standby-settings')).effective.enabled,
        )
        .toBe(standbyBefore);
      const loadingBefore = await loading.isChecked();
      await loading.setChecked(!loadingBefore);
      await expect(loading).toBeChecked({ checked: !loadingBefore });
      await loading.setChecked(loadingBefore);
      await general.getByRole('heading', { name: 'Standby' }).scrollIntoViewIfNeeded();
      await acrossWidths(page, general, 'app-general', scheme, browserName);

      // Project Settings › General.
      const projectNav = page.getByRole('navigation', { name: 'Project Settings' });
      await projectNav.getByRole('button', { name: 'General', exact: true }).click();
      const project = page.getByRole('region', { name: 'General Settings' });
      const autoGroup = project.getByRole('switch', { name: 'Group new tickets automatically' });
      await expect(autoGroup).toBeChecked();
      await expect(project.getByRole('checkbox')).toHaveCount(0);
      await autoGroup.uncheck();
      await expect
        .poll(
          async () =>
            (
              await server.request<{ auto_group_new_tickets: boolean }>(
                `/checkouts/${server.checkoutId}/ticket-creation-settings`,
              )
            ).auto_group_new_tickets,
        )
        .toBe(false);
      await autoGroup.check();
      const verified = project.getByRole('switch', { name: 'Hide Verified column' });
      await verified.check();
      await expect(verified).toBeChecked();
      await acrossWidths(page, project, 'project-general', scheme, browserName);
      await verified.uncheck();

      // Project Settings › Terminals.
      await projectNav.getByRole('button', { name: 'Terminals', exact: true }).click();
      const terminals = page.locator('[data-settings-category="terminals"]');
      const history = terminals.getByRole('switch', { name: 'Use my global shell history' });
      await expect(history).toBeVisible();
      await acrossWidths(page, terminals, 'terminals', scheme, browserName);
    } finally {
      await page.unrouteAll({ behavior: 'ignoreErrors' });
      await server.stop();
    }
  });
