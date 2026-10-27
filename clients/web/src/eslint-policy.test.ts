import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

import { ESLint } from 'eslint';
import { describe, expect, it } from 'vitest';

import packageJson from '../package.json';

describe('web ESLint policy', () => {
  it('HS2-N9X6D5 keeps browser fixture timers for measured windows and cadence only', () => {
    const read = (name: string) => readFileSync(new URL(`../scripts/${name}`, import.meta.url), 'utf8');
    const stable = read('stable-dev.e2e.test.mjs');
    const scale = read('scale-stress.mjs');
    const profile = read('change-stream-worker-profile.mjs');
    expect(stable.match(/\.waitForTimeout\(/g)).toHaveLength(2);
    expect(stable).toContain('waitForTimeout(reconnectObservationMs)');
    expect(stable).toContain('waitForTimeout(lateReloadObservationMs)');
    expect(stable).toContain("response.url().includes('/migration-jobs')");
    // A 202 or a generic app control is not migration readiness: both pages wait for the rendered job banner.
    expect(stable.match(/await renderedImportJob\((?:wide|narrow)\)/g)).toHaveLength(2);
    expect(stable).toContain('[data-component="hs1-job-banner"]');
    expect(scale.match(/\.waitForTimeout\(/g)).toHaveLength(1);
    expect(scale).toContain('waitForTimeout(redundantRequestObservationMs)');
    expect(profile.match(/\.waitForTimeout\(/g)).toHaveLength(1);
    expect(profile).toContain('waitForTimeout(clickCadenceMs)');
    expect(profile).toContain('lastVisibleChurnSlug');
  });

  const webRoot = join(import.meta.dirname, '..');
  const eslint = new ESLint({ cwd: webRoot });
  const scripts = readdirSync(join(webRoot, 'scripts'))
    .filter((name) => /\.(?:mjs|mts)$/.test(name))
    .map((name) => `scripts/${name}`);
  // HS2-2SPPTQ: preserve every real lint input without aggregating their work into one timed case.
  it.each([...scripts, 'eslint.config.mjs'])('HS2-2SPPTQ preserves HS2-HK6KMT lint coverage for %s', async (name) => {
    const filePath = join(webRoot, name);
    expect(await eslint.isPathIgnored(filePath)).toBe(false);
    const realResults = await eslint.lintFiles([filePath]);
    expect(realResults.map((result) => result.filePath)).toEqual([filePath]);
    expect(realResults.every((result) => result.errorCount === 0 && result.warningCount === 0)).toBe(true);
  });

  it('HS2-2SPPTQ preserves HS2-HK6KMT rejection through the actual Node lint route', async () => {
    const [invalid] = await eslint.lintText('const value = missingReference;\n', {
      filePath: join(webRoot, 'scripts/hs2-hk6kmt-invalid.mjs'),
    });
    expect(invalid.messages.map((message) => message.ruleId)).toContain('no-undef');
  });

  it('HS2-6ABCVC keeps the four Kerf packages on one exact version', () => {
    const versions = [
      packageJson.dependencies.kerfjs,
      packageJson.dependencies['@kerfjs/ui'],
      packageJson.devDependencies['create-kerf-component'],
      packageJson.devDependencies['eslint-plugin-kerfjs'],
    ];
    expect(new Set(versions).size).toBe(1);
    for (const version of versions) {
      expect(version).toMatch(/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/);
    }
  });

  it('HS2-JVSFES prevents new browser-native prompts while HS2-WMZ6Z5 retires existing ones', () => {
    const prompts: Record<string, number> = {};
    const scan = (directory: string, prefix = '') => {
      for (const entry of readdirSync(directory, { withFileTypes: true })) {
        const relative = `${prefix}${entry.name}`;
        if (entry.isDirectory()) {
          if (entry.name !== 'ux-demo') scan(join(directory, entry.name), `${relative}/`);
        } else if (/\.tsx?$/.test(entry.name) && !entry.name.endsWith('.test.ts')) {
          const source = readFileSync(join(directory, entry.name), 'utf8');
          const count = source.match(/\b(?:window|globalThis)\.prompt\s*\(/g)?.length ?? 0;
          if (count) prompts[relative] = count;
        }
      }
    };
    scan(import.meta.dirname);
    expect(prompts).toEqual({});
  });

  it('HS2-WMZ6Z5 runs all replacement-parent flows in the required WebKit route', () => {
    const packageSource = readFileSync(new URL('../package.json', import.meta.url), 'utf8');
    const script = packageSource.match(/"test:ci:webkit": "([^"]+)"/)?.[1];
    expect(script).toContain('tests/providers.spec.ts');
    expect(script).toContain('tests/native-prompt-replacements.spec.ts');
    expect(script).toContain('--browser=webkit');
    expect(script).toContain('--grep @ci-webkit');
    const flows = readFileSync(new URL('../tests/native-prompt-replacements.spec.ts', import.meta.url), 'utf8');
    expect(flows.match(/@ci-smoke @ci-webkit/g)).toHaveLength(3);
  });

  it('HS2-WM3SBV ignores generated web target files globally', () => {
    const config = readFileSync(new URL('../eslint.config.mjs', import.meta.url), 'utf8');
    const globalIgnores = config.match(/ignores: \[([\s\S]*?)\]/)?.[1];
    expect(globalIgnores).toContain("'target/**'");
    expect(globalIgnores?.match(/'target\/\*\*'/g)).toHaveLength(1);
  });

  it('keeps the bootstrap and application runtime on the shared typed-rule baseline', () => {
    const config = readFileSync(new URL('../eslint.config.mjs', import.meta.url), 'utf8');
    expect(config).not.toContain("files: ['src/main.tsx']");
    expect(config).not.toContain("files: ['src/app/runtime.tsx']");
  });

  it('limits defensive-condition exceptions to explained next-line boundaries', () => {
    const source = readFileSync(new URL('./app/runtime.tsx', import.meta.url), 'utf8');
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
