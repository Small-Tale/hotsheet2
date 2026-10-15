import { type ConversationMessage, conversationTimeline } from './ai-conversation';
import { isAppleShortcutPlatform } from './keyboard-shortcuts';

/**
 * Message selection in an AI chat transcript (HS2-WV88W3): any set of messages, kept in transcript
 * order, plus the anchor a Shift-extended range starts from.
 */
export interface ConversationMessageSelection {
  readonly messageIds: readonly string[];
  readonly anchorId?: string;
}

/** Platform-normalized intent of one pick: `toggle` is Cmd on Apple platforms and Ctrl elsewhere. */
export interface ConversationSelectionModifiers {
  readonly toggle: boolean;
  readonly extend: boolean;
}

export const EMPTY_CONVERSATION_SELECTION: ConversationMessageSelection = { messageIds: [] };

export function conversationSelectionModifiers(
  event: Pick<MouseEvent | KeyboardEvent, 'metaKey' | 'ctrlKey' | 'shiftKey'>,
  apple = isAppleShortcutPlatform(),
): ConversationSelectionModifiers {
  return { toggle: apple ? event.metaKey : event.ctrlKey, extend: event.shiftKey };
}

/** The selection hint shown before anything is selected, naming the host platform's toggle key. */
export function conversationSelectionHint(apple = isAppleShortcutPlatform()): string {
  return `Click to select. ${apple ? 'Cmd' : 'Ctrl'}-click to add or remove, Shift-click for a range.`;
}

/**
 * Messages in the order the transcript shows them (by sequence, like the rendered timeline), which
 * can differ from arrival order; ranges, gaps, and copies all follow what the person sees.
 */
function transcriptOrder(messages: readonly ConversationMessage[]): ConversationMessage[] {
  return conversationTimeline(messages).flatMap((group) => (group.kind === 'message' ? [group.message] : []));
}

function ordered(messages: readonly ConversationMessage[], ids: ReadonlySet<string>): string[] {
  return messages.filter((message) => ids.has(message.id)).map((message) => message.id);
}

/**
 * The selection after picking `messageId`:
 * - plain: select only that message; picking the sole selected message clears the selection;
 * - toggle: add or remove that message and make it the anchor;
 * - extend: select the inclusive range from the anchor (or the picked message without one);
 *   with toggle as well, the range is added to the existing selection.
 * Unknown messages leave the selection unchanged, and stale ids are dropped.
 */
export function conversationSelectionAfterPick(
  messages: readonly ConversationMessage[],
  selection: ConversationMessageSelection,
  messageId: string,
  modifiers: ConversationSelectionModifiers,
): ConversationMessageSelection {
  const shown = transcriptOrder(messages);
  const picked = shown.findIndex((message) => message.id === messageId);
  if (picked < 0) return selection;
  const current = new Set(ordered(shown, new Set(selection.messageIds))),
    anchor = selection.anchorId ? shown.findIndex((message) => message.id === selection.anchorId) : -1;
  if (modifiers.extend && anchor >= 0) {
    const range = shown.slice(Math.min(anchor, picked), Math.max(anchor, picked) + 1).map((message) => message.id),
      next = modifiers.toggle ? new Set([...current, ...range]) : new Set(range);
    return { messageIds: ordered(shown, next), anchorId: selection.anchorId };
  }
  if (modifiers.toggle) {
    const next = new Set(current);
    if (next.has(messageId)) next.delete(messageId);
    else next.add(messageId);
    return { messageIds: ordered(shown, next), anchorId: messageId };
  }
  if (!modifiers.extend && current.size === 1 && current.has(messageId)) return EMPTY_CONVERSATION_SELECTION;
  return { messageIds: [messageId], anchorId: messageId };
}

/** The selected messages that still exist, in the transcript's displayed order. */
export function selectedConversationMessagesFor(
  messages: readonly ConversationMessage[],
  selection: ConversationMessageSelection | undefined,
): ConversationMessage[] {
  if (!selection?.messageIds.length) return [];
  const ids = new Set(selection.messageIds);
  return transcriptOrder(messages).filter((message) => ids.has(message.id));
}

/**
 * For each selected message after the first, how many unselected messages lie between it and the
 * previous selected one; only gaps inside the selection are counted.
 */
export function conversationSelectionGaps(
  messages: readonly ConversationMessage[],
  selected: readonly ConversationMessage[],
): Map<string, number> {
  const gaps = new Map<string, number>(),
    ids = new Set(selected.map((message) => message.id));
  let previous = -1;
  transcriptOrder(messages).forEach((message, index) => {
    if (!ids.has(message.id)) return;
    if (previous >= 0 && index - previous > 1) gaps.set(message.id, index - previous - 1);
    previous = index;
  });
  return gaps;
}
