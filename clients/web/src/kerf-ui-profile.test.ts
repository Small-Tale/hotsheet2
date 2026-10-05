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
    expect(profile.exceptions).toHaveLength(73);
    for (const exception of profile.exceptions.slice(0, 21)) {
      expect(exception.id).toMatch(/^web-awesome-/);
      expect(exception.rules).toEqual(['KUI-L011']);
      expect(exception.target).toMatch(/^src\/(?:components|ux-demo)\/[a-z0-9-]+\.css$/);
      expect(exception.target).not.toMatch(/[?*]|\.\./);
      if (exception.id === 'web-awesome-project-close-parts') {
        expect(exception.rationale).toContain('raw Web Awesome modal');
        expect(exception.rationale).toContain('KF-E2J9ND; adoption HS2-FXAAA6');
      } else expect(exception.rationale).toContain('Web Awesome shadow parts');
    }
    expect(profile.exceptions.slice(21, 22)).toEqual([
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
    const reviewed = profile.exceptions.slice(22);
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
    // Strict component ownership replaces the app CSS ownership check (HS2-R9GQJE): Kerf judges
    // uncataloged CSS-importing modules, the three shell stylesheets as ownership groups, and other
    // packages' classes used as selector context.
    expect(config).toEqual(
      expect.objectContaining({
        ownership: 'component',
        ownershipContext: 'any-package',
        implicitComponentOwnership: true,
        ownershipGroups: [
          { styleSources: ['src/style.css'], sources: ['src/main.tsx', 'src/app/'] },
          { styleSources: ['src/ux-demo/style.css'], sources: ['src/ux-demo/'] },
          { styleSources: ['src/dev-review/dev-review.css'], sources: ['src/dev-review/'] },
        ],
      }),
    );
    // Known Kerf gaps are documented suppressions, never open-ended budgets: each names the KF ticket
    // whose prop removes it. beta.76's stricter KUI-L022 found eight files whose styling no Kerf API
    // expresses yet (HS2-170JC3); the set only shrinks as those tickets ship.
    expect(config.suppressions.map(({ id, rules, target }) => ({ id, rules, target }))).toEqual([
      { id: 'kf-vh4b52-ai-conversation-heading', rules: ['KUI-L022'], target: 'src/components/ai-conversation.tsx' },
      {
        id: 'kf-vh4b52-command-settings-heading',
        rules: ['KUI-L022'],
        target: 'src/components/command-settings-editor.tsx',
      },
      {
        id: 'kf-vh4b52-connection-details-heading',
        rules: ['KUI-L022'],
        target: 'src/components/connection-details-dialog.tsx',
      },
      { id: 'kf-vh4b52-dialog-layout-demo-heading', rules: ['KUI-L022'], target: 'src/ux-demo/dialog-layout-demo.tsx' },
      {
        id: 'kf-vh4b52-repository-status-heading',
        rules: ['KUI-L022'],
        target: 'src/components/repository-status-popover.tsx',
      },
      {
        id: 'kf-mxe9yv-workspace-header-width-visibility',
        rules: ['KUI-L022'],
        target: 'src/components/workspace-controls.tsx',
      },
      {
        id: 'kf-m8sv15-search-group-anchored-surfaces',
        rules: ['KUI-L022'],
        target: 'src/components/ticket-search-field.tsx',
      },
      {
        id: 'kf-7288md-gallery-filename-dark-tone',
        rules: ['KUI-L022'],
        target: 'src/components/attachment-gallery.tsx',
      },
    ]);
    for (const suppression of config.suppressions)
      expect(suppression.rationale).toContain(suppression.id.slice(0, 9).replace('kf-', 'KF-').toUpperCase());
    // The doctor replaced the app CSS ownership check, so `npm run lint` ends with it (HS2-R9GQJE).
    const scripts = (
      JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8')) as {
        scripts: Record<string, string>;
      }
    ).scripts;
    expect(scripts.lint.split('&&').map((step) => step.trim())).toEqual([
      'npm run format:check',
      'eslint . --max-warnings 0',
      'npm run catalog:check',
      'npm run ui:doctor',
    ]);
    expect(scripts['ui:doctor']).toBe('kerf-ui-doctor --full');
    expect(scripts).not.toHaveProperty('css:ownership');
  });
});
