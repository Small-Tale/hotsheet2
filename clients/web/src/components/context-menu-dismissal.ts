import { createScope } from 'kerfjs/scope';

import { closedPopupMenuRoot } from '../popup-menu-state';
import { eventTargetsContextMenu } from './ticket-row-context-menu';

/** One pointer-positioned context menu whose open state lives outside the PopupMenu element. */
export interface DismissibleContextMenu {
  /** The menu's `data-context-menu` surface. */
  readonly surface: string;
  readonly isOpen: () => boolean;
  /** Identity of the current open request; a later request must survive an older close event. */
  readonly stateToken?: () => unknown;
  readonly close: () => void;
  /** Extra targets, such as the menu's own trigger, whose press does not dismiss it. */
  readonly ignore?: string;
  /** Escape closes this menu (default true). */
  readonly escape?: boolean;
  /**
   * The menu's own close (item choice, light dismiss) also clears the state, deferred so an item's
   * click handler still reads it (default false).
   */
  readonly hide?: boolean;
}

/**
 * Shared dismissal for context menus (HS2-AFCWNG): a press outside a menu closes it, Escape closes
 * every opted-in menu, and opted-in menus follow the Kerf menu's own close. Production wires the shell's
 * menu signals on `document.body`; UX demos wire their fixture menus on the catalog root, so both
 * resolve the same dismissal gestures.
 */
export function wireContextMenuDismissal(root: HTMLElement, menus: readonly DismissibleContextMenu[]): () => void {
  const lifetime = createScope();
  const press = (event: Event) => {
    for (const menu of menus) {
      if (!menu.isOpen()) continue;
      const selector = `[data-context-menu="${menu.surface}"]${menu.ignore ? `, ${menu.ignore}` : ''}`;
      if (!eventTargetsContextMenu(event, selector)) menu.close();
    }
  };
  root.addEventListener('pointerdown', press, { capture: true });
  lifetime.add(() => {
    root.removeEventListener('pointerdown', press, { capture: true });
  });
  const key = (event: KeyboardEvent) => {
    if (event.key !== 'Escape') return;
    for (const menu of menus) if (menu.escape !== false && menu.isOpen()) menu.close();
  };
  root.addEventListener('keydown', key);
  lifetime.add(() => {
    root.removeEventListener('keydown', key);
  });
  const hiding = menus.filter((menu) => menu.hide);
  if (hiding.length) {
    // A popover's `toggle` does not bubble; listen in the capture phase.
    const closed = (event: Event) => {
      const surface = closedPopupMenuRoot(event)?.dataset.contextMenu;
      const menu = surface && hiding.find((candidate) => candidate.surface === surface);
      if (menu) {
        const closingState = menu.stateToken?.();
        setTimeout(() => {
          if (menu.isOpen() && (!menu.stateToken || menu.stateToken() === closingState)) menu.close();
        }, 0);
      }
    };
    root.addEventListener('toggle', closed, { capture: true });
    lifetime.add(() => {
      root.removeEventListener('toggle', closed, { capture: true });
    });
  }
  return () => {
    lifetime.dispose();
  };
}
