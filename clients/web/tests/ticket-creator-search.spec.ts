import { expect, test } from '@playwright/test';

import type { FullTicket } from '../src/api';
import { realTicketServer } from './real-ticket-server';

test('searches recorded AI ticket creators through a bounded real checkout (HS2-18QFJ5)', async ({
  page,
}, testInfo) => {
  test.setTimeout(120_000);
  const server = await realTicketServer();
  try {
    const create = (title: string, actor?: { role: string }) =>
      server.request<FullTicket>('/tickets', 'POST', { title, category: 'task', actor });
    const unknown = await create('Synthetic legacy ticket');
    const human = await create('Synthetic human ticket', { role: 'human' });
    const ai = await create('Synthetic AI ticket', { role: 'ai' });
    const apiPath = `/__hotsheet/project-api/${server.checkoutId}`;
    const ticketRequests: string[] = [];
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
      const incoming = new URL(route.request().url());
      if (incoming.pathname.endsWith('/tickets')) ticketRequests.push(incoming.search);
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
    await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
    await page.getByRole('button', { name: 'Search tickets' }).click();
    const search = page.getByRole('searchbox', { name: 'Search tickets' });
    const result = (slug: string) => page.locator(`[data-component="ticket-list"] [data-ticket-slug="${slug}"]`);
    ticketRequests.length = 0;
    await search.fill('IS:AI-CREATED');
    await search.press('Enter');
    await expect(page.locator('[data-component="search-field-token"][data-token-value="is:AI-CREATED"]')).toBeVisible();
    await expect(result(ai.slug)).toBeVisible();
    await expect(result(human.slug)).toHaveCount(0);
    await expect(result(unknown.slug)).toHaveCount(0);
    expect(ticketRequests.some((query) => new URLSearchParams(query).get('ai_created') === 'true')).toBe(true);
    await page.screenshot({ path: testInfo.outputPath('hs2-18qfj5-wide.png'), animations: 'disabled' });
    await page.setViewportSize({ width: 800, height: 900 });
    await expect(result(ai.slug)).toBeVisible();
    await page.screenshot({ path: testInfo.outputPath('hs2-18qfj5-medium.png'), animations: 'disabled' });
    await page.setViewportSize({ width: 390, height: 844 });
    await expect(result(ai.slug)).toBeVisible();
    await page.screenshot({ path: testInfo.outputPath('hs2-18qfj5-phone.png'), animations: 'disabled' });
    await page.setViewportSize({ width: 1440, height: 900 });
    await expect(result(ai.slug)).toBeVisible();
    await page.screenshot({ path: testInfo.outputPath('hs2-18qfj5-returned-wide.png'), animations: 'disabled' });
    await page.getByRole('button', { name: 'Clear search' }).click();
    ticketRequests.length = 0;
    await search.fill('NOT is:ai-created');
    await expect(result(human.slug)).toBeVisible();
    await expect(result(unknown.slug)).toBeVisible();
    await expect(result(ai.slug)).toHaveCount(0);
    expect(ticketRequests.every((query) => !new URLSearchParams(query).has('ai_created'))).toBe(true);
    await page.screenshot({ path: testInfo.outputPath('hs2-18qfj5-not-wide.png'), animations: 'disabled' });
  } finally {
    await page.unrouteAll({ behavior: 'ignoreErrors' });
    await server.stop();
  }
});
