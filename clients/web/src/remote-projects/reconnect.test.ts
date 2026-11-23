import { describe, expect, it, vi } from 'vitest';

import type { Project } from '../interactions/types';
import { remoteProjectId } from './identity';
import {
  createRemoteReconnect,
  REMOTE_TAB_RANK_BASE,
  remotePlaceholderProjectId,
  remotePlaceholderTabId,
  remoteReconnectDelay,
  remoteTabPlaceholders,
  wireRemoteReconnectWakeups,
} from './reconnect';

const HOST_A = '01ARZ3NDEKTSV4RRFFQ69G5FAV';
const HOST_B = '01ARZ3NDEKTSV4RRFFQ69G5FAW';

/** A manual clock: timers fire only when the test advances time, never on their own. */
function manualTimers() {
  let now = 0,
    next = 1;
  const pending = new Map<number, { at: number; run: () => void }>();
  return {
    timers: {
      now: () => now,
      setTimeout: (run: () => void, ms: number) => {
        const handle = next++;
        pending.set(handle, { at: now + ms, run });
        return handle;
      },
      clearTimeout: (handle: unknown) => {
        pending.delete(handle as number);
      },
    },
    /** Delays of the armed timers, from now. */
    armed: () => [...pending.values()].map((item) => item.at - now),
    async advance(ms: number) {
      now += ms;
      for (const [handle, item] of [...pending].sort((left, right) => left[1].at - right[1].at))
        if (item.at <= now) {
          pending.delete(handle);
          item.run();
        }
      await settle();
    },
  };
}

async function settle() {
  for (let index = 0; index < 10; index += 1) await Promise.resolve();
}

function deferred<T>() {
  let resolve!: (value: T) => void, reject!: (reason: unknown) => void;
  const promise = new Promise<T>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
}

const project = (bindingId: string, checkoutId: string) =>
  ({ id: remoteProjectId(bindingId, checkoutId), name: checkoutId }) as unknown as Project;

/** A fake host registry: each host is up or down; opens and probes are recorded. */
function fakeHosts(up: Record<string, boolean>) {
  const calls: string[] = [];
  return {
    up,
    calls,
    open: vi.fn(async (bindingId: string, checkoutId: string) => {
      calls.push(`open ${bindingId.slice(-1)} ${checkoutId}`);
      await Promise.resolve();
      if (!up[bindingId]) throw new Error('Could not reach or authenticate the approved remote server.');
      return project(bindingId, checkoutId);
    }),
    probe: vi.fn(async (bindingId: string) => {
      calls.push(`probe ${bindingId.slice(-1)}`);
      await Promise.resolve();
      return up[bindingId];
    }),
  };
}

function setup(up: Record<string, boolean>, wakeMinIntervalMs = 10_000) {
  const clock = manualTimers(),
    hosts = fakeHosts(up),
    opened: string[] = [],
    discarded: string[] = [];
  const reconnect = createRemoteReconnect({
    open: hosts.open,
    probe: hosts.probe,
    opened: async (placeholder, value) => {
      await Promise.resolve();
      opened.push(`${placeholder.rank}:${value.id}`);
    },
    discard: (placeholder) => {
      discarded.push(placeholder.id);
    },
    timers: clock.timers,
    baseDelayMs: 2_000,
    maxDelayMs: 60_000,
    wakeMinIntervalMs,
  });
  const ids = () => reconnect.placeholders.value.map((item) => item.checkoutId);
  return { clock, hosts, opened, discarded, reconnect, ids };
}

const seeds = (pairs: Array<[string, string]>) =>
  remoteTabPlaceholders(
    pairs.map(([binding_id, checkout_id]) => ({ binding_id, checkout_id })),
    [
      { id: HOST_A, label: 'Studio', last_confirmed_checkouts: [{ id: 'alpha', name: 'Alpha' }] },
      { id: HOST_B, label: 'Laptop', last_confirmed_checkouts: [] },
    ],
  );

describe('HS2-EJ5K09 remote tab placeholders', () => {
  it('names placeholders from the registry alone and keeps the saved order', () => {
    const items = seeds([
      [HOST_A, 'alpha'],
      [HOST_B, 'beta'],
      [HOST_A, 'alpha'],
      ['not-a-binding', 'gamma'],
    ]);
    expect(items.map((item) => [item.name, item.rank])).toEqual([
      ['Studio · Alpha', REMOTE_TAB_RANK_BASE],
      ['Laptop · beta', REMOTE_TAB_RANK_BASE + 1],
    ]);
    expect(remotePlaceholderProjectId(remotePlaceholderTabId(items[0].id))).toBe(items[0].id);
    expect(remotePlaceholderProjectId(items[0].id)).toBeUndefined();
  });

  it('shows every placeholder before any open resolves and opens hosts in parallel', async () => {
    const gates = new Map<string, ReturnType<typeof deferred<Project>>>();
    const opened: string[] = [];
    const reconnect = createRemoteReconnect({
      open: (bindingId, checkoutId) => {
        const gate = deferred<Project>();
        gates.set(checkoutId, gate);
        return gate.promise;
      },
      probe: () => Promise.resolve(true),
      opened: async (placeholder) => {
        opened.push(placeholder.checkoutId);
        await Promise.resolve();
      },
      discard: () => undefined,
      timers: manualTimers().timers,
    });
    const restoring = reconnect.restore(
      seeds([
        [HOST_A, 'alpha'],
        [HOST_B, 'beta'],
      ]),
    );
    // Both tabs exist and both opens started before either resolved.
    expect(reconnect.placeholders.value.map((item) => [item.checkoutId, item.phase, item.connecting])).toEqual([
      ['alpha', 'loading', true],
      ['beta', 'loading', true],
    ]);
    expect([...gates.keys()]).toEqual(['alpha', 'beta']);
    // A slow host does not hold back the other one.
    gates.get('beta')!.resolve(project(HOST_B, 'beta'));
    await settle();
    expect(opened).toEqual(['beta']);
    expect(reconnect.placeholders.value.map((item) => item.checkoutId)).toEqual(['alpha']);
    gates.get('alpha')!.resolve(project(HOST_A, 'alpha'));
    await restoring;
    expect(opened).toEqual(['beta', 'alpha']);
    expect(reconnect.placeholders.value).toEqual([]);
  });
});

describe('HS2-WNK73S remote reconnect state machine', () => {
  it('backs off exponentially to a cap', () => {
    expect([1, 2, 3, 4, 5, 6, 40].map((failures) => remoteReconnectDelay(failures, 2_000, 60_000))).toEqual([
      2_000, 4_000, 8_000, 16_000, 32_000, 60_000, 60_000,
    ]);
  });

  it('keeps an unreachable host as a reconnecting tab, then opens it once the host answers', async () => {
    const { clock, hosts, opened, reconnect, ids } = setup({ [HOST_A]: false });
    await reconnect.restore(seeds([[HOST_A, 'alpha']]));
    expect(reconnect.placeholders.value[0]).toMatchObject({ phase: 'reconnecting', connecting: false });
    expect(reconnect.placeholders.value[0].error).toMatch(/approved remote server/);
    expect(clock.armed()).toEqual([2_000]);
    await clock.advance(2_000);
    // A retry probes the host once instead of reopening every checkout.
    expect(hosts.calls).toEqual(['open V alpha', 'probe V']);
    expect(clock.armed()).toEqual([4_000]);
    hosts.up[HOST_A] = true;
    await clock.advance(4_000);
    expect(hosts.calls.slice(2)).toEqual(['probe V', 'open V alpha']);
    expect(opened).toEqual([`${REMOTE_TAB_RANK_BASE}:${remoteProjectId(HOST_A, 'alpha')}`]);
    expect(ids()).toEqual([]);
    expect(clock.armed()).toEqual([]);
  });

  it('keeps the idle request rate at one probe per host per capped interval', async () => {
    const { clock, hosts, reconnect } = setup({ [HOST_A]: false });
    await reconnect.restore(
      seeds([
        [HOST_A, 'alpha'],
        [HOST_A, 'beta'],
        [HOST_A, 'gamma'],
      ]),
    );
    hosts.calls.length = 0;
    // An hour of an unreachable host with three selected projects.
    for (let elapsed = 0; elapsed < 3_600_000; elapsed += 1_000) await clock.advance(1_000);
    const probes = hosts.calls.filter((call) => call.startsWith('probe')).length;
    expect(hosts.calls.every((call) => call.startsWith('probe'))).toBe(true);
    // 2+4+8+16+32 s ramp, then one probe per 60 s cap: never a per-second or per-project loop.
    expect(probes).toBeGreaterThan(55);
    expect(probes).toBeLessThanOrEqual(65);
  });

  it('survives a flapping host and resets the backoff after a success', async () => {
    const { clock, hosts, opened, reconnect } = setup({ [HOST_A]: false });
    await reconnect.restore(seeds([[HOST_A, 'alpha']]));
    // Up for the probe, down again for the open.
    hosts.probe.mockResolvedValueOnce(true);
    await clock.advance(2_000);
    expect(reconnect.placeholders.value).toHaveLength(1);
    expect(clock.armed()).toEqual([4_000]);
    hosts.up[HOST_A] = true;
    await clock.advance(4_000);
    expect(opened).toHaveLength(1);
    // A later selection of the same host starts from the base delay again.
    hosts.up[HOST_A] = false;
    await reconnect.restore(seeds([[HOST_A, 'beta']]));
    expect(clock.armed()).toEqual([2_000]);
  });

  it('retries at once on a tab click during backoff without stacking attempts', async () => {
    const { clock, hosts, opened, reconnect } = setup({ [HOST_A]: false });
    await reconnect.restore(seeds([[HOST_A, 'alpha']]));
    await clock.advance(2_000);
    expect(clock.armed()).toEqual([4_000]);
    hosts.up[HOST_A] = true;
    const id = remoteProjectId(HOST_A, 'alpha');
    const first = reconnect.retryNow(id),
      second = reconnect.retryNow(id);
    expect(reconnect.placeholders.value[0].connecting).toBe(true);
    await Promise.all([first, second]);
    expect(hosts.probe).toHaveBeenCalledTimes(2);
    expect(opened).toHaveLength(1);
    expect(clock.armed()).toEqual([]);
  });

  it('wakes on focus or network-online, at most once per wake interval', async () => {
    const clock = manualTimers(),
      hosts = fakeHosts({ [HOST_A]: false });
    const reconnect = createRemoteReconnect({
      open: hosts.open,
      probe: hosts.probe,
      opened: () => Promise.resolve(),
      discard: () => undefined,
      timers: clock.timers,
      baseDelayMs: 600_000,
      maxDelayMs: 600_000,
      wakeMinIntervalMs: 10_000,
    });
    await reconnect.restore(seeds([[HOST_A, 'alpha']]));
    hosts.calls.length = 0;
    const target = new EventTarget() as unknown as Window,
      stop = wireRemoteReconnectWakeups(reconnect, target);
    // Just tried: a focus does not retry yet.
    target.dispatchEvent(new Event('focus'));
    await settle();
    expect(hosts.calls).toEqual([]);
    await clock.advance(10_000);
    // A burst of wakes makes one probe.
    target.dispatchEvent(new Event('online'));
    target.dispatchEvent(new Event('focus'));
    await settle();
    expect(hosts.calls).toEqual(['probe V']);
    // The wake re-armed the backoff from that attempt instead of adding a second timer.
    expect(clock.armed()).toEqual([600_000]);
    stop();
    await clock.advance(20_000);
    target.dispatchEvent(new Event('focus'));
    await settle();
    expect(hosts.calls).toEqual(['probe V']);
  });

  it('forgets a placeholder while its open is pending and undoes the late re-selection', async () => {
    const gate = deferred<Project>();
    const opened: string[] = [],
      discarded: string[] = [];
    const clock = manualTimers();
    const reconnect = createRemoteReconnect({
      open: () => gate.promise,
      probe: () => Promise.resolve(true),
      opened: async (placeholder) => {
        opened.push(placeholder.id);
        await Promise.resolve();
      },
      discard: (placeholder) => {
        discarded.push(placeholder.id);
      },
      timers: clock.timers,
    });
    const restoring = reconnect.restore(seeds([[HOST_A, 'alpha']]));
    const id = remoteProjectId(HOST_A, 'alpha');
    expect(reconnect.forget(id)?.id).toBe(id);
    expect(reconnect.placeholders.value).toEqual([]);
    gate.resolve(project(HOST_A, 'alpha'));
    await restoring;
    expect(opened).toEqual([]);
    expect(discarded).toEqual([id]);
    expect(clock.armed()).toEqual([]);
    expect(reconnect.forget(id)).toBeUndefined();
  });

  it('forgets a reconnecting placeholder and stops its host timer', async () => {
    const { clock, reconnect } = setup({ [HOST_A]: false });
    await reconnect.restore(seeds([[HOST_A, 'alpha']]));
    expect(reconnect.scheduled(HOST_A)).toBe(true);
    reconnect.forget(remoteProjectId(HOST_A, 'alpha'));
    expect(reconnect.scheduled(HOST_A)).toBe(false);
    expect(clock.armed()).toEqual([]);
  });

  it('opens the reachable hosts and keeps only the down host reconnecting', async () => {
    const { clock, hosts, opened, reconnect, ids } = setup({ [HOST_A]: true, [HOST_B]: false });
    await reconnect.restore(
      seeds([
        [HOST_B, 'beta'],
        [HOST_A, 'alpha'],
        [HOST_A, 'delta'],
      ]),
    );
    expect(opened.map((item) => item.split(':')[0])).toEqual([
      String(REMOTE_TAB_RANK_BASE + 1),
      String(REMOTE_TAB_RANK_BASE + 2),
    ]);
    expect(ids()).toEqual(['beta']);
    expect(reconnect.scheduled(HOST_A)).toBe(false);
    expect(reconnect.scheduled(HOST_B)).toBe(true);
    hosts.up[HOST_B] = true;
    await clock.advance(2_000);
    expect(ids()).toEqual([]);
  });

  it('keeps a placeholder when wiring its tab fails, and treats an already open project as done', async () => {
    const clock = manualTimers();
    let fail = true;
    const open = new Set<string>();
    const reconnect = createRemoteReconnect({
      open: (bindingId, checkoutId) => Promise.resolve(project(bindingId, checkoutId)),
      probe: () => Promise.resolve(true),
      opened: () => (fail ? Promise.reject(new Error('wire failed')) : Promise.resolve()),
      discard: () => undefined,
      isOpen: (id) => open.has(id),
      timers: clock.timers,
    });
    await reconnect.restore(seeds([[HOST_A, 'alpha']]));
    expect(reconnect.placeholders.value[0]).toMatchObject({ phase: 'reconnecting', error: 'wire failed' });
    // The user opened it from the Remote project dialog meanwhile.
    open.add(remoteProjectId(HOST_A, 'alpha'));
    fail = false;
    await clock.advance(2_000);
    expect(reconnect.placeholders.value).toEqual([]);
    // A restore of an already open project adds nothing.
    await reconnect.restore(seeds([[HOST_A, 'alpha']]));
    expect(reconnect.placeholders.value).toEqual([]);
  });

  it('stops every timer on dispose', async () => {
    const { clock, reconnect } = setup({ [HOST_A]: false, [HOST_B]: false });
    await reconnect.restore(
      seeds([
        [HOST_A, 'alpha'],
        [HOST_B, 'beta'],
      ]),
    );
    expect(clock.armed()).toHaveLength(2);
    reconnect.dispose();
    expect(clock.armed()).toEqual([]);
    await reconnect.restore(seeds([[HOST_A, 'gamma']]));
    expect(reconnect.placeholders.value.map((item) => item.checkoutId)).toEqual(['alpha', 'beta']);
  });
});

describe('HS2-4B0KSD expired and revoked approvals', () => {
  /** A host whose probe answers with a classified failure. */
  function classified(failure: string | undefined) {
    const result = setup({ [HOST_A]: false });
    result.hosts.probe.mockImplementation(async (bindingId: string) => {
      result.hosts.calls.push(`probe ${bindingId.slice(-1)}`);
      await Promise.resolve();
      return failure === undefined ? false : ({ reachable: false, failure } as unknown as boolean);
    });
    return result;
  }

  for (const [failure, phase] of [
    ['session_expired', 'expired'],
    ['session_revoked', 'revoked'],
  ] as const) {
    it(`stops retrying once the host reports ${failure}`, async () => {
      const { clock, hosts, reconnect } = classified(failure);
      await reconnect.restore(seeds([[HOST_A, 'alpha']]));
      // The first open failed; the first retry probes and learns the approval is gone.
      await clock.advance(2_000);
      expect(reconnect.placeholders.value[0]).toMatchObject({ phase, connecting: false });
      expect(reconnect.placeholders.value[0]?.error).not.toBe('The server did not answer.');
      expect(reconnect.scheduled(HOST_A)).toBe(false);
      const calls = hosts.calls.length;
      // No timer, focus or network wake retries an expired credential.
      await clock.advance(60 * 60_000);
      reconnect.wake();
      await settle();
      expect(hosts.calls.length).toBe(calls);
      // An explicit Check Again probes once, and a renewed approval opens the tab.
      reconnect.renewalRequested(HOST_A);
      expect(reconnect.placeholders.value[0]?.renewalRequested).toBe(true);
      hosts.up[HOST_A] = true;
      hosts.probe.mockImplementation(async () => true);
      await reconnect.retryNow(reconnect.placeholders.value[0]?.id ?? '');
      expect(reconnect.placeholders.value).toEqual([]);
    });
  }

  it('keeps the backoff for an unreachable host and for other failures, with their own text', async () => {
    for (const [failure, text] of [
      [undefined, 'The server did not answer.'],
      ['trust_failed', 'The remote server’s certificate is not trusted.'],
    ] as const) {
      const { clock, reconnect } = classified(failure);
      await reconnect.restore(seeds([[HOST_A, 'alpha']]));
      await clock.advance(2_000);
      expect(reconnect.placeholders.value[0]).toMatchObject({ phase: 'reconnecting', error: text });
      expect(reconnect.scheduled(HOST_A)).toBe(true);
      reconnect.dispose();
    }
  });
});
