// HS2-5R706Q: exercise the same staged bridge and binaries that Tauri packages.
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { resolve } from 'node:path';

const tauri = resolve(import.meta.dirname, '../src-tauri');
const resources = resolve(tauri, 'resources');
const triple = process.arch === 'arm64' ? 'aarch64-apple-darwin' : 'x86_64-apple-darwin';
const bin = (name) => resolve(tauri, 'binaries', `${name}-${triple}`);
const home = mkdtempSync(resolve(tmpdir(), 'hs2-desktop-host-'));
const ready = resolve(home, 'ready.json');
const child = spawn(bin('node'), [resolve(resources, 'dist-host/local-host.mjs'), '--port', '0'], {
  env: {
    ...process.env,
    HOTSHEET_HOME: home,
    HOTSHEET_REPO_ROOT: resources,
    HOTSHEET_SERVER_BIN: bin('hotsheet-server'),
    HOTSHEET_CLI_BIN: bin('hotsheet-cli'),
    HOTSHEET_MIGRATE_BIN: bin('hotsheet-migrate'),
    HOTSHEET_MCP_BIN: bin('hotsheet-mcp'),
    HOTSHEET_MIGRATOR: resolve(resources, 'migrator/src/export.mjs'),
    HOTSHEET_CLIENT_READY_FILE: ready,
    HOTSHEET_HOST_EXIT_ON_STDIN_EOF: '1',
  },
  stdio: ['pipe', 'ignore', 'pipe'],
});
let stderr = '';
child.stderr.on('data', (chunk) => {
  stderr += chunk.toString();
});
try {
  const deadline = Date.now() + 30_000;
  while (!existsSync(ready) && child.exitCode === null && Date.now() < deadline)
    await new Promise((done) => setTimeout(done, 50));
  assert.ok(existsSync(ready), `bridge did not start: ${stderr}`);
  const { url } = JSON.parse(readFileSync(ready, 'utf8'));
  assert.equal(JSON.parse(readFileSync(resolve(home, 'client.json'), 'utf8')).url, url);
  const response = await fetch(url);
  assert.equal(response.status, 200);
  assert.match(await response.text(), /Hot Sheet/);
  child.stdin.end();
  const exit = await Promise.race([
    new Promise((done) => child.once('exit', done)),
    new Promise((_, reject) => setTimeout(() => reject(new Error(`bridge did not stop: ${stderr}`)), 10_000)),
  ]);
  assert.equal(exit, 0, stderr);
  assert.equal(existsSync(ready), false);
  assert.equal(existsSync(resolve(home, 'client.json')), false);
} finally {
  if (child.exitCode === null) child.kill();
  rmSync(home, { recursive: true, force: true });
}
