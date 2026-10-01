import { execFile, spawn } from 'node:child_process';
import { once } from 'node:events';
import { readdir, rm } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { promisify } from 'node:util';

import { beforeAll, expect, it } from 'vitest';

// HS2-VQ8ZWT: a browser-test runner killed before its teardown must not orphan the real ticket
// server it started. The harness holds the server's stdin pipe; SIGKILL closes it.
const webRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..'),
  repoRoot = resolve(webRoot, '../..'),
  run = promisify(execFile),
  harness = pathToFileURL(resolve(webRoot, 'tests/real-ticket-server.ts')).href;

beforeAll(async () => {
  await run('cargo', ['build', '-p', 'hotsheet-cli', '-p', 'hotsheet-server', '--bins'], { cwd: repoRoot });
}, 300_000);

function alive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

it('stops the real ticket server when its runner is SIGKILLed before teardown', async () => {
  const owner = spawn(
    process.execPath,
    [
      '--experimental-strip-types',
      '--no-warnings',
      '--input-type=module',
      '-e',
      `const { realTicketServer } = await import(${JSON.stringify(harness)});
       const server = await realTicketServer();
       console.log(JSON.stringify({ pid: server.pid, store: server.store, home: server.home }));
       setInterval(() => {}, 1 << 30); // a runner mid-test: never reaches stop()`,
    ],
    { cwd: webRoot, stdio: ['ignore', 'pipe', 'pipe'] },
  );
  let output = '';
  owner.stderr.on('data', (chunk) => (output += chunk.toString()));
  let started;
  try {
    started = await new Promise((resolveStarted, reject) => {
      let line = '';
      owner.stdout.on('data', (chunk) => {
        line += chunk.toString();
        const end = line.indexOf('\n');
        if (end >= 0) resolveStarted(JSON.parse(line.slice(0, end)));
      });
      owner.once('exit', () => reject(new Error(`harness exited early:\n${output}`)));
    });
    expect(alive(started.pid)).toBe(true);

    owner.kill('SIGKILL');
    await once(owner, 'exit');
    const deadline = Date.now() + 10_000;
    while (alive(started.pid) && Date.now() < deadline) await new Promise((r) => setTimeout(r, 50));
    expect(alive(started.pid), 'the orphaned server must stop on its own').toBe(false);
    const registrations = (await readdir(resolve(started.home, 'instances')).catch(() => [])).filter((name) =>
      name.endsWith('.json'),
    );
    expect(registrations).toEqual([]);
  } finally {
    if (owner.exitCode === null && owner.signalCode === null) owner.kill('SIGKILL');
    if (started) {
      if (alive(started.pid)) process.kill(started.pid, 'SIGKILL');
      await rm(dirname(started.store), { recursive: true, force: true });
    }
  }
}, 60_000);
