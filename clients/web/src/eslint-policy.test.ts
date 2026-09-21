import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

describe('web ESLint policy', () => {
  it('keeps main.tsx on the shared typed-rule baseline', () => {
    const config = readFileSync(new URL('../eslint.config.mjs', import.meta.url), 'utf8');
    expect(config).not.toContain("files: ['src/main.tsx']");
  });

  it('limits defensive-condition exceptions to explained next-line boundaries', () => {
    const source = readFileSync(new URL('./main.tsx', import.meta.url), 'utf8');
    const exceptions =
      source.match(
        /eslint-disable-next-line @typescript-eslint\/no-unnecessary-condition -- Defensive runtime boundary intentionally exceeds its total static type\./g,
      ) ?? [];
    expect(exceptions.length).toBeGreaterThan(0);
    expect(exceptions.length).toBeLessThanOrEqual(50);
    expect(source).not.toContain('eslint-disable @typescript-eslint/no-unnecessary-condition');
    expect(source).not.toContain('use-unknown-in-catch-callback-variable');
  });
});
