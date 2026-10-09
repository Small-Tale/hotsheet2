import { mkdirSync, mkdtempSync, rmSync } from 'node:fs';
import { availableParallelism, loadavg, tmpdir } from 'node:os';
import { join } from 'node:path';

import { defineConfig } from '@playwright/test';

import { localPlaywrightWorkers } from './scripts/playwright-workers.mjs';

const port = Number(process.env.HOTSHEET_WEB_TEST_PORT ?? 4176);
const viteCacheDir = mkdtempSync(join(tmpdir(), 'hotsheet-playwright-vite-'));
// Legacy visual captures remain easy to inspect, with portable package-relative paths.
// These are separate from Playwright's rotated output and safe alongside Kerf doctor scans.
for (const directory of ['claude', 'claude-501'])
  mkdirSync(join('target', 'visual-captures', directory), { recursive: true });
process.once('exit', () => {
  rmSync(viteCacheDir, { recursive: true, force: true });
});

export default defineConfig({
  testDir: 'tests',
  // Kerf doctor scans this package while browser tests run. Keep Playwright's rotated output
  // under a generated directory that Kerf, ESLint, stable-dev, and Git all exclude (HS2-ZJJ4K6).
  outputDir: 'target/playwright-test-results',
  // One shared Vite dev server serves every worker; scale parallelism to the machine's existing load
  // and give whole flows and assertions headroom for a busy CPU (HS2-MHPHZB). `--workers` or
  // HOTSHEET_PLAYWRIGHT_WORKERS still overrides the count.
  workers: localPlaywrightWorkers({
    cpus: availableParallelism(),
    load1: loadavg()[0],
    override: process.env.HOTSHEET_PLAYWRIGHT_WORKERS,
    ci: Boolean(process.env.CI),
  }),
  timeout: 60_000,
  expect: { timeout: 10_000 },
  webServer: {
    command: `npm run dev:hot -- --port ${port}`,
    url: `http://127.0.0.1:${port}/ux-demo`,
    reuseExistingServer: false,
    env: { HOTSHEET_VITE_CACHE_DIR: viteCacheDir, HOTSHEET_WEB_STABLE_DEV: '1' },
  },
  use: { baseURL: `http://127.0.0.1:${port}` },
});
