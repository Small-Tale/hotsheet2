export interface ProjectTabRefreshTarget {
  id: string;
}

export interface ProjectTabRefreshCoordinatorOptions<Target extends ProjectTabRefreshTarget, Snapshot> {
  waitUntilSafe(): Promise<void>;
  activeProjectId(): string;
  isOpen(target: Target): boolean;
  refreshActive(target: Target): Promise<void>;
  loadBackground(target: Target): Promise<Snapshot | undefined>;
  publishBackground(target: Target, snapshot: Snapshot): void;
}

export interface ProjectTabRefreshCoordinator<Target extends ProjectTabRefreshTarget> {
  request(target: Target): Promise<void>;
  activate(projectId: string): void;
  cancel(projectId: string): void;
}

/** Coalesce per-project invalidations while preserving refreshes for every open tab. */
export function createProjectTabRefreshCoordinator<Target extends ProjectTabRefreshTarget, Snapshot>(
  options: ProjectTabRefreshCoordinatorOptions<Target, Snapshot>,
): ProjectTabRefreshCoordinator<Target> {
  const pending = new Map<string, Target>();
  const revisions = new Map<string, number>();
  const waiters = new Map<
    string,
    Array<{ revision: number; resolve: () => void; reject: (reason: unknown) => void }>
  >();
  let running: Promise<void> | undefined;

  const revision = (projectId: string) => revisions.get(projectId) ?? 0;
  const settle = (projectId: string, throughRevision: number, reason?: unknown) => {
    const queued = waiters.get(projectId);
    if (!queued) return;
    const remaining = queued.filter((waiter) => {
      if (waiter.revision > throughRevision) return true;
      if (reason === undefined) waiter.resolve();
      else waiter.reject(reason);
      return false;
    });
    if (remaining.length) waiters.set(projectId, remaining);
    else waiters.delete(projectId);
  };
  const refresh = async (target: Target, expectedRevision: number): Promise<boolean> => {
    if (revision(target.id) !== expectedRevision) return false;
    if (!options.isOpen(target)) return true;
    if (options.activeProjectId() === target.id) {
      await options.refreshActive(target);
      return true;
    }
    const snapshot = await options.loadBackground(target);
    if (revision(target.id) !== expectedRevision) return false;
    if (!snapshot || !options.isOpen(target)) return true;
    if (options.activeProjectId() === target.id) {
      await options.refreshActive(target);
      return true;
    }
    options.publishBackground(target, snapshot);
    return true;
  };

  const drain = async () => {
    await options.waitUntilSafe();
    while (pending.size > 0) {
      const batch = [...pending.values()].map((target) => ({ target, revision: revision(target.id) }));
      pending.clear();
      await Promise.all(
        batch.map(async (item) => {
          try {
            if (await refresh(item.target, item.revision)) settle(item.target.id, item.revision);
          } catch (reason) {
            settle(item.target.id, item.revision, reason);
          }
        }),
      );
    }
  };
  const ensureRunning = () => {
    if (running) return;
    const cycle = drain()
      .catch((reason: unknown) => {
        pending.clear();
        for (const [projectId] of waiters) settle(projectId, Number.POSITIVE_INFINITY, reason);
      })
      .finally(() => {
        if (running !== cycle) return;
        running = undefined;
        // A request can arrive after `drain` observes an empty map but before this
        // finalizer clears the runner. Start its successor instead of stranding it.
        if (pending.size > 0) ensureRunning();
      });
    running = cycle;
  };

  return {
    request(target) {
      const expectedRevision = revision(target.id) + 1;
      revisions.set(target.id, expectedRevision);
      pending.set(target.id, target);
      const completed = new Promise<void>((resolve, reject) => {
        const queued = waiters.get(target.id) ?? [];
        queued.push({ revision: expectedRevision, resolve, reject });
        waiters.set(target.id, queued);
      });
      ensureRunning();
      return completed;
    },
    activate(projectId) {
      // An activation performs its own authoritative refresh. Invalidate any
      // older background load so it cannot publish after the user switches
      // away again and make that freshly cached projection stale.
      revisions.set(projectId, revision(projectId) + 1);
      pending.delete(projectId);
      settle(projectId, revision(projectId));
    },
    cancel(projectId) {
      revisions.set(projectId, revision(projectId) + 1);
      pending.delete(projectId);
      settle(projectId, revision(projectId));
    },
  };
}
