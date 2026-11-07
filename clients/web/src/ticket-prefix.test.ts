import { describe, expect, it } from 'vitest';

import {
  normalizeTicketPrefix,
  TICKET_PREFIX_RULE,
  ticketPrefixExampleSlug,
  ticketPrefixSavedMessage,
} from './ticket-prefix';

describe('normalizeTicketPrefix (HS2-8QJ6W3)', () => {
  it('upcases and trims a linkable prefix', () => {
    expect(normalizeTicketPrefix(' hs2 ')).toEqual({ prefix: 'HS2' });
    expect(normalizeTicketPrefix('AB')).toEqual({ prefix: 'AB' });
    expect(normalizeTicketPrefix('A234567890123456')).toEqual({ prefix: 'A234567890123456' });
  });

  it('refuses prefixes the server refuses', () => {
    for (const input of ['', 'A', '2HS', 'HS-2', 'HS 2', 'HÖT', 'A2345678901234567']) {
      expect(normalizeTicketPrefix(input), input).toEqual({ error: TICKET_PREFIX_RULE });
    }
  });
});

describe('ticket prefix example copy (HS2-7Z5SCH)', () => {
  it('names a complete example slug, the same one the field hint shows', () => {
    expect(ticketPrefixExampleSlug('HOT')).toBe('HOT-7F3K9Q');
    expect(ticketPrefixSavedMessage('HOT')).toBe('Ticket prefix set to HOT. New tickets are named HOT-7F3K9Q.');
    expect(ticketPrefixSavedMessage('HOT')).not.toContain('…');
  });
});
