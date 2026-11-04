import { describe, expect, it } from 'vitest';

import {
  choiceHeader,
  dedent,
  indentOf,
  interruptsParagraph,
  letteredHeader,
  listItem,
  sourceLines,
} from './feedback-choice-lines';

// Line classification behind the shared CHOICE grammar (HS2-EWF0FT); whole-prompt behavior is
// pinned by the TS/Rust corpus in feedback-choices.test.ts.
describe('feedback choice line classification', () => {
  it('splits source lines with offsets', () => {
    expect(sourceLines('a\n\nb')).toEqual([
      { start: 0, end: 2, text: 'a' },
      { start: 2, end: 3, text: '' },
      { start: 3, end: 4, text: 'b' },
    ]);
    expect(sourceLines('')).toEqual([]);
  });

  it('reads CHOICE headers and their inline questions', () => {
    expect(choiceHeader('CHOICE')).toEqual({});
    expect(choiceHeader('**CHOICE:** **Which?**')).toEqual({ question: 'Which?' });
    expect(choiceHeader('CHOICE A: Not a header')).toBeUndefined();
    expect(choiceHeader('CHOICES:')).toBeUndefined();
  });

  it('measures and removes indentation with tabs as four columns', () => {
    expect(indentOf('\t  x')).toBe(6);
    expect(dedent('\t  x', 5)).toBe(' x');
    expect(dedent('  x', 8)).toBe('x');
  });

  it('classifies list items with their content column', () => {
    expect(listItem('  12) Twelve ')).toEqual({ indent: 2, ordered: true, contentColumn: 6, label: 'Twelve' });
    expect(listItem('-\tTab')).toMatchObject({ ordered: false, contentColumn: 5 });
    expect(listItem('-no space')).toBeUndefined();
    expect(listItem('-   ')).toBeUndefined();
  });

  it('reads lettered leads, bold or plain, keyword or bare', () => {
    expect(letteredHeader('**CHOICE A: Label.** Details.')).toEqual({
      style: 'choice-letter',
      ordinal: 1,
      indent: 0,
      label: 'CHOICE A: Label.',
      details: 'Details.',
    });
    expect(letteredHeader('**Option 2 —** Label')).toMatchObject({
      style: 'option-number',
      ordinal: 2,
      label: 'Option 2 — Label',
    });
    expect(letteredHeader('**Option B** — Label')).toMatchObject({ ordinal: 2, label: 'Option B — Label' });
    expect(letteredHeader('   C) Plain')).toMatchObject({
      style: 'bare-letter',
      ordinal: 3,
      indent: 3,
      label: 'C) Plain',
    });
    for (const prose of [
      'Option A is best',
      'Alpha) x',
      'A.B',
      '    A) code',
      '**A) unclosed',
      '**Option A:Label**',
      '1. item',
    ])
      expect(letteredHeader(prose), prose).toBeUndefined();
  });

  it('knows which lines interrupt a paragraph', () => {
    for (const line of ['```', '## Heading', '> quote', '- - -', 'CHOICE:'])
      expect(interruptsParagraph(line)).toBe(true);
    for (const line of ['prose', '#hashtag', '- item', 'A) lead']) expect(interruptsParagraph(line)).toBe(false);
  });
});
