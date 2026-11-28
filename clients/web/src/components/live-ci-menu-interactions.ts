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
  /** Forget one stored run; the server's live-state push then removes its chip or row. */
  readonly clear: (target: CiClearTarget) => Promise<void> | void;
}

/** The section the menu opens on, and the elements that name a shown run. */
const SECTION = '[data-component="project-live-section"]';
const RUN = '[data-ci-clear-checkout]';

/** The menu's size for viewport-safe placement: its width and one row per item. */
export function liveCiMenuSize(items: number): { width: number; height: number } {
  return { width: 240, height: 16 + 40 * Math.max(1, items) };
}

/**
 * The runs a context-menu press on the Live section offers (HS2-D3JJ49): the run of the chip or row under the
 * pointer, otherwise every shown run the section names, each once. Empty when the section shows no run.
 */
export function liveCiMenuTargets(section: HTMLElement, pressed: EventTarget | null): CiClearTarget[] {
  const own = (pressed as Element | null)?.closest<HTMLElement>(RUN);
  const elements: HTMLElement[] =
    own && section.contains(own) ? [own] : Array.from(section.querySelectorAll<HTMLElement>(RUN));
  const seen = new Set<string>();
  return elements.flatMap((element) => {
    const target = ciClearTargetOf(element.dataset);
    if (!target) return [];
    const key = JSON.stringify([target.checkout, target.reportedAtMs, target.branch, target.workflow]);
    if (seen.has(key)) return [];
    seen.add(key);
    return [target];
  });
}

/**
 * Wire the project sidebar Live section's context menu (HS2-7PSBHQ, HS2-D3JJ49): a right-click on the section,
 * or the context-menu key on its focused toggle, opens **Clear CI Status** for the run under the pointer, or for
 * each shown run. The section's chips and rows carry each run's clear target. The menu follows the shared
 * context-menu dismissal. Production wires `document.body`; the UX demo wires its catalog root.
 */
export function wireLiveCiMenu(root: HTMLElement, { menu, clear }: LiveCiMenuBindings): () => void {
  const lifetime = createScope();
  lifetime.add(
    delegate(root, 'contextmenu', SECTION, (event, target) => {
      const element = target as HTMLElement,
        targets = liveCiMenuTargets(element, event.target);
      if (!targets.length) return;
      event.preventDefault();
      const pointer = event as MouseEvent,
        // The context-menu key reports no pointer: anchor below the focused toggle instead.
        keyboard = pointer.clientX === 0 && pointer.clientY === 0,
        focused = document.activeElement,
        rect = (keyboard && focused && element.contains(focused) ? focused : element).getBoundingClientRect();
      menu.value = {
        targets,
        ...viewportSafeContextMenuPosition(
          keyboard ? rect.left : pointer.clientX,
          keyboard ? rect.bottom : pointer.clientY,
          window.innerWidth,
          window.innerHeight,
          liveCiMenuSize(targets.length),
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
      const request = ciClearTargetOf((target as HTMLElement).dataset);
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
