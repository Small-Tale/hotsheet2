import { expect, type Page, test } from '@playwright/test';

import type { FullTicket } from '../src/api';
import type { DesktopCloseHost } from '../src/desktop-close-contract';
import { ticketDraftKey } from '../src/ticket-draft-store';
import { desktopApp, dialog, openNewTicket } from './desktop-draft-files-fixture';

async function openDetails(page: Page, ticket: FullTicket) {
  await page.locator(`[data-component="summary-row"][data-ticket-slug="${ticket.slug}"]`).click();
  const parent = page.locator('#app-right-rail');
  await parent.locator('[data-action="edit-markdown"]').click({ position: { x: 4, y: 4 } });
  const editor = parent.getByRole('textbox', { name: 'Ticket details' });
  await expect(editor).toBeFocused();
  return editor;
}

for (const field of ['details', 'note'] as const) {
  test(`preserves exact ${field} whitespace when old save finishes during native drain (HS2-E0H7S0)`, async ({
    context,
    baseURL,
  }) => {
    const acks: { request: string; success: boolean }[] = [];
    await context.exposeFunction('recordClose', (ack: { request: string; success: boolean }) => acks.push(ack));
    await context.addInitScript(() => {
      const host = window as unknown as DesktopCloseHost & { recordClose(ack: unknown): Promise<unknown> };
      host.__TAURI_INTERNALS__ = { invoke: async (_command, ack) => host.recordClose(ack) };
    });
    const app = await desktopApp(context, baseURL!);
    let releaseOld = () => {},
      releaseBridge = () => {};
    try {
      let ticket = await app.server.request<FullTicket>(`/checkouts/${app.server.checkoutId}/tickets`, 'POST', {
        title: 'Exact recovery owner',
        category: 'task',
        details: 'Synthetic base',
        up_next: true,
      });
      if (field === 'note')
        ticket = await app.server.request<FullTicket>(
          `/checkouts/${app.server.checkoutId}/tickets/${encodeURIComponent(ticket.qualified_id)}`,
          'PATCH',
          { note: 'Synthetic note base', expected_token: ticket.concurrency_token },
        );
      let page = await app.launch();
      await openNewTicket(page, true);
      await dialog(page).getByRole('button', { name: 'Cancel', exact: true }).click();
      const open = async () => {
        if (field === 'details') return openDetails(page, ticket);
        await page.locator(`[data-component="summary-row"][data-ticket-slug="${ticket.slug}"]`).click();
        const card = page.locator(`#app-right-rail article[data-note-id="${ticket.notes[0].id}"]`);
        await card.click({ position: { x: 12, y: 12 } });
        const editor = card.getByRole('textbox', { name: 'Note body', exact: true });
        await expect(editor).toBeFocused();
        return editor;
      };
      const editor = await open(),
        old = app.pauseApi((method, path) => method === 'PATCH' && path.includes('/tickets/'));
      releaseOld = old.release;
      await editor.fill('Submitted exact text');
      await editor.press('Tab');
      await old.started;
      // Only the older request succeeded. A newer retry must still need its local recovery copy.
      app.failApiWhen((method) => method === 'PATCH');
      const latest = 'Submitted exact text\n  ';
      await editor.fill(latest);
      const bridge = app.pauseBridge((method) => method === 'PUT');
      releaseBridge = bridge.release;
      await page.evaluate(() => {
        (window as unknown as DesktopCloseHost).__HOTSHEET_DESKTOP_CLOSE__!('1');
      });
      await bridge.started;
      expect(acks).toEqual([]);
      const recovery = () =>
        page.evaluate(() =>
          Object.keys(localStorage)
            .filter((key) => key.startsWith('hotsheet.ticket-draft:'))
            .map((key) => JSON.parse(localStorage.getItem(key)!) as { draft: string }),
        );
      expect(await recovery()).toEqual([expect.objectContaining({ draft: latest })]);
      releaseOld();
      // The real parent's completion closes this old edit surface after its save promise resolves.
      await expect(editor).toBeHidden();
      expect(await recovery()).toEqual([expect.objectContaining({ draft: latest })]);
      releaseBridge();
      await expect.poll(() => acks).toEqual([{ request: '1', success: true }]);
      expect(await recovery()).toEqual([expect.objectContaining({ draft: latest })]);
      // Inspect a fresh origin at the ACK boundary before any old-document pagehide can repair it.
      page = await app.launch();
      await expect.poll(recovery).toEqual([expect.objectContaining({ draft: latest })]);
      await expect(await open()).toHaveValue(latest);
    } finally {
      releaseOld();
      releaseBridge();
      await app.stop();
    }
  });
}

for (const conflict of [false, true]) {
  test(`old ${conflict ? 'conflicted' : 'successful'} save cannot clear equal-text recovery in a new ticket owner (HS2-E0H7S0)`, async ({
    context,
    baseURL,
  }) => {
    const acks: { request: string; success: boolean }[] = [];
    await context.exposeFunction('recordClose', (ack: { request: string; success: boolean }) => acks.push(ack));
    await context.addInitScript(() => {
      const host = window as unknown as DesktopCloseHost & { recordClose(ack: unknown): Promise<unknown> };
      host.__TAURI_INTERNALS__ = { invoke: async (_command, ack) => host.recordClose(ack) };
    });
    const app = await desktopApp(context, baseURL!);
    let releaseOld = () => {},
      releaseBridge = () => {};
    try {
      const create = (title: string, details: string) =>
          app.server.request<FullTicket>(`/checkouts/${app.server.checkoutId}/tickets`, 'POST', {
            title,
            details,
            category: 'task',
            up_next: true,
          }),
        a = await create('Old recovery owner', 'Original A'),
        b = await create('New recovery owner', 'Different B base');
      let page = await app.launch();
      await openNewTicket(page, true);
      await dialog(page).getByRole('button', { name: 'Cancel', exact: true }).click();
      const editorA = await openDetails(page, a),
        old = app.pauseApi((method, path) => method === 'PATCH' && path.includes('/tickets/'));
      if (conflict)
        await app.server.request(
          `/checkouts/${app.server.checkoutId}/tickets/${encodeURIComponent(a.qualified_id)}`,
          'PATCH',
          {
            title: 'Unrelated concurrent title',
            expected_token: a.concurrency_token,
          },
        );
      releaseOld = old.release;
      await editorA.fill('Equal latest text');
      await editorA.press('Tab');
      await old.started;
      app.failApiWhen((method, path) => method === 'PATCH' && path.includes(encodeURIComponent(b.qualified_id)));
      const editorB = await openDetails(page, b);
      await editorB.fill('Equal latest text');
      const bridge = app.pauseBridge((method) => method === 'PUT');
      releaseBridge = bridge.release;
      await page.evaluate(() => {
        (window as unknown as DesktopCloseHost).__HOTSHEET_DESKTOP_CLOSE__!('1');
      });
      await bridge.started;
      const key = ticketDraftKey(app.server.checkoutId, b.qualified_id, 'details'),
        recovery = () => page.evaluate((key) => localStorage.getItem(key), key);
      expect(JSON.parse((await recovery()) ?? '{}')).toMatchObject({
        base: 'Different B base',
        draft: 'Equal latest text',
      });
      const oldResponse = page.waitForResponse(
        (response) =>
          response.request().method() === 'PATCH' && response.url().includes(encodeURIComponent(a.qualified_id)),
      );
      releaseOld();
      // HS2-9S8CV1 tracks recognition of the real slug-bearing 409; this asserts failure preservation, not retry.
      expect((await oldResponse).status()).toBe(conflict ? 409 : 200);
      const committed = await app.server.request<FullTicket>(
        `/checkouts/${app.server.checkoutId}/tickets/${encodeURIComponent(a.qualified_id)}`,
      );
      expect(committed.details).toBe(conflict ? 'Original A' : 'Equal latest text');
      // Let the response's resulting parent render finish before releasing native storage drain.
      await page.evaluate(
        () =>
          new Promise<void>((resolve) =>
            requestAnimationFrame(() => {
              resolve();
            }),
          ),
      );
      releaseBridge();
      await expect.poll(() => acks).toEqual([{ request: '1', success: true }]);
      expect(JSON.parse((await recovery()) ?? '{}')).toMatchObject({
        base: 'Different B base',
        draft: 'Equal latest text',
      });
      page = await app.launch();
      await expect.poll(recovery).not.toBeNull();
      await expect(await openDetails(page, b)).toHaveValue('Equal latest text');
    } finally {
      releaseOld();
      releaseBridge();
      await app.stop();
    }
  });
}
