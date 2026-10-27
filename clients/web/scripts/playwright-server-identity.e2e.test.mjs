import { spawn } from 'node:child_process';
import { mkdtemp, rm } from 'node:fs/promises';
import { createServer } from 'node:net';
import { tmpdir } from 'node:os';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { afterEach, expect, it } from 'vitest';

// Two real Vite servers launched the way Playwright's webServer launches them: each answers only its
// own test-server identity, a second server on a taken port refuses instead of sharing it, and
// stopping one run's server leaves the other serving (HS2-BJ00D1).
const webRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const vite = resolve(webRoot, 'node_modules/vite/bin/vite.js');
const owned = [];
const directories = [];

afterEach(async () => {
  // Only the servers this test started are stopped; nothing else on the machine is touched.
  await Promise.all(owned.splice(0).map(stop));
  await Promise.all(directories.splice(0).map((directory) => rm(directory, { recursive: true, force: true })));
});

async function availablePort() {
  const server = createServer();
  await new Promise((resolveListen, reject) => server.once('error', reject).listen(0, '127.0.0.1', resolveListen));
  const address = server.address();
  await new Promise((resolveClose) => server.close(resolveClose));
  if (!address || typeof address === 'string') throw new Error('Could not allocate a test port.');
  return address.port;
}

async function launch(port, id) {
  const cacheDir = await mkdtemp(resolve(tmpdir(), 'hotsheet-identity-vite-'));
  directories.push(cacheDir);
  const child = spawn(process.execPath, [vite, '--host', '127.0.0.1', '--port', String(port), '--strictPort'], {
    cwd: webRoot,
    env: { ...process.env, HOTSHEET_VITE_CACHE_DIR: cacheDir, HOTSHEET_WEB_TEST_SERVER_ID: id },
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  owned.push(child);
  let output = '';
  child.stdout.on('data', (chunk) => {
    output += chunk;
  });
  child.stderr.on('data', (chunk) => {
    output += chunk;
  });
  const exited = new Promise((resolveExit) => child.once('exit', (code) => resolveExit(code)));
  return { child, exited, output: () => output };
}

async function stop(child) {
  if (child.exitCode !== null || child.signalCode !== null) return;
  const exited = new Promise((resolveExit) => child.once('exit', resolveExit));
  child.kill('SIGTERM');
  const force = setTimeout(() => child.kill('SIGKILL'), 10_000);
  try {
    await exited;
  } finally {
    clearTimeout(force);
  }
}

async function identity(port, id) {
  try {
    return (await fetch(`http://127.0.0.1:${port}/__hotsheet-test-server/${id}`)).status;
  } catch {
    return 'refused';
  }
}

/** Resolve when Vite reports it is listening, or fail if it exits first; then its identity must answer. */
async function ready(server, port, id) {
  const listening = new Promise((resolveReady) => {
    const inspect = () => {
      if (/ready in/i.test(server.output())) resolveReady();
    };
    server.child.stdout.on('data', inspect);
    inspect();
  });
  const exited = server.exited.then((code) => {
    throw new Error(`Vite exited (${code}) before serving ${id}: ${server.output()}`);
  });
  await Promise.race([listening, exited]);
  expect(await identity(port, id)).toBe(200);
}

it('answers only its own identity, refuses a taken port, and survives the other run’s teardown (HS2-BJ00D1)', async () => {
  const shared = await availablePort();
  const first = await launch(shared, 'run-a');
  await ready(first, shared, 'run-a');
  expect(await identity(shared, 'run-b')).toBe(404);
  expect(await identity(shared, '')).toBe(404);

  // A second run on the same port cannot bind and exits; the shared port still never proves its identity.
  const collision = await launch(shared, 'run-b');
  expect(await collision.exited).not.toBe(0);
  expect(collision.output()).toMatch(/already in use/i);
  expect(await identity(shared, 'run-b')).toBe(404);
  expect(await identity(shared, 'run-a')).toBe(200);

  // Distinct ports isolate the runs, and stopping the first leaves the second serving its own identity.
  const separate = await availablePort();
  const second = await launch(separate, 'run-b');
  await ready(second, separate, 'run-b');
  expect(await identity(separate, 'run-a')).toBe(404);
  await stop(first.child);
  expect(await identity(shared, 'run-a')).toBe('refused');
  expect(await identity(separate, 'run-b')).toBe(200);
}, 60_000);
