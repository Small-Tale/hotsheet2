import { chromium, expect, type Locator, type Page, test } from '@playwright/test';

import { insecureOriginProxy, remoteOrigin } from './insecure-origin';
import { realTicketServer } from './real-ticket-server';
import { installTerminalFixture, project } from './terminal-feedback-fixture';

async function rememberProject(page: Page) {
  await page.addInitScript((root) => {
    localStorage.setItem('hotsheet.open-projects', JSON.stringify([root]));
  }, project.root);
}

/** Each attach socket the page opens; a blackholed one stays OPEN but passes nothing either way. */
type Attach = { blackholed: boolean; fromPage: string[] };

/**
 * Open a real `cat` session in the project terminal drawer through its Workspace Grid tile, with
 * every attach socket routed through a switchable blackhole. Runs `body`, then tears it all down.
 */
async function withDrawerTerminal(
  baseURL: string,
  body: (input: { page: Page; dedicated: Locator; attaches: Attach[] }) => Promise<void>,
) {
  const server = await realTicketServer(),
    browser = await chromium.launch(),
    proxy = await insecureOriginProxy(baseURL, server),
    context = await browser.newContext({ proxy: { server: proxy.url }, viewport: { width: 1280, height: 800 } }),
    page = await context.newPage();
  const attaches: Attach[] = [];
  try {
    await server.request('/terminals', 'POST', {
      id: 'live',
      command: '/bin/sh',
      args: ['-c', 'printf "LIVE READY\\r\\n"; exec cat'],
      cwd: server.store,
    });
    await rememberProject(page);
    await installTerminalFixture(page, false, false, { nativeSocket: true, warmLimit: 0 });
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
      const state: Attach = { blackholed: false, fromPage: [] };
      attaches.push(state);
      const upstream = route.connectToServer();
      route.onMessage((message) => {
        if (typeof message === 'string') state.fromPage.push(message);
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

    await page.getByRole('button', { name: 'Workspace Grid', exact: true }).click();
    const dashboard = page.getByRole('region', { name: 'Workspace Grid' }),
      tile = dashboard.locator('[data-terminal-key="terminal-feedback:live"]'),
      magnified = dashboard.getByRole('dialog', { name: /^Magnified live$/i });
    await tile.click();
    await magnified.getByRole('button', { name: /^Open live in project terminal drawer$/i }).click();
    const dedicated = page.locator('[data-component="terminal-session"] [data-terminal-id="live"]');
    await expect(dedicated).toHaveAttribute('data-connection', 'connected');
    await expect(dedicated.locator('.xterm-rows')).toContainText('LIVE READY');
    await body({ page, dedicated, attaches });
  } finally {
    await page.unrouteAll({ behavior: 'ignoreErrors' });
    await context.close();
    await browser.close();
    await proxy.close();
    await server.stop();
  }
}

// HS2-51HQ3S: a drawer terminal whose socket went silent while staying OPEN (a half-open connection
// after sleep or a network change) froze on stale output and swallowed keystrokes, while a freshly
// mounted Workspace Grid tile of the same session worked. The viewport now notices the missing
// liveness replies, attaches a fresh socket on its own, and typing reaches the PTY again.
test('a drawer terminal on a silent half-open socket recovers without a reload (HS2-51HQ3S)', async ({
  baseURL,
}, testInfo) => {
  test.setTimeout(120_000);
  await withDrawerTerminal(baseURL!, async ({ page, dedicated, attaches }) => {
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
  });
});

// HS2-J52ZJ7: keystrokes typed into a half-open socket before the 15 s stale window noticed it were
// lost. Typing after a quiet spell now holds the input behind an immediate probe; with no reply the
// viewport reconnects within about 2 s and replays the held keys on the fresh socket.
test('keys typed into a silent half-open socket are delivered after recovery (HS2-J52ZJ7)', async ({
  baseURL,
}, testInfo) => {
  test.setTimeout(120_000);
  await withDrawerTerminal(baseURL!, async ({ page, dedicated, attaches }) => {
    const before = attaches.length;
    for (const attach of attaches) attach.blackholed = true;
    // Let the socket go quiet past the 3 s input-probe threshold.
    await page.waitForTimeout(3_500);
    await dedicated.locator('.xterm-helper-textarea').focus();
    const typedAt = Date.now();
    await page.keyboard.type('held-keys');
    await expect(dedicated).toHaveAttribute('data-input-held', 'true');
    // The held keys never went into the dead socket; only the probe did.
    const dead = attaches[attaches.length - 1];
    expect(dead.fromPage.some((message) => message.includes('held-keys'))).toBe(false);
    expect(dead.fromPage.some((message) => message.includes('terminal_ping'))).toBe(true);
    await page.screenshot({ path: testInfo.outputPath('hs2-j52zj7-held.png') });

    // The unanswered probe reconnects well before the 15 s stale window would have.
    await expect.poll(() => attaches.length, { timeout: 8_000 }).toBeGreaterThan(before);
    expect(Date.now() - typedAt).toBeLessThan(10_000);
    await expect(dedicated).toHaveAttribute('data-connection', 'connected');
    await expect(dedicated.locator('.xterm-rows')).toContainText('held-keys');
    await expect(dedicated).not.toHaveAttribute('data-input-held', 'true');
    const recovered = attaches.length;

    // Adversarial: a quiet but healthy socket answers the probe, so held keys flush on the reply
    // and nothing reconnects.
    await page.waitForTimeout(3_500);
    await dedicated.locator('.xterm-helper-textarea').focus();
    await page.keyboard.type(' still-here');
    await page.keyboard.press('Enter');
    await expect(dedicated.locator('.xterm-rows')).toContainText('held-keys still-here');
    await page.waitForTimeout(2_500);
    expect(attaches.length).toBe(recovered);
    await expect(dedicated).toHaveAttribute('data-connection', 'connected');
    await page.screenshot({ path: testInfo.outputPath('hs2-j52zj7-recovered.png') });
  });
});
