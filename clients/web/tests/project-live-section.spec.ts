import { expect, type Page, test } from '@playwright/test';

import { realTicketServer } from './real-ticket-server';

type Server = Awaited<ReturnType<typeof realTicketServer>>;

/** Open the real app on an isolated server through the dev proxy, as archive-load-more does. */
async function openProject(page: Page, server: Server) {
  const apiPath = `/__hotsheet/project-api/${server.checkoutId}`;
  await page.route('**/__hotsheet/projects/open', (route) =>
    route.fulfill({
      status: 201,
      json: { id: server.checkoutId, root: server.root, name: 'live', stores: [server.store], apiPath },
    }),
  );
  await page.route(`**${apiPath}/**`, async (route) => {
    const incoming = new URL(route.request().url()),
      path = incoming.pathname.slice(apiPath.length);
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
  await page.goto('/');
  await page.getByRole('button', { name: 'Open Project' }).click();
  await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
}

// HS2-F788GW: the project sidebar's Live section, from the server's real `/live-state` and the change
// stream, compact by default; the header action switches to the detail rows and the choice survives a
// reload, and the section sits under the completion chart.
test('project sidebar Live section is compact by default and remembers the detail mode (HS2-F788GW)', async ({
  page,
}) => {
  test.setTimeout(120_000);
  const server = await realTicketServer();
  try {
    const ready = await server.request<{ id: string }>('/tickets', 'POST', { title: 'Ready work', category: 'task' });
    await server.request(`/tickets/${ready.id}`, 'PATCH', { up_next: true });
    // Change delivery falls back from the (unproxied) WebSocket to the forwarded long poll.
    await page.routeWebSocket(/.*/, (route) => {
      void route.close();
    });
    await page.setViewportSize({ width: 1280, height: 900 });
    await openProject(page, server);

    const sidebar = page.locator('section[aria-label="Project sidebar"]').first();
    const live = sidebar.locator('[data-component="project-live-section"]');
    await expect(live).toHaveAttribute('data-mode', 'compact', { timeout: 30_000 });
    // Under the completion chart, above the repository summary.
    const order = await sidebar.evaluate((root) =>
      ['project-summary', 'project-live-section', 'repository-summary'].map((name) => {
        const node = root.querySelector(`[data-component="${name}"]`);
        return node ? node.getBoundingClientRect().top : -1;
      }),
    );
    expect(order[0]).toBeGreaterThanOrEqual(0);
    expect(order[1]).toBeGreaterThan(order[0]);
    expect(order[2]).toBeGreaterThan(order[1]);

    // Compact: two bare chip gauges named by their titles; the runway chip says what it means only
    // in its title and accessible name, and its face is the raw number.
    const gauges = live.locator('svg.glance-gauge');
    await expect(gauges).toHaveCount(2);
    await expect(gauges.nth(0)).toHaveAttribute('data-variant', 'bare');
    await expect(gauges.nth(0)).toHaveAccessibleName(/^Token rate: 0 tokens\/min/);
    await expect(gauges.nth(1)).toHaveAccessibleName(/^Tickets updated: \d+ tickets\/h/);
    const runway = live.locator('[data-live-chip="runway"]');
    await expect(runway).toHaveText('1');
    await expect(runway).toHaveAttribute('title', /^Queue runway.* · 1 ready$/);
    await expect(live.locator('[data-live-chip="waiting"], [data-live-chip="overrun"]')).toHaveCount(0);
    await expect(live.locator('[data-live-row]')).toHaveCount(0);

    // The header action switches to the detail rows and names the way back.
    await live.getByRole('button', { name: 'Show live details' }).click();
    await expect(live).toHaveAttribute('data-mode', 'expanded');
    await expect(live.locator('svg.glance-gauge[data-variant="full"]')).toHaveCount(2);
    await expect(live.locator('[data-live-row="state"]')).toBeVisible();
    await expect(live.locator('[data-live-row="runway"]')).toContainText('1 ready');
    await expect(live.locator('[data-live-row="waiting"]')).toHaveCount(0);
    await page.screenshot({ path: 'target/visual-captures/hs2-f788gw-live-expanded-1280.png' });

    // The choice persists per viewer across a reload, which restores the open project.
    await page.reload();
    await expect(live).toHaveAttribute('data-mode', 'expanded', { timeout: 30_000 });
    await live.getByRole('button', { name: 'Show compact live summary' }).click();
    await expect(live).toHaveAttribute('data-mode', 'compact');
    await expect(live.locator('[data-live-row]')).toHaveCount(0);
    await page.screenshot({ path: 'target/visual-captures/hs2-f788gw-live-compact-1280.png' });
  } finally {
    await server.stop();
  }
});
