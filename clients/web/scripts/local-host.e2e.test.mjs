import { execFile, spawn } from 'node:child_process';
import { mkdir, mkdtemp, rm } from 'node:fs/promises';
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

/** Live processes whose command line mentions `home` (the test's isolated HOTSHEET_HOME). */
async function processesUsing(home) {
  const { stdout } = await run('ps', ['-Ao', 'pid=,command=']);
  return stdout
    .split('\n')
    .filter((line) => line.includes(home))
    .map((line) => line.trim());
}

/**
 * Stop the machine server the host supervised under `home` (HS2-WQ65KT). The CLI must see the same
 * HOTSHEET_HOME to find that server's discovery record, and match the build the host launched.
 */
async function stopMachineServer(home) {
  await run(workingTreeBinaries.HOTSHEET_CLI_BIN, ['serve', '-C', resolve(home, 'server-bootstrap.hs2'), '--stop'], {
    env: { ...process.env, HOTSHEET_HOME: home },
  }).catch(() => undefined);
  const deadline = Date.now() + 10_000;
  while ((await processesUsing(home)).length > 0 && Date.now() < deadline)
    await new Promise((resolveWait) => setTimeout(resolveWait, 100));
}

// Build as `npm run prod` does: without the test runner's environment, which would change Vite's mode.
const productionEnvironment = Object.fromEntries(
  Object.entries(process.env).filter(([name]) => !name.startsWith('VITEST') && name !== 'NODE_ENV' && name !== 'TEST'),
);

// Exercise the working tree's server, not a possibly stale release build the host would prefer.
const workingTreeBinaries = {
  HOTSHEET_SERVER_BIN: resolve(repoRoot, 'target/debug/hotsheet-server'),
  HOTSHEET_CLI_BIN: resolve(repoRoot, 'target/debug/hotsheet-cli'),
  HOTSHEET_MIGRATE_BIN: resolve(repoRoot, 'target/debug/hotsheet-migrate'),
};

beforeAll(async () => {
  await run('npm', ['run', 'build'], { cwd: webRoot, env: productionEnvironment });
  await run('npm', ['run', 'build:host'], { cwd: webRoot, env: productionEnvironment });
}, 180_000);

it('serves the production client and the local bridge without Vite', async () => {
  const home = await mkdtemp(resolve(tmpdir(), 'hotsheet-local-host-home-')),
    port = await availablePort(),
    origin = `http://127.0.0.1:${port}`;
  let log = '',
    child,
    browser;
  // Everything that can fail runs inside the try, so the host and its machine server are always
  // stopped: a browser that failed to launch used to leave both running (HS2-WQ65KT).
  try {
    child = spawn(process.execPath, ['dist-host/local-host.js', '--port', String(port)], {
      cwd: webRoot,
      env: { ...productionEnvironment, ...workingTreeBinaries, HOTSHEET_HOME: home },
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    child.stdout.on('data', (chunk) => (log += chunk));
    child.stderr.on('data', (chunk) => (log += chunk));
    browser = await chromium.launch();
    await waitForHost(`${origin}/`, child, () => log);
    // The HTTP listener may be ready before the child stdout chunk reaches us.
    await expect.poll(() => log).toContain(`Hot Sheet production client on ${origin}/`);
    // HS2-D2JQ9A: the host says which server build it launches (release when one is built).
    await expect.poll(() => log).toMatch(/Using (release|debug) Hot Sheet binaries/);
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
    expect(await (await fetch(`${origin}/settings/anything`)).text()).toContain('<main id="app" class="kui-app-root">');
    // The bridge is live: it supervises a real machine server under the isolated home.
    const checkouts = await fetch(`${origin}/__hotsheet/checkouts`);
    expect(checkouts.status).toBe(200);
    expect(Array.isArray(await checkouts.json())).toBe(true);

    // HS2-ARJ9J1: a project's store is hosted while a tab has it open, unhosted when the last
    // tab closes it, and hosted again by the next request through that project.
    const project = resolve(home, 'app'),
      ticketStore = resolve(home, 'app.hs2');
    await mkdir(project);
    await run(workingTreeBinaries.HOTSHEET_CLI_BIN, ['-C', ticketStore, 'init', '--prefix', 'LH'], {
      env: { ...process.env, HOTSHEET_HOME: home },
    });
    const opened = await fetch(`${origin}/__hotsheet/projects/open`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ root: project, ticketStore }),
    });
    expect(opened.status).toBe(201);
    const { id, apiPath } = await opened.json(),
      api = `${origin}${apiPath}`,
      hostsProject = async () => (await (await fetch(`${api}/stores`)).json()).some((store) => store.prefix === 'LH');
    expect((await fetch(`${api}/ws/poll?timeout_ms=0&since=0&client=e2e-tab`)).status).toBe(200);
    expect(await hostsProject()).toBe(true);
    expect((await fetch(`${api}/close?client=e2e-tab`, { method: 'POST' })).status).toBe(204);
    let unhosted = false;
    for (let attempt = 0; attempt < 50 && !unhosted; attempt += 1) {
      unhosted = !(await hostsProject());
      if (!unhosted) await new Promise((resolveWait) => setTimeout(resolveWait, 100));
    }
    expect(unhosted).toBe(true);
    expect((await fetch(`${api}/checkouts/${encodeURIComponent(id)}/tickets`)).status).toBe(200);
    expect(await hostsProject()).toBe(true);
  } finally {
    await browser?.close();
    if (child) {
      child.kill('SIGTERM');
      await new Promise((resolveExit) =>
        child.exitCode === null && child.signalCode === null ? child.once('exit', resolveExit) : resolveExit(),
      );
    }
    await stopMachineServer(home);
    const survivors = await processesUsing(home);
    await rm(home, { recursive: true, force: true });
    // Nothing the test started may outlive it: not the host, not its supervised machine server.
    expect(survivors).toEqual([]);
  }
}, 120_000);
