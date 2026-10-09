import { existsSync, readdirSync, readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import config from '../playwright.config';

describe('Playwright output isolation (HS2-ZJJ4K6)', () => {
  it('rotates browser artifacts only under the generated directory excluded from UI doctor traversal', () => {
    expect(config.outputDir).toBe('target/playwright-test-results');
  });

  it('keeps visual capture paths portable and prepares their nested folders (HS2-SRGAH3)', () => {
    const tests = new URL('../tests/', import.meta.url);
    for (const name of readdirSync(tests).filter((name) => name.endsWith('.spec.ts')))
      expect(readFileSync(new URL(name, tests), 'utf8'), name).not.toContain('/private/tmp/');
    expect(existsSync(new URL('../target/visual-captures/claude/', import.meta.url))).toBe(true);
    expect(existsSync(new URL('../target/visual-captures/claude-501/', import.meta.url))).toBe(true);
  });
});
