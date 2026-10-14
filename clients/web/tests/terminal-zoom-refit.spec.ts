import { chromium, expect, type Page, test } from '@playwright/test';

import { insecureOriginProxy, remoteOrigin } from './insecure-origin';
import { realTicketServer } from './real-ticket-server';
import { installTerminalFixture, project } from './terminal-feedback-fixture';

async function rememberProject(page: Page) {
  await page.addInitScript((root) => {
    localStorage.setItem('hotsheet.open-projects', JSON.stringify([root]));
  }, project.root);
}

// HS2-6179FV: a zoomed Workspace Grid terminal must take over the real PTY's size from a smaller
// viewer that was driving it, so the session fills the zoom space; zooming again must do it again.
test('a zoomed Workspace Grid terminal refits the real PTY across zoom, unzoom, zoom (HS2-6179FV)', async ({
  baseURL,
}, testInfo) => {
  test.setTimeout(90_000);
  const server = await realTicketServer(),
    browser = await chromium.launch(),
    proxy = await insecureOriginProxy(baseURL!, server),
    context = await browser.newContext({
      proxy: { server: proxy.url },
      viewport: { width: 1440, height: 900 },
      deviceScaleFactor: 2,
      colorScheme: 'dark',
    }),
    page = await context.newPage();
  try {
    await server.request('/terminals', 'POST', {
      id: 'nano',
      command: '/bin/sh',
      args: ['-c', 'printf "ZOOM REFIT\\r\\n"; while true; do stty size; sleep 0.3; done'],
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
      const upstream = route.connectToServer();
      route.onMessage((message) => {
        upstream.send(message);
      });
      upstream.onMessage((message) => {
        route.send(message);
      });
    });
    await page.routeWebSocket(/\/ws\/sync(?:\?.*)?$/, (route) => {
      route.connectToServer();
    });
    await page.goto(remoteOrigin);
    // Another device last interacted with the terminal at a small size and keeps heartbeating.
    const driverUrl = `${server.url.replace(/^http/, 'ws')}/terminals/nano/attach?secret=${encodeURIComponent(server.secret)}`;
    const otherDevice = async () => {
      const { WebSocket: NodeWebSocket } = await import('ws');
      const socket = new NodeWebSocket(driverUrl);
      await new Promise((resolve, reject) => {
        socket.once('open', resolve);
        socket.once('error', reject);
      });
      const claim = (interacting: boolean) => {
        socket.send(
          JSON.stringify({
            resize: { viewer_id: 'other-device', cols: 60, rows: 16, focus: true, visible: true, interacting },
          }),
        );
      };
      claim(true);
      const heartbeat = setInterval(() => {
        claim(false);
      }, 1_000);
      return () => {
        clearInterval(heartbeat);
        socket.close();
      };
    };
    const stopOther = await otherDevice();
    try {
      await page.getByRole('button', { name: 'Workspace Grid', exact: true }).click();
      const dashboard = page.getByRole('region', { name: 'Workspace Grid' }),
        tile = dashboard.locator('[data-terminal-key="terminal-feedback:nano"]'),
        magnified = dashboard.getByRole('dialog', { name: /^Magnified nano$/i }),
        viewport = magnified.locator('[data-display-mode="interactive"]');
      await expect(tile.locator('[data-display-mode="scaled-preview"]')).toHaveAttribute('data-pty-size', '60x16');
      const zoom = async (capture: string) => {
        await tile.click();
        await expect(viewport).toHaveAttribute('data-geometry-ready', 'true');
        // The zoomed terminal drives the real PTY at its own fit (after the arbiter's focus hold),
        // so the session fills the panel instead of the other device's 60x16.
        await expect
          .poll(async () => {
            const [grid, pty] = await Promise.all([
              viewport.getAttribute('data-grid-size'),
              viewport.getAttribute('data-pty-size'),
            ]);
            return grid === pty && grid !== '60x16' && grid !== '80x24' ? grid : undefined;
          })
          .toBeTruthy();
        const grid = (await viewport.getAttribute('data-grid-size'))!;
        await expect(viewport).toHaveAttribute('data-driving', 'true');
        const [cols, rows] = grid.split('x');
        await expect(viewport.locator('.xterm-rows')).toContainText(`${rows} ${cols}`);
        await page.screenshot({ path: testInfo.outputPath(`hs2-6179fv-${capture}.png`) });
        await page.screenshot({ path: `target/visual-captures/hs2-6179fv-real-${capture}.png` });
        return grid;
      };
      const unzoom = async () => {
        await magnified.click({ position: { x: 5, y: 5 } });
        await expect(magnified).toHaveCount(0);
      };
      // This browser also drove the PTY from the project drawer, which stays parked (connected)
      // while the Workspace Grid is shown.
      await zoom('pre-drawer');
      await magnified.getByRole('button', { name: /^Open nano in project terminal drawer$/i }).click();
      const dedicated = page.locator('[data-component="terminal-session"] [data-terminal-id="nano"]');
      await expect(dedicated).toHaveAttribute('data-geometry-ready', 'true');
      await dedicated.locator('.xterm-helper-textarea').focus();
      await page.keyboard.type('x');
      await expect.poll(() => dedicated.getAttribute('data-driving')).toBe('true');
      await page.getByRole('button', { name: 'Workspace Grid', exact: true }).click();
      const wide = await zoom('zoomed-wide');
      await unzoom();
      expect(await zoom('rezoomed-wide')).toBe(wide);
      await unzoom();
      await page.setViewportSize({ width: 1100, height: 640 });
      expect(await zoom('zoomed-narrow')).not.toBe(wide);
      await unzoom();
    } finally {
      stopOther();
    }
  } finally {
    await page.unrouteAll({ behavior: 'ignoreErrors' });
    await context.close();
    await browser.close();
    await proxy.close();
    await server.stop();
  }
});
