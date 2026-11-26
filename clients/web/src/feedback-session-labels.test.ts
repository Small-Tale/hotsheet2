import { describe, expect, it } from 'vitest';

import { feedbackSessionLabel } from './feedback-session-labels';

const context = {
  projectName: 'demo',
  terminals: [{ id: 'term-1', title: 'Claude 1' }, { id: 'term-2', title: '  ' }, { id: 'term-3' }],
  chats: [{ id: 'chat-a', connectionId: 'conn-a', name: 'Codex chat' }],
  claims: [{ worker: 'w-claimed', slug: 'HS2-ABC123', title: 'Fix the thing' }],
  toolLabel: (tool: string) => tool.charAt(0).toUpperCase() + tool.slice(1),
};

describe('feedback session choice labels (HS2-5MCAKY)', () => {
  it('names a terminal by its tab title, project, and the ticket its worker claims', () => {
    expect(feedbackSessionLabel({ id: 'term-1', tool: 'claude', worker: 'w-claimed' }, context)).toEqual({
      label: 'Claude 1 · demo · HS2-ABC123',
      description: 'Claude in demo, working on HS2-ABC123 Fix the thing (session term-1)',
    });
  });

  it('names an AI chat by its chat name through the connection id', () => {
    expect(feedbackSessionLabel({ id: 'conn-a', tool: 'codex' }, context).label).toBe('Codex chat · demo');
  });

  it('falls back to the tool name, never the raw id, when no tab or claim is known', () => {
    for (const id of ['term-2', 'term-3', 'unknown-session']) {
      const { label, description } = feedbackSessionLabel({ id, tool: 'claude', worker: 'w-free' }, context);
      expect(label).toBe('Claude · demo');
      expect(label).not.toContain(id);
      expect(description).toBe(`Claude in demo, no claimed ticket (session ${id})`);
    }
  });
});
