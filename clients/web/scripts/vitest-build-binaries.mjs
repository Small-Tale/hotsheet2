import { spawnSync } from 'node:child_process';
import { readdirSync, statSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

// HS2-A0M8CM: several suites spawn the real target/debug hotsheet-cli, hotsheet-server, and
// hotsheet-migrate binaries. Building them inside one test file raced every other file that
// was already running: cargo relinked (or first created) the binaries mid-run and those files
// saw ENOENT / "not built at". Vitest runs this once, before any worker starts, so the
// binaries are current and nothing in the suite rewrites them while tests use them.
//
// HS2-3PX1E4: only the `binaries` Vitest project (vitest.config.ts) carries this setup, so a
// run that selects no binary-spawning file never starts cargo. When it does run, it skips the
// build if every binary is newer than every Rust build input.
const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');

export const TEST_BINARIES = ['hotsheet-cli', 'hotsheet-server', 'hotsheet-migrate'];

/** Everything whose change can change the binaries: sources, manifests, and embedded assets. */
export const BUILD_INPUTS = [
  'Cargo.toml',
  'Cargo.lock',
  'rust-toolchain.toml',
  '.cargo/config.toml',
  'crates',
  'plugins',
  '.claude/skills/hotsheet',
  '.agents/skills/hotsheet',
];

const SKIPPED_DIRECTORIES = new Set(['target', 'node_modules', '.git']);

/** Newest modification time (ms) under `inputs`, or `Infinity` when nothing can be read. */
export function newestInputMtime(root, inputs = BUILD_INPUTS, fs = { statSync, readdirSync }) {
  let newest = -Infinity;
  const visit = (path) => {
    let stat;
    try {
      stat = fs.statSync(path);
    } catch {
      return;
    }
    if (!stat.isDirectory()) {
      newest = Math.max(newest, stat.mtimeMs);
      return;
    }
    for (const entry of fs.readdirSync(path)) {
      if (!SKIPPED_DIRECTORIES.has(entry)) visit(join(path, entry));
    }
  };
  for (const input of inputs) visit(join(root, input));
  return newest === -Infinity ? Infinity : newest;
}

/** The binaries are fresh when each exists and is newer than every build input. */
export function binariesAreFresh(root, { targetDir = join(root, 'target'), fs = { statSync, readdirSync } } = {}) {
  let oldestBinary = Infinity;
  for (const name of TEST_BINARIES) {
    try {
      oldestBinary = Math.min(oldestBinary, fs.statSync(join(targetDir, 'debug', name)).mtimeMs);
    } catch {
      return false;
    }
  }
  return oldestBinary > newestInputMtime(root, BUILD_INPUTS, fs);
}

/**
 * `HOTSHEET_VITEST_BUILD`: `auto` (default) builds unless the binaries are fresh, `always`
 * forces the cargo build, and `skip` trusts whatever binaries exist.
 */
export function buildDecision(env, fresh) {
  const mode = env.HOTSHEET_VITEST_BUILD ?? 'auto';
  if (mode === 'always') return 'build';
  if (mode === 'skip') return 'skip';
  if (mode !== 'auto') throw new Error(`HOTSHEET_VITEST_BUILD must be auto, always, or skip (got ${mode}).`);
  return fresh() ? 'skip' : 'build';
}

export default function buildTestBinaries() {
  const targetDir = process.env.CARGO_TARGET_DIR ? resolve(repoRoot, process.env.CARGO_TARGET_DIR) : undefined;
  if (buildDecision(process.env, () => binariesAreFresh(repoRoot, { targetDir })) === 'skip') return;
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
