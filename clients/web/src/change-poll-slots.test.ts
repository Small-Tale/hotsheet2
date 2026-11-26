import { afterEach, describe, expect, it, vi } from 'vitest';

import { DirectPollSlots, type SlotContentionChannel, type SlotGrant } from './change-poll-slots';

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

/** An origin-wide fake of `navigator.locks`: exclusive names, `ifAvailable`, abort, and `query`. */
function fakeLocks() {
  const occupied = new Set<string>();
  const queued = new Map<string, Array<{ start(): void; signal?: AbortSignal }>>();
  const pendingNames = () =>
    [...queued.entries()].flatMap(([name, waiters]) => waiters.map(() => ({ name, mode: 'exclusive' })));
  const request = <T>(
    name: string,
    options: { ifAvailable?: boolean; signal?: AbortSignal },
    callback: (lock: { name: string } | null) => Promise<T> | T,
  ): Promise<T> =>
    new Promise((resolve, reject) => {
      const run = () => {
        occupied.add(name);
        void Promise.resolve()
          .then(() => callback({ name }))
          .then(resolve, reject)
          .finally(() => {
            occupied.delete(name);
            const next = queued.get(name)?.shift();
            if (queued.get(name)?.length === 0) queued.delete(name);
            next?.start();
          });
      };
      if (!occupied.has(name)) {
        run();
        return;
      }
      if (options.ifAvailable) {
        void Promise.resolve(callback(null)).then(resolve, reject);
        return;
      }
      const waiter = { start: run, ...(options.signal ? { signal: options.signal } : {}) };
      queued.set(name, [...(queued.get(name) ?? []), waiter]);
      options.signal?.addEventListener(
        'abort',
        () => {
          const rest = (queued.get(name) ?? []).filter((item) => item !== waiter);
          if (rest.length) queued.set(name, rest);
          else queued.delete(name);
          reject(new DOMException('aborted', 'AbortError'));
        },
        { once: true },
      );
    });
  const query = () => Promise.resolve({ held: [...occupied].map((name) => ({ name })), pending: pendingNames() });
  return { occupied, queued, locks: { request, query } };
}

/** An in-memory contention channel shared by the slots of several fake contexts. */
function fakeChannelHub() {
  const listeners = new Set<{ owner: number; listener: () => void }>();
  let next = 0;
  return () => {
    const owner = (next += 1);
    const channel: SlotContentionChannel = {
      announce: () => {
        for (const item of listeners) if (item.owner !== owner) item.listener();
      },
      listen: (listener) => {
        const item = { owner, listener };
        listeners.add(item);
        return () => listeners.delete(item);
      },
    };
    return channel;
  };
}

const flush = async () => {
  for (let index = 0; index < 8; index += 1) await Promise.resolve();
};

describe('direct change-poll slots', () => {
  it('bounds two independent contexts and refills slots after a poll ends', async () => {
    const fake = fakeLocks();
    vi.stubGlobal('navigator', { locks: fake.locks });
    const first = new DirectPollSlots({ channel: null });
    const second = new DirectPollSlots({ channel: null });
    const finish: Array<() => void> = [];
    const poll = vi.fn(
      (grant: SlotGrant) =>
        new Promise<SlotGrant>((resolve) =>
          finish.push(() => {
            resolve(grant);
          }),
        ),
    );
    const controller = new AbortController();
    const results = [first, second].flatMap((slots) =>
      Array.from({ length: 4 }, () => slots.run(controller.signal, poll)),
    );
    await flush();
    expect(poll).toHaveBeenCalledTimes(4);
    expect(fake.occupied.size).toBe(4);
    finish.shift()?.();
    await flush();
    expect(poll).toHaveBeenCalledTimes(5);
    while (finish.length) {
      finish.shift()?.();
      await flush();
    }
    const grants = await Promise.all(results);
    expect(grants).toHaveLength(8);
    // The first four were granted while nobody else waited; later grants saw the queue.
    expect(grants.slice(0, 4).every((grant) => !grant.contended)).toBe(true);
    expect(grants.every((grant) => grant.maxTimeoutMs === undefined)).toBe(true);
    expect(fake.occupied.size).toBe(0);
  });

  // HS2-EZN3NY: a second context takes a free slot instead of queueing behind a held one.
  it('grants any free slot before waiting, uncontended, so two contexts hold four polls at once', async () => {
    const fake = fakeLocks();
    vi.stubGlobal('navigator', { locks: fake.locks });
    const grants: SlotGrant[] = [];
    const poll = (grant: SlotGrant) => {
      grants.push(grant);
      return new Promise<never>(() => undefined);
    };
    const signal = new AbortController().signal;
    for (const slots of [new DirectPollSlots({ channel: null }), new DirectPollSlots({ channel: null })]) {
      void slots.run(signal, poll);
      void slots.run(signal, poll);
    }
    await flush();
    expect(fake.occupied.size).toBe(4);
    expect(fake.queued.size).toBe(0);
    expect(grants).toEqual(Array(4).fill({ contended: false }));
  });

  it('announces a wait to other contexts and reports the contention to the next holder', async () => {
    const fake = fakeLocks();
    vi.stubGlobal('navigator', { locks: fake.locks });
    const channelFor = fakeChannelHub();
    const holder = new DirectPollSlots({ channel: channelFor() });
    const waiter = new DirectPollSlots({ channel: channelFor() });
    const contention = vi.fn();
    holder.onContention(contention);
    const finish: Array<() => void> = [];
    const grants: SlotGrant[] = [];
    const poll = (grant: SlotGrant) =>
      new Promise<void>((resolve) => {
        grants.push(grant);
        finish.push(resolve);
      });
    const signal = new AbortController().signal;
    for (let index = 0; index < 4; index += 1) void holder.run(signal, poll);
    await flush();
    expect(contention).not.toHaveBeenCalled();
    const waiting = waiter.run(signal, poll);
    await flush();
    expect(contention).toHaveBeenCalledTimes(1);
    expect(fake.queued.size).toBe(1);
    // A holder that is reissued while the waiter queues sees the contention.
    const reissue = holder.run(signal, poll);
    await flush();
    finish.shift()?.();
    await flush();
    expect(grants.at(-1)).toEqual({ contended: true });
    for (const done of finish.splice(0)) done();
    await flush();
    for (const done of finish.splice(0)) done();
    await Promise.all([waiting, reissue]);
  });

  it('lets a queued slot wait abort without running its poll', async () => {
    const fake = fakeLocks();
    vi.stubGlobal('navigator', { locks: fake.locks });
    const slots = new DirectPollSlots({ channel: null });
    const signal = new AbortController().signal;
    const held = () => new Promise<never>(() => undefined);
    for (let index = 0; index < 4; index += 1) void slots.run(signal, held);
    await flush();
    const canceled = new AbortController();
    const poll = vi.fn(held);
    const queued = slots.run(canceled.signal, poll);
    await flush();
    canceled.abort();
    await expect(queued).rejects.toHaveProperty('name', 'AbortError');
    expect(poll).not.toHaveBeenCalled();
    expect(fake.queued.size).toBe(0);
  });

  it('paces short fallback polls and lets a queued poll abort', async () => {
    vi.useFakeTimers();
    vi.stubGlobal('navigator', {});
    const slots = new DirectPollSlots({ channel: null });
    const first = vi.fn(async (grant: SlotGrant) => grant);
    const second = vi.fn(async (grant: SlotGrant) => grant);
    const canceled = new AbortController();
    const one = slots.run(new AbortController().signal, first);
    const two = slots.run(canceled.signal, second);
    expect(await one).toEqual({ contended: true, maxTimeoutMs: 0 });
    expect(first).toHaveBeenCalledTimes(1);
    expect(second).not.toHaveBeenCalled();
    canceled.abort();
    await expect(two).rejects.toHaveProperty('name', 'AbortError');
    await vi.advanceTimersByTimeAsync(250);
    expect(second).not.toHaveBeenCalled();
  });

  it('opens a BroadcastChannel only when used and closes it with the last listener', () => {
    const opened: Array<{ name: string; closed: boolean; posted: unknown[] }> = [];
    class FakeChannel {
      onmessage: (() => void) | null = null;
      readonly record: { name: string; closed: boolean; posted: unknown[] };
      constructor(name: string) {
        this.record = { name, closed: false, posted: [] };
        opened.push(this.record);
      }
      postMessage(message: unknown) {
        this.record.posted.push(message);
      }
      close() {
        this.record.closed = true;
      }
    }
    vi.stubGlobal('BroadcastChannel', FakeChannel);
    const slots = new DirectPollSlots();
    expect(opened).toHaveLength(0);
    const stop = slots.onContention(() => undefined);
    expect(opened).toEqual([{ name: 'hotsheet-direct-change-poll', closed: false, posted: [] }]);
    stop();
    expect(opened[0].closed).toBe(true);
  });
});
