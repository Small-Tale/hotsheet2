import { expect, test, webkit } from '@playwright/test';

import type { FullTicket, ToolConnection } from '../src/api';
import type { ConversationExportOpenResult } from '../src/conversation-export';
import { insecureOriginProxy, remoteOrigin } from './insecure-origin';
import { realTicketServer } from './real-ticket-server';

for (const width of [390, 1280]) {
  test(`WebKit LAN identities survive draft, attachment, group and chat transitions at ${width}px (HS2-76ZR5P)`, async ({
    baseURL,
  }, testInfo) => {
    test.setTimeout(120_000);
    const cleanups: Array<() => Promise<unknown>> = [];
    try {
      const server = await realTicketServer();
      cleanups.push(server.stop);
      const proxy = await insecureOriginProxy(baseURL!, server);
      cleanups.push(proxy.close);
      const browser = await webkit.launch();
      cleanups.push(() => browser.close());
      const context = await browser.newContext({
        proxy: { server: proxy.url },
        viewport: { width, height: 844 },
        isMobile: width === 390,
      });
      cleanups.push(() => context.close());
      const page = await context.newPage();
      cleanups.push(() => page.unrouteAll({ behavior: 'ignoreErrors' }));
      let releaseThirdUpload: () => void = () => undefined;
      const thirdUploadGate = new Promise<void>((resolve) => {
        releaseThirdUpload = resolve;
      });
      cleanups.push(async () => {
        releaseThirdUpload();
      });
      const apiPath = `/__hotsheet/project-api/${server.checkoutId}`,
        connections: ToolConnection[] = [],
        uploads: string[] = [],
        turns: string[] = [],
        errors: string[] = [],
        resizeDeferrals: Array<{ message: string; name: string; stack: string }> = [];
      page.on('pageerror', (error) => {
        // WebKit documents this stackless native event as next-frame deferral:
        // https://webkit.org/blog/9997/resizeobserver-in-webkit/
        // Keep real exceptions (including a same-message thrown Error) fatal.
        if (
          error.message === 'ResizeObserver loop completed with undelivered notifications.' &&
          !error.name &&
          !error.stack
        )
          resizeDeferrals.push({ message: error.message, name: error.name, stack: error.stack ?? '' });
        else errors.push(error.message);
      });
      await page.addInitScript((root) => {
        localStorage.setItem('hotsheet.open-projects', JSON.stringify([root]));
      }, server.root);
      await page.route('**/__hotsheet/projects/open', (route) =>
        route.fulfill({
          status: 201,
          json: {
            id: server.checkoutId,
            root: server.root,
            name: 'LAN browser identities',
            stores: [server.store],
            apiPath,
          },
        }),
      );
      // Only external AI tool execution/native folder selection are faked. Ticket
      // creation and attachment upload/readback use the real isolated Rust store.
      await page.route('**/__hotsheet/conversation-exports/open', (route) =>
        route.fulfill({
          json: {
            conversation: {
              displayPath: '/exports/LAN saved chat',
              manifest: {
                format: 'hotsheet-conversation-export',
                manifestVersion: 1,
                exportId: 'saved-export',
                revision: 1,
                exportedAt: '2026-09-23T00:00:00Z',
                selectedMessageIds: ['saved-message'],
                bundle: { includeAttachments: false, includeMedia: false, includeSummary: false },
                entries: [],
                assets: [],
                source: { tool: 'codex', projectId: server.checkoutId, conversationId: 'saved' },
                reopen: {
                  conversationId: 'saved',
                  tool: 'codex',
                  firstMessageId: 'saved-message',
                  lastMessageId: 'saved-message',
                  resumesOriginalSession: false,
                },
              },
              messages: [{ id: 'saved-message', role: 'assistant', content: 'Saved LAN result' }],
              activity: [],
            } satisfies ConversationExportOpenResult,
          },
        }),
      );
      await page.route(`**${apiPath}/**`, async (route) => {
        const request = route.request(),
          url = new URL(request.url()),
          path = url.pathname.slice(apiPath.length);
        if (path === '/ai-tools')
          return route.fulfill({
            json: [
              {
                id: 'codex',
                display_name: 'Codex',
                models: [{ id: 'test', label: 'Test' }],
                default_model: 'test',
                actions: ['send_turn'],
              },
            ],
          });
        if (path === '/ai-settings') return route.fulfill({ json: { tool: 'codex', model: 'test' } });
        if (path === '/connections') return route.fulfill({ json: connections });
        if (path === '/drive/sessions') return route.fulfill({ json: [] });
        if (path === '/drive/connections' && request.method() === 'POST') {
          const body = request.postDataJSON(),
            connection: ToolConnection = {
              id: body.connection_id,
              tool: body.tool,
              project: server.root,
              role: 'main',
              busy: false,
              actions: ['send_turn'],
              model: body.model,
            };
          connections.push(connection);
          return route.fulfill({ status: 201, json: connection });
        }
        if (path.endsWith('/turns')) {
          turns.push(request.postDataJSON().content);
          return route.fulfill({ status: 503, json: { error: 'Test tool is offline' } });
        }
        if (path.endsWith('/attachments') && request.method() === 'POST') {
          uploads.push(decodeURIComponent(request.headers()['x-hotsheet-attachment-batch']));
          // Hold the first attachment on the second ticket. Ticket creation deliberately
          // projects and opens the ticket before its sequential attachment batch settles,
          // so the visible details editor is not the batch-completion boundary.
          if (uploads.length === 3) await thirdUploadGate;
        }
        const response = await route.fetch({
          url: `${server.url}${path}${url.search}`,
          headers: { ...request.headers(), 'X-Hotsheet-Secret': server.secret },
        });
        return route.fulfill({ response });
      });
      await page.goto(remoteOrigin);
      expect(
        await page.evaluate(() => ({
          secure: isSecureContext,
          uuid: typeof crypto.randomUUID,
          values: typeof crypto.getRandomValues,
        })),
      ).toEqual({ secure: false, uuid: 'undefined', values: 'function' });
      const newTicket = page.getByRole('button', { name: 'New ticket…' }),
        composer = page.getByRole('dialog', { name: 'Create ticket' });
      const filenames = ['first.txt', 'second.txt'];
      await newTicket.click();
      await composer.getByRole('textbox', { name: 'Ticket title' }).fill('Cancelled draft');
      await composer
        .getByLabel('Drop or browse attachments for new ticket', { exact: true })
        .setInputFiles({ name: 'discarded.txt', mimeType: 'text/plain', buffer: Buffer.from('discarded') });
      await expect(composer.getByRole('button', { name: 'Remove discarded.txt' })).toBeVisible();
      await composer.getByRole('button', { name: 'Cancel' }).click();
      for (const [index, filename] of filenames.entries()) {
        await newTicket.click();
        await composer.getByRole('textbox', { name: 'Ticket title' }).fill(`LAN attachment ${index + 1}`);
        await expect(composer.getByRole('button', { name: 'Remove discarded.txt' })).toHaveCount(0);
        await composer.getByLabel('Drop or browse attachments for new ticket', { exact: true }).setInputFiles(
          [filename, `extra-${filename}`].map((name) => ({
            name,
            mimeType: 'text/plain',
            buffer: Buffer.from(`Payload ${name}`),
          })),
        );
        await expect(composer.getByRole('button', { name: `Remove ${filename}`, exact: true })).toBeVisible();
        if (index === 1) await composer.screenshot({ path: testInfo.outputPath(`LAN-attachment-draft-${width}.png`) });
        await composer.getByRole('button', { name: 'Create ticket' }).click();
        await expect(composer).toBeHidden();
        await expect(page.getByRole('textbox', { name: 'Ticket details' })).toBeVisible();
        if (index === 1) {
          await expect.poll(() => uploads.length).toBe(3);
          releaseThirdUpload();
        }
      }
      await expect.poll(() => uploads.length).toBe(4);
      expect(uploads[0]).toBe(uploads[1]);
      expect(uploads[2]).toBe(uploads[3]);
      expect(uploads[0]).not.toBe(uploads[2]);
      const tickets = await server.request<Array<{ id: string }>>(`/checkouts/${server.checkoutId}/tickets`);
      expect(tickets).toHaveLength(2);
      for (const row of tickets) {
        const ticket = await server.request<FullTicket>(`/checkouts/${server.checkoutId}/tickets/${row.id}`);
        expect(ticket.attachments).toHaveLength(2);
        expect(new Set(ticket.attachments.map((item) => item.batch_id)).size).toBe(1);
      }
      await page.screenshot({ path: testInfo.outputPath(`LAN-tickets-${width}.png`), fullPage: true });
      await page.getByRole('button', { name: 'Show terminal drawer' }).click();
      const drawer = page.locator('[data-component="terminal-drawer"]'),
        create = drawer.getByRole('button', { name: 'New drawer item' });
      for (let index = 0; index < 2; index++) {
        await create.click();
        await drawer.getByRole('menu', { name: 'New drawer item' }).getByText('AI chat', { exact: true }).click();
        const chat = drawer.locator('[data-component="ai-conversation"]');
        await chat.getByLabel('Message Codex').fill(`LAN turn ${index + 1}`);
        await chat.getByRole('button', { name: 'Send message to Codex' }).click();
        await expect(chat).toContainText('Test tool is offline');
        await expect(chat).toContainText(`LAN turn ${index + 1}`);
      }
      expect(new Set(connections.map((connection) => connection.id)).size).toBe(2);
      expect(turns).toEqual(['LAN turn 1', 'LAN turn 2']);
      for (let index = 0; index < 2; index++) {
        await create.click();
        await drawer.getByRole('menu', { name: 'New drawer item' }).getByText('Saved conversation…').click();
        await expect(drawer.locator('[data-component="ai-conversation"]')).toContainText('Saved LAN result');
      }
      await expect(drawer.getByRole('tab', { name: 'Codex saved chat' })).toHaveCount(2);
      await expect(page.locator('.app-toast')).toBeHidden();
      await drawer
        .locator('[data-component="ai-conversation"]')
        .screenshot({ path: testInfo.outputPath(`LAN-saved-conversation-${width}.png`) });
      await page.getByRole('button', { name: 'Workspace grid' }).click();
      await page.getByRole('button', { name: 'Manage workspace visibility' }).click();
      const visibility = page.locator('[data-terminal-visibility-dialog]'),
        nameDialog = page.locator('[data-terminal-visibility-name-dialog]');
      for (const name of ['Focus one', 'Focus two']) {
        await visibility.getByRole('button', { name: 'Add visibility group' }).click();
        await nameDialog.getByRole('textbox', { name: 'Group name' }).fill(name);
        await nameDialog.getByRole('button', { name: 'Add', exact: true }).click();
        await expect(visibility.getByRole('tab', { name })).toHaveAttribute('aria-selected', 'true');
      }
      const groupIds = await visibility
        .locator('[data-visibility-group-id]')
        .evaluateAll((nodes) =>
          nodes.map((node) => node.getAttribute('data-visibility-group-id')).filter((id) => id !== 'default'),
        );
      expect(new Set(groupIds).size).toBe(2);
      await page.keyboard.press('Escape');
      await expect(visibility).toBeHidden();
      await page.reload();
      expect(await page.evaluate(() => localStorage.getItem('hotsheet.terminals.visibility-groups'))).toContain(
        'Focus two',
      );
      await expect(page.getByRole('button', { name: 'Workspace grid' })).toBeVisible();
      // A continuous layout loop must still fail. At the final stable surface,
      // native resize deferrals must stop instead of recurring on later paints.
      const deferralsBeforePaint = resizeDeferrals.length;
      await page.evaluate(
        () =>
          new Promise<void>((resolve) => {
            requestAnimationFrame(() =>
              requestAnimationFrame(() =>
                requestAnimationFrame(() => {
                  resolve();
                }),
              ),
            );
          }),
      );
      await testInfo.attach('native-resize-deferrals', {
        body: JSON.stringify(resizeDeferrals),
        contentType: 'application/json',
      });
      expect(resizeDeferrals).toHaveLength(deferralsBeforePaint);
      expect(errors).toEqual([]);
    } finally {
      const failures: unknown[] = [];
      for (const close of cleanups.reverse()) {
        try {
          await close();
        } catch (error) {
          failures.push(error);
        }
      }
      expect(failures).toEqual([]);
    }
  });
}
