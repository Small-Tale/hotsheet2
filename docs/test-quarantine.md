# Flaky-test quarantine

A test that fails intermittently for reasons unrelated to the change under test is
**quarantined**: it is skipped by default, so it neither blocks a gate nor triggers a
"just rerun it" loop, and it stays listed here until its fix ticket lands (HS2-3PX1E4).
Quarantine is a holding state, not a fix. Every entry names an open ticket that owns the
repair, and fixing the test removes both its marker and its row.

## Quarantining a test

1. File (or reuse) a bug ticket that describes the flake and its evidence.
2. Add the marker naming that ticket:
   - **Rust:** `#[ignore = "quarantine HS2-XXXXXX: <reason>"]`. Run quarantined Rust
     tests with `cargo nextest run --run-ignored only -E 'test(<name>)'`.
   - **Vitest (`clients/web`):** `it.skipIf(quarantined('HS2-XXXXXX'))('<name>', ...)` with
     `quarantined` from `src/test-quarantine.ts`.
   - **Playwright (`clients/web`):** put `@quarantine HS2-XXXXXX` in the test title.
     `playwright.config.ts` excludes `@quarantine` titles.
3. Add a row below. Set `HOTSHEET_RUN_QUARANTINED=1` to run the quarantined Vitest and
   Playwright tests anyway, for example while fixing one.

`scripts/check-test-quarantine.mjs` runs in CI and fails when a marker has no row, or a
row has no marker.

## Quarantined tests

<!-- quarantine:begin -->

| Ticket | Tier | File | Test | Since | Reason |
| ------ | ---- | ---- | ---- | ----- | ------ |

<!-- quarantine:end -->
