import { delegate, effect, type Signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { type CiClearTarget, ciClearTargetOf } from '../ci-presentation';
import { revealContextPopupMenu, viewportSafeContextMenuPosition } from '../context-menu-position';
import { COMMANDS_AND_AI_ACTIONS } from '../interaction-attrs/commands-and-ai';
import { wireContextMenuDismissal } from './context-menu-dismissal';
import type { LiveCiMenuState } from './live-ci-context-menu';

/** The Live CI context menu's state and its one action (HS2-7PSBHQ). */
export interface LiveCiMenuBindings {
  readonly menu: Signal<LiveCiMenuState | undefined>;
  /** Forget the stored run; the server's live-state push then removes the chip. */
  readonly clear: (target: CiClearTarget) => Promise<void> | void;
}

/** The menu's size for viewport-safe placement: one item. */
const MENU_SIZE = { width: 192, height: 56 };

/**
 * Wire the project sidebar Live section's context menu (HS2-7PSBHQ): a right-click on the section, or the
 * context-menu key on its focused toggle, opens **Clear CI Status** while its `main` CI run shows as unknown
 * (the section then carries the run's clear target). The menu follows the shared context-menu dismissal.
 * Production wires `document.body`; the UX demo wires its catalog root.
 */
export function wireLiveCiMenu(root: HTMLElement, { menu, clear }: LiveCiMenuBindings): () => void {
  const lifetime = createScope();
  lifetime.add(
    delegate(root, 'contextmenu', '[data-ci-clear-checkout]', (event, target) => {
      const element = target as HTMLElement,
        request = ciClearTargetOf(element.dataset.ciClearCheckout, element.dataset.ciClearReportedAtMs);
      if (!request) return;
      event.preventDefault();
      const pointer = event as MouseEvent,
        // The context-menu key reports no pointer: anchor below the focused toggle instead.
        keyboard = pointer.clientX === 0 && pointer.clientY === 0,
        focused = document.activeElement,
        rect = (keyboard && focused && element.contains(focused) ? focused : element).getBoundingClientRect();
      menu.value = {
        ...request,
        ...viewportSafeContextMenuPosition(
          keyboard ? rect.left : pointer.clientX,
          keyboard ? rect.bottom : pointer.clientY,
          window.innerWidth,
          window.innerHeight,
          MENU_SIZE,
        ),
      };
    }),
  );
  lifetime.add(
    effect(() => {
      if (menu.value) revealContextPopupMenu('live-ci');
    }),
  );
  lifetime.add(
    delegate(root, 'click', COMMANDS_AND_AI_ACTIONS.clearLiveCi.selector, (event, target) => {
      event.stopPropagation();
      const element = target as HTMLElement,
        request = ciClearTargetOf(element.dataset.ciCheckout, element.dataset.ciReportedAtMs);
      menu.value = undefined;
      if (request) void clear(request);
    }),
  );
  lifetime.add(
    wireContextMenuDismissal(root, [
      {
        surface: 'live-ci',
        isOpen: () => Boolean(menu.value),
        stateToken: () => menu.value,
        close: () => {
          menu.value = undefined;
        },
        hide: true,
      },
    ]),
  );
  return () => {
    lifetime.dispose();
  };
}
