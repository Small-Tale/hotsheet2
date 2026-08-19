<!-- hotsheet:begin section=ticket-driven-work v=1 -->
## Ticket-Driven Work

When the user gives you work directly (not via the Hot Sheet channel or events), create Hot Sheet tickets before starting implementation — especially for substantial or multi-step work.

- **Do create tickets** for: features, bug fixes, refactoring, multi-step tasks, anything changing code. **Don't** for: simple questions, git commits, quick lookups, trivial one-liners. **When in doubt, create them.**
- Create via the Hot Sheet API (prefer the `hotsheet_*` MCP tools), mark Up Next, then work through them: set status `started` → implement → set `completed` with notes.
- **Always create follow-up tickets** for incomplete work (unfinished steps, open design questions, known gaps, designed-but-unbuilt features). If it's not in a ticket, it's forgotten.
- **Incomplete-work checklist** — before marking a ticket `completed`, file follow-ups for any: (1) UI placeholder text ("coming soon"), (2) TODO/FIXME comments, (3) documented-but-unimplemented requirements, (4) empty/stub functions returning mock data.
- **Use FEEDBACK NEEDED before deferring or asking about follow-ups.** When about to (a) defer a ticket needing more work, (b) ask whether to file follow-ups, or (c) close with a question buried in notes — DON'T. Leave the ticket `started`, add a `FEEDBACK NEEDED:` note (per `.hotsheet/worklist.md`), signal channel done, and wait. It's the only reliable way to surface a question.
<!-- hotsheet:end section=ticket-driven-work -->

<!-- hotsheet:begin section=testing-philosophy v=2 -->
## Testing Philosophy

- **Double coverage**: every feature covered by both unit tests AND E2E tests. Unit = logic in isolation; E2E = real user flows through the running app with minimal mocking.
- **Unit tests**: Mock external deps (filesystem, network), test real logic.
- **E2E tests**: As much as possible, use test automation tools to run realistic, user-facing flows. Minimize mocks.
- **Coverage**: Merge all test coverage (e.g. unit, E2E server, E2E browser) into one report. Low-coverage files should get more of both test types. Aim for 100% coverage of code lines, 100% coverage of branches, and 100% of features described in the requirements documentation.
- **Coverage is a floor, not a ceiling**: 100% line/branch coverage shows every line *ran*, not that every *behavior* — or every *sequence* of behaviors — is *asserted*. It is structurally blind to a **missing state transition**: a bug living in an untested interaction sails through a green 100% report because the individual lines still get hit by isolated, single-operation tests.
- **Transition-matrix testing for stateful modules**: for anything with modes / multiple code paths / a cache / a state machine, enumerate the states AND the transitions between them, then write tests that walk realistic multi-step sequences crossing state boundaries — not just each operation from a clean initial state.
- **Adversarial pass on stateful changes**: when adding or altering a stateful code path, deliberately try to break it with out-of-order / interleaved / repeated / empty-then-refill sequences; pin any that would have failed as permanent regression tests.
- **Manual test plan**: keep a manual test plan doc (e.g. `docs/manual-test-plan.md`) for features that can't be reliably automated. **Keep it up to date** — add such features there; when you add automated coverage for a previously-manual item, remove it and note it in an "Automated Coverage Summary".
- **Always fix lint and type errors before finishing**: Fix as you go, don't batch.

<!-- hotsheet:begin specifics=testing-philosophy v=1 -->
### This project's test setup

> **Early implementation.** The stack below is the *agreed plan* (see
> [`docs/12-code-organization-and-testing.md`](docs/12-code-organization-and-testing.md) §12.7,
> the authority). What exists today: `cargo nextest run` (model + ticketing + CLI
> unit/integration tests) and the migrator's `vitest` suite (`cd migrator && npx
> vitest run`), including the cross-language conformance test (Rust `hotsheet import`
> ingests the Node exporter's JSON). Property/fuzz/snapshot, server/web E2E, and
> per-language coverage gates are not wired yet. Commands that work now:
> `cargo build` · `cargo nextest run` · `cargo fmt --all --check` · `cargo clippy
> --all-targets --all-features -- -D warnings` · `npx vitest run` (in `migrator/`).

- **Rust unit + integration** (`crates/*/src/**` inline `#[cfg(test)]` and
  `crates/*/tests/**`): run with **`cargo-nextest`**. Pure logic uses injected-fake
  adapters (in-memory fs, temp git repo, in-memory SQLite); integration uses a real
  temp store + real SQLite. **Always use the shared fixtures:** `TempStore` builder
  and the `TestServer` harness.
- **Property / fuzz / snapshot:** `proptest` for the semantic **merge driver**,
  `cargo-fuzz` for the file-format parser, `insta` for 3-way-merge snapshots. The
  **git-native claim** has deterministic bare-repo integration tests (concurrent
  workers); the GitHub-live variant is opt-in (creds-gated).
- **Server E2E:** boot the real server on an ephemeral port against a temp store,
  drive over HTTP/WS. **Web E2E** (`clients/web`): **Playwright** against a real
  running server. SwiftUI: XCUITest (later).
- **Migrator** (`migrator/`, Node): **`vitest`**, plus the cross-language
  **conformance test** — real `hotsheet-model` must parse + round-trip what the
  migrator wrote.
- **Stateful modules** (claim/lease, index reconcile, terminal-sizing arbiter, sync
  engine) get **transition-matrix + adversarial-sequence** tests; pin every stateful
  bug as a regression test.
- **Coverage:** per-language gates + an aggregate summary (NOT one merged lcov):
  `cargo llvm-cov` (Rust) · Playwright/istanbul (web) · `vitest` coverage (migrator).
- **Commands** (once code exists): unit `cargo nextest run` · web E2E
  `pnpm -C clients/web test:e2e` · migrator `pnpm -C migrator test` · coverage
  `cargo llvm-cov` (+ per-surface). Fast tier vs. full/live tier (GitHub-remote +
  creds-gated) in CI (GitHub Actions).
<!-- hotsheet:end specifics=testing-philosophy -->
<!-- hotsheet:end section=testing-philosophy -->

<!-- hotsheet:begin section=requirements-documentation v=1 -->
## Requirements Documentation

Keep human-readable requirements documents as the source of truth for what the project does, and **keep them up to date in the same change as the code** (add/remove/modify a requirement → update its doc). Create new docs for major new functional areas. Cross-reference related docs with relative links.

### AI Summaries

Maintain two synthesis docs an AI assistant reads at the start of a fresh session — keep them in sync with reality (source doc/code wins on conflict), and prefer small targeted edits over rewrites:

- A **codebase map** — directory tree, entry points, data schema, build, tests, settings, and a "where do I look for X" index. Update it in the same change when you add a file or directory, add a route/endpoint, change the schema, add a client module, or add a setting key.
- A **requirements summary** — a synthesized view of every requirements doc with status markers (e.g. Shipped / Partial / Design only / Deferred). Update it in the same change when you add a requirements doc, ship a design-only feature, or defer/regress a shipped one.

<!-- hotsheet:begin specifics=requirements-documentation v=1 -->
### This project's docs layout

- **Requirements docs** live in [`docs/`](docs/), numbered by topic (`00-…`, `01-…`,
  … `11-…`, growing). Cross-reference related docs with relative links.
- **Requirements summary** (the AI-read synthesis with status markers): the
  "Requirements summary" table in [`docs/README.md`](docs/README.md). Keep it in
  sync in the same change as a design/code change.
- **Codebase map:** [`docs/CODEBASE-MAP.md`](docs/CODEBASE-MAP.md) — the AI-read
  orientation doc (directory tree, entry points, formats, build/test, where-to-look).
  Keep it in sync in the same change that adds a file/dir, command, schema field, or
  setting.
- The project is now in **early implementation**: the Rust core model + ticket file
  format, a filesystem store, the `hotsheet` CLI (init/new/ls/show/import), and the
  Node HS1 exporter exist; server/index/sync/clients/plugins remain design-only. See
  [`docs/README.md`](docs/README.md) for the index + core bets and
  [`docs/CODEBASE-MAP.md`](docs/CODEBASE-MAP.md) for what's built.
<!-- hotsheet:end specifics=requirements-documentation -->
<!-- hotsheet:end section=requirements-documentation -->

## Commit / push hygiene

Keep the repo in a **known-good state**, and especially so **before pushing**. The
working rhythm (once there is code to lint/test):

**During work — repeatable per round (multiple local commits, no push):**
1. Do a chunk of work (e.g. a ticket).
2. **Lint and fix** — `cargo fmt` + `cargo clippy -D warnings` for Rust; the
   client/migrator linters for their trees.
3. Run the **light/fast tests and fix** — e.g. `cargo nextest run` for the affected
   crates.

**Before pushing:**
1. **Final lint pass** (fix everything).
2. **Full test suite** (fix everything) — the whole `nextest` run + web E2E + the
   conformance/fake-agent suites + the migrator conformance test (per
   [`docs/12-code-organization-and-testing.md`](docs/12-code-organization-and-testing.md) §12.7).
3. **Push** (only when the maintainer has asked / agreed to push).

You may do several work rounds with multiple commits before pushing; once you push,
**local must be green**. CI may still surface issues from heavier CI-only tests or
from merging multiple changes together — that's expected — but the local environment
should be verified good before a push. **Exception:** when deliberately testing CI
itself, a red-ish push may be intentional — but then don't push to `main`.

(While the repo is still design-only, "lint/test" is a no-op for docs changes;
this rhythm applies once implementation code exists.)

## Project attribution

Hot Sheet 2 is developed by **Small Tale Inc.** and lives under the **`Small-Tale`**
GitHub org (`Small-Tale/hotsheet2`) — **not** under an individual (e.g. not "Brian
Westphal"). Whenever the project is named, authored, or copyrighted, use **Small Tale
Inc.**:

- Package manifests (`package.json` `author`/`publisher`, `Cargo.toml`
  `authors`/`publish`, Xcode/Gradle org identifiers) → **Small Tale Inc.**
- `LICENSE` copyright holder → **Small Tale Inc.** (year(s) as applicable).
- User-facing "about"/credits strings, docs bylines, and app bundle identifiers →
  **Small Tale Inc.** (bundle id under a `com.smalltale.*` / similar namespace).
- Individual developers still appear as normal git commit authors; that's separate
  from how the *project* is attributed.

References to the *original* Hot Sheet (the predecessor at
`github.com/brianwestphal/hotsheet`) are historical/factual and may remain as
predecessor links; they do not attribute Hot Sheet 2.
