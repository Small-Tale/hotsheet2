import { describe, expect, it } from 'vitest';

import type { AiCommandRun, ToolConnection } from './api';
import {
  aiCommandRunMessage,
  customAiCommandSignalConnection,
  customAiCommandTicket,
  HOTSHEET_SKILL_SIGNAL,
} from './custom-ai-command';

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
    expect(
      customAiCommandTicket({ id: 'review', title: ' Review changes ', kind: 'ai', prompt: ' Inspect the diff ' }),
    ).toEqual({
      title: 'Review changes',
      details: 'Inspect the diff',
      category: 'task',
      priority: 'urgent',
      up_next: true,
      tags: ['hs2-command:review'],
    });
    expect(HOTSHEET_SKILL_SIGNAL).toBe('$hotsheet');
  });

  it('prefers an idle sendable main connection for the configured tool', () => {
    const connections = [
      connection({ id: 'busy-main', busy: true }),
      connection({ id: 'worker', role: 'worker' }),
      connection({ id: 'claude-main', tool: 'Claude' }),
      connection({ id: 'codex-main' }),
    ];
    expect(customAiCommandSignalConnection(connections, 'CODEX', 'claude')?.id).toBe('codex-main');
    expect(customAiCommandSignalConnection(connections, undefined, 'claude')?.id).toBe('claude-main');
  });

  it('does not signal a busy, read-only, or mismatched connection', () => {
    const connections = [
      connection({ id: 'busy', busy: true }),
      connection({ id: 'readonly', actions: ['interrupt'] }),
      connection({ id: 'claude', tool: 'claude' }),
    ];
    expect(customAiCommandSignalConnection(connections, 'codex', 'codex')).toBeUndefined();
  });
});

describe('HS2-17Y2EC AI command run message', () => {
  const result = (value: Partial<AiCommandRun>): AiCommandRun => ({
    action: 'create_ticket',
    delivery: 'queued',
    ticket: { slug: 'HS2-RUN001' } as AiCommandRun['ticket'],
    run: { id: 'run', command_id: 'review', state: 'completed', output: [] },
    ...value,
  });
  const label = (tool: string) => (tool === 'codex' ? 'Codex' : tool);

  it('names the outcome of each action', () => {
    expect(aiCommandRunMessage(result({}), label)).toBe('Queued HS2-RUN001.');
    expect(
      aiCommandRunMessage(
        result({
          action: 'new_ai_session',
          delivery: 'started',
          run: {
            id: 'run',
            command_id: 'review',
            state: 'completed',
            output: [],
            launch: { kind: 'ai', tool: 'codex' },
          },
        }),
        label,
      ),
    ).toBe('Started HS2-RUN001 in a Codex session.');
    expect(aiCommandRunMessage(result({ action: 'new_ai_session', delivery: 'started' }), label)).toBe(
      'Started HS2-RUN001 in an AI session.',
    );
    expect(
      aiCommandRunMessage(
        result({ action: 'reuse_or_new_ai_session', delivery: 'failed', reason: 'ai_handoff_failed' }),
        label,
      ),
    ).toBe('Queued HS2-RUN001; no AI session could start it (the AI session did not start).');
    expect(aiCommandRunMessage(result({ delivery: 'failed', reason: 'new_code' }), label)).toBe(
      'Queued HS2-RUN001; no AI session could start it (new_code).',
    );
    expect(aiCommandRunMessage(result({ delivery: 'failed' }), label)).toBe(
      'Queued HS2-RUN001; no AI session could start it.',
    );
  });
});
