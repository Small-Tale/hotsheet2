import type { Signal } from 'kerfjs';

import type { Confirm } from '../confirmation';
import {
  closeAllConfirmation,
  type MinimizedDialogItem,
  type MinimizedDialogKind,
  minimizedItemsForTab,
  restoreAllPlan,
} from '../minimized-dialogs';
import { MINIMIZED_TICKETS_CONTROL_SELECTOR } from '../reader-minimize-motion';

/** Focus follows a minimized dialog into the Minimized Tickets control, unless something else took it. */
export function focusMinimizedControl() {
  if (typeof document === 'undefined') return;
  const active = document.activeElement;
  if (active && active !== document.body && active.isConnected) return;
  const control = document.querySelector<HTMLElement>(MINIMIZED_TICKETS_CONTROL_SELECTOR),
    button = control?.matches('button') ? control : control?.querySelector<HTMLElement>('button');
  button?.focus({ preventScroll: true });
}

/** Run once the next two frames have painted (after a dialog opened or closed). */
export function nextFrames(run: () => void) {
  if (typeof requestAnimationFrame === 'function')
    requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        run();
      }),
    );
  else queueMicrotask(run);
}

export interface MinimizedDialogsDependencies {
  /** Every minimizable dialog kind, in the order equal timestamps list them. */
  kinds: readonly MinimizedDialogKind[];
  selectedProjectId: Signal<string>;
  confirm: Confirm;
  /** Open the several-item menu for a keyboard restore; absent where no menu renders. */
  openMinimizedMenu?: () => void;
}

/**
 * The Minimized Tickets control's commands across every minimized dialog kind (HS2-3JR0ZH): it
 * lists the current tab's items oldest first, restores one or all, and closes all after asking when
 * a draft would be lost. Each kind keeps minimizing itself; this only routes by item id and kind.
 */
export function createMinimizedDialogs({
  kinds,
  selectedProjectId,
  confirm,
  openMinimizedMenu,
}: MinimizedDialogsDependencies) {
  const kindById = new Map(kinds.map((kind) => [kind.kind, kind]));

  function items(): MinimizedDialogItem[] {
    return minimizedItemsForTab(kinds, selectedProjectId.value);
  }

  function restore(id: string): Promise<boolean> {
    const item = items().find((candidate) => candidate.id === id),
      kind = item ? kindById.get(item.kind) : undefined;
    return kind ? kind.restore([id]) : Promise.resolve(false);
  }

  /** Restore every item of the tab; the most recently minimized ends on top. */
  async function restoreAll(): Promise<boolean> {
    let restored = false;
    for (const { kind, ids } of restoreAllPlan(items())) {
      const owner = kindById.get(kind);
      if (owner && (await owner.restore(ids))) restored = true;
    }
    return restored;
  }

  /** Close every item of the tab, asking first only when a draft would be lost. */
  async function closeAll(): Promise<boolean> {
    const listed = items();
    if (listed.length === 0) return false;
    const question = closeAllConfirmation(listed);
    if (question && !(await confirm(question))) return false;
    // Items minimized while the question was open stay minimized.
    for (const kind of kinds) {
      const ids = listed.filter((item) => item.kind === kind.kind).map((item) => item.id);
      if (ids.length > 0) kind.discard(ids);
    }
    return true;
  }

  /** The keyboard command with no dialog to minimize: restore the only item, or list several. */
  function restoreOrList() {
    const listed = items();
    const only = listed.at(0);
    if (listed.length === 1 && only) void restore(only.id);
    else if (listed.length > 1) openMinimizedMenu?.();
  }

  /** Closing project tabs drops every kind's items of those tabs. */
  function disposeProjects(ids: readonly string[]) {
    for (const kind of kinds) kind.dispose(ids);
  }

  return { items, restore, restoreAll, closeAll, restoreOrList, disposeProjects };
}

export type MinimizedDialogsController = ReturnType<typeof createMinimizedDialogs>;
