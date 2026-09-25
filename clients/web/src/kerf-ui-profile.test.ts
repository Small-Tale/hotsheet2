import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

interface KerfProfile {
  scope: string;
  exceptions: Array<{ id: string; rules: string[]; target: string; rationale: string }>;
}

describe('Kerf application UI profile', () => {
  it('limits exceptions to exact Web Awesome shadow-part boundaries', () => {
    const profile = JSON.parse(
      readFileSync(new URL('../.kerf-ui-profile.json', import.meta.url), 'utf8'),
    ) as KerfProfile;
    expect(profile.scope).toBe('directory');
    expect(profile.exceptions).toHaveLength(22);
    for (const exception of profile.exceptions) {
      expect(exception.id).toMatch(/^web-awesome-/);
      expect(exception.rules).toEqual(['KUI-L011']);
      expect(exception.target).toMatch(/^src\/(?:components|ux-demo)\/[a-z0-9-]+\.css$/);
      expect(exception.target).not.toMatch(/[?*]|\.\./);
      expect(exception.rationale).toContain('Web Awesome shadow parts');
    }
  });
});
