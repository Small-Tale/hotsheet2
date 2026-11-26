import type { IconNode } from 'lucide';

import type { ConfirmationOptions } from './confirmation';

/**
 * Minimized dialogs (HS2-3JR0ZH). Several dialog kinds can be minimized into the one Minimized
 * Tickets control (`components/minimized-tickets-control.tsx`): ticket readers (HS2-Q8WF64) and the
 * new-ticket composer. Each kind keeps its own records, drafts, and persistence and implements
 * `MinimizedDialogKind`; the shared controller (`features/minimized-dialogs.ts`) merges their items
 * per project tab, routes Restore, Restore All, and Close All, and asks before Close All discards a
 * draft. `docs/06-clients.md` §6.4.6 lists the steps for nominating another dialog.
 */

/** The Lucide icon a minimized item leads with in the control's menu. */
export interface MinimizedDialogIcon {
  icon: IconNode;
  name: string;
}

/** One minimized dialog as the Minimized Tickets control lists it. */
export interface MinimizedDialogItem {
  /** Unique across every kind; Restore names the item by it. */
  id: string;
  /** The kind id of the `MinimizedDialogKind` that owns the item. */
  kind: string;
  /** The full name, such as "HS2-ABC123 Title" or "New ticket: Title". */
  label: string;
  /** The short name the single-item button's tooltip uses: "Restore <shortLabel>". */
  shortLabel: string;
  icon: MinimizedDialogIcon;
  /** The draft indicator, such as "Unsent note", when closing the item would lose a draft. */
  draft?: string;
  /** When it was minimized; the control lists items oldest first. */
  minimizedAt: number;
}

/**
 * A dialog kind that can be minimized (HS2-3JR0ZH). A kind owns its records and their storage and
 * answers for one project tab at a time; the shared controller never reads a kind's records.
 */
export interface MinimizedDialogKind {
  /** A stable id, such as `ticket-reader` or `new-ticket`. */
  readonly kind: string;
  /** The project tab's minimized items of this kind, each with its draft indicator. */
  items(tabProjectId: string): readonly MinimizedDialogItem[];
  /** Reopen these items, oldest first, so the last one is on top; `false` when none reopened. */
  restore(ids: readonly string[]): Promise<boolean>;
  /** Close these items and discard their drafts; the caller has already confirmed. */
  discard(ids: readonly string[]): void;
  /** Closing project tabs drops the items that belong to them (HS2-BSSWGV teardown). */
  dispose(projectIds: readonly string[]): void;
}

/** Every kind's minimized items of one project tab, oldest first (ties keep kind order). */
export function minimizedItemsForTab(
  kinds: readonly MinimizedDialogKind[],
  tabProjectId: string,
): MinimizedDialogItem[] {
  return kinds
    .flatMap((kind) => kind.items(tabProjectId))
    .map((item, index) => ({ item, index }))
    .sort((a, b) => a.item.minimizedAt - b.item.minimizedAt || a.index - b.index)
    .map(({ item }) => item);
}

/**
 * The ids each kind restores for Restore All, ordered so the most recently minimized item ends on
 * top: a kind whose newest item is older restores first.
 */
export function restoreAllPlan(items: readonly MinimizedDialogItem[]): { kind: string; ids: string[] }[] {
  const groups = new Map<string, { kind: string; ids: string[]; newest: number }>();
  for (const item of items) {
    const group = groups.get(item.kind) ?? { kind: item.kind, ids: [], newest: item.minimizedAt };
    group.ids.push(item.id);
    group.newest = Math.max(group.newest, item.minimizedAt);
    groups.set(item.kind, group);
  }
  return [...groups.values()].sort((a, b) => a.newest - b.newest).map(({ kind, ids }) => ({ kind, ids }));
}

/** The question Close All asks first, or `undefined` when closing these items loses no draft. */
export function closeAllConfirmation(items: readonly MinimizedDialogItem[]): ConfirmationOptions | undefined {
  const drafted = items.filter((item) => item.draft !== undefined).length;
  if (drafted === 0) return undefined;
  return {
    title: items.length === 1 ? 'Close the minimized ticket?' : `Close ${items.length} minimized tickets?`,
    message:
      items.length === 1
        ? 'It has unsent text that will be discarded.'
        : `${drafted === 1 ? 'One of them has' : `${drafted} of them have`} unsent text that will be discarded.`,
    confirmLabel: 'Close All',
    tone: 'danger',
  };
}

/** Read a kind's stored records, or none when storage is unavailable or the value is malformed. */
export function loadMinimizedRecords<T>(
  storage: Pick<Storage, 'getItem'> | undefined,
  key: string,
  parse: (raw: string | null | undefined) => T[],
): T[] {
  try {
    return parse(storage?.getItem(key));
  } catch {
    return [];
  }
}

/** Store a kind's records, removing the key once the list is empty. */
export function saveMinimizedRecords<T>(
  storage: Pick<Storage, 'setItem' | 'removeItem'> | undefined,
  key: string,
  list: readonly T[],
  serialize: (list: readonly T[]) => string,
): void {
  try {
    if (list.length === 0) storage?.removeItem(key);
    else storage?.setItem(key, serialize(list));
  } catch {
    // Storage may be unavailable (private mode, quota); the list still lives for this session.
  }
}
