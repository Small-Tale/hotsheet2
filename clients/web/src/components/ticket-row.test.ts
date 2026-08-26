import { describe, expect, it } from 'vitest';
import { normalizeTicketRowProps } from './ticket-row';

describe('TicketRow', () => {
  it('normalizes fallbacks, tags, and boolean defaults', () => {
    expect(normalizeTicketRowProps({
      slug: ' ', title: ' ', status: 'not_started', priority: 'default', category: ' ', tags: [' client ', '', ' ux '],
    })).toEqual({
      slug: 'HS2-UNKNOWN', title: 'Untitled ticket', status: 'not_started', priority: 'default',
      category: 'issue', tags: ['client', 'ux'], upNext: false, selected: false, busy: false,
    });
  });
});
