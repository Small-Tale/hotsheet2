import { describe, expect, it, vi } from 'vitest';

import type { TicketRow } from './api';
import { createProjectTabRefreshCoordinator } from './project-tab-refresh';

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>(next => { resolve = next; });
  return { promise, resolve };
}

const ticket = (id: string): TicketRow => ({
  connection_id: 'git', native_id: id, qualified_id: `git:${id}`, id,
  slug: `HS2-${id}`, title: id, status: 'not_started', up_next: true,
  feedback_needed: false, tags: [], blocked_by: [], claim_count: 0,
});

describe('project tab refresh coordination', () => {
  it('publishes authoritative rows for every non-active project while routing the active project through its full refresh', async () => {
    let active = 'alpha';
    const publishBackground = vi.fn();
    const refreshActive = vi.fn().mockResolvedValue(undefined);
    const loadBackground = vi.fn(async (target: { id: string }) => [ticket(target.id)]);
    const coordinator = createProjectTabRefreshCoordinator({
      waitUntilSafe: async () => undefined,
      activeProjectId: () => active,
      isOpen: () => true,
      refreshActive,
      loadBackground,
      publishBackground,
    });

    await coordinator.request({ id: 'beta' });
    expect(loadBackground).toHaveBeenCalledWith({ id: 'beta' });
    expect(publishBackground).toHaveBeenCalledWith({ id: 'beta' }, [ticket('beta')]);
    active = 'beta';
    await coordinator.request({ id: 'beta' });
    expect(refreshActive).toHaveBeenCalledWith({ id: 'beta' });
    expect(loadBackground).toHaveBeenCalledTimes(1);
  });

  it('keeps distinct project invalidations and coalesces repeated work while refresh is unsafe', async () => {
    const safe = deferred<undefined>();
    const loadBackground = vi.fn(async (target: { id: string }) => [ticket(target.id)]);
    const publishBackground = vi.fn();
    const coordinator = createProjectTabRefreshCoordinator({
      waitUntilSafe: () => safe.promise,
      activeProjectId: () => 'alpha',
      isOpen: () => true,
      refreshActive: vi.fn(),
      loadBackground,
      publishBackground,
    });

    const first = coordinator.request({ id: 'beta' });
    const repeated = coordinator.request({ id: 'beta' });
    const other = coordinator.request({ id: 'gamma' });
    expect(loadBackground).not.toHaveBeenCalled();
    safe.resolve(undefined);
    await Promise.all([first, repeated, other]);
    expect(loadBackground.mock.calls.map(([target]) => target.id).sort()).toEqual(['beta', 'gamma']);
    expect(publishBackground.mock.calls.map(([target]) => target.id).sort()).toEqual(['beta', 'gamma']);
  });

  it('rejects an obsolete background response after project activation or closure', async () => {
    let active = 'alpha';
    let open = true;
    const pending = deferred<TicketRow[] | undefined>();
    const publishBackground = vi.fn();
    const refreshActive = vi.fn().mockResolvedValue(undefined);
    const coordinator = createProjectTabRefreshCoordinator({
      waitUntilSafe: async () => undefined,
      activeProjectId: () => active,
      isOpen: () => open,
      refreshActive,
      loadBackground: () => pending.promise,
      publishBackground,
    });

    const refresh = coordinator.request({ id: 'beta' });
    active = 'beta';
    pending.resolve([ticket('new')]);
    await refresh;
    expect(refreshActive).toHaveBeenCalledOnce();
    expect(publishBackground).not.toHaveBeenCalled();

    const closedPending = deferred<TicketRow[] | undefined>();
    const closedCoordinator = createProjectTabRefreshCoordinator({
      waitUntilSafe: async () => undefined,
      activeProjectId: () => 'alpha',
      isOpen: () => open,
      refreshActive,
      loadBackground: () => closedPending.promise,
      publishBackground,
    });
    const closedRefresh = closedCoordinator.request({ id: 'gamma' });
    open = false;
    closedCoordinator.cancel('gamma');
    closedPending.resolve([ticket('stale')]);
    await closedRefresh;
    expect(publishBackground).not.toHaveBeenCalled();
  });
});
