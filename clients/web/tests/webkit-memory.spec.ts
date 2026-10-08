import { execFile } from 'node:child_process';
import { writeFile } from 'node:fs/promises';
import { promisify } from 'node:util';

import { expect, test, webkit } from '@playwright/test';

import { insecureOriginProxy, remoteOrigin } from './insecure-origin';
import { seedLocalGitTickets } from './real-ticket-fixture';
import { realTicketServer } from './real-ticket-server';

const execute = promisify(execFile);

async function webContentRss() {
  const { stdout } = await execute('ps', ['-axo', 'pid,rss,comm']);
  return new Map(
    stdout
      .split('\n')
      .filter((line) => line.includes('WebKit.WebContent'))
      .map((line) => {
        const [pid, rss] = line.trim().split(/\s+/);
        return [Number(pid), Number(rss)] as const;
      }),
  );
}

test('profile WebKit memory through a real ticket and terminal session (HS2-2G23X9)', async ({ baseURL }, testInfo) => {
  test.skip(
    process.platform !== 'darwin' || process.env.HOTSHEET_WEBKIT_MEMORY_PROFILE !== '1',
    'Run explicitly on macOS to sample local WebKit processes.',
  );
  test.setTimeout(600_000);
  const before = await webContentRss();
  const server = await realTicketServer({
    seed: (fixture) => seedLocalGitTickets(fixture, [{ status: 'started', count: 120 }]).then(() => undefined),
  });
  const browser = await webkit.launch();
  const proxy = await insecureOriginProxy(baseURL!, server);
  const context = await browser.newContext({
    proxy: { server: proxy.url },
    viewport: { width: 1440, height: 900 },
  });
  const page = await context.newPage();
  const errors: string[] = [];
  const measurements: Array<{
    label: string;
    seconds: number;
    processes: Array<{ pid: number; rssKb: number }>;
    dom: { nodes: number; terminalRows: number; ticketRows: number };
  }> = [];
  const startedAt = Date.now();
  const sample = async (label: string) => {
    await page.waitForTimeout(1500);
    const processes = [...(await webContentRss())]
      .filter(([pid]) => !before.has(pid))
      .map(([pid, rssKb]) => ({ pid, rssKb }));
    const dom = await page.evaluate(() => ({
      nodes: document.querySelectorAll('*').length,
      terminalRows: document.querySelectorAll('.xterm-rows > div').length,
      ticketRows: document.querySelectorAll('[data-ticket-slug]').length,
    }));
    expect(processes, 'one isolated WebContent process must own the session').toHaveLength(1);
    expect(dom.terminalRows).toBe(24);
    expect(dom.ticketRows).toBe(120);
    measurements.push({ label, seconds: Math.round((Date.now() - startedAt) / 1000), processes, dom });
  };
  try {
    await server.request('/terminals', 'POST', {
      id: 'memory-shell',
      command: '/bin/sh',
      args: [
        '-c',
        'i=0; while [ "$i" -lt 20 ]; do j=0; while [ "$j" -lt 10 ]; do printf "memory sample %04d %02d abcdefghijklmnopqrstuvwxyz\\r\\n" "$i" "$j"; j=$((j+1)); done; i=$((i+1)); sleep 0.25; done; exec cat',
      ],
      cwd: server.root,
    });
    await page.addInitScript((root) => {
      localStorage.setItem('hotsheet.open-projects', JSON.stringify([root]));
      localStorage.setItem('hotsheet.terminals.drawer-open', 'true');
    }, server.root);
    await page.route('**/__hotsheet/projects/open', (route) =>
      route.fulfill({
        status: 201,
        json: {
          id: 'memory',
          root: server.root,
          name: 'Memory sample',
          stores: [server.store],
          apiPath: '/__hotsheet/project-api/memory',
        },
      }),
    );
    await page.route('**/__hotsheet/project-api/memory/**', async (route) => {
      const url = new URL(route.request().url());
      const path = url.pathname
        .replace('/__hotsheet/project-api/memory', '')
        .replace('/checkouts/memory', `/checkouts/${server.checkoutId}`);
      const response = await route.fetch({
        url: `${server.url}${path}${url.search}`,
        headers: { ...route.request().headers(), 'X-Hotsheet-Secret': server.secret },
      });
      await route.fulfill({ response });
    });
    await page.routeWebSocket('**/terminals/*/attach', (route) => {
      route.connectToServer();
    });
    await page.routeWebSocket(/\/ws\/sync(?:\?.*)?$/, (route) => {
      route.connectToServer();
    });
    page.on('pageerror', (error) => errors.push(error.message));
    await page.goto(`${remoteOrigin}/?dev-review=false`);
    await expect(page.locator('[data-component="ticket-list"]')).toBeVisible();
    await expect(page.locator('[data-ticket-slug]').first()).toBeVisible();
    await page.getByRole('button', { name: 'Workspace grid', exact: true }).click();
    const terminalTile = page.locator('[data-terminal-key="memory:memory-shell"]');
    await expect(terminalTile).toBeVisible();
    await expect(terminalTile.locator('.xterm-rows > div').first()).toBeVisible();
    await expect(terminalTile.locator('.xterm-rows')).toContainText('memory sample');
    await sample('initial');
    const projectTab = page.getByRole('tab', { name: 'Memory sample' });
    const cycles = Number(process.env.HOTSHEET_WEBKIT_MEMORY_CYCLES ?? 6),
      blocks = Number(process.env.HOTSHEET_WEBKIT_MEMORY_BLOCKS ?? 1);
    for (let block = 1; block <= blocks; block += 1) {
      for (let cycle = 1; cycle <= cycles; cycle += 1) {
        const edit = (block - 1) * cycles + cycle;
        await page.getByLabel('Columns view').click();
        await expect(page.locator('[data-component="ticket-board"]')).toBeVisible();
        await page.getByLabel('Notifications view').click();
        await projectTab.click();
        await page.getByLabel('Settings view').click();
        await projectTab.click();
        await page.getByLabel('List view').click();
        await expect(page.locator('[data-component="ticket-list"]')).toBeVisible();
        await page.locator('[data-ticket-slug]').first().click();
        const inspector = page.locator('#app-right-rail');
        await inspector.locator('[data-action="edit-ticket-title"]').dblclick();
        const title = inspector.getByRole('textbox', { name: 'Ticket title' });
        await title.fill(`Memory cycle ${edit}`);
        await title.blur();
        await expect(inspector.locator('[data-action="edit-ticket-title"]')).toContainText(`Memory cycle ${edit}`);
        await page.getByRole('button', { name: 'Workspace grid', exact: true }).click();
        await expect(terminalTile).toBeVisible();
        await sample(`block-${block}-cycle-${cycle}`);
      }
      if (process.env.HOTSHEET_WEBKIT_MEMORY_IDLE === '1') {
        await page.waitForTimeout(30_000);
        await sample(`block-${block}-idle-30s`);
        await page.waitForTimeout(30_000);
        await sample(`block-${block}-idle-60s`);
      }
    }
    await expect
      .poll(async () => {
        const persisted = await server.request<{ items: Array<{ title: string }> }>(
          `/checkouts/${server.checkoutId}/tickets?status=started&page_size=200&counts=false`,
        );
        return persisted.items.some((ticket) => ticket.title === `Memory cycle ${blocks * cycles}`);
      })
      .toBe(true);
    expect(errors).toEqual([]);
    const path = testInfo.outputPath('webkit-memory-profile.json');
    await writeFile(
      path,
      JSON.stringify(
        { webkit: browser.version(), tickets: 120, terminalOutputLines: 200, cycles, blocks, measurements, errors },
        null,
        2,
      ),
    );
    await testInfo.attach('webkit-memory-profile', { path, contentType: 'application/json' });
    console.log(JSON.stringify(measurements));
  } finally {
    await context.close();
    await browser.close();
    await server.stop();
    await proxy.close();
  }
});
