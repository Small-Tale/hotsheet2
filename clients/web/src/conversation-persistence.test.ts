import { afterEach, describe, expect, it, vi } from 'vitest';

import {
  CONVERSATION_STORAGE_KEY,
  createConversationPersistence,
  loadConversationStates,
  saveConversationStates,
} from './conversation-persistence';

afterEach(() => vi.useRealTimers());

describe('durable AI conversation state (HS2-YHQCS2)', () => {
  it('round-trips completed and in-progress transcript state across client reloads', () => {
    const values = new Map<string, string>(),
      storage = {
        getItem: (key: string) => values.get(key) ?? null,
        setItem: (key: string, value: string) => values.set(key, value),
      };
    const states = {
      chat: {
        messages: [
          { id: 'u1', role: 'user' as const, content: 'Keep this' },
          { id: 'a1', role: 'assistant' as const, content: 'Still here', status: 'completed' as const },
        ],
        activity: [
          { id: 'work', tool: 'Codex', kind: 'edit', summary: 'Changed a file', importance: 'normal' as const },
        ],
        nextSequence: 4,
      },
    };
    saveConversationStates(storage, states);
    expect(loadConversationStates(storage)).toEqual(states);
    expect(values.has(CONVERSATION_STORAGE_KEY)).toBe(true);
  });

  it('drops corrupt entries without sacrificing valid conversations', () => {
    const storage = {
      getItem: () =>
        JSON.stringify({
          good: { messages: [{ id: 'u', role: 'user', content: 'hello' }] },
          bad: { messages: [{ id: 1, role: 'robot' }] },
        }),
    };
    expect(loadConversationStates(storage)).toEqual({
      good: { messages: [{ id: 'u', role: 'user', content: 'hello' }] },
    });
    expect(loadConversationStates({ getItem: () => '{broken' })).toEqual({});
  });
});

describe('streamed conversation persistence (HS2-TC93GZ)', () => {
  it('coalesces bursts, writes the latest state, and bounds sustained streams', () => {
    vi.useFakeTimers();
    let latest = 'first';
    const writes: string[] = [];
    const persistence = createConversationPersistence(() => writes.push(latest));
    persistence.schedule();
    vi.advanceTimersByTime(200);
    latest = 'second';
    persistence.schedule();
    expect(writes).toEqual([]);
    vi.advanceTimersByTime(249);
    expect(writes).toEqual([]);
    vi.advanceTimersByTime(1);
    expect(writes).toEqual(['second']);
    for (let elapsed = 0; elapsed < 1_000; elapsed += 100) {
      latest = `stream-${elapsed}`;
      persistence.schedule();
      vi.advanceTimersByTime(100);
    }
    expect(writes).toEqual(['second', 'stream-900']);
    vi.advanceTimersByTime(1_000);
    expect(writes).toHaveLength(2);
  });

  it('flushes the latest state once on completion or page hide and cancels pending timers', () => {
    vi.useFakeTimers();
    let latest = 'partial';
    const writes: string[] = [];
    const persistence = createConversationPersistence(() => writes.push(latest));
    persistence.schedule();
    latest = 'complete';
    persistence.flush();
    persistence.flush();
    vi.advanceTimersByTime(2_000);
    expect(writes).toEqual(['complete']);
    latest = 'next turn';
    persistence.schedule();
    persistence.flush();
    expect(writes).toEqual(['complete', 'next turn']);
  });
});
