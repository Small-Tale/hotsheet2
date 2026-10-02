---
name: analyze-code-quality
description: Run every Hot Sheet 2 test suite, linter, and gate across the Rust CLI/server crates, the web client, and the migrator, check for documented anti-patterns, and produce a code quality report
allowed-tools: Read, Grep, Glob, Bash, Agent
---

> **Thresholds, gates, and file lists live in their config files, not here.** CI
> (`.github/workflows/ci.yml`) owns the Rust coverage floor, `migrator/vitest.config.mjs` owns
> the migrator thresholds, `.cargo/config.toml` owns the `cargo lint` deny list,
> `clients/web/package.json` owns the web scripts, and `CLAUDE.md` owns the conventions. Read
> the source of truth; treat any figure below as a pointer to where to look.

Analyze the overall quality of Hot Sheet 2 and produce one report covering every surface:

| Surface            | Where                                                                                                                                                                       | Gates                                                                            |
| ------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| Core + CLI         | `crates/hotsheet-model`, `hotsheet-ticketing`, `hotsheet-index`, `hotsheet-cli`, `hotsheet-extsync`, `hotsheet-aitools`, `hotsheet-plugins`, `hotsheet-mcp`, `hotsheet-tls` | fmt, clippy, nextest, llvm-cov                                                   |
| Server + terminals | `crates/hotsheet-server` (HTTP/WS, `tests/http.rs`), `crates/hotsheet-terminals`                                                                                            | same Rust gates, plus server E2E                                                 |
| Web client         | `clients/web` (Kerf + Web Awesome)                                                                                                                                          | lint (incl. `css:ownership`, catalog), `tsc`, vitest, Playwright, Kerf UI doctor |
| Migrator           | `migrator/` (Node)                                                                                                                                                          | lint, vitest coverage, cross-language conformance                                |

Confirm the crate list with `ls crates` instead of trusting this table.

The goal is not "does it compile and are the lines covered" but "is the code correct in the
sequences it actually runs". A green suite at full line coverage routinely ships behavioral
bugs (see step 7). Weight the report accordingly.

## Steps

Run independent suites in parallel where the machine allows. Playwright and some vitest
end-to-end files spawn processes or browsers that fail inside a command sandbox; rerun
those unsandboxed before treating a failure as a regression (CLAUDE.md, memory notes).

1. **Rust format, lint, and tests** (CLI, server, and every other crate)

   ```
   cargo fmt --all --check
   cargo lint
   CARGO_INCREMENTAL=0 cargo nextest run --workspace --all-features
   ```

   Report: format drift, clippy findings grouped by lint, and per-crate pass/fail counts.
   The `#[ignore]` live tier (`.github/workflows/live.yml`) is creds-gated; do not run it
   unless asked, and do not report it as missing.

2. **Rust coverage**

   ```
   cargo llvm-cov nextest --workspace --all-features --summary-only
   ```

   Compare against the `--fail-under-lines` floor in `.github/workflows/ci.yml` (read it;
   don't assume). Report per-crate line coverage and the lowest-covered files in
   `hotsheet-server` and `hotsheet-cli` specifically.

3. **Web client gates** (from `clients/web`)

   ```
   npm run lint          # format check, ESLint --max-warnings 0, css:ownership, catalog:check
   npx tsc --noEmit -p .
   npx vitest run
   npm run ui:doctor
   ```

   Judge `npm run lint` by its exit status, not by grepping output. Report: lint findings
   by rule, type errors by file, unit pass/fail, Kerf UI doctor counts, and the size of
   `css-ownership-allowlist.json` (each entry must name a ticket; it may only shrink).

4. **Web end-to-end** (from `clients/web`)

   ```
   HOTSHEET_WEB_TEST_PORT=<free port> npx playwright test
   ```

   Real-server specs need `HOTSHEET_TEST_CLI_BIN` / `HOTSHEET_TEST_SERVER_BIN` pointing at
   `target/debug` binaries. Report pass/fail/flaky. For any failure, rerun that test alone
   (and with `--repeat-each`) before calling it a regression; a test that fails only under
   load is a flake to fix, and its root cause is a finding.

5. **Migrator** (from `migrator/`)

   ```
   npm run lint
   npm run test:coverage
   ```

   Report pass/fail, coverage against `migrator/vitest.config.mjs`, and whether the
   cross-language conformance test (Rust `hotsheet import` ingesting the exporter's JSON)
   ran and passed.

6. **Feature coverage matrix**

   ```
   node scripts/check-test-coverage.mjs
   ```

   Then read `docs/TEST-COVERAGE.md`. List features marked `unit-only`, `e2e-only`, or
   manual-only (CLAUDE.md requires double coverage), and cross-check
   `docs/manual-test-plan.md` for items that are now automatable.

7. **Anti-patterns documented in CLAUDE.md and the docs**

   Read `CLAUDE.md`, `docs/12-code-organization-and-testing.md`, and `docs/ux-components.md`
   first; they are authoritative and evolve. Prefer **ast-grep** for structural checks
   (`ast-grep run --lang <rust|ts|tsx> -p '<pattern>' <path>`) over text grep. Check every
   surface:

   - **CLI and server (Rust)**
     - `unwrap()` / `expect()` / `panic!` on request, file, or network paths in `hotsheet-server`
       and `hotsheet-cli` (tests excepted).
     - Lint suppressions (`#[allow(...)]`) without a stated compatibility reason.
     - Headless parity: a project bootstrap, recovery, source-linking, skill-install, or
       MCP/AI setup capability reachable only from the web client, with no `hotsheet-cli`
       path (CLAUDE.md, "Headless setup parity").
     - Provider neutrality: ticket routing that does not use the qualified
       `(connection_id, native_id)` identity, or a provider operation that silently no-ops
       instead of failing explicitly through capabilities.
     - Live activity derived from `status` or `claim_count` instead of the claim lease.
     - Shell strings built from request input (commands must execute typed definitions only;
       see `hotsheet-ticketing/src/commands.rs`).
   - **Server to client contract**
     - Fixed-interval, timer, or immediate-repeat network polling for application state.
       Live updates must use WebSockets or genuine long polling with bounded backoff.
     - Browser test fixtures whose JSON shape differs from the real server's wire shape.
   - **Web client**
     - Application CSS that styles Kerf components (`.kui-*`, `[data-component]`, Kerf
       `::part`s) or another component's markup. `css:ownership` and the doctor catch most of
       this; review each allowlist and doctor suppression for a still-open ticket.
     - Icons that are not Lucide through the shared icon component (emoji, Unicode shapes).
     - Text editors with Save/Cancel buttons, or writes on every keystroke, instead of the
       blur/page-hide autosave contract.
     - `delegate()` disposers discarded on non-page-lifetime roots; `addEventListener` inside
       a Kerf-mounted tree.
     - Controls that work in `/ux-demo` but are unwired or not capability-disabled in the
       real app.
   - **Everywhere**
     - Files holding more than one concern (judge by concern count, not raw length; rank with
       `wc -l`).
     - Duplicate code across modules or across the CLI and server for the same workflow.
     - Developer-specific absolute paths in docs, tickets, or code.

8. **Behavioral / state-transition audit** (the step coverage cannot do for you)

   Coverage proves every line ran, not that every behavior or sequence is asserted. A bug
   living in an untested transition sails through a green report.

   1. **Identify the stateful modules.** Look for internal modes or phases, state machines or
      lifecycles, caches with a miss path, leases or debounces, "first vs subsequent" or
      "empty vs populated" branching, and accumulated mutable state. CLAUDE.md names the
      canonical ones: claim/lease, index reconcile, the terminal-sizing arbiter, and the sync
      engine. Also consider the terminal broker lifecycle, command runs, the live-update
      replay/cursor channel, client autosave drafts and recovery, and the workbench and
      drawer state. Derive the real set from the code; this list is not exhaustive.
   2. **For each, enumerate states and transitions**, then check whether tests walk
      multi-step sequences that cross state boundaries rather than single operations from a
      clean fixture.
   3. **Flag any stateful module tested only single-operation-from-clean-state**, and
      recommend a transition-matrix test with concrete sequences:
      - out-of-order (release before claim, resize before attach);
      - interleaved (two workers racing a lease, a sync arriving mid-write);
      - repeated (double init, re-claim, double save);
      - empty-then-refill;
      - stale or expired (acting after a lease or debounce window).

## Report Format

- **Summary**: overall health per surface (CLI and core crates, server, web client,
  migrator), plus a one-line behavioral-risk verdict.
- **Test Results**: Rust per crate, server E2E, web unit, Playwright, migrator. Note which
  failures reproduce alone and which are load flakes.
- **Coverage**: Rust per crate against the CI floor, migrator against its thresholds.
  **State that coverage is a floor, not a ceiling.**
- **Lint / Type / Doctor Issues**: grouped by rule and surface; allowlist and suppression
  inventory with their tickets.
- **Feature Coverage Matrix**: single-layer features and manual-only items.
- **Anti-Pattern Violations**: file and line, severity (high / medium / low), and a one-line
  fix suggestion each.
- **Behavioral / State-Transition Assessment**: per stateful module, its states, whether
  transitions are tested, and the transition-matrix sequences to add. Required even at full
  coverage.
- **Recommendations**: prioritized, with behavioral and cross-surface gaps above cosmetic
  ones. File Hot Sheet tickets for every non-trivial finding (`hs-bug` for defects,
  `hs-task` for cleanups) and list their slugs.
