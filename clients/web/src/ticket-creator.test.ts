import { describe, expect, it } from 'vitest';

import { aiCreatorLabel } from './ticket-creator';

describe('aiCreatorLabel (HS2-9FPHWY)', () => {
  it('marks only an AI creator, naming a known tool', () => {
    expect(aiCreatorLabel({ role: 'ai' })).toBe('Created by AI');
    expect(aiCreatorLabel({ role: 'ai', id: 'claude-01ABC' })).toBe('Created by AI (Claude)');
    expect(aiCreatorLabel({ role: 'ai', id: 'codex' })).toBe('Created by AI (Codex)');
    expect(aiCreatorLabel({ role: 'ai', id: 'opaque-session-token' })).toBe('Created by AI');
  });

  it('keeps people, Hot Sheet, and unknown creators prominent', () => {
    expect(aiCreatorLabel(undefined)).toBeUndefined();
    expect(aiCreatorLabel({ role: 'human' })).toBeUndefined();
    expect(aiCreatorLabel({ role: 'human', id: 'claude' })).toBeUndefined();
    expect(aiCreatorLabel({ role: 'system', id: 'hotsheet' })).toBeUndefined();
    expect(aiCreatorLabel({ role: 'unknown' })).toBeUndefined();
  });
});
