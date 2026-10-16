import { describe, expect, it } from 'vitest';

import type { ConversationMessage } from './ai-conversation';
import {
  type ConversationHistoryCursor,
  type ConversationHistoryDirection,
  conversationHistoryDirection,
  conversationSentHistory,
  stepConversationHistory,
} from './conversation-history';

const user = (id: string, content: string): ConversationMessage => ({ id, role: 'user', content }),
  assistant = (id: string, content: string): ConversationMessage => ({ id, role: 'assistant', content });

describe('conversationSentHistory (HS2-AJQKK0)', () => {
  it('keeps only sent user messages, oldest first, without blanks or consecutive repeats', () => {
    expect(
      conversationSentHistory([
        user('1', 'first'),
        assistant('1a', 'reply'),
        user('2', '   '),
        user('3', 'second'),
        user('4', 'second'),
        assistant('4a', 'second'),
        user('5', 'first'),
      ]),
    ).toEqual(['first', 'second', 'first']);
    expect(conversationSentHistory([])).toEqual([]);
  });
});

describe('conversationHistoryDirection (HS2-AJQKK0)', () => {
  const key = (
      name: string,
      modifiers: Partial<Record<'shiftKey' | 'altKey' | 'metaKey' | 'ctrlKey', boolean>> = {},
    ) => ({
      key: name,
      shiftKey: false,
      altKey: false,
      metaKey: false,
      ctrlKey: false,
      ...modifiers,
    }),
    field = (value: string, start: number | null, end = start) => ({ value, selectionStart: start, selectionEnd: end });

  it('browses from an empty composer in both directions', () => {
    expect(conversationHistoryDirection(key('ArrowUp'), field('', 0))).toBe('previous');
    expect(conversationHistoryDirection(key('ArrowDown'), field('', 0))).toBe('next');
  });

  it('recalls only from the first line going up and the last line going down', () => {
    const text = 'one\ntwo\nthree';
    expect(conversationHistoryDirection(key('ArrowUp'), field(text, 2))).toBe('previous');
    expect(conversationHistoryDirection(key('ArrowUp'), field(text, 3))).toBe('previous');
    expect(conversationHistoryDirection(key('ArrowUp'), field(text, 4))).toBeUndefined();
    expect(conversationHistoryDirection(key('ArrowDown'), field(text, 6))).toBeUndefined();
    expect(conversationHistoryDirection(key('ArrowDown'), field(text, 8))).toBe('next');
    expect(conversationHistoryDirection(key('ArrowDown'), field(text, text.length))).toBe('next');
  });

  it('leaves selections, modified arrows, IME composition, unknown carets, and other keys alone', () => {
    expect(conversationHistoryDirection(key('ArrowUp'), field('text', 0, 2))).toBeUndefined();
    expect(conversationHistoryDirection(key('ArrowUp'), field('text', null))).toBeUndefined();
    for (const modifier of ['shiftKey', 'altKey', 'metaKey', 'ctrlKey'] as const)
      expect(conversationHistoryDirection(key('ArrowUp', { [modifier]: true }), field('', 0))).toBeUndefined();
    expect(conversationHistoryDirection({ ...key('ArrowUp'), isComposing: true }, field('', 0))).toBeUndefined();
    expect(conversationHistoryDirection(key('ArrowLeft'), field('', 0))).toBeUndefined();
  });
});

describe('stepConversationHistory transitions (HS2-AJQKK0)', () => {
  const history = ['first', 'second', 'third'];
  /** Walk a key sequence like a person would, returning every visible composer value. */
  function walk(directions: ConversationHistoryDirection[], draft: string, entries: readonly string[] = history) {
    let cursor: ConversationHistoryCursor | undefined,
      value = draft;
    const shown: string[] = [];
    for (const direction of directions) {
      const step = stepConversationHistory(entries, cursor, direction, value);
      if (step) ({ cursor, value } = step);
      shown.push(value);
    }
    return { shown, cursor };
  }

  it('steps back to the oldest entry and stops there', () => {
    expect(walk(['previous', 'previous', 'previous', 'previous'], '').shown).toEqual([
      'third',
      'second',
      'first',
      'first',
    ]);
  });

  it('returns to the preserved in-progress draft after stepping past the newest entry', () => {
    const result = walk(['previous', 'previous', 'next', 'next', 'next'], 'half-typed');
    expect(result.shown).toEqual(['third', 'second', 'third', 'half-typed', 'half-typed']);
    expect(result.cursor).toBeUndefined();
  });

  it('ignores Down while on the draft and does nothing without history', () => {
    expect(stepConversationHistory(history, undefined, 'next', 'draft')).toBeUndefined();
    expect(stepConversationHistory([], undefined, 'previous', 'draft')).toBeUndefined();
  });

  it('restores the draft when the history empties out mid-browse', () => {
    expect(stepConversationHistory([], { index: 1, draft: 'mine' }, 'previous', 'second')).toEqual({
      cursor: undefined,
      value: 'mine',
    });
  });

  it('clamps a stale cursor when the history shrank', () => {
    expect(stepConversationHistory(['only'], { index: 4, draft: 'mine' }, 'previous', 'x')).toEqual({
      cursor: { index: 0, draft: 'mine' },
      value: 'only',
    });
    expect(stepConversationHistory(['only'], { index: 4, draft: 'mine' }, 'next', 'x')).toEqual({
      cursor: undefined,
      value: 'mine',
    });
  });

  it('starts a fresh browse from the newest entry after returning to the draft', () => {
    expect(walk(['previous', 'next', 'previous'], 'draft').shown).toEqual(['third', 'draft', 'third']);
  });

  it('includes a message sent after a browse ended at the newest position', () => {
    expect(walk(['previous'], '', [...history, 'fourth']).shown).toEqual(['fourth']);
  });
});
