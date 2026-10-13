import { delegate } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

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
