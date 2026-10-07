import { openPopupMenuAt } from '@kerfjs/ui/popup-menu';

export interface ContextMenuSize {
  width: number;
  height: number;
}
export interface ContextMenuPosition {
  x: number;
  y: number;
}

export function viewportSafePointerPosition(
  x: number,
  y: number,
  viewportWidth: number,
  viewportHeight: number,
): ContextMenuPosition {
  return { x: Math.max(0, Math.min(x, viewportWidth)), y: Math.max(0, Math.min(y, viewportHeight)) };
}

export function viewportSafeContextMenuPosition(
  x: number,
  y: number,
  viewportWidth: number,
  viewportHeight: number,
  size: ContextMenuSize,
  margin = 8,
): ContextMenuPosition {
  const availableWidth = Math.max(0, viewportWidth - margin * 2),
    availableHeight = Math.max(0, viewportHeight - margin * 2);
  return {
    x: Math.max(margin, Math.min(x, margin + Math.max(0, availableWidth - size.width))),
    y: Math.max(margin, Math.min(y, margin + Math.max(0, availableHeight - size.height))),
  };
}

/** The Kerf PopupMenu root a context-mode menu renders (`data-context-menu` names the surface). */
export type ContextPopupMenuElement = HTMLElement & { open: boolean };

/**
 * Open the context-mode Kerf PopupMenu that a menu signal has just rendered, at the viewport point
 * its app-owned wrapper records in `data-context-anchor-x`/`-y`. The app owns the open state (the
 * menu exists only while its signal is set), so this only hands the anchor to Kerf's
 * `openPopupMenuAt` once the element is in the DOM: now if the render was synchronous, else after
 * the microtask/frame Kerf uses to morph (HS2-2EHD8R).
 */
export function revealContextPopupMenu(surface: string, root: ParentNode = document, attempts = 3): void {
  const menu = root.querySelector<ContextPopupMenuElement>(`[data-context-menu="${surface}"]`);
  if (menu) {
    if (!menu.open) openContextPopupMenu(menu);
    return;
  }
  if (attempts <= 0) return;
  const retry = () => {
    revealContextPopupMenu(surface, root, attempts - 1);
  };
  if (attempts === 3) queueMicrotask(retry);
  else requestAnimationFrame(retry);
}

/** Open one rendered context-mode PopupMenu at the anchor its wrapper records. */
export function openContextPopupMenu(menu: ContextPopupMenuElement): void {
  const wrapper = menu.closest<HTMLElement>('[data-context-anchor-x]'),
    x = Number(wrapper?.dataset.contextAnchorX ?? 0),
    y = Number(wrapper?.dataset.contextAnchorY ?? 0);
  openPopupMenuAt(menu, x, y);
}

/** A live menu can be replaced when its entries change. Reopen only a new DOM host at the
 * wrapper's original pointer, and forget hosts after dismissal (HS2-S1EE53). */
export function reanchorReplacedContextPopupMenus(
  activeSurfaces: readonly string[],
  opened: Map<string, ContextPopupMenuElement>,
  root: ParentNode = document,
): void {
  for (const surface of opened.keys()) if (!activeSurfaces.includes(surface)) opened.delete(surface);
  for (const surface of activeSurfaces) {
    const menu = root.querySelector<ContextPopupMenuElement>(`[data-context-menu="${surface}"]`);
    if (menu && opened.get(surface) !== menu) {
      opened.set(surface, menu);
      openContextPopupMenu(menu);
    }
  }
}

/** Wrapper attributes that record a context-mode PopupMenu's viewport anchor for `revealContextPopupMenu`. */
export function contextPopupMenuAnchor(
  x: number,
  y: number,
): { 'data-context-anchor-x': string; 'data-context-anchor-y': string } {
  return { 'data-context-anchor-x': String(x), 'data-context-anchor-y': String(y) };
}
