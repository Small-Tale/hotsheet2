/**
 * Machine-wide browser slots (HS2-M6TZB5).
 *
 * Playwright's macOS WebKit draws in real offscreen windows and screenshots through WindowServer.
 * Several local WebKit browsers at once, across agent sessions, worktrees and repositories, have
 * exhausted the system IOSurface limit; WindowServer then aborted and logged out the session. Every
 * local WebKit launch therefore first holds one slot of a machine-scoped resource-claim pool
 * (docs/36-resource-claims.md): `browser-engines/webkit-1` … `webkit-N`, shared by every project on
 * the machine through `hotsheet-cli resource`.
 *
 * - Acquiring waits on every slot at once through the claim service's push-signalled wait (no
 *   polling); the first grant wins and the other waits are stopped, releasing any that also won.
 * - The holder renews its lease while the browser lives and releases it on close. A crashed holder
 *   stops renewing, so its short lease lapses and the slot frees itself.
 * - CI runs no pool (dedicated runners), and `HOTSHEET_BROWSER_SLOTS=off` disables it. When
 *   `hotsheet-cli` is not installed the pool degrades to a printed warning and the interim
 *   `--repeat-each` guard (HS2-CKHSM5) stays in force.
 *
 * Renewal and release pass the renewal token through the environment, never on a command line.
 */

import { spawn as nodeSpawn, spawnSync as nodeSpawnSync } from 'node:child_process';

export const BROWSER_SLOT_NAMESPACE = 'browser-engines';
/** WebKit browsers that may live at once on this machine unless `HOTSHEET_WEBKIT_SLOTS` says otherwise. */
export const DEFAULT_WEBKIT_SLOTS = 2;
/** Set by a runner that already holds slots for its workers' browsers (`--browser webkit`). */
export const SLOTS_HELD_ENV = 'HOTSHEET_BROWSER_SLOTS_HELD';
export const DEFAULT_LEASE_MS = 120_000;
export const DEFAULT_WAIT_MS = 30 * 60_000;

/**
 * Whether this process takes slots: `off` in CI or when disabled, `held` when the runner already
 * holds the slot this process's browser uses, otherwise `on`.
 *
 * @param {string} engine
 * @param {Record<string, string | undefined>} env
 * @returns {'on' | 'off' | 'held'}
 */
export function slotPolicy(engine, env) {
  if (env.CI || env.HOTSHEET_BROWSER_SLOTS === 'off') return 'off';
  if ((env[SLOTS_HELD_ENV] ?? '').split(',').includes(engine)) return 'held';
  return 'on';
}

/**
 * The pool's resource names for one engine.
 *
 * @param {string} engine
 * @param {Record<string, string | undefined>} env
 * @returns {string[]}
 */
export function slotNames(engine, env) {
  const configured = Number.parseInt(env[`HOTSHEET_${engine.toUpperCase()}_SLOTS`] ?? '', 10);
  const count = Number.isInteger(configured) && configured >= 1 && configured <= 16 ? configured : DEFAULT_WEBKIT_SLOTS;
  return Array.from({ length: count }, (_, index) => `${engine}-${index + 1}`);
}

/** @param {Record<string, string | undefined>} env */
export function claimCli(env) {
  return env.HOTSHEET_CLI_BIN || 'hotsheet-cli';
}

/** `90s`-style duration for the CLI, rounded up to whole seconds. */
const seconds = (ms) => `${Math.max(1, Math.ceil(ms / 1000))}s`;

/**
 * Parse the JSON object a `resource` command printed; null when there is none.
 *
 * @param {string} stdout
 */
export function parseClaimOutput(stdout) {
  const start = stdout.indexOf('{');
  if (start < 0) return null;
  try {
    return JSON.parse(stdout.slice(start));
  } catch {
    return null;
  }
}

/**
 * One line per holder, for a timeout that names who blocks the pool.
 *
 * @param {Array<{ resource?: string, claim_id?: string, expires_at?: string, metadata?: Record<string, string> }>} holders
 */
export function describeHolders(holders) {
  return holders
    .map((holder) => {
      const meta = Object.entries(holder.metadata ?? {})
        .map(([key, value]) => `${key}=${String(value).replace(/^plain:/, '')}`)
        .join(' ');
      return `${holder.resource ?? '?'} held by claim ${holder.claim_id ?? '?'} until ${holder.expires_at ?? '?'}${meta ? ` (${meta})` : ''}`;
    })
    .join('; ');
}

/**
 * Run one CLI command and collect its output.
 *
 * @returns {{ child: import('node:child_process').ChildProcess, done: Promise<{ code: number | null, stdout: string, stderr: string, error?: Error }> }}
 */
function runCli(spawn, cli, args, env) {
  const child = spawn(cli, args, { env, stdio: ['ignore', 'pipe', 'pipe'] });
  let stdout = '';
  let stderr = '';
  child.stdout?.on('data', (chunk) => (stdout += chunk));
  child.stderr?.on('data', (chunk) => (stderr += chunk));
  const done = new Promise((resolve) => {
    child.once('error', (error) => resolve({ code: null, stdout, stderr, error }));
    child.once('close', (code) => resolve({ code, stdout, stderr }));
  });
  return { child, done };
}

/** One bounded wait for any slot; at most this long, so a stopped waiter's registration lapses soon. */
export const WAIT_ROUND_MS = 5 * 60_000;

/**
 * Wait on every slot of the pool at once for up to `roundMs`. The first grant wins; the other
 * waits are stopped, and a grant that raced the winner is released.
 */
async function acquireRound({ engine, env, cli, spawn, leaseMs, purpose, roundMs }) {
  const attempts = slotNames(engine, env).map((resource) => {
    const run = runCli(
      spawn,
      cli,
      [
        'resource',
        'acquire',
        `${BROWSER_SLOT_NAMESPACE}/${resource}`,
        '--duration',
        seconds(leaseMs),
        '--wait',
        seconds(roundMs),
        '--meta',
        `pid=${process.pid}`,
        '--meta',
        `purpose=${purpose}`,
      ],
      env,
    );
    return { resource, ...run };
  });
  const winner = await new Promise((resolve) => {
    let pending = attempts.length;
    for (const attempt of attempts)
      void attempt.done.then((result) => {
        const parsed = parseClaimOutput(result.stdout);
        if (parsed?.acquired === true) resolve({ attempt, grant: parsed });
        else if (--pending === 0) resolve(null);
      });
  });
  for (const attempt of attempts) if (attempt !== winner?.attempt) attempt.child.kill('SIGTERM');
  const results = await Promise.all(attempts.map((attempt) => attempt.done));
  const raced = attempts
    .map((attempt, index) => (attempt === winner?.attempt ? null : parseClaimOutput(results[index].stdout)))
    .filter((grant) => grant?.acquired === true);
  await Promise.all(raced.map((grant) => releaseToken(spawn, cli, env, grant.renewal_token)));
  return { winner, results };
}

/** A handle for a slot that was not taken: CI, disabled, already held, or no CLI. */
const noSlot = (reason) => ({ resource: null, claimId: null, reason, release: async () => {}, releaseSync: () => {} });

/**
 * Hold one slot of the engine's pool until `release()`.
 *
 * @param {{
 *   engine: string,
 *   env?: Record<string, string | undefined>,
 *   waitMs?: number,
 *   leaseMs?: number,
 *   purpose?: string,
 *   spawn?: typeof nodeSpawn,
 *   warn?: (message: string) => void,
 *   now?: () => number,
 * }} options
 * @returns {Promise<BrowserSlot>}
 */
export async function acquireBrowserSlot({
  engine,
  env = process.env,
  waitMs = Number(env.HOTSHEET_BROWSER_SLOT_WAIT_MS) || DEFAULT_WAIT_MS,
  leaseMs = DEFAULT_LEASE_MS,
  purpose = 'playwright',
  spawn = nodeSpawn,
  warn = console.warn,
  now = Date.now,
}) {
  const policy = slotPolicy(engine, env);
  if (policy !== 'on') return noSlot(policy);
  const cli = claimCli(env);
  const started = now();
  // Each round waits on every slot at once, push-signalled. A stopped wait leaves its waiter
  // registration until its own deadline, so rounds are bounded to keep those short; a round that
  // ends without a grant starts the next until the whole wait is spent (not polling: each round
  // blocks on the claim service until a slot frees or the round ends).
  let winner;
  for (;;) {
    const remaining = waitMs - (now() - started);
    const round = await acquireRound({
      engine,
      env,
      cli,
      spawn,
      leaseMs,
      purpose,
      roundMs: Math.max(1000, Math.min(remaining, WAIT_ROUND_MS)),
    });
    if (round.winner) {
      winner = round.winner;
      break;
    }
    const { results } = round;
    if (results.every((result) => result.error?.code === 'ENOENT')) {
      warn(
        `${cli} is not installed, so no machine-wide ${engine} slot was taken (HS2-M6TZB5). Concurrent ` +
          `${engine} runs from other sessions can exhaust the macOS IOSurface limit.`,
      );
      return noSlot('no-cli');
    }
    const outcomes = results.map((result) => parseClaimOutput(result.stdout));
    if (outcomes.every((outcome) => outcome?.code === 'wait_timeout')) {
      if (now() - started < waitMs - 1000) continue;
      const holders = outcomes.map((outcome) => outcome?.holder).filter(Boolean);
      throw new Error(
        `No ${engine} browser slot came free within ${seconds(waitMs)} (HS2-M6TZB5): ${describeHolders(holders)}. ` +
          `Inspect the pool with \`${cli} resource list ${BROWSER_SLOT_NAMESPACE}\`.`,
      );
    }
    const detail = results
      .map((result) => result.error?.message ?? (parseClaimOutput(result.stdout)?.error || result.stderr.trim()))
      .filter(Boolean)
      .join('; ');
    throw new Error(`Could not take a ${engine} browser slot (HS2-M6TZB5): ${detail || 'unknown failure'}`);
  }

  const { resource } = winner.attempt;
  const token = winner.grant.renewal_token;
  // Renew at a third of the lease; a renewal that fails leaves the lease to lapse on its own.
  const timer = setInterval(() => {
    void runCli(spawn, cli, ['resource', 'renew', '--duration', seconds(leaseMs)], {
      ...env,
      HOTSHEET_RESOURCE_CLAIM_TOKEN: token,
    }).done;
  }, leaseMs / 3);
  timer.unref?.();
  let released;
  return {
    resource,
    claimId: winner.grant.claim_id,
    waitedMs: now() - started,
    release: () => {
      released ??= (async () => {
        clearInterval(timer);
        await releaseToken(spawn, cli, env, token);
      })();
      return released;
    },
    releaseSync: () => {
      if (released) return;
      released = Promise.resolve();
      clearInterval(timer);
      releaseBrowserSlotSync(token, env);
    },
  };
}

/**
 * @typedef {{ resource: string | null, claimId: string | null, reason?: string, waitedMs?: number,
 *   release: () => Promise<void>, releaseSync: () => void }} BrowserSlot
 */

/**
 * The `--workers` value a Playwright command passes, or null.
 *
 * @param {string[]} args
 */
function workersArg(args) {
  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    const value = arg === '--workers' || arg === '-j' ? args[index + 1] : /^(?:--workers|-j)=(.*)$/.exec(arg)?.[1];
    if (value !== undefined) return /^\d+$/.test(value) ? Number(value) : null;
  }
  return null;
}

/**
 * HS2-M6TZB5: a local `playwright test --browser webkit|all` launches one WebKit browser per worker
 * through Playwright's own fixtures, so the runner holds one slot per worker before any worker
 * starts: it waits for the first slot, takes any others that are free at once, and runs that many
 * workers. Workers inherit `HOTSHEET_BROWSER_SLOTS_HELD=webkit`, so their specs' own WebKit
 * launches do not take a second slot. Returns null when the run needs no slots (another engine,
 * `--list`, a worker process, CI, disabled, or no claim service).
 *
 * @param {{ args: string[] | null, env: Record<string, string | undefined>, workers?: number,
 *   acquire?: typeof acquireBrowserSlot, onExit?: (release: () => void) => void }} input
 * @returns {Promise<{ workers: number, slots: BrowserSlot[] } | null>}
 */
export async function holdRunnerWebkitSlots({
  args,
  env,
  workers,
  acquire = acquireBrowserSlot,
  onExit = (release) => process.once('exit', release),
}) {
  if (!args || env.TEST_WORKER_INDEX !== undefined || args.includes('--list')) return null;
  let browser = null;
  for (let index = 0; index < args.length; index += 1) {
    if (args[index] === '--browser') browser = args[index + 1] ?? null;
    else if (args[index].startsWith('--browser=')) browser = args[index].slice(10);
  }
  if (browser !== 'webkit' && browser !== 'all') return null;
  if (slotPolicy('webkit', env) !== 'on') return null;
  const requested = workersArg(args);
  const wanted = Math.max(1, Math.min(requested ?? workers ?? Infinity, slotNames('webkit', env).length));
  const first = await acquire({ engine: 'webkit', env, purpose: 'playwright-runner' });
  if (!first.resource) return null;
  const slots = [first];
  while (slots.length < wanted) {
    try {
      const next = await acquire({ engine: 'webkit', env, waitMs: 1000, purpose: 'playwright-runner' });
      if (!next.resource) break;
      slots.push(next);
    } catch {
      break;
    }
  }
  onExit(() => {
    for (const slot of slots) slot.releaseSync();
  });
  if (requested !== null && requested > slots.length) {
    for (const slot of slots) slot.releaseSync();
    throw new Error(
      `--workers ${requested} with local WebKit needs ${requested} machine-wide WebKit slots, but ${slots.length} ` +
        `were free (HS2-M6TZB5). Pass --workers ${slots.length} or fewer, or omit --workers.`,
    );
  }
  env[SLOTS_HELD_ENV] = [...new Set([...(env[SLOTS_HELD_ENV] ?? '').split(',').filter(Boolean), 'webkit'])].join(',');
  return { workers: slots.length, slots };
}

async function releaseToken(spawn, cli, env, token) {
  if (!token) return;
  await runCli(spawn, cli, ['resource', 'release'], { ...env, HOTSHEET_RESOURCE_CLAIM_TOKEN: token }).done;
}

/**
 * Release a token synchronously, for a process `exit` handler.
 *
 * @param {string} token
 * @param {Record<string, string | undefined>} [env]
 */
export function releaseBrowserSlotSync(token, env = process.env, spawnSync = nodeSpawnSync) {
  spawnSync(claimCli(env), ['resource', 'release'], {
    env: { ...env, HOTSHEET_RESOURCE_CLAIM_TOKEN: token },
    stdio: 'ignore',
  });
}

/**
 * Whether the claim service answers on this machine, for the runner's guard.
 *
 * @param {Record<string, string | undefined>} [env]
 */
export function browserSlotPoolAvailable(env = process.env, spawnSync = nodeSpawnSync) {
  const result = spawnSync(claimCli(env), ['resource', 'list', BROWSER_SLOT_NAMESPACE], {
    env,
    stdio: ['ignore', 'pipe', 'ignore'],
    encoding: 'utf8',
  });
  return result.status === 0 && parseClaimOutput(result.stdout ?? '')?.namespace === BROWSER_SLOT_NAMESPACE;
}
