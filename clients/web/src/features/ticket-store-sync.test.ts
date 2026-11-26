import { describe, expect, it, vi } from 'vitest';

import { ApiHttpError, type TicketStoreSyncReport, type TicketStoreSyncState } from '../api';
import type { Project } from '../interactions/types';
import { createTicketStoreSync } from './ticket-store-sync';

const PROJECT = { id: 'web-1a2b3c', apiPath: '/api' } as Project;

function report(state: TicketStoreSyncState): TicketStoreSyncReport {
  return {
    checkout_id: PROJECT.id,
    stores: [
      {
        connection_id: 'git:web',
        label: 'web.hs2',
        locator: '/work/web.hs2',
        health: { state, syncing: false, unpublished_commits: 0, incoming_commits: 0, consecutive_failures: 0 },
      },
    ],
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((done, fail) => {
    resolve = done;
    reject = fail;
  });
  return { promise, resolve, reject };
}

describe('createTicketStoreSync (HS2-ADHFKZ)', () => {
  it('loads a project once on first show, then only on explicit reloads', async () => {
    const request = vi.fn(async () => report('rejected'));
    const sync = createTicketStoreSync(request);
    sync.loadOnce(PROJECT);
    sync.loadOnce(PROJECT);
    await vi.waitFor(() => {
      expect(sync.report(PROJECT.id)?.stores[0]?.health.state).toBe('rejected');
    });
    expect(request).toHaveBeenCalledTimes(1);

    request.mockResolvedValueOnce(report('synced'));
    await sync.load(PROJECT);
    expect(sync.report(PROJECT.id)?.stores[0]?.health.state).toBe('synced');
    expect(request).toHaveBeenCalledTimes(2);
  });

  it('keeps the last report when a reload fails', async () => {
    const request = vi.fn(async () => report('conflict'));
    const failures: unknown[] = [];
    const sync = createTicketStoreSync(request, {
      delay: () => Promise.resolve(),
      onPersistentFailure: (_target, reason) => void failures.push(reason),
    });
    await sync.load(PROJECT);
    request.mockRejectedValue(new Error('offline'));
    await sync.load(PROJECT);
    expect(sync.report(PROJECT.id)?.stores[0]?.health.state).toBe('conflict');
    expect(failures).toHaveLength(1);
  });

  it('ignores a stale response that resolves after a newer one', async () => {
    const first = deferred<TicketStoreSyncReport>();
    const second = deferred<TicketStoreSyncReport>();
    const request = vi.fn().mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
    const sync = createTicketStoreSync(request);
    const older = sync.load(PROJECT);
    const newer = sync.load(PROJECT);
    second.resolve(report('synced'));
    await newer;
    first.resolve(report('rejected'));
    await older;
    expect(sync.report(PROJECT.id)?.stores[0]?.health.state).toBe('synced');
  });

  it('drops a response requested before a reset that lands after a fresh load', async () => {
    const stale = deferred<TicketStoreSyncReport>();
    const fresh = deferred<TicketStoreSyncReport>();
    const request = vi.fn().mockReturnValueOnce(stale.promise).mockReturnValueOnce(fresh.promise);
    const sync = createTicketStoreSync(request);
    const before = sync.load(PROJECT);
    sync.reset();
    const after = sync.load(PROJECT);
    fresh.resolve(report('up_to_date'));
    await after;
    stale.resolve(report('rejected'));
    await before;
    expect(sync.report(PROJECT.id)?.stores[0]?.health.state).toBe('up_to_date');
  });

  it('reset forgets reports and allows a fresh first load', async () => {
    const request = vi.fn(async () => report('offline'));
    const sync = createTicketStoreSync(request);
    await sync.load(PROJECT);
    sync.loadOnce(PROJECT);
    sync.reset();
    expect(sync.report(PROJECT.id)).toBeUndefined();
    sync.loadOnce(PROJECT);
    await vi.waitFor(() => {
      expect(sync.report(PROJECT.id)).toBeDefined();
    });
    expect(request).toHaveBeenCalledTimes(3);
  });
});

describe('retrying a failed sync-health load (HS2-JGFPG4)', () => {
  function harness(options: { hold?: boolean } = {}) {
    const delays: number[] = [],
      held: Array<() => void> = [],
      failures: Array<{ project: string; reason: unknown }> = [];
    const delay = (ms: number, abort: AbortSignal) => {
      delays.push(ms);
      if (!options.hold) return Promise.resolve();
      return new Promise<void>((resolve) => {
        held.push(resolve);
        abort.addEventListener(
          'abort',
          () => {
            resolve();
          },
          { once: true },
        );
      });
    };
    const onPersistentFailure = (target: Project, reason: unknown) =>
      void failures.push({ project: target.id, reason });
    return { delays, held, failures, options: { delay, onPersistentFailure } };
  }

  it('shows the banner after a transient first-load failure recovers', async () => {
    const h = harness();
    const request = vi
      .fn<(target: Project) => Promise<TicketStoreSyncReport>>()
      .mockRejectedValueOnce(new TypeError('network'))
      .mockRejectedValueOnce(new ApiHttpError('busy', 503))
      .mockResolvedValue(report('rejected'));
    const sync = createTicketStoreSync(request, h.options);
    sync.loadOnce(PROJECT);
    await vi.waitFor(() => {
      expect(sync.report(PROJECT.id)?.stores[0]?.health.state).toBe('rejected');
    });
    expect(request).toHaveBeenCalledTimes(3);
    expect(h.delays).toEqual([1_000, 2_000]);
    expect(h.failures).toEqual([]);
  });

  it('reports a failure that persists once, and asks again when the project is shown again', async () => {
    const h = harness();
    const request = vi
      .fn<(target: Project) => Promise<TicketStoreSyncReport>>()
      .mockRejectedValue(new TypeError('network'));
    const sync = createTicketStoreSync(request, h.options);
    sync.loadOnce(PROJECT);
    await vi.waitFor(() => {
      expect(h.failures).toHaveLength(1);
    });
    expect(request).toHaveBeenCalledTimes(5);
    expect(h.delays).toEqual([1_000, 2_000, 4_000, 8_000]);
    expect(h.failures[0]).toMatchObject({ project: PROJECT.id });
    expect(sync.report(PROJECT.id)).toBeUndefined();
    // The failed first show is forgotten: showing the project again asks again and fills the banner.
    request.mockResolvedValue(report('synced'));
    sync.loadOnce(PROJECT);
    await vi.waitFor(() => {
      expect(sync.report(PROJECT.id)?.stores[0]?.health.state).toBe('synced');
    });
    expect(request).toHaveBeenCalledTimes(6);
    expect(h.failures).toHaveLength(1);
  });

  it('reports a refusal at once without asking again', async () => {
    const h = harness();
    const refusal = new ApiHttpError('no checkout', 404, 'checkout_not_found');
    const request = vi.fn<(target: Project) => Promise<TicketStoreSyncReport>>().mockRejectedValue(refusal);
    const sync = createTicketStoreSync(request, h.options);
    await sync.load(PROJECT);
    expect(request).toHaveBeenCalledTimes(1);
    expect(h.delays).toEqual([]);
    expect(h.failures).toEqual([{ project: PROJECT.id, reason: refusal }]);
  });

  it('ends a waiting retry quietly on reset, and a fresh first load fills again', async () => {
    const h = harness({ hold: true });
    const request = vi
      .fn<(target: Project) => Promise<TicketStoreSyncReport>>()
      .mockRejectedValueOnce(new TypeError('network'))
      .mockResolvedValue(report('offline'));
    const sync = createTicketStoreSync(request, h.options);
    const waiting = sync.load(PROJECT);
    await vi.waitFor(() => {
      expect(h.delays).toEqual([1_000]);
    });
    sync.reset();
    await waiting;
    expect(request).toHaveBeenCalledTimes(1);
    expect(sync.report(PROJECT.id)).toBeUndefined();
    expect(h.failures).toEqual([]);
    sync.loadOnce(PROJECT);
    await vi.waitFor(() => {
      expect(sync.report(PROJECT.id)?.stores[0]?.health.state).toBe('offline');
    });
    expect(request).toHaveBeenCalledTimes(2);
  });

  it('lets a newer load own the project while an older one waits to retry', async () => {
    const h = harness({ hold: true });
    const request = vi
      .fn<(target: Project) => Promise<TicketStoreSyncReport>>()
      .mockRejectedValueOnce(new TypeError('network'))
      .mockResolvedValueOnce(report('synced'))
      .mockResolvedValue(report('rejected'));
    const sync = createTicketStoreSync(request, h.options);
    const older = sync.load(PROJECT);
    await vi.waitFor(() => {
      expect(h.delays).toEqual([1_000]);
    });
    await sync.load(PROJECT);
    expect(sync.report(PROJECT.id)?.stores[0]?.health.state).toBe('synced');
    // The older load's wait ends; it sees it was superseded and neither asks again nor reports.
    for (const release of h.held.splice(0)) release();
    await older;
    expect(request).toHaveBeenCalledTimes(2);
    expect(sync.report(PROJECT.id)?.stores[0]?.health.state).toBe('synced');
    expect(h.failures).toEqual([]);
  });
});
