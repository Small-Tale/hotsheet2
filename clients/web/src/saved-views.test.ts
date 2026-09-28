import { describe, expect, it } from 'vitest';

import { consumeSearchTokens } from './inline-search';
import { customViewNameAvailable, customViewQueryText, customViewSearch, uniqueCustomViewId } from './saved-views';

describe('saved views', () => {
  it('creates readable collision-free ids', () => {
    expect(uniqueCustomViewId('Needs docs!', [])).toBe('needs-docs');
    expect(uniqueCustomViewId('Needs docs', [{ id: 'needs-docs', name: 'Earlier', query: 'tag:docs' }])).toBe(
      'needs-docs-2',
    );
    expect(uniqueCustomViewId('✨', [])).toBe('view');
  });

  it('treats display names as trimmed and case-insensitively unique', () => {
    const views = [{ id: 'review', name: 'Needs Review', query: 'status:completed' }];
    expect(customViewNameAvailable(' needs review ', views)).toBe(false);
    expect(customViewNameAvailable(' needs review ', views, 'review')).toBe(true);
    expect(customViewNameAvailable('Docs', views)).toBe(true);
    expect(customViewNameAvailable(' ', views)).toBe(false);
  });

  it('scopes a search-bar query to the shared view with AND instead of replacing it (HS2-50R1YQ)', () => {
    const view = { id: 'docs', name: 'Docs', query: 'tag:docs OR status:completed' };
    expect(customViewQueryText(view, '')).toBe('tag:docs OR status:completed');
    expect(customViewQueryText(view, '   ')).toBe('tag:docs OR status:completed');
    expect(customViewQueryText(view, 'parser')).toBe('(tag:docs OR status:completed) AND (parser)');
    const bar = consumeSearchTokens('parser tag:client ', true);
    expect(customViewQueryText(view, bar.text, bar.tokens)).toBe(
      '(tag:docs OR status:completed) AND (parser tag:client)',
    );
    // The combined expression is boolean, so its effective text keeps every token in place for evaluation.
    const scoped = customViewSearch(view, bar.text, bar.tokens);
    expect(scoped.text).toBe('(tag:docs OR status:completed) AND (parser tag:client)');
    expect(scoped.tokens.map((token) => token.raw)).toEqual(['tag:docs', 'tag:client']);
    // With an empty bar a plain tag view stays a token-only search the server can index.
    const plain = customViewSearch({ id: 'tagged', name: 'Tagged', query: 'tag:docs' }, '');
    expect(plain.text).toBe('');
    expect(plain.tokens.map((token) => token.raw)).toEqual(['tag:docs']);
  });
});
