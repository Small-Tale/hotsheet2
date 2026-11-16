import { describe, expect, it } from 'vitest';

import { conversationComposers } from './conversation-composers';

function mount(html: string) {
  const root = document.createElement('div');
  root.innerHTML = html;
  return root;
}

describe('conversationComposers (HS2-YNCQ36)', () => {
  it('returns only the sending conversation’s composer when the drawer chat precedes the dialog', () => {
    const root = mount(`
      <section data-selection-id="drawer-chat"><textarea name="conversation-draft" id="drawer"></textarea></section>
      <div data-selection-id="dialog-chat"><form><textarea name="conversation-draft" id="dialog"></textarea></form></div>
    `);
    expect(conversationComposers(root, 'dialog-chat').map((field) => field.id)).toEqual(['dialog']);
    expect(conversationComposers(root, 'drawer-chat').map((field) => field.id)).toEqual(['drawer']);
  });

  it('returns every mounted composer of one conversation and none for an unknown or unmarked one', () => {
    const root = mount(`
      <section data-selection-id="shared"><textarea name="conversation-draft" id="a"></textarea></section>
      <div data-selection-id="shared"><textarea name="conversation-draft" id="b"></textarea></div>
      <textarea name="conversation-draft" id="unmarked"></textarea>
      <section data-selection-id="other"><textarea name="ticket-draft"></textarea></section>
    `);
    expect(conversationComposers(root, 'shared').map((field) => field.id)).toEqual(['a', 'b']);
    expect(conversationComposers(root, 'other')).toEqual([]);
    expect(conversationComposers(root, 'missing')).toEqual([]);
  });
});
