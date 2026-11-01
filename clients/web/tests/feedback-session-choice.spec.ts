import { expect, test } from '@playwright/test';

import type { FullTicket } from '../src/api';
import { realTicketServer } from './real-ticket-server';

test('HS2-25TP3N saves human feedback and offers a project session chooser across resize', async ({ page }) => {
  const server = await realTicketServer();
  try {
    const created = await server.request<FullTicket>('/tickets', 'POST', { title: 'Synthetic session feedback' });
    await server.request(`/tickets/${created.id}`, 'PATCH', { status: 'started' });
    const apiPath = `/__hotsheet/project-api/${server.checkoutId}`;
    const notifications: Array<{ path: string; connection_id: string }> = [];
    await page.routeWebSocket(/.*/, (route) => {
      void route.close();
    });
    await page.route('**/__hotsheet/projects/open', (route) =>
      route.fulfill({
        status: 201,
        json: { id: server.checkoutId, root: server.root, name: 'feedback', stores: [server.store], apiPath },
      }),
    );
    await page.route(`**${apiPath}/**`, async (route) => {
      const request = route.request(),
        incoming = new URL(request.url()),
        path = incoming.pathname.slice(apiPath.length);
      if (request.method() === 'POST' && /\/notes\/[^/]+\/notify$/u.test(path)) {
        notifications.push({
          path,
          connection_id: (request.postDataJSON() as { connection_id: string }).connection_id,
        });
        await route.fulfill({
          json: { status: 'sent', note_id: path.split('/').at(-2), connection_id: 'session-123456' },
        });
        return;
      }
      const response = await route.fetch({
        url: `${server.url}${path}${incoming.search}`,
        headers: { ...request.headers(), 'X-Hotsheet-Secret': server.secret },
      });
      if (request.method() === 'PATCH' && /\/checkouts\/[^/]+\/tickets\/[^/]+$/u.test(path)) {
        const body = request.postDataJSON() as Record<string, unknown>;
        if (typeof body.note === 'string') {
          const saved = (await response.json()) as FullTicket;
          expect(body.actor).toEqual({ role: 'human' });
          await route.fulfill({
            response,
            json: {
              ...saved,
              feedback_delivery: {
                status: 'choose',
                note_id: saved.notes.at(-1)?.id,
                sessions: [
                  { id: 'session-123456', tool: 'codex' },
                  { id: 'session-654321', tool: 'claude' },
                ],
              },
            },
          });
          return;
        }
      }
      await route.fulfill({ response });
    });
    await page.setViewportSize({ width: 1280, height: 850 });
    await page.goto('/');
    await page.getByRole('button', { name: 'Open Project' }).click();
    await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
    await page.locator(`[data-action="select-ticket-row"][data-ticket-slug="${created.slug}"]`).click();
    const inspector = page.locator('#app-right-rail');
    await inspector.getByRole('button', { name: 'Add Note' }).first().click();
    const editor = inspector.getByRole('textbox', { name: 'New note' });
    await editor.fill('Synthetic human feedback');
    await editor.blur();
    const choice = inspector.locator('[data-component="feedback-session-choice"]');
    await expect(choice).toBeVisible();
    await expect(choice.getByRole('button', { name: 'codex 123456' })).toBeVisible();
    await page.screenshot({ path: test.info().outputPath('feedback-chooser-wide.png') });
    await page.setViewportSize({ width: 768, height: 850 });
    await page.getByRole('button', { name: 'Show Ticket Inspector' }).click();
    await expect(choice).toBeVisible();
    await page.waitForTimeout(500);
    await expect
      .poll(async () => {
        const box = await choice.boundingBox();
        return Boolean(box && box.x >= 0 && box.x + box.width <= 768);
      })
      .toBe(true);
    await page.screenshot({ path: test.info().outputPath('feedback-chooser-medium.png') });
    await page.setViewportSize({ width: 390, height: 844 });
    if (await page.getByRole('button', { name: 'Show Ticket Inspector' }).isVisible())
      await page.getByRole('button', { name: 'Show Ticket Inspector' }).click();
    await expect(choice).toBeVisible();
    await page.waitForTimeout(500);
    await expect
      .poll(async () => {
        const box = await choice.boundingBox();
        return Boolean(box && box.x >= 0 && box.x + box.width <= 390);
      })
      .toBe(true);
    await page.screenshot({ path: test.info().outputPath('feedback-chooser-narrow.png') });
    await page.setViewportSize({ width: 1280, height: 850 });
    await expect(choice).toBeVisible();
    await page.waitForTimeout(500);
    await page.screenshot({ path: test.info().outputPath('feedback-chooser-wide-return.png') });
    await choice.getByRole('button', { name: 'codex 123456' }).click();
    await expect(choice).toHaveCount(0);
    expect(notifications).toHaveLength(1);
    expect(notifications[0]?.connection_id).toBe('session-123456');
    const stored = await server.request<FullTicket>(`/tickets/${created.id}`);
    expect(stored.notes.some((note) => note.text === 'Synthetic human feedback')).toBe(true);
  } finally {
    await server.stop();
  }
});
