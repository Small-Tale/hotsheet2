import { describe, expect, it } from 'vitest';

import { copiedTicketPlacement } from './ticket-transfer';

describe('copiedTicketPlacement', () => {
  it.each([
    undefined,
    'not_started',
    'backlog',
    'started',
    'completed',
    'verified',
    'archive',
    'deleted',
    'moved',
  ])('starts a copied %s ticket in Not Started', status => {
    expect(copiedTicketPlacement({ status, up_next: false })).toEqual({
      status: 'not_started',
      up_next: false,
    });
  });

  it('preserves whether the source was in Up Next', () => {
    expect(copiedTicketPlacement({ status: 'completed', up_next: true })).toEqual({
      status: 'not_started',
      up_next: true,
    });
  });
});
