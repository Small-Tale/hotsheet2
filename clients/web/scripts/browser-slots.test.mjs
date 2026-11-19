import { EventEmitter } from 'node:events';
import { readdirSync, readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

import { afterEach, describe, expect, it, vi } from 'vitest';

import {
  acquireBrowserSlot,
  BROWSER_SLOT_NAMESPACE,
  DEFAULT_WEBKIT_SLOTS,
  describeHolders,
  holdRunnerWebkitSlots,
  slotNames,
  slotPolicy,
  SLOTS_HELD_ENV,
} from './browser-slots.mjs';
import { webkitRepeatGuard } from './playwright-webkit-guard.mjs';

/**
 * A scripted `hotsheet-cli resource` backend behind a fake `spawn`, with the real JSON shapes:
 * an acquire grants a free slot at once, waits while it is held, and answers `wait_timeout` when
 * its wait ends (driven by `expire`). Synthetic data only.
 */
function fakeClaims() {
  const held = new Map();
  const waiting = [];
  const calls = [];
  let next = 1;
  const holder = (resource) => ({
    resource,
    claim_id: held.get(resource).claim,
    expires_at: '2026-11-19T00:10:00Z',
    metadata: { pid: 'plain:4242', purpose: 'plain:playwright' },
  });
  const finish = (child, code, body) => {
    queueMicrotask(() => {
      if (body) child.stdout.emit('data', JSON.stringify(body, null, 2));
      child.emit('close', code);
    });
  };
  const grant = (child, resource) => {
    const claim = `claim-${next++}`;
    held.set(resource, { claim, token: `token-${claim}` });
    finish(child, 0, {
      acquired: true,
      claim_id: claim,
      namespace: BROWSER_SLOT_NAMESPACE,
      resource,
      renewal_token: `token-${claim}`,
    });
  };
  const free = (resource) => {
    held.delete(resource);
    const index = waiting.findIndex((wait) => wait.resource === resource && !wait.done);
    if (index >= 0) {
      const [wait] = waiting.splice(index, 1);
      wait.done = true;
      grant(wait.child, resource);
    }
  };
  const spawn = vi.fn((cli, args, options) => {
    const child = new EventEmitter();
    child.stdout = new EventEmitter();
    child.stderr = new EventEmitter();
    calls.push({ cli, args, token: options.env.HOTSHEET_RESOURCE_CLAIM_TOKEN });
    child.kill = () => {
      const wait = waiting.find((entry) => entry.child === child);
      if (wait && !wait.done) {
        wait.done = true;
        waiting.splice(waiting.indexOf(wait), 1);
        queueMicrotask(() => child.emit('close', null));
      }
      return true;
    };
    const [, command] = args;
    if (command === 'acquire') {
      const resource = args[2].split('/')[1];
      const wait = args[args.indexOf('--wait') + 1];
      if (!held.has(resource)) grant(child, resource);
      // A one-second wait on a held slot ends at once here; longer waits end through `expire`.
      else if (wait === '1s')
        finish(child, 1, {
          acquired: false,
          code: 'wait_timeout',
          error: 'timed out waiting',
          holder: holder(resource),
        });
      else waiting.push({ resource, child, done: false });
    } else if (command === 'release' || command === 'renew') {
      const token = options.env.HOTSHEET_RESOURCE_CLAIM_TOKEN;
      const found = [...held.entries()].find(([, value]) => value.token === token);
      if (command === 'release' && found) free(found[0]);
      finish(child, 0, { released: true });
    } else finish(child, 0, {});
    return child;
  });
  return {
    spawn,
    calls,
    held,
    hold: (resource) => held.set(resource, { claim: `other-${resource}`, token: `other-${resource}` }),
    free,
    /** End every pending wait with `wait_timeout`, as the CLI does when `--wait` runs out. */
    expire: () => {
      for (const wait of waiting.splice(0)) {
        wait.done = true;
        finish(wait.child, 1, {
          acquired: false,
          code: 'wait_timeout',
          error: 'timed out waiting',
          holder: holder(wait.resource),
        });
      }
    },
    waiting: () => waiting.length,
  };
}

const env = (extra = {}) => ({ PATH: '/usr/bin', HOTSHEET_CLI_BIN: '/opt/fake/hotsheet-cli', ...extra });
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

afterEach(() => {
  vi.useRealTimers();
});

describe('browser slot pool (HS2-M6TZB5)', () => {
  it('names the pool per engine and takes no slot in CI, when disabled, or when the runner holds one', () => {
    expect(slotNames('webkit', {})).toEqual(['webkit-1', 'webkit-2']);
    expect(slotNames('webkit', {}).length).toBe(DEFAULT_WEBKIT_SLOTS);
    expect(slotNames('webkit', { HOTSHEET_WEBKIT_SLOTS: '3' })).toEqual(['webkit-1', 'webkit-2', 'webkit-3']);
    for (const bad of ['0', '17', 'two', ''])
      expect(slotNames('webkit', { HOTSHEET_WEBKIT_SLOTS: bad })).toHaveLength(2);
    expect(slotPolicy('webkit', {})).toBe('on');
    expect(slotPolicy('webkit', { CI: 'true' })).toBe('off');
    expect(slotPolicy('webkit', { HOTSHEET_BROWSER_SLOTS: 'off' })).toBe('off');
    expect(slotPolicy('webkit', { [SLOTS_HELD_ENV]: 'chromium,webkit' })).toBe('held');
    expect(slotPolicy('chromium', { [SLOTS_HELD_ENV]: 'webkit' })).toBe('on');
  });

  it('takes a free slot at once, stops the other waits, and releases with the token outside argv', async () => {
    const fake = fakeClaims();
    fake.hold('webkit-1');
    const slot = await acquireBrowserSlot({ engine: 'webkit', env: env(), spawn: fake.spawn });
    expect(slot.resource).toBe('webkit-2');
    await settle();
    expect(fake.waiting()).toBe(0);
    const acquire = fake.calls.find((call) => call.args[1] === 'acquire');
    expect(acquire.cli).toBe('/opt/fake/hotsheet-cli');
    expect(acquire.args).toEqual(
      expect.arrayContaining(['--duration', '120s', '--wait', '300s', '--meta', `pid=${process.pid}`]),
    );
    await slot.release();
    await slot.release();
    const releases = fake.calls.filter((call) => call.args[1] === 'release');
    expect(releases).toHaveLength(1);
    expect(releases[0].token).toMatch(/^token-claim-/);
    expect(fake.calls.every((call) => !call.args.join(' ').includes('token-'))).toBe(true);
    expect(fake.held.has('webkit-2')).toBe(false);
  });

  it('waits on every slot while all are held and takes the first one released', async () => {
    const fake = fakeClaims();
    fake.hold('webkit-1');
    fake.hold('webkit-2');
    let done = false;
    const pending = acquireBrowserSlot({ engine: 'webkit', env: env(), spawn: fake.spawn }).then((slot) => {
      done = true;
      return slot;
    });
    await settle();
    expect(done).toBe(false);
    expect(fake.waiting()).toBe(2);
    fake.free('webkit-2');
    const slot = await pending;
    expect(slot.resource).toBe('webkit-2');
    await settle();
    expect(fake.waiting()).toBe(0);
    // Empty, then refilled: the released slot is taken again by the next caller.
    await slot.release();
    const again = await acquireBrowserSlot({ engine: 'webkit', env: env(), spawn: fake.spawn });
    expect(again.resource).toBe('webkit-2');
  });

  it('waits in bounded rounds until a slot frees, and names the blocking holders when the wait runs out', async () => {
    let clock = 0;
    const now = () => clock;
    const fake = fakeClaims();
    fake.hold('webkit-1');
    fake.hold('webkit-2');
    // A 12-minute wait runs as 5-minute rounds: a round that ends without a grant starts the next.
    const rounds = acquireBrowserSlot({ engine: 'webkit', env: env(), waitMs: 12 * 60_000, spawn: fake.spawn, now });
    await settle();
    expect(fake.calls.filter((call) => call.args.includes('300s'))).toHaveLength(2);
    clock = 5 * 60_000;
    fake.expire();
    await settle();
    await settle();
    expect(fake.waiting()).toBe(2);
    fake.free('webkit-1');
    expect((await rounds).resource).toBe('webkit-1');
    fake.hold('webkit-1');

    clock = 0;
    const pending = acquireBrowserSlot({ engine: 'webkit', env: env(), waitMs: 5_000, spawn: fake.spawn, now });
    await settle();
    clock = 5_000;
    fake.expire();
    await expect(pending).rejects.toThrow(
      /No webkit browser slot came free within 5s .*webkit-1 held by claim other-webkit-1/,
    );
    expect(
      describeHolders([{ resource: 'webkit-1', claim_id: 'c', expires_at: 'x', metadata: { pid: 'plain:7' } }]),
    ).toBe('webkit-1 held by claim c until x (pid=7)');
  });

  it('releases a second grant that raced the first', async () => {
    const fake = fakeClaims();
    const slot = await acquireBrowserSlot({ engine: 'webkit', env: env(), spawn: fake.spawn });
    await settle();
    // Both slots were free, so both acquires were granted; only the winner stays held.
    expect([...fake.held.keys()]).toEqual([slot.resource]);
    await slot.release();
    expect(fake.held.size).toBe(0);
  });

  it('degrades to a warning when hotsheet-cli is not installed, and takes nothing in CI', async () => {
    const warn = vi.fn();
    const spawn = vi.fn(() => {
      const child = new EventEmitter();
      child.stdout = new EventEmitter();
      child.stderr = new EventEmitter();
      child.kill = () => true;
      queueMicrotask(() => {
        child.emit('error', Object.assign(new Error('spawn hotsheet-cli ENOENT'), { code: 'ENOENT' }));
        child.emit('close', null);
      });
      return child;
    });
    const slot = await acquireBrowserSlot({ engine: 'webkit', env: env(), spawn, warn });
    expect(slot).toMatchObject({ resource: null, reason: 'no-cli' });
    expect(warn.mock.calls[0][0]).toContain('is not installed');
    const ci = await acquireBrowserSlot({ engine: 'webkit', env: env({ CI: 'true' }), spawn, warn });
    expect(ci.reason).toBe('off');
    expect(spawn).toHaveBeenCalledTimes(DEFAULT_WEBKIT_SLOTS);
  });

  it('renews at a third of the lease while held and stops after release', async () => {
    vi.useFakeTimers();
    const fake = fakeClaims();
    const slot = await acquireBrowserSlot({ engine: 'webkit', env: env(), leaseMs: 30_000, spawn: fake.spawn });
    await vi.advanceTimersByTimeAsync(10_000);
    await vi.advanceTimersByTimeAsync(10_000);
    const renewals = () => fake.calls.filter((call) => call.args[1] === 'renew');
    expect(renewals()).toHaveLength(2);
    expect(renewals()[0].args).toEqual(['resource', 'renew', '--duration', '30s']);
    expect(renewals()[0].token).toMatch(/^token-claim-/);
    await slot.release();
    await vi.advanceTimersByTimeAsync(30_000);
    expect(renewals()).toHaveLength(2);
  });
});

describe('runner slots for --browser webkit (HS2-M6TZB5)', () => {
  const run = (args, extra = {}, fake = fakeClaims()) => {
    const environment = env(extra);
    const exits = [];
    const acquire = (options) => acquireBrowserSlot({ ...options, spawn: fake.spawn });
    return {
      fake,
      environment,
      exits,
      result: holdRunnerWebkitSlots({ args, env: environment, workers: 4, acquire, onExit: (fn) => exits.push(fn) }),
    };
  };

  it('holds no slot for other engines, --list, workers, or CI', async () => {
    expect(await run(['tests/a.spec.ts']).result).toBeNull();
    expect(await run(['--browser=chromium']).result).toBeNull();
    expect(await run(['--browser', 'webkit', '--list']).result).toBeNull();
    expect(await run(['--browser', 'webkit'], { TEST_WORKER_INDEX: '0' }).result).toBeNull();
    expect(await run(['--browser', 'webkit'], { CI: '1' }).result).toBeNull();
    expect(await run(null).result).toBeNull();
  });

  it('holds one slot per worker up to the pool, marks workers, and releases at exit', async () => {
    const { fake, environment, exits, result } = run(['--browser', 'webkit']);
    const held = await result;
    expect(held.workers).toBe(2);
    expect(environment[SLOTS_HELD_ENV]).toBe('webkit');
    expect(fake.held.size).toBe(2);
    expect(exits).toHaveLength(1);
  });

  it('runs with the free slots only and refuses an explicit --workers above them', async () => {
    const fake = fakeClaims();
    fake.hold('webkit-2');
    const one = run(['--browser=all'], {}, fake);
    expect((await one.result).workers).toBe(1);
    const refused = run(['--browser', 'webkit', '--workers', '2'], {}, fakeClaims());
    refused.fake.hold('webkit-2');
    await expect(refused.result).rejects.toThrow(/--workers 2 with local WebKit needs 2 .* Pass --workers 1 or fewer/);
  });
});

describe('WebKit guard with the slot pool (HS2-M6TZB5)', () => {
  const specs = { 'tests/terminal-safari.spec.ts': { launched: ['webkit'], optIn: [] } };
  const argv = ['node', 'cli.js', 'test', 'terminal-safari', '--repeat-each=4'];

  it('lets repeated WebKit wait for slots when the pool answers, and keeps the interim refusal otherwise', () => {
    const pooled = webkitRepeatGuard({ argv, env: {}, specs, pool: () => true });
    expect(pooled.refuse).toBeNull();
    expect(pooled.warn).toBeNull();
    expect(webkitRepeatGuard({ argv, env: {}, specs, pool: () => false }).refuse).toContain('Refusing --repeat-each');
    // Disabling the pool restores the interim guard even when the service answers.
    expect(
      webkitRepeatGuard({ argv, env: { HOTSHEET_BROWSER_SLOTS: 'off' }, specs, pool: () => true }).refuse,
    ).toContain('Refusing');
  });
});

describe('spec launches go through the slot pool (HS2-M6TZB5)', () => {
  it('never calls a Playwright BrowserType launch directly in a spec that imports webkit', () => {
    const directory = fileURLToPath(new URL('../tests/', import.meta.url));
    const offenders = [];
    let slotted = 0;
    for (const file of readdirSync(directory).filter((name) => name.endsWith('.spec.ts'))) {
      const text = readFileSync(`${directory}${file}`, 'utf8');
      if (!/import\s*\{[^}]*\bwebkit\b[^}]*\}\s*from\s*'@playwright\/test'/.test(text)) continue;
      for (const match of text.matchAll(/\b(?!app\b)([A-Za-z_$][\w$]*)\.launch\(/g))
        offenders.push(`${file}: ${match[0]}`);
      slotted += [...text.matchAll(/slotted\(\w+\)\.launch\(/g)].length;
    }
    expect(offenders).toEqual([]);
    expect(slotted).toBeGreaterThanOrEqual(5);
  });
});
