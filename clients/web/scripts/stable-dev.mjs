import { cp, mkdir, mkdtemp, readdir, readFile, rm, stat, symlink, writeFile } from 'node:fs/promises';
import { homedir } from 'node:os';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawn } from 'node:child_process';

const scriptRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const excludedTopLevel = new Set(['coverage', 'dist', 'node_modules', 'playwright-report', 'target', 'test-results']);

const SNAPSHOT_PREFIX = 'hotsheet-web-stable-';
const OWNER_FILE = '.stable-dev-owner.json';
/** An ownerless snapshot younger than this may still be starting up, so pruning leaves it alone. */
const OWNERLESS_GRACE_MS = 10 * 60 * 1000;

/**
 * Where snapshots live (HS2-ZJ6VN3). Not the OS temporary directory: macOS's daily `$TMPDIR` cleanup
 * deletes files not accessed for three days, and `fs.cp` clones keep each source file's old access time,
 * so every module Vite had not read yet (such as the lazily imported Dev Review entry) vanished from a
 * running snapshot and was served as `index.html`. The per-user cache directory is never swept that way.
 */
export function stableSnapshotParent(environment = process.env, platform = process.platform, home = homedir()) {
  if (environment.HOTSHEET_WEB_STABLE_TEMP_ROOT) return resolve(environment.HOTSHEET_WEB_STABLE_TEMP_ROOT);
  if (platform === 'darwin') return join(home, 'Library', 'Caches', 'hotsheet-web-stable');
  if (platform === 'win32')
    return join(environment.LOCALAPPDATA ?? join(home, 'AppData', 'Local'), 'hotsheet-web-stable');
  return join(environment.XDG_CACHE_HOME ?? join(home, '.cache'), 'hotsheet-web-stable');
}

function processIsAlive(pid) {
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    return error?.code === 'EPERM';
  }
}

/**
 * Remove snapshots whose owning stable-dev process is gone. Snapshots outside `$TMPDIR` are no longer
 * reclaimed by the OS, so a crashed or killed server would otherwise leave its copy behind forever.
 */
export async function pruneStaleSnapshots(parent, { isAlive = processIsAlive, now = Date.now() } = {}) {
  let entries;
  try {
    entries = await readdir(parent, { withFileTypes: true });
  } catch {
    return [];
  }
  const removed = [];
  for (const entry of entries) {
    if (!entry.isDirectory() || !entry.name.startsWith(SNAPSHOT_PREFIX)) continue;
    const path = join(parent, entry.name);
    let owner;
    try {
      owner = JSON.parse(await readFile(join(path, OWNER_FILE), 'utf8')).pid;
    } catch {
      owner = undefined;
    }
    if (Number.isInteger(owner) ? isAlive(owner) : now - (await stat(path)).mtimeMs < OWNERLESS_GRACE_MS) continue;
    await rm(path, { recursive: true, force: true });
    removed.push(path);
  }
  return removed;
}

export async function createStableSnapshot(
  sourceRoot = scriptRoot,
  temporaryRoot = stableSnapshotParent(),
  ownerPid = process.pid,
) {
  await mkdir(temporaryRoot, { recursive: true });
  const snapshotRoot = await mkdtemp(join(temporaryRoot, SNAPSHOT_PREFIX));
  const filter = (source) => {
    const firstSegment = relative(sourceRoot, source).split('/')[0];
    return !excludedTopLevel.has(firstSegment);
  };
  try {
    // Record the owner first so a concurrent pruner never mistakes a snapshot being copied for an orphan.
    await writeFile(join(snapshotRoot, OWNER_FILE), JSON.stringify({ pid: ownerPid }));
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

export async function runStableDev({
  sourceRoot = resolve(process.env.HOTSHEET_WEB_STABLE_SOURCE_ROOT ?? scriptRoot),
  temporaryRoot = stableSnapshotParent(),
  viteArguments = process.argv.slice(2),
  environment = process.env,
  processHost = process,
  createSnapshot = createStableSnapshot,
  removeSnapshot = removeStableSnapshot,
  pruneSnapshots = pruneStaleSnapshots,
  spawnChild = spawn,
  log = console.log,
} = {}) {
  let stoppingSignal;
  let snapshotRoot;
  let child;
  const signalHandlers = new Map();
  for (const signal of ['SIGINT', 'SIGTERM']) {
    const handler = () => {
      if (stoppingSignal) return;
      stoppingSignal = signal;
      child?.kill(signal);
    };
    signalHandlers.set(signal, handler);
    processHost.on(signal, handler);
  }

  try {
    await pruneSnapshots(temporaryRoot).catch(() => []);
    snapshotRoot = await createSnapshot(sourceRoot, temporaryRoot);
    if (stoppingSignal) return stoppingSignal === 'SIGINT' ? 130 : 143;

    const viteEntry = resolve(sourceRoot, 'node_modules/vite/bin/vite.js');
    child = spawnChild(processHost.execPath, [viteEntry, '--host', '127.0.0.1', ...viteArguments], {
      cwd: snapshotRoot,
      env: stableDevEnvironment(sourceRoot, snapshotRoot, environment),
      stdio: 'inherit',
    });

    log(`Stable dev snapshot: ${snapshotRoot}`);
    log('Workspace edits will be visible after this command is restarted.');

    const result = await new Promise((resolveClose, rejectClose) => {
      child.once('error', rejectClose);
      child.once('close', (code, signal) => resolveClose({ code, signal }));
    });
    const exitSignal = stoppingSignal ?? result.signal;
    return result.code ?? (exitSignal === 'SIGINT' ? 130 : 143);
  } finally {
    try {
      if (snapshotRoot) await removeSnapshot(snapshotRoot);
    } finally {
      for (const [signal, handler] of signalHandlers) processHost.off(signal, handler);
    }
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = await runStableDev();
}
