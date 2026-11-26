import { describe, expect, it } from 'vitest';

import { assertInitialAssetBudget, assertUxDemoIsLazy, initialAssetPaths } from './production-bundle-policy.mjs';

describe('production bundle startup policy', () => {
  it('counts unique initial bundle assets but not unrelated links', () => {
    const html =
      '<link rel="stylesheet" href="/assets/app.css"><script src="/assets/app.js"></script><link rel="icon" href="/favicon.svg"><script src="/assets/app.js"></script>';
    expect(initialAssetPaths(html)).toEqual(['/assets/app.css', '/assets/app.js']);
    expect(assertInitialAssetBudget(html, 2)).toEqual(['/assets/app.css', '/assets/app.js']);
  });

  it('rejects an entry point that fragments startup beyond its request budget', () => {
    const html = Array.from({ length: 5 }, (_, index) => `<script src="/assets/chunk-${index}.js"></script>`).join('');
    expect(() => assertInitialAssetBudget(html, 4)).toThrow('loads 5 assets; budget is 4');
  });

  it('accepts the UX demo catalog only as a chunk outside the startup scripts (HS2-ZF70Y9)', () => {
    const chunks = {
      'index-a.js': ['../../../src/main.tsx', '../../../src/ux-demo-entry.ts', '../../../src/host-features.ts'],
      'main-b.js': ['../../../src/ux-demo/main.tsx', '../../../src/ux-demo/registry.ts'],
      'lucide-c.js': ['../../node_modules/lucide/dist/esm/icons/x.js'],
    };
    expect(assertUxDemoIsLazy(['index-a.js'], chunks)).toEqual(['main-b.js']);
    expect(() =>
      assertUxDemoIsLazy(['index-a.js'], {
        ...chunks,
        'index-a.js': [...chunks['index-a.js'], '../../../src/ux-demo/fixtures.ts'],
      }),
    ).toThrow('The startup script index-a.js contains UX demo catalog modules: ../../../src/ux-demo/fixtures.ts');
    expect(() => assertUxDemoIsLazy(['index-a.js'], { 'index-a.js': chunks['index-a.js'] })).toThrow(
      'No production chunk contains the UX demo catalog entry',
    );
  });
});
