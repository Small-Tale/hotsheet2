import { spawn } from 'node:child_process';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { createServer } from 'node:net';
import { tmpdir } from 'node:os';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { afterEach, expect, it } from 'vitest';

import { stopServerProcess, trackServerProcess } from './playwright-server-process.mjs';

// Two real Vite servers launched the way Playwright's webServer launches them: each answers only its
// own test-server identity, a second server on a taken port refuses instead of sharing it, and
// stopping one run's server leaves the other serving (HS2-BJ00D1).
const webRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const vite = resolve(webRoot, 'node_modules/vite/bin/vite.js');
const owned = [];
const directories = [];

afterEach(async () => {
  // Only the servers this test started are stopped; nothing else on the machine is touched.
  await Promise.all(owned.splice(0).map(stopServerProcess));
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
  const lifetime = trackServerProcess(child);
  owned.push(lifetime);
  let output = '';
  child.stdout.on('data', (chunk) => {
    output += chunk;
  });
  child.stderr.on('data', (chunk) => {
    output += chunk;
  });
  return { ...lifetime, output: () => output };
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
  await stopServerProcess(first);
  expect(await identity(shared, 'run-a')).toBe('refused');
  expect(await identity(separate, 'run-b')).toBe(200);
}, 60_000);

it.each(['already exited', 'running'])(
  'waits for a %s parent’s inherited-pipe writer before removing its cache (HS2-0BQ8J9)',
  async (state) => {
    const cacheDir = await mkdtemp(resolve(tmpdir(), 'hotsheet-identity-writer-'));
    directories.push(cacheDir);
    // Like esbuild's service, this real descendant inherits the parent's output pipes. A separate
    // inherited control pipe holds its final cache write until the test releases it, without sleeps.
    const writer = `
    import { createReadStream } from 'node:fs';
    import { writeFile } from 'node:fs/promises';
    import { resolve } from 'node:path';
    createReadStream(null, { fd: 3 }).once('data', async () => {
      await writeFile(resolve(process.argv[1], 'writer-finished'), 'finished');
      process.exit(0);
    });
    process.stdout.write('writer-ready\\n');
  `;
    const parent = `
    import { spawn } from 'node:child_process';
    spawn(process.execPath, ['--input-type=module', '-e', process.argv[1], process.argv[2]], {
      stdio: ['ignore', 'inherit', 'inherit', 3],
    }).once('spawn', () => { if (process.argv[3] === 'already exited') process.exit(0); });
  `;
    const child = spawn(process.execPath, ['--input-type=module', '-e', parent, writer, cacheDir, state], {
      stdio: ['ignore', 'pipe', 'pipe', 'pipe'],
    });
    const server = trackServerProcess(child);
    owned.push(server);
    const writerReady = new Promise((resolveReady) => {
      child.stdout.on('data', (chunk) => {
        if (chunk.toString().includes('writer-ready')) resolveReady();
      });
    });
    let stopFinished = false;
    try {
      await writerReady;
      if (state === 'already exited') expect(await server.exited).toBe(0);
      const stopping = stopServerProcess(server).then(() => {
        stopFinished = true;
      });
      await server.exited;
      // Give an incorrectly resolved stop its promise turn; the writer remains event-gated.
      await new Promise(setImmediate);
      expect(stopFinished).toBe(false);
      child.stdio[3].end('release');
      await stopping;
      expect(await readFile(resolve(cacheDir, 'writer-finished'), 'utf8')).toBe('finished');
      await rm(cacheDir, { recursive: true, force: true });
    } finally {
      if (!child.stdio[3].writableEnded) child.stdio[3].end('release');
      await server.closed;
    }
  },
);
