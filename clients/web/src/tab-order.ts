export type TabDropPosition = 'before' | 'after';

export function reorderTabs<T>(
  items: readonly T[],
  id: (item: T) => string,
  sourceId: string,
  targetId: string,
  position: TabDropPosition,
): T[] {
  if (sourceId === targetId) return [...items];
  const source = items.find((item) => id(item) === sourceId),
    targetIndex = items.findIndex((item) => id(item) === targetId);
  if (!source || targetIndex < 0) return [...items];
  const remaining = items.filter((item) => id(item) !== sourceId),
    adjustedTarget = remaining.findIndex((item) => id(item) === targetId);
  remaining.splice(adjustedTarget + (position === 'after' ? 1 : 0), 0, source);
  return remaining;
}

/** Apply a remembered partial order while keeping newly discovered tabs in source order. */
export function applyRememberedTabOrder<T>(
  items: readonly T[],
  id: (item: T) => string,
  remembered: readonly string[],
): T[] {
  const rank = new Map(remembered.map((value, index) => [value, index]));
  return items
    .map((item, index) => ({ item, index, rank: rank.get(id(item)) }))
    .sort((left, right) => {
      if (left.rank === undefined && right.rank === undefined) return left.index - right.index;
      if (left.rank === undefined) return 1;
      if (right.rank === undefined) return -1;
      return left.rank - right.rank;
    })
    .map((entry) => entry.item);
}

export function replaceTabInPlace<T>(items: readonly T[], id: (item: T) => string, next: T): T[] {
  const index = items.findIndex((item) => id(item) === id(next));
  if (index < 0) return [...items, next];
  return items.map((item, itemIndex) => (itemIndex === index ? next : item));
}

/**
 * Replace a tab in place, or insert a new one before the first tab with a greater restore rank
 * (unranked tabs sort last). Projects registered out of order during startup therefore land in
 * their remembered positions without later reordering (HS2-X74D4B).
 */
export function insertTabByRank<T>(
  items: readonly T[],
  id: (item: T) => string,
  next: T,
  rank: number,
  rankOf: (item: T) => number | undefined,
): T[] {
  if (items.some((item) => id(item) === id(next))) return replaceTabInPlace(items, id, next);
  const before = items.findIndex((item) => {
    const other = rankOf(item);
    return other === undefined || other > rank;
  });
  if (before < 0) return [...items, next];
  return [...items.slice(0, before), next, ...items.slice(before)];
}

/**
 * Merge ranked pending entries into an ordered list: each goes before the first item whose rank is
 * greater or unknown, preserving the relative order of both inputs. Startup uses it to place
 * still-opening project placeholders (and their remembered roots) at their remembered positions
 * among registered projects (HS2-2BEJXD).
 */
export function interleaveByRank<T>(
  items: readonly T[],
  rankOf: (item: T) => number | undefined,
  pending: readonly { rank: number; item: T }[],
): T[] {
  const queue = [...pending].sort((left, right) => left.rank - right.rank),
    merged: T[] = [];
  for (const item of items) {
    const rank = rankOf(item);
    while (queue.length && (rank === undefined || queue[0].rank < rank)) merged.push(queue.shift()!.item);
    merged.push(item);
  }
  return [...merged, ...queue.map((entry) => entry.item)];
}
