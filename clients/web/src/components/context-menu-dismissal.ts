import { delegate } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { eventTargetsContextMenu } from './ticket-row-context-menu';

/** One pointer-positioned context menu whose open state lives outside the PopupMenu element. */
export interface DismissibleContextMenu {
  /** The menu's `data-context-menu` surface. */
  readonly surface: string;
  readonly isOpen: () => boolean;
  readonly close: () => void;
  /** Extra targets, such as the menu's own trigger, whose press does not dismiss it. */
  readonly ignore?: string;
  /** Escape closes this menu (default true). */
  readonly escape?: boolean;
  /**
   * Web Awesome's own hide (item choice, light dismiss) also clears the state, deferred so an item's
   * click handler still reads it (default false).
   */
  readonly hide?: boolean;
}

/**
 * Shared dismissal for context menus (HS2-AFCWNG): a press outside a menu closes it, Escape closes
 * every opted-in menu, and opted-in menus follow Web Awesome's hide. Production wires the shell's
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
  if (hiding.length)
    lifetime.add(
      delegate(root, 'wa-hide', '[data-context-menu]', (event, target) => {
        if (event.target !== target) return;
        const menu = hiding.find((candidate) => candidate.surface === (target as HTMLElement).dataset.contextMenu);
        if (menu)
          setTimeout(() => {
            if (menu.isOpen()) menu.close();
          }, 0);
      }),
    );
  return () => {
    lifetime.dispose();
  };
}
