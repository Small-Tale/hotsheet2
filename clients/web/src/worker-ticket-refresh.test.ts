import { describe, expect, it, vi } from 'vitest';

import type { FullTicket } from './api';
import { createRefreshBarrier } from './refresh-barrier';
import { createTicketDetailCache } from './ticket-detail-cache';
import { createWorkerTicketRefresh, reconcileWorkerTicketSelection } from './worker-ticket-refresh';

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

const ticket = (qualifiedId = 'other:01', revision = 'one') =>
  ({
    id: '01',
    slug: 'HS2-SAME',
    qualified_id: qualifiedId,
    updated_at: revision,
    concurrency_token: revision,
    title: revision,
    details: 'Original details',
    notes: [],
    attachments: [],
  }) as unknown as FullTicket;

function owner() {
  const barrier = createRefreshBarrier(),
    load = vi.fn<(id: string) => Promise<FullTicket>>(),
    publish = vi.fn();
  const refresh = createWorkerTicketRefresh({ load, publish, waitUntilSafe: () => barrier.wait() });
  return { barrier, load, publish, refresh };
}

describe('HS2-J118P3 authoritative worker detail publication', () => {
  it('publishes changed cursors once and does not request on idle repeated snapshots', async () => {
    const { load, publish, refresh } = owner();
    load.mockResolvedValue(ticket());
    refresh.request('other:01', 'c1');
    await vi.waitFor(() => {
      expect(publish).toHaveBeenCalledTimes(1);
    });
    refresh.request('other:01', 'c1');
    expect(load).toHaveBeenCalledTimes(1);
    load.mockResolvedValue(ticket('other:01', 'two'));
    refresh.request('other:01', 'c2');
    await vi.waitFor(() => {
      expect(publish).toHaveBeenLastCalledWith(ticket('other:01', 'two'));
    });
    expect(load).toHaveBeenCalledTimes(2);
    refresh.dispose();
  });

  it('coalesces interleaved cursors and never publishes an out-of-date response', async () => {
    const { load, publish, refresh } = owner(),
      first = deferred<FullTicket>();
    load.mockReturnValueOnce(first.promise).mockResolvedValue(ticket('other:01', 'three'));
    refresh.request('other:01', 'c1');
    await vi.waitFor(() => {
      expect(load).toHaveBeenCalledTimes(1);
    });
    refresh.request('other:01', 'c2');
    refresh.request('other:01', 'c3');
    first.resolve(ticket());
    await vi.waitFor(() => {
      expect(publish).toHaveBeenCalledTimes(1);
    });
    expect(publish).toHaveBeenCalledWith(ticket('other:01', 'three'));
    expect(load).toHaveBeenCalledTimes(2);
    refresh.dispose();
  });

  it('waits for an existing save before reading the authoritative detail', async () => {
    const { barrier, load, publish, refresh } = owner(),
      release = barrier.begin();
    load.mockResolvedValue(ticket());
    refresh.request('other:01', 'c1');
    await Promise.resolve();
    expect(load).not.toHaveBeenCalled();
    release();
    await vi.waitFor(() => {
      expect(publish).toHaveBeenCalledTimes(1);
    });
    refresh.dispose();
  });

  it('rejects a read overlapping a local save and retries after it without a new cursor', async () => {
    const { barrier, load, publish, refresh } = owner(),
      first = deferred<FullTicket>();
    load.mockReturnValueOnce(first.promise).mockResolvedValue(ticket('other:01', 'saved'));
    refresh.request('other:01', 'c1');
    await vi.waitFor(() => {
      expect(load).toHaveBeenCalledTimes(1);
    });
    const release = barrier.begin();
    refresh.invalidate();
    first.resolve(ticket('other:01', 'old'));
    await first.promise;
    expect(publish).not.toHaveBeenCalled();
    expect(load).toHaveBeenCalledTimes(1);
    release();
    await vi.waitFor(() => {
      expect(publish).toHaveBeenCalledWith(ticket('other:01', 'saved'));
    });
    expect(publish).toHaveBeenCalledTimes(1);
    expect(load).toHaveBeenCalledTimes(2);
    refresh.dispose();
  });

  it('rechecks the barrier if a save begins between waiting and starting the read', async () => {
    const { barrier, load, publish, refresh } = owner();
    load.mockResolvedValue(ticket());
    refresh.request('other:01', 'c1');
    const release = barrier.begin();
    refresh.invalidate();
    await Promise.resolve();
    await Promise.resolve();
    expect(load).not.toHaveBeenCalled();
    release();
    await vi.waitFor(() => {
      expect(publish).toHaveBeenCalledTimes(1);
    });
    refresh.dispose();
  });

  it('cannot overwrite a newer ordinary-event refresh with an older worker response', async () => {
    const first = deferred<FullTicket>(),
      newer = ticket('other:01', 'ordinary-new'),
      load = vi.fn().mockReturnValueOnce(first.promise).mockResolvedValue(newer),
      publications: FullTicket[] = [];
    let selected = ticket();
    const refresh = createWorkerTicketRefresh({
      load,
      waitUntilSafe: async () => undefined,
      publish: (next) => {
        publications.push(next);
        selected = next;
      },
    });
    refresh.request('other:01', 'c1');
    await vi.waitFor(() => {
      expect(load).toHaveBeenCalledTimes(1);
    });
    // The normal project refresh invalidates outstanding worker reads when it reconciles.
    expect(selected.title).toBe('one');
    refresh.invalidate();
    selected = newer;
    first.resolve(ticket('other:01', 'old'));
    await vi.waitFor(() => {
      expect(load).toHaveBeenCalledTimes(2);
    });
    await vi.waitFor(() => {
      expect(publications).toEqual([newer]);
    });
    expect(selected).toBe(newer);
    refresh.dispose();
  });

  it('keeps failed details until the next changed cursor rather than retrying on a timer', async () => {
    const { load, publish, refresh } = owner();
    load.mockRejectedValueOnce(new Error('offline')).mockResolvedValue(ticket());
    refresh.request('other:01', 'c1');
    await vi.waitFor(() => {
      expect(load).toHaveBeenCalledTimes(1);
    });
    refresh.request('other:01', 'c1');
    expect(publish).not.toHaveBeenCalled();
    refresh.request('other:01', 'c2');
    await vi.waitFor(() => {
      expect(publish).toHaveBeenCalledTimes(1);
    });
    refresh.dispose();
  });

  it('discards disposed responses and cannot refill a later project with them', async () => {
    const { load, publish, refresh } = owner(),
      first = deferred<FullTicket>();
    load.mockReturnValue(first.promise);
    refresh.request('other:01', 'c1');
    await vi.waitFor(() => {
      expect(load).toHaveBeenCalledTimes(1);
    });
    refresh.dispose();
    refresh.invalidate();
    refresh.request('other:01', 'c2');
    first.resolve(ticket());
    await first.promise;
    expect(publish).not.toHaveBeenCalled();
    expect(load).toHaveBeenCalledTimes(1);
  });

  it('disposes while waiting for a save and rejects a response with another qualified identity', async () => {
    const first = owner(),
      release = first.barrier.begin();
    first.refresh.request('other:01', 'c1');
    first.refresh.dispose();
    release();
    await Promise.resolve();
    expect(first.load).not.toHaveBeenCalled();
    const second = owner();
    second.load.mockResolvedValue(ticket('local:01'));
    second.refresh.request('other:01', 'c1');
    await vi.waitFor(() => {
      expect(second.load).toHaveBeenCalledTimes(1);
    });
    expect(second.publish).not.toHaveBeenCalled();
    second.refresh.dispose();
  });

  it('remembers details but reconciles only the exact current project and qualified selection', () => {
    const cache = createTicketDetailCache(),
      refreshed = ticket('other:01', 'new'),
      reconcile = vi.fn();
    for (const [selectedProjectId, selectedTicket] of [
      ['p', ticket('local:01')],
      ['another-project', ticket('other:01')],
      ['p', null],
    ] as const)
      reconcileWorkerTicketSelection({
        projectId: 'p',
        ticket: refreshed,
        selectedProjectId,
        selectedTicket,
        cache,
        reconcile,
      });
    expect(reconcile).not.toHaveBeenCalled();
    expect(cache.get('p', refreshed)?.ticket).toBe(refreshed);
    const previous = ticket();
    reconcileWorkerTicketSelection({
      projectId: 'p',
      ticket: refreshed,
      selectedProjectId: 'p',
      selectedTicket: previous,
      cache,
      reconcile,
    });
    expect(reconcile).toHaveBeenCalledExactlyOnceWith(previous, refreshed);
  });
});
