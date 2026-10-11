import { spawnSync } from 'node:child_process';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

// HS2-A0M8CM: several suites spawn the real target/debug hotsheet-cli, hotsheet-server, and
// hotsheet-migrate binaries. Building them inside one test file raced every other file that
// was already running: cargo relinked (or first created) the binaries mid-run and those files
// saw ENOENT / "not built at". Vitest runs this once, before any worker starts, so the
// binaries are current and nothing in the suite rewrites them while tests use them.
const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');

export default function buildTestBinaries() {
  const result = spawnSync(
    'cargo',
    [
      'build',
      '--manifest-path',
      resolve(repoRoot, 'Cargo.toml'),
      '-p',
      'hotsheet-cli',
      '-p',
      'hotsheet-server',
      '--bins',
    ],
    { cwd: repoRoot, stdio: 'inherit' },
  );
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`cargo build of the test binaries exited with status ${result.status}.`);
}
