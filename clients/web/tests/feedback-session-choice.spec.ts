import { expect, test } from '@playwright/test';

import type { FullTicket } from '../src/api';
import { openMockedProject } from './open-mocked-project';
import { proxyFetch, realTicketServer } from './real-ticket-server';

test('HS2-25TP3N saves human feedback and offers a project session chooser across resize', async ({ page }) => {
  const server = await realTicketServer();
  try {
    const created = await server.request<FullTicket>('/tickets', 'POST', { title: 'Synthetic session feedback' });
    const other = await server.request<FullTicket>('/tickets', 'POST', { title: 'Synthetic other ticket' });
    await server.request(`/tickets/${created.id}`, 'PATCH', { status: 'started' });
    // The codex session's worker holds the live claim, so its choice names this ticket (HS2-5MCAKY).
    await server.request(`/tickets/${created.id}/claim`, 'POST', { worker: 'synthetic-codex-worker' });
    const apiPath = `/__hotsheet/project-api/${server.checkoutId}`;
    const notifications: Array<{ path: string; connection_id: string }> = [];
    await page.routeWebSocket(/.*/, (route) => {
      void route.close();
    });
    await page.route(/\/__hotsheet\/(?:remote\/)?projects\/open$/, (route) =>
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
      const response = await proxyFetch(route, {
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
                  { id: 'session-123456', tool: 'codex', worker: 'synthetic-codex-worker' },
                  { id: 'session-654321', tool: 'claude', worker: 'synthetic-idle-worker' },
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
    await openMockedProject(page);
    await page.locator(`[data-action="select-ticket-row"][data-ticket-slug="${created.slug}"]`).click();
    const inspector = page.locator('#app-right-rail');
    await inspector.getByRole('button', { name: 'Add Note' }).first().click();
    const editor = inspector.getByRole('textbox', { name: 'New note' });
    await editor.fill('Synthetic human feedback');
    await editor.blur();
    // The choice is an app-level prompt like a permission request, not an inspector banner (HS2-6RXBS9).
    const choice = page.locator('[data-component="note-delivery-popup"]');
    await expect(choice).toBeVisible();
    await expect(inspector.locator('[data-component="note-delivery-popup"]')).toHaveCount(0);
    await expect(choice).toContainText(created.slug);
    await expect(choice).toContainText('feedback');
    // Choices name the session's tab (or tool), project, and claimed ticket, never a raw id (HS2-5MCAKY).
    const codex = choice.getByRole('button', { name: `Codex · feedback · ${created.slug}` }),
      claude = choice.getByRole('button', { name: 'Claude · feedback', exact: true });
    await expect(codex).toBeVisible();
    await expect(claude).toBeVisible();
    await expect(codex).toHaveAttribute(
      'title',
      new RegExp(`working on ${created.slug} .*\\(session session-123456\\)`),
    );
    await expect(choice).not.toContainText('123456');
    await page.screenshot({ path: test.info().outputPath('feedback-chooser-wide.png') });
    // Selecting another ticket leaves the prompt up: it belongs to the note, not the inspector.
    await page.locator(`[data-action="select-ticket-row"][data-ticket-slug="${other.slug}"]`).click();
    await expect(inspector.getByRole('heading', { name: 'Synthetic other ticket' }).first()).toBeVisible();
    await expect(choice).toBeVisible();
    // Hiding the inspector leaves the prompt up.
    await page.getByRole('button', { name: 'Hide Ticket Inspector' }).click();
    await expect(choice).toBeVisible();
    await page.setViewportSize({ width: 768, height: 850 });
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
    await codex.click();
    await expect(choice).toHaveCount(0);
    await expect(page.locator('.app-toast, [data-component="app-toast"]').first()).toContainText('notified');
    expect(notifications).toHaveLength(1);
    expect(notifications[0]?.connection_id).toBe('session-123456');
    // A second note's prompt can be dismissed without notifying anyone.
    await page.locator(`[data-action="select-ticket-row"][data-ticket-slug="${created.slug}"]`).click();
    if (await page.getByRole('button', { name: 'Show Ticket Inspector' }).isVisible())
      await page.getByRole('button', { name: 'Show Ticket Inspector' }).click();
    await inspector.getByRole('button', { name: 'Add Note' }).first().click();
    await editor.fill('Second synthetic feedback');
    await editor.blur();
    await expect(choice).toBeVisible();
    await choice.getByRole('button', { name: 'Dismiss' }).click();
    await expect(choice).toHaveCount(0);
    expect(notifications).toHaveLength(1);
    const stored = await server.request<FullTicket>(`/tickets/${created.id}`);
    expect(stored.notes.some((note) => note.text === 'Synthetic human feedback')).toBe(true);
  } finally {
    await page.unrouteAll({ behavior: 'wait' });
    await server.stop();
  }
});
