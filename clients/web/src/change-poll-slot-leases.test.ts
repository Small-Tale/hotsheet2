import { afterEach, describe, expect, it, vi } from 'vitest';

import {
  broadcastLeaseBus,
  LEASE_CLAIM_TTL_MS,
  LEASE_GRACE_MS,
  LEASE_HOLD_MAX_MS,
  LEASE_SETTLE_MS,
  type PageLifecycle,
  type SlotLeaseBus,
  type SlotLeaseMessage,
  SlotLeases,
  windowPageLifecycle,
} from './change-poll-slot-leases';

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

/** An origin-wide in-memory lease bus: delivers asynchronously to every other context. */
function fakeLeaseHub() {
  const members = new Set<{ owner: number; listener: (message: SlotLeaseMessage) => void }>();
  const sent: SlotLeaseMessage[] = [];
  let next = 0;
  const bus = (): SlotLeaseBus => {
    const owner = (next += 1);
    return {
      post: (message) => {
        sent.push(message);
        const targets = [...members].filter((member) => member.owner !== owner);
        queueMicrotask(() => {
          for (const member of targets) member.listener(structuredClone(message));
        });
      },
      listen: (listener) => {
        const member = { owner, listener };
        members.add(member);
        return () => members.delete(member);
      },
    };
  };
  return { bus, sent };
}

const flush = async () => {
  for (let index = 0; index < 8; index += 1) await Promise.resolve();
};

/** A poll that stays open until the test finishes it. */
function heldPolls() {
  const open = new Set<() => void>();
  const grants: boolean[] = [];
  let maxOpen = 0;
  const poll = (contended: boolean) =>
    new Promise<boolean>((resolve) => {
      grants.push(contended);
      const done = () => {
        open.delete(done);
        resolve(contended);
      };
      open.add(done);
      maxOpen = Math.max(maxOpen, open.size);
    });
  return {
    poll,
    grants,
    open,
    maxOpen: () => maxOpen,
    finishOne: () => {
      [...open][0]?.();
    },
  };
}

describe('direct-poll slot leases without Web Locks (HS2-QS2P6B)', () => {
  it('two contexts share the four-slot cap and refill freed slots', async () => {
    vi.useFakeTimers();
    const hub = fakeLeaseHub();
    const first = new SlotLeases({ bus: hub.bus(), capacity: 4, contextId: 'a' });
    const second = new SlotLeases({ bus: hub.bus(), capacity: 4, contextId: 'b' });
    const polls = heldPolls();
    const signal = new AbortController().signal;
    const runs = [first, second].flatMap((leases) => Array.from({ length: 4 }, () => leases.run(signal, polls.poll)));
    await vi.advanceTimersByTimeAsync(LEASE_SETTLE_MS * 4);
    expect(polls.open.size).toBe(4);
    expect(first.held() + second.held()).toBe(4);
    for (let finished = 0; finished < 8; finished += 1) {
      polls.finishOne();
      await vi.advanceTimersByTimeAsync(LEASE_SETTLE_MS * 4);
      expect(polls.open.size).toBeLessThanOrEqual(4);
    }
    await Promise.all(runs);
    expect(polls.maxOpen()).toBe(4);
    expect(polls.grants).toHaveLength(8);
    expect(first.held() + second.held()).toBe(0);
  });

  it('a lone context holds without contention and posts no waiting', async () => {
    vi.useFakeTimers();
    const hub = fakeLeaseHub();
    const leases = new SlotLeases({ bus: hub.bus(), capacity: 4, contextId: 'solo' });
    const polls = heldPolls();
    const run = leases.run(new AbortController().signal, polls.poll);
    await vi.advanceTimersByTimeAsync(LEASE_SETTLE_MS);
    expect(polls.grants).toEqual([false]);
    expect(hub.sent.map((message) => message.type)).toEqual(['query', 'claim', 'hold']);
    polls.finishOne();
    await run;
    expect(hub.sent.at(-1)?.type).toBe('release');
  });

  it('the oldest waiter wins a race for a freed slot', async () => {
    vi.useFakeTimers();
    const hub = fakeLeaseHub();
    const holder = new SlotLeases({ bus: hub.bus(), capacity: 1, contextId: 'h' });
    const older = new SlotLeases({ bus: hub.bus(), capacity: 1, contextId: 'o' });
    const newer = new SlotLeases({ bus: hub.bus(), capacity: 1, contextId: 'n' });
    const order: string[] = [];
    const signal = new AbortController().signal;
    let finishHolder: () => void = () => undefined;
    const held = holder.run(
      signal,
      () =>
        new Promise<void>((resolve) => {
          finishHolder = resolve;
        }),
    );
    await vi.advanceTimersByTimeAsync(LEASE_SETTLE_MS * 2);
    const a = older.run(signal, () => {
      order.push('older');
      return Promise.resolve();
    });
    await vi.advanceTimersByTimeAsync(10);
    const b = newer.run(signal, () => {
      order.push('newer');
      return Promise.resolve();
    });
    await vi.advanceTimersByTimeAsync(LEASE_SETTLE_MS * 2);
    expect(order).toEqual([]);
    finishHolder();
    await held;
    await vi.advanceTimersByTimeAsync(LEASE_SETTLE_MS * 4);
    await Promise.all([a, b]);
    expect(order).toEqual(['older', 'newer']);
  });

  it('a context opened later learns existing holds by query and waits', async () => {
    vi.useFakeTimers();
    const hub = fakeLeaseHub();
    const holder = new SlotLeases({ bus: hub.bus(), capacity: 1, contextId: 'h' });
    const signal = new AbortController().signal;
    void holder.run(signal, () => new Promise<never>(() => undefined));
    await vi.advanceTimersByTimeAsync(LEASE_SETTLE_MS * 2);
    const late = new SlotLeases({ bus: hub.bus(), capacity: 1, contextId: 'l' });
    const poll = vi.fn(() => Promise.resolve());
    void late.run(signal, poll);
    await vi.advanceTimersByTimeAsync(LEASE_SETTLE_MS * 2);
    expect(poll).not.toHaveBeenCalled();
    expect(hub.sent.filter((message) => message.type === 'hold' && message.from === 'h')).toHaveLength(2);
  });

  it('reclaims the slot of a context that vanished once its lease expires', async () => {
    vi.useFakeTimers();
    const hub = fakeLeaseHub();
    const gone = hub.bus();
    const survivor = new SlotLeases({ bus: hub.bus(), capacity: 1, contextId: 's' });
    const poll = vi.fn(() => Promise.resolve());
    const run = survivor.run(new AbortController().signal, poll);
    // A context that confirmed a hold and then closed without releasing it.
    gone.post({
      type: 'hold',
      from: 'gone',
      id: 'gone:1',
      rank: 0,
      until: Date.now() + LEASE_HOLD_MAX_MS + LEASE_GRACE_MS,
    });
    await vi.advanceTimersByTimeAsync(LEASE_SETTLE_MS * 2);
    expect(poll).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(LEASE_HOLD_MAX_MS + LEASE_GRACE_MS);
    await run;
    expect(poll).toHaveBeenCalledTimes(1);
  });

  it('ignores a tentative claim that was never confirmed after its TTL', async () => {
    vi.useFakeTimers();
    const hub = fakeLeaseHub();
    const gone = hub.bus();
    const leases = new SlotLeases({ bus: hub.bus(), capacity: 1, contextId: 'z' });
    void leases.run(new AbortController().signal, () => Promise.resolve());
    await vi.advanceTimersByTimeAsync(LEASE_SETTLE_MS * 2);
    const poll = vi.fn(() => Promise.resolve());
    gone.post({ type: 'claim', from: 'gone', id: 'gone:1', rank: 0 });
    await flush();
    const run = leases.run(new AbortController().signal, poll);
    await vi.advanceTimersByTimeAsync(LEASE_SETTLE_MS * 2);
    expect(poll).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(LEASE_CLAIM_TTL_MS);
    await run;
    expect(poll).toHaveBeenCalledTimes(1);
  });

  it('announces a wait, reports contention to holders, and lets the wait abort cleanly', async () => {
    vi.useFakeTimers();
    const hub = fakeLeaseHub();
    const holder = new SlotLeases({ bus: hub.bus(), capacity: 1, contextId: 'h' });
    const waiter = new SlotLeases({ bus: hub.bus(), capacity: 1, contextId: 'w' });
    const contention = vi.fn();
    holder.onContention(contention);
    const polls = heldPolls();
    void holder.run(new AbortController().signal, polls.poll);
    await vi.advanceTimersByTimeAsync(LEASE_SETTLE_MS * 2);
    const canceled = new AbortController();
    const poll = vi.fn(() => Promise.resolve());
    const queued = waiter.run(canceled.signal, poll);
    await vi.advanceTimersByTimeAsync(LEASE_SETTLE_MS * 2);
    expect(contention).toHaveBeenCalledTimes(1);
    canceled.abort();
    await expect(queued).rejects.toHaveProperty('name', 'AbortError');
    expect(poll).not.toHaveBeenCalled();
    // The holder's next grant sees the recent wait; it expires on its own.
    polls.finishOne();
    await flush();
    void holder.run(new AbortController().signal, polls.poll);
    await vi.advanceTimersByTimeAsync(LEASE_SETTLE_MS);
    expect(polls.grants).toEqual([false, true]);
  });

  it('aborting during the settle window withdraws the claim', async () => {
    vi.useFakeTimers();
    const hub = fakeLeaseHub();
    const leases = new SlotLeases({ bus: hub.bus(), capacity: 1, contextId: 'x' });
    const canceled = new AbortController();
    const poll = vi.fn(() => Promise.resolve());
    const run = leases.run(canceled.signal, poll);
    await vi.advanceTimersByTimeAsync(LEASE_SETTLE_MS / 2);
    canceled.abort();
    await expect(run).rejects.toHaveProperty('name', 'AbortError');
    expect(hub.sent.map((message) => message.type)).toEqual(['query', 'claim', 'release']);
    expect(poll).not.toHaveBeenCalled();
  });

  it('without any bus holds one slot per context and rotates local waiters', async () => {
    const leases = new SlotLeases({ bus: null, capacity: 4 });
    expect(leases.capacity).toBe(1);
    const contention = vi.fn();
    leases.onContention(contention);
    const polls = heldPolls();
    const signal = new AbortController().signal;
    const runs = [leases.run(signal, polls.poll), leases.run(signal, polls.poll)];
    await flush();
    expect(polls.open.size).toBe(1);
    expect(contention).toHaveBeenCalledTimes(1);
    // The holder already sees the local waiter, so it will rotate on the short cap.
    expect(polls.grants).toEqual([true]);
    polls.finishOne();
    await flush();
    expect(polls.open.size).toBe(1);
    polls.finishOne();
    await Promise.all(runs);
    expect(leases.held()).toBe(0);
  });

  it('opens the BroadcastChannel lazily and drops malformed messages', () => {
    const opened: Array<{ name: string; posted: unknown[]; onmessage: ((event: MessageEvent) => void) | null }> = [];
    class FakeChannel {
      onmessage: ((event: MessageEvent) => void) | null = null;
      readonly posted: unknown[] = [];
      constructor(readonly name: string) {
        opened.push(this);
      }
      postMessage(message: unknown) {
        this.posted.push(message);
      }
    }
    vi.stubGlobal('BroadcastChannel', FakeChannel);
    const bus = broadcastLeaseBus();
    expect(bus).not.toBeNull();
    expect(opened).toHaveLength(0);
    const received: SlotLeaseMessage[] = [];
    bus?.listen((message) => received.push(message));
    expect(opened[0].name).toBe('hotsheet-direct-change-poll-leases');
    opened[0].onmessage?.({ data: { type: 'bogus', from: 'x' } } as MessageEvent);
    opened[0].onmessage?.({ data: 'garbage' } as MessageEvent);
    opened[0].onmessage?.({ data: { type: 'release', from: 'x', id: 'x:1' } } as MessageEvent);
    expect(received).toEqual([{ type: 'release', from: 'x', id: 'x:1' }]);
    vi.stubGlobal('BroadcastChannel', undefined);
    expect(broadcastLeaseBus()).toBeNull();
  });
});

/** A page whose lifecycle events the test fires by hand. */
function fakeLifecycle() {
  const listeners = new Set<Parameters<PageLifecycle['listen']>[0]>();
  let registrations = 0;
  const lifecycle: PageLifecycle = {
    listen: (listener) => {
      registrations += 1;
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  };
  const fire = (type: 'pagehide' | 'pageshow', persisted: boolean) => {
    for (const listener of [...listeners]) listener({ type, persisted });
  };
  return { lifecycle, fire, registrations: () => registrations };
}

describe('direct-poll slot leases across page hide and show (HS2-S3RHFP)', () => {
  it('a page that hides mid-hold frees its slot at once, not after the lease expires', async () => {
    vi.useFakeTimers();
    const hub = fakeLeaseHub();
    const page = fakeLifecycle();
    const closing = new SlotLeases({ bus: hub.bus(), capacity: 1, contextId: 'c', lifecycle: page.lifecycle });
    const survivor = new SlotLeases({ bus: hub.bus(), capacity: 1, contextId: 's', lifecycle: null });
    // The closing tab's poll never settles: its page is torn down mid-request.
    void closing.run(new AbortController().signal, () => new Promise<never>(() => undefined));
    await vi.advanceTimersByTimeAsync(LEASE_SETTLE_MS * 2);
    expect(closing.held()).toBe(1);
    const poll = vi.fn(() => Promise.resolve());
    const run = survivor.run(new AbortController().signal, poll);
    await vi.advanceTimersByTimeAsync(LEASE_SETTLE_MS * 2);
    expect(poll).not.toHaveBeenCalled();
    page.fire('pagehide', false);
    expect(closing.held()).toBe(0);
    expect(hub.sent.at(-1)).toEqual({ type: 'release', from: 'c', id: 'c:1' });
    // One settle window, far short of the 35 s lease.
    await vi.advanceTimersByTimeAsync(LEASE_SETTLE_MS * 2);
    await run;
    expect(poll).toHaveBeenCalledTimes(1);
  });

  it('an aborted poll releases its slot to a waiting peer at once', async () => {
    vi.useFakeTimers();
    const hub = fakeLeaseHub();
    const holder = new SlotLeases({ bus: hub.bus(), capacity: 1, contextId: 'h', lifecycle: null });
    const waiter = new SlotLeases({ bus: hub.bus(), capacity: 1, contextId: 'w', lifecycle: null });
    const canceled = new AbortController();
    const held = holder.run(
      canceled.signal,
      () =>
        new Promise<never>((_resolve, reject) => {
          canceled.signal.addEventListener('abort', () => {
            reject(new DOMException('aborted', 'AbortError'));
          });
        }),
    );
    await vi.advanceTimersByTimeAsync(LEASE_SETTLE_MS * 2);
    const poll = vi.fn(() => Promise.resolve());
    const run = waiter.run(new AbortController().signal, poll);
    await vi.advanceTimersByTimeAsync(LEASE_SETTLE_MS * 2);
    expect(poll).not.toHaveBeenCalled();
    canceled.abort();
    await expect(held).rejects.toHaveProperty('name', 'AbortError');
    expect(hub.sent.filter((message) => message.type === 'release' && message.from === 'h')).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(LEASE_SETTLE_MS * 2);
    await run;
    expect(poll).toHaveBeenCalledTimes(1);
  });

  it('registers no page listener until leases are first used, and only with a bus', async () => {
    vi.useFakeTimers();
    const added: string[] = [];
    const removed: string[] = [];
    vi.stubGlobal('addEventListener', (type: string) => added.push(type));
    vi.stubGlobal('removeEventListener', (type: string) => removed.push(type));
    const hub = fakeLeaseHub();
    const leases = new SlotLeases({ bus: hub.bus(), capacity: 1, contextId: 'l' });
    const lonely = new SlotLeases({ bus: null, capacity: 1 });
    expect(added).toEqual([]);
    await lonely.run(new AbortController().signal, () => Promise.resolve());
    expect(added).toEqual([]);
    const run = leases.run(new AbortController().signal, () => Promise.resolve());
    expect(added).toEqual(['pagehide', 'pageshow']);
    await vi.advanceTimersByTimeAsync(LEASE_SETTLE_MS);
    await run;
    const again = leases.run(new AbortController().signal, () => Promise.resolve());
    await vi.advanceTimersByTimeAsync(LEASE_SETTLE_MS);
    await again;
    // Registered once per context, not per poll.
    expect(added).toEqual(['pagehide', 'pageshow']);
    const stop = windowPageLifecycle()?.listen(() => undefined);
    stop?.();
    expect(removed).toEqual(['pagehide', 'pageshow']);
  });

  it('maps window page transition events and is absent without a global event target', () => {
    const target = new EventTarget();
    vi.stubGlobal('addEventListener', target.addEventListener.bind(target));
    vi.stubGlobal('removeEventListener', target.removeEventListener.bind(target));
    const seen: Array<{ type: string; persisted: boolean }> = [];
    const stop = windowPageLifecycle()?.listen((event) => seen.push(event));
    target.dispatchEvent(Object.assign(new Event('pagehide'), { persisted: true }));
    target.dispatchEvent(new Event('pageshow'));
    stop?.();
    target.dispatchEvent(new Event('pagehide'));
    expect(seen).toEqual([
      { type: 'pagehide', persisted: true },
      { type: 'pageshow', persisted: false },
    ]);
    vi.stubGlobal('addEventListener', undefined);
    expect(windowPageLifecycle()).toBeNull();
  });

  it('repeated hide and back/forward restore release once each time and relearn peers', async () => {
    vi.useFakeTimers();
    const hub = fakeLeaseHub();
    const page = fakeLifecycle();
    const leases = new SlotLeases({ bus: hub.bus(), capacity: 1, contextId: 'p', lifecycle: page.lifecycle });
    const stale = hub.bus();
    const polls = heldPolls();
    for (let round = 1; round <= 3; round += 1) {
      const run = leases.run(new AbortController().signal, polls.poll);
      await vi.advanceTimersByTimeAsync(LEASE_SETTLE_MS * 2);
      expect(leases.held()).toBe(1);
      page.fire('pagehide', true);
      expect(leases.held()).toBe(0);
      // The interrupted poll ends after the hide; its release is not posted twice.
      polls.finishOne();
      await run;
      const releases = hub.sent.filter((message) => message.type === 'release' && message.from === 'p');
      expect(releases).toHaveLength(round);
      // While frozen, a peer's hold arrived and its release was missed.
      stale.post({ type: 'hold', from: 'gone', id: `gone:${round.toString()}`, rank: 0, until: Date.now() + 35_000 });
      await flush();
      const queries = hub.sent.filter((message) => message.type === 'query' && message.from === 'p').length;
      page.fire('pageshow', false);
      expect(hub.sent.filter((message) => message.type === 'query' && message.from === 'p')).toHaveLength(queries);
      page.fire('pageshow', true);
      expect(hub.sent.filter((message) => message.type === 'query' && message.from === 'p')).toHaveLength(queries + 1);
    }
    // The stale holds were forgotten on restore, so the next poll starts without waiting.
    const poll = vi.fn(() => Promise.resolve());
    const run = leases.run(new AbortController().signal, poll);
    await vi.advanceTimersByTimeAsync(LEASE_SETTLE_MS * 2);
    await run;
    expect(poll).toHaveBeenCalledTimes(1);
    expect(page.registrations()).toBe(1);
  });

  it('a claim dropped by a hide during its settle window is retried, never granted unheld', async () => {
    vi.useFakeTimers();
    const hub = fakeLeaseHub();
    const page = fakeLifecycle();
    const leases = new SlotLeases({ bus: hub.bus(), capacity: 1, contextId: 'd', lifecycle: page.lifecycle });
    const contended = vi.fn(() => {
      expect(leases.held()).toBe(1);
      return Promise.resolve();
    });
    const run = leases.run(new AbortController().signal, contended);
    await vi.advanceTimersByTimeAsync(LEASE_SETTLE_MS / 2);
    page.fire('pagehide', true);
    expect(hub.sent.at(-1)).toEqual({ type: 'release', from: 'd', id: 'd:1' });
    page.fire('pageshow', true);
    await vi.advanceTimersByTimeAsync(LEASE_SETTLE_MS * 2);
    await run;
    expect(contended).toHaveBeenCalledTimes(1);
    expect(hub.sent.filter((message) => message.type === 'hold').map((message) => message.id)).toEqual(['d:2']);
  });
});
