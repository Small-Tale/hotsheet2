import { readdirSync, readFileSync } from 'node:fs';
import { join, relative, sep } from 'node:path';

import { ESLint } from 'eslint';
import { beforeAll, describe, expect, it } from 'vitest';

import packageJson from '../package.json';

describe('web ESLint policy', () => {
  it('HS2-7KXGMC schedules the cold policy route before parallel unit work without dropping fast-suite coverage', () => {
    const config = readFileSync(new URL('../vitest.config.ts', import.meta.url), 'utf8');
    expect(config).toContain(
      "name: 'eslint-policy', include: ['src/eslint-policy.test.ts'], sequence: { groupOrder: 0 }",
    );
    expect(config).toContain("...BINARY_SUITES, 'src/eslint-policy.test.ts'");
    // HS2-3FQNGT adds one isolated jsdom pilot after the policy route.
    expect(config.match(/sequence: \{ groupOrder: 1 \}/g)).toHaveLength(3);
    expect(config).toContain("'src/**/*.dom.test.ts'");
    expect(config).toContain("name: 'dom-pilot'");
    expect(config).toContain("environment: 'jsdom'");
    expect(packageJson.scripts['test:unit:fast']).toContain('--project eslint-policy --project unit');
    expect(config).not.toMatch(/\b(?:hookTimeout|testTimeout|maxWorkers)\b/);
  });

  it('HS2-V94A8M never waits for networkidle in browser tests while the change long poll stays open', () => {
    // A live page always has a change poll in flight (HS2-AZ11V9), so `networkidle` never arrives.
    const tests = new URL('../tests/', import.meta.url);
    const offenders = readdirSync(tests)
      .filter((name) => name.endsWith('.ts'))
      .filter((name) =>
        /networkidle/.test(readFileSync(new URL(name, tests), 'utf8').replace(/^\s*(?:\/\/|\*).*$/gm, '')),
      );
    expect(offenders).toEqual([]);
  });

  it('HS2-3TWWWD bounds the type-aware ESLint heap so it does not depend on machine memory', () => {
    // Typed linting of the client peaks near 3.3 GB and needs about 2.5 GB of heap. Node's default
    // heap limit is derived from RAM, so the 7 GB macOS runner aborted at about 2 GB. Pin an explicit,
    // cross-platform bound on the one ESLint invocation.
    // HS2-9N8ZMD moved this invocation into the observed lint entrypoint. Its focused
    // tests check the ordered arguments and first-failure behavior; keep the heap
    // bound pinned here as a policy on the shipped helper.
    expect(packageJson.scripts['lint:web']).toBe('node scripts/lint-web-observations.mjs');
    const helper = readFileSync(new URL('../scripts/lint-web-observations.mjs', import.meta.url), 'utf8');
    expect(helper).toMatch(/id: 'web-lint:eslint',[\s\S]*?'--max-old-space-size=4096'/);
    expect(helper).toMatch(/id: 'web-lint:eslint',[\s\S]*?'node_modules\/eslint\/bin\/eslint\.js'/);
    expect(Object.values(packageJson.scripts).filter((script) => /\beslint(?:\.js)? \./.test(script))).toEqual([]);
  });

  it('HS2-TY5JHN bounds the type-aware Kerf UI doctor heap the same way', () => {
    // After HS2-3TWWWD the macOS runner's ESLint passed and the next type-aware step, the Kerf UI
    // doctor, aborted at the same RAM-derived ~2 GB default heap. Pin the same explicit bound.
    expect(packageJson.scripts['ui:doctor']).toBe(
      'node --max-old-space-size=4096 node_modules/@kerfjs/ui/doctor/cli.mjs --full',
    );
    expect(Object.values(packageJson.scripts).filter((script) => /\bkerf-ui-doctor\b/.test(script))).toEqual([]);
  });

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

  // HS2-X3JM3J: each proof runs the real ESLint route; a typed lint of a src/ path builds the
  // TypeScript program inside its case (2-13 s on a loaded gate host). The budget covers that work;
  // no lint result is cached.
  describe('HS2-RY22P6 shares configuration setup across actual Node lint proofs', { timeout: 30_000 }, () => {
    const webRoot = join(import.meta.dirname, '..');
    const eslint = new ESLint({ cwd: webRoot });
    const scripts = readdirSync(join(webRoot, 'scripts'))
      .filter((name) => /\.(?:mjs|mts)$/.test(name))
      .map((name) => `scripts/${name}`);
    beforeAll(async () => {
      // Config/plugin loading is shared setup, not the first file's lint work (HS2-RY22P6).
      // Keep the default hook budget and every per-input proof; no lint result is cached here.
      await eslint.calculateConfigForFile(join(webRoot, 'eslint.config.mjs'));
    });
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

    it('HS2-K5E2KF rejects imports of the trailing-task implementation', async () => {
      const [invalid] = await eslint.lintText(
        "import { createTrailingTaskInternal } from './trailing-task-internal';\n",
        {
          filePath: join(webRoot, 'src/trailing-task.test.ts'),
        },
      );
      expect(invalid.messages.map((message) => message.ruleId)).toContain('no-restricted-imports');
      const publicSource = readFileSync(join(webRoot, 'src/trailing-task.ts'), 'utf8');
      expect(publicSource.match(/^export (?:interface|function).*$/gm)).toEqual([
        'export interface TrailingTask<T> {',
        'export function createTrailingTask<T>(delayMs: number, run: (value: T) => void): TrailingTask<T> {',
      ]);
    });

    it('HS2-ECT6FE rejects every form of the web APIs the desktop webview cannot show (HS2-JVSFES, HS2-WMZ6Z5)', async () => {
      const forms = [
        "alert('a');",
        "confirm('a');",
        "prompt('a');",
        "window.alert('a');",
        "window.confirm('a');",
        "window.prompt('a');",
        "globalThis.confirm('a');",
        "self.prompt('a');",
        "open('https://example.test');",
        "window.open('https://example.test');",
        "globalThis.open('https://example.test');",
      ];
      const [result] = await eslint.lintText(`${forms.join('\n')}\n`, {
        filePath: join(webRoot, 'src/child-process.ts'),
      });
      const banned = new Set(['no-alert', 'no-restricted-globals', 'no-restricted-properties']);
      const flaggedLines = new Set(
        result.messages.filter((message) => banned.has(message.ruleId ?? '')).map((message) => message.line),
      );
      expect([...flaggedLines].sort((left, right) => left - right)).toEqual(forms.map((_, index) => index + 1));
    });

    it('HS2-ECT6FE HS2-6S1346 HS2-0A7FWS allows no native dialog and opens windows only through openExternal', () => {
      let dialogs = 0;
      const windowOpeners: string[] = [];
      const scan = (directory: string) => {
        for (const entry of readdirSync(directory, { withFileTypes: true })) {
          const path = join(directory, entry.name);
          if (entry.isDirectory()) scan(path);
          else if (/\.tsx?$/.test(entry.name) && !entry.name.endsWith('.test.ts')) {
            const source = readFileSync(path, 'utf8');
            // Any suppressed alert, confirm, or prompt counts, whatever ticket its reason names.
            dialogs += source.match(/eslint-disable[^\n]*\bno-alert\b/g)?.length ?? 0;
            // Any suppressed window.open (the only no-restricted-properties use for it).
            for (const match of source.match(/eslint-disable[^\n]*\bno-restricted-properties\b[^\n]*/g) ?? [])
              if (/open|window|HS2-0A7FWS/i.test(match))
                windowOpeners.push(relative(webRoot, path).split(sep).join('/'));
          }
        }
      };
      scan(join(webRoot, 'src'));
      // Confirmations use the shared in-app ConfirmationDialog (HS2-6S1346). New windows go through
      // src/open-external.ts, which the desktop shell routes to the system browser (HS2-0A7FWS).
      expect(dialogs).toBe(0);
      expect(windowOpeners).toEqual(['src/open-external.ts']);
    });
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
