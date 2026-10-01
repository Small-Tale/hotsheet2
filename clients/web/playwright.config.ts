import { mkdtempSync, rmSync } from 'node:fs';
import { availableParallelism, loadavg, tmpdir } from 'node:os';
import { join } from 'node:path';

import { defineConfig } from '@playwright/test';

import { localPlaywrightWorkers } from './scripts/playwright-workers.mjs';

const port = Number(process.env.HOTSHEET_WEB_TEST_PORT ?? 4176);
const viteCacheDir = mkdtempSync(join(tmpdir(), 'hotsheet-playwright-vite-'));
process.once('exit', () => {
  rmSync(viteCacheDir, { recursive: true, force: true });
});

export default defineConfig({
  testDir: 'tests',
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
