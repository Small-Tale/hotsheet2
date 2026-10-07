import { describe, expect, it } from 'vitest';

import { aiToolName, noteAuthorship } from './note-authorship';

describe('noteAuthorship (HS2-32QDZ3)', () => {
  it('labels AI notes with their tool and keeps unrecorded authorship as Hot Sheet', () => {
    expect(noteAuthorship({ text: 'x', actor: { role: 'ai', id: 'claude-01M3TMXDT5-cli' } })).toEqual({
      author: 'Claude',
      aiAuthored: true,
      aiTool: 'Claude',
    });
    expect(noteAuthorship({ text: 'x', actor: { role: 'ai', id: 'codex-terminal-7' } }).aiTool).toBe('Codex');
    expect(noteAuthorship({ text: 'x', actor: { role: 'ai' } })).toEqual({
      author: 'AI',
      aiAuthored: true,
      aiTool: 'AI',
    });
    expect(noteAuthorship({ text: 'x', actor: { role: 'human', id: 'dana@example.com' } })).toEqual({
      author: 'dana@example.com',
      aiAuthored: false,
    });
    expect(noteAuthorship({ text: 'x', actor: { role: 'human' } }).author).toBe('Human');
    for (const actor of [undefined, { role: 'system' as const }]) {
      expect(noteAuthorship({ text: 'x', actor })).toEqual({ author: 'Hot Sheet', aiAuthored: false });
    }
    expect(noteAuthorship({ text: '<!-- hotsheet:activity-distillation:v1:abc -->' })).toEqual({
      author: 'Hot Sheet AI',
      aiAuthored: true,
      aiTool: 'Hot Sheet AI',
    });
    expect(noteAuthorship({ text: 'Edited distillation', actor: { role: 'ai', id: 'hotsheet' } }).aiTool).toBe(
      'Hot Sheet AI',
    );
  });

  it('never echoes an opaque session id as the tool name', () => {
    expect(aiToolName('01M3TMXDT5TCYDYY9HPJD92YRK')).toBe('AI');
    expect(aiToolName('  OpenCode-session ')).toBe('OpenCode');
    expect(aiToolName(undefined)).toBe('AI');
  });
});
