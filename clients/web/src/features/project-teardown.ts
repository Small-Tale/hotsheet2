import type { Signal } from 'kerfjs';

/**
 * Pure helpers behind closing project tabs (HS2-DCTS07): dropping every per-project record for the
 * closing ids and choosing which remaining tab takes the selection. The orchestration (closing server
 * sessions, activating the next project, refreshing) stays with the application runtime.
 */

/** Replace each per-project record with a copy that omits the closing project ids, in order. */
export function pruneProjectRecords(
  records: ReadonlyArray<Signal<Record<string, unknown>>>,
  closing: ReadonlySet<string>,
): void {
  for (const record of records)
    record.value = Object.fromEntries(Object.entries(record.value).filter(([id]) => !closing.has(id)));
}

/**
 * The project tab that takes the selection after the selected one closes: the nearest surviving tab
 * to its right, then the nearest to its left, else the first remaining tab, else none (`''`).
 */
export function nextProjectAfterClose<T extends { id: string }>(
  before: readonly T[],
  remaining: readonly T[],
  selectedIndex: number,
): string {
  return (
    remaining.find((item) => before.indexOf(item) > selectedIndex)?.id ??
    [...remaining].reverse().find((item) => before.indexOf(item) < selectedIndex)?.id ??
    remaining.at(0)?.id ??
    ''
  );
}
