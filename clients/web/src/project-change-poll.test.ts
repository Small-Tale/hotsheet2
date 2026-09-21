import { describe, expect, it, vi } from 'vitest';

import type { PollResponse } from './api';
import {
  containsRepositoryChange,
  containsTicketChange,
  startProjectChangePoll,
  startProjectChangeStream,
} from './project-change-poll';

const response = (cursor: number, kind?: string, overflow = false): PollResponse => ({
  cursor,
  events: kind ? [{ store: 'local', kind, id: '01', slug: 'HS2-ONE' }] : [],
  overflow,
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => {
    resolve = next;
  });
  return { promise, resolve };
}

class FakeSocket extends EventTarget {
  readyState = 0;
  open() {
    this.readyState = 1;
    this.dispatchEvent(new Event('open'));
  }
  message(value: unknown) {
    const event = new Event('message') as MessageEvent;
    Object.defineProperty(event, 'data', { value: JSON.stringify(value) });
    this.dispatchEvent(event);
  }
  close() {
    if (this.readyState === 3) return;
    this.readyState = 3;
    this.dispatchEvent(new Event('close'));
  }
  fail() {
    this.dispatchEvent(new Event('error'));
  }
}

describe('project change long polling', () => {
  it('classifies ticket invalidations without treating activity as ticket presence', () => {
    expect(containsTicketChange(response(1, 'changed'))).toBe(true);
    expect(containsTicketChange(response(1, 'claimed'))).toBe(true);
    expect(containsTicketChange(response(1, 'activity'))).toBe(false);
    expect(containsTicketChange(response(1, undefined, true))).toBe(true);
  });

  it('scopes repository invalidations to their checkout without treating them as ticket changes', () => {
    const changed = response(2, 'repository_changed');
    changed.events[0].id = 'checkout-one';
    expect(containsRepositoryChange(changed, 'checkout-one')).toBe(true);
    expect(containsRepositoryChange(changed, 'checkout-two')).toBe(false);
    expect(containsTicketChange(changed)).toBe(false);
  });

  it('handshakes, coalesces batches, ignores unrelated events, and refreshes overflow', async () => {
    const pending = deferred<PollResponse>();
    const pollEvents = vi
      .fn()
      .mockResolvedValueOnce(response(4))
      .mockResolvedValueOnce(response(6, 'changed'))
      .mockResolvedValueOnce(response(7, 'activity'))
      .mockResolvedValueOnce(response(520, undefined, true))
      .mockReturnValueOnce(pending.promise);
    const refresh = vi.fn().mockResolvedValue(undefined);
    const stop = startProjectChangePoll({ client: { pollEvents }, refresh });
    await vi.waitFor(() => {
      expect(pollEvents).toHaveBeenCalledTimes(5);
    });
    expect(pollEvents.mock.calls.map((call) => call[0])).toEqual([undefined, 4, 6, 7, 520]);
    expect(refresh).toHaveBeenCalledTimes(2);
    stop();
    pending.resolve(response(520));
  });

  it('reconciles after failure, backs off, and performs a fresh handshake', async () => {
    const pending = deferred<PollResponse>();
    const pollEvents = vi
      .fn()
      .mockRejectedValueOnce(new Error('server restarted'))
      .mockResolvedValueOnce(response(2))
      .mockResolvedValueOnce(response(3, 'updated'))
      .mockReturnValueOnce(pending.promise);
    const refresh = vi.fn().mockResolvedValue(undefined);
    const wait = vi.fn().mockResolvedValue(undefined);
    const onError = vi.fn();
    const stop = startProjectChangePoll({ client: { pollEvents }, refresh, wait, onError });
    await vi.waitFor(() => {
      expect(pollEvents).toHaveBeenCalledTimes(4);
    });
    expect(pollEvents.mock.calls.map((call) => call[0])).toEqual([undefined, undefined, 2, 3]);
    expect(refresh).toHaveBeenCalledTimes(2);
    expect(wait).toHaveBeenCalledWith(500, expect.any(AbortSignal));
    expect(onError).toHaveBeenCalledTimes(1);
    stop();
    pending.resolve(response(3));
  });

  it('does not refresh from a response that arrives after stop', async () => {
    const pending = deferred<PollResponse>();
    const refresh = vi.fn().mockResolvedValue(undefined);
    const stop = startProjectChangePoll({ client: { pollEvents: vi.fn(() => pending.promise) }, refresh });
    stop();
    pending.resolve(response(1, 'changed'));
    await Promise.resolve();
    await Promise.resolve();
    expect(refresh).not.toHaveBeenCalled();
  });

  it('delivers non-ticket events without forcing a project refresh', async () => {
    const pending = deferred<PollResponse>();
    const pollEvents = vi
      .fn()
      .mockResolvedValueOnce(response(2))
      .mockResolvedValueOnce(response(3, 'permission_asked'))
      .mockReturnValueOnce(pending.promise);
    const refresh = vi.fn().mockResolvedValue(undefined);
    const onEvents = vi.fn().mockResolvedValue(undefined);
    const stop = startProjectChangePoll({ client: { pollEvents }, refresh, onEvents });
    await vi.waitFor(() => {
      expect(pollEvents).toHaveBeenCalledTimes(3);
    });
    expect(onEvents).toHaveBeenCalledWith(response(3, 'permission_asked'));
    expect(refresh).not.toHaveBeenCalled();
    stop();
    pending.resolve(response(3));
  });

  it('holds an authoritative refresh behind an in-flight local projection', async () => {
    const pendingPoll = deferred<PollResponse>();
    const projection = deferred<undefined>();
    const pollEvents = vi
      .fn()
      .mockResolvedValueOnce(response(8))
      .mockResolvedValueOnce(response(9, 'created'))
      .mockReturnValueOnce(pendingPoll.promise);
    const refresh = vi.fn().mockResolvedValue(undefined);
    const beforeRefresh = vi.fn(() => projection.promise);
    const stop = startProjectChangePoll({ client: { pollEvents }, refresh, beforeRefresh });
    await vi.waitFor(() => {
      expect(beforeRefresh).toHaveBeenCalledTimes(1);
    });
    expect(refresh).not.toHaveBeenCalled();
    projection.resolve(undefined);
    await vi.waitFor(() => {
      expect(refresh).toHaveBeenCalledTimes(1);
    });
    expect(pollEvents).toHaveBeenCalledTimes(3);
    stop();
    pendingPoll.resolve(response(9));
  });

  it('rechecks an invalidation after the local mutation barrier settles', async () => {
    const pendingPoll = deferred<PollResponse>();
    const projection = deferred<undefined>();
    let locallyAcknowledged = false;
    const pollEvents = vi
      .fn()
      .mockResolvedValueOnce(response(8))
      .mockResolvedValueOnce(response(9, 'updated'))
      .mockReturnValueOnce(pendingPoll.promise);
    const refresh = vi.fn().mockResolvedValue(undefined);
    const stop = startProjectChangePoll({
      client: { pollEvents },
      refresh,
      beforeRefresh: () => projection.promise,
      shouldRefresh: () => !locallyAcknowledged,
    });
    await vi.waitFor(() => {
      expect(pollEvents).toHaveBeenCalledTimes(2);
    });
    locallyAcknowledged = true;
    projection.resolve(undefined);
    await vi.waitFor(() => {
      expect(pollEvents).toHaveBeenCalledTimes(3);
    });
    expect(refresh).not.toHaveBeenCalled();
    stop();
    pendingPoll.resolve(response(9));
  });

  it('does not reconcile repeatedly while a polling outage continues', async () => {
    const pending = deferred<PollResponse>();
    const pollEvents = vi
      .fn()
      .mockRejectedValueOnce(new Error('unsupported by running server'))
      .mockRejectedValueOnce(new Error('still unsupported'))
      .mockReturnValueOnce(pending.promise);
    const refresh = vi.fn().mockResolvedValue(undefined);
    const wait = vi.fn().mockResolvedValue(undefined);
    const stop = startProjectChangePoll({ client: { pollEvents }, refresh, wait });
    await vi.waitFor(() => {
      expect(pollEvents).toHaveBeenCalledTimes(3);
    });
    expect(refresh).not.toHaveBeenCalled();
    expect(wait.mock.calls.map((call) => call[0])).toEqual([500, 1_000]);
    stop();
    pending.resolve(response(0));
  });
});

describe('project change WebSocket stream', () => {
  it('uses one handshake and one subscribe-race replay while an idle socket stays open', async () => {
    const pollEvents = vi.fn().mockResolvedValueOnce(response(4)).mockResolvedValueOnce(response(4));
    const refresh = vi.fn().mockResolvedValue(undefined),
      onEvents = vi.fn().mockResolvedValue(undefined),
      sockets: FakeSocket[] = [];
    const stop = startProjectChangeStream({
      client: { pollEvents, changeWebSocketUrl: () => 'ws://localhost/project/ws/sync' },
      refresh,
      onEvents,
      openWebSocket: () => {
        const socket = new FakeSocket();
        sockets.push(socket);
        queueMicrotask(() => {
          socket.open();
        });
        return socket as unknown as WebSocket;
      },
    });
    await vi.waitFor(() => {
      expect(pollEvents).toHaveBeenCalledTimes(2);
    });
    sockets[0].message({ cursor: 5, store: 'local', kind: 'updated', id: '01', slug: 'HS2-ONE' });
    await vi.waitFor(() => {
      expect(refresh).toHaveBeenCalledTimes(1);
    });
    expect(onEvents).toHaveBeenCalledWith({
      cursor: 5,
      events: [{ cursor: 5, store: 'local', kind: 'updated', id: '01', slug: 'HS2-ONE' }],
      overflow: false,
    });
    await new Promise((resolve) => setTimeout(resolve, 10));
    expect(pollEvents).toHaveBeenCalledTimes(2);
    stop();
  });

  it('replays the disconnect gap before reconnecting without duplicating queued socket events', async () => {
    const replay = {
      cursor: 6,
      events: [
        { cursor: 5, store: 'local', kind: 'updated', id: '01', slug: 'HS2-ONE' },
        { cursor: 6, store: 'local', kind: 'activity', id: '02', slug: 'HS2-TWO' },
      ],
      overflow: false,
    };
    const pollEvents = vi
      .fn()
      .mockResolvedValueOnce(response(4))
      .mockResolvedValueOnce(response(4))
      .mockResolvedValueOnce(replay)
      .mockResolvedValueOnce(response(6));
    const refresh = vi.fn().mockResolvedValue(undefined),
      onEvents = vi.fn().mockResolvedValue(undefined),
      wait = vi.fn().mockResolvedValue(undefined),
      sockets: FakeSocket[] = [];
    const stop = startProjectChangeStream({
      client: { pollEvents, changeWebSocketUrl: () => 'ws://localhost/project/ws/sync' },
      refresh,
      onEvents,
      wait,
      openWebSocket: () => {
        const socket = new FakeSocket();
        sockets.push(socket);
        queueMicrotask(() => {
          socket.open();
        });
        return socket as unknown as WebSocket;
      },
    });
    await vi.waitFor(() => {
      expect(pollEvents).toHaveBeenCalledTimes(2);
    });
    sockets[0].close();
    await vi.waitFor(() => {
      expect(sockets).toHaveLength(2);
    });
    expect(onEvents).toHaveBeenCalledWith(replay);
    expect(refresh).toHaveBeenCalledTimes(1);
    expect(wait).toHaveBeenCalledWith(expect.any(Number), expect.any(AbortSignal));
    expect(wait.mock.calls[0][0]).toBeGreaterThan(0);
    expect(wait.mock.calls[0][0]).toBeLessThanOrEqual(500);
    sockets[1].message({ cursor: 6, store: 'local', kind: 'activity', id: '02', slug: 'HS2-TWO' });
    await Promise.resolve();
    expect(onEvents).toHaveBeenCalledTimes(1);
    stop();
  });

  it('falls back and backs off when an older socket sends a non-replayable event', async () => {
    const waiting = deferred<undefined>(),
      pollEvents = vi
        .fn()
        .mockResolvedValueOnce(response(9))
        .mockResolvedValueOnce(response(9))
        .mockResolvedValueOnce(response(9));
    const refresh = vi.fn().mockResolvedValue(undefined),
      onError = vi.fn(),
      wait = vi.fn((milliseconds: number, signal: AbortSignal) => {
        void milliseconds;
        void signal;
        return waiting.promise;
      }),
      sockets: FakeSocket[] = [];
    const stop = startProjectChangeStream({
      client: { pollEvents, changeWebSocketUrl: () => 'ws://localhost/project/ws/sync' },
      refresh,
      onError,
      wait,
      openWebSocket: () => {
        const socket = new FakeSocket();
        sockets.push(socket);
        queueMicrotask(() => {
          socket.open();
        });
        return socket as unknown as WebSocket;
      },
    });
    await vi.waitFor(() => {
      expect(pollEvents).toHaveBeenCalledTimes(2);
    });
    sockets[0].message({ store: 'local', kind: 'updated', id: '01', slug: 'HS2-ONE' });
    await vi.waitFor(() => {
      expect(wait).toHaveBeenCalledWith(expect.any(Number), expect.any(AbortSignal));
    });
    expect(wait.mock.calls[0][0]).toBeGreaterThan(0);
    expect(wait.mock.calls[0][0]).toBeLessThanOrEqual(500);
    expect(onError.mock.calls.some(([reason]) => String(reason).includes('not replayable'))).toBe(true);
    expect(refresh).not.toHaveBeenCalled();
    stop();
    waiting.resolve(undefined);
  });

  it('caps repeated upgrade retry delays while long-poll remains available', async () => {
    const delays: number[] = [],
      done = deferred<undefined>(),
      pollEvents = vi.fn().mockResolvedValue(response(0));
    let stop = () => {};
    const wait = vi.fn(async (milliseconds: number) => {
      delays.push(milliseconds);
      if (delays.length === 4) {
        stop();
        done.resolve(undefined);
      }
    });
    stop = startProjectChangeStream({
      client: { pollEvents, changeWebSocketUrl: () => 'ws://localhost/project/ws/sync' },
      refresh: vi.fn().mockResolvedValue(undefined),
      wait,
      retryMs: 500,
      maxRetryMs: 2_000,
      openWebSocket: () => {
        const socket = new FakeSocket();
        queueMicrotask(() => {
          socket.fail();
        });
        return socket as unknown as WebSocket;
      },
    });
    await done.promise;
    expect(delays).toHaveLength(4);
    expect(delays[0]).toBeLessThanOrEqual(500);
    expect(delays[1]).toBeLessThanOrEqual(1_000);
    expect(delays[2]).toBeLessThanOrEqual(2_000);
    expect(delays[3]).toBeLessThanOrEqual(2_000);
    expect(Math.max(...delays)).toBeLessThanOrEqual(2_000);
  });
});
