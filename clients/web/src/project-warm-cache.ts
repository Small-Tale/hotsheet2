/**
 * Default number of open projects whose loaded workspace projection (ticket rows, cursors, corrupt
 * tickets, repository, commands, AI tool inventory) stays resident so a project tab switch paints
 * instantly (HS2-AZZ9TF). The active project always counts toward the bound.
 */
export const PROJECT_WARM_CACHE_CAPACITY = 8;

/**
 * Bounded least-recently-used membership set of "warm" project ids. It stores no data itself: the
 * owner keeps per-project caches and drops a project's entries when {@link ProjectWarmCache.touch}
 * reports it evicted. Background prefetches use {@link ProjectWarmCache.admit}, which fills spare
 * capacity without displacing a project the user actually visited.
 */
export interface ProjectWarmCache {
  readonly capacity: number;
  /** Mark `id` most recently used (e.g. it was activated). Returns the ids evicted to stay in bounds. */
  touch(id: string): string[];
  /** Add `id` as least recently used only when there is spare capacity. Returns whether it is warm. */
  admit(id: string): boolean;
  has(id: string): boolean;
  /** Drop `id` (e.g. its project was closed). */
  forget(id: string): void;
  /** Warm ids, most recently used first. */
  ids(): string[];
}

export function createProjectWarmCache(capacity = PROJECT_WARM_CACHE_CAPACITY): ProjectWarmCache {
  const bound = Math.max(1, Math.trunc(capacity));
  // Most recently used first.
  let order: string[] = [];
  return {
    capacity: bound,
    touch(id) {
      order = [id, ...order.filter((item) => item !== id)];
      return order.length > bound ? order.splice(bound) : [];
    },
    admit(id) {
      if (order.includes(id)) return true;
      if (order.length >= bound) return false;
      order.push(id);
      return true;
    },
    has(id) {
      return order.includes(id);
    },
    forget(id) {
      order = order.filter((item) => item !== id);
    },
    ids() {
      return [...order];
    },
  };
}
