import { randomUUID } from 'node:crypto';
import { mkdirSync, mkdtempSync } from 'node:fs';
import { availableParallelism, getPriority, loadavg, setPriority, tmpdir } from 'node:os';
import { join } from 'node:path';

import { defineConfig } from '@playwright/test';

import { holdRunnerWebkitSlots } from './scripts/browser-slots.mjs';
import { recordBridgeOwner, releaseBridgeHome, sweepAbandonedBridgeHomes } from './scripts/playwright-bridge-home.mjs';
import { playwrightServerPort } from './scripts/playwright-port.mjs';
import { playwrightTestArgs, runnerWebkitGuard } from './scripts/playwright-webkit-guard.mjs';
import { localPlaywrightWorkers, lowerPlaywrightPriority } from './scripts/playwright-workers.mjs';
import {
  isolateAiToolProbes,
  isolateHostHealthSampler,
  isolateLaunchedSession,
  isolateNativeSecrets,
} from './scripts/test-ai-tool-isolation.mjs';
import { playwrightRetries, quarantineGrepInvert } from './src/test-quarantine';

// Before any worker, browser, or web server starts: they inherit the lowered priority (HS2-FVT34A).
lowerPlaywrightPriority({ getPriority, setPriority }, process.env);
// Fixture servers and CLIs inherit this and never run installed AI tools (HS2-F6R2HQ).
isolateAiToolProbes();
isolateHostHealthSampler();
isolateNativeSecrets();
// No worker, browser, web server or binary inherits a launched session's live server route
// or terminal and actor identity (HS2-KDVHN5).
isolateLaunchedSession();
// Concurrent WebKit across sessions has aborted WindowServer. Every local WebKit browser holds a
// machine-wide `browser-engines` slot; without that pool, refuse repeated local WebKit and warn on
// any local WebKit run (HS2-CKHSM5, HS2-M6TZB5).
const webkitGuard = runnerWebkitGuard();
if (webkitGuard.warn) console.warn(webkitGuard.warn);
if (webkitGuard.refuse) throw new Error(webkitGuard.refuse);
const localWorkers = localPlaywrightWorkers({
  cpus: availableParallelism(),
  load1: loadavg()[0],
  override: process.env.HOTSHEET_PLAYWRIGHT_WORKERS,
  ci: Boolean(process.env.CI),
});
// `--browser webkit|all` launches a WebKit browser per worker: the runner holds one slot per worker
// before any worker starts, and runs only as many workers as it holds slots (HS2-M6TZB5).
const runnerSlots = await holdRunnerWebkitSlots({
  args: playwrightTestArgs(process.argv),
  env: process.env,
  workers: localWorkers,
});

// A free port per run unless HOTSHEET_WEB_TEST_PORT names one; workers inherit the runner's choice
// (HS2-6WH4PQ). Readiness still proves this run's own Vite answers (HS2-BJ00D1).
const port = playwrightServerPort(process.env);
const serverId = randomUUID();
const viteCacheDir = mkdtempSync(join(tmpdir(), 'hotsheet-playwright-vite-'));
// The dev bridge never sees the developer's real Hot Sheet homes: their remote registry, machine
// secrets, bootstrap server, or HS1 data (HS2-9NB1KA). Fixture servers name their own homes.
const bridgeHome = mkdtempSync(join(tmpdir(), 'hotsheet-playwright-home-'));
const bridgeLegacyHome = mkdtempSync(join(tmpdir(), 'hotsheet-playwright-legacy-home-'));
// Legacy visual captures remain easy to inspect, with portable package-relative paths.
// These are separate from Playwright's rotated output and safe alongside Kerf doctor scans.
for (const directory of ['claude', 'claude-501'])
  mkdirSync(join('target', 'visual-captures', directory), { recursive: true });
// Stop the bootstrap server the bridge starts under its home before the home is removed; removing the
// directory alone left that detached server running after every run (HS2-B0WP94).
process.once('exit', () => {
  releaseBridgeHome({ home: bridgeHome, directories: [viteCacheDir, bridgeHome, bridgeLegacyHome] });
});
// A runner killed outright (SIGKILL) never reaches that handler, so its server and directories
// outlive it. Each run records its owner, and the next runner releases homes whose owner is gone
// (HS2-P69XHN). Workers load this config too; only the runner sweeps.
recordBridgeOwner({ home: bridgeHome, directories: [viteCacheDir, bridgeHome, bridgeLegacyHome] });
if (process.env.TEST_WORKER_INDEX === undefined) sweepAbandonedBridgeHomes({ root: tmpdir() });

export default defineConfig({
  testDir: 'tests',
  // Kerf doctor scans this package while browser tests run. Keep Playwright's rotated output
  // under a generated directory that Kerf, ESLint, stable-dev, and Git all exclude (HS2-ZJJ4K6).
  outputDir: 'target/playwright-test-results',
  // One shared Vite dev server serves every worker; scale parallelism to the machine's existing load
  // and give whole flows and assertions headroom for a busy CPU (HS2-MHPHZB). `--workers` or
  // HOTSHEET_PLAYWRIGHT_WORKERS still overrides the count; local WebKit runs as many workers as it
  // holds slots (HS2-M6TZB5).
  workers: runnerSlots?.workers ?? localWorkers,
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
    env: {
      HOTSHEET_VITE_CACHE_DIR: viteCacheDir,
      HOTSHEET_WEB_STABLE_DEV: '1',
      HOTSHEET_WEB_TEST_SERVER_ID: serverId,
      HOTSHEET_HOME: bridgeHome,
      HOTSHEET_LEGACY_HOME: bridgeLegacyHome,
    },
  },
  use: { baseURL: `http://127.0.0.1:${port}` },
});
