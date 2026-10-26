import { spawn } from 'node:child_process';
import { mkdir, mkdtemp, readFile, readdir, rm, symlink, writeFile } from 'node:fs/promises';
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

// HS2-KFKE8W: the real stable host serves a snapshot copied under a cache directory. A migration
// job's lock binary used to resolve relative to that snapshot, so every HS1 import died with
// `spawn …/target/debug/hotsheet-migrate ENOENT` before the migrator ever ran.
// Keep the long real-process fixture readable while changing only its scheduling (HS2-XSP0GJ).
// prettier-ignore
it.concurrent('starts an HS1 import job from the cache snapshot with the checkout migrator', async () => {
  const runtimeTemp = await mkdtemp(resolve(tmpdir(), 'hotsheet-stable-runtime-'));
  const home = await mkdtemp(resolve(tmpdir(), 'hotsheet-stable-home-'));
  const project = resolve(home, 'legacy-project');
  await mkdir(resolve(project, '.hotsheet/db'), { recursive: true });
  await writeFile(resolve(project, '.hotsheet/db/PG_VERSION'), '17\n');
  const port = await availablePort();
  const environment = { ...process.env, HOTSHEET_WEB_STABLE_TEMP_ROOT: runtimeTemp, HOTSHEET_HOME: home, HOTSHEET_TEST_SERVER_OWNER_PID: 'host' };
  // The bridge must find the checkout's migrator on its own, as a developer's `npm run dev` does,
  // and a pinned build revision belongs to the production host, not this launcher.
  delete environment.HOTSHEET_MIGRATE_BIN;
  delete environment.HOT_SHEET_BUILD_REVISION;
  const child = spawn(
    process.execPath,
    [resolve(webRoot, 'scripts/stable-dev.mjs'), '--port', String(port), '--strictPort'],
    { env: environment, stdio: ['ignore', 'pipe', 'pipe'] },
  );
  const output = captureChildOutput(child);
  let browser;
  try {
    await waitForSource(`http://127.0.0.1:${port}/`, { child, output, timeoutMs: 90_000 });
    // A migration job belongs to an open project session, so open the legacy project first (the
    // bridge supervises a machine server under the isolated home).
    const opened = await fetch(`http://127.0.0.1:${port}/__hotsheet/projects/open`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ root: project }),
    });
    const session = await opened.json();
    expect(opened.status, JSON.stringify(session)).toBe(201);
    expect(session.needsHs1Migration).toBe(true);
    // The dialog the reviewer captured (HS2-P2W5D7) no longer shows a spawn error after
    // "Import project": the job starts and the page reports the import's own progress or outcome.
    // Capture the dialog at the reviewer's width, then the running state on a phone; the API start
    // below then rejoins the running job.
    browser = await chromium.launch();
    await mkdir(resolve(webRoot, 'target/visual-captures'), { recursive: true });
    const wide = await browser.newPage({ viewport: { width: 1106, height: 1199 } });
    await wide.goto(`http://127.0.0.1:${port}/?project=${encodeURIComponent(project)}`);
    const importButton = wide.getByRole('button', { name: 'Import project' });
    await importButton.waitFor({ state: 'visible', timeout: 30_000 });
    await wide.screenshot({ path: resolve(webRoot, 'target/visual-captures/hs2-kfke8w-import-dialog-wide.png') });
    await importButton.click();
    await wide.waitForTimeout(1_500);
    expect(await wide.locator('body').innerText()).not.toMatch(/ENOENT|Library\/Caches/);
    await wide.screenshot({ path: resolve(webRoot, 'target/visual-captures/hs2-kfke8w-import-started-wide.png') });
    await wide.close();
    const narrow = await browser.newPage({ viewport: { width: 390, height: 844 } });
    await narrow.goto(`http://127.0.0.1:${port}/?project=${encodeURIComponent(project)}`);
    await narrow.getByRole('combobox', { name: 'Ticket view' }).waitFor({ state: 'visible', timeout: 30_000 });
    await narrow.waitForTimeout(500);
    expect(await narrow.locator('body').innerText()).not.toMatch(/ENOENT|Library\/Caches/);
    await narrow.screenshot({ path: resolve(webRoot, 'target/visual-captures/hs2-kfke8w-import-started-narrow.png') });
    await narrow.close();

    const started = await fetch(`http://127.0.0.1:${port}/__hotsheet/projects/migration-jobs`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ projectId: session.id, root: project, location: `${project}.hs2`, kind: 'import' }),
    });
    const body = await started.json();
    // Before the fix this was a 409 whose error named a migrator beside the snapshot's parent.
    expect(JSON.stringify(body)).not.toMatch(/ENOENT|Library\/Caches/);
    expect(started.status, JSON.stringify(body)).toBe(202);
    expect(body.kind).toBe('import');
  } finally {
    await browser?.close();
    await stopChild(child);
    // The bridge's machine server outlives the host by design; stop the one under this home.
    const cli = resolve(webRoot, '../../target/debug/hotsheet-cli');
    await new Promise((resolveStop) =>
      spawn(cli, ['serve', '-C', resolve(home, 'server-bootstrap.hs2'), '--stop'], {
        env: { ...process.env, HOTSHEET_HOME: home },
        stdio: 'ignore',
      }).once('exit', resolveStop),
    );
    await new Promise((resolveWait) => setTimeout(resolveWait, 500));
    await rm(home, { recursive: true, force: true });
    await rm(runtimeTemp, { recursive: true, force: true });
  }
}, 120_000);

// Keep the process fixture readable while changing only its scheduling (HS2-XSP0GJ).
// prettier-ignore
it.concurrent('exits with its Vite child when the process that started it dies (HS2-4SSWV5)', async () => {
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
    const requests = [],
      pageErrors = [];
    page.on('request', (request) => requests.push(new URL(request.url()).pathname));
    page.on('pageerror', (error) => pageErrors.push(error.message));
    await page.goto(`http://127.0.0.1:${port}/`);
    await page.waitForLoadState('networkidle');
    expect(await page.evaluate(() => sessionStorage.getItem('hotsheet-stable-document-loads'))).toBe('1');
    // Dependencies are pre-bundled at startup (HS2-N9RD7X): no raw package JavaScript is fetched
    // module-by-module (only package stylesheets, which Vite serves directly).
    const rawPackageScripts = requests.filter(
      (path) =>
        path.includes('/node_modules/') && !path.includes('/.vite') && !path.includes('/deps/') && !/\.css$/.test(path),
    );
    console.log(`stable-dev first load: ${requests.length} requests, ${rawPackageScripts.length} raw package scripts`);
    expect(rawPackageScripts).toEqual([]);
    await page.goto(`http://127.0.0.1:${port}/ux-demo`);
    await page.waitForTimeout(1_000);
    expect(await page.evaluate(() => sessionStorage.getItem('hotsheet-stable-document-loads'))).toBe('2');
    // Pre-bundled packages still run as one module graph: the catalog renders and registers its
    // Web Awesome elements with no page errors.
    await page.waitForLoadState('networkidle');
    expect(await page.evaluate(() => Boolean(customElements.get('wa-button')))).toBe(true);
    expect(pageErrors).toEqual([]);
    expect(output()).not.toContain('new dependencies optimized');
    expect(output()).not.toContain('optimized dependencies changed. reloading');
  } finally {
    await browser.close();
    await stopChild(child);
    expect(await readdir(runtimeTemp)).toEqual([]);
    await rm(runtimeTemp, { recursive: true, force: true });
  }
}, 60_000);

// Keep the browser fixture readable while changing only its scheduling (HS2-XSP0GJ).
// prettier-ignore
it.concurrent('serves the app without a Vite reconnect client, HMR websocket, or reconnect logging', async () => {
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
    const captureDir = await mkdtemp(resolve(tmpdir(), 'hotsheet-stable-capture-'));
    const capturePath = resolve(captureDir, 'hs2-8jv12r-stable-client-after.png');
    const screenshotBytes = await page.screenshot({ path: capturePath, fullPage: true });
    expect(await readFile(capturePath)).toEqual(screenshotBytes);
  } finally {
    await browser.close();
    await stopChild(child);
    expect(await readdir(runtimeTemp)).toEqual([]);
    await rm(runtimeTemp, { recursive: true, force: true });
  }
}, 60_000);

// Keep the browser fixture readable while changing only its scheduling (HS2-XSP0GJ).
// prettier-ignore
it.concurrent('does not reload when the terminal runtime first lazy-loads its xterm dependencies', async () => {
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
