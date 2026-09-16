import { describe, expect, it } from 'vitest';

import { isLucideCatalogLoaded, loadLucideCatalog, lucideIconNode, lucidePascalToKebab, searchLucideCatalog } from './lucide-catalog';

describe('lucidePascalToKebab', () => {
  it('converts Lucide export names to kebab-case ids', () => {
    expect(lucidePascalToKebab('GitBranch')).toBe('git-branch');
    expect(lucidePascalToKebab('Activity')).toBe('activity');
    expect(lucidePascalToKebab('CircleCheckBig')).toBe('circle-check-big');
    expect(lucidePascalToKebab('AArrowUp')).toBe('a-arrow-up');
  });
});

describe('loadLucideCatalog', () => {
  it('loads the full catalog once and resolves + searches icons by name', async () => {
    expect(isLucideCatalogLoaded()).toBe(false);
    await loadLucideCatalog();
    expect(isLucideCatalogLoaded()).toBe(true);
    // A known icon resolves to a real Lucide node (array of [tag, attrs, ...] tuples).
    const node = lucideIconNode('git-branch');
    expect(Array.isArray(node)).toBe(true);
    expect(node?.[0]?.[0]).toBe('path');
    // Search ranks name-prefix matches and is bounded.
    const results = searchLucideCatalog('git', 10);
    expect(results.length).toBeGreaterThan(0);
    expect(results.length).toBeLessThanOrEqual(10);
    expect(results.every(entry => entry.name.includes('git'))).toBe(true);
    expect(results.some(entry => entry.name === 'git-branch')).toBe(true);
    // An unknown query returns nothing.
    expect(searchLucideCatalog('definitely-not-an-icon-zzz')).toEqual([]);
  });
});
