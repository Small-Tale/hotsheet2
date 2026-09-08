import { cp, mkdtemp, rm, symlink } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawn } from 'node:child_process';

const scriptRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const excludedTopLevel = new Set([
  'coverage',
  'dist',
  'node_modules',
  'playwright-report',
  'test-results',
]);

export async function createStableSnapshot(sourceRoot = scriptRoot, temporaryRoot = tmpdir()) {
  const snapshotRoot = await mkdtemp(join(temporaryRoot, 'hotsheet-web-stable-'));
  const filter = source => {
    const firstSegment = relative(sourceRoot, source).split('/')[0];
    return !excludedTopLevel.has(firstSegment);
  };
  try {
    await cp(sourceRoot, snapshotRoot, { recursive: true, filter });
    await symlink(resolve(sourceRoot, 'node_modules'), resolve(snapshotRoot, 'node_modules'), 'dir');
    return snapshotRoot;
  } catch (error) {
    await rm(snapshotRoot, { recursive: true, force: true });
    throw error;
  }
}

export async function removeStableSnapshot(snapshotRoot) {
  await rm(snapshotRoot, { recursive: true, force: true });
}

export function stableDevEnvironment(sourceRoot, snapshotRoot, environment = process.env) {
  const repoRoot = environment.HOTSHEET_REPO_ROOT ?? resolve(sourceRoot, '../..');
  return {
    ...environment,
    HOTSHEET_REPO_ROOT: repoRoot,
    HOTSHEET_DEV_REVIEW_REPO_ROOT: environment.HOTSHEET_DEV_REVIEW_REPO_ROOT ?? repoRoot,
    HOTSHEET_WEB_STABLE_DEV: '1',
    HOTSHEET_VITE_CACHE_DIR: resolve(snapshotRoot, '.vite-cache'),
  };
}

async function main() {
  const sourceRoot = resolve(process.env.HOTSHEET_WEB_STABLE_SOURCE_ROOT ?? scriptRoot);
  const temporaryRoot = resolve(process.env.HOTSHEET_WEB_STABLE_TEMP_ROOT ?? tmpdir());
  const snapshotRoot = await createStableSnapshot(sourceRoot, temporaryRoot);
  const viteEntry = resolve(sourceRoot, 'node_modules/vite/bin/vite.js');
  const child = spawn(process.execPath, [viteEntry, '--host', '127.0.0.1', ...process.argv.slice(2)], {
    cwd: snapshotRoot,
    env: stableDevEnvironment(sourceRoot, snapshotRoot),
    stdio: 'inherit',
  });

  console.log(`Stable dev snapshot: ${snapshotRoot}`);
  console.log('Workspace edits will be visible after this command is restarted.');

  let stoppingSignal;
  const signalHandlers = new Map();
  for (const signal of ['SIGINT', 'SIGTERM']) {
    const handler = () => {
      if (stoppingSignal) return;
      stoppingSignal = signal;
      child.kill(signal);
    };
    signalHandlers.set(signal, handler);
    process.on(signal, handler);
  }
  const result = await new Promise((resolveClose, rejectClose) => {
    child.once('error', rejectClose);
    child.once('close', (code, signal) => resolveClose({ code, signal }));
  });
  try {
    await removeStableSnapshot(snapshotRoot);
  } finally {
    for (const [signal, handler] of signalHandlers) process.off(signal, handler);
  }
  const exitSignal = stoppingSignal ?? result.signal;
  process.exitCode = result.code ?? (exitSignal === 'SIGINT' ? 130 : 143);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  await main();
}
