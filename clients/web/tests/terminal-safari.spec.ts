import { writeFile } from 'node:fs/promises';

import { expect, type Locator, type Page, test, webkit } from '@playwright/test';

import { insecureOriginProxy, remoteOrigin } from './insecure-origin';
import { realTicketServer } from './real-ticket-server';
import { installTerminalFixture, project } from './terminal-feedback-fixture';

async function rememberProject(page: Page) {
  await page.addInitScript((root) => {
    localStorage.setItem('hotsheet.open-projects', JSON.stringify([root]));
  }, project.root);
}

async function expectGlyphPixels(page: Page, viewport: Locator, text: string) {
  const row = viewport.locator('.xterm-rows > div').filter({ hasText: text }).first();
  await expect(row).toContainText(text);
  if (await viewport.getAttribute('data-grid-policy'))
    await expect(viewport).toHaveAttribute('data-geometry-ready', 'true');
  await expect(row).toBeVisible();
  await page.evaluate(() => document.fonts.ready.then(() => undefined));
  await expectPaintedText(page, row);
}

async function expectPaintedText(page: Page, row: Locator) {
  const pixels = await row.screenshot({
    animations: 'disabled',
    scale: 'device',
  });
  const counts = await page.evaluate(async (data) => {
    const image = new Image();
    image.src = `data:image/png;base64,${data}`;
    await image.decode();
    const canvas = document.createElement('canvas');
    canvas.width = image.width;
    canvas.height = image.height;
    const context = canvas.getContext('2d')!;
    context.drawImage(image, 0, 0);
    const rgba = context.getImageData(0, 0, image.width, image.height).data;
    let light = 0,
      dark = 0;
    for (let index = 0; index < rgba.length; index += 4) {
      if (rgba[index] > 140 && rgba[index + 1] > 140 && rgba[index + 2] > 140) light++;
      if (rgba[index] < 30 && rgba[index + 1] < 30 && rgba[index + 2] < 30) dark++;
    }
    return { light, dark };
  }, pixels.toString('base64'));
  // Both actual ink and its dark background must paint. DOM text/renderer labels
  // alone previously passed while the real remote device remained entirely black.
  expect(counts.light).toBeGreaterThan(10);
  expect(counts.dark).toBeGreaterThan(20);
}

for (const width of [390, 1280]) {
  test(`WebKit LAN terminal paints replay and live glyphs across lifecycle at ${width}px (HS2-3ZBQDG)`, async ({
    baseURL,
  }, testInfo) => {
    test.setTimeout(90_000);
    const server = await realTicketServer(),
      browser = await webkit.launch(),
      proxy = await insecureOriginProxy(baseURL!, server),
      context = await browser.newContext({
        proxy: { server: proxy.url },
        viewport: { width, height: 844 },
        isMobile: width === 390,
        deviceScaleFactor: 2,
        colorScheme: 'dark',
      }),
      page = await context.newPage(),
      errors: string[] = [],
      connections: Array<{ viewers: Set<string>; close(): Promise<void> }> = [];
    let binaryBytes = 0;
    try {
      await server.request('/terminals', 'POST', {
        id: 'nano',
        command: '/bin/sh',
        args: ['-c', 'printf "\\033[2J\\033[HREPLAY LAN GLYPHS\\r\\n"; exec cat'],
        cwd: server.store,
      });
      await rememberProject(page);
      await installTerminalFixture(page, false, false, { nativeSocket: true });
      await page.route('**/__hotsheet/projects/open', (route) =>
        route.fulfill({ status: 201, json: { ...project, root: server.store } }),
      );
      // The actual Rust PTY and WebSocket protocol own terminal data. Only project
      // discovery and unrelated ticket-provider surfaces use the shared fixture.
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
        const upstream = route.connectToServer(),
          connection = {
            viewers: new Set<string>(),
            close: async () => {
              await upstream.close({ code: 1012, reason: 'exercise reconnect' });
              await route.close({ code: 1012, reason: 'exercise reconnect' });
            },
          };
        connections.push(connection);
        route.onMessage((message) => {
          if (typeof message === 'string' && message.startsWith('{')) {
            const control = JSON.parse(message) as { resize?: { viewer_id: string } };
            if (control.resize) connection.viewers.add(control.resize.viewer_id);
          }
          upstream.send(message);
        });
        upstream.onMessage((message) => {
          if (typeof message !== 'string') binaryBytes += message.length;
          route.send(message);
        });
      });
      await page.routeWebSocket('**/ws/sync', (route) => {
        route.connectToServer();
      });
      page.on('pageerror', (error) => errors.push(error.message));
      await page.goto(remoteOrigin);
      expect(
        await page.evaluate(() => ({
          secure: isSecureContext,
          uuid: typeof crypto.randomUUID,
          bytes: typeof crypto.getRandomValues,
        })),
      ).toEqual({ secure: false, uuid: 'undefined', bytes: 'function' });
      await page.getByRole('button', { name: 'Workspace grid', exact: true }).click();
      const dashboard = page.getByRole('region', { name: 'Workspace grid' }),
        tile = dashboard.locator('[data-terminal-key="terminal-feedback:nano"]'),
        preview = tile.locator('[data-display-mode="scaled-preview"]');
      const zoom = dashboard.getByRole('button', { name: /Zoom in/ });
      while (await zoom.isEnabled()) await zoom.click();
      await expectGlyphPixels(page, preview, 'REPLAY LAN GLYPHS');
      await tile.screenshot({ path: testInfo.outputPath(`safari-lan-preview-${width}.png`) });
      await tile.click();
      const magnified = dashboard.getByRole('dialog', { name: /^Magnified nano$/i }),
        expanded = magnified.locator('[data-display-mode="interactive"]');
      await expectGlyphPixels(page, expanded, 'REPLAY LAN GLYPHS');
      await magnified.screenshot({ path: testInfo.outputPath(`safari-lan-magnified-${width}.png`) });
      await magnified.getByRole('button', { name: /^Open nano in project terminal drawer$/i }).click();
      const dedicated = page.locator('[data-component="terminal-session"] [data-terminal-id="nano"]');
      await expectGlyphPixels(page, dedicated, 'REPLAY LAN GLYPHS');
      await dedicated.locator('.xterm-helper-textarea').focus();
      await page.keyboard.type('LIVE LAN GLYPHS\n');
      await expectGlyphPixels(page, dedicated, 'LIVE LAN GLYPHS');
      for (const nextWidth of [width === 390 ? 1280 : 390, width]) {
        await page.setViewportSize({ width: nextWidth, height: 844 });
        await expectGlyphPixels(page, dedicated, 'LIVE LAN GLYPHS');
      }
      const beforeReconnect = connections.length,
        current = connections.at(-1)!;
      expect(current.viewers.size).toBe(1);
      await current.close();
      await expect.poll(() => connections.length).toBeGreaterThan(beforeReconnect);
      await expectGlyphPixels(page, dedicated, 'REPLAY LAN GLYPHS');
      await expectGlyphPixels(page, dedicated, 'LIVE LAN GLYPHS');
      await expect.poll(() => [...connections.at(-1)!.viewers]).toEqual([...current.viewers]);
      await dedicated.screenshot({ path: testInfo.outputPath(`safari-lan-drawer-${width}.png`) });
      await page.screenshot({ path: testInfo.outputPath(`safari-lan-context-${width}.png`) });
      const diagnostics = await dedicated.evaluate((element) => {
        const rows = element.querySelector<HTMLElement>('.xterm-rows')!,
          screen = element.querySelector<HTMLElement>('.xterm-screen')!,
          terminal = element.querySelector<HTMLElement>('.terminal')!,
          style = getComputedStyle(rows);
        return {
          renderer: (element as HTMLElement).dataset.renderer,
          canvases: element.querySelectorAll('canvas').length,
          color: style.color,
          opacity: style.opacity,
          visibility: style.visibility,
          fontFamily: style.fontFamily,
          fontSize: style.fontSize,
          lineHeight: style.lineHeight,
          background: getComputedStyle(terminal).backgroundColor,
          fontStatus: document.fonts.status,
          transform: getComputedStyle(terminal).transform,
          screen: { width: screen.getBoundingClientRect().width, height: screen.getBoundingClientRect().height },
          viewport: { width: element.clientWidth, height: element.clientHeight },
        };
      });
      const diagnosticPath = testInfo.outputPath(`safari-terminal-diagnostics-${width}.json`);
      await writeFile(
        diagnosticPath,
        JSON.stringify(
          { diagnostics, binaryBytes, viewers: connections.map((connection) => [...connection.viewers]), errors },
          null,
          2,
        ),
      );
      await testInfo.attach('terminal-render-diagnostics', { path: diagnosticPath, contentType: 'application/json' });
      expect(binaryBytes).toBeGreaterThan(0);
      expect(new Set(connections.flatMap((connection) => [...connection.viewers])).size).toBeGreaterThanOrEqual(3);
      expect(errors).toEqual([]);
    } finally {
      await page.unrouteAll({ behavior: 'ignoreErrors' });
      await context.close();
      await browser.close();
      await proxy.close();
      await server.stop();
    }
  });
}

test('WebKit shows terminal initialization failures instead of an empty black surface (HS2-3ZBQDG)', async ({
  baseURL,
}, testInfo) => {
  test.setTimeout(90_000);
  const browser = await webkit.launch(),
    proxy = await insecureOriginProxy(baseURL!),
    context = await browser.newContext({
      proxy: { server: proxy.url },
      viewport: { width: 390, height: 844 },
      isMobile: true,
      colorScheme: 'dark',
    }),
    page = await context.newPage();
  try {
    await rememberProject(page);
    await installTerminalFixture(page);
    await page.addInitScript(() => {
      Object.defineProperty(crypto, 'getRandomValues', {
        value: () => {
          throw new Error('deliberate initialization failure');
        },
      });
    });
    await page.goto(remoteOrigin);
    await page.getByRole('button', { name: 'Workspace grid', exact: true }).click();
    const zoom = page.getByRole('region', { name: 'Workspace grid' }).getByRole('button', { name: /Zoom in/ });
    while (await zoom.isEnabled()) await zoom.click();
    const tile = page.locator('[data-terminal-key="terminal-feedback:nano"]');
    await expect(tile.getByRole('alert')).toHaveText('Terminal could not start. Reload the page to try again.');
    await expect(tile.getByRole('alert')).toBeVisible();
    await expectPaintedText(page, tile.getByRole('alert'));
    await tile.screenshot({ path: testInfo.outputPath('safari-terminal-initialization-error.png') });
  } finally {
    await page.unrouteAll({ behavior: 'ignoreErrors' });
    await context.close();
    await browser.close();
    await proxy.close();
  }
});
