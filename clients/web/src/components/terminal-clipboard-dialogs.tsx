import './terminal-clipboard-dialogs.css';

import { List } from '@kerfjs/ui/list';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { ClipboardPaste, Copy } from 'lucide';

import { TERMINALS_ACTIONS, TERMINALS_TARGETS } from '../interaction-attrs/terminals';

/** The terminal a clipboard sheet acts on, plus a generation that gives every opening a fresh field. */
export interface TerminalClipboardTarget {
  title: string;
  generation: number;
}
/** Copy sheet state (HS2-FRB545). `open` drops on close while the snapshot stays for the hide animation. */
export interface TerminalCopyState extends TerminalClipboardTarget {
  open: boolean;
  text: string;
}
/** Paste fallback sheet state, opened when the system clipboard cannot be read (HS2-FRB545). */
export interface TerminalPasteState extends TerminalClipboardTarget {
  open: boolean;
  reason: 'unavailable' | 'denied';
}

/**
 * Phone terminal Copy sheet (HS2-FRB545): the terminal buffer as native, long-press-selectable text.
 * Copy takes the selection when one exists and otherwise the whole snapshot.
 */
export function TerminalCopyDialog({ state }: { state?: TerminalCopyState }) {
  const open = Boolean(state?.open);
  return (
    <wa-dialog
      {...TERMINALS_TARGETS.terminalCopyDialog.attrs}
      class="terminal-clipboard-dialog"
      label="Copy terminal text"
      open={open}
      data-controlled-open={String(open)}
    >
      <List gap="m">
        <p class="terminal-clipboard-dialog__hint">
          Touch and hold to select part of {state?.title ?? 'the terminal'}. Copy takes the selection, or all the text
          when nothing is selected.
        </p>
        <textarea
          class="terminal-clipboard-dialog__text"
          data-key={`terminal-copy-text:${state?.generation ?? 0}`}
          name="terminal-copy-text"
          aria-label="Terminal text"
          readOnly
          wrap="off"
          spellcheck="false"
          autoCapitalize="off"
          autocomplete="off"
        >
          {state?.text ?? ''}
        </textarea>
        <Row hAlign="right" vAlign="middle" gap="xs">
          <wa-button appearance="plain" type="button" {...TERMINALS_ACTIONS.closeTerminalCopy.attrs}>
            Done
          </wa-button>
          <wa-button appearance="accent" type="button" {...TERMINALS_ACTIONS.confirmTerminalCopy.attrs}>
            <LucideIcon icon={Copy} name="copy" slot="start" />
            Copy
          </wa-button>
        </Row>
      </List>
    </wa-dialog>
  );
}

/**
 * Phone terminal Paste fallback (HS2-FRB545): when the clipboard cannot be read, the user pastes into a
 * native field with the platform's own Paste callout and sends it to the terminal.
 */
export function TerminalPasteDialog({ state }: { state?: TerminalPasteState }) {
  const open = Boolean(state?.open);
  return (
    <wa-dialog
      {...TERMINALS_TARGETS.terminalPasteDialog.attrs}
      class="terminal-clipboard-dialog"
      label="Paste into terminal"
      open={open}
      data-controlled-open={String(open)}
    >
      <form {...TERMINALS_ACTIONS.submitTerminalPaste.attrs}>
        <List gap="m">
          <p class="terminal-clipboard-dialog__hint">
            {state?.reason === 'unavailable'
              ? 'This browser does not let Hot Sheet read the clipboard.'
              : 'Clipboard access was not allowed.'}{' '}
            Touch and hold in the box, choose Paste, then send it to {state?.title ?? 'the terminal'}.
          </p>
          <textarea
            class="terminal-clipboard-dialog__text"
            data-key={`terminal-paste-text:${state?.generation ?? 0}`}
            name="terminal-paste-text"
            aria-label="Text to paste"
            wrap="off"
            spellcheck="false"
            autoCapitalize="off"
            autocomplete="off"
            autocorrect="off"
          ></textarea>
          <Row hAlign="right" vAlign="middle" gap="xs">
            <wa-button appearance="plain" type="button" {...TERMINALS_ACTIONS.cancelTerminalPaste.attrs}>
              Cancel
            </wa-button>
            <wa-button appearance="accent" type="submit">
              <LucideIcon icon={ClipboardPaste} name="clipboard-paste" slot="start" />
              Paste
            </wa-button>
          </Row>
        </List>
      </form>
    </wa-dialog>
  );
}
