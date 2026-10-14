import { expect, type Page, test } from '@playwright/test';

import { ticketsJson } from './ticket-page-fixture';

// HS2-AMNNBR / HS2-TBQQ00: switching from a project's terminal grid into the workspace grid, and
// toggling the workspace grid's full screen, reuses the mounted terminals instead of recreating them.
const TERMINALS = ['one', 'two', 'three', 'four', 'five', 'six'].map((name) => `demo-${name}`);

async function openGridWorkspace(page: Page) {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.addInitScript(() => {
    localStorage.setItem('hotsheet.open-projects', JSON.stringify(['/work/demo']));
    localStorage.setItem('hotsheet.workspace.active-project-root.v1', '/work/demo');
    localStorage.setItem('hotsheet.terminals.drawer-open', 'true');
    localStorage.setItem('hotsheet.project.demo-checkout.terminal-drawer-selection', 'grid');
    class FakeSocket extends EventTarget {
      static CONNECTING = 0;
      static OPEN = 1;
      static CLOSING = 2;
      static CLOSED = 3;
      readyState = 0;
      binaryType = 'blob';
      sent: unknown[] = [];
      constructor(public url: string) {
        super();
        const owner = window as unknown as { __gridSockets?: FakeSocket[] };
        owner.__gridSockets ??= [];
        owner.__gridSockets.push(this);
        setTimeout(() => {
          this.readyState = 1;
          this.dispatchEvent(new Event('open'));
          const id = decodeURIComponent(url.split('/terminals/')[1].split('/')[0]);
          this.dispatchEvent(
            new MessageEvent('message', { data: new TextEncoder().encode(`output of ${id}\r\n% `).buffer }),
          );
        });
      }
      send(value: unknown) {
        this.sent.push(value);
      }
      close() {
        this.readyState = 3;
        this.dispatchEvent(new CloseEvent('close'));
      }
    }
    Object.assign(window, { WebSocket: FakeSocket });
  });
  await page.route('**/*', async (route) => {
    const request = route.request(),
      url = new URL(request.url()),
      path = url.pathname;
    if (path === '/__hotsheet/projects/open')
      return route.fulfill({
        status: 201,
        json: {
          id: 'demo-checkout',
          root: '/work/demo',
          name: 'demo',
          stores: ['/work/demo.hs2'],
          apiPath: '/__hotsheet/project-api/demo-checkout',
        },
      });
    if (path.endsWith('/providers'))
      return route.fulfill({
        json: [
          {
            connection_id: 'git-local',
            provider: 'git',
            display_name: 'Hot Sheet Git',
            locator: '/tickets',
            default: true,
            capabilities: { create: true, update: true, query_fields: [] },
          },
        ],
      });
    if (path.endsWith('/tickets')) return route.fulfill({ json: ticketsJson(request, []) });
    if (path.endsWith('/terminals'))
      return route.fulfill({ json: TERMINALS.map((id) => ({ id, alive: true, busy: false, cwd: '/work/demo' })) });
    if (
      path.endsWith('/permissions') ||
      path.endsWith('/connections') ||
      path.endsWith('/commands') ||
      path.endsWith('/command-runs') ||
      path.endsWith('/corrupt-tickets')
    )
      return route.fulfill({ json: [] });
    if (path.endsWith('/repository/status'))
      return route.fulfill({
        json: { branch: 'main', ahead: 0, behind: 0, staged: 0, unstaged: 0, untracked: 0, conflicted: 0, clean: true },
      });
    if (path.endsWith('/ws/poll'))
      return route.fulfill({
        json: { cursor: Number(url.searchParams.get('since') ?? 0), events: [], overflow: false },
      });
    return route.continue();
  });
  await page.goto('/');
}

/** Tags each mounted xterm so a later step can tell a reused emulator from a recreated one. */
function tagTerminals(page: Page) {
  return page.evaluate(() => {
    const tags: Record<string, string> = {};
    for (const xterm of document.querySelectorAll<HTMLElement>('[data-component="terminal-viewport"] .xterm')) {
      const viewport = xterm.closest<HTMLElement>('[data-component="terminal-viewport"]');
      if (!viewport?.dataset.terminalId) continue;
      xterm.dataset.reuseTag ??= `${viewport.dataset.terminalId}:${Math.random()}`;
      tags[viewport.dataset.terminalId] = xterm.dataset.reuseTag;
    }
    return tags;
  });
}
const socketCount = (page: Page) =>
  page.evaluate(
    () =>
      (window as unknown as { __gridSockets?: Array<{ url: string }> }).__gridSockets?.filter((socket) =>
        socket.url.includes('/terminals/'),
      ).length ?? 0,
  );

async function settledGrid(page: Page, region: string) {
  const viewports = page
    .getByRole('region', { name: region })
    .locator('[data-component="terminal-viewport"][data-display-mode="scaled-preview"]');
  await expect(viewports).toHaveCount(6);
  await expect(viewports.and(page.locator('[data-geometry-ready="true"]'))).toHaveCount(6);
  for (const id of TERMINALS)
    await expect(viewports.and(page.locator(`[data-terminal-id="${id}"]`))).toContainText(`output of ${id}`);
}

test('project grid -> workspace grid -> full screen -> exit -> project grid reuses every terminal', async ({
  page,
}) => {
  await openGridWorkspace(page);
  const drawer = page.locator('[data-component="terminal-drawer"]');
  await expect(drawer).toHaveAttribute('data-mode', 'grid');
  // Maximize the drawer so all six tiles are in view and mount.
  await page.getByRole('tab', { name: 'Project grid' }).dblclick();
  await settledGrid(page, 'demo Terminal Drawer');
  const tags = await tagTerminals(page),
    sockets = await socketCount(page);
  expect(Object.keys(tags)).toHaveLength(6);
  expect(sockets).toBeGreaterThanOrEqual(6);
  const timings: Record<string, number> = {};
  const step = async (name: string, action: () => Promise<void>, region: string) => {
    const started = Date.now();
    await action();
    await settledGrid(page, region);
    timings[name] = Date.now() - started;
    expect(await tagTerminals(page), name).toEqual(tags);
    expect(await socketCount(page), name).toBe(sockets);
  };
  await step(
    'workspace grid',
    () => page.getByRole('button', { name: 'Workspace Grid' }).first().click(),
    'Workspace Grid',
  );
  await step(
    'full screen',
    () => page.getByRole('button', { name: 'Enter workspace grid full screen' }).click(),
    'Workspace Grid',
  );
  await expect(page.locator('[data-component="app-shell"]')).toHaveAttribute('data-grid-fullscreen', 'true');
  await page.screenshot({ path: 'target/visual-captures/hs2-tbqq00-fullscreen-reused.png' });
  await step(
    'exit full screen',
    () => page.getByRole('button', { name: 'Exit workspace grid full screen' }).click(),
    'Workspace Grid',
  );
  await page.screenshot({ path: 'target/visual-captures/hs2-amnnbr-workspace-grid-reused.png' });
  await step('project grid', () => page.getByRole('tab', { name: /demo/ }).click(), 'demo Terminal Drawer');
  test.info().annotations.push({ type: 'grid-switch-ms', description: JSON.stringify(timings) });
  console.log('grid switch timings (ms)', JSON.stringify(timings));
});
