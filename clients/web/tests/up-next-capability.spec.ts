import { mkdirSync } from 'node:fs';
import { join } from 'node:path';

import { expect, type Locator, type Page, test } from '@playwright/test';

import type { FullTicket } from '../src/api';
import { openMockedProject } from './open-mocked-project';
import { proxyFetch, realTicketServer } from './real-ticket-server';

/**
 * HS2-Y32T8A: a ticket provider without the `up_next` capability (GitLab and Jira refuse it,
 * HS2-K9H38S) keeps every Up Next control mounted but disabled with its reason, in the real app
 * against a private real server. The server's own descriptor is rewritten to report `up_next: false`,
 * so the client follows the capability rather than any provider name. Every ticket is synthetic.
 */
type Server = Awaited<ReturnType<typeof realTicketServer>>;
const captures = join('target', 'visual-captures', 'hs2-y32t8a');
mkdirSync(captures, { recursive: true });
const capture = (page: Page, name: string) =>
  page.screenshot({ path: join(captures, `${name}.png`), animations: 'disabled' });

const READY = 'Synthetic ready ticket',
  ACTIVE = 'Synthetic active ticket';

async function seed(server: Server) {
  const checkout = `/checkouts/${server.checkoutId}`,
    ready = await server.request<FullTicket>(`${checkout}/tickets`, 'POST', {
      title: READY,
      category: 'task',
      tags: [],
    }),
    created = await server.request<FullTicket>(`${checkout}/tickets`, 'POST', {
      title: ACTIVE,
      category: 'bug',
      tags: [],
    }),
    active = await server.request<FullTicket>(`${checkout}/tickets/${created.id}`, 'PATCH', { status: 'started' });
  return { ready, active };
}

/** Open the real server's checkout, rewriting its provider descriptor when `upNext` is false. */
async function open(page: Page, server: Server, upNext: boolean): Promise<() => number> {
  let rewritten = 0;
  const apiPath = `/__hotsheet/project-api/${server.checkoutId}`;
  await page.routeWebSocket(/.*/, (route) => {
    void route.close();
  });
  await page.route(/\/__hotsheet\/(?:remote\/)?projects\/open$/, (route) =>
    route.fulfill({
      status: 201,
      json: { id: server.checkoutId, root: server.root, name: 'synthetic', stores: [server.store], apiPath },
    }),
  );
  await page.route(`**${apiPath}/**`, async (route) => {
    const incoming = new URL(route.request().url()),
      path = incoming.pathname.slice(apiPath.length);
    try {
      const response = await proxyFetch(route, {
        url: `${server.url}${path}${incoming.search}`,
        headers: { ...route.request().headers(), 'X-Hotsheet-Secret': server.secret },
        timeout: 60_000,
      });
      let body: string | undefined;
      if (!upNext && response.ok() && path.endsWith('/providers')) {
        const text = await response.text();
        if (text.includes('"up_next":true')) {
          rewritten += 1;
          body = text.replaceAll('"up_next":true', '"up_next":false');
        }
      }
      await route.fulfill(body === undefined ? { response } : { response, body });
    } catch {
      await route.abort().catch(() => undefined);
    }
  });
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto('/');
  await openMockedProject(page);
  await expect(page.locator('[data-component="ticket-list"] [data-ticket-slug]').first()).toBeVisible();
  await expect(page.locator('#app-loading-indicator')).toHaveCount(0);
  return () => rewritten;
}

const rowOf = (page: Page, slug: string) =>
  page.locator(`[data-component="ticket-list"] [data-component="summary-row"][data-ticket-slug="${slug}"]`);
const rowStar = (row: Locator) => row.locator('[data-action="toggle-row-up-next"]');
const upNextOf = (server: Server, id: string) =>
  server.request<FullTicket>(`/tickets/${id}`).then((ticket) => ticket.up_next);

async function withServer(
  page: Page,
  run: (server: Server, tickets: Awaited<ReturnType<typeof seed>>) => Promise<void>,
): Promise<void> {
  const server = await realTicketServer();
  try {
    await run(server, await seed(server));
  } finally {
    await page.unrouteAll({ behavior: 'ignoreErrors' });
    await server.stop();
  }
}

test('HS2-Y32T8A disables every Up Next control with its reason for a provider without Up Next', async ({ page }) => {
  test.setTimeout(180_000);
  await withServer(page, async (server, { ready, active }) => {
    const rewrites = await open(page, server, false);
    expect(rewrites()).toBeGreaterThan(0);
    const reason = /tickets do not support Up Next\.$/u;

    // The list row and the column card keep the star, disabled, with the reason as its tooltip.
    for (const ticket of [ready, active]) {
      const star = rowStar(rowOf(page, ticket.slug));
      await expect(star).toBeVisible();
      await expect(star).toBeDisabled();
      await expect(star).toHaveAttribute('title', reason);
    }

    // Selecting a row: the inspector star and the selected-ticket toolbar toggle are disabled too.
    await rowOf(page, ready.slug).click();
    const inspectorStar = page.locator('[data-action="toggle-inspector-up-next"]').first();
    await expect(inspectorStar).toBeVisible();
    await expect(inspectorStar).toBeDisabled();
    await expect(inspectorStar).toHaveAttribute('title', reason);
    const toolbarToggle = page.locator('[data-action="toggle-selected-up-next"]');
    await expect(toolbarToggle).toBeDisabled();
    await expect(toolbarToggle).toHaveAttribute('title', reason);
    await capture(page, 'list-inspector-light-1440');

    // The ticket menu keeps Toggle Up Next, disabled with the reason, while other updates stay.
    await rowOf(page, ready.slug).click({ button: 'right' });
    const menu = page.locator('[data-context-menu="ticket"]'),
      item = menu.locator('[data-context-action="Toggle Up Next"]');
    await expect(item).toBeVisible();
    await expect(item).toHaveAttribute('aria-disabled', 'true');
    await expect(item).toHaveAttribute('title', reason);
    await expect(menu.locator('[data-context-action="Add tag"]')).not.toHaveAttribute('aria-disabled', 'true');
    await capture(page, 'context-menu-light-1440');
    await page.keyboard.press('Escape');
    await expect(menu).toHaveCount(0);

    // Medium and narrow widths keep the disabled star, then back to wide.
    for (const width of [1024, 640, 1440]) {
      await page.setViewportSize({ width, height: 900 });
      const star = rowStar(rowOf(page, ready.slug));
      await expect(star).toBeVisible();
      await expect(star).toBeDisabled();
      if (width !== 1440) await capture(page, `list-light-${width}`);
    }

    // The Columns view card shares the row's disabled star.
    await page.getByRole('button', { name: 'Columns View', exact: true }).click();
    const card = page.locator(`[data-component="ticket-board"] [data-ticket-slug="${active.slug}"]`).first();
    await expect(rowStar(card)).toBeDisabled();
    await capture(page, 'columns-light-1440');
    await page.getByRole('button', { name: 'List View', exact: true }).click();

    // The new-ticket composer for this source cannot start a ticket in Up Next.
    await page.getByRole('button', { name: 'New ticket…', exact: true }).first().click();
    const composer = page.getByRole('dialog', { name: 'Create ticket' }),
      composerStar = composer.locator('[data-action="toggle-new-ticket-up-next"]');
    await expect(composerStar).toBeDisabled();
    await expect(composerStar).toHaveAttribute('aria-pressed', 'false');
    await expect(composerStar).toHaveAttribute('title', reason);
    await capture(page, 'composer-light-1440');

    // Nothing reached the server.
    expect(await upNextOf(server, ready.id)).toBe(false);
    expect(await upNextOf(server, active.id)).toBe(false);
  });
});

test('HS2-Y32T8A shows the disabled Up Next states in the dark color scheme', async ({ page }) => {
  test.setTimeout(120_000);
  await page.emulateMedia({ colorScheme: 'dark' });
  await withServer(page, async (server, { ready }) => {
    await open(page, server, false);
    expect(await page.evaluate(() => matchMedia('(prefers-color-scheme: dark)').matches)).toBe(true);
    const reason = /tickets do not support Up Next\.$/u;
    await rowOf(page, ready.slug).click();
    const inspectorStar = page.locator('[data-action="toggle-inspector-up-next"]').first();
    await expect(inspectorStar).toBeDisabled();
    await expect(inspectorStar).toHaveAttribute('title', reason);
    await expect(rowStar(rowOf(page, ready.slug))).toBeDisabled();
    await capture(page, 'list-inspector-dark-1440');

    await rowOf(page, ready.slug).click({ button: 'right' });
    const menu = page.locator('[data-context-menu="ticket"]'),
      item = menu.locator('[data-context-action="Toggle Up Next"]');
    await expect(item).toHaveAttribute('aria-disabled', 'true');
    await expect(item).toHaveAttribute('title', reason);
    await capture(page, 'context-menu-dark-1440');
    await page.keyboard.press('Escape');
    await expect(menu).toHaveCount(0);

    await page.setViewportSize({ width: 640, height: 900 });
    await expect(rowStar(rowOf(page, ready.slug))).toBeDisabled();
    await capture(page, 'list-dark-640');
  });
});

test('HS2-Y32T8A keeps the row star working for a provider that supports Up Next', async ({ page }) => {
  test.setTimeout(120_000);
  await withServer(page, async (server, { ready }) => {
    await open(page, server, true);
    const star = rowStar(rowOf(page, ready.slug));
    await expect(star).toBeEnabled();
    await expect(star).toHaveAttribute('title', 'Add to Up Next');
    await star.click();
    await expect.poll(() => upNextOf(server, ready.id)).toBe(true);
    await expect(star).toHaveAttribute('title', 'Remove from Up Next');
  });
});
