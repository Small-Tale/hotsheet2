// HS2-RKZKX7: real production frontend + bridge, fresh browser origins and isolated synthetic state.
import assert from 'node:assert/strict';
import { mkdtemp, rm } from 'node:fs/promises';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

import { getRequestListener } from '@hono/node-server';
import { chromium, webkit } from '@playwright/test';
import { Hono } from 'hono';

const { createLocalHostApp } = await import(pathToFileURL(resolve('dist-host/local-host.js')).href);
const storage = await mkdtemp(resolve(tmpdir(), 'hs2-desktop-persistence-'));
const servers = [];
const opened = [];
async function host() {
  const bridge = new Hono();
  bridge.get('/__hotsheet/checkouts', (context) => context.json([]));
  bridge.post('/__hotsheet/projects/open', async (context) => {
    opened.push((await context.req.json()).root);
    return context.json({ error: 'Synthetic project is unavailable for this persistence test.' }, 409);
  });
  bridge.all('/__hotsheet/*', (context) => context.json({ error: 'Synthetic endpoint unavailable.' }, 404));
  const listener = getRequestListener(createLocalHostApp(resolve('dist'), bridge, storage).fetch);
  const server = createServer((request, response) => void listener(request, response));
  await new Promise((done) => server.listen(0, '127.0.0.1', done));
  servers.push(server);
  return `http://127.0.0.1:${server.address().port}`;
}
try {
  const first = await host(),
    second = await host();
  assert.notEqual(first, second);
  for (const engine of [chromium, webkit]) {
    const browser = await engine.launch();
    try {
      const context = await browser.newContext();
      await context.addInitScript(() => {
        window.__HOTSHEET_DESKTOP_STORAGE__ = true;
      });
      const page = await context.newPage();
      page.on('pageerror', (error) => console.error(error.message));
      await page.goto(first);
      await page.locator('#app').waitFor();
      await page.waitForFunction(() => document.querySelector('#app')?.children.length > 0);
      const committed = page.waitForResponse(
        (response) =>
          response.url().endsWith('/__hotsheet/desktop-storage') &&
          response.request().method() === 'PUT' &&
          response.request().postDataJSON().key === 'hotsheet.show-loading-activity' &&
          response.status() === 204,
      );
      await page.evaluate(() => {
        localStorage.setItem('hotsheet.open-projects', '["/synthetic/remembered-project"]');
        localStorage.setItem('hotsheet.show-loading-activity', 'true');
      });
      await committed;
      await page.close();
      const restored = await context.newPage();
      await restored.goto(second);
      await restored.waitForFunction(() => document.querySelector('#app')?.children.length > 0);
      await restored.waitForFunction(() => localStorage.getItem('hotsheet.show-loading-activity') === 'true');
      await restored.getByText('remembered-project could not be reopened.', { exact: true }).waitFor();
      assert.ok(opened.includes('/synthetic/remembered-project'));
      assert.equal(
        await restored.evaluate(() => localStorage.getItem('hotsheet.open-projects')),
        '["/synthetic/remembered-project"]',
      );
      await restored.reload();
      await restored.getByText('remembered-project could not be reopened.', { exact: true }).waitFor();
      await restored.evaluate(() => {
        window.addEventListener('pagehide', () => localStorage.setItem('hotsheet.show-loading-activity', 'false'));
      });
      await restored.close();
      const afterClose = await context.newPage();
      await afterClose.goto(first);
      await afterClose.waitForFunction(() => localStorage.getItem('hotsheet.show-loading-activity') === 'false');
      await context.close();
      console.log(`${engine.name()}: production bootstrap restored preferences/projects across ports and reload`);
    } finally {
      await browser.close();
    }
  }
} finally {
  await Promise.all(
    servers.map(
      (server) =>
        new Promise((done) => {
          server.closeAllConnections();
          server.close(done);
        }),
    ),
  );
  await rm(storage, { recursive: true, force: true });
}
