import { EventEmitter } from 'node:events';

import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import {
  DIRECTORY_PROBE_PREFIX,
  type DirectorySubscriber,
  SharedDirectoryWatch,
  type WatchDirectory,
} from './shared-directory-watch';

/** An in-process directory stream the test drives; no OS watcher is involved. */
function inProcessStream() {
  const watchers: (EventEmitter & { close: ReturnType<typeof vi.fn>; deliver: (name: string | null) => void })[] = [];
  const watchDirectory = vi.fn<WatchDirectory>((_directory, changed) => {
    const watcher = Object.assign(new EventEmitter(), { close: vi.fn(), deliver: changed });
    watchers.push(watcher);
    return watcher;
  });
  const written: string[] = [];
  const removed: string[] = [];
  const shared = new SharedDirectoryWatch({
    directory: '/registry',
    watchDirectory,
    writeProbe: async (name) => {
      written.push(name);
    },
    removeProbe: async (name) => {
      removed.push(name);
    },
    lingerMs: 1_000,
    probeWaitMs: 100,
    probeWaitMaxMs: 400,
  });
  return { shared, watchDirectory, watchers, written, removed, current: () => watchers.at(-1)! };
}
function subscriber() {
  return {
    changed: vi.fn<() => void>(),
    recheck: vi.fn<() => void>(),
    failed: vi.fn<() => void>(),
  } satisfies DirectorySubscriber;
}

describe('shared directory watch (HS2-R1KHPN)', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it('shares one native watcher across subscribers and fans changes out by file name', () => {
    const stream = inProcessStream();
    const a = subscriber(),
      b = subscriber(),
      c = subscriber();
    stream.shared.subscribe('a.json', a);
    stream.shared.subscribe('b.json', b);
    stream.shared.subscribe('a.json', c);
    expect(stream.watchDirectory).toHaveBeenCalledTimes(1);
    expect(stream.watchDirectory).toHaveBeenCalledWith('/registry', expect.any(Function));
    stream.current().deliver('a.json');
    expect(a.changed).toHaveBeenCalledTimes(1);
    expect(c.changed).toHaveBeenCalledTimes(1);
    expect(b.changed).not.toHaveBeenCalled();
    stream.current().deliver('b.json');
    expect(b.changed).toHaveBeenCalledTimes(1);
    expect(stream.shared.subscriberCount).toBe(3);
  });

  it('keeps the watcher open between back-to-back polls and closes it only after the linger window', () => {
    const stream = inProcessStream();
    for (let poll = 0; poll < 5; poll++) {
      const unsubscribe = stream.shared.subscribe('a.json', subscriber());
      unsubscribe();
      vi.advanceTimersByTime(999);
    }
    expect(stream.watchDirectory).toHaveBeenCalledTimes(1);
    expect(stream.current().close).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(stream.current().close).toHaveBeenCalledTimes(1);
    expect(stream.shared.open).toBe(false);
    // A later poll registers a fresh watcher and closes it again after its own linger.
    const later = subscriber();
    const unsubscribe = stream.shared.subscribe('a.json', later);
    expect(stream.watchDirectory).toHaveBeenCalledTimes(2);
    stream.watchers[0].deliver('a.json');
    expect(later.changed).not.toHaveBeenCalled();
    stream.current().deliver('a.json');
    expect(later.changed).toHaveBeenCalledTimes(1);
    unsubscribe();
    unsubscribe();
    vi.advanceTimersByTime(1_000);
    expect(stream.current().close).toHaveBeenCalledTimes(1);
  });

  it('does not linger while another subscriber still watches', () => {
    const stream = inProcessStream();
    const first = stream.shared.subscribe('a.json', subscriber());
    const second = subscriber();
    stream.shared.subscribe('b.json', second);
    first();
    vi.advanceTimersByTime(10_000);
    expect(stream.current().close).not.toHaveBeenCalled();
    stream.current().deliver('b.json');
    expect(second.changed).toHaveBeenCalledTimes(1);
  });

  it('asks every subscriber for a catch-up read when the stream first proves live', () => {
    const stream = inProcessStream();
    const a = subscriber(),
      b = subscriber();
    stream.shared.subscribe('a.json', a);
    stream.shared.subscribe('b.json', b);
    expect(stream.shared.live).toBe(false);
    expect(stream.written).toHaveLength(1);
    expect(stream.written[0].startsWith(DIRECTORY_PROBE_PREFIX)).toBe(true);
    stream.current().deliver(stream.written[0]);
    expect(stream.shared.live).toBe(true);
    expect(a.recheck).toHaveBeenCalledTimes(1);
    expect(b.recheck).toHaveBeenCalledTimes(1);
    // A probe is never reported as a change, and the proof stops probing.
    expect(a.changed).not.toHaveBeenCalled();
    expect(stream.removed).toEqual(stream.written);
    vi.advanceTimersByTime(10_000);
    expect(stream.written).toHaveLength(1);
    // Later events are plain changes, without further catch-up reads.
    stream.current().deliver('a.json');
    expect(a.changed).toHaveBeenCalledTimes(1);
    expect(a.recheck).toHaveBeenCalledTimes(1);
  });

  it('takes any delivered event as proof, and a change that proves the stream is reported too', () => {
    const stream = inProcessStream();
    const a = subscriber();
    stream.shared.subscribe('a.json', a);
    stream.current().deliver('a.json');
    expect(a.recheck).toHaveBeenCalledTimes(1);
    expect(a.changed).toHaveBeenCalledTimes(1);
    expect(stream.shared.live).toBe(true);
  });

  it('bridges an unproven stream with catch-up reads under bounded backoff', () => {
    const stream = inProcessStream();
    const a = subscriber();
    stream.shared.subscribe('a.json', a);
    vi.advanceTimersByTime(99);
    expect(a.recheck).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(a.recheck).toHaveBeenCalledTimes(1);
    expect(stream.written).toHaveLength(2);
    expect(stream.removed).toEqual([stream.written[0]]);
    vi.advanceTimersByTime(200);
    expect(a.recheck).toHaveBeenCalledTimes(2);
    vi.advanceTimersByTime(400);
    expect(a.recheck).toHaveBeenCalledTimes(3);
    // Capped at probeWaitMaxMs, never faster.
    vi.advanceTimersByTime(399);
    expect(a.recheck).toHaveBeenCalledTimes(3);
    vi.advanceTimersByTime(1);
    expect(a.recheck).toHaveBeenCalledTimes(4);
    stream.current().deliver(stream.written.at(-1)!);
    expect(a.recheck).toHaveBeenCalledTimes(5);
    vi.advanceTimersByTime(10_000);
    expect(a.recheck).toHaveBeenCalledTimes(5);
  });

  it('stops probing while nobody subscribes and resumes for the next unproven subscriber', () => {
    const stream = inProcessStream();
    const unsubscribe = stream.shared.subscribe('a.json', subscriber());
    unsubscribe();
    expect(stream.removed).toEqual(stream.written);
    vi.advanceTimersByTime(900);
    expect(stream.written).toHaveLength(1);
    const next = subscriber();
    stream.shared.subscribe('a.json', next);
    expect(stream.watchDirectory).toHaveBeenCalledTimes(1);
    expect(stream.written).toHaveLength(2);
    vi.advanceTimersByTime(100);
    expect(next.recheck).toHaveBeenCalledTimes(1);
  });

  it('keeps a proven stream proven across lingering resubscriptions', () => {
    const stream = inProcessStream();
    const unsubscribe = stream.shared.subscribe('a.json', subscriber());
    stream.current().deliver(null);
    unsubscribe();
    const next = subscriber();
    stream.shared.subscribe('a.json', next);
    expect(stream.shared.live).toBe(true);
    expect(stream.written).toHaveLength(1);
    vi.advanceTimersByTime(10_000);
    expect(next.recheck).not.toHaveBeenCalled();
  });

  it('turns an unnamed event into a catch-up read for every subscriber', () => {
    const stream = inProcessStream();
    const a = subscriber();
    stream.shared.subscribe('a.json', a);
    stream.current().deliver(stream.written[0]);
    stream.current().deliver(null);
    expect(a.recheck).toHaveBeenCalledTimes(2);
    expect(a.changed).not.toHaveBeenCalled();
  });

  it('fails every subscriber on a native error, then opens a fresh watcher for the next poll', () => {
    const stream = inProcessStream();
    const a = subscriber(),
      b = subscriber();
    const unsubscribeA = stream.shared.subscribe('a.json', a);
    stream.shared.subscribe('b.json', b);
    const broken = stream.current();
    broken.emit('error', Object.assign(new Error('EMFILE: too many open files, watch'), { code: 'EMFILE' }));
    expect(a.failed).toHaveBeenCalledTimes(1);
    expect(b.failed).toHaveBeenCalledTimes(1);
    expect(broken.close).toHaveBeenCalledTimes(1);
    expect(stream.shared.subscriberCount).toBe(0);
    expect(stream.removed).toEqual(stream.written);
    // Late callbacks from the broken watcher reach nobody and start nothing.
    broken.deliver('a.json');
    broken.emit('error', new Error('late'));
    expect(a.changed).not.toHaveBeenCalled();
    expect(a.failed).toHaveBeenCalledTimes(1);
    unsubscribeA();
    vi.advanceTimersByTime(10_000);
    expect(a.recheck).not.toHaveBeenCalled();
    const next = subscriber();
    stream.shared.subscribe('a.json', next);
    expect(stream.watchDirectory).toHaveBeenCalledTimes(2);
    stream.current().deliver('a.json');
    expect(next.changed).toHaveBeenCalledTimes(1);
  });

  it('closes a lingering watcher that errors, without notifying anyone', () => {
    const stream = inProcessStream();
    const a = subscriber();
    stream.shared.subscribe('a.json', a)();
    stream.current().emit('error', new Error('EMFILE'));
    expect(stream.current().close).toHaveBeenCalledTimes(1);
    expect(a.failed).not.toHaveBeenCalled();
    vi.advanceTimersByTime(10_000);
    expect(stream.current().close).toHaveBeenCalledTimes(1);
  });

  it('fails a subscriber synchronously when the OS refuses the watcher, and retries for the next one', () => {
    const stream = inProcessStream();
    stream.watchDirectory.mockImplementationOnce(() => {
      throw Object.assign(new Error('EMFILE: too many open files, watch'), { code: 'EMFILE' });
    });
    const refused = subscriber();
    const unsubscribe = stream.shared.subscribe('a.json', refused);
    expect(refused.failed).toHaveBeenCalledTimes(1);
    expect(stream.shared.open).toBe(false);
    expect(stream.written).toHaveLength(0);
    unsubscribe();
    vi.advanceTimersByTime(10_000);
    expect(refused.recheck).not.toHaveBeenCalled();
    const next = subscriber();
    stream.shared.subscribe('a.json', next);
    expect(stream.shared.open).toBe(true);
    expect(next.failed).not.toHaveBeenCalled();
  });

  it('lets a subscriber unsubscribe from its own change callback while others still hear it', () => {
    const stream = inProcessStream();
    const unsubscribers: (() => void)[] = [];
    const first = {
      ...subscriber(),
      changed: vi.fn(() => {
        unsubscribers[0]();
      }),
    };
    const second = subscriber();
    unsubscribers.push(stream.shared.subscribe('a.json', first));
    unsubscribers.push(stream.shared.subscribe('a.json', second));
    stream.current().deliver('a.json');
    expect(first.changed).toHaveBeenCalledTimes(1);
    expect(second.changed).toHaveBeenCalledTimes(1);
    expect(stream.shared.subscriberCount).toBe(1);
  });

  it('keeps probing when the probe cannot be written', async () => {
    const watchers: EventEmitter[] = [];
    const shared = new SharedDirectoryWatch({
      directory: '/read-only',
      watchDirectory: () => {
        const watcher = Object.assign(new EventEmitter(), { close: vi.fn() });
        watchers.push(watcher);
        return watcher;
      },
      writeProbe: () => Promise.reject(new Error('EACCES')),
      removeProbe: () => Promise.reject(new Error('ENOENT')),
      probeWaitMs: 100,
    });
    const a = subscriber();
    shared.subscribe('a.json', a);
    await vi.advanceTimersByTimeAsync(100);
    expect(a.recheck).toHaveBeenCalledTimes(1);
    shared.close();
  });

  it('closes immediately on close() and ends subscriptions silently', () => {
    const stream = inProcessStream();
    const a = subscriber();
    const unsubscribe = stream.shared.subscribe('a.json', a);
    stream.shared.close();
    expect(stream.current().close).toHaveBeenCalledTimes(1);
    expect(stream.shared.subscriberCount).toBe(0);
    unsubscribe();
    vi.advanceTimersByTime(10_000);
    expect(stream.current().close).toHaveBeenCalledTimes(1);
    expect(a.recheck).not.toHaveBeenCalled();
    expect(a.failed).not.toHaveBeenCalled();
    stream.shared.close();
  });
});
