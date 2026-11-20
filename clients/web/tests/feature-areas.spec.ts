import { execFileSync } from 'node:child_process';
import { mkdirSync } from 'node:fs';
import { join } from 'node:path';

import { expect, type Page, test } from '@playwright/test';

import type { FullTicket } from '../src/api';
import { openMockedProject } from './open-mocked-project';
import { proxyFetch, realTicketServer } from './real-ticket-server';

/**
 * App Settings › Features (HS2-3GJZGG) in the real app against a private real server with the shipped
 * defaults: experimental areas are hidden and their shortcut ignored; a switch turns one on at once;
 * a headless `hotsheet-cli settings set features.<id>` reaches the open client without a reload; Ticket
 * groups off shows grouped tickets ungrouped; Restore Defaults clears every choice. Synthetic tickets
 * only. The captures are the ticket's after images, named by state, width and color scheme.
 */
type Server = Awaited<ReturnType<typeof realTicketServer>>;
const captures = join('target', 'visual-captures', 'hs2-3gjzgg');
mkdirSync(captures, { recursive: true });

async function seed(server: Server) {
  const checkout = `/checkouts/${server.checkoutId}`;
  for (const [title, group] of [
    ['Persist column widths per view', 'Board polish'],
    ['Fix flicker when the board reconnects', 'Board polish'],
    ['Terminal drawer remembers its last tab', undefined],
    ['Onboarding checklist copy review', undefined],
  ] as const)
    await server.request<FullTicket>(`${checkout}/tickets`, 'POST', { title, category: 'task', tags: [], group });
}

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

const shot = async (page: Page, name: string, scheme: string) => {
  await settle(page);
  return page.screenshot({
    path: join(captures, `hs2-3gjzgg-${name}-${page.viewportSize()?.width ?? 0}-${scheme}-v2.png`),
  });
};

/** Wide, medium, narrow and back to wide, capturing each (the wide-narrow-wide transition). */
async function acrossWidths(page: Page, capture: (width: number) => Promise<unknown>) {
  for (const width of [1440, 1100, 390, 1440]) {
    await page.setViewportSize({ width, height: width === 390 ? 844 : 1100 });
    await capture(width);
  }
}

for (const scheme of ['light', 'dark'] as const)
  test(`HS2-3GJZGG feature areas: defaults, Features page, headless change, groups off (${scheme})`, async ({
    page,
  }) => {
    test.setTimeout(240_000);
    await page.emulateMedia({ colorScheme: scheme });
    await page.setViewportSize({ width: 1440, height: 1100 });
    const server = await realTicketServer({ features: 'defaults' });
    try {
      await seed(server);
      await openProject(page, server);
      const sidebar = page.locator('#app-left-rail');

      // Ticket groups and saved views is on by default: the grouped tickets show as a group.
      await expect(page.locator('[data-component="ticket-list"] [data-ticket-group]').first()).toBeVisible();
      // Defaults: no Drive, no chat, no Cross-Project Stats; the chart stays but is not a button.
      await expect(sidebar.locator('[data-component="drive-control"]')).toHaveCount(0);
      await expect(page.getByRole('button', { name: 'Cross-Project Stats' })).toHaveCount(0);
      await expect(page.getByRole('button', { name: 'Workspace Grid' })).toBeVisible();
      await expect(sidebar.locator('[data-component="project-summary"]')).toHaveAttribute('data-interactive', 'false');
      // The All-Project Stats shortcut of a turned-off feature is ignored.
      await page.keyboard.press('ControlOrMeta+Shift+D');
      await expect(page.locator('[data-component="project-tab-bar"]')).toHaveAttribute('data-mode', 'project');
      await acrossWidths(page, () => shot(page, 'shell-defaults-project', scheme));
      await page.setViewportSize({ width: 1440, height: 1100 });

      // App Settings › Features is the last category; a switch applies at once.
      await page.getByLabel('Settings View').click();
      const categories = page.locator('nav[aria-label="App Settings"] [data-action="select-settings-category"]');
      await expect(categories.last()).toHaveAttribute('data-item-id', 'features');
      await categories.last().click();
      const features = page.locator('[data-settings-category="features"]');
      await expect(features.getByRole('heading', { name: 'Experimental' })).toBeVisible();
      await acrossWidths(page, async () => {
        await features.getByRole('heading', { name: 'Experimental' }).scrollIntoViewIfNeeded();
        await shot(page, 'features-top', scheme);
        await features.getByRole('button', { name: 'Restore Defaults' }).scrollIntoViewIfNeeded();
        await shot(page, 'features-bottom', scheme);
      });
      await page.setViewportSize({ width: 1440, height: 1100 });
      // The switch tracks line up with the headings' text (the maintainer's v1 annotation).
      const headingX = await features.getByRole('heading', { name: 'Experimental' }).evaluate((node) => {
        const range = document.createRange();
        range.selectNodeContents(node);
        return range.getBoundingClientRect().left;
      });
      const boxX = await features
        .locator('[data-feature-row="drive"] input[role="switch"]')
        .evaluate((node) => node.getBoundingClientRect().left);
      expect(Math.abs(boxX - headingX)).toBeLessThanOrEqual(2);

      const drive = features.getByRole('switch', { name: 'Drive with AI' });
      await expect(drive).not.toBeChecked();
      await drive.check();
      await expect
        .poll(
          async () =>
            (await server.request<{ effective: Record<string, boolean> }>('/feature-settings')).effective.drive,
        )
        .toBe(true);

      // A headless change reaches the open client through the change stream, without a reload.
      await expect(page.getByLabel(/^Workers View/)).toHaveCount(1);
      execFileSync(server.cli, ['settings', 'set', 'features.workers-view', 'off'], { env: server.env });
      await server.request('/feature-settings/changed', 'POST');
      await expect(page.getByLabel(/^Workers View/)).toHaveCount(0);
      await expect(features.getByRole('switch', { name: 'Workers view' })).not.toBeChecked();

      // The quiet workspace: every optional area off; Ticket groups off shows grouped tickets ungrouped.
      for (const name of [
        'Workspace Grid',
        'Commands',
        'Repository status',
        'Live activity',
        'Ticket groups and saved views',
        'Announcer',
        'Standby',
      ])
        await features.getByRole('switch', { name }).uncheck();
      await expect
        .poll(
          async () =>
            (await server.request<{ effective: Record<string, boolean> }>('/feature-settings')).effective[
              'ticket-groups'
            ],
        )
        .toBe(false);
      await expect(page.locator('[data-action="select-settings-category"][data-item-id="groups"]')).toHaveCount(0);
      await page.getByLabel('List View').click();
      await expect(page.locator('[data-component="ticket-list"] [data-ticket-slug]').first()).toBeVisible();
      // Drive, turned on above, is in the project sidebar.
      await expect(sidebar.locator('[data-component="drive-control"]')).toBeVisible();
      await expect(page.locator('[data-component="ticket-list"] [data-ticket-group]')).toHaveCount(0);
      await expect(page.getByRole('button', { name: 'Workspace Grid' })).toHaveCount(0);
      await expect(page.getByRole('button', { name: 'Enter Standby' })).toHaveCount(0);
      await acrossWidths(page, () => shot(page, 'shell-quiet-groups-off-project', scheme));
      await page.setViewportSize({ width: 1440, height: 1100 });

      // Restore Defaults clears every choice.
      await page.getByLabel('Settings View').click();
      await categories.last().click();
      await features.getByRole('button', { name: 'Restore Defaults' }).click();
      await expect
        .poll(
          async () =>
            (await server.request<{ configured: Record<string, boolean | null> }>('/feature-settings')).configured
              .drive,
        )
        .toBeNull();
      await expect(features.getByRole('switch', { name: 'Standby' })).toBeChecked();
      await page.getByLabel('List View').click();
      await expect(page.locator('[data-component="ticket-list"] [data-ticket-group]').first()).toBeVisible();
      await expect(sidebar.locator('[data-component="drive-control"]')).toHaveCount(0);
    } finally {
      await page.unrouteAll({ behavior: 'ignoreErrors' });
      await server.stop();
    }
  });
