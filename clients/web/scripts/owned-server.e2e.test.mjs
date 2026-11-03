import { spawn, spawnSync } from 'node:child_process';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { connect, createServer } from 'node:net';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { afterAll, afterEach, beforeAll, expect, it, vi } from 'vitest';

// HS2-F6F8MA: real processes launched the way Playwright launches `webServer` (a `shell: true`,
// `detached: true` child whose stdin is a pipe from the runner). Every process here is started by this
// file, recorded by pid and start time, and signalled only while that identity still matches.
const webRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const launcher = resolve(webRoot, 'scripts/owned-server.mjs');
const vite = resolve(webRoot, 'node_modules/vite/bin/vite.js');
// Process exits are awaited by state for up to 15 s each on a loaded gate host.
vi.setConfig({ testTimeout: 30_000 });
let fixtures;
const recorded = [];
const handles = [];

// The runner stands in for Playwright's test runner: detached webServer group, stdin pipe held open.
const RUNNER = `
const { spawn } = require('node:child_process');
const child = spawn(process.argv[1], { shell: true, detached: true, stdio: ['pipe', 'pipe', 'pipe'] });
console.log('launcher ' + child.pid);
child.stdout.on('data', (chunk) => process.stdout.write(chunk));
child.stderr.on('data', (chunk) => process.stdout.write(chunk));
child.on('close', (code, signal) => { console.log('closed ' + code + ' ' + signal); process.exit(0); });
`;

// A synthetic fixture server. Modes add the descendants the launcher must not leave behind.
const SERVER = `
const net = require('node:net');
const { spawn } = require('node:child_process');
const mode = process.argv[2] ?? 'plain';
if (mode === 'ignore-term') process.on('SIGTERM', () => {});
if (mode === 'exit-7') setTimeout(() => process.exit(7), 0);
const descendants = [];
if (mode === 'descendants') {
  // Same group, ignores SIGTERM, bound to nothing: only the launcher's group sweep can end it.
  descendants.push(spawn(process.execPath, ['-e', 'process.on("SIGTERM", () => {}); setInterval(() => {}, 1e6)'], { stdio: 'ignore' }));
  // Detached into its own group but bound to this server by its stdin, like Vite's esbuild service.
  descendants.push(spawn(process.execPath, ['-e', 'process.stdin.on("end", () => process.exit(0)); process.stdin.resume(); setInterval(() => {}, 1e6)'], { stdio: ['pipe', 'ignore', 'ignore'], detached: true }));
}
const server = net.createServer().listen(0, '127.0.0.1', () => {
  console.log('ready ' + [process.pid, server.address().port, ...descendants.map((d) => d.pid)].join(' '));
});
`;

beforeAll(async () => {
  fixtures = await mkdtemp(join(tmpdir(), 'hotsheet-owned-server-'));
  await writeFile(join(fixtures, 'runner.cjs'), RUNNER);
  await writeFile(join(fixtures, 'server.cjs'), SERVER);
});

afterAll(async () => {
  if (fixtures) await rm(fixtures, { recursive: true, force: true });
});

afterEach(() => {
  // Stop only fixtures this file started whose pid still names the same process.
  for (const entry of recorded.splice(0))
    if (alive(entry))
      try {
        process.kill(entry.pid, 'SIGKILL');
      } catch {
        /* already gone */
      }
  for (const handle of handles.splice(0)) handle.kill('SIGKILL');
});

function processInfo(pid) {
  const result = spawnSync('ps', ['-o', 'pgid=,lstart=', '-p', String(pid)], { encoding: 'utf8' });
  const match = /^\s*(\d+)\s+(.+?)\s*$/.exec(result.stdout);
  return match ? { pgid: Number(match[1]), started: match[2] } : null;
}

function record(pid) {
  const info = processInfo(pid);
  if (!info) throw new Error(`Fixture ${pid} exited before it was recorded.`);
  const entry = { pid, ...info };
  recorded.push(entry);
  return entry;
}

/** Same pid and same start time: a reused pid never counts as the recorded process. */
function alive(entry) {
  return processInfo(entry.pid)?.started === entry.started;
}

function descendantsOf(rootPid) {
  const rows = spawnSync('ps', ['-A', '-o', 'pid=,ppid='], { encoding: 'utf8' })
    .stdout.trim()
    .split('\n')
    .map((line) => line.trim().split(/\s+/).map(Number));
  const found = [rootPid];
  for (let index = 0; index < found.length; index += 1)
    for (const [pid, ppid] of rows) if (ppid === found[index] && !found.includes(pid)) found.push(pid);
  return found.slice(1);
}

async function listening(port) {
  return new Promise((resolveProbe) => {
    const socket = connect(port, '127.0.0.1');
    socket.once('connect', () => {
      socket.destroy();
      resolveProbe(true);
    });
    socket.once('error', () => resolveProbe(false));
  });
}

async function availablePort() {
  const server = createServer();
  await new Promise((resolveListen, reject) => server.once('error', reject).listen(0, '127.0.0.1', resolveListen));
  const address = server.address();
  await new Promise((resolveClose) => server.close(resolveClose));
  if (!address || typeof address === 'string') throw new Error('Could not allocate a test port.');
  return address.port;
}

/** Poll process state (not the network) until `check` holds or the bound expires. */
async function eventually(check, label, timeoutMs = 15_000) {
  const deadline = Date.now() + timeoutMs;
  while (!(await check())) {
    if (Date.now() > deadline) throw new Error(`Timed out waiting for ${label}`);
    await new Promise((resolveTick) => setTimeout(resolveTick, 50));
  }
}

/** Start a runner in its own group so the test can end it the way an interrupted gate ends. */
function startRunner(command) {
  const runner = spawn(process.execPath, ['-e', RUNNER, command], {
    cwd: webRoot,
    detached: true,
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  handles.push(runner);
  let output = '';
  const waiters = [];
  const onData = (chunk) => {
    output += chunk;
    for (const waiter of waiters.splice(0)) waiter();
  };
  runner.stdout.on('data', onData);
  runner.stderr.on('data', onData);
  const closed = new Promise((resolveClose) => runner.once('close', (code, signal) => resolveClose({ code, signal })));
  const line = (pattern) =>
    new Promise((resolveLine, reject) => {
      const inspect = () => {
        const match = pattern.exec(output);
        if (match) resolveLine(match);
        else waiters.push(inspect);
      };
      void closed.then(() => {
        if (!pattern.test(output)) reject(new Error(`Runner closed before ${pattern}: ${output}`));
      });
      inspect();
    });
  return { runner, closed, line, output: () => output };
}

function launch(serverArgs, launcherArgs = ['--exit-on-stdin-eof']) {
  return `node ${launcher} ${launcherArgs.join(' ')} -- node ${join(fixtures, 'server.cjs')} ${serverArgs}`;
}

async function startOwned(mode, launcherArgs) {
  const run = startRunner(launch(mode, launcherArgs));
  const [, launcherPid] = await run.line(/launcher (\d+)/);
  const owned = { launcher: record(Number(launcherPid)) };
  const ready = await run.line(/ready ([\d ]+)\n/);
  const [serverPid, port, ...descendantPids] = ready[1].trim().split(' ').map(Number);
  owned.server = record(serverPid);
  owned.descendants = descendantPids.map(record);
  return { ...run, owned, port };
}

async function startForeign() {
  // Same program and command line as an owned server, but never launched by owned-server.
  const foreign = spawn(process.execPath, [join(fixtures, 'server.cjs'), 'descendants'], {
    detached: true,
    stdio: ['ignore', 'pipe', 'ignore'],
  });
  handles.push(foreign);
  const ready = await new Promise((resolveReady) => {
    let output = '';
    foreign.stdout.on('data', (chunk) => {
      output += chunk;
      const match = /ready ([\d ]+)\n/.exec(output);
      if (match) resolveReady(match[1].trim().split(' ').map(Number));
    });
  });
  const [pid, port, ...descendantPids] = ready;
  return { entries: [record(pid), ...descendantPids.map(record)], port };
}

async function expectReleased({ owned, port }) {
  const all = [owned.launcher, owned.server, ...owned.descendants];
  await eventually(() => all.every((entry) => !alive(entry)), 'every owned process to exit');
  expect(await listening(port)).toBe(false);
}

it('runs the server in the launcher-led group Playwright creates for its webServer (HS2-F6F8MA)', async () => {
  const run = await startOwned('plain');
  // `sh -c` execs the launcher, so it leads the detached group and the server shares it.
  expect(run.owned.launcher.pgid).toBe(run.owned.launcher.pid);
  expect(run.owned.server.pgid).toBe(run.owned.launcher.pid);
  expect(await listening(run.port)).toBe(true);
  process.kill(-run.runner.pid, 'SIGKILL');
  await expectReleased(run);
});

for (const signal of ['SIGKILL', 'SIGTERM'])
  it(`releases the owned port and every owned descendant when the runner ends by ${signal} (HS2-F6F8MA)`, async () => {
    const foreign = await startForeign();
    const run = await startOwned('descendants');
    expect(run.owned.descendants).toHaveLength(2);
    // The detached descendant really left the server's group: a group signal alone cannot reach it.
    expect(run.owned.descendants[1].pgid).not.toBe(run.owned.launcher.pgid);
    // The runner dies without any teardown of its own; only the closed stdin pipe remains.
    process.kill(-run.runner.pid, signal);
    await expectReleased(run);
    // A same-command process and listener the launcher never started is untouched.
    expect(foreign.entries.every(alive)).toBe(true);
    expect(await listening(foreign.port)).toBe(true);
  });

it('force-kills a server that ignores SIGTERM after the grace period (HS2-F6F8MA)', async () => {
  const run = await startOwned('ignore-term', ['--exit-on-stdin-eof', '--grace-ms', '300']);
  process.kill(-run.runner.pid, 'SIGKILL');
  await expectReleased(run);
});

it('stops on a signal to its group while the runner stays alive (HS2-F6F8MA)', async () => {
  const run = await startOwned('descendants');
  process.kill(-run.owned.launcher.pid, 'SIGTERM');
  await expectReleased(run);
  await run.closed;
  expect(run.output()).toMatch(/closed /);
});

it('reports the server exit code when the server stops by itself (HS2-F6F8MA)', async () => {
  const run = startRunner(launch('exit-7'));
  await run.closed;
  expect(run.output()).toMatch(/closed 7 null/);
});

it('ignores stdin without --exit-on-stdin-eof (HS2-F6F8MA)', async () => {
  const run = await startOwned('plain', []);
  // Close only the runner's pipe handle to the launcher, not the runner: the server keeps serving.
  process.kill(-run.runner.pid, 'SIGKILL');
  await run.closed;
  expect(alive(run.owned.server)).toBe(true);
  expect(await listening(run.port)).toBe(true);
  // This file started it; end its group by the recorded launcher identity.
  if (alive(run.owned.launcher)) process.kill(-run.owned.launcher.pid, 'SIGKILL');
  await eventually(() => !alive(run.owned.server), 'manual fixture stop');
});

it('stops the real Vite fixture server and its esbuild service when the runner is killed (HS2-F6F8MA)', async () => {
  const port = await availablePort();
  const cacheDir = await mkdtemp(join(tmpdir(), 'hotsheet-owned-vite-'));
  try {
    // The same command shape as playwright.config.ts.
    const run = startRunner(
      `HOTSHEET_VITE_CACHE_DIR=${cacheDir} node ${launcher} --exit-on-stdin-eof -- node ${vite} --host 127.0.0.1 --port ${port} --strictPort`,
    );
    const [, launcherPid] = await run.line(/launcher (\d+)/);
    await run.line(/ready in/i);
    expect(await listening(port)).toBe(true);
    const tree = [Number(launcherPid), ...descendantsOf(Number(launcherPid))].map(record);
    // Launcher, Vite, and at least esbuild's service.
    expect(tree.length).toBeGreaterThanOrEqual(3);
    process.kill(-run.runner.pid, 'SIGKILL');
    await eventually(() => tree.every((entry) => !alive(entry)), 'every Vite fixture process to exit');
    expect(await listening(port)).toBe(false);
  } finally {
    await rm(cacheDir, { recursive: true, force: true });
  }
}, 60_000);
