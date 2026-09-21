import { spawn } from 'node:child_process';
import { mkdir, mkdtemp, readdir, rm, symlink, writeFile } from 'node:fs/promises';
import { createServer } from 'node:net';
import { tmpdir } from 'node:os';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { chromium } from '@playwright/test';
import { expect, it } from 'vitest';

const webRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');

async function availablePort() {
  const server = createServer();
  await new Promise((resolveListen, reject) => server.once('error', reject).listen(0, '127.0.0.1', resolveListen));
  const address = server.address();
  await new Promise((resolveClose) => server.close(resolveClose));
  if (!address || typeof address === 'string') throw new Error('Could not allocate a test port.');
  return address.port;
}

async function waitForSource(url) {
  let lastError;
  for (let attempt = 0; attempt < 100; attempt += 1) {
    try {
      const response = await fetch(url);
      if (response.ok) return response.text();
    } catch (error) {
      lastError = error;
    }
    await new Promise((resolveWait) => setTimeout(resolveWait, 50));
  }
  throw lastError ?? new Error(`Timed out waiting for ${url}`);
}

it('serves the startup snapshot until the stable dev process restarts', async () => {
  const fixture = await mkdtemp(resolve(tmpdir(), 'hotsheet-stable-e2e-'));
  const runtimeTemp = await mkdtemp(resolve(tmpdir(), 'hotsheet-stable-runtime-'));
  const port = await availablePort();
  await mkdir(resolve(fixture, 'src'));
  await writeFile(resolve(fixture, 'package.json'), '{"type":"module"}');
  await writeFile(resolve(fixture, 'index.html'), '<script type="module" src="/src/main.js"></script>');
  await writeFile(resolve(fixture, 'src/main.js'), 'window.snapshot = "before";');
  await symlink(resolve(webRoot, 'node_modules'), resolve(fixture, 'node_modules'), 'dir');

  const child = spawn(
    process.execPath,
    [resolve(webRoot, 'scripts/stable-dev.mjs'), '--port', String(port), '--strictPort'],
    {
      env: {
        ...process.env,
        HOTSHEET_WEB_STABLE_SOURCE_ROOT: fixture,
        HOTSHEET_WEB_STABLE_TEMP_ROOT: runtimeTemp,
      },
      stdio: 'ignore',
    },
  );
  try {
    const url = `http://127.0.0.1:${port}/src/main.js`;
    expect(await waitForSource(url)).toContain('before');
    await writeFile(resolve(fixture, 'src/main.js'), 'window.snapshot = "after";');
    expect(await waitForSource(`${url}?after-edit`)).toContain('before');
  } finally {
    child.kill('SIGTERM');
    await new Promise((resolveExit) => child.once('exit', resolveExit));
    const remainingSnapshots = await readdir(runtimeTemp);
    await rm(fixture, { recursive: true, force: true });
    await rm(runtimeTemp, { recursive: true, force: true });
    expect(remainingSnapshots).toEqual([]);
  }
}, 15_000);

it('does not reload when a later route first imports another dependency', async () => {
  const runtimeTemp = await mkdtemp(resolve(tmpdir(), 'hotsheet-stable-runtime-'));
  const port = await availablePort();
  let output = '';
  const child = spawn(
    process.execPath,
    [resolve(webRoot, 'scripts/stable-dev.mjs'), '--port', String(port), '--strictPort'],
    {
      env: {
        ...process.env,
        HOTSHEET_WEB_STABLE_SOURCE_ROOT: webRoot,
        HOTSHEET_WEB_STABLE_TEMP_ROOT: runtimeTemp,
      },
      stdio: ['ignore', 'pipe', 'pipe'],
    },
  );
  child.stdout.on('data', (chunk) => {
    output += chunk;
  });
  child.stderr.on('data', (chunk) => {
    output += chunk;
  });
  const browser = await chromium.launch();
  try {
    await waitForSource(`http://127.0.0.1:${port}/`);
    const page = await browser.newPage();
    await page.addInitScript(() => {
      const key = 'hotsheet-stable-document-loads';
      sessionStorage.setItem(key, String(Number(sessionStorage.getItem(key) ?? 0) + 1));
    });
    await page.goto(`http://127.0.0.1:${port}/`);
    await page.waitForTimeout(250);
    expect(await page.evaluate(() => sessionStorage.getItem('hotsheet-stable-document-loads'))).toBe('1');
    await page.goto(`http://127.0.0.1:${port}/ux-demo`);
    await page.waitForTimeout(1_000);
    expect(await page.evaluate(() => sessionStorage.getItem('hotsheet-stable-document-loads'))).toBe('2');
    expect(output).not.toContain('new dependencies optimized');
    expect(output).not.toContain('optimized dependencies changed. reloading');
  } finally {
    await browser.close();
    child.kill('SIGTERM');
    await new Promise((resolveExit) => child.once('exit', resolveExit));
    expect(await readdir(runtimeTemp)).toEqual([]);
    await rm(runtimeTemp, { recursive: true, force: true });
  }
}, 30_000);

it('serves the app without a Vite reconnect client, HMR websocket, or reconnect logging', async () => {
  const runtimeTemp = await mkdtemp(resolve(tmpdir(), 'hotsheet-stable-runtime-'));
  const port = await availablePort();
  const child = spawn(
    process.execPath,
    [resolve(webRoot, 'scripts/stable-dev.mjs'), '--port', String(port), '--strictPort'],
    {
      env: {
        ...process.env,
        HOTSHEET_WEB_STABLE_SOURCE_ROOT: webRoot,
        HOTSHEET_WEB_STABLE_TEMP_ROOT: runtimeTemp,
      },
      stdio: 'ignore',
    },
  );
  const browser = await chromium.launch();
  try {
    const origin = `http://127.0.0.1:${port}`;
    const rootHtml = await waitForSource(`${origin}/`);
    const mainSource = await waitForSource(`${origin}/src/main.tsx`);
    const demoHtml = await waitForSource(`${origin}/ux-demo`);
    const stableClientShim = await waitForSource(`${origin}/@vite/client`);
    expect(rootHtml).not.toContain('/@vite/client');
    expect(mainSource).not.toContain('/@vite/client');
    expect(demoHtml).not.toContain('/@vite/client');
    expect(stableClientShim).toContain('/src/stable-vite-client.ts');

    const page = await browser.newPage();
    const requested = [];
    const sockets = [];
    const consoleMessages = [];
    page.on('request', (request) => requested.push(request.url()));
    page.on('websocket', (socket) => sockets.push(socket.url()));
    page.on('console', (message) => consoleMessages.push(message.text()));
    await page.goto(`${origin}/`);
    await page.waitForTimeout(500);
    expect(await page.locator('#app').count()).toBe(1);
    expect(requested.filter((url) => url.includes('/@vite/client'))).toEqual([`${origin}/@vite/client`]);
    expect(sockets.filter((url) => url.includes('vite-hmr'))).toEqual([]);
    expect(
      consoleMessages.filter(
        (message) => message.includes('[vite] connecting') || message.includes('[vite] connected'),
      ),
    ).toEqual([]);
    await page.screenshot({ path: '/private/tmp/hs2-8jv12r-stable-client-after.png', fullPage: true });
  } finally {
    await browser.close();
    child.kill('SIGTERM');
    await new Promise((resolveExit) => child.once('exit', resolveExit));
    expect(await readdir(runtimeTemp)).toEqual([]);
    await rm(runtimeTemp, { recursive: true, force: true });
  }
}, 30_000);

it('does not reload when the terminal runtime first lazy-loads its xterm dependencies', async () => {
  // HS2-8JV12R ("client randomly restarts") investigation: a common cause of a dev-server full reload is
  // Vite discovering a new dependency at runtime and re-optimizing when a route first lazy-loads it. The
  // terminal viewport dynamically imports terminal-viewport-runtime, which pulls in @xterm/*. This pins
  // that stable-dev does NOT reload when that lazy import first happens, ruling the class out as the cause.
  const runtimeTemp = await mkdtemp(resolve(tmpdir(), 'hotsheet-stable-runtime-'));
  const port = await availablePort();
  let output = '';
  const child = spawn(
    process.execPath,
    [resolve(webRoot, 'scripts/stable-dev.mjs'), '--port', String(port), '--strictPort'],
    {
      env: {
        ...process.env,
        HOTSHEET_WEB_STABLE_SOURCE_ROOT: webRoot,
        HOTSHEET_WEB_STABLE_TEMP_ROOT: runtimeTemp,
      },
      stdio: ['ignore', 'pipe', 'pipe'],
    },
  );
  child.stdout.on('data', (chunk) => {
    output += chunk;
  });
  child.stderr.on('data', (chunk) => {
    output += chunk;
  });
  const browser = await chromium.launch();
  try {
    await waitForSource(`http://127.0.0.1:${port}/`);
    const page = await browser.newPage();
    await page.addInitScript(() => {
      const key = 'hotsheet-stable-document-loads';
      sessionStorage.setItem(key, String(Number(sessionStorage.getItem(key) ?? 0) + 1));
    });
    await page.goto(`http://127.0.0.1:${port}/`);
    await page.waitForTimeout(250);
    expect(await page.evaluate(() => sessionStorage.getItem('hotsheet-stable-document-loads'))).toBe('1');
    // Trigger the same dynamic import the terminal viewport uses, pulling in the xterm dependencies.
    // Passed as a string so Vitest's SSR transform can't rewrite the browser-side import() call.
    const runtimeExports = await page.evaluate(
      "import('/src/terminal-viewport-runtime.ts').then(module => Object.keys(module).sort())",
    );
    expect(runtimeExports).toContain('mountTerminalViewportRuntime');
    await page.waitForTimeout(1_000);
    // A re-optimize would have full-reloaded the document, resetting/incrementing this counter.
    expect(await page.evaluate(() => sessionStorage.getItem('hotsheet-stable-document-loads'))).toBe('1');
    expect(output).not.toContain('new dependencies optimized');
    expect(output).not.toContain('optimized dependencies changed. reloading');
  } finally {
    await browser.close();
    child.kill('SIGTERM');
    await new Promise((resolveExit) => child.once('exit', resolveExit));
    expect(await readdir(runtimeTemp)).toEqual([]);
    await rm(runtimeTemp, { recursive: true, force: true });
  }
}, 30_000);
