<!-- hotsheet:begin section=ticket-driven-work v=3 -->

## Ticket-Driven Work

When the user gives you work directly (not via the Hot Sheet channel or events), create Hot Sheet tickets before starting implementation — especially for substantial or multi-step work.

- **Do create tickets** for: features, bug fixes, refactoring, multi-step tasks, anything changing code. **Don't** for: simple questions, git commits, quick lookups, trivial one-liners. **When in doubt, create them.**
- Create via the Hot Sheet API (prefer the `hotsheet_*` MCP tools), mark Up Next, then work through them: set status `started` → implement → set `completed` with notes.
- **Create every follow-up immediately — without asking.** As soon as you identify unfinished steps, open design questions, known gaps, out-of-scope work, or designed-but-unbuilt behavior, create a follow-up ticket. Do not ask permission, wait for confirmation, promise to file it later, or leave it only in a comment/TODO/note. Reference every follow-up slug in the current ticket's completing note, then continue.
- **FEEDBACK NEEDED is for a blocker on the current ticket, not deferred work.** Use it only when the current ticket cannot proceed without a user decision or unavailable external state. Leave that ticket `started`, add a `FEEDBACK NEEDED:` note with the specific decision or state required, and wait after continuing any independent work. This does not replace a follow-up: create tickets first for every independently describable gap or later step.
- **Completion checklist** — before marking a ticket `completed`: (1) finish and verify its scope; (2) update required tests, coverage, and docs; (3) scan for placeholders, TODO/FIXME comments, stubs/mock returns, documented-but-unimplemented behavior, open questions, and known gaps; (4) immediately create a follow-up for every incomplete item; (5) include the result, verification, and all follow-up slugs in the completing note.
- **UI evidence continuity** — when a UI ticket already contains an image demonstrating the problem or requested design, reproduce that component/state/viewport after the change, attach the resulting screenshot to the ticket, and reference it in the completion note. Do not silently leave the evidence only in a local temporary path; if capture or attachment is genuinely impossible, say exactly why in the completion note.
- **Commit traceability** — every commit for ticket-driven work must include every ticket slug addressed by that commit in its commit message. This applies to both single-ticket and intentionally combined commits.

<!-- hotsheet:end section=ticket-driven-work -->

## Identity terminology

A checkout id is a readable Hot Sheet path identity (`folder-shortpathhash`), not a git
id, ticket-store id, or secret. Keep checkout↔store links many-to-many. Only server
instance data carries bearer credentials.

## Headless setup parity

No project-bootstrap or recovery capability may exist only in a graphical client.
Every workflow that initializes or links an HS2 ticket source, installs or updates the
shared Hot Sheet skills, configures MCP integrations, or prepares supported AI tools
must be available through an idempotent headless CLI path. Graphical clients should
invoke the same underlying application workflow rather than reimplementing setup.
When remote creation cannot be automated safely, the CLI must prompt for an existing
remote or print exact provider-neutral next steps; a checkout prepared from a terminal
must be as usable as one prepared through the client.

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

Activity notes are timeline history, not the primary home for a result. Put an
investigation's conclusion, decision, recommendation, or other important durable content
in a `regular` Markdown note. A finishing activity may briefly point to that regular note.
Format every AI-authored note for human scanning. Lead with the outcome or decision, not a
chronological transcript. For a substantial note, use short descriptive sections such as
`## Result`, `## Verification`, and `## Follow-ups`; use bullets for parallel facts, numbered
lists only for a real sequence, and tables when they make a dense comparison or timeline
materially easier to read. Break long prose into short paragraphs and format commands,
paths, and ticket slugs as code. Never paste an undifferentiated text/log dump or compress a
multi-part result into one dense paragraph. Keep simple updates brief and omit empty sections.
For multiline Markdown passed through the CLI, always use `--note-file`; `--note` rejects
literal `\n` sequences outside inline/fenced backtick code so escaped line breaks cannot
silently become visible text. Use `--allow-literal-backslash-n` only when such prose is
intentional.

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
claim before active work; claiming atomically advances a Not Started ticket to Started.
Workers renew during long work and release when they stop. Never
derive live activity from status or claim count. Self-claim workers use `claim-next`;
general orchestration and delegated agents use exact `claim <slug-or-id>`, then renew and
release that same ticket with one stable, session-specific worker id.
On git-backed Started tickets, set `started_phase` as work moves through analyzing,
planning, working, initial testing, integrating, and final testing. The phase is
durable progress; it does not signal a live worker without a claim.

For a claim ETA, estimate the time until the ticket can actually be completed, including
local checks and likely CI or handoff waits. Base it on the work in this ticket rather
than a habitual 60-, 90-, or 120-minute allowance; completed single-claim tickets have
usually taken much less than those allowances. If work is released and later reclaimed,
estimate the remaining work afresh, including any external wait. Renew the ETA when
new scope or a delayed gate changes the expected finish time.

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

**Styling a Kerf UI component with application CSS is a bug** (HS2-AD9WRF). Kerf components
are configured: through their documented props, variants, and public `--kui-*` theme tokens,
never through selectors that target `.kui-*` classes, Kerf-rendered parts, or the Kerf root
an app class happens to sit on. When no prop or token expresses what the app needs, add the
prop, token, or a new component to Kerf (file a `KF-*` ticket in the Kerf store and adopt it
when it ships) instead of overriding the component; until then the finding stays counted in
the Kerf UI doctor budgets, which only ever decrease. **This holds even when the
maintainer directly asks for the visual change** (HS2-JS9PSP): if satisfying a request
would mean overriding a Kerf component's styling, do not quietly break the rule. Either
ask the maintainer for feedback (naming the conflict and the Kerf API that is missing) or
proactively file a `KF-*` ticket in the Kerf store requesting the prop, token, variant, or
component, and adopt it when it ships. Application-owned components may carry
CSS for their own markup (native HTML and raw Web Awesome elements), and every surface should
compose Kerf components before implementing a custom one.

Treat reusable visual presentations as explicit component API variants, not
consumer-specific descendant CSS overrides. A component's UX demo must expose every
supported public variant and state that consumers rely on, including appearance and
size options, so the catalog is a complete interactive account of the component API.

Before selecting or wrapping a visual component, read Hot Sheet's consumer catalog at
`clients/web/ai/component-catalog-extension.json` together with Kerf's installed
`@kerfjs/ui/ai/component-catalog.json`. Preserve the package source identity and use the
declared `geometry` ownership (`self`, `parent`, `child`, `none`, or documented
`conditional`) instead of inferring margin, border, or padding from a screenshot. When
an app-owned reusable visual component or composition changes, update its UX-demo entry
and run `npm run catalog:sync` from `clients/web`; CI/lint checks that the generated
extension remains current.

Use native `<button type="button">` controls for ordinary actions inside Kerf
`ToolbarControlGroup`. The group owns their sizing, border, padding, and hover/focus
geometry; do not add Web Awesome button hosts or recreate that chrome in consumer CSS.
Reserve `wa-button` in a group for an actual Web Awesome-specific feature, such as a
dropdown popup trigger, as documented by the installed component catalog.

UX demos may replace production data sources and external side effects with deterministic
fixtures, but they must not be the sole owners of component interaction behavior. When a
component is composed into the real app, inventory every rendered action/event from that
component and wire or deliberately capability-disable each one. An enabled control that
only works in `/ux-demo` is a production bug. Browser coverage must exercise representative
child actions through every shipped parent composition (for example TicketRow through both
TicketList and TicketBoard), not merely through the isolated demo.

Ticket text editing autosaves when focus leaves the editing surface, never while the user
is still typing: keystrokes update the controlled draft and, after a 150 ms debounce, a
local recovery copy (localStorage, keyed by project, ticket, and field, with the value the
edit started from); the single server write happens on blur, page hide, or an explicit
finish, and merges against that edit-start value so a user's own typing can never be
presented as a conflict. Do not add routine Save/Cancel buttons for details, notes, titles,
tags, blocked reasons, or similar fields. Keep the controlled draft visible while saving,
restore an unsaved local copy when the editor reopens, and test rapid coalescing, blur,
composed-editor focus moves, post-save editing, and recovery after a reload. Explicit
submission actions remain appropriate when they create a new object or complete a
workflow rather than merely persisting an edit; discrete edits such as tag chips still save
immediately.

Every change that can affect rendered client visuals requires a deliberate visual QA
pass in a real browser before completion. Automated DOM, accessibility, computed-style,
and geometry assertions remain necessary, but are not substitutes for looking at the
rendered result. Exercise the affected demo states and transitions at representative
wide and narrow viewport sizes; capture screenshots when they make comparison easier.
An existing problem/design screenshot on the ticket makes a corresponding attached
after screenshot a required part of the visual QA pass, not merely an optional aid.
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
- **Coverage is a floor, not a ceiling**: 100% line/branch coverage shows every line _ran_, not that every _behavior_ — or every _sequence_ of behaviors — is _asserted_. It is structurally blind to a **missing state transition**: a bug living in an untested interaction sails through a green 100% report because the individual lines still get hit by isolated, single-operation tests.
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

> **Implemented test paths.** Rust unit/integration tests run with `cargo nextest run`;
> server HTTP/WS tests use temporary stores. `clients/web` runs Vitest unit tests and
> Playwright browser flows (`npm run test:unit`, `npm run test:e2e`), including real
> checkout/server compositions. CI runs web lint, typecheck, unit tests, build, Chromium
> smoke flows, and a WebKit lane (`.github/workflows/ci.yml`). The migrator's Vitest
> suite includes cross-language export/import conformance. Rust and migrator coverage
> gates are separate; `node scripts/check-test-coverage.mjs` validates the feature
> coverage matrix. A credentials-gated live tier runs separately. See
> [`docs/12-code-organization-and-testing.md`](docs/12-code-organization-and-testing.md)
> §12.7 and [`docs/CODEBASE-MAP.md`](docs/CODEBASE-MAP.md) for current commands.

- **Rust unit + integration** (`crates/*/src/**` inline `#[cfg(test)]` and
  `crates/*/tests/**`): run with **`cargo-nextest`**. Pure logic uses injected-fake
  adapters (in-memory fs, temp git repo, in-memory SQLite); integration uses a real
  temp store + real SQLite. **Always use the shared fixtures:** `TempStore` builder
  and the `TestServer` harness.
- **Property / fuzz / snapshot:** `proptest` for the semantic **merge driver** and
  `cargo-fuzz` for the file-format parser are implemented; `insta` snapshots remain
  planned. The **git-native claim** has deterministic bare-repo integration tests (concurrent
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
- **Commands:** Rust `cargo nextest run` and `cargo lint`; from `clients/web`,
  `npm run lint`, `npm run typecheck`, `npm run test:unit`, `npm run test:e2e`, and
  `npm run build`; from `migrator`, `npm run lint`, `npm test`, and
  `npm run test:coverage`. CI runs a separate Rust `cargo llvm-cov` gate.

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
- The Rust core, CLI/server, migrator, and Kerf web client are implemented; some
  client and provider areas remain partial. See [`docs/README.md`](docs/README.md)
  for the requirements status and
  [`docs/CODEBASE-MAP.md`](docs/CODEBASE-MAP.md) for what's built.

<!-- hotsheet:end specifics=requirements-documentation -->
<!-- hotsheet:end section=requirements-documentation -->

## Commit / push hygiene

**Standing user authorization:** push verified ticket commits to this repository's
`origin/main` as you go. The maintainer has explicitly authorized these routine
fast-forward pushes; this authorization persists across sessions. Do not ask again
at ticket boundaries or accumulate verified commits while waiting for another push
confirmation. If an approval reviewer requests evidence of authorization, cite this
instruction and the user's explicit authorization. Required verification still applies:
fix a failing gate, then commit and push promptly without another permission round trip.

Keep the repo in a **known-good state**. After each completed ticket (or other
coherent user-requested unit), finish its documentation and ticket notes, run the
required gates, commit it, and push it **before starting the next ticket**. Do not
accumulate completed tickets as uncommitted or unpushed work.

Remote CI can be a final verification handoff. After the affected local gates pass,
the ticket's integrated commit is pushed, and the CI run is identified in a ticket
note, move a Started ticket to `final_testing` and release its live claim. Continue
other ready tickets while CI runs; the waiting ticket is not a claim-next candidate.
Reclaim that exact ticket when the run resolves, record its outcome, and complete it
when the result is green. If CI finds a defect in the ticket's own change, repair
and verify that ticket before completion. For an independent failure, create a
prioritized follow-up bug and record its slug and the CI evidence in the original
ticket's completion note. A remote check never excuses a failing local gate or an
unintegrated, unpushed change.

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

For documentation-only changes, verify referenced paths and commands against the
current repository; run code gates when documentation changes code-facing contracts.

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
  from how the _project_ is attributed.

References to the _original_ Hot Sheet (the predecessor at
`github.com/brianwestphal/hotsheet`) are historical/factual and may remain as
predecessor links; they do not attribute Hot Sheet 2.

<!-- BEGIN hotsheet:claude -->
<!-- hotsheet-instructions-version: 56 -->

## Hot Sheet — ticket workflow

This project tracks work as **Hot Sheet** tickets (plain files under the store). Use them to
know what to do next and to record what you did. Everything below works **headless** — no
app, and no server required.

**Create tickets by default for real work — even when work is described directly to you.**
When someone asks you to do something in this terminal (not through the Hot Sheet queue),
open a ticket before you start, then work through it: **claim it** to begin, implement, set it
`completed` with a note. Do this for features, bug fixes, refactors, and any multi-step or
code-changing task. Skip ticketing only for trivial one-offs: simple questions, quick
lookups, a single-line fix, or a git commit. When in doubt, create the ticket.

**Find and plan the queue:**
- `hotsheet-cli ls --up-next` — the prioritized Up Next queue.
- `hotsheet-cli show <slug>` — read one ticket in full.
- Or the MCP tools: `hotsheet_query` (with `up_next: true`) and `hotsheet_get`.

**Claim a ticket before you work it — claiming, not `started`, is what signals live work:**
- `hotsheet-cli claim <slug> --worker <your-id>` when you begin. This atomically moves a Not
  Started ticket to **Started** *and* takes a renewable live lease that tells everyone you are
  actively on it. Always claim before you touch code. Prefer it over `hotsheet-cli edit <slug>
  --status started`, which only flips the status and does **not** claim or signal live work.
  (Self-serve the top of the queue with `hotsheet-cli claim-next --worker <your-id>`.)
- **Your worker id:** if `HOTSHEET_WORKER_ID` is set in your environment, use its value as
  `<your-id>`. Hot Sheet gave it to this session and releases whatever it still holds when the
  session ends. Otherwise choose one stable id for the session.
- `hotsheet-cli renew <slug> --worker <your-id>` during long work; `hotsheet-cli release <slug>
  --worker <your-id>` whenever you stop working it (see below).
- **Estimate non-trivial work.** When you claim a ticket that is not trivially simple, add
  `--eta <duration>` (for example `--eta 45m`; MCP `eta`) with your honest estimate of when
  you will finish. If `renew` reports that the ETA has passed, renew again with a new `--eta`.
- `hotsheet-cli edit <slug> --status completed --note "what you did"` when done.
- Or the MCP tools: `hotsheet_claim_next` / `hotsheet_renew` / `hotsheet_release` for the lease,
  and `hotsheet_update` (it takes a `note`) / `hotsheet_close`.
- Create work with `hotsheet-cli new --title "…" --category <bug|feature|task>` or
  `hotsheet_create`.

**Release your claim the moment you stop working a ticket — every time, for any reason.** A
live claim tells people and other agents that someone is actively on that ticket, and it blocks
them until its lease expires. Release it (`hotsheet-cli release <slug> --worker <your-id>` or
`hotsheet_release`) as soon as you stop: you completed it, hit a blocker or `FEEDBACK NEEDED`,
are handing it off, are switching to a different ticket, decided to defer or not continue it,
ran out of time or budget, or are about to end your turn or session. Releasing never changes
the ticket's status; a ticket you set down part-way stays `started`, so add a note saying
where you stopped. Before your final response, run `hotsheet-cli ls --claimed` and release
every claim you hold but are no longer actively working.

**Create every follow-up immediately, without asking.** As soon as you identify an
unfinished step, open question, known gap, out-of-scope task, or designed-but-unbuilt
behavior, create its ticket rather than leaving it in a comment, TODO, or note. Do not ask
permission or promise to file it later. Reference every follow-up slug in the current
ticket's completing note, then continue.

**Before completing a ticket:** finish and verify its scope; update the tests, coverage, and
docs the change requires; scan for placeholders, TODO/FIXME, stubs, and documented-but-
unbuilt behavior; create a follow-up for every incomplete item; and put the result,
verification, and all follow-up slugs in the completing note. `FEEDBACK NEEDED` is only for a
blocker on the *current* ticket that needs a user decision or unavailable external state —
leave that ticket `started`, name the blocker, and release its lease (`hotsheet-cli release`).
It does not replace follow-ups for independently describable work.

**Integrate worktree and background-agent work before you complete its ticket.** Work done in a
git worktree, on a side branch, or by a sub-agent or background worker is not done until it is
on the branch this project ships from. Before marking the ticket `completed`:
1. Merge, rebase, or cherry-pick that work into the main checkout's branch, or open the
   project's pull request when that is its convention.
2. Confirm the commit is reachable there (`git branch --contains <sha>`), pushed wherever this
   repository pushes, and that the gates pass on the integrated result.
3. Name the integrated commit(s) in the completing note, then remove the finished worktree.

A delegated worker's report that it "completed" a ticket is not completion: the agent that owns
the ticket verifies the integration itself. If integration fails or needs a decision, leave the
ticket `started` with a note naming the branch, worktree path, and commit instead of completing
it. When other agents share your checkout, stage and commit only your own files (explicit
paths, never a directory-wide `git add`), and check the staged diff for changes you did not make.

**Share preliminary thoughts on non-trivial tickets.** After your initial analysis of a
ticket that is not trivially simple, and before you implement, add a short `regular` note
headed `## Preliminary thoughts`: your understanding of the problem (or likely root cause),
the approach you plan, the main risks or open questions, and how you will verify it. It lets
people steer early and gives a later reader your starting reasoning. Skip it for trivial
tickets (a quick, obvious change); never let it replace a `FEEDBACK NEEDED` blocker.

**Report completion confidence.** When you move a ticket to `completed`, the completing note
must include a `## Confidence` section: the integer score (0-100), then one short line per
factor, each rated high/medium/low with a phrase — clarity of the request; context and
supporting information available; comprehensiveness and realism of verification (unit, E2E,
real-browser visual QA; actually ran vs. assumed); scope deviation or unverified assumptions;
known gaps deferred to follow-ups. Pass the same integer in that same update as
`--note-confidence <0-100>` (MCP `note_confidence`) so clients never parse prose. Anchor
bands: **90-100** fully verified end to end against the real system; **70-89** verified with
minor assumptions; **40-69** partially verified or an ambiguous ask; **below 40** largely
unverified — name the gaps. A bare number without the factor lines is non-compliant.
Identify yourself as the AI actor: sessions Hot Sheet launches already set
`HOTSHEET_ACTOR_ROLE=ai`, and generated MCP configs declare it for `hotsheet-mcp`;
otherwise pass `--actor-role ai --actor-id <your-id>` (MCP `actor_role: "ai"`,
`actor_id`). An AI completion without a score is rejected with
`confidence_required` and changes nothing; retry the same call with the score.

**Format AI-authored notes for human scanning.** Lead with the outcome or decision, not a
chronological transcript. For a substantial note, use short Markdown sections such as
`## Result`, `## Verification`, and `## Follow-ups`; use bullets for parallel facts,
numbered lists only for a real sequence, and tables only when they clarify a dense
comparison or timeline. Break long prose into short paragraphs and format commands, paths,
and ticket slugs as code. Never leave an undifferentiated text/log dump or one dense
paragraph. Keep simple updates brief and omit empty sections.

Normally continue until every actionable Up Next ticket is complete. Read the whole queue
before choosing an order; weigh dependencies, overlap, risk, and safe parallelization. Treat
priority as important guidance, not a hard rule. The CLI and MCP tools use the same engine —
use whichever is handier.

**Write portable durable references.** In documentation, ticket text, and notes, never copy a
developer-specific home directory, username, or absolute clone path. Use repository-relative
paths, a stable repo name/URL, or a placeholder such as `<repo-root>/path`. Keep an exact
local path only as clearly labeled machine-local diagnostic evidence.

## Testing

- **Double coverage:** cover each feature with both unit tests (logic in isolation, external
  dependencies mocked) **and** end-to-end tests (real user flows through the running system,
  minimal mocking). Keep test fakes faithful to the real contract — same shapes, fields, and
  status codes.
- **Coverage is a floor, not a ceiling.** 100% lines means every line *ran*, not that every
  *behavior* — or every *sequence* of behaviors — is *asserted*. It is blind to missing state
  transitions.
- **Stateful code gets transition-matrix + adversarial tests.** For anything with modes, a
  cache, or a state machine, enumerate the states *and* the transitions, then walk realistic
  multi-step sequences that cross boundaries. Deliberately try to break it with out-of-order,
  interleaved, repeated, and empty-then-refill sequences; pin any bug you find as a permanent
  regression test.
- **Fix lint and type errors before finishing** — as you go, not batched.

## Requirements & docs

Keep human-readable requirements/docs as the source of truth for what the project does, and
update them **in the same change as the code**: add, remove, or modify a behavior → update
its doc in the same commit. Create a new doc for a major new functional area and cross-link
related docs.

## Commit hygiene

Keep the repo in a known-good state.

1. Implement one coherent, ticket-sized change and update its docs and tests.
2. Lint and fix every affected package; run the affected unit and end-to-end tests. Do not
   leave a lint warning or failing test behind.
3. Mark the ticket `completed` with its verification note, review the diff, and make **one
   commit per ticket** whose message names every ticket slug it addresses. Combine tickets in
   one commit only when their changes overlap so strongly that separating them would be unsafe
   or misleading.
4. Get the worktree clean before starting the next ticket.

**Pushing is up to this repository.** Follow whatever push/PR/review conventions this project
already uses; this default guidance does not require or forbid pushing on its own.
<!-- END hotsheet:claude -->
