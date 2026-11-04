import { applyRememberedTabOrder, reorderTabs, type TabDropPosition } from './tab-order';

export interface DrawerOrderStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

export function drawerTabOrderStorageKey(projectId: string): string {
  return `hotsheet.project.${projectId}.terminal-drawer-order`;
}

export function parseDrawerTabOrder(raw: string | null): string[] {
  if (!raw) return [];
  try {
    const parsed = JSON.parse(raw) as unknown;
    return Array.isArray(parsed)
      ? [...new Set(parsed.filter((item): item is string => typeof item === 'string' && item.length > 0))]
      : [];
  } catch {
    return [];
  }
}

export function loadDrawerTabOrder(storage: DrawerOrderStorage, projectId: string): string[] {
  return parseDrawerTabOrder(storage.getItem(drawerTabOrderStorageKey(projectId)));
}

export function saveDrawerTabOrder(storage: DrawerOrderStorage, projectId: string, ids: readonly string[]): string[] {
  const order = [...new Set(ids.filter(Boolean))];
  storage.setItem(drawerTabOrderStorageKey(projectId), JSON.stringify(order));
  return order;
}

export function orderedDrawerTabIds(
  terminalIds: readonly string[],
  chatIds: readonly string[],
  remembered: readonly string[],
): string[] {
  return applyRememberedTabOrder([...terminalIds, ...chatIds], (id) => id, remembered);
}

export function reorderDrawerTabIds(
  ids: readonly string[],
  sourceId: string,
  targetId: string,
  position: TabDropPosition,
): string[] {
  return reorderTabs(ids, (id) => id, sourceId, targetId, position);
}

export function drawerTabFocusRequestStillOwned(
  scheduled: object | null,
  current: object | null,
  body: object,
): boolean {
  return current === scheduled || current === body;
}

/** A newer drawer focus intent cancels an older input-focus frame before it can steal tab focus. */
export function drawerInputFocusRequestStillOwned(
  scheduledGeneration: number,
  currentGeneration: number,
  scheduled: object | null,
  current: object | null,
  body: object,
): boolean {
  return scheduledGeneration === currentGeneration && drawerTabFocusRequestStillOwned(scheduled, current, body);
}

/** Return the keyboard-input surface for the drawer's currently selected live item. */
export function selectedDrawerInput(drawer: ParentNode): HTMLElement | undefined {
  return (
    drawer.querySelector<HTMLElement>('[data-component="terminal-session"]:not([hidden]) .xterm-helper-textarea') ??
    drawer.querySelector<HTMLElement>(
      '[data-component="ai-conversation"] textarea[name="conversation-draft"]:not(:disabled)',
    ) ??
    undefined
  );
}

export type DrawerTabCloseAction = 'close' | 'close-others' | 'close-right' | 'close-left' | 'close-all';

export function drawerTabCloseIds(ids: readonly string[], activeId: string, action: DrawerTabCloseAction): string[] {
  const index = ids.indexOf(activeId);
  if (index < 0) return [];
  if (action === 'close') return [activeId];
  if (action === 'close-others') return ids.filter((id) => id !== activeId);
  if (action === 'close-right') return ids.slice(index + 1);
  if (action === 'close-left') return ids.slice(0, index);
  return [...ids];
}

export function drawerTabSelectionAfterClose(
  ids: readonly string[],
  selectedId: string,
  closingIds: readonly string[],
): string {
  const closing = new Set(closingIds);
  if (!closing.has(selectedId)) return selectedId;
  const index = ids.indexOf(selectedId);
  return (
    ids.find((id, itemIndex) => itemIndex > index && !closing.has(id)) ??
    [...ids].reverse().find((id, itemIndex) => ids.length - 1 - itemIndex < index && !closing.has(id)) ??
    'grid'
  );
}

/**
 * A drawer item the person asked for whose instance is still being created (HS2-AQ40G7). It is
 * selected at once so the drawer never falls back to the Project grid while the create resolves;
 * `resolvedId` names the created terminal or chat until that item reaches the drawer's snapshot.
 */
export interface PendingDrawerTab {
  id: string;
  name: string;
  kind: 'terminal' | 'ai-chat';
  resolvedId?: string;
}

export const PENDING_DRAWER_TAB_PREFIX = 'pending:';

export function isPendingDrawerTabId(id: string): boolean {
  return id.startsWith(PENDING_DRAWER_TAB_PREFIX);
}

/** Pending tabs still shown: a resolved one disappears once its live item is present. */
export function visiblePendingDrawerTabs(
  pending: readonly PendingDrawerTab[],
  liveIds: ReadonlySet<string>,
): PendingDrawerTab[] {
  return pending.filter((tab) => tab.resolvedId === undefined || !liveIds.has(tab.resolvedId));
}

export type DrawerSelectionView =
  { kind: 'live'; id: string } | { kind: 'pending'; tab: PendingDrawerTab } | { kind: 'grid' };

/**
 * What the drawer shows for its selection (HS2-AQ40G7): a live terminal or chat; a pending tab's
 * placeholder, also while the selection already names the created item that has not arrived yet;
 * otherwise the Project grid.
 */
export function drawerSelectionView(
  selectedId: string,
  liveIds: ReadonlySet<string>,
  pending: readonly PendingDrawerTab[],
): DrawerSelectionView {
  if (liveIds.has(selectedId)) return { kind: 'live', id: selectedId };
  const tab = pending.find((item) => item.id === selectedId || item.resolvedId === selectedId);
  return tab ? { kind: 'pending', tab } : { kind: 'grid' };
}
