import { mkdirSync, mkdtempSync, rmSync } from 'node:fs';
import { availableParallelism, getPriority, loadavg, setPriority, tmpdir } from 'node:os';
import { join } from 'node:path';

import { defineConfig } from '@playwright/test';

import { localPlaywrightWorkers, lowerPlaywrightPriority } from './scripts/playwright-workers.mjs';
import { playwrightRetries, quarantineGrepInvert } from './src/test-quarantine';

// Before any worker, browser, or web server starts: they inherit the lowered priority (HS2-FVT34A).
lowerPlaywrightPriority({ getPriority, setPriority }, process.env);

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
  // Known flakes titled `@quarantine <slug>` are skipped unless HOTSHEET_RUN_QUARANTINED=1;
  // docs/test-quarantine.md tracks each one (HS2-3PX1E4).
  grepInvert: quarantineGrepInvert(),
  // Only CI's full browser suite retries, at most twice, and its job summary lists every test that
  // needed a retry; each one gets a follow-up ticket (docs/test-quarantine.md, HS2-8YHY6V). Local runs
  // and the required `@ci-smoke` group never retry.
  retries: playwrightRetries(),
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
