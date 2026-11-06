import { randomUUID } from 'node:crypto';
import { mkdirSync, mkdtempSync, rmSync } from 'node:fs';
import { availableParallelism, getPriority, loadavg, setPriority, tmpdir } from 'node:os';
import { join } from 'node:path';

import { defineConfig } from '@playwright/test';

import { playwrightServerPort } from './scripts/playwright-port.mjs';
import { runnerWebkitGuard } from './scripts/playwright-webkit-guard.mjs';
import { localPlaywrightWorkers, lowerPlaywrightPriority } from './scripts/playwright-workers.mjs';
import { isolateAiToolProbes, isolateHostHealthSampler } from './scripts/test-ai-tool-isolation.mjs';
import { playwrightRetries, quarantineGrepInvert } from './src/test-quarantine';

// Before any worker, browser, or web server starts: they inherit the lowered priority (HS2-FVT34A).
lowerPlaywrightPriority({ getPriority, setPriority }, process.env);
// Fixture servers and CLIs inherit this and never run installed AI tools (HS2-F6R2HQ).
isolateAiToolProbes();
isolateHostHealthSampler();
// Until a machine-wide browser slot pool exists, refuse repeated local WebKit and warn on any local
// WebKit run: concurrent WebKit across sessions has aborted WindowServer (HS2-CKHSM5, HS2-M6TZB5).
const webkitGuard = runnerWebkitGuard();
if (webkitGuard.warn) console.warn(webkitGuard.warn);
if (webkitGuard.refuse) throw new Error(webkitGuard.refuse);

// A free port per run unless HOTSHEET_WEB_TEST_PORT names one; workers inherit the runner's choice
// (HS2-6WH4PQ). Readiness still proves this run's own Vite answers (HS2-BJ00D1).
const port = playwrightServerPort(process.env);
const serverId = randomUUID();
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
    // The runner's stdin pipe bounds the server's lifetime: a runner ended by SIGTERM or SIGKILL
    // skips Playwright's teardown of this detached group, so EOF stops Vite instead (HS2-F6F8MA).
    // `exec` makes the launcher replace Playwright's `sh -c` and lead the group it sweeps: Ubuntu's
    // dash otherwise keeps the shell as leader and the launcher's same-group sweep never runs.
    // Windows runs the command through cmd.exe, which has no `exec` and no process groups.
    command: `${process.platform === 'win32' ? '' : 'exec '}node scripts/owned-server.mjs --exit-on-stdin-eof -- node node_modules/vite/bin/vite.js --host 127.0.0.1 --port ${port}`,
    url: `http://127.0.0.1:${port}/__hotsheet-test-server/${serverId}`,
    reuseExistingServer: false,
    env: { HOTSHEET_VITE_CACHE_DIR: viteCacheDir, HOTSHEET_WEB_STABLE_DEV: '1', HOTSHEET_WEB_TEST_SERVER_ID: serverId },
  },
  use: { baseURL: `http://127.0.0.1:${port}` },
});
