/**
 * Guard against unbounded concurrent local WebKit (HS2-CKHSM5, HS2-M6TZB5).
 *
 * Playwright's macOS WebKit draws in real offscreen windows and screenshots through WindowServer,
 * so several local WebKit browsers at once can exhaust the system IOSurface limit; WindowServer then
 * aborts and logs out the session. Every local WebKit browser holds a slot of the machine-wide
 * `browser-engines` resource-claim pool (`scripts/browser-slots.mjs`), so repeated WebKit runs wait
 * for a slot instead of stacking up. When that pool is unavailable (no `hotsheet-cli`), the interim
 * guard stays: a local run refuses `--repeat-each` with WebKit unless
 * HOTSHEET_ALLOW_LOCAL_WEBKIT_REPEAT=1, and any local WebKit run warns that nothing limits WebKit
 * across sessions. CI is unaffected. Pure: the caller supplies argv, env, specs and pool state.
 */

import { fileURLToPath } from 'node:url';

import { specIndex } from '../../../scripts/browser-engines.mjs';
import { browserSlotPoolAvailable, slotPolicy } from './browser-slots.mjs';

export const WEBKIT_REPEAT_OVERRIDE = 'HOTSHEET_ALLOW_LOCAL_WEBKIT_REPEAT';

/**
 * The runner's `playwright test` arguments, or null when argv is not a Playwright test run.
 *
 * @param {string[]} argv
 * @returns {string[] | null}
 */
export function playwrightTestArgs(argv) {
  const at = argv.indexOf('test');
  return at < 0 ? null : argv.slice(at + 1);
}

/**
 * @param {string} pattern
 * @returns {(path: string) => boolean}
 */
function fileFilter(pattern) {
  try {
    const regex = new RegExp(pattern);
    return (path) => regex.test(path) || path.includes(pattern);
  } catch {
    return (path) => path.includes(pattern);
  }
}

/**
 * What a local run asks of WebKit.
 *
 * @param {{ args: string[], env: Record<string, string | undefined>,
 *   specs: Record<string, { launched: string[], optIn: string[] }> }} input
 * @returns {{ repeatEach: number, list: boolean, webkit: string[] }} `webkit` names the reasons
 *   WebKit runs: `--browser` or each selected spec that launches it.
 */
export function localWebkitUse({ args, env, specs }) {
  let repeatEach = 1;
  let browser = null;
  let list = false;
  const filters = [];
  const valued = new Set([
    '--grep',
    '-g',
    '--grep-invert',
    '--workers',
    '-j',
    '--reporter',
    '--timeout',
    '--retries',
    '--project',
    '--config',
    '-c',
    '--output',
    '--max-failures',
    '--shard',
    '--global-timeout',
    '--trace',
    '--update-snapshots',
    '-u',
    '--tsconfig',
    '--ui-host',
    '--ui-port',
  ]);
  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    if (arg === '--repeat-each') repeatEach = Number.parseInt(args[++index] ?? '1', 10) || 1;
    else if (arg.startsWith('--repeat-each=')) repeatEach = Number.parseInt(arg.slice(14), 10) || 1;
    else if (arg === '--browser') browser = args[++index] ?? null;
    else if (arg.startsWith('--browser=')) browser = arg.slice(10);
    else if (arg === '--list') list = true;
    else if (valued.has(arg)) index += 1;
    else if (!arg.startsWith('-')) filters.push(fileFilter(arg));
  }
  const webkit = [];
  if (browser === 'webkit' || browser === 'all') webkit.push(`--browser ${browser}`);
  for (const [path, spec] of Object.entries(specs).sort()) {
    if (!spec.launched.includes('webkit')) continue;
    if (filters.length && !filters.some((matches) => matches(path))) continue;
    // An opt-in spec skips itself unless its environment flag is set.
    if (spec.optIn.length && !spec.optIn.some((flag) => env[flag] === '1')) continue;
    webkit.push(path);
  }
  return { repeatEach, list, webkit };
}

/**
 * The guard's decision for one local runner start. `refuse` is an error message; `warn` is
 * printed when the run uses local WebKit without the slot pool. `webkit` names why WebKit runs.
 *
 * @param {{ argv: string[], env: Record<string, string | undefined>,
 *   specs: Record<string, { launched: string[], optIn: string[] }>, pool?: () => boolean }} input
 * @returns {{ refuse: string | null, warn: string | null, webkit?: string[] }}
 */
export function webkitRepeatGuard({ argv, env, specs, pool = () => false }) {
  const none = { refuse: null, warn: null };
  // CI covers WebKit on dedicated runners; a worker re-loads the config and must not repeat this.
  if (env.CI || env.TEST_WORKER_INDEX !== undefined) return none;
  const args = playwrightTestArgs(argv);
  if (!args) return none;
  const use = localWebkitUse({ args, env, specs });
  if (use.list || use.webkit.length === 0) return none;
  // HS2-M6TZB5: with the machine-wide slot pool every WebKit browser waits for a slot.
  if (slotPolicy('webkit', env) === 'on' && pool()) return { refuse: null, warn: null, webkit: use.webkit };
  const reasons = use.webkit.join(', ');
  const warn =
    `Local WebKit (${reasons}): no machine-wide browser slot pool limits WebKit across sessions ` +
    '(HS2-M6TZB5; hotsheet-cli resource is unavailable or HOTSHEET_BROWSER_SLOTS=off). Concurrent WebKit ' +
    'runs from other agents can exhaust the macOS IOSurface limit.';
  if (use.repeatEach <= 1 || env[WEBKIT_REPEAT_OVERRIDE] === '1') return { refuse: null, warn };
  return {
    refuse:
      `Refusing --repeat-each ${use.repeatEach} with local WebKit (${reasons}) (HS2-CKHSM5). Repeated ` +
      'WebKit runs alongside other sessions have aborted WindowServer and logged out the macOS session. ' +
      `Repeat with Chromium, run WebKit once, or set ${WEBKIT_REPEAT_OVERRIDE}=1 when no other session runs WebKit.`,
    warn,
  };
}

/**
 * The guard for this runner process, over the package's real specs.
 *
 * @param {string[]} [argv]
 * @param {Record<string, string | undefined>} [env]
 */
export function runnerWebkitGuard(argv = process.argv, env = process.env, pool = () => browserSlotPoolAvailable(env)) {
  return webkitRepeatGuard({ argv, env, pool, specs: specIndex(fileURLToPath(new URL('..', import.meta.url))) });
}
