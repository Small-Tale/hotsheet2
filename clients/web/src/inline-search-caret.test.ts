import { placeSearchFieldCaret } from '@kerfjs/ui/search-field';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { restoreInlineSearchCaret } from './inline-search-caret';

vi.mock('@kerfjs/ui/search-field', () => ({ placeSearchFieldCaret: vi.fn() }));

function fixture() {
  const body = {},
    document = { body, activeElement: body },
    editor = { ownerDocument: document } as unknown as HTMLElement,
    querySelector = vi.fn<(selector: string) => HTMLElement | null>(() => editor),
    root = { querySelector } as unknown as ParentNode;
  document.activeElement = editor;
  return { document, editor, querySelector, root };
}

describe('inline-search caret restoration (HS2-PR5TNA)', () => {
  beforeEach(() => vi.clearAllMocks());

  it('uses the latest offset and rendered editor before the next input task', async () => {
    const { root, document, querySelector } = fixture();
    restoreInlineSearchCaret(root, 'workspace', 12);
    restoreInlineSearchCaret(root, 'workspace', 6);
    const replacement = { ownerDocument: document } as unknown as HTMLElement;
    querySelector.mockReturnValue(replacement);
    document.activeElement = document.body;
    expect(placeSearchFieldCaret).not.toHaveBeenCalled();
    await Promise.resolve();
    expect(placeSearchFieldCaret).toHaveBeenCalledExactlyOnceWith(replacement, 6);
    // A later refill/end request must work after the earlier request was consumed.
    restoreInlineSearchCaret(root, 'workspace');
    await Promise.resolve();
    expect(placeSearchFieldCaret).toHaveBeenLastCalledWith(replacement, undefined);
  });

  it('does not steal a newer focus handoff or restore a removed field', async () => {
    const { root, document, querySelector } = fixture();
    restoreInlineSearchCaret(root, 'workspace', 6);
    document.activeElement = { button: true };
    await Promise.resolve();
    expect(placeSearchFieldCaret).not.toHaveBeenCalled();
    restoreInlineSearchCaret(root, 'workspace', 0);
    querySelector.mockReturnValue(null);
    await Promise.resolve();
    expect(placeSearchFieldCaret).not.toHaveBeenCalled();
    restoreInlineSearchCaret(root, 'workspace');
    await Promise.resolve();
    expect(placeSearchFieldCaret).not.toHaveBeenCalled();
  });

  it('keeps restoration requests independent across fields sharing one root', async () => {
    const { root, document, editor, querySelector } = fixture(),
      savedView = { ownerDocument: document } as unknown as HTMLElement;
    querySelector.mockImplementation((selector) => (selector === 'workspace' ? editor : savedView));
    restoreInlineSearchCaret(root, 'workspace', 2);
    restoreInlineSearchCaret(root, 'saved-view', 4);
    await Promise.resolve();
    expect(placeSearchFieldCaret).toHaveBeenCalledTimes(2);
    expect(placeSearchFieldCaret).toHaveBeenCalledWith(editor, 2);
    expect(placeSearchFieldCaret).toHaveBeenCalledWith(savedView, 4);
  });
});
