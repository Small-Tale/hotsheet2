import { openPopupMenu, openPopupMenuAt } from '@kerfjs/ui/popup-menu';

import { closedPopupMenuRoot, isPopupMenuOpen } from './popup-menu-state';

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
  // A synthetic or keyboard-originated event can lack pointer coordinates; Kerf's context anchor
  // rejects non-finite points, so fall back to the viewport origin (HS2-H4JZDQ).
  const clamp = (value: number, max: number) => (Number.isFinite(value) ? Math.max(0, Math.min(value, max)) : 0);
  return { x: clamp(x, viewportWidth), y: clamp(y, viewportHeight) };
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
export type ContextPopupMenuElement = HTMLElement;

/**
 * Open the context-mode Kerf PopupMenu that a menu signal has just rendered. The menu carries its
 * own viewport `anchor` (KF-FXGW0C, HS2-H4JZDQ), and the app owns the open state (the menu exists
 * only while its signal is set), so this only calls Kerf's `openPopupMenu` once the element is in
 * the DOM: now if the render was synchronous, else after the microtask/frame Kerf uses to morph
 * (HS2-2EHD8R).
 */
export function revealContextPopupMenu(surface: string, root: ParentNode = document, attempts = 3): void {
  const menu = root.querySelector<ContextPopupMenuElement>(`[data-context-menu="${surface}"]`);
  if (menu) {
    if (!isPopupMenuOpen(menu)) openContextPopupMenu(menu);
    return;
  }
  if (attempts <= 0) return;
  const retry = () => {
    revealContextPopupMenu(surface, root, attempts - 1);
  };
  if (attempts === 3) queueMicrotask(retry);
  else requestAnimationFrame(retry);
}

/** Open one rendered context-mode PopupMenu at the `anchor` it was rendered with. */
export function openContextPopupMenu(menu: ContextPopupMenuElement): void {
  openPopupMenu(menu);
}

/** Keep a context popup opened from a persistent trigger at its click anchor while
 * the view morphs. Unlike signal-owned context menus, the trigger and menu remain
 * mounted after dismissal, so the menu's closing `toggle` ends this observation. */
export function maintainContextPopupMenuAnchor(
  selector: string,
  position: ContextMenuPosition,
  isCurrent: () => boolean,
  root: ParentNode = document,
): () => void {
  let current: ContextPopupMenuElement | null = null;
  let active = true;
  let anchoredStyle: string | null = null;
  const stop = () => {
    if (!active) return;
    active = false;
    observer.disconnect();
    document.removeEventListener('toggle', onHide, true);
  };
  const onHide = (event: Event) => {
    if (current?.isConnected && closedPopupMenuRoot(event) === current) stop();
  };
  const reanchor = () => {
    if (!isCurrent()) {
      stop();
      return;
    }
    const menu = root.querySelector<ContextPopupMenuElement>(selector);
    if (!menu) return;
    if (menu === current && !isPopupMenuOpen(menu)) {
      stop();
      return;
    }
    if (menu !== current || menu.getAttribute('style') !== anchoredStyle) {
      current = menu;
      openPopupMenuAt(menu, position.x, position.y);
      anchoredStyle = menu.getAttribute('style');
    }
  };
  const observer = new MutationObserver(reanchor);
  observer.observe(document.body, { childList: true, attributes: true, attributeFilter: ['style'], subtree: true });
  document.addEventListener('toggle', onHide, true);
  reanchor();
  return stop;
}

/** A ticket update can replace a live menu's host or re-render it at a new anchor. Kerf renders
 * the anchor into the menu's inline style (KF-FXGW0C), so a morph restores it with the host; open
 * a replacement host or re-anchored menu and forget hosts after dismissal (HS2-S1EE53). */
const anchoredStyles = new WeakMap<ContextPopupMenuElement, string | null>();

export function reanchorReplacedContextPopupMenus(
  activeSurfaces: readonly string[],
  opened: Map<string, ContextPopupMenuElement>,
  root: ParentNode = document,
): void {
  for (const surface of opened.keys()) if (!activeSurfaces.includes(surface)) opened.delete(surface);
  for (const surface of activeSurfaces) {
    const menu = root.querySelector<ContextPopupMenuElement>(`[data-context-menu="${surface}"]`);
    if (!menu) continue;
    if (opened.get(surface) === menu && anchoredStyles.get(menu) === menu.getAttribute('style')) continue;
    opened.set(surface, menu);
    openContextPopupMenu(menu);
    anchoredStyles.set(menu, menu.getAttribute('style'));
  }
}

/** The `anchor` for a context-mode Kerf PopupMenu. Kerf rejects non-finite points, and a synthetic or
 * keyboard-originated event can lack pointer coordinates, so those fall back to 0 (HS2-H4JZDQ). */
export function contextMenuAnchor(x: number, y: number): ContextMenuPosition {
  return { x: Number.isFinite(x) ? x : 0, y: Number.isFinite(y) ? y : 0 };
}
