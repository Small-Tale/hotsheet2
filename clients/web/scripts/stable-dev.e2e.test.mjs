import { spawn } from 'node:child_process';
import { mkdir, mkdtemp, readdir, rm, symlink, writeFile } from 'node:fs/promises';
import { createServer } from 'node:net';
import { tmpdir } from 'node:os';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { expect, it } from 'vitest';

const webRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');

async function availablePort() {
  const server = createServer();
  await new Promise((resolveListen, reject) => server.once('error', reject).listen(0, '127.0.0.1', resolveListen));
  const address = server.address();
  await new Promise(resolveClose => server.close(resolveClose));
  if (!address || typeof address === 'string') throw new Error('Could not allocate a test port.');
  return address.port;
}

async function waitForSource(url) {
  let lastError;
  for (let attempt = 0; attempt < 100; attempt += 1) {
    try {
      const response = await fetch(url);
      if (response.ok) return response.text();
    } catch (error) {
      lastError = error;
    }
    await new Promise(resolveWait => setTimeout(resolveWait, 50));
  }
  throw lastError ?? new Error(`Timed out waiting for ${url}`);
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

  const child = spawn(process.execPath, [resolve(webRoot, 'scripts/stable-dev.mjs'), '--port', String(port), '--strictPort'], {
    env: {
      ...process.env,
      HOTSHEET_WEB_STABLE_SOURCE_ROOT: fixture,
      HOTSHEET_WEB_STABLE_TEMP_ROOT: runtimeTemp,
    },
    stdio: 'ignore',
  });
  try {
    const url = `http://127.0.0.1:${port}/src/main.js`;
    expect(await waitForSource(url)).toContain('before');
    await writeFile(resolve(fixture, 'src/main.js'), 'window.snapshot = "after";');
    expect(await waitForSource(`${url}?after-edit`)).toContain('before');
  } finally {
    child.kill('SIGTERM');
    await new Promise(resolveExit => child.once('exit', resolveExit));
    const remainingSnapshots = await readdir(runtimeTemp);
    await rm(fixture, { recursive: true, force: true });
    await rm(runtimeTemp, { recursive: true, force: true });
    expect(remainingSnapshots).toEqual([]);
  }
}, 15_000);
