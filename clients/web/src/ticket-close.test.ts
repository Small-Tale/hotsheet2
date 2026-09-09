import { describe, expect, it } from 'vitest';

import { ticketCloseReasonLabel, validateTicketClose } from './ticket-close';

describe('structured ticket close outcomes', () => {
  it('requires a distinct canonical ticket only for duplicates', () => {
    expect(validateTicketClose('completed', 'source')).toBe('');
    expect(validateTicketClose('duplicate', 'source')).toContain('Select');
    expect(validateTicketClose('duplicate', 'source', { id: 'source', slug: 'HS2-SELF', title: 'Same' })).toContain('itself');
    expect(validateTicketClose('duplicate', 'source', { id: 'target', slug: 'HS2-TARGET', title: 'Canonical' })).toBe('');
  });

  it('presents every persisted close reason in human language', () => {
    expect(['completed', 'not_planned', 'duplicate', 'obsolete'].map(value => ticketCloseReasonLabel(value as never))).toEqual(['Completed', 'Not planned', 'Duplicate', 'Obsolete']);
  });
});
