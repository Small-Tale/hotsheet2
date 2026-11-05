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

## Read-only dependency edge preview (HS2-55MWQ9)

`scripts/check.sh --plan` also reports an `edgePreview`. This is a candidate
selection for two exact private paths from HS2-D461XW: the Rust CLI broken-pipe
parser body and the web trailing-task timer module created by HS2-K5E2KF. The
Rust preview reads `cargo metadata --no-deps`, walks reverse regular dependencies,
and names direct parser/CLI tests plus compilation of every reached crate. It
checks that every byte outside the private parser body is unchanged against the
Git base. The web preview requires the public-import lint boundary, public
contract tests, and shipped provider composition case, then names the focused
Vitest and browser commands. These commands are candidates alongside the
existing global floor, lint, typecheck, build, and cross-package checks; the
preview does not execute or issue receipts for them.

A public signature, generated contract, router, shared input, mixed path set,
missing baseline, missing consumer, or ambiguous read reports `mode: broad` and
keeps the existing gate. The `requiredGate` field explicitly says `unchanged
pending HS2-DAA8AB`; `scripts/check.sh`, the push hook, and exact-input receipts
still use the same required stage selection. Synthetic mutation tests pin both
candidate paths and compare private, public-contract, and cross-package plans
against the complete checked-in stage catalog. The measured required
edit-to-green reduction is **0 seconds** at this stage because the preview does
not execute; no missed failure has been observed in those plan tests.
HS2-DAA8AB's quiet-host matched selected/full outcomes, cross-edge mutations,
and independent flake check remain required before a policy change can shorten
mandatory runs. CI continues
its full regression. No measured edit-to-green saving is attributed to this
preview until those runs exist.

## Browser specs that exercise a changed module (HS2-N7EY18, HS2-R1D0S5)

Browser selection used to pick specs from path keywords (`terminal`, `timeline`,
`command`, and similar). Two defects escaped that way:

- A `terminal-viewport-runtime.ts` change ran only `terminal-feedback.spec.ts`. It
  missed the specs that route the terminal attach socket.
- A `ticket-search-field.tsx` change ran no `providers.spec.ts` case that renders the
  field in its shipped parent.

`scripts/gate-browser-exercise.mjs` now reads the working tree and adds the specs
that reach each changed client module.

The index uses source rather than a hand-kept map, so a new spec or module needs no
gate edit. It works from three kinds of hooks:

- **The module's own hooks.** These are the class tokens prefixed by its file stem
  (the root-class convention) and the classes its sibling stylesheet defines. For the
  changed module, they also include the classes it queries or toggles, with their BEM
  block. They also include the route shapes it builds or matches: the template
  `/terminals/${id}/attach` and the regex `\/terminals\/([^/]+)\/attach` both become
  `terminals/*/attach`.
- **Its direct plain-`.ts` helpers' hooks.** The runtime's attach URL comes from
  `terminal-viewport.ts`. Child `.tsx` components keep their own surfaces.
- **Its nearest shipped parents' hooks**, when the module has none of its own.
  `app/`, `main.tsx`, and `ux-demo/` are never walked through, because they import
  nearly everything. A module reached only through them keeps the existing
  shared-infrastructure rules.

Each spec is split into top-level test, `test.describe`, and helper blocks. A hook
selects specs as follows:

| Where the hook appears                                                           | What it selects                                |
| -------------------------------------------------------------------------------- | ---------------------------------------------- |
| A test body                                                                      | That test                                      |
| A helper                                                                         | Every test that calls the helper, transitively |
| Shared setup, an imported `tests/*.ts` fixture, or a direct import of the module | The whole file                                 |

`providers.spec.ts` and `ux-demo.spec.ts` are large, so each stays a bounded title
`--grep`. Other spec files run whole. A changed stylesheet stands in for its sibling
component.

Demo modules under `src/ux-demo/` follow a separate rule. A third escape showed why:
HS2-EWF0FT added a note to the shared `READER_NOTES` fixture in
`content-components-demo.tsx`, and main CI then failed two `ux-demo.spec.ts` tests that
count those notes.

- **Entries reached.** A changed demo file reaches the catalog entries declared in its
  own `demoModule.entries`. It also reaches the entries of every demo file that imports
  it, transitively; `workspace-components-demo.tsx` renders `READER_NOTES` in
  `ticket-inspector`.
- **Matching.** Those entry ids match only in specs that open `/ux-demo`, either as
  `component=<id>` or, for a multi-word id, as a quoted literal. The selected
  `ux-demo.spec.ts` tests run by title.
- **No class hooks.** Demo code is not shipped, so a demo module's class hooks never
  select shipped specs.
- **Aggregators.** `registry.ts`, `catalog.ts` and `main.tsx` compose every entry, so
  a change to one of them selects the whole demo spec.

This keeps a fixture change to the demos that render it rather than the whole spec.
For example, `content-components-demo.tsx` reaches 46 entries.

Measured selections, in spec files (counting `ux-demo.spec.ts` once when its titles
are selected) plus `providers.spec.ts` grep titles. The counts include the contract and
shell-composition floor. The suite has 87 spec files, about 494 provider test
declarations, and about 194 demo test declarations.

| Changed module                             | Before            | After                            |
| ------------------------------------------ | ----------------- | -------------------------------- |
| `terminal-viewport-runtime.ts`             | 3 files, 0 titles | 26 files, 39 titles              |
| `terminal-replay-stream.ts`                | 3, 0              | 14, 15                           |
| `terminal-ws-bridge.ts`                    | 3, 0              | 23, 25                           |
| `components/ticket-search-field.tsx`       | 2, 0              | 5, 11                            |
| `inline-search.ts` (hook-less helper)      | 2, 0              | 8, 11                            |
| `ticket-timeline-data.ts`                  | 3, 0              | 4, 7                             |
| `components/note-card.tsx`                 | 2, 0              | 8, 11                            |
| `components/app-shell.tsx`                 | 4, 1              | 18, 41                           |
| `state.ts`, `trailing-task.ts` (root-only) | unchanged         | unchanged                        |
| `ux-demo/content-components-demo.tsx`      | 2, 0              | 14 files plus 115 demo titles, 0 |
| `ux-demo/list-demo.tsx`                    | 2, 0              | 2 files plus 1 demo title, 0     |
| `ux-demo/registry.ts`                      | 2, 0              | 3 (whole `ux-demo.spec.ts`), 0   |

## Scoped repository-wide checks (HS2-3ZJBRM)

The local stage catalog now runs repository-wide Prettier and `cargo fmt`
separately, then selects web source lint only when `clients/web/` inputs or the
catalog-linked `docs/ux-components.md` change. Web source lint still includes
warning-free ESLint, generated component catalog byte checks, and UI doctor.
The ordinary `clients/web` `npm run lint` command retains all four checks for
developers and CI. Thus a Rust, migrator, spike, or unrelated docs edit does not
pay for web source lint, while web source and generated-artifact edits do.

Root Node tests are two declared stage lists. The cross-package policy list
(instruction, pinned guidance, feature coverage, quarantine, gitgist and Rulesync)
stays mandatory. The script-local tooling list is selected when the full fallback
applies. Its tests read `scripts/` and configuration inputs already classified as
full-fallback paths; a new or changed test list is unclassified until reviewed.
`scripts/`, hooks, CI, package manifests and locks, toolchains, formatter code,
shared guidance generators, and unknown paths still select every stage. The
planner tests mutate positive and negative paths, and exact-input receipt tests
check that a docs edit invalidates global formatting but not web-only lint.
Full CI keeps its existing package and browser coverage.

On the shared Mac, HS2-D461XW's 20-sample p75 for the former mandatory
repository-wide stages was 256.501 s: `cargo fmt` 2.958 s, combined root tests
74.667 s, coverage/quarantine/role checks 3.691 s, and web lint 175.185 s.
Direct runs of the new cross-package policy command and Prettier command took
5.084 s (32 tests) and 24.640 s respectively. Combining those observations with
the unchanged p75 stages gives a **36.373 s modeled sequential floor**, a
**220.128 s (85.8%) reduction** for a narrow non-web change. This is a stage-cost
comparison from different runs on one shared host, not a matched end-to-end
edit-to-green result; affected Rust, typecheck, unit, browser, and other package
stages still add their own time. HS2-DAA8AB owns the quiet-host selected/full
outcome comparison before broader gate selection changes.

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
greps. `testing:perf` forces the directly affected web, Rust ticketing, or
migrator performance command into the local gate (HS2-A4HD9X).
`testing:platform` flags the ticket for Windows/Linux covering-CI
confirmation in the completion workflow. `test-budget:15m` replaces the default
T0/T1 target. Tags combine; `--plan` shows each selected stage's reason and the
resolved ticket/profile. Unknown `testing:*` tags, conflicting tier tags and
invalid budget tags fail explicitly.

The Rust command, `node scripts/perf-ticketing.mjs`, builds the local CLI and
measures 20 Up Next listings and claim/release pairs against 100 synthetic tickets.
It caps their p95 at 1,000 ms and 2,000 ms respectively. The migrator's
`npm run test:performance` exports 500 synthetic PGLite tickets after two warmups
and caps the five-sample p95 at 500 ms. Both commands print counts, observations,
and limits, use no provider credentials or customer content, and exclude fixture
setup/build time from the measured operation. These are broad regression bounds,
not throughput estimates. A tagged docs-only or unsupported path fails the plan
with an explicit missing-baseline message. The selected commands and profile are
part of exact-input receipts; unknown timing estimates remain visible in `--plan`
and cannot silently defer a forced performance stage. The opt-in checks add roughly
one minute of local setup for Rust on the measured busy host and a few seconds for
the migrator; CI still runs its full functional suites and does not treat these
local performance bounds as a replacement for covering CI.

The exact-input receipt records the ticket profile, budget, tier, selected and deferred commands.
The diagnostic text explaining whether a ticket came from `--ticket` or the clean commit
subject is excluded from the key; the qualified ticket, tags and effective settings remain
exact inputs, so a bare push check reuses the ticket gate's receipt (HS2-Q5XAAK).
Changing the budget or analyzed set changes the receipt key. The key also hashes the
build and browser environment, including the Playwright worker count, retry allowance and
quarantine opt-in (`HOTSHEET_PLAYWRIGHT_WORKERS`, `HOTSHEET_PLAYWRIGHT_RETRIES`,
`HOTSHEET_RUN_QUARANTINED`), because each changes what a browser run proves (HS2-XW38CB).
Gate code and config
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

The main publishing reservation reads its Git ref once and then reads the captured
immutable object (HS2-W110HQ). A release or replacement between those reads cannot
turn a valid snapshot into an ambiguous-ref failure or substitute another owner's
record. An absent ref permits admission; Git failures and malformed records remain
errors. Compare-and-swap acquisition, renewal and release still decide ownership.

HS2-BYDMV4 distinguishes quiet unresolved lookup from proven absence. Only a quiet
failed OID lookup uses Git's typed `show-ref --exists` result: missing status 2 is
absence at that later instant only if Git's own loose-ref path is absent. Git can
return status 2 for a directory at that path, which blocks acquisition; this
physical collision fails closed without reading ref contents. Present status 0 is
unresolved, and other outcomes
fail closed. Ordinary status-2 diagnostic text is not language-parsed; subprocess
errors, signals and unexpected output are not absence. Git installations without
that capability fail with an explicit support/readability error, not a guessed
minimum version or a quiet-missing assumption. A captured valid OID never uses
this fallback and is still read only as an immutable blob.

Only acquisition may retry unresolved presence, within its existing one-second
contention cadence and ten-minute budget; persistent unresolved presence reports
an actionable ref-inspection error. Owner verification, renewal and release fail
closed. Zero-OID acquisition and expected-old-OID updates/deletes retain their CAS
fences. The retained fourteen-case matrix used different Git binaries reporting
the same Apple Git version; it is not cross-version or ref-backend verification.
Real-Git and interleaving regressions run against the local Apple Git 2.54 files backend.

## Batch smoke profile (HS2-9G0M3V)

`scripts/check.sh --smoke` is the one gate of a batch integration push, and of a
ci-staged push to the integration branch (HS2-GT57BQ). A ci-staged promotion of
`main` runs no local gate: the green CI run on that exact staged SHA is its gate. It runs
exactly the stages declared in `scripts/gate-smoke.json`: build or compile, lint, and
a fast sanity set. It never expands to affected suites, and the declaration refuses
browser suites (`playwright`, `test:browser`, `test:e2e`). Each stage declares only a
`cwd`, a `command` and a `reason`; duplicates and unknown keys are refused. Today's
set:

- `cargo fmt --all --check` and `cargo lint` (every Rust target compiled and linted);
- `node scripts/sync-role-skills.mjs --check` and `cargo nextest run -p
hotsheet-plugins` (skill, plugin and mirror contracts);
- `npm run format:sources:check`, `npm run lint:web` and `npm run typecheck` in
  `clients/web`.

The profile cannot be combined with `--full`, `--ticket`, `--tier`, `--t2-stage` or
`--budget`. Its receipt is exact for the whole tree, under its own `smoke` profile
and tier, so it is never reused as an affected-gate receipt, nor the reverse. It never
writes or clears the `hotsheet-root-gate/<tree>` marker that serial prerequisite
resumption reads. `--plan` lists every other `check-full.sh` stage as deferred to the
batch CI run. `scripts/push.sh` accepts `--gate-arg --smoke` only for a live batch
integrator, and requires it for a multi-assignment push. See
[05 §5.9.1 batch integration](05-ai-tool-plugins.md#batch-integration-hs2-9g0m3v).

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
