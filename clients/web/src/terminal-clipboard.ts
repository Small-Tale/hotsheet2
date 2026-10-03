/**
 * Phone terminal clipboard (HS2-FRB545).
 *
 * xterm's surface is not natively selectable on touch devices and its hidden helper textarea offers
 * no Paste callout, so phone chrome exposes explicit Copy and Paste actions. Copy snapshots the
 * terminal buffer into a selectable text view; Paste reads the system clipboard and falls back to a
 * text-entry sheet when reading is unavailable or denied. Both talk to a mounted interactive
 * viewport through DOM events, like the key bar's {@link TERMINAL_KEY_EVENT}.
 */

/** Event dispatched on an interactive viewport to read its buffer text into `detail.text`. */
export const TERMINAL_READ_TEXT_EVENT = 'hotsheet-terminal-read-text';
/** Event dispatched on an interactive viewport to paste `detail.text` as terminal input. */
export const TERMINAL_PASTE_EVENT = 'hotsheet-terminal-paste';

/**
 * Bubbling event an interactive viewport dispatches when a touch long-press asks for the terminal
 * edit menu at `detail` (viewport coordinates) (HS2-KKP8YJ).
 */
export const TERMINAL_EDIT_MENU_EVENT = 'hotsheet-terminal-edit-menu';

export interface TerminalEditMenuDetail {
  x: number;
  y: number;
}

export interface TerminalReadTextDetail {
  text?: string;
}
export interface TerminalPasteDetail {
  text: string;
}

/** The slice of xterm's `IBuffer` the text snapshot reads. */
export interface TerminalBufferLike {
  readonly length: number;
  getLine(y: number): { readonly isWrapped: boolean; translateToString(trimRight?: boolean): string } | undefined;
}

/**
 * The buffer as plain text: soft-wrapped rows are rejoined into their logical line (keeping the
 * wrapped row's trailing cells, which are content), other rows lose trailing padding, and trailing
 * blank rows below the last output are dropped.
 */
export function terminalBufferText(buffer: TerminalBufferLike): string {
  const lines: string[] = [];
  for (let y = 0; y < buffer.length; y += 1) {
    const line = buffer.getLine(y);
    if (!line) continue;
    const text = line.translateToString(!buffer.getLine(y + 1)?.isWrapped);
    if (line.isWrapped && lines.length > 0) lines[lines.length - 1] += text;
    else lines.push(text);
  }
  while (lines.length > 0 && lines[lines.length - 1].trim() === '') lines.pop();
  return lines.join('\n');
}

/** Read the snapshot text of a mounted interactive viewport; undefined when none answered. */
export function readTerminalViewportText(viewport: HTMLElement): string | undefined {
  const detail: TerminalReadTextDetail = {};
  viewport.dispatchEvent(new CustomEvent(TERMINAL_READ_TEXT_EVENT, { detail }));
  return detail.text;
}

/** Paste text into a mounted interactive viewport as terminal input. */
export function pasteIntoTerminalViewport(viewport: HTMLElement, text: string): void {
  viewport.dispatchEvent(new CustomEvent<TerminalPasteDetail>(TERMINAL_PASTE_EVENT, { detail: { text } }));
}

export type ClipboardReadResult = { status: 'ok'; text: string } | { status: 'unavailable' } | { status: 'denied' };

/**
 * Read the system clipboard. `unavailable` means the API is missing (insecure origin, older
 * browser); `denied` covers a refused permission or any other read failure. Both fall back to the
 * paste sheet.
 */
export async function readClipboardText(
  clipboard: Pick<Clipboard, 'readText'> | undefined,
): Promise<ClipboardReadResult> {
  if (typeof clipboard?.readText !== 'function') return { status: 'unavailable' };
  try {
    return { status: 'ok', text: await clipboard.readText() };
  } catch {
    return { status: 'denied' };
  }
}

/**
 * Write text to the system clipboard, falling back to a synchronous `execCommand('copy')` path
 * (for insecure origins and browsers without the async API). Resolves whether either succeeded.
 */
export async function writeClipboardText(
  text: string,
  clipboard: Pick<Clipboard, 'writeText'> | undefined,
  fallback: () => boolean,
): Promise<boolean> {
  if (typeof clipboard?.writeText === 'function') {
    try {
      await clipboard.writeText(text);
      return true;
    } catch {
      /* fall through to the legacy copy path */
    }
  }
  try {
    return fallback();
  } catch {
    return false;
  }
}

/** The text a Copy action takes from a text view: its selection when one exists, else everything. */
export function terminalCopySelection(
  value: string,
  selectionStart: number | null,
  selectionEnd: number | null,
): { text: string; selection: boolean } {
  if (selectionStart !== null && selectionEnd !== null && selectionEnd > selectionStart)
    return { text: value.slice(selectionStart, selectionEnd), selection: true };
  return { text: value, selection: false };
}

/** Toast confirming a copy, counting lines so a partial copy reads differently from a full one. */
export function terminalCopyMessage(text: string, selection: boolean): string {
  const lines = text === '' ? 0 : text.split('\n').length;
  return `Copied ${selection ? 'selection' : 'terminal text'} (${lines} ${lines === 1 ? 'line' : 'lines'})`;
}
