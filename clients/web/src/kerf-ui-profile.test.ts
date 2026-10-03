import { existsSync, readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

interface KerfProfile {
  scope: string;
  exceptions: Array<{ id: string; rules: string[]; target: string; rationale: string }>;
}

interface KerfDoctorConfig {
  mode: string;
  stages: Record<string, boolean>;
  cache: boolean;
  failOn?: string;
  suppressions: { id: string; rules: string[]; target: string; rationale: string }[];
}

describe('Kerf application UI profile', () => {
  it('limits exceptions to exact reviewed integration boundaries', () => {
    const profile = JSON.parse(
      readFileSync(new URL('../.kerf-ui-profile.json', import.meta.url), 'utf8'),
    ) as KerfProfile;
    expect(profile.scope).toBe('workspace');
    expect(profile.exceptions).toHaveLength(77);
    for (const exception of profile.exceptions.slice(0, 22)) {
      expect(exception.id).toMatch(/^web-awesome-/);
      expect(exception.rules).toEqual(['KUI-L011']);
      expect(exception.target).toMatch(/^src\/(?:components|ux-demo)\/[a-z0-9-]+\.css$/);
      expect(exception.target).not.toMatch(/[?*]|\.\./);
      expect(exception.rationale).toContain('Web Awesome shadow parts');
    }
    expect(profile.exceptions.slice(22, 23)).toEqual([
      {
        id: 'mobile-side-panel-safe-area-composition',
        rules: ['KUI-L004'],
        target: 'src/components/app-shell.tsx',
        rationale:
          'The mobile shell deliberately composes safe-area padding on its app-owned sidebar and inspector content inside Kerf overlay regions.',
      },
    ]);
    // HS2-M6B8AD reviewed every remaining nested-inset and dynamic-class review finding site by
    // site; each exception is exact (one file, one rule) and its rationale names the classes or
    // expressions it covers, so a new finding elsewhere still fails the gate.
    const reviewed = profile.exceptions.slice(23);
    expect(reviewed.length).toBeGreaterThan(0);
    const ids = new Set<string>(),
      targets = new Set<string>();
    for (const exception of reviewed) {
      expect(exception.id).toMatch(/^(?:intentional-nested-inset|state-modifier-classes)-[a-z0-9-]+$/);
      expect(exception.rules).toEqual([exception.id.startsWith('intentional-') ? 'KUI-L004' : 'KUI-L008']);
      expect(exception.target).toMatch(/^src\/(?:components|ux-demo)\/[a-z0-9-]+\.tsx$/);
      expect(existsSync(new URL(`../${exception.target}`, import.meta.url)), exception.target).toBe(true);
      expect(exception.rationale).toMatch(/^Reviewed HS2-[0-9A-Z]{6}: /);
      if (exception.rules[0] === 'KUI-L004') expect(exception.rationale).toMatch(/design: [a-z][a-z0-9-]*__[a-z0-9-]+/);
      else expect(exception.rationale).toMatch(/Expressions: class(?:Name)?=\{/);
      expect(ids.has(exception.id), exception.id).toBe(false);
      ids.add(exception.id);
      targets.add(`${exception.rules[0]} ${exception.target}`);
    }
    expect(targets.size).toBe(reviewed.length);
  });

  it('runs every static doctor stage while keeping browser execution opt-in', () => {
    const config = JSON.parse(
      readFileSync(new URL('../.kerf-ui-doctor.json', import.meta.url), 'utf8'),
    ) as KerfDoctorConfig;
    expect(config).toEqual(
      expect.objectContaining({
        mode: 'full',
        cache: true,
        stages: {
          catalog: true,
          typescript: true,
          eslint: true,
          analyzer: true,
          browser: false,
        },
      }),
    );
    // The doctor itself is the CI gate: any active error, review finding, or warning fails it
    // (beta.69 `failOn`, KF-6S5EKX; HS2-Z44YPD retired the wrapper script).
    expect(config.failOn).toBe('warning');
    // Known Kerf gaps are documented suppressions, never budgets: the three KUI-L022 hook classes
    // HS2-8FS5BJ moved into their owning components (search layout, header yield, rail view title),
    // each naming the Kerf ticket whose adoption removes it (HS2-DAMHD1).
    expect(config.suppressions.map(({ rules, target }) => `${rules.join(',')} ${target}`)).toEqual([
      'KUI-L022 src/components/ticket-search-field.tsx',
      'KUI-L022 src/components/workspace-controls.tsx',
      'KUI-L022 src/components/terminal-ticket-rail.tsx',
    ]);
    for (const suppression of config.suppressions) expect(suppression.rationale).toMatch(/KF-[0-9A-Z]{6}/);
  });
});
