import { expect, type Page, test } from '@playwright/test';

import { realTicketServer } from './real-ticket-server';

interface Report {
  kind: string;
  label: string;
  duration_ms: number;
  reasons: string[];
  requests?: { label: string; duration_ms: number; server_ms?: number }[];
  server_ms?: number;
  trace_id?: string;
  stages?: { headers_ms?: number; body_ms?: number; paint_ms?: number; bridge_ms?: number; upstream_ms?: number };
  message: string;
}

const reports = (page: Page) =>
  page.evaluate(
    () =>
      (window as unknown as { __hotsheetDevLatency?: { reports(): Report[] } }).__hotsheetDevLatency?.reports() ?? [],
  );

// HS2-ZGBQ5Q: in a development build, a click whose result takes longer than the 150 ms peak budget is
// flagged with a console warning, a non-blocking dev toast, and a report to the dev bridge's log; the
// report names the slow request and the server's own handler time.
for (const viewport of [
  { name: 'wide', width: 1280, height: 900 },
  { name: 'narrow', width: 390, height: 844 },
]) {
  test(`flags a slow click and its slow request in dev mode (${viewport.name}) (HS2-ZGBQ5Q)`, async ({ page }) => {
    test.setTimeout(120_000);
    const server = await realTicketServer();
    const bridgePosts: { reports: Report[] }[] = [],
      warnings: string[] = [];
    let slowTicket: string | undefined;
    try {
      const ticket = await server.request<{ id: string }>('/tickets', 'POST', {
        title: 'Slow to open',
        category: 'task',
      });
      const apiPath = `/__hotsheet/project-api/${server.checkoutId}`;
      await page.addInitScript(() => {
        localStorage.setItem('hotsheet-dev-latency', 'on');
      });
      page.on('console', (message) => {
        if (message.type() === 'warning' && message.text().includes('[hotsheet dev latency]'))
          warnings.push(message.text());
      });
      await page.routeWebSocket(/.*/, (route) => {
        void route.close();
      });
      await page.route('**/__hotsheet/dev/latency', async (route) => {
        bridgePosts.push(route.request().postDataJSON() as { reports: Report[] });
        await route.fulfill({ status: 200, json: { accepted: 1 } });
      });
      await page.route('**/__hotsheet/projects/open', (route) =>
        route.fulfill({
          status: 201,
          json: { id: server.checkoutId, root: server.root, name: 'latency', stores: [server.store], apiPath },
        }),
      );
      await page.route(`**${apiPath}/**`, async (route) => {
        const incoming = new URL(route.request().url()),
          path = incoming.pathname.slice(apiPath.length);
        // Simulate a backed-up server for the ticket's detail reads once the test arms it.
        if (slowTicket && route.request().method() === 'GET' && path.includes(slowTicket) && !path.includes('/ws/'))
          await new Promise((resolve) => setTimeout(resolve, 400));
        try {
          const response = await route.fetch({
            url: `${server.url}${path}${incoming.search}`,
            headers: { ...route.request().headers(), 'X-Hotsheet-Secret': server.secret },
            timeout: 60_000,
          });
          await route.fulfill({
            response,
            headers: {
              ...response.headers(),
              'server-timing': `${response.headers()['server-timing'] ?? 'handler;dur=1'}, bridge;dur=5, upstream;dur=3`,
              'x-hotsheet-trace-id': 'trace-fixture',
            },
          });
        } catch {
          await route.abort().catch(() => undefined);
        }
      });
      await page.setViewportSize({ width: viewport.width, height: viewport.height });
      await page.goto('/');
      await page.getByRole('button', { name: 'Open Project' }).click();
      await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
      const row = page.locator('[data-action="select-ticket-row"]').filter({ hasText: 'Slow to open' });
      await expect(row).toBeVisible({ timeout: 30_000 });
      // Let project-open work settle, then forget its reports so only the armed click is judged.
      await page.waitForTimeout(500);
      const before = (await reports(page)).length;

      slowTicket = ticket.id;
      await row.click();
      await expect
        .poll(async () => (await reports(page)).slice(before).find((report) => report.kind === 'interaction'), {
          timeout: 15_000,
        })
        .toBeTruthy();
      const interaction = (await reports(page)).slice(before).find((report) => report.kind === 'interaction')!;
      expect(interaction.label).toContain('click');
      expect(interaction.reasons).toContain('peak');
      expect(interaction.duration_ms).toBeGreaterThan(400);
      const slowRequest = interaction.requests?.find((request) => request.duration_ms >= 400);
      expect(slowRequest?.label).toMatch(/^GET \/.*\{id\}/);
      // The server itself was fast: Server-Timing shows the delay was not in the handler.
      expect(slowRequest?.server_ms).toBeLessThan(400);
      expect(interaction.message).toContain('slowest request GET');
      // The slow request is flagged on its own too, with ids templated out.
      await expect
        .poll(async () => (await reports(page)).slice(before).find((report) => report.kind === 'request'))
        .toBeTruthy();
      const request = (await reports(page)).slice(before).find((report) => report.kind === 'request')!;
      expect(request.label).not.toContain(ticket.id);
      expect(request.stages?.headers_ms).toBeGreaterThanOrEqual(400);
      expect(request.stages?.body_ms).toBeGreaterThanOrEqual(0);
      expect(request.stages?.paint_ms).toBeGreaterThanOrEqual(0);
      expect(request.stages?.bridge_ms).toBe(5);
      expect(request.stages?.upstream_ms).toBe(3);
      expect(request.trace_id).toBe('trace-fixture');

      // Visible, non-blocking dev indicator.
      const toast = page.locator('[data-dev-latency-toast]');
      await expect(toast).toBeVisible();
      await expect(toast).toContainText('Dev: slow interaction');
      await page.screenshot({ path: `target/visual-captures/hs2-zgbq5q-dev-latency-${viewport.width}.png` });
      // The app stays usable underneath it.
      await expect(row).toBeVisible();

      // Console warning and one debounced bridge report batch.
      expect(warnings.some((line) => line.includes('Slow interaction'))).toBe(true);
      await expect
        .poll(() => bridgePosts.flatMap((post) => post.reports).some((report) => report.kind === 'interaction'))
        .toBe(true);
    } finally {
      await server.stop();
    }
  });
}

test('stays off under automation unless explicitly enabled (HS2-ZGBQ5Q)', async ({ page }) => {
  await page.goto('/');
  await expect(page.getByRole('button', { name: 'Open Project' })).toBeVisible();
  expect(await page.evaluate(() => '__hotsheetDevLatency' in window)).toBe(false);
});
