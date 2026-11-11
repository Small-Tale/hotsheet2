import { execFile, spawn } from 'node:child_process';
import { cp, mkdir, mkdtemp, readdir, realpath, rename, rm, writeFile } from 'node:fs/promises';
import { createServer } from 'node:net';
import { tmpdir } from 'node:os';
import { basename, dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';

import { chromium } from '@playwright/test';
import { expect, it } from 'vitest';

import { assertFixtureCli, assertFixtureServer } from './test-fixture-capabilities.mjs';

// HS2-AGGWG2: the real compiled host, browser restore, and project API share one
// private two-ticket fixture. The ordered trace distinguishes a failed open from
// a later ticket read; it never sends a request to the user's running host.
const webRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = resolve(webRoot, '../..');
const target = resolve(repoRoot, process.env.CARGO_TARGET_DIR ?? 'target');
const executable = (name) => resolve(target, 'debug', `${name}${process.platform === 'win32' ? '.exe' : ''}`);
const cli = executable('hotsheet-cli');
const server = executable('hotsheet-server');
const migrate = executable('hotsheet-migrate');
const vite = resolve(webRoot, 'node_modules/vite/bin/vite.js');
const run = promisify(execFile);
const environment = Object.fromEntries(
  Object.entries(process.env).filter(([name]) => !name.startsWith('VITEST') && name !== 'NODE_ENV' && name !== 'TEST'),
);

async function port() {
  const socket = createServer();
  await new Promise((resolveListen, reject) => socket.once('error', reject).listen(0, '127.0.0.1', resolveListen));
  const address = socket.address();
  await new Promise((resolveClose) => socket.close(resolveClose));
  if (!address || typeof address === 'string') throw new Error('Could not allocate an isolated host port.');
  return address.port;
}

async function waitForHost(child, origin, output) {
  const deadline = Date.now() + 90_000;
  while (Date.now() < deadline) {
    if (child.exitCode !== null) throw new Error(`Compiled host exited: ${output()}`);
    if (output().includes(`Hot Sheet production client on ${origin}/`)) {
      const response = await fetch(origin);
      if (response.ok) return;
    }
    await new Promise((resolveWait) => setTimeout(resolveWait, 100));
  }
  throw new Error(`Compiled host did not become ready: ${output()}`);
}

async function ticketFiles(store) {
  const root = resolve(store, 'tickets');
  const buckets = (await readdir(root)).filter((name) => !name.startsWith('.'));
  const files = (
    await Promise.all(
      buckets.map(async (bucket) =>
        (await readdir(resolve(root, bucket)))
          .filter((name) => name.endsWith('.md'))
          .map((name) => resolve(root, bucket, name)),
      ),
    )
  ).flat();
  expect(files).toHaveLength(2);
  return files.sort();
}

function recordResponses(page) {
  const responses = [];
  page.on('response', (response) => {
    const { pathname } = new URL(response.url());
    if (pathname.startsWith('/__hotsheet/')) responses.push({ path: pathname, status: response.status() });
  });
  return responses;
}

it('traces compiled-host remembered reopen with a missing synthetic ticket (HS2-AGGWG2)', async () => {
  assertFixtureServer(server);
  assertFixtureCli(cli);
  const home = await mkdtemp(resolve(tmpdir(), 'hs2-aggwg2-home-'));
  await mkdir(resolve(webRoot, 'target'), { recursive: true });
  const build = await mkdtemp(resolve(webRoot, 'target/hs2-aggwg2-build-'));
  const project = resolve(home, 'project');
  const store = resolve(home, 'project.hs2');
  const origin = `http://127.0.0.1:${await port()}`;
  let host;
  let browser;
  let moved;
  let hostOutput = '';
  try {
    await mkdir(project);
    await cp(resolve(repoRoot, 'plugins'), resolve(home, 'plugins'), { recursive: true });
    const fixtureEnv = { ...environment, HOTSHEET_HOME: home };
    await run(cli, ['-C', store, 'init', '--prefix', 'TR'], { env: fixtureEnv });
    for (const title of ['Missing synthetic', 'Healthy synthetic'])
      await run(cli, ['-C', store, 'new', title, '--category', 'task'], { env: fixtureEnv });
    await run(process.execPath, [vite, 'build', '--outDir', resolve(build, 'dist'), '--emptyOutDir'], {
      cwd: webRoot,
      env: environment,
    });
    await run(
      process.execPath,
      [vite, 'build', '--ssr', 'src/local-host.ts', '--outDir', resolve(build, 'dist-host'), '--emptyOutDir'],
      {
        cwd: webRoot,
        env: environment,
      },
    );
    const revision = JSON.parse((await run(server, ['--revision-status'])).stdout).build_revision;
    host = spawn(process.execPath, [resolve(build, 'dist-host/local-host.js'), '--port', new URL(origin).port], {
      cwd: webRoot,
      env: {
        ...environment,
        HOTSHEET_HOME: home,
        HOTSHEET_REPO_ROOT: home,
        HOTSHEET_SERVER_BIN: server,
        HOTSHEET_CLI_BIN: cli,
        HOTSHEET_MIGRATE_BIN: migrate,
        HOTSHEET_TEST_SERVER_OWNER_PID: 'host',
        HOT_SHEET_BUILD_REVISION: revision,
      },
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    host.stdout.on('data', (chunk) => {
      hostOutput += chunk;
    });
    host.stderr.on('data', (chunk) => {
      hostOutput += chunk;
    });
    await waitForHost(host, origin, () => hostOutput);

    const registered = await fetch(`${origin}/__hotsheet/projects/open`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ root: project, ticketStore: store }),
    });
    expect(registered.status).toBe(201);
    const { id, apiPath } = await registered.json();
    const api = `${origin}${apiPath}`;
    browser = await chromium.launch();
    const page = await browser.newPage();
    const responses = recordResponses(page);
    await page.goto(origin);
    await page.getByRole('button', { name: 'Open Project' }).first().click();
    await page.getByRole('textbox', { name: 'Project Folder' }).fill(project);
    await page.getByRole('button', { name: 'Open Project', exact: true }).last().click();
    await page.getByRole('heading', { name: 'Queue', level: 1 }).waitFor();
    const canonicalProject = await realpath(project);
    await expect
      .poll(() => page.evaluate(() => JSON.parse(localStorage.getItem('hotsheet.open-projects') ?? '[]')))
      .toContain(canonicalProject);
    const baseline = responses.length;
    const [missing] = await ticketFiles(store);
    moved = `${missing}.held`;
    await rename(missing, moved);

    await page.reload();
    await page.getByRole('heading', { name: 'Queue', level: 1 }).waitFor();
    const reopen = responses.slice(baseline);
    const opens = reopen.filter((response) => response.path === '/__hotsheet/projects/open');
    expect(opens.length).toBeGreaterThan(0);
    expect(opens.every((response) => response.status === 201)).toBe(true);
    expect(await page.locator('[data-restore-failure="true"]').count()).toBe(0);
    const listing = await fetch(`${api}/checkouts/${encodeURIComponent(id)}/tickets`);
    expect(listing.status).toBe(200);
    const tickets = await listing.json();
    expect(tickets).toHaveLength(1);
    const detail = await fetch(
      `${api}/checkouts/${encodeURIComponent(id)}/tickets/${encodeURIComponent(tickets[0].id)}`,
    );
    expect(detail.status).toBe(200);
    const missingId = basename(missing).slice(0, -3);
    const absentDetail = await fetch(`${api}/checkouts/${encodeURIComponent(id)}/tickets/${missingId}`);
    expect(absentDetail.status).toBe(404);
    expect(reopen.some((response) => response.status >= 500)).toBe(false);
    console.info(
      'HS2-AGGWG2 synthetic missing-file reopen trace:',
      JSON.stringify({
        bridge: reopen,
        listing: listing.status,
        healthyDetail: detail.status,
        absentDetail: absentDetail.status,
        failedTile: false,
      }),
    );

    // Hold the file absent for precisely the compiled bridge's next open request.
    // This is a stronger control than the earlier free-running 4 ms file toggle.
    await rename(moved, missing);
    moved = undefined;
    let targetedOpens = 0;
    await page.route('**/__hotsheet/projects/open', async (route) => {
      targetedOpens += 1;
      const held = `${missing}.held`;
      await rename(missing, held);
      try {
        const response = await route.fetch();
        await route.fulfill({ response });
      } finally {
        await rename(held, missing);
      }
    });
    const transientStart = responses.length;
    await page.reload();
    await page.getByRole('heading', { name: 'Queue', level: 1 }).waitFor();
    await page.unroute('**/__hotsheet/projects/open');
    const transient = responses.slice(transientStart);
    const transientOpenResponses = transient.filter((response) => response.path === '/__hotsheet/projects/open');
    expect(targetedOpens).toBeGreaterThan(0);
    expect(transientOpenResponses.length).toBeGreaterThan(0);
    expect(transientOpenResponses.every((response) => response.status === 201)).toBe(true);
    expect(await page.locator('[data-restore-failure="true"]').count()).toBe(0);
    console.info('HS2-AGGWG2 targeted missing-file reopen trace:', JSON.stringify(transient));
    await writeFile(
      resolve(webRoot, 'target/hs2-aggwg2-reopen-trace.json'),
      JSON.stringify(
        {
          initialOpen: registered.status,
          stableMissing: {
            bridge: reopen,
            listing: listing.status,
            healthyDetail: detail.status,
            absentDetail: absentDetail.status,
            failedTile: false,
          },
          targetedOpenMissing: { bridge: transient, targetedOpens, failedTile: false },
        },
        null,
        2,
      ),
    );
  } finally {
    if (moved) await rename(moved, moved.slice(0, -5)).catch(() => {});
    await browser?.close();
    if (host?.exitCode === null && host.signalCode === null) {
      host.kill('SIGTERM');
      await new Promise((resolveExit) => host.once('exit', resolveExit));
    }
    await run(cli, ['serve', '-C', resolve(home, 'server-bootstrap.hs2'), '--stop', '--force'], {
      env: { ...environment, HOTSHEET_HOME: home },
    }).catch(() => {});
    // The stopped server can still remove its instance registration while the home is deleted
    // (ENOTEMPTY on instances/, HS2-4XV5K3), so retry like the other fixture cleanups.
    await rm(home, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
    await rm(build, { recursive: true, force: true, maxRetries: 10, retryDelay: 100 });
  }
}, 300_000);
