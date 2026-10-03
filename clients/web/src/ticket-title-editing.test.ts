import { describe, expect, it } from 'vitest';

import { normalizeTicketTitleField, singleLineTicketTitle, ticketTitleKeyFinishesEdit } from './ticket-title-editing';

/** The slice of a live textarea the normalizer reads and writes. */
function fakeField(value: string, selectionEnd: number) {
  const field = {
    value,
    selectionEnd,
    selection: [selectionEnd, selectionEnd] as [number, number],
    setSelectionRange(start: number, end: number) {
      field.selection = [start, end];
    },
  };
  return field;
}

describe('single-line ticket title editing (HS2-98ZVPE)', () => {
  it('collapses each line-break run and the whitespace around it into one space', () => {
    expect(singleLineTicketTitle('Fix the\nparser')).toBe('Fix the parser');
    expect(singleLineTicketTitle('Fix the  \r\n\n   parser\rcrash')).toBe('Fix the parser crash');
    expect(singleLineTicketTitle('Trailing break\n')).toBe('Trailing break ');
    expect(singleLineTicketTitle('Keeps  inner\tspacing')).toBe('Keeps  inner\tspacing');
    expect(singleLineTicketTitle('')).toBe('');
  });

  it('finishes the edit on Enter but not during IME composition or on other keys', () => {
    const key = (init: { key: string; isComposing?: boolean; shiftKey?: boolean }) =>
      ({ isComposing: false, shiftKey: false, ...init }) as KeyboardEvent;
    expect(ticketTitleKeyFinishesEdit(key({ key: 'Enter' }))).toBe(true);
    expect(ticketTitleKeyFinishesEdit(key({ key: 'Enter', shiftKey: true }))).toBe(true);
    expect(ticketTitleKeyFinishesEdit(key({ key: 'Enter', isComposing: true }))).toBe(false);
    expect(ticketTitleKeyFinishesEdit(key({ key: 'Escape' }))).toBe(false);
    expect(ticketTitleKeyFinishesEdit(key({ key: 'a' }))).toBe(false);
  });

  it('rewrites a pasted multi-line value and keeps the caret after the pasted text', () => {
    const field = fakeField('Fix the first\nsecond line parser', 'Fix the first\nsecond line'.length);
    expect(normalizeTicketTitleField(field as unknown as HTMLTextAreaElement)).toBe('Fix the first second line parser');
    expect(field.value).toBe('Fix the first second line parser');
    expect(field.selection).toEqual([25, 25]);
  });

  it('leaves a single-line field and its selection untouched', () => {
    const field = fakeField('Already one line', 3);
    expect(normalizeTicketTitleField(field as unknown as HTMLTextAreaElement)).toBe('Already one line');
    expect(field.selection).toEqual([3, 3]);
  });

  it('normalizes repeatedly without drifting: break, refill, break again', () => {
    const field = fakeField('a\nb', 3);
    expect(normalizeTicketTitleField(field as unknown as HTMLTextAreaElement)).toBe('a b');
    field.value = 'a b\n\nc';
    field.selectionEnd = field.value.length;
    expect(normalizeTicketTitleField(field as unknown as HTMLTextAreaElement)).toBe('a b c');
    expect(field.selection).toEqual([5, 5]);
    field.value = '';
    field.selectionEnd = 0;
    expect(normalizeTicketTitleField(field as unknown as HTMLTextAreaElement)).toBe('');
  });
});
