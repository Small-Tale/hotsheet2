import { describe, expect, it } from 'vitest';

import { customViewNameAvailable, uniqueCustomViewId } from './saved-views';

describe('saved views', () => {
  it('creates readable collision-free ids', () => {
    expect(uniqueCustomViewId('Needs docs!', [])).toBe('needs-docs');
    expect(uniqueCustomViewId('Needs docs', [{ id: 'needs-docs', name: 'Earlier', query: 'tag:docs' }])).toBe('needs-docs-2');
    expect(uniqueCustomViewId('✨', [])).toBe('view');
  });

  it('treats display names as trimmed and case-insensitively unique', () => {
    const views = [{ id: 'review', name: 'Needs Review', query: 'status:completed' }];
    expect(customViewNameAvailable(' needs review ', views)).toBe(false);
    expect(customViewNameAvailable(' needs review ', views, 'review')).toBe(true);
    expect(customViewNameAvailable('Docs', views)).toBe(true);
    expect(customViewNameAvailable(' ', views)).toBe(false);
  });
});
