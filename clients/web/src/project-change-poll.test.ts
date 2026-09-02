import { describe, expect, it, vi } from 'vitest';

import type { PollResponse } from './api';
import { containsTicketChange, startProjectChangePoll } from './project-change-poll';

const response = (cursor: number, kind?: string, overflow = false): PollResponse => ({
  cursor,
  events: kind ? [{ store: 'local', kind, id: '01', slug: 'HS2-ONE' }] : [],
  overflow,
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>(next => { resolve = next; });
  return { promise, resolve };
}

describe('project change long polling', () => {
  it('classifies ticket invalidations without treating activity as ticket presence', () => {
    expect(containsTicketChange(response(1, 'changed'))).toBe(true);
    expect(containsTicketChange(response(1, 'claimed'))).toBe(true);
    expect(containsTicketChange(response(1, 'activity'))).toBe(false);
    expect(containsTicketChange(response(1, undefined, true))).toBe(true);
  });

  it('handshakes, coalesces batches, ignores unrelated events, and refreshes overflow', async () => {
    const pending = deferred<PollResponse>();
    const pollEvents = vi.fn()
      .mockResolvedValueOnce(response(4))
      .mockResolvedValueOnce(response(6, 'changed'))
      .mockResolvedValueOnce(response(7, 'activity'))
      .mockResolvedValueOnce(response(520, undefined, true))
      .mockReturnValueOnce(pending.promise);
    const refresh = vi.fn().mockResolvedValue(undefined);
    const stop = startProjectChangePoll({ client: { pollEvents }, refresh });
    await vi.waitFor(() => { expect(pollEvents).toHaveBeenCalledTimes(5); });
    expect(pollEvents.mock.calls.map(call => call[0])).toEqual([undefined, 4, 6, 7, 520]);
    expect(refresh).toHaveBeenCalledTimes(2);
    stop();
    pending.resolve(response(520));
  });

  it('reconciles after failure, backs off, and performs a fresh handshake', async () => {
    const pending = deferred<PollResponse>();
    const pollEvents = vi.fn()
      .mockRejectedValueOnce(new Error('server restarted'))
      .mockResolvedValueOnce(response(2))
      .mockResolvedValueOnce(response(3, 'updated'))
      .mockReturnValueOnce(pending.promise);
    const refresh = vi.fn().mockResolvedValue(undefined);
    const wait = vi.fn().mockResolvedValue(undefined);
    const onError = vi.fn();
    const stop = startProjectChangePoll({ client: { pollEvents }, refresh, wait, onError });
    await vi.waitFor(() => { expect(pollEvents).toHaveBeenCalledTimes(4); });
    expect(pollEvents.mock.calls.map(call => call[0])).toEqual([undefined, undefined, 2, 3]);
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

  it('does not reconcile repeatedly while a polling outage continues', async () => {
    const pending = deferred<PollResponse>();
    const pollEvents = vi.fn()
      .mockRejectedValueOnce(new Error('unsupported by running server'))
      .mockRejectedValueOnce(new Error('still unsupported'))
      .mockReturnValueOnce(pending.promise);
    const refresh = vi.fn().mockResolvedValue(undefined);
    const wait = vi.fn().mockResolvedValue(undefined);
    const stop = startProjectChangePoll({ client: { pollEvents }, refresh, wait });
    await vi.waitFor(() => { expect(pollEvents).toHaveBeenCalledTimes(3); });
    expect(refresh).not.toHaveBeenCalled();
    expect(wait.mock.calls.map(call => call[0])).toEqual([500, 1_000]);
    stop();
    pending.resolve(response(0));
  });
});
