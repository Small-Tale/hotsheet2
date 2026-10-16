import { chromium, expect, type Page, test } from '@playwright/test';

import { insecureOriginProxy, remoteOrigin } from './insecure-origin';
import { realTicketServer } from './real-ticket-server';
import { installTerminalFixture, project } from './terminal-feedback-fixture';

async function rememberProject(page: Page) {
  await page.addInitScript((root) => {
    localStorage.setItem('hotsheet.open-projects', JSON.stringify([root]));
  }, project.root);
}

// HS2-51HQ3S: a drawer terminal whose socket went silent while staying OPEN (a half-open connection
// after sleep or a network change) froze on stale output and swallowed keystrokes, while a freshly
// mounted Workspace Grid tile of the same session worked. The viewport now notices the missing
// liveness replies, attaches a fresh socket on its own, and typing reaches the PTY again.
test('a drawer terminal on a silent half-open socket recovers without a reload (HS2-51HQ3S)', async ({
  baseURL,
}, testInfo) => {
  test.setTimeout(120_000);
  const server = await realTicketServer(),
    browser = await chromium.launch(),
    proxy = await insecureOriginProxy(baseURL!, server),
    context = await browser.newContext({ proxy: { server: proxy.url }, viewport: { width: 1280, height: 800 } }),
    page = await context.newPage();
  // Each attach socket the page opens; a blackholed one stays OPEN but passes nothing either way.
  const attaches: Array<{ blackholed: boolean }> = [];
  try {
    await server.request('/terminals', 'POST', {
      id: 'live',
      command: '/bin/sh',
      args: ['-c', 'printf "LIVE READY\\r\\n"; exec cat'],
      cwd: server.store,
    });
    await rememberProject(page);
    await installTerminalFixture(page, false, false, { nativeSocket: true });
    await page.route('**/__hotsheet/projects/open', (route) =>
      route.fulfill({ status: 201, json: { ...project, root: server.store } }),
    );
    await page.route('**/__hotsheet/project-api/terminal-feedback/terminals**', async (route) => {
      const url = new URL(route.request().url()),
        path = url.pathname.replace(project.apiPath, '');
      const response = await route.fetch({
        url: `${server.url}${path}${url.search}`,
        headers: { ...route.request().headers(), 'X-Hotsheet-Secret': server.secret },
      });
      await route.fulfill({ response });
    });
    await page.routeWebSocket('**/terminals/*/attach', (route) => {
      const state = { blackholed: false };
      attaches.push(state);
      const upstream = route.connectToServer();
      route.onMessage((message) => {
        if (!state.blackholed) upstream.send(message);
      });
      upstream.onMessage((message) => {
        if (!state.blackholed) route.send(message);
      });
    });
    await page.routeWebSocket(/\/ws\/sync(?:\?.*)?$/, (route) => {
      route.connectToServer();
    });
    await page.goto(remoteOrigin);

    // Open the session in the project drawer through its Workspace Grid tile.
    await page.getByRole('button', { name: 'Workspace Grid', exact: true }).click();
    const dashboard = page.getByRole('region', { name: 'Workspace Grid' }),
      tile = dashboard.locator('[data-terminal-key="terminal-feedback:live"]'),
      magnified = dashboard.getByRole('dialog', { name: /^Magnified live$/i });
    await tile.click();
    await magnified.getByRole('button', { name: /^Open live in project terminal drawer$/i }).click();
    const dedicated = page.locator('[data-component="terminal-session"] [data-terminal-id="live"]');
    await expect(dedicated).toHaveAttribute('data-connection', 'connected');
    await expect(dedicated.locator('.xterm-rows')).toContainText('LIVE READY');
    const before = attaches.length;

    // The connection goes half-open: nothing flows, but the browser socket never closes.
    for (const attach of attaches) attach.blackholed = true;
    await dedicated.locator('.xterm-helper-textarea').focus();
    await page.keyboard.type('lost');
    await page.screenshot({ path: testInfo.outputPath('hs2-51hq3s-half-open.png') });

    // Within the 15 s liveness window plus one heartbeat, the viewport attaches a fresh socket.
    await expect.poll(() => attaches.length, { timeout: 40_000 }).toBeGreaterThan(before);
    await expect(dedicated).toHaveAttribute('data-connection', 'connected');
    await dedicated.locator('.xterm-helper-textarea').focus();
    await page.keyboard.type('after recovery');
    await page.keyboard.press('Enter');
    await expect(dedicated.locator('.xterm-rows')).toContainText('after recovery');
    await page.screenshot({ path: testInfo.outputPath('hs2-51hq3s-recovered.png') });
    await page.screenshot({ path: 'target/visual-captures/hs2-51hq3s-recovered-1280.png' });
  } finally {
    await page.unrouteAll({ behavior: 'ignoreErrors' });
    await context.close();
    await browser.close();
    await proxy.close();
    await server.stop();
  }
});
