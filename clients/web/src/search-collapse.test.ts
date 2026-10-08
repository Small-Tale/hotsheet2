import { describe, expect, it } from 'vitest';

import { isWhitespaceOnlySearch } from './search-collapse';

describe('isWhitespaceOnlySearch', () => {
  it('treats spaces, tabs, and line breaks as empty content', () => {
    for (const query of [' ', '  ', '\t', '\n', ' \t\n '])
      expect(isWhitespaceOnlySearch({ query, tokens: [] })).toBe(true);
  });

  it('leaves actual query text and filter tokens expanded', () => {
    expect(isWhitespaceOnlySearch({ query: '', tokens: [] })).toBe(false);
    expect(isWhitespaceOnlySearch({ query: ' ticket ', tokens: [] })).toBe(false);
    expect(isWhitespaceOnlySearch({ query: ' ', tokens: [{ value: 'is:active', label: 'Active', offset: 0 }] })).toBe(
      false,
    );
  });
});
