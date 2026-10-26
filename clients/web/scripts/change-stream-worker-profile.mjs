#!/usr/bin/env node
// HS2-MH5PWD: profile the shared change-stream worker against a real Hot Sheet server.
//
// Builds the production client and local host, supervises a real machine server under an
// isolated HOTSHEET_HOME, opens two projects in Chromium, and compares the main-thread stream
// with the worker over real WebSockets: event delivery lag (server write to rendered row),
// p95 click-to-paint (Event Timing), and garbage-collected heap (page and worker isolates).
//
// Usage: npm run profile:worker:real [-- --events 40 --clicks 30 --json out.json]
import { execFile, spawn } from 'node:child_process';
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { createServer } from 'node:net';
import { tmpdir } from 'node:os';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs, promisify } from 'node:util';

import { chromium } from '@playwright/test';

import { withoutGitRepositoryEnv } from './repository-env.mjs';
import { assertFixtureCli, assertFixtureServer } from './test-fixture-capabilities.mjs';

const webRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..'),
  repoRoot = resolve(webRoot, '../..'),
  run = promisify(execFile),
  targetDir = process.env.CARGO_TARGET_DIR ?? resolve(repoRoot, 'target'),
  binaries = {
    HOTSHEET_SERVER_BIN: resolve(targetDir, 'debug/hotsheet-server'),
    HOTSHEET_CLI_BIN: resolve(targetDir, 'debug/hotsheet-cli'),
    HOTSHEET_MIGRATE_BIN: resolve(targetDir, 'debug/hotsheet-migrate'),
  };
assertFixtureServer(binaries.HOTSHEET_SERVER_BIN);
assertFixtureCli(binaries.HOTSHEET_CLI_BIN);
const { values: args } = parseArgs({
  options: {
    events: { type: 'string', default: '40' },
    background: { type: 'string', default: '40' },
    clicks: { type: 'string', default: '30' },
    json: { type: 'string' },
  },
});
const productionEnvironment = Object.fromEntries(
  Object.entries(withoutGitRepositoryEnv(process.env)).filter(
    ([name]) => !name.startsWith('VITEST') && name !== 'NODE_ENV' && name !== 'TEST',
  ),
);

async function availablePort() {
  const server = createServer();
  await new Promise((done, reject) => server.once('error', reject).listen(0, '127.0.0.1', done));
  const address = server.address();
  await new Promise((done) => server.close(done));
  if (!address || typeof address === 'string') throw new Error('Could not allocate a port.');
  return address.port;
}

const percentile = (values, portion) => {
  if (!values.length) return null;
  const sorted = [...values].sort((a, b) => a - b);
  return Math.round(sorted[Math.min(sorted.length - 1, Math.ceil(sorted.length * portion) - 1)]);
};
const summarize = (values) => ({
  n: values.length,
  p50: percentile(values, 0.5),
  p95: percentile(values, 0.95),
  max: values.length ? Math.round(Math.max(...values)) : null,
});

/** Used heap after a forced GC in the page and in each change-stream worker isolate. */
async function collectedHeap(page) {
  const cdp = await page.context().newCDPSession(page);
  await cdp.send('HeapProfiler.collectGarbage');
  const pageBytes = (await cdp.send('Runtime.getHeapUsage')).usedSize;
  let workerBytes = 0;
  const pending = new Map();
  let nextId = 1;
  cdp.on('Target.receivedMessageFromTarget', ({ message }) => {
    const parsed = JSON.parse(message);
    pending.get(parsed.id)?.(parsed.result);
  });
  const sendTo = (sessionId, method, params = {}) =>
    new Promise((done) => {
      const id = nextId++;
      pending.set(id, done);
      void cdp.send('Target.sendMessageToTarget', { sessionId, message: JSON.stringify({ id, method, params }) });
    });
  const { targetInfos } = await cdp.send('Target.getTargets');
  const workerTargets = targetInfos.filter(
    (info) => (info.type === 'worker' || info.type === 'shared_worker') && info.url.includes('change-stream'),
  );
  for (const target of workerTargets) {
    const { sessionId } = await cdp.send('Target.attachToTarget', { targetId: target.targetId, flatten: false });
    await sendTo(sessionId, 'HeapProfiler.collectGarbage');
    workerBytes += (await sendTo(sessionId, 'Runtime.getHeapUsage')).usedSize;
    await cdp.send('Target.detachFromTarget', { sessionId });
  }
  await cdp.detach();
  return { pageBytes, workerBytes, totalBytes: pageBytes + workerBytes, workerCount: workerTargets.length };
}

async function profileMode(browser, origin, projects, worker) {
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  const page = await context.newPage();
  await page.addInitScript(
    ({ roots, enabled }) => {
      localStorage.setItem('hotsheet.open-projects', JSON.stringify(roots));
      localStorage.setItem('hotsheet.change-stream-worker', enabled ? 'true' : 'false');
      const scope = window;
      scope.__workerProfile = { seen: {}, events: [], sockets: 0 };
      new MutationObserver(() => {
        for (const row of document.querySelectorAll('[data-ticket-slug]')) {
          const slug = row.getAttribute('data-ticket-slug');
          if (!(slug in scope.__workerProfile.seen)) scope.__workerProfile.seen[slug] = Date.now();
        }
      }).observe(document, { subtree: true, childList: true });
      new PerformanceObserver((list) => {
        for (const entry of list.getEntries())
          if (entry.name === 'click' || entry.name === 'pointerup') scope.__workerProfile.events.push(entry.duration);
      }).observe({ type: 'event', durationThreshold: 16, buffered: true });
      const NativeWebSocket = window.WebSocket;
      window.WebSocket = class extends NativeWebSocket {
        constructor(url, protocols) {
          super(url, protocols ?? []);
          if (String(url).includes('/ws/sync')) scope.__workerProfile.sockets += 1;
        }
      };
    },
    { roots: projects.map((project) => project.root), enabled: worker },
  );
  const workers = [];
  // Long-poll requests (from the page or the worker) show whether delivery stayed on WebSockets.
  let longPolls = 0;
  context.on('request', (request) => {
    if (new URL(request.url()).pathname.endsWith('/ws/poll')) longPolls += 1;
  });
  page.on('worker', (candidate) => workers.push(candidate.url()));
  await page.goto(`${origin}/`);
  const [first, second] = projects;
  await page.getByRole('tab', { name: first.name }).waitFor();
  await page.getByRole('tab', { name: second.name }).waitFor();
  await page.getByRole('tab', { name: first.name }).click();
  await page.locator(`[data-ticket-slug="${first.seedSlug}"]`).waitFor();
  await page.waitForTimeout(1_500);
  const heapBefore = await collectedHeap(page);

  // Delivery lag: write through the real server, then time until the visible project renders it.
  const lags = [];
  for (let index = 0; index < Number(args.events); index += 1) {
    const background = index < Number(args.background) ? createTicket(second, `Background ${index}`) : undefined;
    const started = Date.now();
    const created = await createTicket(first, `Visible ${index}`);
    await background;
    const handle = await page.waitForFunction((slug) => window.__workerProfile.seen[slug], created.slug, {
      timeout: 30_000,
      polling: 'raf',
    });
    lags.push((await handle.jsonValue()) - started);
  }

  // Click-to-paint while both projects keep streaming.
  await page.evaluate(() => {
    window.__workerProfile.events = [];
  });
  let streaming = true;
  const churn = (async () => {
    for (let index = 0; streaming; index += 1) {
      await createTicket(second, `Churn ${index}`);
      await createTicket(first, `Churn ${index}`);
    }
  })();
  for (let index = 0; index < Number(args.clicks); index += 1) {
    await page.getByRole('tab', { name: (index % 2 ? first : second).name }).click();
    await page.waitForTimeout(150);
  }
  streaming = false;
  await churn;
  await page.getByRole('tab', { name: first.name }).click();
  await page.waitForTimeout(2_000);
  const clicks = await page.evaluate(() => window.__workerProfile.events);
  const heapAfter = await collectedHeap(page);
  const sockets = await page.evaluate(() => window.__workerProfile.sockets);
  await context.close();
  return {
    mode: worker ? 'worker' : 'main-thread',
    changeStreamWorkers: Math.max(workers.filter((url) => url.includes('change-stream')).length, heapAfter.workerCount),
    mainThreadSyncSockets: sockets,
    longPollRequests: longPolls,
    deliveryLagMs: summarize(lags),
    clickToPaintMs: summarize(clicks),
    heapBefore,
    heapAfter,
  };
}

let createTicket;

async function main() {
  // The host bundle resolves its runtime packages from this package's node_modules.
  await mkdir(resolve(webRoot, 'target'), { recursive: true });
  const buildRoot = await mkdtemp(resolve(webRoot, 'target/worker-profile-'));
  const home = await mkdtemp(resolve(tmpdir(), 'hotsheet-worker-profile-home-'));
  let host;
  let browser;
  try {
    console.error('Building the production client and local host…');
    await run('npx', ['vite', 'build', '--outDir', resolve(buildRoot, 'dist'), '--emptyOutDir'], {
      cwd: webRoot,
      env: productionEnvironment,
      maxBuffer: 64 * 1024 * 1024,
    });
    await run(
      'npx',
      ['vite', 'build', '--ssr', 'src/local-host.ts', '--outDir', resolve(buildRoot, 'dist-host'), '--emptyOutDir'],
      { cwd: webRoot, env: productionEnvironment, maxBuffer: 64 * 1024 * 1024 },
    );
    const revision = JSON.parse((await run(binaries.HOTSHEET_SERVER_BIN, ['--revision-status'])).stdout).build_revision;
    const port = await availablePort(),
      origin = `http://127.0.0.1:${port}`;
    let log = '';
    host = spawn(process.execPath, [resolve(buildRoot, 'dist-host/local-host.js'), '--port', String(port)], {
      cwd: webRoot,
      env: { ...productionEnvironment, ...binaries, HOT_SHEET_BUILD_REVISION: revision, HOTSHEET_HOME: home },
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    host.stdout.on('data', (chunk) => (log += chunk));
    host.stderr.on('data', (chunk) => (log += chunk));
    for (let attempt = 0; ; attempt += 1) {
      try {
        if ((await fetch(`${origin}/`)).ok) break;
      } catch {
        /* not listening yet */
      }
      if (attempt > 300 || host.exitCode !== null) throw new Error(`Local host did not start.\n${log}`);
      await new Promise((done) => setTimeout(done, 100));
    }

    const projects = [];
    for (const [name, prefix] of [
      ['alpha', 'PA'],
      ['beta', 'PB'],
    ]) {
      const root = resolve(home, name),
        ticketStore = resolve(home, `${name}.hs2`);
      await mkdir(root);
      await run(binaries.HOTSHEET_CLI_BIN, ['-C', ticketStore, 'init', '--prefix', prefix], {
        env: { ...productionEnvironment, HOTSHEET_HOME: home },
      });
      const opened = await fetch(`${origin}/__hotsheet/projects/open`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ root, ticketStore }),
      });
      if (opened.status !== 201) throw new Error(`Could not open ${root}: ${await opened.text()}`);
      const project = await opened.json();
      projects.push({ ...project, root, api: `${origin}${project.apiPath}` });
    }
    createTicket = async (project, title) => {
      const response = await fetch(`${project.api}/checkouts/${encodeURIComponent(project.id)}/tickets`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ title, category: 'task', up_next: true }),
      });
      if (!response.ok) throw new Error(`Ticket create failed: ${response.status} ${await response.text()}`);
      return response.json();
    };
    for (const project of projects) project.seedSlug = (await createTicket(project, `Seed ${project.name}`)).slug;

    browser = await chromium.launch();
    const results = [];
    // Alternate modes so machine drift affects both equally.
    for (const worker of [false, true, false, true]) results.push(await profileMode(browser, origin, projects, worker));
    const report = { generatedAt: new Date().toISOString(), args, results };
    if (args.json) await writeFile(args.json, `${JSON.stringify(report, null, 2)}\n`);
    console.log(JSON.stringify(report, null, 2));
  } finally {
    await browser?.close();
    if (host && host.exitCode === null) host.kill('SIGTERM');
    await run(binaries.HOTSHEET_CLI_BIN, ['serve', '-C', resolve(home, 'server-bootstrap.hs2'), '--stop', '--force'], {
      env: { ...productionEnvironment, HOTSHEET_HOME: home },
    }).catch(() => undefined);
    await rm(buildRoot, { recursive: true, force: true });
    await rm(home, { recursive: true, force: true }).catch(() => undefined);
  }
}

await main();
