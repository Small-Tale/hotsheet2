import type { TokenSearchFieldValue } from '@kerfjs/ui/token-search-field';

/** A query with only whitespace has no search meaning and can close on blur. */
export function isWhitespaceOnlySearch(value: TokenSearchFieldValue): boolean {
  return value.tokens.length === 0 && value.query.length > 0 && value.query.trim().length === 0;
}
