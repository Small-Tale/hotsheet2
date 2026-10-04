import { describe, expect, it } from 'vitest';

import { INLINE_EDITOR_SELECTOR, inlineEditorEscape } from './inline-editor-escape';

/** An element stand-in that matches the editor selector (when `editor`) and an own-Escape selector. */
function element({ editor = true, own = false } = {}) {
  return {
    matches: (selector: string) =>
      (selector === INLINE_EDITOR_SELECTOR && editor) || (selector === '[name="label"]' && own),
  };
}
const escape = { key: 'Escape', isComposing: false };

describe('inline editor Escape (HS2-Q2T01A)', () => {
  it('finishes a plain inline editor and only stops a field with its own Escape', () => {
    expect(inlineEditorEscape(escape, element())).toBe('finish');
    expect(inlineEditorEscape(escape, element(), '[name="label"]')).toBe('finish');
    expect(inlineEditorEscape(escape, element({ own: true }), '[name="label"]')).toBe('stop');
  });

  it('leaves other keys, IME composition, and non-editors alone', () => {
    expect(inlineEditorEscape({ key: 'Enter', isComposing: false }, element())).toBe('none');
    expect(inlineEditorEscape({ key: 'Escape', isComposing: true }, element())).toBe('none');
    expect(inlineEditorEscape(escape, element({ editor: false }))).toBe('none');
    expect(inlineEditorEscape(escape, null)).toBe('none');
  });

  it('covers text-entry controls but not buttons, toggles, or file pickers', () => {
    expect(INLINE_EDITOR_SELECTOR).toContain('textarea');
    expect(INLINE_EDITOR_SELECTOR).toContain('wa-input');
    for (const type of ['checkbox', 'radio', 'button', 'submit', 'file'])
      expect(INLINE_EDITOR_SELECTOR).toContain(`[type="${type}"]`);
  });
});
