import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { configDefaults, defineConfig } from 'vitest/config';

import {
  isolateAiToolProbes,
  isolateHostHealthSampler,
  isolateLaunchedSession,
  isolateMachineHome,
  isolateNativeSecrets,
} from './scripts/test-ai-tool-isolation.mjs';
import { BINARY_SUITES } from './scripts/vitest-binary-suites.mjs';

// Before any worker starts: spawned servers and CLIs never run installed AI tools (HS2-F6R2HQ).
isolateAiToolProbes();
isolateHostHealthSampler();
isolateNativeSecrets();
// No worker, browser, web server or binary inherits a launched session's live server route
// or terminal and actor identity (HS2-KDVHN5).
isolateLaunchedSession();

const viteCacheDir = mkdtempSync(join(tmpdir(), 'hotsheet-vitest-vite-'));
// Every project's workers inherit a per-run machine home, never the real or an inherited
// `~/.hotsheet2` (HS2-34QFBQ).
const machineHomeRoot = mkdtempSync(join(tmpdir(), 'hotsheet-vitest-home-'));
isolateMachineHome(machineHomeRoot);
process.once('exit', () => {
  rmSync(viteCacheDir, { recursive: true, force: true });
  rmSync(machineHomeRoot, { recursive: true, force: true });
});

const testFiles = ['src/**/*.test.ts', 'scripts/**/*.test.mjs'];

export default defineConfig({
  cacheDir: viteCacheDir,
  test: {
    setupFiles: ['./src/source-format-matchers.ts'],
    // Vitest runs a project's globalSetup only when that project has files in the run. The
    // binary-spawning files form their own project whose setup builds the real CLI/server/
    // migrator once before any worker runs (HS2-A0M8CM); runs that select none of them skip
    // the cargo build entirely (HS2-3PX1E4).
    // Cold ESLint plugin/config loading ran past its default hook budget when it competed
    // with the parallel CI suite. Run its real policy proofs first (HS2-7KXGMC).
    projects: [
      {
        extends: true,
        test: { name: 'eslint-policy', include: ['src/eslint-policy.test.ts'], sequence: { groupOrder: 0 } },
      },
      {
        extends: true,
        test: {
          name: 'unit',
          include: testFiles,
          exclude: [...configDefaults.exclude, ...BINARY_SUITES, 'src/eslint-policy.test.ts', 'src/**/*.dom.test.ts'],
          sequence: { groupOrder: 1 },
        },
      },
      {
        extends: true,
        test: {
          name: 'dom-pilot',
          environment: 'jsdom',
          include: ['src/**/*.dom.test.ts'],
          sequence: { groupOrder: 1 },
        },
      },
      {
        extends: true,
        test: {
          name: 'binaries',
          include: BINARY_SUITES,
          globalSetup: ['./scripts/vitest-build-binaries.mjs'],
          sequence: { groupOrder: 1 },
        },
      },
    ],
  },
});
