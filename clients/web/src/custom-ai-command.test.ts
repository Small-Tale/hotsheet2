import { describe, expect, it } from 'vitest';

import type { ToolConnection } from './api';
import { customAiCommandSignalConnection, customAiCommandTicket, HOTSHEET_SKILL_SIGNAL } from './custom-ai-command';

const connection = (value: Partial<ToolConnection> & Pick<ToolConnection, 'id'>): ToolConnection => ({
  tool: 'codex',
  project: '/work/demo.hs2',
  role: 'main',
  busy: false,
  actions: ['send_turn'],
  ...value,
  id: value.id,
});

describe('custom AI commands', () => {
  it('becomes an urgent Up Next task that preserves the configured prompt', () => {
    expect(customAiCommandTicket({id:'review',title:' Review changes ',kind:'ai',prompt:' Inspect the diff '})).toEqual({
      title: 'Review changes',
      details: 'Inspect the diff',
      category: 'task',
      priority: 'urgent',
      up_next: true,
    });
    expect(HOTSHEET_SKILL_SIGNAL).toBe('$hotsheet');
  });

  it('prefers an idle sendable main connection for the configured tool', () => {
    const connections = [
      connection({id:'busy-main',busy:true}),
      connection({id:'worker',role:'worker'}),
      connection({id:'claude-main',tool:'Claude'}),
      connection({id:'codex-main'}),
    ];
    expect(customAiCommandSignalConnection(connections,'CODEX','claude')?.id).toBe('codex-main');
    expect(customAiCommandSignalConnection(connections,undefined,'claude')?.id).toBe('claude-main');
  });

  it('does not signal a busy, read-only, or mismatched connection', () => {
    const connections = [
      connection({id:'busy',busy:true}),
      connection({id:'readonly',actions:['interrupt']}),
      connection({id:'claude',tool:'claude'}),
    ];
    expect(customAiCommandSignalConnection(connections,'codex','codex')).toBeUndefined();
  });
});
