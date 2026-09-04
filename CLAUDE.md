<!-- hotsheet:begin section=ticket-driven-work v=3 -->
## Ticket-Driven Work

When the user gives you work directly (not via the Hot Sheet channel or events), create Hot Sheet tickets before starting implementation — especially for substantial or multi-step work.

- **Do create tickets** for: features, bug fixes, refactoring, multi-step tasks, anything changing code. **Don't** for: simple questions, git commits, quick lookups, trivial one-liners. **When in doubt, create them.**
- Create via the Hot Sheet API (prefer the `hotsheet_*` MCP tools), mark Up Next, then work through them: set status `started` → implement → set `completed` with notes.
- **Create every follow-up immediately — without asking.** As soon as you identify unfinished steps, open design questions, known gaps, out-of-scope work, or designed-but-unbuilt behavior, create a follow-up ticket. Do not ask permission, wait for confirmation, promise to file it later, or leave it only in a comment/TODO/note. Reference every follow-up slug in the current ticket's completing note, then continue.
- **FEEDBACK NEEDED is for a blocker on the current ticket, not deferred work.** Use it only when the current ticket cannot proceed without a user decision or unavailable external state. Leave that ticket `started`, add a `FEEDBACK NEEDED:` note with the specific decision or state required, and wait after continuing any independent work. This does not replace a follow-up: create tickets first for every independently describable gap or later step.
- **Completion checklist** — before marking a ticket `completed`: (1) finish and verify its scope; (2) update required tests, coverage, and docs; (3) scan for placeholders, TODO/FIXME comments, stubs/mock returns, documented-but-unimplemented behavior, open questions, and known gaps; (4) immediately create a follow-up for every incomplete item; (5) include the result, verification, and all follow-up slugs in the completing note.
- **Commit traceability** — every commit for ticket-driven work must include every ticket slug addressed by that commit in its commit message. This applies to both single-ticket and intentionally combined commits.
<!-- hotsheet:end section=ticket-driven-work -->

## Identity terminology

A checkout id is a readable Hot Sheet path identity (`folder-shortpathhash`), not a git
id, ticket-store id, or secret. Keep checkout↔store links many-to-many. Only server
instance data carries bearer credentials.

## Ticket-provider architecture

Ticketing is provider-neutral. The existing Markdown/git implementation is the
default and fullest-featured `git` provider, not a mandatory store for every project.
GitHub Issues, Jira, GitLab, and future providers are accessed directly as their own
authoritative systems; do not build automatic bidirectional mirrors into git. One
code project may connect multiple ticket-provider instances. Route every ticket by
its qualified `(connection_id, native_id)` identity and expose provider capabilities
so unsupported fields or operations fail explicitly.

Cross-provider ticket transfer is user-initiated `copy`/`move`, not synchronization.
Make transfers idempotent using a stable operation id plus source provenance so
concurrent collaborators and retries resolve the same destination ticket.

## Ticket activity notes

Use `activity` notes for meaningful subtask boundaries: record when an investigation
or other important subtask starts and when it finishes. Keep repeated and reversed
transitions as separate entries; they are history, not current-state fields. Ordinary
commentary remains a `regular` note.

When an AI writes an `activity` note, provide a durable `note_summary` alongside the
full Markdown body. The summary is the Timeline headline: plain text, one line,
outcome-oriented, preferably no more than 80 characters, and free of verification
inventories or implementation detail. The complete body remains visible in Notes.
Generate both in the same turn; never invoke AI while rendering or opening a ticket.
Legacy notes and providers without summary metadata use a deterministic bounded
first-line fallback.

## Active AI work

Treat a live, renewable ticket claim lease as the authoritative signal that an AI is
actively working on that ticket. `started` is durable workflow state and may remain set
while nobody is working; `claim_count` is historical retry/poison metadata. Workers
claim before active work, renew during long work, and release when they stop. Never
derive live activity from status or claim count. Self-claim workers use `claim-next`;
general orchestration and delegated agents use exact `claim <slug-or-id>`, then renew and
release that same ticket with one stable, session-specific worker id.

## Client UI stack

Use **Kerf (`kerfjs`) + Web Awesome Core** for the Tauri web UI. Kerf owns
application state, routing, lists, API resources, rendering/morphing, and delegated
event handling. Web Awesome owns reusable accessible UI primitives such as form
controls, dialogs, drawers, and menus. Build custom components only for Hot
Sheet-specific interactions that those primitives do not cover; Web Awesome Pro is
optional and requires a separate decision/license.

Never use fixed-interval, timer-based, or immediate-repeat network polling for
application state. Live updates must use WebSockets or genuine long polling: an idle
long-poll request remains pending until an event or bounded server timeout, then the
client reconnects with replay/cursor semantics and bounded failure backoff. Local-only
UI timers such as countdown rendering are allowed, but they must not issue network
requests. Cover idle request rate, timeout blocking, reconnect, and client/server
version skew in integration tests so an authentication or protocol error cannot turn
into a tight request loop.

Import only the Web Awesome components the client uses, style them primarily through
their documented theme tokens and parts, and keep application state outside custom
elements. Form controls such as `<wa-input>` emit standard host-level `input` and
`change` events—not `wa-input` or `wa-change`. Web Awesome-specific lifecycle events
remain prefixed (for example `wa-show`, `wa-hide`, `wa-after-show`, and
`wa-after-hide`) and can be handled through Kerf delegation. The validated integration
and executable browser tests live in [`spikes/kerf-webawesome/`](spikes/kerf-webawesome/);
the durable rationale is in [`docs/09-technology-decisions.md`](docs/09-technology-decisions.md).

Keep shared domain components aligned across clients and presentations. In particular,
list and column tickets use one responsive ticket-summary component contract (rather
than separate row/card behavior) that remains usable at narrow horizontal sizes.
Keep production component styles colocated with their component modules and imported
from those modules; demos may provide stage/shell styles but must not own or duplicate
the production component CSS being validated.

Treat reusable visual presentations as explicit component API variants, not
consumer-specific descendant CSS overrides. A component's UX demo must expose every
supported public variant and state that consumers rely on, including appearance and
size options, so the catalog is a complete interactive account of the component API.

UX demos may replace production data sources and external side effects with deterministic
fixtures, but they must not be the sole owners of component interaction behavior. When a
component is composed into the real app, inventory every rendered action/event from that
component and wire or deliberately capability-disable each one. An enabled control that
only works in `/ux-demo` is a production bug. Browser coverage must exercise representative
child actions through every shipped parent composition (for example TicketRow through both
TicketList and TicketBoard), not merely through the isolated demo.

Ticket text editing is autosaved with a 150 ms debounce. Do not add routine Save/Cancel
buttons for details, notes, titles, tags, blocked reasons, or similar fields. Keep the
controlled draft visible while saving, flush when focus leaves the editing surface, and
test rapid coalescing, blur, composed-editor focus moves, and post-save editing. Explicit
submission actions remain appropriate when they create a new object or complete a
workflow rather than merely persisting an edit.

Every change that can affect rendered client visuals requires a deliberate visual QA
pass in a real browser before completion. Automated DOM, accessibility, computed-style,
and geometry assertions remain necessary, but are not substitutes for looking at the
rendered result. Exercise the affected demo states and transitions at representative
wide and narrow viewport sizes; capture screenshots when they make comparison easier.
Review critically for correctness, clipping/overflow, alignment, spacing, typography,
contrast, icon rendering, responsive behavior, platform/design-system conformance,
consistency with adjacent components, and overall aesthetic appeal. Fix issues found,
rerun behavioral tests, and record the visual states/viewports inspected on the ticket.
If no usable browser is available, do not claim visual validation or complete the
visual ticket: record `FEEDBACK NEEDED`/the outstanding review and leave it open.
The web client keeps `domotion-svg` as a development dependency so its bundled Chromium
is available for reproducible local screenshots even when no interactive browser is
attached. Use that Chromium (or Playwright's browser when available) for the required
rendered review; dependency presence alone is not visual validation.

Use cursor semantics that accurately communicate the interaction under the pointer.
Clickable controls and selectable rows use `pointer`; editable text uses `text`;
disabled controls use `not-allowed`; draggable/resizable surfaces use the appropriate
grab or resize cursor; non-interactive content keeps the platform default. Apply the
rule to custom elements through their documented CSS parts, and cover representative
native and Web Awesome controls in rendered browser tests.

Use **Lucide icons** for all decorative or symbolic client iconography across web,
Tauri, SwiftUI, and later clients. Never substitute emoji, Unicode geometric shapes,
dingbats, or other font characters (for example `◇`, `✓`, or `●`) as icons. Render
official Lucide assets through a shared icon component; do not copy SVG path markup
into feature components. Decorative icons are hidden from assistive technology when
adjacent text carries the meaning; icon-only controls require an accessible name.
Literal characters in user-authored content, code, or text whose actual content is
the character are unaffected. If several Lucide icons are materially good semantic
choices, ask the maintainer which metaphor to standardize before committing one.
Every actionable context-menu item should carry a meaningful Lucide icon; separators
and other non-action structure are the only ordinary exception.

<!-- hotsheet:begin section=testing-philosophy v=2 -->
## Testing Philosophy

- **Double coverage**: every feature covered by both unit tests AND E2E tests. Unit = logic in isolation; E2E = real user flows through the running app with minimal mocking.
- **Unit tests**: Mock external deps (filesystem, network), test real logic.
- **E2E tests**: As much as possible, use test automation tools to run realistic, user-facing flows. Minimize mocks.
- **Mock the exact transport contract**: browser fixtures must use the server's real wire
  shape, including flattening/envelopes, optional fields, and status codes. Do not invent a
  more convenient response shape for a client test. For each newly composed real API
  surface, run at least one integration or opt-in local-browser flow against the actual
  server; a mocked UI test alone cannot validate the adapter boundary.
- **Coverage**: Merge all test coverage (e.g. unit, E2E server, E2E browser) into one report. Low-coverage files should get more of both test types. Aim for 100% coverage of code lines, 100% coverage of branches, and 100% of features described in the requirements documentation.
- **Coverage is a floor, not a ceiling**: 100% line/branch coverage shows every line *ran*, not that every *behavior* — or every *sequence* of behaviors — is *asserted*. It is structurally blind to a **missing state transition**: a bug living in an untested interaction sails through a green 100% report because the individual lines still get hit by isolated, single-operation tests.
- **Transition-matrix testing for stateful modules**: for anything with modes / multiple code paths / a cache / a state machine, enumerate the states AND the transitions between them, then write tests that walk realistic multi-step sequences crossing state boundaries — not just each operation from a clean initial state.
- **Adversarial pass on stateful changes**: when adding or altering a stateful code path, deliberately try to break it with out-of-order / interleaved / repeated / empty-then-refill sequences; pin any that would have failed as permanent regression tests.
- **Client state synchronization is bidirectional**: for every stateful control, test
  both control → application state/rendered output and programmatic application state
  → the live control. A stateful client interaction test must establish the initial
  state, change every exposed setting, verify output, reset/replace state, verify every
  control and output again, then make another edit after reset. For custom elements,
  assert live properties such as `value` and `checked` (plus focus and emitted events
  when relevant), not attributes alone. Exercise every rendered action such as Reset,
  Save, Cancel, Apply, Remove, and Undo. Prefer one tested binding/synchronization
  abstraction over per-component repair code.
- **Assert the complete selected presentation after transitions**: when a composite
  control renders a label plus icon, color, badge, checkmark, count, or other derived
  decoration, changing its value must assert every visible facet in the closed/current
  state—not only the label, initial render, or popup options. Exercise the transition
  through the real owning surface (for example an inspector), then verify stale
  decorations disappeared and the new label and decorations agree with application
  state. A menu containing correct icons does not prove its selected-value projection.
- **Exercise child actions through every shipped composition**: a component demo proving
  an action works does not prove a parent composition projects the changed state back
  into that child. For each interactive child used by a composite surface, trigger its
  real action through the composite, assert the owning state changed, and assert the
  composite rerendered the correct child view. Never hardcode a controlled child prop
  in a composition when a shared signal/controller owns that state.
- **Manual test plan**: keep a manual test plan doc (e.g. `docs/manual-test-plan.md`) for features that can't be reliably automated. **Keep it up to date** — add such features there; when you add automated coverage for a previously-manual item, remove it and note it in an "Automated Coverage Summary".
- **Feature coverage matrix**: update [`docs/TEST-COVERAGE.md`](docs/TEST-COVERAGE.md)
  in the same change whenever a feature is added, shipped, changed, deferred, or gains
  or loses unit, E2E, or manual coverage. `node scripts/check-test-coverage.mjs` is the
  CI gate for valid statuses and live evidence paths; line coverage is not a substitute
  for recording both behavioral layers.
- **Always fix lint and type errors before finishing**: Fix as you go, don't batch.
  Every client, server, tool, spike, and other code package must ship with a real
  lint configuration and a package-local lint command from the moment code is added.
  The web/TypeScript baseline is the shared Glassbox ESLint stack (ESLint recommended,
  typed TypeScript rules, import ordering, TSDoc, and Kerf rules); Rust uses the pinned
  toolchain's `rustfmt` and `cargo lint` alias (Clippy for the workspace/all targets/all
  features, warnings plus debug/TODO/unimplemented macros denied). Do not push with a
  lint warning or error. Suppress a rule only at a documented compatibility or external
  boundary—not merely to make the command green—and tighten transitional exceptions as
  touched code is made safe.

<!-- hotsheet:begin specifics=testing-philosophy v=1 -->
### This project's test setup

> **Early implementation.** The stack below is the *agreed plan* (see
> [`docs/12-code-organization-and-testing.md`](docs/12-code-organization-and-testing.md) §12.7,
> the authority). What exists today: `cargo nextest run` (model + ticketing + CLI
> unit/integration tests) and the migrator's `vitest` suite (`cd migrator && npx
> vitest run`), including the cross-language conformance test (Rust `hotsheet import`
> ingests the Node exporter's JSON). **Property tests** cover the parser (`proptest`:
> round-trip + byte-idempotent + never-panics), and a **cargo-fuzz** target exists
> (`crates/hotsheet-model/fuzz`, nightly: `cargo +nightly fuzz run parse_file`).
> **Per-language coverage gates** are wired for Rust (CI `cargo llvm-cov` with a
> `--fail-under-lines` floor) and the migrator (`npm run test:coverage`, thresholds in
> `migrator/vitest.config.mjs`); a creds-gated **live tier** (`.github/workflows/live.yml`)
> runs the `#[ignore]` codex/claude turns nightly. Server E2E is wired
> (`crates/hotsheet-server/tests/http.rs` — in-process HTTP/WS against a temp store);
> snapshot tests and **web** (Playwright) E2E are not wired yet. Commands that work now:
> `cargo build` · `cargo nextest run` · `cargo fmt --all --check` · `cargo clippy
> --all-targets --all-features -- -D warnings` · `npx vitest run` / `npm run test:coverage`
> (in `migrator/`).

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
- **Stateful clients** get bidirectional binding contract tests: controls → state and
  rendered output, then programmatic reset/replacement → live control properties and
  output, followed by another edit. Browser tests must exercise every user-visible
  action and inspect custom-element properties rather than relying on attributes.
- **Coverage:** per-language gates + the feature-layer matrix in
  `docs/TEST-COVERAGE.md` (NOT one merged lcov):
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
  format, filesystem stores, CLI, server, SQLite/FTS index, automatic sync, MCP shim,
  AI-tool plugins, and terminal/permission infrastructure exist; clients remain
  design-only. See [`docs/README.md`](docs/README.md) for the index + core bets and
  [`docs/CODEBASE-MAP.md`](docs/CODEBASE-MAP.md) for what's built.
<!-- hotsheet:end specifics=requirements-documentation -->
<!-- hotsheet:end section=requirements-documentation -->

## Commit / push hygiene

Keep the repo in a **known-good state**. After each completed ticket (or other
coherent user-requested unit), finish its documentation and ticket notes, run the
required gates, commit it, and push it **before starting the next ticket**. Do not
accumulate completed tickets as uncommitted or unpushed work.

For each ticket:
1. Implement the coherent ticket-sized change and update its docs and coverage matrix.
2. **Lint and fix** every affected package — `cargo fmt --all --check` + `cargo lint`
   for Rust; `npm run lint` in affected TypeScript packages.
3. Run the affected unit, integration, browser, and visual tests. Use the full local
   suite for shared/risky changes and always before publishing an accumulated recovery
   batch (per [`docs/12-code-organization-and-testing.md`](docs/12-code-organization-and-testing.md)
   §12.7).
4. Mark the ticket completed with its verification note, review the diff, make **one
   commit for that ticket**, and push immediately. Combine tickets in one commit only
   when their implementations overlap so strongly that separating them would be unsafe
   or misleading, or when the tickets are duplicates of the same work.
5. Confirm the push succeeded and the worktree is clean before taking the next ticket.

If pre-existing changes have accumulated, separate them into one commit per ticket
wherever attribution remains safe. Use a combined recovery commit only for strongly
overlapping or duplicate tickets whose changes cannot be separated without rewriting
or risking completed work. CI may still surface issues from heavier CI-only tests, but
the local environment must be green before every push. **Exception:** when deliberately
testing CI itself, a red-ish push may be intentional, but then do not push to `main`.

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

<!-- BEGIN hotsheet:claude -->
## Hot Sheet — ticket workflow

This project tracks work as **Hot Sheet** tickets (plain files under the store). Use
them to know what to do next and to record what you did. Everything below works
**headless** — no app, and no server required.

**Find and plan the complete queue:**
- `hotsheet-cli ls --up-next` — the prioritized Up Next queue.
- `hotsheet-cli show <slug>` — read one ticket in full (e.g. `hotsheet-cli show HS-7F3K9Q`).
- Or the MCP tools: `hotsheet_query` (with `up_next: true`) and `hotsheet_get`.

**Do the work, and record progress on the ticket as you go:**
- `hotsheet-cli edit <slug> --status started` when you begin.
- `hotsheet-cli edit <slug> --status completed --note "what you did"` when done.
- Or `hotsheet_update` (it takes a `note`) / `hotsheet_close` (same effect through MCP).

**Create tickets for new work you discover** (bugs, follow-ups, gaps) rather than
leaving them in comments:
- `hotsheet-cli new --title "…" --category bug` — or the `hotsheet_create` MCP tool.

Normally continue until every actionable Up Next ticket is complete. Read the whole queue
before choosing an order; consider dependencies, overlap, shared context, risk, and safe
parallelization. Treat priority as important guidance rather than a hard ordering rule.
The CLI and MCP tools use the same engine, so use whichever is handier.
<!-- END hotsheet:claude -->
