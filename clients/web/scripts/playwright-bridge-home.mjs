import { spawnSync } from 'node:child_process';
import { existsSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { basename, dirname, join, resolve } from 'node:path';

import { nonBlank } from './non-blank.mjs';

/**
 * The Hot Sheet CLI the dev bridge launches binaries with: an explicit HOTSHEET_CLI_BIN, else the
 * checkout's debug build (the same resolution as `toolBinary` in `src/project-bridge.ts`).
 *
 * @param {NodeJS.ProcessEnv} env
 * @param {string} cwd the web client directory Playwright runs from
 * @param {NodeJS.Platform} platform
 * @returns {string}
 */
export function bridgeCliBinary(env, cwd = process.cwd(), platform = process.platform) {
  const explicit = nonBlank(env.HOTSHEET_CLI_BIN);
  if (explicit !== undefined) return explicit;
  const repoRoot = nonBlank(env.HOTSHEET_REPO_ROOT);
  const root = repoRoot === undefined ? resolve(cwd, '../..') : resolve(repoRoot);
  return resolve(root, 'target/debug', `hotsheet-cli${platform === 'win32' ? '.exe' : ''}`);
}

/**
 * Stop the machine bootstrap server the dev bridge started under a Playwright run's temporary home,
 * then remove the run's temporary directories (HS2-B0WP94).
 *
 * The bridge auto-starts a detached `hotsheet-server` for `<home>/server-bootstrap.hs2` the first
 * time a spec opens a project through it. Removing the home alone leaves that server running with
 * its directory deleted, so every local run that opened a project leaked one. A home whose bootstrap
 * store was never created has no server to stop. A failed stop is reported, never thrown, so the
 * runner still exits and removes its directories.
 *
 * @param {{
 *   home: string;
 *   directories: string[];
 *   env?: NodeJS.ProcessEnv;
 *   cli?: string;
 *   exists?: (path: string) => boolean;
 *   run?: typeof spawnSync;
 *   remove?: (path: string) => void;
 *   warn?: (message: string) => void;
 * }} input
 * @returns {{ stopped: boolean; error?: string }}
 */
export function releaseBridgeHome({
  home,
  directories,
  env = process.env,
  cli = bridgeCliBinary(env),
  exists = existsSync,
  run = spawnSync,
  // Node retries ENOTEMPTY, EBUSY, and EPERM itself: the stopped server can still be writing (HS2-XFFBA7).
  remove = (path) => rmSync(path, { recursive: true, force: true, maxRetries: 5, retryDelay: 100 }),
  warn = (message) => console.warn(message),
}) {
  const store = join(home, 'server-bootstrap.hs2');
  /** @type {{ stopped: boolean; error?: string }} */
  let outcome = { stopped: false };
  if (exists(store)) {
    const result = run(cli, ['serve', '-C', store, '--stop', '--force'], {
      env: { ...env, HOTSHEET_HOME: home },
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
      timeout: 20_000,
    });
    if (result.error || result.status !== 0) {
      const error = result.error?.message ?? (String(result.stderr ?? '').trim() || `exit ${String(result.status)}`);
      outcome = { stopped: false, error };
      warn(`Could not stop the Playwright bridge's bootstrap Hot Sheet server (${store}): ${error}`);
    } else outcome = { stopped: true };
  }
  // A directory that still cannot be removed is reported, never thrown: this runs in the runner's exit
  // handler, and the next run's sweep releases a home whose owner record remains (HS2-XFFBA7, HS2-P69XHN).
  for (const directory of directories)
    try {
      remove(directory);
    } catch (error) {
      warn(
        `Could not remove the Playwright run directory ${directory}: ${error instanceof Error ? error.message : String(error)}`,
      );
    }
  return outcome;
}

/** The record a Playwright runner leaves in its bridge home naming itself and its temporary directories. */
export const BRIDGE_OWNER_FILE = 'playwright-owner.json';
const BRIDGE_HOME_PREFIX = 'hotsheet-playwright-home-';
const RUN_DIRECTORY_PREFIX = 'hotsheet-playwright-';

/**
 * Record which process owns a run's bridge home and which temporary directories belong to that run
 * (HS2-P69XHN), so a later run can release them when this runner dies without its exit handler.
 *
 * @param {{ home: string; directories: string[]; pid?: number; write?: (path: string, text: string) => void }} input
 */
export function recordBridgeOwner({ home, directories, pid = process.pid, write = writeFileSync }) {
  write(join(home, BRIDGE_OWNER_FILE), JSON.stringify({ pid, directories }));
}

/**
 * Whether `pid` names a live process. A process this user may not signal still exists (`EPERM`).
 *
 * @param {number} pid
 * @param {(pid: number, signal: 0) => unknown} kill
 */
export function processAlive(pid, kill = (target, signal) => process.kill(target, signal)) {
  try {
    kill(pid, 0);
    return true;
  } catch (error) {
    return /** @type {NodeJS.ErrnoException} */ (error).code === 'EPERM';
  }
}

/**
 * Release the bridge homes of earlier runs whose runner died without its exit handler, such as a
 * SIGKILLed gate (HS2-P69XHN). HS2-B0WP94's exit handler cannot run then, so the detached bootstrap
 * server and the run's directories survive. Only a home whose owner record names a process that no
 * longer exists is released; a home without a record, or whose owner is alive (including a reused
 * pid), is left alone, so the sweep can never stop another live run's server. Only directories
 * directly under `root` with the run prefix are removed.
 *
 * @param {{
 *   root: string;
 *   list?: (path: string) => string[];
 *   read?: (path: string) => string;
 *   alive?: (pid: number) => boolean;
 *   release?: (input: { home: string; directories: string[] }) => unknown;
 * }} input
 * @returns {string[]} the released homes
 */
export function sweepAbandonedBridgeHomes({
  root,
  list = (path) => readdirSync(path),
  read = (path) => readFileSync(path, 'utf8'),
  alive = processAlive,
  release = releaseBridgeHome,
}) {
  /** @type {string[]} */
  const released = [];
  // HS2-41WX3Q: compare resolved paths. A raw string match drops every recorded run directory
  // when they and `root` differ only in separators (Windows) or normal form.
  const base = resolve(root);
  let names;
  try {
    names = list(root);
  } catch {
    return released;
  }
  for (const name of names) {
    if (!name.startsWith(BRIDGE_HOME_PREFIX)) continue;
    const home = join(root, name);
    let owner;
    try {
      owner = JSON.parse(read(join(home, BRIDGE_OWNER_FILE)));
    } catch {
      continue;
    }
    if (!Number.isInteger(owner?.pid) || owner.pid <= 0 || alive(owner.pid)) continue;
    const listed = Array.isArray(owner.directories) ? owner.directories : [];
    const directories = listed.filter(
      (directory) =>
        typeof directory === 'string' &&
        resolve(dirname(directory)) === base &&
        basename(directory).startsWith(RUN_DIRECTORY_PREFIX),
    );
    if (!directories.includes(home)) directories.push(home);
    release({ home, directories });
    released.push(home);
  }
  return released;
}
