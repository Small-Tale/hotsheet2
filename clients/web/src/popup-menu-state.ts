/**
 * Open state of a rendered Kerf PopupMenu (HS2-BH7NZ4).
 *
 * Since Kerf UI 5.0.0-beta.106 (KF-EMJJGV) a PopupMenu is a plain `.kui-popup-menu` span holding a
 * native trigger (or, in context mode, a hidden anchor button) and a `role="menu"` manual popover. The
 * open state is the opener's `aria-expanded`, not a Web Awesome `open` property, and a closing menu
 * fires the popover's own `toggle` event (`newState: "closed"`) instead of `wa-hide`.
 */

/** The opener a PopupMenu root keeps its open state on. */
const OPENER = ':scope > .kui-popup-menu__trigger, :scope > .kui-popup-menu__context-anchor';
/** A PopupMenu root's top-level menu panel (not a nested submenu). */
export const POPUP_MENU_PANEL = '.kui-popup-menu__menu';

const isElement = (target: unknown): target is HTMLElement =>
  typeof (target as Partial<HTMLElement> | null)?.matches === 'function';

/** Whether a rendered PopupMenu root is open. */
export function isPopupMenuOpen(menu: Element): boolean {
  return menu.querySelector(OPENER)?.getAttribute('aria-expanded') === 'true';
}

/**
 * The PopupMenu root whose top-level panel just closed, when `event` is that panel's `toggle`
 * event; otherwise undefined (an opening, or a submenu closing inside it).
 */
export function closedPopupMenuRoot(event: Event): HTMLElement | undefined {
  const panel = event.target;
  if (event.type !== 'toggle' || (event as ToggleEvent).newState !== 'closed') return undefined;
  // Duck-typed rather than `instanceof HTMLElement`, so it also runs where no DOM globals exist.
  if (!isElement(panel) || !panel.matches(POPUP_MENU_PANEL)) return undefined;
  const root = panel.parentElement;
  return root?.matches('.kui-popup-menu') ? root : undefined;
}
