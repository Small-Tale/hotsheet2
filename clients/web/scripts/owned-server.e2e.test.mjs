import { spawn, spawnSync } from 'node:child_process';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { connect, createServer } from 'node:net';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { afterAll, afterEach, beforeAll, expect, it, vi } from 'vitest';

import { descendantsIn, windowsProcessTable } from './windows-process-table.mjs';

// HS2-F6F8MA: real processes launched the way Playwright launches `webServer` (a `shell: true`,
// `detached: true` child whose stdin is a pipe from the runner). Every process here is started by this
// file, recorded by pid and start time, and signalled only while that identity still matches.
const webRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const launcher = resolve(webRoot, 'scripts/owned-server.mjs');
const vite = resolve(webRoot, 'node_modules/vite/bin/vite.js');
const windows = process.platform === 'win32';
// Process exits are awaited by state for up to 15 s each on a loaded gate host.
vi.setConfig({ testTimeout: 30_000 });
let fixtures;
const recorded = [];
const handles = [];
// HS2-XH1S69: POSIX process groups this file created (detached runners, launchers, foreign fixtures).
// They are swept by group id, which needs no process-table read: a pgid cannot name another process
// while any member of the group is alive, so a successful group signal only reaches our fixtures.
const groups = new Set();
// HS2-XH1S69: an upper bound on every fixture's life, far beyond any test's bound, so a hard-killed
// test run (no afterEach) cannot leave fixtures running for days.
const FIXTURE_LIFETIME_MS = 5 * 60_000;

// The runner stands in for Playwright's test runner: detached webServer group, stdin pipe held open.
const RUNNER = `
const { spawn } = require('node:child_process');
setTimeout(() => process.exit(0), ${FIXTURE_LIFETIME_MS}).unref();
const config = JSON.parse(process.argv[1]);
const child = spawn(config.command, config.args, {
  shell: config.shell, detached: config.detached ?? true, stdio: ['pipe', 'pipe', 'pipe'],
  env: { ...process.env, ...config.env },
});
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
setTimeout(() => process.exit(0), ${FIXTURE_LIFETIME_MS}).unref();
const lifetime = 'setTimeout(() => process.exit(0), ${FIXTURE_LIFETIME_MS}).unref(); ';
if (mode === 'ignore-term') process.on('SIGTERM', () => {});
if (mode === 'exit-7') setTimeout(() => process.exit(7), 0);
const descendants = [];
if (mode === 'descendants') {
  // Same group, ignores SIGTERM, bound to nothing: only the launcher's group sweep can end it.
  descendants.push(spawn(process.execPath, ['-e', lifetime + 'process.on("SIGTERM", () => {}); setInterval(() => {}, 1e6)'], { stdio: 'ignore' }));
  // Detached into its own group but bound to this server by its stdin, like Vite's esbuild service.
  descendants.push(spawn(process.execPath, ['-e', lifetime + 'process.stdin.on("end", () => process.exit(0)); process.stdin.resume(); setInterval(() => {}, 1e6)'], { stdio: ['pipe', 'ignore', 'ignore'], detached: true }));
}
const server = net.createServer().listen(0, '127.0.0.1', () => {
  console.log('ready ' + [process.pid, server.address().port, ...descendants.map((d) => d.pid)].join(' '));
});
`;

beforeAll(async () => {
  fixtures = await mkdtemp(join(tmpdir(), 'hotsheet-owned-server-'));
  await writeFile(join(fixtures, 'runner.cjs'), RUNNER);
  await writeFile(join(fixtures, 'server.cjs'), SERVER);
  // HS2-XH1S69: every ownership check reads the process table. Without it (a command sandbox that
  // denies `ps`), fixtures would start but never be recorded, so stop before starting any.
  if (!(await processInfo(process.pid)))
    throw new Error('owned-server e2e cannot read the process table (`ps` unavailable); run it unsandboxed.');
  // Take the process-table helper's PowerShell cold start before any test's bounds start (HS2-S5BQRD).
  if (windows) await windowsProcessTable().warm();
}, 150_000);

afterAll(async () => {
  if (windows) windowsProcessTable().close();
  if (fixtures) await rm(fixtures, { recursive: true, force: true });
});

afterEach(async () => {
  // Stop only fixtures this file started whose pid still names the same process.
  for (const entry of recorded.splice(0))
    if (await alive(entry))
      try {
        if (windows) spawnSync('taskkill', ['/PID', String(entry.pid), '/T', '/F'], { windowsHide: true });
        else process.kill(entry.pid, 'SIGKILL');
      } catch {
        /* already gone */
      }
  for (const handle of handles.splice(0)) handle.kill('SIGKILL');
  sweepGroups();
});

/** SIGKILL every process group this file created and forget it; an empty group is already gone. */
function sweepGroups() {
  for (const group of groups) {
    groups.delete(group);
    try {
      process.kill(-group, 'SIGKILL');
    } catch {
      /* no member left */
    }
  }
}

/** Remember a detached POSIX group leader this file started, before anything can fail. */
function ownGroup(pid) {
  if (!windows && pid) groups.add(pid);
}

async function processInfo(pid) {
  if (windows) {
    // One warm helper answers every query; a PowerShell cold start per call timed this file out on
    // the arm64 runner (HS2-S5BQRD).
    const info = (await windowsProcessTable().snapshot()).get(pid);
    return info ? { pgid: null, ppid: info.ppid, started: info.started, name: info.name } : null;
  }
  const result = spawnSync('ps', ['-o', 'pgid=,stat=,lstart=', '-p', String(pid)], { encoding: 'utf8' });
  const match = /^\s*(\d+)\s+(\S+)\s+(.+?)\s*$/.exec(result.stdout);
  // A zombie has released its port and every resource; only its reaper can still remove the row.
  return match && !match[2].startsWith('Z') ? { pgid: Number(match[1]), started: match[3] } : null;
}

async function record(pid) {
  const info = await processInfo(pid);
  if (!info) throw new Error(`Fixture ${pid} exited before it was recorded.`);
  const entry = { pid, ...info };
  recorded.push(entry);
  return entry;
}

/** Same pid and same start time: a reused pid never counts as the recorded process. */
async function alive(entry) {
  return (await processInfo(entry.pid))?.started === entry.started;
}

/** The recorded entries still alive, by pid and name, for a timeout message. */
async function survivors(entries) {
  const live = [];
  for (const entry of entries) if (await alive(entry)) live.push(`${entry.pid} ${entry.name ?? ''}`.trim());
  return live.length ? `; still running: ${live.join(', ')}` : '';
}

/** Whether every entry has exited; one process-table read covers them all on Windows. */
async function allExited(entries) {
  return (await Promise.all(entries.map(alive))).every((live) => !live);
}

async function descendantsOf(rootPid) {
  if (windows) return descendantsIn(await windowsProcessTable().snapshot(), rootPid);
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

/**
 * Poll process state (not the network) until `check` holds or the bound expires. `explain` names
 * what was still there, so a Windows-only leak says which process survived (HS2-F6F8MA).
 */
async function eventually(check, label, timeoutMs = 15_000, explain = async () => '') {
  const deadline = Date.now() + timeoutMs;
  while (!(await check())) {
    if (Date.now() > deadline) throw new Error(`Timed out waiting for ${label}${await explain()}`);
    await new Promise((resolveTick) => setTimeout(resolveTick, 50));
  }
}

/** Start a runner in its own group so the test can end it the way an interrupted gate ends. */
function startRunner(command) {
  const runner = spawn(process.execPath, ['-e', RUNNER, JSON.stringify(command)], {
    cwd: webRoot,
    detached: true,
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  handles.push(runner);
  ownGroup(runner.pid);
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
  // `exec`, exactly as playwright.config.ts: without it Ubuntu's dash stays the group leader.
  if (windows)
    return {
      command: process.execPath,
      args: [launcher, ...launcherArgs, '--', process.execPath, join(fixtures, 'server.cjs'), serverArgs],
      shell: false,
    };
  return {
    command: `exec node ${launcher} ${launcherArgs.join(' ')} -- node ${join(fixtures, 'server.cjs')} ${serverArgs}`,
    args: [],
    shell: true,
  };
}

async function startOwned(mode, launcherArgs) {
  const run = startRunner(launch(mode, launcherArgs));
  const [, launcherPid] = await run.line(/launcher (\d+)/);
  // The launcher leads its own detached group; own it before the process-table read can fail.
  ownGroup(Number(launcherPid));
  const owned = { launcher: await record(Number(launcherPid)) };
  const ready = await run.line(/ready ([\d ]+)\n/);
  const [serverPid, port, ...descendantPids] = ready[1].trim().split(' ').map(Number);
  owned.server = await record(serverPid);
  owned.descendants = await Promise.all(descendantPids.map(record));
  return { ...run, owned, port };
}

async function startForeign() {
  // Same program and command line as an owned server, but never launched by owned-server.
  const foreign = spawn(process.execPath, [join(fixtures, 'server.cjs'), 'descendants'], {
    detached: true,
    stdio: ['ignore', 'pipe', 'ignore'],
  });
  handles.push(foreign);
  // Its same-group descendant ignores SIGTERM, and killing the leader alone orphans it (HS2-XH1S69).
  ownGroup(foreign.pid);
  const ready = await new Promise((resolveReady) => {
    let output = '';
    foreign.stdout.on('data', (chunk) => {
      output += chunk;
      const match = /ready ([\d ]+)\n/.exec(output);
      if (match) resolveReady(match[1].trim().split(' ').map(Number));
    });
  });
  const [pid, port, ...descendantPids] = ready;
  return { entries: await Promise.all([pid, ...descendantPids].map(record)), port };
}

async function expectReleased({ owned, port }) {
  const all = [owned.launcher, owned.server, ...owned.descendants];
  await eventually(() => allExited(all), 'every owned process to exit');
  expect(await listening(port)).toBe(false);
}

function stopRunner(run, signal) {
  // POSIX can signal the detached runner group; Windows has no such group and the runner's
  // process handle is the identity this fixture owns. Its death closes the launcher's pipe.
  process.kill(windows ? run.runner.pid : -run.runner.pid, signal);
}

it('runs the server in the launcher-led group Playwright creates for its webServer (HS2-F6F8MA)', async () => {
  const run = await startOwned('plain');
  // `sh -c exec` makes the shell's pid the launcher itself, so it leads the detached group and the
  // server shares it. Without `exec`, dash would keep that pid and leave the launcher a plain member.
  if (windows) {
    expect(run.owned.launcher.name).toMatch(/^node\.exe$/iu);
    expect(run.owned.server.ppid).toBe(run.owned.launcher.pid);
  } else {
    expect(spawnSync('ps', ['-o', 'comm=', '-p', String(run.owned.launcher.pid)], { encoding: 'utf8' }).stdout).toMatch(
      /node/u,
    );
    expect(run.owned.launcher.pgid).toBe(run.owned.launcher.pid);
    expect(run.owned.server.pgid).toBe(run.owned.launcher.pid);
  }
  expect(await listening(run.port)).toBe(true);
  stopRunner(run, 'SIGKILL');
  await expectReleased(run);
});

if (windows)
  it('closes the real cmd.exe webServer pipe when its runner dies (HS2-XP3Y4D)', async () => {
    const run = startRunner({
      command: `node scripts/owned-server.mjs --exit-on-stdin-eof -- node "${join(fixtures, 'server.cjs')}" plain`,
      args: [],
      shell: true,
      // Exactly as Playwright's launchProcess: `detached` only off Windows. A detached cmd.exe has no
      // console, so the node it starts gets a new console instead of the runner's pipes, and its
      // output never arrives (HS2-BQ5QK6).
      detached: false,
    });
    const ready = await run.line(/ready (\d+) (\d+)\n/);
    const server = await record(Number(ready[1]));
    const port = Number(ready[2]);
    expect(await listening(port)).toBe(true);
    stopRunner(run, 'SIGKILL');
    await eventually(async () => !(await alive(server)), 'cmd.exe fixture server to exit');
    expect(await listening(port)).toBe(false);
  });

for (const signal of ['SIGKILL', 'SIGTERM'])
  it(`releases the owned port and every owned descendant when the runner ends by ${signal} (HS2-F6F8MA)`, async () => {
    const foreign = await startForeign();
    const run = await startOwned('descendants');
    expect(run.owned.descendants).toHaveLength(2);
    // The detached descendant really left the server's group: a group signal alone cannot reach it.
    if (windows) expect(run.owned.descendants[1].ppid).toBe(run.owned.server.pid);
    else expect(run.owned.descendants[1].pgid).not.toBe(run.owned.launcher.pgid);
    // The runner dies without any teardown of its own; only the closed stdin pipe remains.
    stopRunner(run, signal);
    await expectReleased(run);
    // A same-command process and listener the launcher never started is untouched.
    expect((await Promise.all(foreign.entries.map(alive))).every(Boolean)).toBe(true);
    expect(await listening(foreign.port)).toBe(true);
  });

it('force-kills a server that ignores SIGTERM after the grace period (HS2-F6F8MA)', async () => {
  const run = await startOwned('ignore-term', ['--exit-on-stdin-eof', '--grace-ms', '300']);
  stopRunner(run, 'SIGKILL');
  await expectReleased(run);
});

it('stops on a signal to its group while the runner stays alive (HS2-F6F8MA)', async () => {
  const run = await startOwned('descendants');
  process.kill(windows ? run.owned.launcher.pid : -run.owned.launcher.pid, 'SIGTERM');
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
  stopRunner(run, 'SIGKILL');
  await run.closed;
  expect(await alive(run.owned.server)).toBe(true);
  expect(await listening(run.port)).toBe(true);
  // This file started it; end its group by the recorded launcher identity.
  if (await alive(run.owned.launcher)) {
    if (windows) spawnSync('taskkill', ['/PID', String(run.owned.launcher.pid), '/T', '/F'], { windowsHide: true });
    else process.kill(-run.owned.launcher.pid, 'SIGKILL');
  }
  await eventually(async () => !(await alive(run.owned.server)), 'manual fixture stop');
});

it('stops the real Vite fixture server and its esbuild service when the runner is killed (HS2-F6F8MA)', async () => {
  const port = await availablePort();
  const cacheDir = await mkdtemp(join(tmpdir(), 'hotsheet-owned-vite-'));
  try {
    // The same command shape as playwright.config.ts.
    const run = startRunner(
      windows
        ? {
            command: process.execPath,
            args: [
              launcher,
              '--exit-on-stdin-eof',
              '--',
              process.execPath,
              vite,
              '--host',
              '127.0.0.1',
              '--port',
              String(port),
              '--strictPort',
            ],
            shell: false,
            env: { HOTSHEET_VITE_CACHE_DIR: cacheDir },
          }
        : {
            command: `HOTSHEET_VITE_CACHE_DIR=${cacheDir} node ${launcher} --exit-on-stdin-eof -- node ${vite} --host 127.0.0.1 --port ${port} --strictPort`,
            args: [],
            shell: true,
          },
    );
    const [, launcherPid] = await run.line(/launcher (\d+)/);
    ownGroup(Number(launcherPid));
    await run.line(/ready in/i);
    expect(await listening(port)).toBe(true);
    const tree = await Promise.all([Number(launcherPid), ...(await descendantsOf(Number(launcherPid)))].map(record));
    // Launcher, Vite, and at least esbuild's service.
    expect(tree.length).toBeGreaterThanOrEqual(3);
    stopRunner(run, 'SIGKILL');
    await eventually(
      () => allExited(tree),
      'every Vite fixture process to exit',
      15_000,
      () => survivors(tree),
    );
    expect(await listening(port)).toBe(false);
  } finally {
    await rm(cacheDir, { recursive: true, force: true });
  }
}, 60_000);

if (!windows)
  it('sweeps an unrecorded foreign fixture and its SIGTERM-ignoring descendant by group (HS2-XH1S69)', async () => {
    // A fixture whose pids never reached `recorded` (the process-table read failed) must still end.
    const foreign = spawn(process.execPath, [join(fixtures, 'server.cjs'), 'descendants'], {
      detached: true,
      stdio: ['ignore', 'pipe', 'ignore'],
    });
    handles.push(foreign);
    ownGroup(foreign.pid);
    const pids = await new Promise((resolveReady) => {
      let output = '';
      foreign.stdout.on('data', (chunk) => {
        output += chunk;
        const match = /ready ([\d ]+)\n/.exec(output);
        if (match) resolveReady(match[1].trim().split(' ').map(Number));
      });
    });
    const [serverPid, port, ...descendantPids] = pids;
    const entries = await Promise.all([serverPid, ...descendantPids].map(processInfo));
    const tree = [serverPid, ...descendantPids].map((pid, index) => ({ pid, ...entries[index] }));
    expect(tree.every((entry) => entry.started)).toBe(true);
    // Leader first, as afterEach does with its handles, then the group sweep.
    foreign.kill('SIGKILL');
    sweepGroups();
    await eventually(
      () => allExited(tree),
      'the unrecorded foreign group to exit',
      15_000,
      () => survivors(tree),
    );
    expect(await listening(port)).toBe(false);
  });

it('gives every fixture a bounded lifetime (HS2-XH1S69)', () => {
  // A hard-killed test run skips afterEach; each fixture still exits on its own.
  expect(RUNNER).toContain(`setTimeout(() => process.exit(0), ${FIXTURE_LIFETIME_MS}).unref()`);
  expect(SERVER.match(/process\.exit\(0\), \d+\)\.unref\(\)/gu)).toHaveLength(2);
  expect(SERVER.match(/\['-e', lifetime \+/gu)).toHaveLength(2);
  expect(FIXTURE_LIFETIME_MS).toBeGreaterThan(60_000);
});
