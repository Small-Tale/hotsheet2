import { describe, expect, it, vi } from 'vitest';

import {
  pasteIntoTerminalViewport,
  readClipboardText,
  readTerminalViewportSelection,
  readTerminalViewportText,
  TERMINAL_PASTE_EVENT,
  TERMINAL_READ_TEXT_EVENT,
  terminalBufferText,
  terminalCopyMessage,
  terminalCopySelection,
  terminalEditMenuDismissKey,
  type TerminalReadTextDetail,
  writeClipboardText,
} from './terminal-clipboard';

function buffer(rows: Array<string | { text: string; wrapped: true }>) {
  return {
    length: rows.length,
    getLine(y: number) {
      if (y < 0 || y >= rows.length) return undefined;
      const row = rows[y];
      const text = typeof row === 'string' ? row : row.text,
        isWrapped = typeof row !== 'string';
      return { isWrapped, translateToString: (trimRight?: boolean) => (trimRight ? text.trimEnd() : text) };
    },
  };
}

describe('terminalBufferText', () => {
  it('trims row padding and trailing blank rows', () => {
    expect(terminalBufferText(buffer(['$ ls   ', 'a  b   ', '      ', '   ']))).toBe('$ ls\na  b');
  });

  it('rejoins soft-wrapped rows, keeping the wrapped row trailing cells', () => {
    expect(terminalBufferText(buffer(['echo one ', { text: 'two  ', wrapped: true }, 'next']))).toBe(
      'echo one two\nnext',
    );
  });

  it('keeps interior blank lines and handles an empty buffer', () => {
    expect(terminalBufferText(buffer(['a', '', 'b']))).toBe('a\n\nb');
    expect(terminalBufferText(buffer([]))).toBe('');
    expect(terminalBufferText({ length: 2, getLine: () => undefined })).toBe('');
  });

  it('treats a leading wrapped row as its own line', () => {
    expect(terminalBufferText(buffer([{ text: 'tail', wrapped: true }, 'x']))).toBe('tail\nx');
  });
});

describe('viewport clipboard events', () => {
  it('reads text answered by a mounted viewport and undefined when none answers', () => {
    const viewport = new EventTarget() as HTMLElement;
    expect(readTerminalViewportText(viewport)).toBeUndefined();
    viewport.addEventListener(TERMINAL_READ_TEXT_EVENT, (event) => {
      (event as CustomEvent<TerminalReadTextDetail>).detail.text = 'snapshot';
    });
    expect(readTerminalViewportText(viewport)).toBe('snapshot');
  });

  it('asks a mounted viewport for only its selection (HS2-EYR96N)', () => {
    const viewport = new EventTarget() as HTMLElement;
    expect(readTerminalViewportSelection(viewport)).toBeUndefined();
    viewport.addEventListener(TERMINAL_READ_TEXT_EVENT, (event) => {
      const detail = (event as CustomEvent<TerminalReadTextDetail>).detail;
      detail.text = detail.selectionOnly === true ? 'nano 8.4' : 'whole buffer';
    });
    expect(readTerminalViewportSelection(viewport)).toBe('nano 8.4');
    expect(readTerminalViewportText(viewport)).toBe('whole buffer');
  });

  it('dispatches paste text to the viewport', () => {
    const viewport = new EventTarget() as HTMLElement,
      received: string[] = [];
    viewport.addEventListener(TERMINAL_PASTE_EVENT, (event) => {
      received.push((event as CustomEvent<{ text: string }>).detail.text);
    });
    pasteIntoTerminalViewport(viewport, 'ls\n');
    expect(received).toEqual(['ls\n']);
  });
});

describe('readClipboardText', () => {
  it('reports a missing API as unavailable', async () => {
    await expect(readClipboardText(undefined)).resolves.toEqual({ status: 'unavailable' });
    await expect(readClipboardText({} as Clipboard)).resolves.toEqual({ status: 'unavailable' });
  });

  it('returns clipboard text, including empty text', async () => {
    await expect(readClipboardText({ readText: () => Promise.resolve('pwd') })).resolves.toEqual({
      status: 'ok',
      text: 'pwd',
    });
    await expect(readClipboardText({ readText: () => Promise.resolve('') })).resolves.toEqual({
      status: 'ok',
      text: '',
    });
  });

  it('reports a rejected read as denied', async () => {
    await expect(
      readClipboardText({ readText: () => Promise.reject(new DOMException('no', 'NotAllowedError')) }),
    ).resolves.toEqual({ status: 'denied' });
  });
});

describe('writeClipboardText', () => {
  it('uses the async clipboard without the fallback', async () => {
    const writeText = vi.fn(() => Promise.resolve()),
      fallback = vi.fn(() => true);
    await expect(writeClipboardText('a', { writeText }, fallback)).resolves.toBe(true);
    expect(writeText).toHaveBeenCalledWith('a');
    expect(fallback).not.toHaveBeenCalled();
  });

  it('falls back when the async clipboard rejects or is missing', async () => {
    const fallback = vi.fn(() => true);
    await expect(writeClipboardText('a', { writeText: () => Promise.reject(new Error('x')) }, fallback)).resolves.toBe(
      true,
    );
    await expect(writeClipboardText('a', undefined, fallback)).resolves.toBe(true);
    expect(fallback).toHaveBeenCalledTimes(2);
  });

  it('reports failure when the fallback fails or throws', async () => {
    await expect(writeClipboardText('a', undefined, () => false)).resolves.toBe(false);
    await expect(
      writeClipboardText('a', undefined, () => {
        throw new Error('blocked');
      }),
    ).resolves.toBe(false);
  });
});

describe('terminalCopySelection and terminalCopyMessage', () => {
  it('copies the selection when one exists, otherwise everything', () => {
    expect(terminalCopySelection('hello world', 6, 11)).toEqual({ text: 'world', selection: true });
    expect(terminalCopySelection('hello', 2, 2)).toEqual({ text: 'hello', selection: false });
    expect(terminalCopySelection('hello', null, null)).toEqual({ text: 'hello', selection: false });
  });

  it('counts copied lines', () => {
    expect(terminalCopyMessage('a\nb', false)).toBe('Copied terminal text (2 lines)');
    expect(terminalCopyMessage('a', true)).toBe('Copied selection (1 line)');
    expect(terminalCopyMessage('', false)).toBe('Copied terminal text (0 lines)');
  });
});

describe('terminalEditMenuDismissKey (HS2-B06X7Y)', () => {
  it('dismisses only an open menu on a plain Escape', () => {
    expect(terminalEditMenuDismissKey({ key: 'Escape', isComposing: false }, true)).toBe(true);
    expect(terminalEditMenuDismissKey({ key: 'Escape', isComposing: false }, false)).toBe(false);
    expect(terminalEditMenuDismissKey({ key: 'Escape', isComposing: true }, true)).toBe(false);
    expect(terminalEditMenuDismissKey({ key: 'Enter', isComposing: false }, true)).toBe(false);
  });
});
