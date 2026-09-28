import { describe, expect, it } from 'vitest';

import { syncFocusedDraftControl } from './focused-draft-sync';

function control(tagName: string, value: string, caret = value.length) {
  return {
    tagName,
    value,
    selectionStart: caret as number | null,
    selectionEnd: caret as number | null,
    setSelectionRange(start: number, end: number) {
      this.selectionStart = start;
      this.selectionEnd = end;
    },
  };
}

describe('syncFocusedDraftControl (HS2-A4XCXE)', () => {
  it('writes a merged draft into the focused control and keeps the caret with the surrounding text', () => {
    const element = control('TEXTAREA', 'Intro\nEnd', 5);
    expect(syncFocusedDraftControl(element, 'Intro\nEnd', 'Intro\nEnd\nAppended')).toBe(true);
    expect(element.value).toBe('Intro\nEnd\nAppended');
    expect(element.selectionStart).toBe(5);
    const shifted = control('TEXTAREA', 'b\nmine', 6);
    expect(syncFocusedDraftControl(shifted, 'b\nmine', 'a\nb\nmine')).toBe(true);
    expect([shifted.selectionStart, shifted.selectionEnd]).toEqual([8, 8]);
  });

  it('leaves the control alone when the user already typed past what was saved', () => {
    const element = control('TEXTAREA', 'Intro typed more');
    expect(syncFocusedDraftControl(element, 'Intro', 'Intro\nmerged')).toBe(false);
    expect(element.value).toBe('Intro typed more');
    expect(syncFocusedDraftControl(null, 'a', 'b')).toBe(false);
    expect(syncFocusedDraftControl(control('DIV', 'a'), 'a', 'b')).toBe(false);
    expect(syncFocusedDraftControl(control('TEXTAREA', 'same'), 'same', 'same')).toBe(false);
  });

  it('accepts a custom match for trimmed fields', () => {
    const input = control('INPUT', 'Title ');
    expect(syncFocusedDraftControl(input, 'Title', 'Title, merged', (value) => value.trim() === 'Title')).toBe(true);
    expect(input.value).toBe('Title, merged');
  });
});
