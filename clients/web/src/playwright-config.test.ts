import { describe, expect, it } from 'vitest';

import config from '../playwright.config';

describe('Playwright output isolation (HS2-ZJJ4K6)', () => {
  it('rotates browser artifacts only under the generated directory excluded from UI doctor traversal', () => {
    expect(config.outputDir).toBe('target/playwright-test-results');
  });
});
