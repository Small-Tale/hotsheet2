import { describe, expect, it } from 'vitest';

import { assertInitialAssetBudget, initialAssetPaths } from './production-bundle-policy.mjs';

describe('production bundle startup policy', () => {
  it('counts unique initial bundle assets but not unrelated links', () => {
    const html = '<link rel="stylesheet" href="/assets/app.css"><script src="/assets/app.js"></script><link rel="icon" href="/favicon.svg"><script src="/assets/app.js"></script>';
    expect(initialAssetPaths(html)).toEqual(['/assets/app.css', '/assets/app.js']);
    expect(assertInitialAssetBudget(html, 2)).toEqual(['/assets/app.css', '/assets/app.js']);
  });

  it('rejects an entry point that fragments startup beyond its request budget', () => {
    const html = Array.from({ length: 5 }, (_, index) => `<script src="/assets/chunk-${index}.js"></script>`).join('');
    expect(() => assertInitialAssetBudget(html, 4)).toThrow('loads 5 assets; budget is 4');
  });
});
