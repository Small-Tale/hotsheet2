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

HS2-PDNJA0 makes a Rust server router edit or an OpenAPI source edit select the
real-server `contracts.spec.ts` browser check and its fixture as mandatory local
work. A router-only or OpenAPI-only edit does not pull in shell composition. This
closes the selection gap exposed by HS2-F1QJ03's missing note-doctor contract.

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

## Browser engines in gate evidence (HS2-HW6VED)

Every selected and deferred Playwright stage in `--plan` output and in the receipt carries
its actual engines, files and grep under `browserCoverage`, together with the CI browser
matrix. A passing gate prints `Browser engines run locally: …` and, for deferred browser
stages, `Browser engines deferred to CI: …`. The default engine is Chromium. WebKit appears
only for `--browser=webkit` commands and for specs that launch it themselves. A handoff
quotes these lines rather than describing focused WebKit coverage as a full engine matrix.
The matrix and its measurements are in
[12-code-organization-and-testing.md](12-code-organization-and-testing.md#browser-engine-matrix-hs2-hw6ved).

## Platform cfg code in gate evidence (HS2-A7X8F5)

The gate compiles Rust only for its host, and a local cross `cargo check` cannot build the C
dependencies' build scripts. `--plan` output and each receipt therefore carry `platformCfg`:
the changed Rust files whose changed lines lie in, or are, platform `cfg`-gated code. A passing
or reused gate prints `Platform cfg code deferred to CI: …`. A handoff quotes that line, and the
covering `main` CI run (Linux) or the Platforms workflow (Windows, macOS) is the evidence for
those branches. The scan and its measured 55-65 ms cost are described in
[12-code-organization-and-testing.md](12-code-organization-and-testing.md).

## Workspace chrome selection (HS2-P9A21R, HS2-4S22C4)

A change to a workspace chrome owner selects the phone navigation and resize cases
in `mobile-layout.spec.ts`, the title remeasurement case in `search-scope.spec.ts`,
the warm-switch search width cases in `project-tab-warm-cache.spec.ts`, and the
mode-transition and search-focus cases in `providers.spec.ts`. The owners are
`app-shell`, `workspace-controls`, `workspace-header` and
`workspace-composition-surfaces` (`.tsx` or `.css`), `workspace-toolbar-visibility.ts`
and `features/hot-sheet-shell.tsx`. HS2-6PZ35D narrowed the phone workspace search
from `app-shell.tsx` and broke the last two after a green local gate.

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

**Catalog specimen counts (HS2-B42XMS).** The `ux-demo` inventory test (titled after
`CatalogExample`, HS2-Z3FMEZ) pins each catalog entry's specimen count. A changed demo file selects
the tests that open its entries. An entry id with a hyphen also matches the inventory's
`['entry-id', count]` literal, but a single-word id (`list`, `toolbar`) does not. When a changed
`ux-demo` file renders a different number of `<CatalogExample` specimens than its base (added,
removed, new or deleted file), the plan therefore adds the inventory title to the `ux-demo` grep.
Edits that keep the count, aggregators and test files add nothing, and other demo flows stay
deferred. Measured on 2026-11-07: the planning check took about 20 ms, and the inventory test ran
in 9.2 s in Chromium, including Vite startup.

**Catalog-walking tests (HS2-1DF5PQ).** Some browser tests render every catalog entry without
naming one. The phone audit `touch-field-font.spec.ts` › `every UX demo keeps its editable text
fields at 16px or larger` walks the catalog through `flattenCatalog` from `/src/ux-demo/catalog.ts`.
Because no entry id appears, a new or changed demo never matched it, which is how CI run
41296063177 found 12 px Announcer demo fields on phones after the local gate passed. Any changed
`ux-demo` source (not tests) now selects each catalog-walking test by its own title: the nearest
enclosing `test('…')`, through a per-file `--grep` stage. A walk outside any test (in a helper)
selects the whole file. Measured on 2026-11-07, the audit test alone took 17.0 s in Chromium,
against 53.4 s for its whole file. Shared layout compositions such as `project-tab-bar.tsx`
already select their geometry cases through the exercise index; the project-strip overflow case
is selected for a `project-tab-bar.tsx` change.

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
CI's root test steps hand-list the same files, and `scripts/gate-plan.test.mjs`
fails when a file in either list is missing from `.github/workflows/ci.yml` or CI
runs a root test that neither list classifies (HS2-E6NFF6).
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

### Verified ticket worktree cleanup (HS2-HR3ZX4)

After a direct ticket or reviewed code handoff is completed, its claim is released,
and a green `main` CI run covers the shipping commit, invoke
`node scripts/verified-worktree-cleanup.mjs --project <main-checkout> --store
<store> --ticket <slug> --worktree <path> --shipping <full-main-sha>`. Add
`--operation <assignment-id>` for an integrator-reviewed handoff. The handoff
requires its durable integration record and reviewed worktree/branch proposal.
For a reviewed findings handoff later retired with its recovery note preserved,
cleanup also requires the exact findings, review-return, and retirement notes plus
one later same-worker code handoff that is done and shipped through the usual green
batch and CI proof (HS2-X4MM1P). Active, ambiguous, or unmatched handoffs refuse.
The command rechecks ownership, no live claim or work, an unlocked clean worktree,
source preservation, remote `main` ancestry, the exact-tree local gate receipt,
and covering green `main` CI immediately before removal. It uses ordinary
`git worktree remove`, then compare-and-deletes only the verified branch tip
(HS2-EBART6). A direct branch must be an ancestor of the live `origin/main` SHA
from `git ls-remote`, never the primary checkout's possibly lagging local `main`
or a stale tracking ref; a reviewed handoff may instead be preserved by its
archive ref. A missing, unfetched, or diverged `origin/main` refuses before any
mutation. Ignored `target/` output leaves with the worktree. The JSON result
reports each mutation in `mutations` (`worktree`: `removed` or `already_removed`;
`branch`: `removed`, `preserved`, `absent`, or `verified` on a dry run). If the
worktree was removed but the branch was preserved, `state` is `partial`, a
`branchReason` names why, and the command exits 2. A durable intent in the Git
common directory lets the same command, rerun, finish only the remaining verified
branch deletion without claiming the worktree removal again. It reports
reclaimed filesystem bytes; a refusal keeps the checkout intact and names the missing proof. Historical
dirty worktrees and stashes need the separate HS2-J37WRT review.

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
For a project with a configured development strategy, HS2-RRNZYN also requires a
reviewed version 1 JSON test plan after the implementation and before the gate:
`scripts/check.sh --test-plan /path/to/reviewed.json`. The plan names the qualified
ticket, current `tree` and `base` from `--plan`, sorted `changed_paths`, effective
`strategy` and `strategy_source`, behavior, risks, `reviewed_by`, `analyzed` and `proposed` stage
identifiers, and every selected and deferred `<cwd>::<command>`. Each selected
entry records a reason and nonnegative `estimate_ms`, or `null` with
`estimate_source: "unknown"` when no trustworthy history exists. The plan output
and receipt show the known estimate total and count of unknown checks; neither
turns an unknown into a zero-cost claim. Each deferred entry records
a reason and `ci_coverage`. An explicit override also names its author and reason.
The deferred list includes every unselected registered local stage, including
unaffected Rust, migrator, and spike checks in a web-only plan; each entry names
the covering CI job. Target-specific Windows, macOS, and arm64 code remains
separate platform evidence under `platformCfg` and needs the `Platforms` workflow,
not a claim that Linux x86 main CI executed those targets.
The checked-in gate computes the mandatory floor and checks the plan against its
actual selection; a plan cannot suppress a mandatory check.
Auth, claim, sync, session, checkout, and shared-state source changes also force
the shipped-flow browser contract and its fixture stage. Migration changes retain
their direct migrator suite in the mandatory floor. The plan is stored outside
the source tree under the Git common gate cache and its hash, selection,
strategy provenance, and risk rationale enter the exact-input receipt. The bare
push check reuses it only for that tree. A changed tree, diff, ticket, or strategy
requires a newly reviewed plan. An unconfigured project retains its existing
risk-selected gate without a plan. A configured strategy is resolved by the
current-source `hotsheet-cli development-strategy-resolve` capability; build the
CLI in the checkout if the installed binary lacks that command. Resolution fails
clearly rather than silently applying a different strategy. A failed local gate
cannot be waived by a green remote run.

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

A failed suite stage does not rerun the whole suite (HS2-5340N1). When a Playwright,
Vitest, or nextest stage fails and its fresh per-case report maps every failed case
to a rerunnable selector, the gate stores `stage-<key>.partial.json` beside the stage
cache. The selectors are a Playwright `/file:line` location (the stage's other options
are kept), a Vitest file plus an anchored full-name `-t` pattern, or a nextest
`binary_id(=…) & test(=…)` filter. The next attempt for the same stage key runs only
those cases, and earlier passing stages stay reused as before. If each requested case
passes, the stage passes and the cases are recorded as `flaky` in the stage record, the
receipt, and the log; each still needs a flake ticket. If some still fail, the record
narrows to them. An interrupted run, a missing or unreadable report, a nonzero exit
with no failed case, a file-level failure, an unexpected command shape, or a requested
case the narrowed run did not execute leaves no record, so the next attempt reruns
the whole stage. A changed stage key (tree, command, or inputs) also invalidates the
record. The gate never retries within one run; retries stay a CI-only policy.

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

`scripts/check.sh --timings` prints recent stage and test-file cost estimates and
HS2-5RDMEV outcome observations as JSON. The estimator still uses only completed
successful work: the latest 20 samples per stage/file key, local samples preferred
over CI, nearest-rank p75, and `null`/`unknown` for an unsampled key. A separate
bounded ledger in `hotsheet-gate-timings.json` under the Git common directory
records up to 20 gate attempts. Each attempt has a cohort, hashed host id, OS,
selected and deferred checks, and reached check outcomes. A started check is
provisionally interrupted, so an abruptly stopped gate retains its last reached
check; a completed check replaces that provisional outcome. Receipt and stage-cache
reuse are `reused`, never a fast pass. Summary distributions separate pass and
failure wall times (p50, p75, p90), plus pass/fail/skip/reused/interrupted counts,
selected/deferred denominators, and unknown unsampled checks. The JSON export
includes separate per-cohort check and case summaries so CI and selected local
worktrees are not pooled as one sampling population. First failure still
stops the gate and exact-input receipts are unchanged. File times sum test durations,
so parallel execution means they are not a gate wall time.

Runner reports add test-case identities and outcomes when present. Vitest and
Playwright stages write temporary JSON under ignored `target/`; nextest writes
JUnit. Case titles are hashed and only safe project-relative source paths and
metrics enter the history. Raw failures, user data, and machine-local paths are
discarded. The `clients/web/scripts/lint-web-observations.mjs` helper is the
`lint:web` entrypoint (HS2-9N8ZMD); it records the existing ESLint, catalog, and
UI doctor order and first-failure behavior. Doctor
JSON supplies catalog, TypeScript, ESLint, analyzer, and optional browser stage
outcomes. The doctor report does not provide per-stage wall time, so that duration
remains unknown. The entrypoint and its static policy assertions passed the real
lint command and exact-tree gate; covering main CI supplies full browser regression.

CI uploads available nextest/Vitest reports on success or failure as
`gate-timing-reports`, with run and attempt metadata, and uploads the merged
Playwright JSON when available as `browser-timing-report`. Download artifacts
from one run into one directory. The GitHub jobs API adds per-step start/end,
outcome, and skipped steps without copying raw logs:

```sh
gh run download <run-id> -n gate-timing-reports -D target/ci-timings
gh run download <run-id> -n browser-timing-report -D target/ci-timings
gh api 'repos/Small-Tale/hotsheet2/actions/runs/<run-id>/jobs?per_page=100' > target/ci-timings/jobs.json
scripts/check.sh --import-ci-timings target/ci-timings
```

The import accepts `nextest-junit.xml`, `vitest-report.json`,
`playwright-report.json`, `manifest.json`, and optional `jobs.json`. Failed
reports contribute outcome observations but never success-cost estimates.
Run/attempt/job identities deduplicate repeated imports while preserving a new CI
retry as another attempt. Local samples take precedence over CI samples; CI
estimates are a different-host seed and must be scaled or distrusted by any
budget planner. Import and `--timings` never create gate receipts or change test
selection. A missing report or step duration remains unknown rather than zero.

## Periodic gate review (HS2-6QEZB0)

After importing the latest CI artifacts, run the headless review from any checkout:

```sh
HS2_GIT_COMMON=$(git rev-parse --git-common-dir)
node scripts/gate-review.mjs --history "$HS2_GIT_COMMON/hotsheet-gate-timings.json"
node scripts/gate-review.mjs --history "$HS2_GIT_COMMON/hotsheet-gate-timings.json" --format json --source ci --sort failures
```

`--os`, `--cohort`, and `--area` select comparable slices; `--sort cost|failures|name`
orders the hierarchy, and `--period-days N` compares the latest two N-day windows
anchored to the newest observation in each cohort. The JSON export contains
area → stage → file → hashed case identifiers, pass/fail/skip/reused/interrupted
counts, selected/deferred denominators, safe failure categories, observed CI
retry and fail-then-pass counts, p50/p75/p90, sample counts, and observed compute
time. File and case compute durations may overlap; never add them as gate wall time.
Nextest case identities name a binary and test, without a reliable case-to-source-file
link in the ledger. The report groups them under `unknown-source-file`; separately
mapped Rust file observations remain visible without a fabricated join.

Review the report weekly after a covering CI run, while the relevant observations
are still inside the rolling 20-attempt ledger. Keep any exported snapshot local and
delete it when the review is done; do not commit identifiers or machine paths.
Fewer than five observations in a cohort are marked sparse, and period trends need
at least two observations in each period to be comparable. These selected local
worktree and CI attempts are not a representative failure-rate sample. The ledger
does not identify runner concurrency or warm/cold state; both remain unknown,
while receipt reuse is explicit. A fail-then-pass observation is not a population
flake rate or proof that a check found a product defect. Hashed stage labels without
a safe command mapping remain unknown. Attempts before the observed `lint:web`
entrypoint ran have no lint substage observations.

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
