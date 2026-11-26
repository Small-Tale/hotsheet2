# lint:web cost (2026-11-26, HS2-N3XBS3)

`npm run lint:web` in `clients/web` runs ESLint, the Kerf catalog check, and the Kerf UI
doctor in that order. It had a local p75 of about 424 s, which made it the largest part
of the CI First floor for any web change. This note records where that time went, what
changed, and what was left to Kerf. The behavior is described in
[29 §lint:web cost](29-affected-local-verification.md#lintweb-cost-hs2-n3xbs3).

## Method

All runs used one macOS arm64 machine, Node 22, `@kerfjs/ui` and `eslint-plugin-kerfjs`
5.0.0-beta.135, about 1,590 lintable files, and synthetic edits only. Other agents shared
the machine, and the load average moved between 4 and 100 during the session. Wall times
are therefore noisy. Each table row gives the one-minute load average at its start, and
only rows taken at similar load are compared. Each `lint:web` run called
`runLintWeb()` from the committed script (before) or the changed script (after), with a
scratch history file. `/usr/bin/time` gave CPU time. The "one-file CSS change" appends a
different comment to `src/components/ticket-row.css` before each run, so every run misses
the doctor's whole-report cache.

## Where the time went (before)

| Step or stage                        | Time                                   |
| ------------------------------------ | -------------------------------------- |
| ESLint (typed, `--max-warnings 0`)   | 64–71 s (ledger p75 85 s)              |
| Catalog check                        | under 1 s                              |
| Doctor `--full`, cold                | 282 s at load 4 (ledger p75 440 s)     |
| Doctor, warm and unchanged           | 0.5 s (cache hit)                      |
| Doctor stage: catalog                | 2 s                                    |
| Doctor stage: TypeScript             | 28 s wall, 34 s CPU at load 19         |
| Doctor stage: ESLint (Kerf preset)   | 279 s CPU, 578 s wall under heavy load |
| Doctor stage: analyzer               | 110 s wall under heavy load            |
| `tsc --noEmit` (`npm run typecheck`) | 23 s wall, 33 s CPU at load 19         |

The ledger p75 for the whole `lint:web` stage was 423.6 s over 20 local gate samples
(p50 290 s, p90 465 s). The per-step p75 values come from the 7 `lint-web` attempts
recorded since HS2-9N8ZMD.

The ESLint `TIMING` top rules were `no-misused-promises` (8.7 s), `no-floating-promises`
(7.1 s), `no-unused-vars` (5.0 s), `no-deprecated` (3.7 s), and `no-unsafe-assignment`
(3.2 s). Rules took about 43 s in total. The rest of the step is parsing and building the
type program.

A CPU profile of the doctor's ESLint stage put 74% of its samples in the
`kerfjs/ui-wiring` rule's `Program:exit`. For every linted file, that rule re-reads and
re-resolves the whole entry module graph from disk, which is quadratic in the number of
files. The doctor also adds one projected core-rule config per file (1,592 configs with
6 distinct contents). Grouping them saved about 14% of that stage's CPU in an experiment.
Both problems are in Kerf code and are filed as KF-3J0GC6. Hot Sheet 2 does not patch or
work around them.

## Duplicated work

- **ESLint does not run twice.** The doctor's ESLint stage is an isolated, untyped pass
  with the Kerf `recommended-ui` preset plus only the core rules projected from our
  config. The direct pass runs typed `typescript-eslint` and `kerfjs.configs.recommended`
  rules, with no `ui-*` rules. They overlap only on core rules, and each enforces rules
  the other cannot.
- **TypeScript ran twice.** The doctor's TypeScript stage builds the same `tsconfig.json`
  program as `npm run typecheck`, minus gitignored files, and every runner of `lint:web`
  also runs `npm run typecheck`. The `lint:web` doctor run now turns off only that stage.
- **ESLint `--cache`** is not used. Type-aware rules can change their result for an
  unchanged file when a file it imports changes, so a per-file cache could miss a
  finding. A whole-pass content key is used instead.
- **Doctor `--changed`** narrows TypeScript and ESLint findings to the changed files, and
  Kerf documents `--full` for gates. It is not used.

## Results

| Scenario (load at start)                            | Before                                    | After                                                            |
| --------------------------------------------------- | ----------------------------------------- | ---------------------------------------------------------------- |
| One-file CSS change, matched pair (22 / 21)         | 456 s: ESLint 70 s, doctor 386 s          | 311 s: ESLint reused 1.8 s, doctor 308 s                         |
| Warm, nothing changed (8 / 59)                      | 70 s: ESLint 69 s, doctor cache hit 0.5 s | 12 s: ESLint reused 5.8 s, catalog 2 s, doctor 4 s               |
| Full cold, every cache empty (4 / 8, rising to 100) | 347 s: ESLint 64 s, doctor 282 s          | 1,119 s: ESLint 68 s, doctor 1,050 s (not comparable, see below) |

- A one-file CSS change no longer runs ESLint, because CSS is not an ESLint input. The
  doctor saves the TypeScript stage, measured alone at 28 s. The rest of the 78 s
  difference between the two doctor runs is load noise.
- For an unchanged tree, ESLint is reused and the doctor hits its own cache. Hashing the
  inputs took 1.8 s at load 21 and 5.8 s at load 59.
- A change to any `.ts`, `.tsx`, `.mjs`, JSON, Markdown, or configuration file under
  `clients/web` still runs ESLint in full, then the doctor without TypeScript. The
  expected full-cold result is the before figure minus the TypeScript stage, about 320 s
  at low load. The after full-cold run happened at load 100 (1,119 s wall), so it is not
  reported as a comparison.
- CI is unchanged except for the duplicated doctor TypeScript stage. `CI` disables reuse,
  CI checkouts have no `target/` record, and the CI web step still runs `npm run
typecheck`.

The after p75 for the `lint:web` gate stage needs new samples in the rolling ledger. Read
it with `scripts/check.sh --timings` once about ten gated web changes have run.

## What remains

The doctor's ESLint stage is now most of `lint:web` for a CSS change. KF-3J0GC6 asks Kerf
to build the `ui-wiring` entry graph once per lint run. With that fixed, the doctor's cost
for a cache miss would be roughly its TypeScript-free remainder: catalog, the preset's
other rules, and the analyzer.

Running ESLint and the doctor at the same time would hide the shorter one on a
multi-core host. That was not done: it changes the documented first-failure order, and
the two passes together need more memory than the 7 GB CI runners guarantee (ESLint alone
peaks near 3.3 GB, HS2-3TWWWD).

## Kerf 5.0.0-beta.136 (HS2-6H9P37)

KF-3J0GC6 is completed in Kerf, but the fix is not in beta.136. That release was published
at 2026-11-25 12:59 UTC, and the KF-3J0GC6 fix landed on Kerf `main` later that day. The
beta.136 `doctor/` directory is byte-identical to beta.135, and `eslint-plugin-kerfjs`
changed only its version string. Two cold `lint:web` runs on the same tree, one before the
upgrade and one after, used `HOTSHEET_LINT_WEB_REUSE=0` and an empty `.kerf-cache`. They
confirm that nothing changed:

| Run (load at start, at end) | Wall    | ESLint | Doctor  | CPU (user + sys) |
| --------------------------- | ------- | ------ | ------- | ---------------- |
| beta.135, before (22, 17)   | 435.7 s | 72.0 s | 362.9 s | 306 s + 91 s     |
| beta.136, after (20, 85)    | 445.5 s | 67.5 s | 377.2 s | 311 s + 97 s     |

The difference is within load noise. Re-measure when Hot Sheet upgrades to the first Kerf
release that contains KF-3J0GC6.
