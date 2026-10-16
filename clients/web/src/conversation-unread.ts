import { effect, signal } from 'kerfjs';

import type { ConversationState } from './ai-conversation';

/**
 * Unread AI chat messages (HS2-59CXAC). A conversation's read mark is how many of its settled
 * assistant messages the person has seen; anything settled beyond the mark is unread. Viewing a
 * conversation (its dialog open, or its drawer tab shown) moves the mark to the current count. A
 * conversation seen for the first time starts fully read, so restored history is never "new".
 */
export type ConversationReadMarks = Readonly<Record<string, number>>;

export const CONVERSATION_READS_STORAGE_KEY = 'hotsheet.ai-conversation-reads.v1';

/** Assistant messages that finished arriving; a still-streaming reply is not yet unread. */
export function settledAssistantCount(state: ConversationState | undefined): number {
  return (
    state?.messages.filter((message) => message.role === 'assistant' && message.status !== 'streaming').length ?? 0
  );
}

/** Unread messages in one conversation; unknown conversations have none. */
export function conversationUnreadCount(state: ConversationState | undefined, mark: number | undefined): number {
  return mark === undefined ? 0 : Math.max(0, settledAssistantCount(state) - mark);
}

/**
 * Advance the marks for `states`: new conversations start read, visible ones are read up to their
 * current count, a conversation that shrank (replaced or cleared) lowers its mark to match, and
 * marks for vanished conversations are dropped. Returns `marks` itself when nothing changed.
 */
export function reconcileConversationReadMarks(
  marks: ConversationReadMarks,
  states: Readonly<Record<string, ConversationState>>,
  visible: ReadonlySet<string>,
): ConversationReadMarks {
  const next: Record<string, number> = {};
  let changed = false;
  for (const [id, state] of Object.entries(states)) {
    const settled = settledAssistantCount(state),
      mark = id in marks ? marks[id] : undefined,
      wanted = mark === undefined || visible.has(id) || mark > settled ? settled : mark;
    next[id] = wanted;
    if (wanted !== mark) changed = true;
  }
  // Marks for conversations that no longer exist are dropped so storage does not grow without bound.
  if (!changed && Object.keys(marks).some((id) => !(id in states))) changed = true;
  return changed ? next : marks;
}

/** Unread counts for every conversation that has any. */
export function conversationUnreadCounts(
  marks: ConversationReadMarks,
  states: Readonly<Record<string, ConversationState>>,
): Record<string, number> {
  const counts: Record<string, number> = {};
  for (const [id, state] of Object.entries(states)) {
    const unread = conversationUnreadCount(state, marks[id]);
    if (unread) counts[id] = unread;
  }
  return counts;
}

/** Total unread across a project's conversations, each counted once. */
export function projectUnreadCount(
  unreadByConnection: Readonly<Record<string, number>>,
  connectionIds: Iterable<string>,
): number {
  let total = 0;
  for (const id of new Set(connectionIds)) total += unreadByConnection[id] ?? 0;
  return total;
}

type StorageLike = Pick<Storage, 'getItem' | 'setItem'>;

export function loadConversationReadMarks(storage: StorageLike | undefined): ConversationReadMarks {
  try {
    const parsed: unknown = JSON.parse(storage?.getItem(CONVERSATION_READS_STORAGE_KEY) ?? '{}');
    if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) return {};
    return Object.fromEntries(
      Object.entries(parsed).filter(
        (entry): entry is [string, number] =>
          typeof entry[1] === 'number' && Number.isSafeInteger(entry[1]) && entry[1] >= 0,
      ),
    );
  } catch {
    return {};
  }
}

function sameCounts(left: Readonly<Record<string, number>>, right: Readonly<Record<string, number>>) {
  const keys = Object.keys(left);
  return keys.length === Object.keys(right).length && keys.every((key) => left[key] === right[key]);
}

export interface ConversationUnreadDependencies {
  readonly conversationStates: { readonly value: Readonly<Record<string, ConversationState>> };
  /** Connection ids whose transcript the person can currently see. Read reactively. */
  readonly visibleConnectionIds: () => Iterable<string>;
  readonly storage?: StorageLike;
}

/** Keep read marks current as conversations stream and as chats are shown or hidden. */
export function createConversationUnreadTracker(dependencies: ConversationUnreadDependencies) {
  const { storage } = dependencies;
  const readMarks = signal<ConversationReadMarks>(loadConversationReadMarks(storage)),
    unreadByConnection = signal<Record<string, number>>({});
  const dispose = effect(() => {
    const states = dependencies.conversationStates.value,
      visible = new Set(dependencies.visibleConnectionIds()),
      current = readMarks.peek(),
      marks = reconcileConversationReadMarks(current, states, visible);
    if (marks !== current) {
      readMarks.value = marks;
      try {
        storage?.setItem(CONVERSATION_READS_STORAGE_KEY, JSON.stringify(marks));
      } catch {
        /* storage quota/privacy mode only loses unread state across reloads */
      }
    }
    const unread = conversationUnreadCounts(marks, states);
    if (!sameCounts(unread, unreadByConnection.peek())) unreadByConnection.value = unread;
  });
  return {
    readMarks,
    unreadByConnection,
    projectUnreadCount: (connectionIds: Iterable<string>) =>
      projectUnreadCount(unreadByConnection.value, connectionIds),
    dispose,
  };
}
