import { describe, expect, it } from 'vitest';

import type { ConversationState } from './ai-conversation';
import type { ToolConnection } from './api';
import type { TerminalDashboardGroup } from './components/terminal-dashboard';
import {
  HALTED_SESSION_SEEN_KEY,
  type HaltedSessionEpisode,
  HaltedSessionInbox,
  HISTORICAL_SEEN_LIMIT,
  loadHaltedSessionSeen,
  parseHaltedSessionSeen,
  persistHaltedSessionSeen,
  projectHaltedSessions,
  synchronizeHaltedSessionSeen,
} from './halted-sessions';
import type { Project } from './interactions/types';

const episode = (key: string): HaltedSessionEpisode => ({
  key,
  kind: 'terminal',
  projectId: 'p',
  projectName: 'Project',
  sessionId: 't',
  sessionName: 'Claude',
  message: 'At capacity',
});

describe('halted session inbox (HS2-E6KAWY)', () => {
  it('retains recently acknowledged history when an active episode resolves', () => {
    const inbox = new HaltedSessionInbox(Array.from({ length: HISTORICAL_SEEN_LIMIT }, (_, index) => `old-${index}`));
    inbox.reconcile([episode('recent')]);
    inbox.visible(false, false);
    inbox.presented('recent');
    inbox.reconcile([]);
    expect(inbox.seenKeys()).toContain('recent');
    expect(inbox.seenKeys()).not.toContain('old-0');
  });
  it('converges overlapping window writes once and respects explicit removal without an echo loop', () => {
    const inbox = new HaltedSessionInbox(['local']),
      writes: string[] = [],
      storage = {
        getItem: () => '["peer"]',
        setItem: (_key: string, value: string) => {
          writes.push(value);
        },
      };
    synchronizeHaltedSessionSeen(inbox, '["peer"]', storage);
    expect(JSON.parse(writes[0])).toEqual(['local', 'peer']);
    synchronizeHaltedSessionSeen(inbox, writes[0], storage);
    synchronizeHaltedSessionSeen(inbox, null, storage);
    expect(writes).toHaveLength(1);
  });
  it('queues through pause, permission priority and deferred presentation without marking seen', () => {
    const inbox = new HaltedSessionInbox();
    inbox.reconcile([episode('a'), episode('b')]);
    expect(inbox.visible(true, false)).toBeUndefined();
    expect(inbox.visible(false, true)).toBeUndefined();
    expect(inbox.waiting()).toBe(2);
    expect(inbox.visible(false, false)?.key).toBe('a');
    expect(inbox.seenKeys()).toEqual([]);
    expect(inbox.presented('b')).toBe(false);
    expect(inbox.presented('a')).toBe(true);
    expect(inbox.presented('a')).toBe(false);
    expect(inbox.visible(false, false)?.key).toBe('a');
    expect(inbox.visible(false, true)).toBeUndefined();
    expect(inbox.visible(false, false)?.key).toBe('a');
    inbox.dismiss('a');
    expect(inbox.visible(false, false)?.key).toBe('b');
  });

  it('drops resolved queued/current episodes and never repeats an acknowledged active episode after reload', () => {
    const inbox = new HaltedSessionInbox();
    inbox.reconcile([episode('a'), episode('b')]);
    inbox.visible(false, false);
    inbox.presented('a');
    expect(inbox.visible(true, false)).toBeUndefined();
    inbox.reconcile([episode('a')]);
    expect(inbox.visible(false, false)).toBeUndefined();
    const restored = new HaltedSessionInbox(inbox.seenKeys());
    restored.reconcile([episode('a'), episode('new-at')]);
    expect(restored.visible(false, false)?.key).toBe('new-at');
    restored.reconcile([]);
    expect(restored.visible(false, false)).toBeUndefined();
    expect(restored.waiting()).toBe(0);
    restored.reconcile([episode('a')]);
    expect(restored.visible(false, false)).toBeUndefined();
  });

  it('merges another window acknowledgment without removing a locally presented prompt', () => {
    const inbox = new HaltedSessionInbox();
    inbox.reconcile([episode('a'), episode('b')]);
    inbox.visible(false, false);
    inbox.mergeSeen(['a']);
    expect(inbox.visible(false, false)?.key).toBe('b');
    inbox.presented('b');
    inbox.mergeSeen(['b']);
    expect(inbox.visible(false, false)?.key).toBe('b');
  });

  it('bounds inactive history while retaining every active acknowledgment and deduplicating snapshots', () => {
    const inbox = new HaltedSessionInbox(['active']);
    inbox.reconcile([episode('active'), episode('active')]);
    inbox.mergeSeen(Array.from({ length: 400 }, (_, i) => `old-${i}`));
    expect(inbox.seenKeys()).toHaveLength(HISTORICAL_SEEN_LIMIT + 1);
    expect(inbox.seenKeys()).toContain('active');
    expect(inbox.seenKeys()).not.toContain('old-0');
    expect(inbox.visible(false, false)).toBeUndefined();
    inbox.reconcile([]);
    expect(inbox.seenKeys()).toHaveLength(HISTORICAL_SEEN_LIMIT);
  });

  it('tolerates malformed/unavailable storage and merges before writing', () => {
    expect(parseHaltedSessionSeen('{')).toEqual([]);
    expect(parseHaltedSessionSeen('{"key":"a"}')).toEqual([]);
    expect(parseHaltedSessionSeen('["a",2,"a"]')).toEqual(['a']);
    const blocked = {
      getItem: () => {
        throw new Error('blocked');
      },
      setItem: () => {
        throw new Error('blocked');
      },
    };
    expect(loadHaltedSessionSeen(blocked)).toEqual([]);
    const inbox = new HaltedSessionInbox(['a']);
    persistHaltedSessionSeen(inbox, blocked);
    expect(inbox.seenKeys()).toEqual(['a']);
    let saved = '';
    persistHaltedSessionSeen(inbox, {
      getItem: () => '["b"]',
      setItem: (key, value) => {
        expect(key).toBe(HALTED_SESSION_SEEN_KEY);
        saved = value;
      },
    });
    expect(JSON.parse(saved)).toEqual(['a', 'b']);
  });
});

describe('halt episode projection', () => {
  it('retains active dedupe while reloaded project snapshots are unresolved, then bounds resolved history', () => {
    const keys = Array.from({ length: 300 }, (_, index) =>
        JSON.stringify(['terminal', 'loading-project', `t-${index}`, 'at']),
      ),
      inbox = new HaltedSessionInbox(keys);
    inbox.reconcile([], ['loading-project']);
    expect(inbox.seenKeys()).toHaveLength(300);
    inbox.reconcile([], [], true);
    expect(inbox.seenKeys()).toHaveLength(300);
    inbox.reconcile(keys.map(episode));
    expect(inbox.visible(false, false)).toBeUndefined();
    expect(inbox.seenKeys()).toHaveLength(300);
    inbox.reconcile([]);
    expect(inbox.seenKeys()).toHaveLength(HISTORICAL_SEEN_LIMIT);
  });
  const projects = [{ id: 'p', name: 'Project' }] as Project[],
    chats = { p: [{ id: 'chat', connectionId: 'c', name: 'Codex', tool: 'codex' }] },
    failed: ConversationState = {
      messages: [{ id: 'assistant-1', role: 'assistant', status: 'failed', content: '' }],
      error: 'Capacity',
    };

  it('identifies terminal episodes by project/session/at and ignores closed projects', () => {
    const groups = [
      { projectId: 'p', sessions: [{ id: 't', title: 'Claude', halt: { at: 'one', message: 'Stopped' } }] },
    ] as TerminalDashboardGroup[];
    expect(projectHaltedSessions(projects, groups, {}, {}, {})[0].key).toBe(
      JSON.stringify(['terminal', 'p', 't', 'one']),
    );
    expect(projectHaltedSessions([], groups, {}, {}, {})).toEqual([]);
  });

  it('requires the latest assistant to have completed failure, suppressing retryable errors and in-flight retries', () => {
    const project = (state: ConversationState, busy = false) =>
      projectHaltedSessions(
        projects,
        [],
        chats,
        { p: [{ id: 'c', busy, last_error: 'Old failure' } as ToolConnection] },
        { c: state },
      );
    expect(project(failed)[0].key).toBe(JSON.stringify(['chat', 'p', 'chat', 'assistant-1']));
    expect(project({ ...failed, activeAssistantId: 'retry' })).toEqual([]);
    expect(project(failed, true)).toEqual([]);
    expect(
      project({
        ...failed,
        messages: [
          ...failed.messages,
          { id: 'assistant-2', role: 'assistant', status: 'completed', content: 'Success' },
        ],
      }),
    ).toEqual([]);
    expect(
      project({
        messages: [{ id: 'retrying', role: 'assistant', status: 'streaming', content: '' }],
        error: 'Retryable provider error',
      }),
    ).toEqual([]);
    expect(projectHaltedSessions(projects, [], {}, {}, { c: failed })).toEqual([]);
  });
});
