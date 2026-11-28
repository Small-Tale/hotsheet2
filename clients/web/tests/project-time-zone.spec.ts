import { execFileSync } from 'node:child_process';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

import { expect, type Locator, type Page, test } from '@playwright/test';

import type { FullTicket, TimeZoneSettings } from '../src/api';
import { openMockedProject } from './open-mocked-project';
import { proxyFetch, realTicketServer } from './real-ticket-server';

/**
 * HS2-SZF152: Project Settings › General › Time Zone in the real app against a private real server. The
 * select starts at UTC, a choice saves the shared `time_zone` at once and survives a reload, a zone the
 * server refuses restores the saved one with the server's message under the control, a headless
 * `hotsheet-cli settings set time_zone` shows when the page is opened again, and a hand-edited unknown name
 * reads as UTC with the diagnostic. Synthetic tickets only. The captures are the ticket's after images at
 * wide, medium, narrow and back to wide in both color schemes.
 */
type Server = Awaited<ReturnType<typeof realTicketServer>>;
const captures = join('target', 'visual-captures', 'hs2-szf152');
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

/** Open Project Settings › General and return its region once the zone has been read. */
async function openGeneral(page: Page): Promise<Locator> {
  const nav = page.getByRole('navigation', { name: 'Project Settings' });
  // Visiting another category first makes a second visit re-read the setting.
  await nav.getByRole('button', { name: 'Lifecycle', exact: true }).click();
  await nav.getByRole('button', { name: 'General', exact: true }).click();
  const general = page.getByRole('region', { name: 'General Settings' });
  await expect(general.locator('[data-component="project-time-zone-setting"]')).not.toHaveAttribute(
    'data-time-zone',
    '',
  );
  return general;
}

const zoneOf = (server: Server) =>
  server.request<TimeZoneSettings>(`/checkouts/${server.checkoutId}/time-zone-settings`).then((s) => s.time_zone);

/** Choose a zone from the open listbox by its label's leading name. */
async function choose(page: Page, select: Locator, name: string) {
  await select.click();
  const label = name.replaceAll('_', ' ').replace('/', '\\/');
  const option = page.getByRole('option', { name: new RegExp(`^${label} \\(UTC`) });
  await option.scrollIntoViewIfNeeded();
  await option.click();
}

for (const scheme of ['light', 'dark'] as const)
  test(`HS2-SZF152 the project Time Zone select saves the shared zone, survives reload, and shows refusals (${scheme})`, async ({
    page,
    browserName,
  }) => {
    test.setTimeout(180_000);
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
      let general = await openGeneral(page);
      let select = general.getByRole('combobox', { name: 'Time Zone' });

      // Unset reads as UTC, whatever the browser's own zone is.
      await expect(select).toContainText('UTC');
      await expect(general.locator('[data-component="project-time-zone-setting"]')).toHaveAttribute(
        'data-time-zone',
        'UTC',
      );

      // A choice saves the shared setting at once, without a Save button.
      await choose(page, select, 'Europe/Berlin');
      await expect(select).toContainText('Europe/Berlin (UTC');
      await expect.poll(() => zoneOf(server)).toBe('Europe/Berlin');
      const shared = JSON.parse(readFileSync(join(server.root, '.hotsheet2', 'settings.json'), 'utf8')) as Record<
        string,
        unknown
      >;
      expect(shared.time_zone).toBe('Europe/Berlin');
      await expect(general.getByRole('button', { name: /^Save/ })).toHaveCount(0);

      // It survives a reload.
      await page.reload();
      await page.getByLabel('Settings View').click();
      general = await openGeneral(page);
      select = general.getByRole('combobox', { name: 'Time Zone' });
      await expect(select).toContainText('Europe/Berlin (UTC');

      // A zone the server does not know is refused: the saved zone comes back with the server's reason.
      const apiPath = `/__hotsheet/project-api/${server.checkoutId}`;
      await page.route(`**${apiPath}/checkouts/*/time-zone-settings`, async (route) => {
        if (route.request().method() !== 'PUT') return route.fallback();
        const response = await fetch(`${server.url}/checkouts/${server.checkoutId}/time-zone-settings`, {
          method: 'PUT',
          headers: { 'content-type': 'application/json', 'X-Hotsheet-Secret': server.secret },
          body: JSON.stringify({ time_zone: 'Mars/Olympus_Mons' }),
        });
        expect(response.status).toBe(400);
        await route.fulfill({ status: response.status, contentType: 'application/json', body: await response.text() });
      });
      await choose(page, select, 'Asia/Tokyo');
      await expect(general.getByText('"Mars/Olympus_Mons" is not a known time zone')).toBeVisible();
      await expect(select).toHaveAttribute('aria-invalid', 'true');
      await expect(select).toContainText('Europe/Berlin (UTC');
      expect(await zoneOf(server)).toBe('Europe/Berlin');
      await page.unroute(`**${apiPath}/checkouts/*/time-zone-settings`);

      // Choosing again after the refusal saves and clears the message.
      await choose(page, select, 'America/New_York');
      await expect.poll(() => zoneOf(server)).toBe('America/New_York');
      await expect(general.getByText('is not a known time zone')).toHaveCount(0);
      await expect(select).not.toHaveAttribute('aria-invalid', 'true');

      // A headless change shows the next time the page is opened.
      execFileSync(server.cli, ['-C', server.store, 'settings', 'set', 'time_zone', 'Asia/Kolkata'], {
        env: server.env,
        cwd: server.root,
      });
      general = await openGeneral(page);
      select = general.getByRole('combobox', { name: 'Time Zone' });
      await expect(select).toContainText('Asia/Kolkata (UTC+05:30)');

      // The captures: a chosen zone at each width, then the open list at the narrow width.
      for (const [step, width] of [
        ['wide', 1440],
        ['medium', 1100],
        ['narrow', 390],
        ['wide-again', 1440],
      ] as const) {
        await page.setViewportSize({ width, height: width === 390 ? 844 : 1000 });
        await general.scrollIntoViewIfNeeded();
        await expect(select).toBeVisible();
        expect(await general.evaluate((node) => node.scrollWidth - node.clientWidth)).toBeLessThanOrEqual(1);
        await settle(page);
        const box = await select.boundingBox();
        expect(box && box.x >= 0 && box.x + box.width <= width).toBe(true);
        await page.screenshot({ path: join(captures, `hs2-szf152-${step}-${width}-${scheme}-${browserName}.png`) });
        if (width === 390) {
          await select.click();
          const listbox = page.getByRole('listbox');
          await expect(listbox).toBeVisible();
          await settle(page);
          const menu = await listbox.boundingBox();
          expect(menu && menu.x >= 0 && menu.x + menu.width <= width).toBe(true);
          await page.screenshot({ path: join(captures, `hs2-szf152-open-${width}-${scheme}-${browserName}.png`) });
          await page.keyboard.press('Escape');
          await expect(listbox).toBeHidden();
        }
      }

      // A hand-edited unknown name reads as UTC and is named under the control.
      writeFileSync(
        join(server.root, '.hotsheet2', 'settings.json'),
        JSON.stringify({ $hotsheetSchema: 1, time_zone: 'Mars/Olympus_Mons' }),
      );
      general = await openGeneral(page);
      select = general.getByRole('combobox', { name: 'Time Zone' });
      await expect(select).toContainText('UTC');
      await expect(general.getByText('"Mars/Olympus_Mons" in .hotsheet2/settings.json')).toBeVisible();
      await page.screenshot({ path: join(captures, `hs2-szf152-diagnostic-1440-${scheme}-${browserName}.png`) });
      // Choosing a zone replaces it.
      await choose(page, select, 'Europe/Berlin');
      await expect.poll(() => zoneOf(server)).toBe('Europe/Berlin');
      await expect(general.getByText('in .hotsheet2/settings.json')).toHaveCount(0);
    } finally {
      await page.unrouteAll({ behavior: 'ignoreErrors' });
      await server.stop();
    }
  });
