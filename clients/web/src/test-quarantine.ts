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
