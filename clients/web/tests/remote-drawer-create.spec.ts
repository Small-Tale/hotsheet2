import { mkdir } from 'node:fs/promises';
import { dirname, join } from 'node:path';

import { expect, test, type WebSocketRoute } from '@playwright/test';

import { proxyFetch, realTicketServer } from './real-ticket-server';

// HS2-72HT9Y: the shipped drawer, opened on a read-write remote project. The Rust fixture owns
// the checkouts and ticket reads; only external terminal and AI-tool processes are synthetic.
const BINDING = '01ARZ3NDEKTSV4RRFFQ69G5FAV';

test('creates Terminal, AI shell, and AI Chat on the selected remote checkout (HS2-72HT9Y)', async ({ page }) => {
  test.setTimeout(120_000);
  const server = await realTicketServer();
  try {
    const otherRoot = join(dirname(server.root), 'other');
    await mkdir(otherRoot);
    const other = await server.request<{ id: string }>('/checkouts', 'POST', {
      root: otherRoot,
      stores: [server.store],
      alias: 'Other',
    });
    const checkouts = [
      { id: server.checkoutId, root: server.root, name: 'Build host · Launch' },
      { id: other.id, root: otherRoot, name: 'Build host · Other' },
    ].map((checkout) => {
      const remoteId = `remote:${BINDING}:${checkout.id}`;
      return { ...checkout, remoteId, apiPath: `/__hotsheet/project-api/${encodeURIComponent(remoteId)}` };
    });
    const sessions: Array<{ id: string; cwd: string; alive: boolean; busy: boolean; tool?: string; name?: string }> =
      [];
    const connections: Array<{
      id: string;
      tool: string;
      project: string;
      role: string;
      busy: boolean;
      actions: string[];
    }> = [];
    const launches: Array<{ project: string; body: Record<string, unknown> }> = [];
    const chats: Array<{ project: string; body: Record<string, unknown> }> = [];
    const turns: Array<{ project: string; content: string }> = [];
    const sockets: string[] = [];
    const changeSockets: WebSocketRoute[] = [];
    const events: Record<string, unknown>[] = [];
    await page.addInitScript(
      (roots) => {
        localStorage.setItem('hotsheet.open-projects', JSON.stringify(roots));
      },
      checkouts.map((item) => item.remoteId),
    );
    await page.route(/\/__hotsheet\/(?:remote\/)?projects\/open$/, (route) => {
      const root = route.request().postDataJSON().root;
      const item = checkouts.find((candidate) => candidate.remoteId === root);
      if (!item) return route.fulfill({ status: 404, json: { error: 'Unknown synthetic checkout' } });
      return route.fulfill({
        status: 201,
        json: {
          id: item.remoteId,
          root: item.remoteId,
          name: item.name,
          remoteHost: 'Build host',
          stores: [],
          apiPath: item.apiPath,
        },
      });
    });
    await page.route('**/__hotsheet/remotes', (route) => route.fulfill({ json: [] }));
    await page.routeWebSocket(/\/__hotsheet\/project-api\/remote[^/]+\/terminals\/[^/]+\/attach/, (socket) => {
      sockets.push(socket.url());
      const id = /\/terminals\/([^/]+)\/attach/.exec(socket.url())?.[1];
      socket.send(Buffer.from(`Live remote output from ${id}\r\n`));
      socket.onMessage((message) => {
        if (typeof message !== 'string') socket.send(Buffer.from(`Remote echo: ${message.toString()}\r\n`));
      });
    });
    await page.routeWebSocket(/\/__hotsheet\/project-api\/remote[^/]+\/ws\/sync/, (socket) => {
      changeSockets.push(socket);
      socket.onClose(() => {
        const index = changeSockets.indexOf(socket);
        if (index >= 0) changeSockets.splice(index, 1);
      });
    });
    for (const item of checkouts) {
      await page.context().route(`**${item.apiPath}/**`, async (route) => {
        const request = route.request();
        const incoming = new URL(request.url());
        const path = incoming.pathname
          .slice(item.apiPath.length)
          .replace(`/checkouts/${encodeURIComponent(item.remoteId)}`, `/checkouts/${item.id}`);
        if (path === '/ai-tools')
          return route.fulfill({
            json: [
              {
                id: 'claude',
                display_name: 'Claude',
                models: [{ id: 'test', label: 'Test' }],
                default_model: 'test',
                actions: ['send_turn'],
              },
            ],
          });
        if (path === '/ai-settings') return route.fulfill({ json: { tool: 'claude', model: 'test' } });
        if (path === '/terminals' && request.method() === 'GET')
          return route.fulfill({
            json: sessions
              .filter((session) => session.cwd === item.root)
              .map((session) => ({ ...session, cwd: item.remoteId, launch_cwd: item.remoteId })),
          });
        if (path === '/terminals' && request.method() === 'POST') {
          const body = request.postDataJSON() as Record<string, unknown>;
          // Mirror the bridge's composite-cwd translation before the read-write host receives it.
          if (body.cwd === item.remoteId) body.cwd = item.root;
          launches.push({ project: item.remoteId, body });
          const created = {
            id: String(body.id),
            cwd: String(body.cwd),
            alive: true,
            busy: false,
            ...(typeof body.connect === 'string' ? { tool: body.connect, name: 'Claude' } : {}),
          };
          sessions.push(created);
          return route.fulfill({ status: 201, json: { ...created, cwd: item.remoteId, launch_cwd: item.remoteId } });
        }
        if (path === '/connections')
          return route.fulfill({ json: connections.filter((connection) => connection.project === item.root) });
        if (path === '/drive/sessions') return route.fulfill({ json: [] });
        if (path === '/ws/poll') {
          const since = Number(incoming.searchParams.get('since') ?? events.length);
          return route.fulfill({
            json: {
              cursor: events.length,
              events: events.filter((event) => Number(event.cursor) > since),
              overflow: false,
            },
          });
        }
        if (path === '/drive/connections' && request.method() === 'POST') {
          const body = request.postDataJSON() as Record<string, unknown>;
          // HS2-TCJQHX: the host receives its native checkout, never a composite browser id.
          if (body.checkout === item.remoteId) body.checkout = item.id;
          chats.push({ project: item.remoteId, body });
          const created = {
            id: String(body.connection_id),
            tool: String(body.tool),
            project: item.root,
            role: 'main',
            busy: false,
            actions: ['send_turn'],
          };
          connections.push(created);
          return route.fulfill({ status: 201, json: created });
        }
        if (/\/drive\/connections\/[^/]+\/turns$/.test(path) && request.method() === 'POST') {
          const body = request.postDataJSON() as { content: string; turn_id?: string };
          turns.push({ project: item.remoteId, content: body.content });
          const connection = connections.find((entry) => path.includes(entry.id));
          await route.fulfill({ status: 202, json: connection });
          for (const event of [
            { type: 'output', content: 'Synthetic answer from remote host.', truncated: false },
            { type: 'done', reason: 'completed' },
          ]) {
            const update = {
              cursor: events.length + 1,
              store: item.root,
              kind: 'turn_event',
              id: connection!.id,
              slug: connection!.tool,
              turn: { connection_id: connection!.id, turn_id: body.turn_id, event },
            };
            events.push(update);
            for (const socket of changeSockets.filter((candidate) => candidate.url().includes(item.apiPath)))
              socket.send(JSON.stringify(update));
          }
          return;
        }
        try {
          const response = await proxyFetch(route, {
            url: `${server.url}${path}${incoming.search}`,
            headers: { ...request.headers(), 'X-Hotsheet-Secret': server.secret },
            timeout: 60_000,
          });
          await route.fulfill({ response });
        } catch {
          await route.abort().catch(() => undefined);
        }
      });
    }
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto('/');
    const projectTabs = page.locator('[data-tab-kind="project"][data-location="remote"]');
    await expect(projectTabs).toHaveCount(2);
    await projectTabs.first().click();
    await page.getByRole('button', { name: 'Show Terminal Drawer' }).click();
    await expect(page.locator('.app-shell')).toHaveAttribute('data-terminal-drawer-transitioning', 'false');
    const drawer = page.locator('[data-component="terminal-drawer"]');
    const create = async (name: string) => {
      await drawer.getByRole('button', { name: 'New Drawer Item' }).click();
      await drawer.getByRole('menuitem', { name, exact: true }).click();
    };
    await create('Terminal');
    await expect(drawer.locator('[data-tab-kind="terminal"]')).toHaveCount(1);
    await expect(drawer.locator('[data-component="terminal-viewport"]')).toContainText('Live remote output');
    await create('Claude shell');
    await expect(drawer.locator('[data-tab-kind="terminal"]')).toHaveCount(2);
    await expect(drawer.locator('[data-component="terminal-viewport"]')).toContainText('Live remote output');
    await expect(drawer.locator('[data-tab-kind="terminal"]').last().getByRole('tab')).toHaveAttribute(
      'aria-selected',
      'true',
    );
    await expect(drawer.locator('[data-tab-kind="terminal"]').last().getByRole('tab')).toHaveAccessibleName('Claude');
    await page.setViewportSize({ width: 2048, height: 1128 });
    await page.screenshot({
      path: test.info().outputPath('hs2-72ht9y-after-claude-shell-2048.png'),
      animations: 'disabled',
    });
    await page.setViewportSize({ width: 1440, height: 900 });
    await create('AI Chat');
    const chat = drawer.locator('[data-component="ai-conversation"]');
    await expect(chat.getByRole('textbox', { name: 'Message Claude' })).toBeVisible();
    await chat.getByRole('textbox', { name: 'Message Claude' }).fill('Synthetic remote chat turn');
    await chat.getByRole('textbox', { name: 'Message Claude' }).press('Enter');
    await expect.poll(() => turns.length).toBe(1);
    await expect(chat).toContainText('Synthetic answer from remote host.');
    expect(launches.map((entry) => entry.body.cwd)).toEqual([server.root, server.root]);
    expect(launches.map((entry) => entry.project)).toEqual([checkouts[0].remoteId, checkouts[0].remoteId]);
    expect(chats).toMatchObject([
      { project: checkouts[0].remoteId, body: { checkout: server.checkoutId, tool: 'claude' } },
    ]);
    expect(turns).toEqual([{ project: checkouts[0].remoteId, content: 'Synthetic remote chat turn' }]);
    expect(sockets.length).toBeGreaterThanOrEqual(2);
    await page.setViewportSize({ width: 2048, height: 1128 });
    await page.screenshot({
      path: test.info().outputPath('hs2-72ht9y-after-2048-source-size.png'),
      animations: 'disabled',
    });
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.screenshot({ path: test.info().outputPath('hs2-72ht9y-after-1440.png'), animations: 'disabled' });
    await projectTabs.last().click();
    await expect(drawer.locator('[data-tab-kind="terminal"]')).toHaveCount(0);
    await expect(drawer.locator('[data-tab-kind="ai-chat"]')).toHaveCount(0);
    await projectTabs.first().click();
    await expect(drawer.locator('[data-tab-kind="terminal"]')).toHaveCount(2);
    await expect(drawer.locator('[data-tab-kind="ai-chat"]')).toHaveCount(1);
    for (const width of [1100, 390, 1440]) {
      await page.setViewportSize({ width, height: width === 390 ? 844 : 900 });
      await expect(drawer.locator('[data-tab-kind="ai-chat"]')).toHaveCount(1);
      await page.screenshot({ path: test.info().outputPath(`hs2-72ht9y-after-${width}.png`), animations: 'disabled' });
    }
    await page.emulateMedia({ colorScheme: 'dark' });
    await page.screenshot({ path: test.info().outputPath('hs2-72ht9y-after-1440-dark.png'), animations: 'disabled' });
  } finally {
    await server.stop(page).catch(() => undefined);
  }
});
