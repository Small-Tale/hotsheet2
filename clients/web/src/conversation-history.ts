import type { ConversationMessage } from './ai-conversation';

/**
 * Shell-like recall of previously sent messages in the AI chat composer (HS2-AJQKK0). Up and
 * Down step through one conversation's own sent messages; the in-progress draft is kept aside
 * while browsing and comes back when the person steps past the newest entry.
 */
export type ConversationHistoryDirection = 'previous' | 'next';

/** Where the person is while browsing: an index into the history plus the draft set aside. */
export interface ConversationHistoryCursor {
  readonly index: number;
  readonly draft: string;
}

export interface ConversationHistoryStep {
  /** `undefined` means the person is back on their own draft. */
  readonly cursor: ConversationHistoryCursor | undefined;
  readonly value: string;
}

/**
 * The conversation's sent messages, oldest first. Blank sends are skipped and consecutive
 * repeats collapse to one entry, like a shell's `ignoredups` history.
 */
export function conversationSentHistory(messages: readonly ConversationMessage[]): string[] {
  const history: string[] = [];
  for (const message of messages) {
    if (message.role !== 'user' || !message.content.trim()) continue;
    if (history.at(-1) !== message.content) history.push(message.content);
  }
  return history;
}

interface HistoryKey {
  readonly key: string;
  readonly shiftKey: boolean;
  readonly altKey: boolean;
  readonly metaKey: boolean;
  readonly ctrlKey: boolean;
  readonly isComposing?: boolean;
}

interface HistoryField {
  readonly value: string;
  readonly selectionStart: number | null;
  readonly selectionEnd: number | null;
}

/**
 * Which way an arrow key browses history, or `undefined` when it should keep moving the caret.
 * Up recalls only while the caret sits on the field's first line (an empty field qualifies);
 * Down only on its last line. A selection, a modifier, or an IME composition never recalls.
 */
export function conversationHistoryDirection(
  event: HistoryKey,
  field: HistoryField,
): ConversationHistoryDirection | undefined {
  if (event.shiftKey || event.altKey || event.metaKey || event.ctrlKey || event.isComposing) return;
  const { value, selectionStart, selectionEnd } = field;
  if (selectionStart === null || selectionEnd === null || selectionStart !== selectionEnd) return;
  if (event.key === 'ArrowUp') return value.slice(0, selectionStart).includes('\n') ? undefined : 'previous';
  if (event.key === 'ArrowDown') return value.slice(selectionEnd).includes('\n') ? undefined : 'next';
  return;
}

/**
 * Move one step through `history`. Returns `undefined` when there is nowhere to go (no history,
 * already at the oldest entry, or Down while on the draft), so the key keeps its default effect.
 */
export function stepConversationHistory(
  history: readonly string[],
  cursor: ConversationHistoryCursor | undefined,
  direction: ConversationHistoryDirection,
  currentValue: string,
): ConversationHistoryStep | undefined {
  if (!history.length) return cursor ? { cursor: undefined, value: cursor.draft } : undefined;
  if (direction === 'previous') {
    if (!cursor) return { cursor: { index: history.length - 1, draft: currentValue }, value: history.at(-1)! };
    const index = Math.min(cursor.index, history.length) - 1;
    if (index < 0) return;
    return { cursor: { index, draft: cursor.draft }, value: history[index] };
  }
  if (!cursor) return;
  const index = cursor.index + 1;
  if (index >= history.length) return { cursor: undefined, value: cursor.draft };
  return { cursor: { index, draft: cursor.draft }, value: history[index] };
}
