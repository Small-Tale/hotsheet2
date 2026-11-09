import { expect, type Locator, test } from '@playwright/test';

import type { FullTicket } from '../src/api';
import { realTicketServer } from './real-ticket-server';

// HS2-1NBJZH: compare the actual painted list/column surfaces with a private real server.
test('quietly paints ordinary AI ticket surfaces without dimming content', async ({ page }, testInfo) => {
  test.setTimeout(120_000);
  const server = await realTicketServer();
  try {
    const create = (title: string, actor?: { role: string; id?: string }) =>
      server.request<FullTicket>('/tickets', 'POST', { title, category: 'task', tags: ['synthetic'], actor });
    const ai = await create('Synthetic agent request', { role: 'ai', id: 'claude-fixture' });
    const human = await create('Synthetic person request', { role: 'human' });
    const unknown = await create('Synthetic legacy request');
    const apiPath = `/__hotsheet/project-api/${server.checkoutId}`;
    await page.routeWebSocket(/.*/, (route) => {
      void route.close();
    });
    await page.route('**/__hotsheet/projects/open', (route) =>
      route.fulfill({
        status: 201,
        json: { id: server.checkoutId, root: server.root, name: 'synthetic', stores: [server.store], apiPath },
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
    await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();

    const surfaceAlpha = (row: Locator) =>
      row.locator('[data-component="summary-row"]').evaluate((node) => {
        const canvas = document.createElement('canvas');
        canvas.width = canvas.height = 1;
        const context = canvas.getContext('2d')!;
        context.fillStyle = getComputedStyle(node).backgroundColor;
        context.fillRect(0, 0, 1, 1);
        return context.getImageData(0, 0, 1, 1).data[3];
      });
    for (const presentation of ['list', 'column'] as const) {
      if (presentation === 'column') await page.getByLabel('Columns View').click();
      const view = page.locator(
        presentation === 'list' ? '[data-component="ticket-list"]' : '[data-component="ticket-board"]',
      );
      const row = (slug: string) =>
        view.locator(`[data-component="ticket-list-row-container"]:has([data-ticket-slug="${slug}"])`);
      await expect(row(ai.slug)).toBeVisible();
      for (const theme of ['light', 'dark'] as const) {
        await page.emulateMedia({ colorScheme: theme });
        for (const [index, width] of [1440, 900, 390, 1440].entries()) {
          await page.setViewportSize({ width, height: 900 });
          const agent = row(ai.slug);
          await expect(agent).toBeVisible();
          await expect(agent).toHaveAttribute('data-row-quiet-ai-surface', 'true');
          await expect(row(human.slug)).toHaveAttribute('data-row-quiet-ai-surface', 'false');
          await expect(row(unknown.slug)).toHaveAttribute('data-row-quiet-ai-surface', 'false');
          expect(await surfaceAlpha(agent)).toBeGreaterThanOrEqual(126);
          expect(await surfaceAlpha(agent)).toBeLessThanOrEqual(129);
          expect(await surfaceAlpha(row(human.slug))).toBe(255);
          expect(await surfaceAlpha(row(unknown.slug))).toBe(255);
          expect(
            await agent.locator('.ticket-list-row__title').evaluate((node) => getComputedStyle(node).opacity),
          ).toBe('1');
          await page.screenshot({
            path: testInfo.outputPath(`hs2-1nbjzh-${presentation}-${theme}-${index}-${width}.png`),
            animations: 'disabled',
          });
        }
      }
    }
    const selectedAi = page
      .locator('[data-component="ticket-board"]')
      .locator(`[data-component="ticket-list-row-container"]:has([data-ticket-slug="${ai.slug}"])`);
    await selectedAi.click();
    await expect(selectedAi).toHaveAttribute('data-row-quiet-ai-surface', 'false');
    expect(await surfaceAlpha(selectedAi)).toBe(255);
  } finally {
    await page.unrouteAll({ behavior: 'ignoreErrors' });
    await server.stop();
  }
});
