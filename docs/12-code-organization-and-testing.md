# 12. Code Organization & Testing

> **Status: Decided** (maintainer, 2026-08-19, interactive session). This is the
> plan for how Hot Sheet 2's code is structured and tested, agreed before any
> implementation. Build/setup tracked in **HS2-16**.

## 12.1 Guiding principles

1. **Ports & adapters (hexagonal).** The core is pure domain logic that reaches the
   outside world only through **injected trait objects** (`Clock`, `FileSystem`,
   `GitLocal`, `GitRemote`, `ProcessSpawner`, …). Real implementations live in the
   binaries; fakes live in tests. This is how the docs' "policy-free, I/O via
   injected adapters" ([04](04-core-server-cli.md) §4.1) becomes concrete, and it is
   the backbone of testability.
2. **One simplification we earned:** because clients don't embed the core
   ([09](09-technology-decisions.md) §9.1e), the core has **no client-safety
   constraint** — it may use `tokio`/`fs`/`git` freely. HS1's constant "keep it
   browser-safe" fight (its docs/132) simply doesn't exist here.
3. **Correctness-critical surfaces get more than line coverage.** The merge driver
   and the claim primitive can lose data or double-claim, so they get property tests
   and adversarial sequences, not just examples.

## 12.2 Workspace layout (full monorepo)

A single Cargo workspace at the repo root, everything in one repo so a wire-type
change regenerates client types and updates every client in one PR (no cross-repo
drift).

```
hotsheet2/
  Cargo.toml                      # workspace
  crates/
    hotsheet-model/               # PURE: types, ULID+slug, md+yaml file format
                                  #   parse/serialize, and the semantic 3-way MERGE. No I/O.
    hotsheet-ticketing/           # store(git) + index(sqlite/fts5) + watch + query + coord.
                                  #   Defines adapter traits; composes model.  ← the ONLY
                                  #   domain crate the CLI links (+ model).
    hotsheet-aitools/             # AI-tool plugin host: drive / instructions / skills / command /
                                  #   permissions / mcp / metrics / activity + permission bridge.
                                  #   Deps: ticketing + terminals.
    hotsheet-providers/           # External authoritative ticket providers
                                  #   (GitHub/GitLab/Jira). Deps: ticketing + HTTP;
                                  #   NO terminals. (docs/16)
    hotsheet-terminals/           # PTY manager + broker client + busy inference. Nearly standalone
                                  #   (needs project cwd/config, not the ticket index).
    hotsheet-types/               # wire/API types (serde) + client codegen (ts-rs → Kerf/TS; later Swift)
    hotsheet-server/  (bin)       # axum/tokio — HTTP/WS/MCP + watcher + terminal host
    hotsheet-cli/     (bin)       # clap — ticket ops + init/serve/reindex/doctor + merge-driver
    hotsheet-ptybroker/ (bin)     # the detached PTY broker
  clients/
    web/                          # Kerf (kerfjs) SPA (the Tauri webview UI and the browser build)
    tauri/                        # Rust shell: launches/supervises the server, mTLS proxy
    apple/                        # SwiftUI macOS/iOS (stages 2–3)
    android/                      # Kotlin/Compose (stage 4)
  migrator/                       # standalone Node PGLite→git migrator (disposable)
  docs/  spikes/                  # design docs + spikes
```

### 12.2.1 Dependency rules (what may depend on what)

- **`hotsheet-model`** depends on nothing Hot-Sheet-specific and does **no I/O** — so
  it links into the migrator's conformance test and any surface cheaply.
- **`hotsheet-ticketing`** = the CLI's entire domain surface. It must **not** depend
  on the plugin crates or `terminals` — that keeps `hotsheet-cli` a tiny binary
  (maintainer's requirement: ticketing is separable from terminals/AI-hosting because
  the CLI needs none of the latter).
- **One crate per plugin _type_** (maintainer, 2026-08-19), named `hotsheet-<type>`,
  each a registry of that type's plugins with its **own conformance suite** — they do
  not share a mega-crate, so each pulls only its own deps:
  - **`hotsheet-aitools`** (AI-tool plugins) depends on `ticketing` **+ `terminals`**
    (it drives agents in PTYs). Holds the drive/metrics/activity/permission/mcp/… host.
  - **`hotsheet-providers`** (external authoritative ticket providers) depends on
    `ticketing` **+ HTTP clients**, **not** `terminals`. The provider-neutral contract
    and default git adapter remain in `hotsheet-ticketing`; network providers live
    here so the server/CLI can opt into them without terminal/agent machinery.
  - Future plugin types get their own `hotsheet-<type>` crate the same way.
  - The **pattern** is shared across all of them (declarative identity + behavioral
    half, injected adapters for testability §12.7, a conformance gate) even though the
    _interfaces_ differ; extract a shared `hotsheet-plugin-core` only if a third type
    reveals real common machinery — don't pre-abstract.
- **`hotsheet-terminals`** is nearly standalone (project cwd/config, not the ticket
  index) — which is what keeps the future **process split** (a separate durable
  terminal server) cheap. See §12.5.

> **§12.3–§12.5 are implementation-choice _decisions_.** They shape the crate APIs and
> boundaries, so they're summarized here, but the **decision + rationale of record
> lives in the ADR log** ([09-technology-decisions.md](09-technology-decisions.md)) —
> the intuitive home for "what we chose and why."

## 12.3 Async model — sync core, async at the edges

`model`/`ticketing` are **synchronous**; the server wraps them in one async facade
that centralizes `spawn_blocking`; SQLite = WAL + read-pool + single writer; async
only at inherent edges (server, watcher bridge, terminal streams, git-remote via
`tokio::process`). So the domain crates carry **no `async fn` in their public APIs** —
a fact you rely on when laying out `hotsheet-ticketing`. Decision + rationale:
[09](09-technology-decisions.md) §9.12.

## 12.4 Git access — gix local, git CLI network

A `GitLocal`/`GitRemote` **adapter split**: `gix` (pure Rust) for local
commit/diff/history; shell out to `git` for fetch/push/claim-CAS (uses the user's real
config/credentials/SSH). Both adapters are **injected**, so tests fake them. Decision +
rationale: [09](09-technology-decisions.md) §9.13.

## 12.5 Terminal process topology — separable crate, split deferred

`hotsheet-terminals` is its **own crate**; v1 runs one ticket+terminal server + the
detached PTY broker; a fully separate terminal _process_ is a later, cheap change the
crate boundary preserves. Decision + rationale: [09](09-technology-decisions.md) §9.14.

## 12.6 Conventions

- Errors: `thiserror` in libraries, `anyhow` at binary edges; `Result` throughout.
- Logging: `tracing` structured logs.
- Workspace-level dependency versions; `rustfmt` + the repository `cargo lint` Clippy
  alias are enforced in CI. Every code package also exposes a zero-warning lint command;
  TypeScript clients use the shared Glassbox-derived ESLint baseline. The web application
  root has no per-file rule override: Promise rejection values are typed `unknown`, and
  defensive checks that intentionally exceed a total static type use explained,
  single-next-line exceptions so new code remains subject to the full baseline
  (HS2-W3RDCB).
- Repository formatting uses Prettier for supported JavaScript, TypeScript, JSON, CSS,
  HTML, Markdown, and YAML plus `cargo fmt` for Rust. `clients/web` owns the pinned
  Prettier toolchain and exposes `npm run format` / `npm run format:check`; its lint command
  runs the repository-wide format check first, so CI rejects drift in all supported
  source and structured-content areas (HS2-F0BC6Q).
  Both commands require the tracked web, migrator, compatibility-spike, docs, and
  workflow paths. The setup-generated root `opencode.json` is optional in a clean
  checkout; when present, it receives the same formatting and syntax checks. Missing
  tracked paths and unreadable or malformed local config remain errors. Rust formatting
  continues through `cargo fmt` after Prettier succeeds (HS2-G9K0NY).
- Wire types in `hotsheet-types` derive serde + `ts-rs` (→ TypeScript for the Kerf client;
  Swift generation added for the native client).
- Kerf component props that represent CSS lengths, flex shorthands, or colors use the
  typed builders from `@kerfjs/ui/css-values`; raw CSS strings are not passed through
  component APIs. Product colors are application-owned custom properties referenced with
  `colorVar()`. Components without a raw `style` escape hatch, such as `ListItem`, use
  finite `data-*` states and public component tokens in static CSS (HS2-KAWND3).

### 12.6.1 Optional local Rust compiler cache

Rust contributors can use [`sccache`](https://github.com/mozilla/sccache) to reuse
compiler outputs across repeated lint, nextest, and coverage builds. Install it with
`brew install sccache` on macOS or `cargo install sccache --locked`, then enable it in
the shell that runs Cargo:

```sh
export RUSTC_WRAPPER=sccache
sccache --start-server
```

Add the export to your shell profile if you want it enabled for every checkout. The
repository deliberately does not set `build.rustc-wrapper` in `.cargo/config.toml`:
that would make Cargo fail for contributors and automation without `sccache`
installed. `RUSTC_WRAPPER` applies unchanged to the `cargo lint` alias,
`cargo nextest run`, and `cargo llvm-cov`; inspect effectiveness with
`sccache --show-stats`. CI continues to use its existing Cargo cache and does not
enable sccache.

### 12.6.2 Isolated Clippy artifacts

The repository's `cargo lint` alias supplies the scoped Cargo config
`build.target-dir="target/clippy"`. `cargo nextest run`, ordinary builds, and coverage
retain the default `target` layout, so alternating lint and test runs no longer
invalidate one another's incremental artifacts. Cargo's normal environment precedence
still lets an explicit caller/CI `CARGO_TARGET_DIR` win.

This intentionally spends additional local disk space and duplicates the first cold
compile. The common edit → lint → nextest loop recovers that cost by keeping both warm;
optional sccache remains complementary because it can reuse compiler outputs across the
two target directories. `cargo clean` removes the nested Clippy directory along with the
rest of `target`.

---

### 12.6.3 Web interaction ownership

The web client keeps `clients/web/src/main.tsx` as a bounded browser entry that imports
global styles and explicitly starts the application runtime. Runtime composition lives
under `clients/web/src/app/`; `wire-interactions.ts` owns the behavior-sensitive ordering
of the twelve registration groups, while feature modules own their state and workflows.
Twelve modules under `src/interactions/` own the existing
project lifecycle, repository, navigation/tabs, terminals, ticket selection, saved
views, commands/AI, notifications/links, search/composer, attachments/gallery,
inspector/editor, and shell/global handler groups (HS2-YWF98M).

Each module exports a wiring function and an explicit dependency interface. Importing
it does not register listeners. Main invokes the functions once in their original
order; each retains its original delegation root, capture/bubble phase, event name,
selector, cancellation behavior, and shared Kerf adapter. Modules import pure helpers
directly and never import main or create replacement application signals.

Signal objects and functions can be passed directly. Shared mutable plain bindings
such as selection anchors, long-press flags, drag state, gesture state, and editor
draft bases cross the boundary as typed getters/setters; handlers read them through
the dependency object rather than destructuring a stale primitive snapshot. Small
shared DOM/domain types live in `interactions/types.ts`, and dataset adaptation lives
in `interactions/dom.ts`.

`interaction-wiring.test.ts` compares the ordered registration inventory with the
pre-extraction baseline. Callback tests exercise external state replacement, repeated
native dismissal, range anchors, and long-press suppression; production browser
flows remain the end-to-end behavior contract. Changing registrations intentionally
requires reviewing the inventory alongside those behavior tests.

`project-startup.ts` owns transport preparation and the remembered-open coordinator
(HS2-V9ZVW0). Each initial/retry pass fetches concurrently without owning application
signals; `main.tsx` registers successful projects serially, then activates/restores one
selected project. Explicit project opens reuse the same fetch and registration boundaries.
The startup unit matrix controls completion ordering and retry transitions, and the
production-browser suite checks request overlap, active-only loading, saved drafts,
onboarding, failure identities, and empty/refilled sessions.

### 12.6.4 Web feature state and presentation owners

Eleven factories under `clients/web/src/features/` own cohesive state/controllers and
render projections extracted from the root (HS2-DHYGXJ):

- `commands.tsx`: project command drafts, selection anchors, autosave, icon search,
  and command-dialog composition.
- `repository.tsx`: repository status, detail pagination/generation, file selection,
  observer lifetime, review state, and repository/evidence surfaces.
- `permissions.tsx`: the permission inbox, history, decision rollback, polling,
  countdown, and popup projection.
- `gallery.tsx`: media selection, gestures, playback, measurement, annotation sessions,
  and gallery/menu composition.
- `conversation-archive.tsx`: message-range selection, copy, export, and saved-chat opening.
- `saved-views.ts`: shared-view dialog/query state, validation, persistence, rename, and deletion.
- `project-lifecycle.ts`: project restoration/opening, migration and ticket-source setup,
  provider authentication, stale-response guards, and lifecycle-owned dialog state.
- `ticket-workflows.ts`: ticket mutations, autosave, selection, bulk and clipboard operations,
  linked readers, attachments, creation, close, and Not Working workflows.
- `ai-configuration.tsx`: project AI configuration, tool/model/effort selection,
  provider changes, and manual-model lifecycle.
- `terminal-viewports.ts`: DOM mount identity, intersection observation, progressive
  setup/teardown, and focus-request consumption.
- `terminal-presentation.tsx`: live workspace, drawer, and conversation surface props.

Factories have explicit typed ports and no import-time listeners, observers, polling,
or imports of `main.tsx`. The runtime creates each owner before mount or startup can use it,
passes signals rather than value snapshots, and retains cross-feature actions as
callbacks. Mutable plain state crosses ports through getters/setters; each timer,
generation, observer, or gesture binding has one owner. Presentation functions run
inside the root render so their original `.value` dependencies and `.peek()` reads
retain their behavior. The twelve interaction registrations remain in the same order.

The refactor does not change the feature contracts. Controller unit tests cover
project replacement with delayed old responses, optimistic failure rollback,
selection/reset/refill, model/effort propagation, export cancellation, gallery reset,
and stale viewport candidates. The full production browser suite remains the parity
gate; an additional wide/narrow command workflow crosses edit, autosave, run, cancel,
navigation, and a second edit. The pre-existing export cancel/reopen async-identity
gap is tracked separately in HS2-1S33A9.

## 12.7 Testing strategy

Follows the project's philosophy (double coverage; transition-matrix testing for
stateful modules; adversarial pass; a manual test plan) — adapted to Rust +
distributed + git. **Coverage is a floor, not a ceiling:** the behavior / property /
transition-matrix tests are the real assurance.

### 12.7.1 Tiers

- **Unit** (`cargo test` via **`cargo-nextest`**): pure logic against injected fakes
  (in-memory fs, temp git repos, in-memory SQLite). The adapter traits make this fast
  and mock-light.
- **Integration** (per crate): real temp git store + real SQLite, no mocks — e.g.
  "write ticket → file committed → index upserted → query returns it."
- **Server E2E:** boot the real server on an ephemeral port against a temp store;
  drive it over HTTP/WS with a test client; assert full flows with minimal mocks.
- **Client E2E:** the Kerf web UI via **Playwright** against a real running server;
  the Tauri app via its harness; SwiftUI via XCUITest (later).
  Keep independent startup and reload contracts in focused scenarios with positive
  application/component readiness before absence assertions. Responsive layout tests
  resize the same adversarial content and assert it remains present at each size;
  a second navigation must not silently reset the content under test. Test structure
  should absorb parallel-suite startup costs without weakening the shared timeout
  or replacing a required reload with an in-page update (HS2-9TZ9AF).
- **CLI + MCP E2E:** drive the CLI against a temp store (assert disk state +
  idempotence); drive the `hotsheet_*` MCP tools via a test MCP client.

### 12.7.2 High-risk surfaces get more than examples

- **Merge driver** → **`proptest`** (random two-branch edits: never lose a note,
  deterministic, frontmatter field-merge invariants) + **`insta`** snapshots for
  specific 3-way scenarios. Highest data-loss risk → most scrutiny.
- **File-format parser** → **`cargo-fuzz`**: malformed frontmatter must degrade,
  never panic.
- **Git-native claim** → the HS2-63 spike harness **productized as deterministic
  bare-repo integration tests** (concurrent "workers" → assert mutual exclusion,
  renew, steal, sweep). GitHub-specific behavior stays an **opt-in live test**
  (needs a real remote + creds), gated like HS1's `test:fast` exclusions.

### 12.7.3 Stateful modules → transition-matrix + adversarial sequences

Per the CLAUDE.md mandate, enumerate states **and** transitions and walk realistic
multi-step sequences across boundaries for: the **claim/lease** machine, the **index
reconcile** (create/edit/delete/move/rename, git-HEAD-moves), the **terminal-sizing
arbiter** (viewport focus/blur/join/leave/disconnect → size decisions —
[06](06-clients.md) §6.7), and the **sync engine** (offline→online, conflict→resolve).
Every stateful bug found → a permanent regression test walking the exact bad sequence.

Client controls add a second state boundary: the Kerf signal/resource and the live DOM
property held by a native or Web Awesome element. Component contracts must therefore
test both directions. For each exposed setting, drive the control and verify application
state plus rendered output; then reset or externally replace the application state and
verify the live control properties plus output. Make one more edit after reset to prove
the binding remains usable. Playwright assertions for custom elements inspect properties
such as `value` and `checked`, not attributes alone, and exercise every visible action
(including Reset, Save, Cancel, Apply, Remove, and Undo). Shared binding helpers receive
their own exhaustive contract tests; each consuming component retains a focused flow test.

### 12.7.4 Cross-language conformance (the migrator's guard)

A CI test where the real `hotsheet-model` **parses and round-trips what the Node
migrator wrote**. That single test removes the drift risk and lets the migrator stay
a simple, disposable standalone tool ([07](07-migration.md) §7.2.1).

### 12.7.5 Coverage — per-language gates + an aggregate

A single literal merged report across Rust + TS + (later) Swift is impractical, so:

- **Per-surface gates in CI:** `cargo-llvm-cov` (Rust), Playwright/istanbul (web),
  `vitest` (migrator) — each with its own high threshold.
- **A feature-layer report** in [`TEST-COVERAGE.md`](TEST-COVERAGE.md) records unit,
  E2E, and manual evidence for each shipped surface; CI validates its statuses and
  evidence paths with `scripts/check-test-coverage.mjs`. There is **no fake merged lcov**.
- The property / transition-matrix / behavior tests are the real bar — a green
  coverage number is necessary, not sufficient.

### 12.7.6 Fixtures & CI

- **Shared fixtures ("helpers to always use"):** a `TempStore` builder (temp git repo
  - seeded tickets), a `TestServer` harness (real server + temp store on an ephemeral
    port), and an in-memory adapter set for pure unit tests.
- **CI (GitHub Actions) — built:** the `check` job runs `fmt --check`, `cargo lint`,
  `nextest`, web client lint/typecheck/unit/build, spike lint/typecheck/build, the plugin
  **conformance test** (HS2-64), the CLI
  build, and the **migrator vitest + coverage** (now `test:coverage`, gated on the
  per-language thresholds in `migrator/vitest.config.mjs`) + lint + the cross-language
  conformance. A separate **`coverage` job** collects Rust coverage once
  (`cargo llvm-cov nextest --no-report`), uploads an lcov artifact (a _separate_
  per-language summary, not one merged lcov), and **gates** on a conservative line
  floor (`report --fail-under-lines`). A scheduled, creds-gated **`Live tier`**
  workflow (`live.yml`, nightly + manual dispatch) runs the `#[ignore]` live
  codex/claude turns (`HOTSHEET_CODEX_LIVE`/`HOTSHEET_CLAUDE_LIVE`) only on a runner
  flagged `HOTSHEET_LIVE_RUNNER`, keeping the default tier fast. Playwright web E2E is
  now part of the normal suite. **Pending (HS2-FPXSD0):** raise measured coverage
  floors, close remaining web coverage gaps, and add the macOS matrix leg for
  terminal/native-client surfaces.
- The `check` job also validates the feature double-coverage matrix. Its validator accepts
  formatter-padded Markdown cells, ignores table separators, and checks every feature row
  and evidence reference, including rejecting rows outside the matrix markers. Repository guidance
  requires the matrix to change with feature/requirement status or test-layer changes,
  making missing unit/E2E evidence visible even when line coverage remains green.
- **Manual test plan** (`docs/manual-test-plan.md`, created with the first code):
  real multi-device terminal-sizing focus handoff (iOS↔macOS), mTLS enrollment / QR
  pairing across devices, native-client UX, long-term GitHub custom-ref behavior —
  migrated to automation as we can.

### 12.7.7 AI-tool integration testing (the HS1 Codex pain, designed out)

Adding an AI tool to HS1 (Codex, when it started Claude-only) was a heavy **manual**
effort. The root cause: HS1 conflated two very different questions into one manual
test — **"does _our host_ handle the protocol correctly?"** (automatable) and **"does
the _real tool_ actually speak that protocol?"** (real-tool drift). Splitting them is
the whole strategy: ~95% becomes deterministic automation; the ~5% drift check is a
small, explicit layer. Build: **HS2-64**.

**1 — Testability is a plugin-interface rule.** Every side-effecting interaction a
plugin performs goes through an **injected adapter** (`ProcessSpawner`, config-file
writer, `PermissionTransport`, `McpConfigWriter`, `Clock`). No plugin touches a real
process, file, or global directly. This is the non-negotiable that makes everything
below deterministic — see [05-ai-tool-plugins.md](05-ai-tool-plugins.md) §5.10.

**2 — `hs-fake-agent`: a scriptable test double.** A workspace test binary that speaks
the _same protocols a real tool does_, but scripted and deterministic. It can be told
to: connect over **MCP** and call `hotsheet_*` tools in sequence; **request a
permission** through a plugin's transport and await the decision; run in a **PTY** and
emit scripted bytes — OSC 7/8/9/133, **spinner glyphs**, output, chosen exit code;
emit **busy/idle** signals. So "does the Codex plugin's drive/permission/busy wiring
work?" is tested by pointing Hot Sheet at `hs-fake-agent` configured per the Codex
plugin's declared protocol — **no real tool, no LLM, no keys.**

**3 — Per-aspect automated E2E** (against the fake agent):

- **MCP usage** — a fake MCP client hits the per-project shim: assert the tool list +
  schemas, each call's store effect, error handling; each plugin's `mcp` capability
  writes a valid config entry in that tool's format.
- **Permission checks** — drive a request through the bridge: FIFO enqueue
  (concurrent requests preserved), WS push, answer routed to the _originating_
  connection, allow-once/always → persisted rules; each `permissions` capability's
  install-then-remove leaves foreign hook entries intact (merge-safety).
- **Terminal integration** — the terminal manager against the fake agent: scrollback,
  multi-viewer attach, sizing arbitration, OSC parsing, **survival across a broker
  restart**; the `command` capability is a pure resolve-the-launch-line test.
- **Busy-state monitoring** — a **transition-matrix** test: feed scripted hook signals
  _and_ byte-stream spinner glyphs; walk busy→sustained→idle, stale-clear, the
  spinner-liveness gate, dropped-Stop-hook recovery.

**4 — A conformance suite over every plugin, as a hard CI gate.** One suite
parameterized over the plugin registry, run against a temp fixture project — identity,
instructions, skills, command (injected spawner), drive, permissions (merge-safety),
MCP config. **A new tool inherits the entire suite by existing**, and can't merge
until it passes conformance _and_ the fake-agent E2E. This is the forcing function
that makes adding a tool boring instead of painful.

> **Built (HS2-64):** `crates/hotsheet-cli/tests/plugin_conformance.rs` — parameterized
> over `builtin_plugins()` **and** a fresh on-disk plugin (so a third-party tool is
> validated the same way): identity/detection, instructions (managed block present +
> safe target), **skills absence-as-feature** (accessor ⇔ manifest agree), **MCP config
> written + re-parses** in its declared format with the shim server present, **drive
> declaration resolves to a real host `Drive`** of the matching transport, no write
> target escapes the project, and a **full headless `setup` E2E** (idempotent). Runs
> under `cargo nextest run`, so it's already the hard CI gate. Testability rule (part 1)
> is enforced by `hotsheet-aitools`' injected adapters; the busy-state transition matrix
> (part 3) is covered by `ConnectionRegistry` tests (HS2-107). The `hs-fake-agent`
> PTY/permission emulator + terminal E2E (parts 2/3) need terminals (HS2-10) + the
> permission bridge (HS2-113) → **HS2-1GJY50**.

**5 — The drift layer (thin + explicit), for real-tool protocol changes:**

- **Recorded contracts** — capture each real tool's actual protocol messages once as
  fixtures (cassette-style) and replay them in fast CI; if a tool's real format
  diverges from its recording, a test fails and names exactly what changed.
- **Opt-in live smoke** — a tiny per-tool suite that runs the _real_ binary
  (creds-gated, nightly / pre-release) for the end-to-end sanity a recording can't
  give.

> **Built:** sanitized, version-pinned Codex 0.148 and Claude 2.1.241 usage cassettes
> captured by the deliberate HS2-CQ6B96 live verification replay through the production
> parsers in fast CI. The ignored, credentials-gated live tests remain the drift oracle.
> HS2-SW655F adds Codex 0.152.1 completed-item and Claude Code 2.1.258 PreToolUse
> activity cassettes, replayed through the production activity mappers and transport
> event adapters; their live vocabulary checks require the explicit
> `HOTSHEET_CODEX_LIVE=1` / `HOTSHEET_CLAUDE_LIVE=1` opt-ins.
> ACP/OpenCode now joins this gate via the ACP v1 contract cassette and the ignored,
> credentials-gated `HOTSHEET_OPENCODE_LIVE=1` smoke test (HS2-PEQ6Q8).

**Payoff:** adding a tool = write the plugin module + record its real protocol once;
everything else is inherited and automated.

## 12.8 Cross-references

- Core / server / CLI split: [04-core-server-cli.md](04-core-server-cli.md)
- Storage + merge driver (the property-test target): [02-ticket-storage.md](02-ticket-storage.md) §2.7
- Index + reconcile (a transition-matrix target): [03-indexing-and-query.md](03-indexing-and-query.md)
- Git-native claim (integration-test target): [08-distributed-and-remote.md](08-distributed-and-remote.md) §8.5
- Terminal-sizing arbiter (a transition-matrix target): [06-clients.md](06-clients.md) §6.7
- Migrator conformance test: [07-migration.md](07-migration.md) §7.2.1
- AI-tool plugin interface + its testability rule: [05-ai-tool-plugins.md](05-ai-tool-plugins.md) §5.10

### Real ticket-server browser regression

`npm run test:e2e` in `clients/web` builds the CLI and server before Playwright. Focused
`npx playwright test` runs can reuse those binaries after `cargo build -p hotsheet-cli
-p hotsheet-server --bins` at the repository root. `tests/real-ticket-server.ts` creates an
isolated store, checkout registry, ephemeral loopback server, and process cleanup for
browser flows that must prove real storage/classification/index behavior. Optional
`HOTSHEET_TEST_CLI_BIN` and `HOTSHEET_TEST_SERVER_BIN` paths select frozen verification
binaries when other worktrees share the build cache.

HS2-AVXYCB exercises the actual reader's quoted inline answer against that server,
checks the persisted ordinary note and cleared indexed/full-ticket review state, then
reloads to confirm persistence. Provider discovery stays deterministic; ticket reads and
writes cross the real HTTP and filesystem boundaries.
