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

function captureChildOutput(child) {
  let output = '';
  child.stdout.on('data', (chunk) => {
    output += chunk;
  });
  child.stderr.on('data', (chunk) => {
    output += chunk;
  });
  return () => output;
}

async function stopChild(child, { forceAfterMs = 10_000 } = {}) {
  if (child.exitCode !== null || child.signalCode !== null) return;
  const exited = new Promise((resolveExit) => child.once('exit', resolveExit));
  child.kill('SIGTERM');
  // One hung server must not stall the suite or survive it (HS2-4SSWV5).
  const force = setTimeout(() => child.kill('SIGKILL'), forceAfterMs);
  try {
    await exited;
  } finally {
    clearTimeout(force);
  }
}

function processAlive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    return error?.code === 'EPERM';
  }
}

async function waitForSource(url, { child, output = () => '', timeoutMs = 30_000 } = {}) {
  const deadline = Date.now() + timeoutMs;
  let lastError;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(url);
      if (response.ok) return response.text();
    } catch (error) {
      lastError = error;
    }
    if (child && (child.exitCode !== null || child.signalCode !== null)) {
      throw new Error(
        `Stable dev exited before serving ${url} (code ${String(child.exitCode)}, signal ${String(child.signalCode)}).\n${output()}`,
        { cause: lastError },
      );
    }
    await new Promise((resolveWait) => setTimeout(resolveWait, 100));
  }
  throw new Error(`Timed out after ${timeoutMs}ms waiting for ${url}.\nStable dev output:\n${output()}`, {
    cause: lastError,
  });
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
      stdio: ['ignore', 'pipe', 'pipe'],
    },
  );
  const output = captureChildOutput(child);
  try {
    const url = `http://127.0.0.1:${port}/src/main.js`;
    expect(await waitForSource(url, { child, output })).toContain('before');
    await writeFile(resolve(fixture, 'src/main.js'), 'window.snapshot = "after";');
    expect(await waitForSource(`${url}?after-edit`, { child, output })).toContain('before');
  } finally {
    await stopChild(child);
    const remainingSnapshots = await readdir(runtimeTemp);
    await rm(fixture, { recursive: true, force: true });
    await rm(runtimeTemp, { recursive: true, force: true });
    expect(remainingSnapshots).toEqual([]);
  }
}, 45_000);

it('exits with its Vite child when the process that started it dies (HS2-4SSWV5)', async () => {
  const fixture = await mkdtemp(resolve(tmpdir(), 'hotsheet-stable-e2e-'));
  const runtimeTemp = await mkdtemp(resolve(tmpdir(), 'hotsheet-stable-runtime-'));
  const port = await availablePort();
  await mkdir(resolve(fixture, 'src'));
  await writeFile(resolve(fixture, 'package.json'), '{"type":"module"}');
  await writeFile(resolve(fixture, 'index.html'), '<script type="module" src="/src/main.js"></script>');
  await writeFile(resolve(fixture, 'src/main.js'), 'window.snapshot = "orphan";');
  await symlink(resolve(webRoot, 'node_modules'), resolve(fixture, 'node_modules'), 'dir');
  // A stand-in test runner: it starts stable-dev, reports its pid, and is then killed without
  // any chance to clean up, exactly like an interrupted runner.
  const runner = spawn(
    process.execPath,
    [
      '-e',
      `const { spawn } = require('node:child_process');
       const child = spawn(process.execPath, process.argv.slice(1), { stdio: 'ignore' });
       console.log(child.pid);
       setInterval(() => {}, 1000);`,
      resolve(webRoot, 'scripts/stable-dev.mjs'),
      '--port',
      String(port),
      '--strictPort',
    ],
    {
      env: {
        ...process.env,
        HOTSHEET_WEB_STABLE_SOURCE_ROOT: fixture,
        HOTSHEET_WEB_STABLE_TEMP_ROOT: runtimeTemp,
      },
      stdio: ['ignore', 'pipe', 'pipe'],
    },
  );
  let stablePid;
  try {
    stablePid = await new Promise((resolvePid, reject) => {
      runner.stdout.once('data', (chunk) => resolvePid(Number(String(chunk).trim())));
      runner.once('exit', () => reject(new Error('runner exited before starting stable-dev')));
    });
    expect(await waitForSource(`http://127.0.0.1:${port}/src/main.js`)).toContain('orphan');
    runner.kill('SIGKILL');
    const deadline = Date.now() + 20_000;
    while (processAlive(stablePid) && Date.now() < deadline) await new Promise((wait) => setTimeout(wait, 100));
    expect(processAlive(stablePid)).toBe(false);
    // Vite went with it: the port no longer serves.
    await expect(fetch(`http://127.0.0.1:${port}/src/main.js`)).rejects.toThrow();
    expect(await readdir(runtimeTemp)).toEqual([]);
  } finally {
    if (runner.exitCode === null && runner.signalCode === null) runner.kill('SIGKILL');
    if (stablePid && processAlive(stablePid)) process.kill(stablePid, 'SIGKILL');
    await rm(fixture, { recursive: true, force: true });
    await rm(runtimeTemp, { recursive: true, force: true });
  }
}, 60_000);

it('does not reload when a later route first imports another dependency', async () => {
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
      stdio: ['ignore', 'pipe', 'pipe'],
    },
  );
  const output = captureChildOutput(child);
  const browser = await chromium.launch();
  try {
    await waitForSource(`http://127.0.0.1:${port}/`, { child, output });
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
    expect(output()).not.toContain('new dependencies optimized');
    expect(output()).not.toContain('optimized dependencies changed. reloading');
  } finally {
    await browser.close();
    await stopChild(child);
    expect(await readdir(runtimeTemp)).toEqual([]);
    await rm(runtimeTemp, { recursive: true, force: true });
  }
}, 60_000);

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
      stdio: ['ignore', 'pipe', 'pipe'],
    },
  );
  const output = captureChildOutput(child);
  const browser = await chromium.launch();
  try {
    const origin = `http://127.0.0.1:${port}`;
    const rootHtml = await waitForSource(`${origin}/`, { child, output });
    const mainSource = await waitForSource(`${origin}/src/main.tsx`, { child, output });
    const demoHtml = await waitForSource(`${origin}/ux-demo`, { child, output });
    const stableClientShim = await waitForSource(`${origin}/@vite/client`, { child, output });
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
    await stopChild(child);
    expect(await readdir(runtimeTemp)).toEqual([]);
    await rm(runtimeTemp, { recursive: true, force: true });
  }
}, 60_000);

it('does not reload when the terminal runtime first lazy-loads its xterm dependencies', async () => {
  // HS2-8JV12R ("client randomly restarts") investigation: a common cause of a dev-server full reload is
  // Vite discovering a new dependency at runtime and re-optimizing when a route first lazy-loads it. The
  // terminal viewport dynamically imports terminal-viewport-runtime, which pulls in @xterm/*. This pins
  // that stable-dev does NOT reload when that lazy import first happens, ruling the class out as the cause.
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
      stdio: ['ignore', 'pipe', 'pipe'],
    },
  );
  const output = captureChildOutput(child);
  const browser = await chromium.launch();
  try {
    await waitForSource(`http://127.0.0.1:${port}/`, { child, output });
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
    expect(output()).not.toContain('new dependencies optimized');
    expect(output()).not.toContain('optimized dependencies changed. reloading');
  } finally {
    await browser.close();
    await stopChild(child);
    expect(await readdir(runtimeTemp)).toEqual([]);
    await rm(runtimeTemp, { recursive: true, force: true });
  }
}, 60_000);
