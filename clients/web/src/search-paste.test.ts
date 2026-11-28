import { describe, expect, it, vi } from 'vitest';

import { pastePlainSearchText, searchPasteText } from './search-paste';

describe('searchPasteText (HS2-PQWZRG)', () => {
  it('folds every line-break style into a space and keeps other text as is', () => {
    expect(searchPasteText('Big title\nline two\r\nthree\rfour')).toBe('Big title line two three four');
    expect(searchPasteText('  tag:docs\tparser ')).toBe('  tag:docs\tparser ');
    expect(searchPasteText('\n\n')).toBe('  ');
  });

  it('treats a missing or empty clipboard as no text', () => {
    expect(searchPasteText(undefined)).toBe('');
    expect(searchPasteText('')).toBe('');
  });
});

describe('pastePlainSearchText (HS2-PQWZRG)', () => {
  // Node-shaped fakes of the editor and the paste event: only the members the handler reads.
  const paste = (plain: string | undefined) => {
    const types: Record<string, string> = { 'text/html': '<h1>Big <b>title</b></h1>' };
    if (plain !== undefined) types['text/plain'] = plain;
    const event = {
      defaultPrevented: false,
      clipboardData: { getData: (type: string) => types[type] ?? '' },
      preventDefault() {
        event.defaultPrevented = true;
      },
    };
    return event;
  };
  const editor = (result: boolean | Error, editable = true) => {
    const execCommand = vi.fn<(command: string, ui: boolean, value: string) => boolean>(() => {
      if (result instanceof Error) throw result;
      return result;
    });
    const caret = {},
      selection = { rangeCount: 1, getRangeAt: () => caret, removeAllRanges: vi.fn(), addRange: vi.fn() };
    return {
      execCommand,
      selection,
      caret,
      node: { isContentEditable: editable, ownerDocument: { execCommand, getSelection: () => selection } },
    };
  };
  const run = (event: ReturnType<typeof paste>, target: ReturnType<typeof editor>) => {
    pastePlainSearchText(event as unknown as ClipboardEvent, target.node as unknown as HTMLElement);
  };

  it('inserts only the plain text, on one line, through the undoable editing command', () => {
    const target = editor(true),
      event = paste('Big title\nline two');
    run(event, target);
    expect(target.execCommand).toHaveBeenCalledExactlyOnceWith('insertText', false, 'Big title line two');
    expect(event.defaultPrevented).toBe(true);
  });

  it('reapplies the caret before and after the insertion so Undo keeps the paste as its own step', () => {
    const target = editor(true);
    run(paste('title'), target);
    expect(target.selection.removeAllRanges).toHaveBeenCalledTimes(2);
    expect(target.selection.addRange.mock.calls).toEqual([[target.caret], [target.caret]]);
    const refused = editor(false);
    run(paste('title'), refused);
    expect(refused.selection.addRange).toHaveBeenCalledOnce();
    const caretless = editor(true);
    caretless.selection.rangeCount = 0;
    run(paste('title'), caretless);
    expect(caretless.selection.addRange).not.toHaveBeenCalled();
    expect(caretless.execCommand).toHaveBeenCalledOnce();
  });

  it('leaves the paste to Kerf when the browser refuses or lacks the command', () => {
    for (const result of [false, new Error('unsupported')]) {
      const target = editor(result),
        event = paste('title');
      run(event, target);
      expect(target.execCommand).toHaveBeenCalledOnce();
      expect(event.defaultPrevented).toBe(false);
    }
  });

  it('blocks a rich paste with no plain text without inserting anything', () => {
    for (const plain of [undefined, '']) {
      const target = editor(true),
        event = paste(plain);
      run(event, target);
      expect(target.execCommand).not.toHaveBeenCalled();
      expect(event.defaultPrevented).toBe(true);
    }
  });

  it('ignores a disabled editor and a paste another handler already took', () => {
    const disabled = editor(true, false),
      first = paste('title');
    run(first, disabled);
    expect(first.defaultPrevented).toBe(false);
    expect(disabled.execCommand).not.toHaveBeenCalled();
    const target = editor(true),
      taken = paste('title');
    taken.preventDefault();
    run(taken, target);
    expect(target.execCommand).not.toHaveBeenCalled();
  });
});
