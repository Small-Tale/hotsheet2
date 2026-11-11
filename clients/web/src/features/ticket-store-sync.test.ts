import { describe, expect, it, vi } from 'vitest';

import type { TicketStoreSyncReport, TicketStoreSyncState } from '../api';
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
    const sync = createTicketStoreSync(request);
    await sync.load(PROJECT);
    request.mockRejectedValueOnce(new Error('offline'));
    await sync.load(PROJECT);
    expect(sync.report(PROJECT.id)?.stores[0]?.health.state).toBe('conflict');
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
