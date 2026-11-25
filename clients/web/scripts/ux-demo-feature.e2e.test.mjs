// HS2-ZF70Y9: a production-built client serves the `/ux-demo` catalog only from a host started with
// `--feature ux-demo`, and loads it as an on-demand chunk that the app itself never requests.
import { execFile } from 'node:child_process';
import { mkdir, mkdtemp, readdir, readFile, rm } from 'node:fs/promises';
import { createServer } from 'node:http';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';

import { getRequestListener } from '@hono/node-server';
import { chromium } from '@playwright/test';
import { Hono } from 'hono';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';

import { parseHostFeatures } from '../src/host-features.ts';
import { createLocalHostApp } from '../src/local-host.ts';

const webRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..'),
  run = promisify(execFile);
// Build without the test runner's Vite mode, as `npm run build` does.
const productionEnvironment = Object.fromEntries(
  Object.entries(process.env).filter(([name]) => !name.startsWith('VITEST') && name !== 'NODE_ENV' && name !== 'TEST'),
);

let buildRoot, demoChunk, browser;
const servers = [];

/** A local host over the production `dist/`, with a synthetic bridge and no machine server. */
async function host(argv) {
  const bridge = new Hono();
  bridge.get('/__hotsheet/checkouts', (context) => context.json([]));
  bridge.get('/__hotsheet/registered-projects', (context) => context.json([]));
  bridge.all('/__hotsheet/*', (context) => context.json({ error: 'Synthetic endpoint unavailable.' }, 404));
  const features = parseHostFeatures(argv, {}),
    app = createLocalHostApp(resolve(buildRoot, 'dist'), bridge, undefined, (_context, next) => next(), features),
    listener = getRequestListener(app.fetch),
    server = createServer((request, response) => void listener(request, response));
  await new Promise((done) => server.listen(0, '127.0.0.1', done));
  servers.push(server);
  return `http://127.0.0.1:${server.address().port}`;
}

/** Load `url` and record every script the page requests and every uncaught page error. */
async function open(url) {
  const page = await browser.newPage({ viewport: { width: 1280, height: 800 } }),
    scripts = [],
    errors = [];
  page.on('request', (request) => {
    const path = new URL(request.url()).pathname;
    if (path.endsWith('.js')) scripts.push(path.split('/').pop());
  });
  page.on('pageerror', (error) => errors.push(error.message));
  const response = await page.goto(url);
  return { page, response, scripts, errors };
}

beforeAll(async () => {
  await mkdir(resolve(webRoot, 'target'), { recursive: true });
  buildRoot = await mkdtemp(resolve(webRoot, 'target/ux-demo-feature-e2e-'));
  await run(
    process.execPath,
    [
      resolve(webRoot, 'node_modules/vite/bin/vite.js'),
      'build',
      '--outDir',
      resolve(buildRoot, 'dist'),
      '--emptyOutDir',
      '--logLevel',
      'error',
    ],
    {
      cwd: webRoot,
      env: productionEnvironment,
    },
  );
  // The catalog's chunk is the one whose source map lists its entry module.
  for (const name of await readdir(resolve(buildRoot, 'dist/assets'))) {
    if (!name.endsWith('.js.map')) continue;
    const { sources } = JSON.parse(await readFile(resolve(buildRoot, 'dist/assets', name), 'utf8'));
    if (sources.some((source) => source.endsWith('src/ux-demo/main.tsx'))) demoChunk = name.slice(0, -'.map'.length);
  }
  browser = await chromium.launch();
}, 180_000);

afterAll(async () => {
  await browser?.close();
  for (const server of servers) await new Promise((done) => server.close(done));
  if (buildRoot) await rm(buildRoot, { recursive: true, force: true });
});

describe('production /ux-demo behind --feature ux-demo (HS2-ZF70Y9)', () => {
  it('builds the catalog as its own chunk', () => {
    expect(demoChunk).toMatch(/\.js$/);
  });

  it('is the host’s ordinary 404 without the flag, and the app never loads the catalog', async () => {
    const origin = await host(['--port', '0']);
    const demo = await open(`${origin}/ux-demo?component=tag-chip`);
    expect(demo.response?.status()).toBe(404);
    expect(await demo.page.locator('body').innerText()).toBe('404 Not Found');
    expect(await demo.page.locator('#ux-demo').count()).toBe(0);
    expect(demo.scripts).toEqual([]);
    await demo.page.close();

    const app = await open(`${origin}/`);
    expect(app.response?.status()).toBe(200);
    await app.page.getByRole('button', { name: 'Open Project' }).first().waitFor();
    expect(app.scripts).not.toContain(demoChunk);
    expect(await app.page.locator('meta[name="hotsheet-features"]').count()).toBe(0);
    await app.page.close();
  }, 60_000);

  it('renders the catalog with the flag, and the app still never loads it', async () => {
    for (const flag of [['--feature', 'ux-demo'], ['--feature:ux-demo']]) {
      const origin = await host(flag);
      const demo = await open(`${origin}/ux-demo?component=attachment-gallery`);
      expect(demo.response?.status()).toBe(200);
      // Kerf's catalog chrome around the selected production component.
      await demo.page.locator('#ux-demo #kui-catalog [data-component="attachment-gallery"]').first().waitFor();
      expect(await demo.page.title()).toBe('Hot Sheet UX components');
      expect(await demo.page.locator('#app').count()).toBe(0);
      expect(demo.scripts).toContain(demoChunk);
      expect(demo.errors).toEqual([]);
      await demo.page.close();

      const app = await open(`${origin}/`);
      await app.page.getByRole('button', { name: 'Open Project' }).first().waitFor();
      expect(app.scripts).not.toContain(demoChunk);
      expect(await app.page.locator('#ux-demo').count()).toBe(0);
      await app.page.close();
    }
  }, 60_000);
});
