/** The message shown when a ticket prefix cannot be used (HS2-8QJ6W3). */
export const TICKET_PREFIX_RULE = 'Use 2 to 16 letters or digits, starting with a letter.';

/**
 * Validate and upcase a Git source's ticket prefix the way the server does (HS2-8QJ6W3): 2 to 16
 * ASCII letters or digits starting with a letter, so new slugs stay linkable. Surrounding
 * whitespace is ignored. The server re-validates; this only avoids a round trip for a typo.
 */
export function normalizeTicketPrefix(input: string): { prefix: string } | { error: string } {
  const prefix = input.trim().toUpperCase();
  return /^[A-Z][A-Z0-9]{1,15}$/.test(prefix) ? { prefix } : { error: TICKET_PREFIX_RULE };
}

/**
 * An example slug for a prefix (HS2-7Z5SCH), so the field hint and the saved toast show the same
 * complete name instead of an elided `HOT-…`.
 */
export function ticketPrefixExampleSlug(prefix: string): string {
  return `${prefix}-7F3K9Q`;
}

/** The toast shown after a Git source's ticket prefix is saved (HS2-7Z5SCH). */
export function ticketPrefixSavedMessage(prefix: string): string {
  return `Ticket prefix set to ${prefix}. New tickets are named ${ticketPrefixExampleSlug(prefix)}.`;
}
