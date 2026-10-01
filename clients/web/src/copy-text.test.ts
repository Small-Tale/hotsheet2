import { afterEach, describe, expect, it, vi } from 'vitest';

import { copyText, copyWithSelection } from './copy-text';

/** A minimal document whose legacy copy command reports `copies` and records what was selected. */
function fakeDocument(copies: boolean | 'throws') {
  const copied: string[] = [],
    focused: string[] = [],
    ranges = ['user-range'],
    selection = {
      get rangeCount() {
        return ranges.length;
      },
      getRangeAt: (index: number) => ranges[index],
      removeAllRanges: () => ranges.splice(0),
      addRange: (range: string) => ranges.push(range),
    };
  class FakeElement {
    focus() {
      focused.push('previous');
    }
  }
  let selected = '';
  const document = {
    activeElement: new FakeElement(),
    getSelection: () => selection,
    createElement: () => ({
      value: '',
      readOnly: false,
      style: {} as Record<string, string>,
      setAttribute: vi.fn(),
      select() {
        selected = this.value;
        ranges.splice(0, ranges.length, 'textarea-range');
      },
      remove: vi.fn(),
    }),
    body: { append: vi.fn() },
    execCommand: vi.fn(() => {
      if (copies === 'throws') throw new Error('unsupported');
      if (copies) copied.push(selected);
      return copies;
    }),
  };
  vi.stubGlobal('HTMLElement', FakeElement);
  vi.stubGlobal('document', document);
  return { copied, focused, ranges, document };
}

describe('copyText (HS2-1A2BQR)', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('writes through the Clipboard API without touching the selection when it is allowed', async () => {
    const page = fakeDocument(true),
      writeText = vi.fn(() => Promise.resolve());
    vi.stubGlobal('navigator', { clipboard: { writeText } });
    await copyText('HS2-0TXM8S');
    expect(writeText).toHaveBeenCalledWith('HS2-0TXM8S');
    expect(page.document.execCommand).not.toHaveBeenCalled();
  });

  it('falls back to a selection copy when Safari refuses the Clipboard API', async () => {
    const page = fakeDocument(true);
    vi.stubGlobal('navigator', {
      clipboard: { writeText: vi.fn(() => Promise.reject(new DOMException('refused', 'NotAllowedError'))) },
    });
    await copyText('HS2-0TXM8S');
    expect(page.copied).toEqual(['HS2-0TXM8S']);
    // The user's selection and focus come back after the temporary textarea is removed.
    expect(page.ranges).toEqual(['user-range']);
    expect(page.focused).toEqual(['previous']);
  });

  it('rejects with the original reason when both copy paths are refused, and copies again later', async () => {
    fakeDocument(false);
    const refused = new DOMException('refused', 'NotAllowedError'),
      writeText = vi.fn(() => Promise.reject(refused));
    vi.stubGlobal('navigator', { clipboard: { writeText } });
    await expect(copyText('HS2-0TXM8S')).rejects.toBe(refused);
    writeText.mockImplementation(() => Promise.resolve() as Promise<never>);
    await expect(copyText('HS2-0TXM8S')).resolves.toBeUndefined();
  });

  it('uses the selection copy when the Clipboard API is missing', async () => {
    const page = fakeDocument(true);
    vi.stubGlobal('navigator', {});
    await copyText('path/to/file');
    expect(page.copied).toEqual(['path/to/file']);
    fakeDocument('throws');
    await expect(copyText('path/to/file')).rejects.toThrow('Copying is not available in this browser.');
  });

  it('reports no copy without a document', () => {
    vi.stubGlobal('document', undefined);
    expect(copyWithSelection('text')).toBe(false);
  });
});
