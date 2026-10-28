# Affected local verification

HS2-RS0FFT promotes the HS2-FCCRCK dependency planner to the supported local
gate. `scripts/check.sh --plan` prints the baseline, changed paths, selected and
omitted stages, reasons, and bounded browser commands. `scripts/check.sh` executes
that plan. `scripts/check.sh --full` selects every local package stage. CI retains
the full browser shards, Rust coverage, and platform checks.

Client changes normally run web lint, typecheck, unit tests and build, shared
format/policy/contract/generated-artifact checks, real-server contract and shell
smoke flows, and affected browser files or provider test names. They omit unrelated
Rust, migrator, and spike suites. Source-derived cross-package edges remain in
`scripts/gate-plan.mjs`: public assets can affect Rust, and Rust/migrator changes
select their dependent packages. Unknown paths, missing baseline, configuration,
lockfiles, toolchain, gate scripts, or ambiguous stages select the full package gate.
High-risk changes retain full affected suites.

## Per-ticket time budget (HS2-GN06JT)

The checked-in `scripts/gate-budget.json` sets a five-minute planning target. With
`scripts/check.sh --tier T0` or `--tier T1`, the planner always keeps formatting,
lint, typechecks, repository policy and generated-artifact checks, plus suites that
directly test changed source. It ranks the remaining affected suites by relevance
and admits only stages with a known local timing estimate that fit the remaining
budget. An unknown or CI-only estimate is deferred to covering CI. The plan lists
each selected and deferred stage, its estimate and the reason. If the mandatory
floor exceeds five minutes, every floor check still runs and the overrun is shown;
there is no deadline or mid-stage interruption. `--budget 15m` overrides the
checked-in target for that invocation. `--full` still runs the full local gate.

`--tier T2 --t2-stage '<cwd>::<exact command>'` accepts an explicit analyzed stage
selection without a time cap. Mandatory floor checks remain selected. The command
requires an explicit selection when other affected checks exist. The receipt
records that selection, so `scripts/push.sh` and the pre-push hook, which run a bare
`scripts/check.sh`, reuse a passing T2 receipt for the same tree when re-planning
with its recorded selection reproduces the exact receipt key. A bare run with no such
receipt is refused and names `--t2-stage` (HS2-BNAYKG). The risk analysis
and chosen checks belong in the ticket's preliminary thoughts. HS2-VQ5ND5 resolves
tags through normalized provider tickets. `--ticket <slug>` or
`--ticket <connection_id>:<native_id>` takes precedence, followed by one slug in
the clean HEAD commit subject, then the one live claim held by
`HOTSHEET_WORKER_ID`. Ambiguous or missing resolution is printed in `--plan` and
uses the default T1 budget. An untagged ticket also uses T1. A ticket with
`tier:T2` is uncapped and needs the analyzed stage selection when affected
non-floor stages exist; conflicting tier tags fail the plan. `--tier` remains a
headless override for an explicit analyzed or diagnostic run. HS2-EJ8JTG owns
the corresponding worker and policy wording.

`deep-testing` runs every affected local stage without a cap. `testing:browser`
selects the complete affected Playwright files rather than bounded test-name
greps. `testing:perf` adds the web performance baseline when web paths are
affected. `testing:platform` flags the ticket for Windows/Linux covering-CI
confirmation in the completion workflow. `test-budget:15m` replaces the default
T0/T1 target. Tags combine; `--plan` shows each selected stage's reason and the
resolved ticket/profile. Unknown `testing:*` tags, conflicting tier tags and
invalid budget tags fail explicitly.

The exact-input receipt records the ticket profile, budget, tier, selected and deferred commands.
Changing the budget or analyzed set changes the receipt key. Gate code and config
changes still force the full fallback. The executor reports actual elapsed time
beside the budget after success or failure. Covering main CI checks the deferred
stages before completion. HS2-DAA8AB owns quiet-host selected-versus-full outcome
comparison, and HS2-K1QNGC owns live throughput measurement.

Run additional browser checks when an actual shared routing/rendering/state change,
auth change, data format/migration, ambiguous dependency, or concrete failure needs
them. Record the reason in the ticket. A full local browser run is warranted for an
unresolved broad browser risk, rather than being a routine duplicate of CI. Visible
changes still receive real-app visual review at wide, medium, and narrow sizes,
including resize transitions. Overlapping tickets in an authorized batch share
one review of the combined result.

## Evidence and push

The executor hashes the committed tree, baseline, toolchain evidence, selected
commands, and relevant tracked input blobs. Successful stage results can be reused
only when those relevant inputs match. Source/dependency, fixture/configuration,
lock/toolchain, or baseline changes invalidate evidence. Dirty inputs are rejected;
preserve local edits and verify the committed snapshot in a clean worktree.
Inputs are rechecked after every executed stage and at completion. Failure or
interruption stops execution and cannot produce a successful receipt.

Receipts live under the Git common directory's `hotsheet-local-gate` directory.
The existing per-worktree `hotsheet-root-gate/<tree>` compatibility marker is written
only after successful verification. Push and its hook call the same executor;
`HOTSHEET_GATE_PASSED_TREE` no longer bypasses checks. The push script runs the gate
before opening the remote connection; the hook reuses valid evidence. A successful
covering main CI run is still required before ticket completion.

## Timing history (HS2-TCCKRZ)

`scripts/check.sh --timings` prints recent stage and test-file cost estimates as
JSON. The executor records wall time only after a complete successful gate and
only for stages it actually executed. Receipt hits, interrupted runs, and failed
gates add no samples. It keeps the latest 20 samples per key in
`hotsheet-gate-timings.json` under the Git common directory, outside tracked
source. The estimate is the nearest-rank p75 with a sample count; a missing key
reports `null` and `unknown`, never zero. Vitest and Playwright stages write
temporary JSON reports under their ignored `target/` directory. Nextest's JUnit
test target and module names are mapped only to Rust source files that exist in
the checkout; unmapped cases add no guessed file sample. File times are summed
test durations, so parallel execution means they are not a gate wall time.

CI uploads passing nextest/Vitest reports as `gate-timing-reports` and the merged
Playwright JSON as `browser-timing-report`. Download those artifacts from one
successful main run into one directory, then import them:

```sh
gh run download <run-id> -n gate-timing-reports -D target/ci-timings
gh run download <run-id> -n browser-timing-report -D target/ci-timings
scripts/check.sh --import-ci-timings target/ci-timings
```

The import accepts files named
`nextest-junit.xml`, `vitest-report.json`, and `playwright-report.json`, skips
failed reports and duplicate contents, and labels their samples `ci`. Local
samples take precedence over CI samples; CI estimates are a different-host seed
and must be scaled or distrusted by any budget planner. Import and `--timings`
never create gate receipts or change test selection.

## Measurement and limitations (2026-10-28)

The current UI batch's full package gate passed 2,225 Rust tests, 2,806 web unit
tests, 115 root script tests, and 539 coverage rows in roughly nine minutes on a
shared host. Its full local browser run took 35 minutes: 1,016 passed, 13 skipped,
and one obsolete assertion expected the magnified frame to differ from 5:3.
HS2-KR0C0B intentionally changed that aspect. Correcting the assertion retained
containment and PTY checks; the affected terminal suites then passed 16/16 in
1.5 minutes. This was a test-expectation mismatch, not a missed production defect.

Synthetic package selection, receipt invalidation, failure propagation, hook, and
instruction tests pass. HS2-FCCRCK's prior historical model estimated about 70
seconds per integration from package omission; this is not a measured throughput
gain. The shared-host timings are indicative wall observations, without a CPU
breakdown or a matched control. Claim-to-completion time and CI delay are recorded
on HS2-RS0FFT after shipping. HS2-DAA8AB retains quiet-host comparisons and edge
mutation validation; HS2-K1QNGC retains independent-session throughput measurement.
Neither synthetic selection proof nor a green CI run establishes zero missed
regressions. No customer content was copied.
