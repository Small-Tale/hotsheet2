/**
 * The remembered order of the whole project tab strip, local and remote tabs together (HS2-NRBWGF).
 *
 * `hotsheet.open-projects` lists only local roots, because it is also the list of projects to reopen,
 * and remote tabs come from this machine's registry in its own saved order. Ranked separately, every
 * remote tab sorted after every local one on reload, so a strip the user had mixed by dragging came
 * back regrouped. A drag now also saves this combined order (local roots and remote project ids), and
 * startup ranks both kinds of tab by it, falling back to the old ranks for a tab it does not name.
 */

export const PROJECT_TAB_ORDER_KEY = 'hotsheet.project-tab-order.v1';

/** The saved combined order, or empty when none was saved or it cannot be read. */
export function loadProjectTabOrder(storage: Pick<Storage, 'getItem'>): string[] {
  try {
    const value: unknown = JSON.parse(storage.getItem(PROJECT_TAB_ORDER_KEY) ?? '[]');
    return Array.isArray(value)
      ? [...new Set(value.filter((item): item is string => typeof item === 'string' && item !== ''))]
      : [];
  } catch {
    return [];
  }
}

export function saveProjectTabOrder(storage: Pick<Storage, 'setItem'>, keys: readonly string[]): void {
  try {
    storage.setItem(PROJECT_TAB_ORDER_KEY, JSON.stringify([...new Set(keys)]));
  } catch {
    // Storage can be full or blocked; the strip still works with the fallback order.
  }
}

/**
 * A rank function over the saved order: a key it names ranks by its position; any other key ranks
 * after all of them by its `fallback` (its old local index, or a remote tab's registry rank), so the
 * two fallback spaces keep their relative order.
 */
export function projectTabRanks(order: readonly string[]): (key: string, fallback: number) => number {
  const position = new Map(order.map((key, index) => [key, index]));
  return (key, fallback) => position.get(key) ?? order.length + fallback;
}
