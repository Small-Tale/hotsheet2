import { execFile, spawn } from 'node:child_process';
import { mkdtemp, rm } from 'node:fs/promises';
import { createServer } from 'node:net';
import { tmpdir } from 'node:os';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';

import { chromium } from '@playwright/test';
import { beforeAll, expect, it } from 'vitest';

// HS2-587N4D: `npm run prod` serves the built client through the same local bridge without Vite.
const webRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..'),
  repoRoot = resolve(webRoot, '../..'),
  run = promisify(execFile);

async function availablePort() {
  const server = createServer();
  await new Promise((resolveListen, reject) => server.once('error', reject).listen(0, '127.0.0.1', resolveListen));
  const address = server.address();
  await new Promise((resolveClose) => server.close(resolveClose));
  if (!address || typeof address === 'string') throw new Error('Could not allocate a test port.');
  return address.port;
}

async function waitForHost(url, child, output) {
  const deadline = Date.now() + 30_000;
  while (Date.now() < deadline) {
    try {
      if ((await fetch(url)).ok) return;
    } catch {
      /* not listening yet */
    }
    if (child.exitCode !== null) throw new Error(`Local host exited early.\n${output()}`);
    await new Promise((resolveWait) => setTimeout(resolveWait, 100));
  }
  throw new Error(`Timed out waiting for ${url}.\n${output()}`);
}

// Build as `npm run prod` does: without the test runner's environment, which would change Vite's mode.
const productionEnvironment = Object.fromEntries(
  Object.entries(process.env).filter(([name]) => !name.startsWith('VITEST') && name !== 'NODE_ENV' && name !== 'TEST'),
);

beforeAll(async () => {
  await run('npm', ['run', 'build'], { cwd: webRoot, env: productionEnvironment });
  await run('npm', ['run', 'build:host'], { cwd: webRoot, env: productionEnvironment });
}, 180_000);

it('serves the production client and the local bridge without Vite', async () => {
  const home = await mkdtemp(resolve(tmpdir(), 'hotsheet-local-host-home-')),
    port = await availablePort(),
    child = spawn(process.execPath, ['dist-host/local-host.js', '--port', String(port)], {
      cwd: webRoot,
      env: { ...productionEnvironment, HOTSHEET_HOME: home },
      stdio: ['ignore', 'pipe', 'pipe'],
    });
  let log = '';
  child.stdout.on('data', (chunk) => (log += chunk));
  child.stderr.on('data', (chunk) => (log += chunk));
  const origin = `http://127.0.0.1:${port}`,
    browser = await chromium.launch();
  try {
    await waitForHost(`${origin}/`, child, () => log);
    expect(log).toContain(`Hot Sheet production client on ${origin}/`);
    // HS2-D2JQ9A: the host says which server build it launches (release when one is built).
    expect(log).toMatch(/Using (release|debug) Hot Sheet binaries/);
    const page = await browser.newPage(),
      requests = [],
      pageErrors = [];
    page.on('request', (request) => requests.push(new URL(request.url()).pathname));
    page.on('pageerror', (error) => pageErrors.push(error.message));
    await page.goto(`${origin}/`);
    await page.getByRole('button', { name: 'Open project' }).first().waitFor();
    await page.waitForLoadState('networkidle');
    expect(pageErrors).toEqual([]);
    expect(
      requests.filter((path) => path.startsWith('/@') || path.includes('/node_modules/') || path.startsWith('/src/')),
    ).toEqual([]);
    expect(requests.filter((path) => path.startsWith('/assets/')).length).toBeLessThanOrEqual(6);
    // Development-only surfaces are absent; client routes resolve to the document.
    expect((await fetch(`${origin}/ux-demo`)).status).toBe(404);
    expect((await fetch(`${origin}/__hotsheet/dev-review/tickets`, { method: 'POST' })).status).toBe(404);
    expect(await (await fetch(`${origin}/settings/anything`)).text()).toContain('<main id="app">');
    // The bridge is live: it supervises a real machine server under the isolated home.
    const checkouts = await fetch(`${origin}/__hotsheet/checkouts`);
    expect(checkouts.status).toBe(200);
    expect(Array.isArray(await checkouts.json())).toBe(true);
  } finally {
    await browser.close();
    child.kill('SIGTERM');
    await new Promise((resolveExit) => (child.exitCode === null ? child.once('exit', resolveExit) : resolveExit()));
    await run(resolve(repoRoot, 'target/debug/hotsheet-cli'), [
      'serve',
      '-C',
      resolve(home, 'server-bootstrap.hs2'),
      '--stop',
    ]).catch(() => undefined);
    await rm(home, { recursive: true, force: true });
  }
}, 120_000);
