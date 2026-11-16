import { spawnSync } from 'node:child_process';
import { existsSync, rmSync } from 'node:fs';
import { join, resolve } from 'node:path';

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
  remove = (path) => rmSync(path, { recursive: true, force: true }),
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
  for (const directory of directories) remove(directory);
  return outcome;
}
