import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { scanBrowserDependencies } from './browser-dependency-scan';

describe('stable-dev browser dependency scan (HS2-N9RD7X)', () => {
  it('follows static, re-exported, and lazy relative imports while skipping types, styles, and Node modules', async () => {
    const root = await mkdtemp(resolve(tmpdir(), 'hs-dep-scan-'));
    try {
      await mkdir(resolve(root, 'src/feature'), { recursive: true });
      await writeFile(
        resolve(root, 'src/main.tsx'),
        [
          "import { mount } from 'kerfjs';",
          "import type { Unused } from 'types-only-package';",
          "import '@scope/ui/button.css';",
          "import './feature';",
          "export { helper } from './helper';",
          "const lazy = () => import('./lazy');",
          "import raw from './data.json?raw';",
        ].join('\n'),
      );
      await writeFile(resolve(root, 'src/feature/index.ts'), "import { Button } from '@scope/ui/button';\n");
      await writeFile(resolve(root, 'src/helper.ts'), "export { helper } from 'helper-lib';\nimport 'node:fs';\n");
      await writeFile(resolve(root, 'src/lazy.ts'), "const x = await import('lazy-lib');\nimport './main';\n");
      await writeFile(resolve(root, 'src/unreached.ts'), "import 'server-only';\n");
      expect(scanBrowserDependencies(root, ['src/main.tsx', 'src/missing.tsx'], ['extra/runtime'])).toEqual([
        '@scope/ui/button',
        'extra/runtime',
        'helper-lib',
        'kerfjs',
        'lazy-lib',
      ]);
      // Empty entries yield only the always-included extras.
      expect(scanBrowserDependencies(root, [], [])).toEqual([]);
    } finally {
      await rm(root, { recursive: true, force: true });
    }
  });

  it('covers the real browser entries without server-only packages', () => {
    const dependencies = scanBrowserDependencies(resolve(import.meta.dirname, '..'));
    for (const expected of [
      'kerfjs',
      'kerfjs/jsx-runtime',
      'lucide',
      '@xterm/xterm',
      '@xterm/addon-fit',
      '@kerfjs/ui/lucide-icon',
    ])
      expect(dependencies).toContain(expected);
    for (const serverOnly of ['ws', 'hono', 'vitest', '@hono/vite-dev-server'])
      expect(dependencies).not.toContain(serverOnly);
    expect(dependencies.every((specifier) => !specifier.startsWith('node:') && !specifier.endsWith('.css'))).toBe(true);
  });
});
