// Flaky-test quarantine for the web client's test runners (HS2-3PX1E4). See
// docs/test-quarantine.md: each quarantined test names its fix ticket and has a row there,
// which scripts/check-test-quarantine.mjs enforces.

const SLUG = /^HS2-[0-9A-Z]+$/;

/** True while the flake tracked by `ticket` stays skipped: `it.skipIf(quarantined('<fix ticket slug>'))(...)`. */
export function quarantined(ticket: string, env: Record<string, string | undefined> = process.env): boolean {
  if (!SLUG.test(ticket)) throw new Error(`quarantined() needs the fix ticket's slug, got ${ticket}`);
  return env.HOTSHEET_RUN_QUARANTINED !== '1';
}

/** Playwright `grepInvert` that drops `@quarantine` titles unless HOTSHEET_RUN_QUARANTINED=1. */
export function quarantineGrepInvert(env: Record<string, string | undefined> = process.env): RegExp | undefined {
  return env.HOTSHEET_RUN_QUARANTINED === '1' ? undefined : /@quarantine\b/;
}

/** The most retries any Playwright run may use; a test that needs more is broken, not flaky. */
export const MAX_PLAYWRIGHT_RETRIES = 2;

/**
 * Playwright `retries` (HS2-8YHY6V). Only CI's full browser suite sets HOTSHEET_PLAYWRIGHT_RETRIES; local
 * runs and the required `@ci-smoke` group leave it unset and get no retries, so a flake fails them.
 * The value is clamped to 0..MAX_PLAYWRIGHT_RETRIES, and anything unparsable means no retries.
 */
export function playwrightRetries(env: Record<string, string | undefined> = process.env): number {
  const raw = env.HOTSHEET_PLAYWRIGHT_RETRIES?.trim() ?? '';
  if (!/^\d+$/.test(raw)) return 0;
  return Math.min(Number(raw), MAX_PLAYWRIGHT_RETRIES);
}
