import { expect, test } from '@playwright/test';

import { realTicketServer } from './real-ticket-server';

// HS2-9DF0DA: on a live project, another writer's change refreshes the open list. That refresh used
// to fetch only the first 200 rows and replace the list, so rows appended by Load more flashed and
// vanished. A refresh now brings back every row already loaded for the same query.
test('keeps rows appended by Load more when a concurrent change refreshes the list (HS2-9DF0DA)', async ({ page }) => {
  test.setTimeout(180_000);
  const server = await realTicketServer();
  const ticketRequests: string[] = [];
  try {
    const ids: string[] = [];
    for (let start = 0; start < 230; start += 23)
      ids.push(
        ...(
          await Promise.all(
            Array.from({ length: 23 }, (_, offset) =>
              server.request<{ id: string }>('/tickets', 'POST', { title: `Archived ${start + offset}` }),
            ),
          )
        ).map((ticket) => ticket.id),
      );
    await server.request('/batch', 'POST', { ids, status: 'archive' });
    const apiPath = `/__hotsheet/project-api/${server.checkoutId}`;
    // Change delivery falls back from the (unproxied) WebSocket to the forwarded long poll.
    await page.routeWebSocket(/.*/, (route) => {
      void route.close();
    });
    await page.route('**/__hotsheet/projects/open', (route) =>
      route.fulfill({
        status: 201,
        json: { id: server.checkoutId, root: server.root, name: 'archive', stores: [server.store], apiPath },
      }),
    );
    await page.route(`**${apiPath}/**`, async (route) => {
      const incoming = new URL(route.request().url()),
        path = incoming.pathname.slice(apiPath.length);
      if (/\/tickets$/.test(path) && route.request().method() === 'GET')
        ticketRequests.push(incoming.searchParams.get('page_size') ?? '');
      try {
        const response = await route.fetch({
          url: `${server.url}${path}${incoming.search}`,
          headers: { ...route.request().headers(), 'X-Hotsheet-Secret': server.secret },
          timeout: 60_000,
        });
        await route.fulfill({ response });
      } catch {
        await route.abort().catch(() => undefined);
      }
    });
    await page.setViewportSize({ width: 1280, height: 900 });
    await page.goto('/');
    await page.getByRole('button', { name: 'Open Project' }).click();
    await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
    await page
      .getByRole('button', { name: /^Archive/ })
      .first()
      .click();
    const rows = page.locator('[data-action="select-ticket-row"]');
    await expect(rows).toHaveCount(200, { timeout: 30_000 });
    await page.getByRole('button', { name: 'Load more tickets' }).click();
    await expect(rows).toHaveCount(230);
    await expect(page.getByRole('button', { name: 'Load more tickets' })).toHaveCount(0);

    // Another writer changes the project; the change event refreshes the open Archive list.
    ticketRequests.length = 0;
    await server.request('/tickets', 'POST', { title: 'Concurrent change', category: 'task' });
    await expect.poll(() => ticketRequests.length, { timeout: 20_000 }).toBeGreaterThan(0);
    // The refresh asked for every loaded row, not just the first page, and the list kept them.
    expect(Math.max(...ticketRequests.map(Number))).toBeGreaterThanOrEqual(230);
    await page.waitForTimeout(1_000);
    await expect(rows).toHaveCount(230);
    await page.screenshot({ path: 'target/visual-captures/hs2-9df0da-archive-after-change-1280.png' });
  } finally {
    await server.stop();
  }
});
