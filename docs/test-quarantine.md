# Flaky-test quarantine

A test that fails intermittently for reasons unrelated to the change under test is
**quarantined**: it is skipped by default, so it neither blocks a gate nor triggers a
"just rerun it" loop, and it stays listed here until its fix ticket lands (HS2-3PX1E4).
Quarantine is a holding state, not a fix. Every entry names an open ticket that owns the
repair, and fixing the test removes both its marker and its row.

## Tests that needed a retry

Only CI's full browser suite (`browser-full`) retries a failed Playwright test, at most twice
(HS2-8YHY6V). Local runs and the required `@ci-smoke` and `@ci-webkit` groups never retry.
A test that passes only on a retry does not fail the job, but it stays visible: the merged
HTML report marks it flaky, and the `browser-full-report` job summary lists it under
"Browser tests that needed a retry" (`scripts/playwright-retries.mjs`), with a warning
annotation for each.

**A test that needs a retry gets a follow-up ticket.** File (or reuse) a bug ticket for its
root cause, with the CI run and the failed attempt's error as evidence. A retry is a
reprieve, not a fix. If the test keeps needing retries before its fix lands, quarantine it
below.

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

| Ticket     | Tier       | File                                  | Test                                                                                                                   | Since      | Reason                                                                                          |
| ---------- | ---------- | ------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------- | ----------------------------------------------------------------------------------------------- |
| HS2-9PDWMK | Playwright | `clients/web/tests/providers.spec.ts` | paints terminal dashboard transitions immediately and progressively mounts only visible previews at scale (HS2-XEGA6V) | 2026-10-16 | 100 ms transition budget exceeded (184 ms) on a 2-vCPU `browser-full` shard in one of two runs  |
| HS2-WGZMQE | Playwright | `clients/web/tests/providers.spec.ts` | keeps primary 138-ticket interactions within the painted UI budget                                                     | 2026-10-16 | 100 ms painted budget exceeded (107-227 ms) on a 2-vCPU `browser-full` shard in one of two runs |
| HS2-X0A331 | Playwright | `clients/web/tests/providers.spec.ts` | switches and searches large ticket views without cloning every row into motion ghosts                                  | 2026-10-16 | 250 ms budget times the Playwright `fill` round trip (284 ms on a CI shard)                     |

<!-- quarantine:end -->
