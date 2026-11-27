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

## Native file-event latency

A file-watcher test that times out while `fseventsd` is backlogged (busy builds or other
churn in the shared temp directory) is not a flake to quarantine: the event was late, not
lost (HS2-P702Y8, HS2-BN9MMJ). Watcher consumers are tested on the in-process stream
(`hotsheet_jobs::watch::InProcessStream`, or the store watcher's `StartupHooks::stream`).
Each module keeps one real-stream smoke test, and the platform adapter's own tests stay
real; those wait on ordered delivery (a marker written after the events they check) up to
`REAL_STREAM_TEST_BOUND`, never on a quiet window. A real-stream test that still times out
past that bound gets a ticket with a retries-off reproduction, as any other.

The same rule holds for Node `fs.watch` in `clients/web`. On macOS, closing a directory
watcher blocks the event loop until `fseventsd` re-registers the process's stream, so a
real watcher inside an ordinary unit test can push it past its timeout under load
(HS2-E0185C). Inject the module's watcher seam (for example `WatchDirectory` in
`src/migration-jobs.ts`) and keep one real-watcher smoke test bounded like
`REAL_STREAM_TEST_BOUND`.

## Quarantined tests

<!-- quarantine:begin -->

| Ticket     | Tier       | File                                  | Test                                                                                                                   | Since      | Reason                                                                                          |
| ---------- | ---------- | ------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- | ---------- | ----------------------------------------------------------------------------------------------- |
| HS2-9PDWMK | Playwright | `clients/web/tests/providers.spec.ts` | paints terminal dashboard transitions immediately and progressively mounts only visible previews at scale (HS2-XEGA6V) | 2026-10-16 | 100 ms transition budget exceeded (184 ms) on a 2-vCPU `browser-full` shard in one of two runs  |
| HS2-WGZMQE | Playwright | `clients/web/tests/providers.spec.ts` | keeps primary 138-ticket interactions within the painted UI budget                                                     | 2026-10-16 | 100 ms painted budget exceeded (107-227 ms) on a 2-vCPU `browser-full` shard in one of two runs |

<!-- quarantine:end -->
