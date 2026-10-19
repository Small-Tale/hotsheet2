import { afterEach, describe, expect, it } from 'vitest';

import { conversationContextMenu, conversationContextMessageIds } from './conversation-context-menu';

afterEach(() => {
  conversationContextMenu.value = undefined;
});

describe('conversation message context scope (HS2-YR405B)', () => {
  it('uses the selection only when the requested message belongs to it', () => {
    const selection = ['first', 'third'];
    expect(conversationContextMessageIds('first', selection)).toEqual(selection);
    expect(conversationContextMessageIds('third', selection)).toEqual(selection);
    expect(conversationContextMessageIds('second', selection)).toEqual(['second']);
    expect(conversationContextMessageIds('second', [])).toEqual(['second']);
  });
});
