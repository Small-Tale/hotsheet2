import type { TicketRow } from './api';

export interface ProjectTabRefreshTarget {
  id: string;
}

export interface ProjectTabRefreshCoordinatorOptions<Target extends ProjectTabRefreshTarget> {
  waitUntilSafe(): Promise<void>;
  activeProjectId(): string;
  isOpen(target: Target): boolean;
  refreshActive(target: Target): Promise<void>;
  loadBackground(target: Target): Promise<TicketRow[] | undefined>;
  publishBackground(target: Target, tickets: TicketRow[]): void;
}

export interface ProjectTabRefreshCoordinator<Target extends ProjectTabRefreshTarget> {
  request(target: Target): Promise<void>;
  cancel(projectId: string): void;
}

/** Coalesce per-project invalidations while preserving refreshes for every open tab. */
export function createProjectTabRefreshCoordinator<Target extends ProjectTabRefreshTarget>(options: ProjectTabRefreshCoordinatorOptions<Target>): ProjectTabRefreshCoordinator<Target> {
  const pending = new Map<string, Target>();
  const revisions = new Map<string, number>();
  let running: Promise<void> | undefined;

  const revision = (projectId: string) => revisions.get(projectId) ?? 0;
  const refresh = async (target: Target, expectedRevision: number) => {
    if (revision(target.id) !== expectedRevision || !options.isOpen(target)) return;
    if (options.activeProjectId() === target.id) {
      await options.refreshActive(target);
      return;
    }
    const tickets = await options.loadBackground(target);
    if (revision(target.id) !== expectedRevision || !tickets || !options.isOpen(target)) return;
    if (options.activeProjectId() === target.id) {
      await options.refreshActive(target);
      return;
    }
    options.publishBackground(target, tickets);
  };

  const drain = async () => {
    await options.waitUntilSafe();
    while (pending.size > 0) {
      const batch = [...pending.values()].map(target => ({ target, revision: revision(target.id) }));
      pending.clear();
      await Promise.all(batch.map(item => refresh(item.target, item.revision)));
    }
  };

  return {
    request(target) {
      revisions.set(target.id, revision(target.id) + 1);
      pending.set(target.id, target);
      running ??= drain().finally(() => { running = undefined; });
      return running;
    },
    cancel(projectId) {
      revisions.set(projectId, revision(projectId) + 1);
      pending.delete(projectId);
    },
  };
}
