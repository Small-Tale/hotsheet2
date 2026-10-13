import { describe, expect, it } from 'vitest';

import { copiedTicketPlacement, ticketClipboardText } from './ticket-transfer';

describe('ticketClipboardText', () => {
  it('joins each ticket id, title, details, and notes, skipping empty details', () => {
    expect(
      ticketClipboardText([
        { slug: 'HS2-A', title: 'First', details: 'Body', notes: [{ text: 'n1' }] },
        { slug: 'HS2-B', title: 'Second', details: '', notes: [] },
      ]),
    ).toBe('HS2-A: First\n\nBody\n\n- n1\n\nHS2-B: Second');
  });
});

describe('copiedTicketPlacement', () => {
  it.each([undefined, 'not_started', 'backlog', 'started', 'completed', 'verified', 'archive', 'deleted', 'moved'])(
    'starts a copied %s ticket in Not Started',
    (status) => {
      expect(copiedTicketPlacement({ status, up_next: false })).toEqual({
        status: 'not_started',
        up_next: false,
      });
    },
  );

  it('preserves whether the source was in Up Next', () => {
    expect(copiedTicketPlacement({ status: 'completed', up_next: true })).toEqual({
      status: 'not_started',
      up_next: true,
    });
  });
});
