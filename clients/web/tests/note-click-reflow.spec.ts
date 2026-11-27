import { expect, type Locator, type Page, test } from '@playwright/test';

import { openMockedProject } from './open-mocked-project';
import { proxyFetch, realTicketServer } from './real-ticket-server';

// HS2-628K80: pressing a note while another note's editor is open blurs that editor, which saves
// and closes before the button is released. Closing it re-flows the notes list, so the release
// lands elsewhere and the browser sends the click to the list, not the pressed note. The press
// still opens the pressed note's editor; a drag that moves the pointer does not. Runs against the
// real ticket server in Chromium and WebKit (`--browser webkit`), where Safari has no scroll
// anchoring to hide the re-flow.

type Server = Awaited<ReturnType<typeof realTicketServer>>;

async function openRealProject(page: Page, server: Server) {
  const apiPath = `/__hotsheet/project-api/${server.checkoutId}`;
  // Change delivery falls back from the (unproxied) WebSocket to the forwarded long poll.
  await page.routeWebSocket(/.*/, (route) => {
    void route.close();
  });
  await page.route(/\/__hotsheet\/(?:remote\/)?projects\/open$/, (route) =>
    route.fulfill({
      status: 201,
      json: { id: server.checkoutId, root: server.root, name: 'reflow', stores: [server.store], apiPath },
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
      await route.fulfill({ response });
    } catch {
      await route.abort().catch(() => undefined);
    }
  });
  await page.setViewportSize({ width: 1280, height: 1400 });
  await page.goto('/');
  await openMockedProject(page);
}

/** Press `target`, let the open note editor close and re-flow the list, then release in place. */
async function pressAcrossReflow(page: Page, host: Locator, target: Locator, moveBy = 0) {
  const before = (await target.boundingBox())!,
    x = before.x + 8,
    y = before.y + before.height / 2;
  await page.mouse.move(x, y);
  await page.mouse.down();
  // The first note's editor closes on the press, before the release.
  await expect(host.getByRole('textbox', { name: 'Note body' })).toHaveCount(0);
  const after = (await target.boundingBox())!;
  // The re-flow moved the pressed note out from under the pointer.
  expect(Math.abs(after.y - before.y)).toBeGreaterThan(before.height);
  if (moveBy) await page.mouse.move(x + moveBy, y, { steps: 4 });
  await page.mouse.up();
}

for (const surface of ['inspector', 'reader'] as const)
  test(`a note pressed while another note is open opens its editor after the re-flow (${surface})`, async ({
    page,
  }) => {
    const server = await realTicketServer();
    try {
      const ticket = await server.request<{ id: string; slug: string }>('/tickets', 'POST', {
        title: 'Note re-flow',
        details: 'Details for the re-flow fixture.',
      });
      for (const note of ['First note body.', 'Second note body.', 'Third note body.'])
        await server.request(`/tickets/${ticket.id}`, 'PATCH', { note });
      await openRealProject(page, server);
      await page.locator(`[data-action="select-ticket-row"][data-ticket-slug="${ticket.slug}"]`).click();
      let host = page.locator('#app-right-rail');
      await expect(host.getByRole('button', { name: 'Edit Ticket details' })).toContainText('Details for');
      if (surface === 'reader') {
        await host.getByRole('button', { name: 'Open Ticket Reader' }).click();
        host = page.getByRole('dialog', { name: /Read and edit/ });
      }
      const cards = host.locator('[data-component="note-card"]'),
        first = cards.filter({ hasText: 'First note body.' }),
        second = cards.filter({ hasText: 'Second note body.' }),
        editor = host.getByRole('textbox', { name: 'Note body' });

      // A drag that ends away from the pressed note is not a click: nothing opens.
      await first.locator('.note-card__body').click();
      await expect(first.getByRole('textbox', { name: 'Note body' })).toBeFocused();
      await pressAcrossReflow(page, host, second.locator('.note-card__body'), 40);
      await expect(editor).toHaveCount(0);

      // A still click opens the pressed note even though its release landed on the list.
      await first.locator('.note-card__body').click();
      await expect(first.getByRole('textbox', { name: 'Note body' })).toBeFocused();
      await pressAcrossReflow(page, host, second.locator('.note-card__body'));
      await expect(second.getByRole('textbox', { name: 'Note body' })).toBeFocused();
      await expect(editor).toHaveCount(1);
      await expect(editor).toHaveValue('Second note body.');
      await editor.blur();
      await expect(editor).toHaveCount(0);
      await expect(second).toContainText('Second note body.');
    } finally {
      await server.stop(page);
    }
  });
