import { signal } from 'kerfjs';
import { describe, expect, it } from 'vitest';

import type { ConversationMessage, ConversationState } from './ai-conversation';
import {
  CONVERSATION_READS_STORAGE_KEY,
  conversationUnreadCount,
  conversationUnreadCounts,
  createConversationUnreadTracker,
  loadConversationReadMarks,
  projectUnreadCount,
  reconcileConversationReadMarks,
  settledAssistantCount,
  trackDocumentVisibility,
} from './conversation-unread';

const user = (id: string): ConversationMessage => ({ id, role: 'user', content: id }),
  reply = (id: string, status: ConversationMessage['status'] = 'completed'): ConversationMessage => ({
    id,
    role: 'assistant',
    content: id,
    status,
  });
const chat = (...messages: ConversationMessage[]): ConversationState => ({ messages });

function memoryStorage(initial: Record<string, string> = {}) {
  const values = new Map(Object.entries(initial));
  return {
    values,
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => {
      values.set(key, value);
    },
  };
}

describe('unread derivation (HS2-59CXAC)', () => {
  it('counts settled assistant messages only', () => {
    expect(settledAssistantCount(undefined)).toBe(0);
    expect(
      settledAssistantCount(
        chat(user('u1'), reply('a1'), reply('a2', 'failed'), reply('a3', 'interrupted'), reply('a4', 'streaming')),
      ),
    ).toBe(3);
    expect(conversationUnreadCount(chat(reply('a1')), undefined)).toBe(0);
    expect(conversationUnreadCount(chat(reply('a1'), reply('a2')), 1)).toBe(1);
    expect(conversationUnreadCount(chat(reply('a1')), 4)).toBe(0);
  });

  it('starts new conversations read, reads visible ones, lowers shrunk marks, and drops vanished ones', () => {
    const states = { seen: chat(reply('a1'), reply('a2')), fresh: chat(reply('b1')), open: chat(reply('c1')) };
    const marks = reconcileConversationReadMarks({ seen: 1, open: 0, gone: 4 }, states, new Set(['open']));
    expect(marks).toEqual({ seen: 1, fresh: 1, open: 1 });
    expect(conversationUnreadCounts(marks, states)).toEqual({ seen: 1 });
    // Unchanged input returns the same object so callers can skip writes.
    expect(reconcileConversationReadMarks(marks, states, new Set())).toBe(marks);
    expect(reconcileConversationReadMarks({ seen: 9 }, { seen: chat(reply('a1')) }, new Set())).toEqual({ seen: 1 });
  });

  it('sums a project once per conversation', () => {
    expect(projectUnreadCount({ a: 2, b: 3, other: 5 }, ['a', 'b', 'a', 'missing'])).toBe(5);
    expect(projectUnreadCount({}, [])).toBe(0);
  });

  it('loads only well-formed persisted marks', () => {
    expect(loadConversationReadMarks(undefined)).toEqual({});
    expect(loadConversationReadMarks(memoryStorage({ [CONVERSATION_READS_STORAGE_KEY]: 'not json' }))).toEqual({});
    expect(loadConversationReadMarks(memoryStorage({ [CONVERSATION_READS_STORAGE_KEY]: '[1]' }))).toEqual({});
    expect(
      loadConversationReadMarks(
        memoryStorage({ [CONVERSATION_READS_STORAGE_KEY]: JSON.stringify({ a: 2, b: -1, c: 1.5, d: 'x', e: 0 }) }),
      ),
    ).toEqual({ a: 2, e: 0 });
  });
});

describe('unread tracker transitions (HS2-59CXAC)', () => {
  function tracker(initial: Record<string, ConversationState> = {}, storage = memoryStorage()) {
    const states = signal<Record<string, ConversationState>>(initial),
      visible = signal<string[]>([]),
      pageShown = signal(true),
      unread = createConversationUnreadTracker({
        conversationStates: states,
        visibleConnectionIds: () => visible.value,
        documentVisible: () => pageShown.value,
        storage,
      });
    const update = (id: string, ...messages: ConversationMessage[]) => {
      states.value = { ...states.value, [id]: chat(...messages) };
    };
    return { states, visible, pageShown, unread, update, storage };
  }

  it('counts replies that settle in an open chat while the page is hidden, then reads them once shown (HS2-1ZPD4A)', () => {
    const { unread, visible, pageShown, update } = tracker({ chat: chat(reply('a1')) });
    visible.value = ['chat'];
    update('chat', reply('a1'), reply('a2'));
    expect(unread.projectUnreadCount(['chat'])).toBe(0);
    // Hidden page: the open chat no longer reads, so new replies count.
    pageShown.value = false;
    update('chat', reply('a1'), reply('a2'), reply('a3'));
    update('chat', reply('a1'), reply('a2'), reply('a3'), reply('a4'));
    expect(unread.projectUnreadCount(['chat'])).toBe(2);
    // Hiding and showing again with the chat closed keeps them unread.
    visible.value = [];
    pageShown.value = true;
    expect(unread.projectUnreadCount(['chat'])).toBe(2);
    // Shown with the chat open reads them; a repeated hidden -> shown with no new reply stays clear.
    visible.value = ['chat'];
    expect(unread.projectUnreadCount(['chat'])).toBe(0);
    pageShown.value = false;
    pageShown.value = true;
    expect(unread.projectUnreadCount(['chat'])).toBe(0);
  });

  it('treats restored history as read, then counts replies that settle while hidden', () => {
    const { unread, update } = tracker({ chat: chat(user('u1'), reply('a1')) });
    expect(unread.projectUnreadCount(['chat'])).toBe(0);
    update('chat', user('u1'), reply('a1'), user('u2'), reply('a2', 'streaming'));
    expect(unread.projectUnreadCount(['chat'])).toBe(0);
    update('chat', user('u1'), reply('a1'), user('u2'), reply('a2'));
    expect(unread.projectUnreadCount(['chat'])).toBe(1);
  });

  it('clears on view, stays clear while visible, and counts again after hiding', () => {
    const { unread, visible, update } = tracker({ chat: chat(reply('a1')) });
    update('chat', reply('a1'), reply('a2'), reply('a3'));
    expect(unread.projectUnreadCount(['chat'])).toBe(2);
    visible.value = ['chat'];
    expect(unread.projectUnreadCount(['chat'])).toBe(0);
    update('chat', reply('a1'), reply('a2'), reply('a3'), reply('a4'));
    expect(unread.projectUnreadCount(['chat'])).toBe(0);
    visible.value = [];
    update('chat', reply('a1'), reply('a2'), reply('a3'), reply('a4'), reply('a5'));
    expect(unread.projectUnreadCount(['chat'])).toBe(1);
  });

  it('keeps each project and conversation separate, and viewing one chat leaves the others unread', () => {
    const { unread, visible, update } = tracker({ a1: chat(), a2: chat(), b1: chat() });
    update('a1', reply('x'));
    update('a2', reply('y'), reply('z'));
    update('b1', reply('w'));
    expect(unread.projectUnreadCount(['a1', 'a2'])).toBe(3);
    expect(unread.projectUnreadCount(['b1'])).toBe(1);
    visible.value = ['a2'];
    expect(unread.projectUnreadCount(['a1', 'a2'])).toBe(1);
    expect(unread.projectUnreadCount(['b1'])).toBe(1);
  });

  it('survives a conversation being cleared and refilled', () => {
    const { unread, states, update } = tracker({ chat: chat() });
    update('chat', reply('a1'), reply('a2'));
    expect(unread.projectUnreadCount(['chat'])).toBe(2);
    states.value = { chat: chat() };
    expect(unread.projectUnreadCount(['chat'])).toBe(0);
    update('chat', reply('b1'));
    expect(unread.projectUnreadCount(['chat'])).toBe(1);
    // Removing the conversation forgets it; a later one with the same id starts read.
    states.value = {};
    expect(unread.readMarks.value).toEqual({});
    update('chat', reply('c1'), reply('c2'));
    expect(unread.projectUnreadCount(['chat'])).toBe(0);
  });

  it('persists marks so unread replies survive a reload, and tolerates a failing store', () => {
    const storage = memoryStorage(),
      first = tracker({ chat: chat() }, storage);
    first.update('chat', reply('a1'));
    expect(first.unread.projectUnreadCount(['chat'])).toBe(1);
    first.unread.dispose();
    const reloaded = tracker({ chat: chat(reply('a1')) }, storage);
    expect(reloaded.unread.projectUnreadCount(['chat'])).toBe(1);
    const failing = {
      getItem: () => null,
      setItem: () => {
        throw new Error('quota');
      },
    };
    const quiet = tracker({}, failing as never);
    expect(() => {
      quiet.update('chat', reply('a1'));
    }).not.toThrow();
  });
});

describe('page visibility signal (HS2-1ZPD4A)', () => {
  it('follows visibilitychange through hidden -> visible -> hidden and stops after dispose', () => {
    const listeners = new Set<() => void>();
    const page = {
      visibilityState: 'hidden' as DocumentVisibilityState,
      addEventListener: (_type: 'visibilitychange', listener: () => void) => {
        listeners.add(listener);
      },
      removeEventListener: (_type: 'visibilitychange', listener: () => void) => {
        listeners.delete(listener);
      },
    };
    const change = (state: DocumentVisibilityState) => {
      page.visibilityState = state;
      for (const listener of listeners) listener();
    };
    const tracked = trackDocumentVisibility(page);
    expect(tracked.visible.value).toBe(false);
    change('visible');
    expect(tracked.visible.value).toBe(true);
    change('hidden');
    expect(tracked.visible.value).toBe(false);
    tracked.dispose();
    expect(listeners.size).toBe(0);
    change('visible');
    expect(tracked.visible.value).toBe(false);
  });
});
