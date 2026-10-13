import { delegate } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { viewportSafeContextMenuPosition } from '../context-menu-position';
import { TERMINALS_ACTIONS, TERMINALS_TARGETS } from '../interaction-attrs/terminals';
import type { TerminalModifier, TerminalSpecialKey } from '../terminal-keys';

/**
 * Side effects for the terminal key bar, clipboard actions and sheets, and rename dialog
 * (HS2-XE048K). Production passes the live terminal operations; the UX demo passes callbacks that
 * report what production would do, so both run the same event handling.
 */

export interface TerminalKeyBarBindings {
  readonly toggleModifier: (modifier: TerminalModifier, target: Element) => void;
  /** The Fn row toggled; the helper has already scrolled the bar back to its leading edge. */
  readonly toggleFunctionRow: (target: Element) => void;
  readonly sendKey: (key: TerminalSpecialKey, target: Element) => void;
}

/** Wire TerminalKeyBar modifier, Fn-row, and key presses under `root`. */
export function wireTerminalKeyBar(root: HTMLElement, bindings: TerminalKeyBarBindings): () => void {
  const lifetime = createScope();
  lifetime.add(
    delegate(root, 'click', TERMINALS_ACTIONS.toggleTerminalModifier.selector, (_event, target) => {
      const modifier = (target as HTMLElement).dataset.modifier as TerminalModifier | undefined;
      if (modifier) bindings.toggleModifier(modifier, target);
    }),
  );
  lifetime.add(
    delegate(root, 'click', TERMINALS_ACTIONS.toggleTerminalFunctionRow.selector, (_event, target) => {
      // Each row starts at its leading edge, so a swapped row never opens mid-scroll.
      target.closest(TERMINALS_TARGETS.terminalKeyBar.selector)?.scrollTo({ left: 0 });
      bindings.toggleFunctionRow(target);
    }),
  );
  lifetime.add(
    delegate(root, 'click', TERMINALS_ACTIONS.sendTerminalKey.selector, (_event, target) => {
      const key = (target as HTMLElement).dataset.key as TerminalSpecialKey | undefined;
      if (key) bindings.sendKey(key, target);
    }),
  );
  return () => {
    lifetime.dispose();
  };
}

/** The copy-selection, copy-text, and paste actions offered by the key bar, rail, card, and edit menu. */
export interface TerminalClipboardActionBindings {
  readonly copySelection: (target: Element) => void;
  readonly copyText: (target: Element) => void;
  readonly paste: (target: Element) => void;
}

/** Wire the terminal clipboard actions under `root`. */
export function wireTerminalClipboardActions(root: HTMLElement, bindings: TerminalClipboardActionBindings): () => void {
  const lifetime = createScope();
  lifetime.add(
    delegate(root, 'click', TERMINALS_ACTIONS.copyTerminalSelection.selector, (_event, target) => {
      bindings.copySelection(target);
    }),
  );
  lifetime.add(
    delegate(root, 'click', TERMINALS_ACTIONS.copyTerminalText.selector, (_event, target) => {
      bindings.copyText(target);
    }),
  );
  lifetime.add(
    delegate(root, 'click', TERMINALS_ACTIONS.pasteTerminalText.selector, (_event, target) => {
      bindings.paste(target);
    }),
  );
  return () => {
    lifetime.dispose();
  };
}

export interface TerminalClipboardSheetBindings {
  /** Copy from the Copy sheet's text field (its selection, or all of it). */
  readonly confirmCopy: (field: HTMLTextAreaElement) => void;
  /** Close, or the Copy sheet hid by any other route. */
  readonly closeCopy: () => void;
  readonly submitPaste: (text: string) => void;
  /** Cancel, or the Paste sheet hid by any other route. */
  readonly closePaste: () => void;
}

/** Wire the terminal Copy and Paste sheets under `root`. */
export function wireTerminalClipboardSheets(root: HTMLElement, bindings: TerminalClipboardSheetBindings): () => void {
  const lifetime = createScope();
  lifetime.add(
    delegate(root, 'click', TERMINALS_ACTIONS.confirmTerminalCopy.selector, () => {
      const field = root.querySelector<HTMLTextAreaElement>('textarea[name="terminal-copy-text"]');
      if (field) bindings.confirmCopy(field);
    }),
  );
  lifetime.add(
    delegate(root, 'click', TERMINALS_ACTIONS.closeTerminalCopy.selector, () => {
      bindings.closeCopy();
    }),
  );
  lifetime.add(
    delegate(root, 'wa-hide', TERMINALS_TARGETS.terminalCopyDialog.selector, () => {
      bindings.closeCopy();
    }),
  );
  lifetime.add(
    delegate(root, 'submit', TERMINALS_ACTIONS.submitTerminalPaste.selector, (event, target) => {
      event.preventDefault();
      bindings.submitPaste(
        target.querySelector<HTMLTextAreaElement>('textarea[name="terminal-paste-text"]')?.value ?? '',
      );
    }),
  );
  lifetime.add(
    delegate(root, 'click', TERMINALS_ACTIONS.cancelTerminalPaste.selector, () => {
      bindings.closePaste();
    }),
  );
  lifetime.add(
    delegate(root, 'wa-hide', TERMINALS_TARGETS.terminalPasteDialog.selector, () => {
      bindings.closePaste();
    }),
  );
  return () => {
    lifetime.dispose();
  };
}

export interface TerminalRenameDialogBindings {
  /** Rename to the trimmed, non-empty `name`. */
  readonly rename: (name: string) => void;
  readonly reset: () => void;
  /** Cancel, or the dialog hid by any other route. */
  readonly dismiss: (reason: 'cancel' | 'hide') => void;
}

/** Wire the terminal rename dialog under `root`. */
export function wireTerminalRenameDialog(root: HTMLElement, bindings: TerminalRenameDialogBindings): () => void {
  const lifetime = createScope();
  lifetime.add(
    delegate(root, 'submit', TERMINALS_ACTIONS.renameTerminalForm.selector, (event, target) => {
      event.preventDefault();
      const name = target.querySelector<HTMLInputElement>('[name="terminal-name"]')?.value.trim() ?? '';
      if (name) bindings.rename(name);
    }),
  );
  lifetime.add(
    delegate(root, 'click', TERMINALS_ACTIONS.resetTerminalRename.selector, () => {
      bindings.reset();
    }),
  );
  lifetime.add(
    delegate(root, 'click', TERMINALS_ACTIONS.cancelTerminalRename.selector, () => {
      bindings.dismiss('cancel');
    }),
  );
  lifetime.add(
    delegate(root, 'wa-hide', '[data-terminal-rename-dialog]', () => {
      bindings.dismiss('hide');
    }),
  );
  return () => {
    lifetime.dispose();
  };
}

/** Where the terminal dashboard context menu opens, already kept inside the viewport. */
export interface TerminalDashboardMenuRequest {
  readonly key: string;
  readonly x: number;
  readonly y: number;
}

export interface TerminalDashboardMenuBindings {
  readonly open: (menu: TerminalDashboardMenuRequest) => void;
  /** Clear a halted session from its menu; the helper reports the menu's terminal key. */
  readonly clearHalt: (key: string | undefined) => void;
}

const TERMINAL_MENU_SIZE = { width: 224, height: 104 };
const menuAt = (key: string, x: number, y: number): TerminalDashboardMenuRequest => ({
  key,
  ...viewportSafeContextMenuPosition(x, y, window.innerWidth, window.innerHeight, TERMINAL_MENU_SIZE),
});

/**
 * Wire the terminal dashboard context menu under `root` (HS2-05NMW0): a right-click on an unmagnified
 * tile, the tile's menu button, and the menu's clear-halt action. Production opens its terminal menu
 * signal and clears the halt over the API; the UX demo passes fixture callbacks.
 */
export function wireTerminalDashboardMenu(root: HTMLElement, bindings: TerminalDashboardMenuBindings): () => void {
  const lifetime = createScope();
  lifetime.add(
    delegate(root, 'contextmenu', TERMINALS_TARGETS.terminalTile.selector, (event, target) => {
      const tile = (target as HTMLElement).dataset;
      if (tile.magnified === 'true') return;
      event.preventDefault();
      const pointer = event as MouseEvent;
      bindings.open(menuAt(tile.terminalKey ?? tile.itemId ?? '', pointer.clientX, pointer.clientY));
    }),
  );
  lifetime.add(
    delegate(root, 'click', TERMINALS_ACTIONS.openTerminalContextMenu.selector, (event, target) => {
      event.preventDefault();
      const trigger = (target as HTMLElement).dataset,
        box = target.getBoundingClientRect();
      bindings.open(menuAt(trigger.itemId ?? trigger.terminalKey ?? '', box.right, box.bottom));
    }),
  );
  lifetime.add(
    delegate(root, 'click', TERMINALS_ACTIONS.clearTerminalHalt.selector, (_event, target) => {
      bindings.clearHalt((target as HTMLElement).dataset.itemId);
    }),
  );
  return () => {
    lifetime.dispose();
  };
}

/** Wire the terminal drawer focus mode's text-size control under `root` (HS2-05NMW0). */
export function wireTerminalTextSizeControl(root: HTMLElement, cycle: (control: Element) => void): () => void {
  return delegate(root, 'click', TERMINALS_ACTIONS.cycleMobileTerminalColumns.selector, (_event, target) => {
    cycle(target);
  });
}
