import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

interface KerfProfile {
  scope: string;
  exceptions: Array<{ id: string; rules: string[]; target: string; rationale: string }>;
}

interface KerfDoctorConfig {
  mode: string;
  stages: Record<string, boolean>;
  cache: boolean;
  suppressions: unknown[];
}

describe('Kerf application UI profile', () => {
  it('limits exceptions to exact reviewed integration boundaries', () => {
    const profile = JSON.parse(
      readFileSync(new URL('../.kerf-ui-profile.json', import.meta.url), 'utf8'),
    ) as KerfProfile;
    expect(profile.scope).toBe('workspace');
    expect(profile.exceptions).toHaveLength(24);
    for (const exception of profile.exceptions.slice(0, 22)) {
      expect(exception.id).toMatch(/^web-awesome-/);
      expect(exception.rules).toEqual(['KUI-L011']);
      expect(exception.target).toMatch(/^src\/(?:components|ux-demo)\/[a-z0-9-]+\.css$/);
      expect(exception.target).not.toMatch(/[?*]|\.\./);
      expect(exception.rationale).toContain('Web Awesome shadow parts');
    }
    expect(profile.exceptions.slice(22)).toEqual([
      {
        id: 'mobile-side-panel-safe-area-composition',
        rules: ['KUI-L004'],
        target: 'src/components/app-shell.tsx',
        rationale:
          'The mobile shell deliberately composes safe-area padding on its app-owned sidebar and inspector content inside Kerf overlay regions.',
      },
      {
        id: 'mobile-side-panel-viewport-height',
        rules: ['KUI-L005'],
        target: 'src/components/mobile-side-panels.css',
        rationale:
          "Hot Sheet mobile side panels are full-viewport navigation surfaces, so they intentionally override Kerf's generic 85vh overlay cap.",
      },
    ]);
  });

  it('runs every static doctor stage while keeping browser execution opt-in', () => {
    const config = JSON.parse(
      readFileSync(new URL('../.kerf-ui-doctor.json', import.meta.url), 'utf8'),
    ) as KerfDoctorConfig;
    expect(config).toEqual(
      expect.objectContaining({
        mode: 'full',
        cache: true,
        suppressions: [],
        stages: {
          catalog: true,
          typescript: true,
          eslint: true,
          analyzer: true,
          browser: false,
        },
      }),
    );
  });
});
