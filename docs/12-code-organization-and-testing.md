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

The CLI constructs and parses its full Clap command tree on one explicitly sized
8 MiB thread, then dispatches the parsed command on the original thread
(HS2-H0MF53). The debug generated builder alone measured about 1.66 MiB; real
startup failed with 1 and 2 MiB entry stacks and succeeded with 4 MiB on macOS.
The chosen parser budget provides headroom without changing help/error exit codes,
panic propagation, dispatch, or test timeouts. Portable real-process coverage checks
help, invalid arguments and coordination setup/status with a 1 MiB entry-stack cap
on Unix and the native default entry stack on Windows. Native Windows verification
requires its actual CI job.

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
  (HS2-W3RDCB). Childless JSX elements, native and custom alike, use the self-closing form
  (`<wa-input … />`, not `<wa-input …></wa-input>`); `@stylistic/jsx-self-closing-comp`
  enforces it in `clients/web` and `spikes/kerf-webawesome`, and `--fix` rewrites violations
  (HS2-G5WBZ5, HS2-AHS3BV). Prettier owns web source layout: `// prettier-ignore` is a lint
  error (`no-warning-comments`), and `@stylistic/max-len` rejects code lines over 200
  characters outside strings, templates, regexes, URLs, and comments, so minified one-line
  blocks cannot return (HS2-PR1BST).
- Web `scripts/*.mjs`, `scripts/*.mts`, and `eslint.config.mjs` run through the normal
  warning-free ESLint command with an explicit untyped Node configuration. Browser
  globals are limited to the scripts that execute Playwright page callbacks. The
  `HS2-HK6KMT` policy test checks real script inclusion and rejection of an invalid
  synthetic script through the same configuration. Shared asynchronous configuration
  loading belongs to the scoped setup hook (HS2-RY22P6). The ESLint policy runs in an
  ordered Vitest project before the parallel unit and binary projects so cold plugin
  loading keeps its default hook budget under CI load (HS2-7KXGMC). Every real input
  and invalid source still runs its complete proof under the default per-test budget
  (HS2-2SPPTQ); the fast unit command includes that policy project.
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
- The web lint command ends with `npm run ui:doctor` (`kerf-ui-doctor --full`), which enforces
  component CSS ownership between the app's own components. A stylesheet may style only the class
  blocks its component renders, plus the native and raw Web Awesome elements it authors, and a
  module renders only its own class blocks (HS2-TM6K9V). The doctor runs strict component
  ownership: `source` on every catalog entry, `implicitComponentOwnership`, shell-stylesheet
  `ownershipGroups`, and `ownershipContext: "any-package"`, with no suppressions. Kerf
  5.0.0-beta.73 closed the last gaps (`KF-D5EY24`, `KF-303MPV`), so HS2-R9GQJE deleted the former
  app check (`scripts/check-css-ownership.mjs`, HS2-EWYDH7) and its allowlist. See
  [ux-components.md](ux-components.md) for the rule set.
- Wire types in `hotsheet-types` derive serde + `ts-rs` (→ TypeScript for the Kerf client;
  Swift generation added for the native client).
- Kerf component props that represent CSS lengths, flex shorthands, or colors use the
  typed builders from `@kerfjs/ui/css-values`; raw CSS strings are not passed through
  component APIs. Product colors are application-owned custom properties referenced with
  `colorVar()`. Components without a raw `style` escape hatch, such as `ListItem`, use
  finite `data-*` states and public component tokens in static CSS (HS2-KAWND3).

### 12.6.1 Optional local Rust compiler cache

Rust contributors can use [`sccache`](https://github.com/mozilla/sccache) as an
optional local compiler cache. Its benefit depends on the command and checkout paths;
the worktree measurement in §12.7.9 did not show Rust cache reuse across checkouts.
Install it with `brew install sccache` on macOS or
`cargo install sccache --locked`, then enable it in the shell that runs Cargo:

```sh
export RUSTC_WRAPPER=sccache
sccache --start-server
```

Add the export to your shell profile if you want it enabled for every checkout. The
repository deliberately does not set `build.rustc-wrapper` in `.cargo/config.toml`:
that would make Cargo fail for contributors and automation without `sccache`
installed. `RUSTC_WRAPPER` applies unchanged to the `cargo lint` alias,
`cargo nextest run`, and `cargo llvm-cov`; inspect effectiveness with
`sccache --show-stats`. Some sandboxed agent sessions cannot start or contact the
local sccache server (`Operation not permitted`); leave `RUSTC_WRAPPER` unset in those
sessions instead of making the wrapper a repository requirement. CI continues to use
its existing Cargo cache and does not enable sccache.

Never point several concurrently edited git worktrees at one shared `CARGO_TARGET_DIR`.
A build in one worktree can then link another worktree's `hotsheet-server` (or other
workspace path crate) build: new routes 404 and the source-revision tests see whichever
worktree's `HOT_SHEET_LOCAL_SOURCE_ROOT` was built last (HS2-2PVW3W). Give each worktree
its own target directory (the default `target/` inside the worktree already does).
`sccache` can reuse compiler outputs where its cache keys match; it does not make
separate worktrees share Cargo artifacts.

`SCCACHE_BASEDIRS` is not a Rust worktree fix with the measured sccache 0.17.0:
[upstream issue #2652](https://github.com/mozilla/sccache/issues/2652) says its path
normalization covers C compilation but not the Rust hash key. Do not normalize Rust
keys alone or copy a whole target tree between worktrees without checking embedded
paths. Hot Sheet's server and CLI build scripts deliberately embed the worktree's
`HOT_SHEET_LOCAL_SOURCE_ROOT`; a reused artifact carrying another checkout's root
would defeat the source-revision safeguards. See the HS2-ZJXAXY findings in §12.7.9.

Two other local environment effects look like regressions but are not:

- A shell that inherits the local host's `HOT_SHEET_BUILD_REVISION` pin builds an
  explicitly revisioned binary that monitors no source root. The server compatibility and
  `--revision-status` tests accept that shape; unset the variable to exercise the
  source-root comparison.
- `terminal_ws` drives the workspace `hs-fake-agent` binary, which `-p hotsheet-server`
  alone does not build; run nextest with `--workspace` or first run
  `cargo build -p hotsheet-terminals --bin hs-fake-agent`.

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

Keep the ordinary development build unoptimized, including its dependencies
(HS2-DRW277). On a 1,961-ticket store, a 500-row list in that build spent about
4 ms in the index query and 50 ms encoding JSON (27 ms for compact output).
The maintainer accepted this development-only cost rather than adding dependency
optimization or changing the local server's build profile, both of which would
change build time and disk use across worktrees.

### 12.6.2.1 Build artifact retention and disk headroom

Keep a separate Cargo `target/` for each active checkout, as described above. Before
starting concurrent cold Rust builds, check free space with `df -h .` and measure the
existing targets with `du -sh target` in each checkout. Reserve the greater of 100 GiB
or 50 GiB plus twice the most recently observed growth of one cold worker target. If
there is no useful growth measurement, run only one cold build at a time and measure
its target before scheduling another. Recheck free space after each build. These are
local scheduling limits, not reasons to remove source or diagnostic evidence.

Retain an active checkout's artifacts while its tests, lint, server, or browser checks
need those binaries. For a finished isolated worktree, first confirm its ticket commit
is reachable from the shipping branch and pushed, required CI is green, and any logs or
captures needed for the ticket are attached or stored outside `target/`. In that exact
worktree, inspect `cargo clean --dry-run`, then run `cargo clean` to discard generated
artifacts. Remove the finished worktree with `git worktree remove <path>`; its branch
remains available. Apply the same integration and evidence checks before removing a
separate scratch clone. Never leave a scratch clone as the only copy of an unintegrated
commit.

In the main checkout, keep warm binaries while space permits. If headroom falls below
the reserve, measure `target/clippy` and `target/debug` separately. When no lint is
running, inspect `cargo clean --target-dir target/clippy --dry-run` and then run the
same command without `--dry-run` to reclaim the isolated Clippy artifacts. If that is
insufficient and no development build is running, inspect and run `cargo clean
--profile dev` in the same way; the next build will be cold. Recheck `df -h .` and
update the cold-build growth estimate. Do not use broad `git clean` or delete an
unscoped `target/`: ignored captures and other evidence may live there.

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

Every wiring function returns one teardown built from a Kerf `DisposerScope`
(`createScope()` from `kerfjs/scope`, adopted in HS2-PNFYCV): Kerf `delegate()`/`delegateCapture()`
disposers and Kerf UI wiring handles are retained through `lifetime.add(...)`, and native
`addEventListener` registrations pass `lifetime.signal`. Teardown runs the disposers newest
first and best-effort (a throwing disposer cannot strand the rest), then aborts the signal;
repeating it is a no-op, and the scope is refillable with a fresh signal. Groups return
`() => { lifetime.dispose(); }` because Kerf types `dispose` as an unbound method (KF-9AH3G8).
The UX demo catalog uses the same scope for its delegated listeners (HS2-J26QXQ). `wireHotSheetInteractions` composes the twelve
group teardowns into one page-lifetime teardown that the runtime returns as
`disposeInteractions`, so no delegate disposer is discarded (HS2-NZT3MT).

Delegated selectors are Kerf `attr()`/`action()` specs, never literal `[name="value"]`
strings. Each interaction domain owns one module under `src/interaction-attrs/`
(for example `PROJECT_LIFECYCLE_ACTIONS` for `data-action` values and
`PROJECT_LIFECYCLE_TARGETS` for components, named fields, and flags); handlers register
`SPEC.selector` and the components that render a literal attribute spread `SPEC.attrs`,
so markup and handler share one definition. `interaction-wiring.test.ts` resolves each
spec back to its selector string and checks it against `registration-inventory.txt`.

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
onboarding, failure identities, and empty/refilled sessions. A mixed-source startup
scenario holds the full provider page while the local page renders, then releases it
and checks that both sources appear and a user's intervening view choice survives.
The server HTTP suite checks that `source=git`
pages and cursors never call an external provider.

### 12.6.4 Web feature state and presentation owners

Fifteen factories under `clients/web/src/features/` own cohesive state/controllers and
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

HS2-K7SYHQ continued the split with four more owners lifted verbatim out of
`app/runtime.tsx`:

- `drive-conversations.ts`: per-project tool connections, persisted transcripts, drafts,
  the start generation guard, and drive / sidebar-chat start, send, stop, and refresh.
- `claim-clock.ts`: the local ETA countdown tick and lease-expiry wake-up timers.
- `corrupt-ticket-recovery.ts`: per-file recovery state and reveal / queue-repair actions.
- `terminal-names.ts`: browser-local in-flight names, serialized server writes, resets,
  reconciliation, and live `terminal_renamed` events.

HS2-3JGWTV lifted three more behavior-preserving owners out of `app/runtime.tsx`:

- `workspace-search.ts`: Kerf's token search model projection, the debounced server search,
  boolean-expression paging, sidebar per-view match counts, and the partial-source warning.
- `project-close.ts`: the project-close confirmation queue, live-resource inventory,
  confirm / close-everything / cancel, and the borrowed drawer terminal hand-back.
  Runtime teardown of a closed project (`closeProjectIds`) stays in the runtime as a callback.
- `project-change-streams.ts`: one WebSocket/long-poll change stream per open project, its
  event fan-out, and the debounced repository refresh; the runtime's background-refresh flag
  crosses the port through a setter. Its `dispose()` (called from the runtime's
  `disposeInteractions`) stops every stream and pending repository refresh and ignores later
  syncs (HS2-A9E7QB).

HS2-ZJ67VE lifted the terminal dashboard snapshot owner:

- `terminal-dashboard.ts`: per-project terminal fetches merged by snapshot version, AI
  connection grace states and their local re-derive timer, terminal visibility scopes and
  persistence, and the remembered drawer tab order. It reads the later-built
  `workspaceTerminalGroups` lazily, and its `dispose()` runs from `disposeInteractions`.
  Drawer visibility, focus, and terminal creation stay in the runtime for now.

HS2-T1DX2V lifted the terminal drawer geometry and phone viewport owner:

- `terminal-drawer-geometry.ts`: the drawer's measured minimum/maximum height, maximize
  toggle, and settle pass (which dispatches the drawer resize-end event), plus the phone
  terminal viewport: focus-mode enter/viewport-change/exit transitions, visual-viewport and
  keyboard geometry (published only on change), and the magnified column count. It is
  created right after the drawer signals and reads the later-built `viewportMobile` signal
  lazily; `currentMobileViewportGeometry()` is module-level so the geometry signal's initial
  value needs no owner. The remaining runtime split (drawer visibility/selection/focus,
  terminal and AI-chat creation, `closeDrawerTabIds`, surface-prop builders and
  `HotSheetShell`, `refreshProject` and board paging, `closeProjectIds`) was tracked as
  `HS2-H2R2JB`.

HS2-H2R2JB lifted the per-column board paging owner:

- `board-column-paging.ts`: the per-column board refresh spec (which keeps each paged
  column's loaded length), the bulk status-change refill begin/finish pair, and one
  column's Load more (status-stream cursors, deduped row append, partial-source warning).
  The column page/loading signals, their visible-limit merge, reset, and per-project cache
  stay in the runtime; it is created after the workspace search owner and reads the
  later-declared `refreshProject`, `loadNextTicketPage`, and `visibleTickets` through
  callbacks. The remaining split (drawer visibility/selection/focus, terminal and AI-chat
  creation, `closeDrawerTabIds`, surface-prop builders and `HotSheetShell`,
  `refreshProject`, `closeProjectIds`) is tracked as `HS2-DCTS07`.

HS2-DCTS07 lifted the pure project-teardown helpers:

- `project-teardown.ts`: `pruneProjectRecords` drops the closing ids from each per-project
  record signal (in the runtime's original order, before and after the selection change),
  and `nextProjectAfterClose` picks the surviving tab that takes the selection (nearest to
  the right, then the left, then the first). `closeProjectIds` keeps the orchestration:
  closing server sessions, cancelling refreshes, activating the next project, and
  refreshing. The remaining split (drawer visibility/selection/focus, terminal and AI-chat
  creation, `closeDrawerTabIds`, surface-prop builders and `HotSheetShell`,
  `refreshProject`, the `closeProjectIds` orchestration) is tracked as `HS2-BSSWGV`.

HS2-BSSWGV lifted five more behavior-preserving owners:

- `terminal-drawer.ts`: drawer visibility (mount, close transition, remembered open state),
  item selection, tab and input focus requests (`pendingTerminalFocus`,
  `drawerInputFocusGeneration`), the serialized `queueTerminalCreate` chain behind
  terminal, shell-command, and drawer AI-chat creation, opening a terminal or grid chat
  in its project, and `closeDrawerTabIds`. The drawer signals and size observers stay in
  the runtime. Earlier-built owners (drive conversations, AI configuration, terminal
  dashboard) reach the drawer through lazy wrappers, and the terminal viewport owner reads
  `pendingTerminalFocus` through the controller.
- `project-refresh.ts`: `refreshProject` and its stale-response generation (local writes
  invalidate it through `invalidate()`), the git-only first paint, and the authoritative
  merge into rows, counts, board pages, selection, corrupt tickets, repository status,
  and the warm projection (`CachedProjectProjection`).
- `project-teardown.ts` (`createProjectTeardown`): the `closeProjectIds` orchestration and
  ticket-reader disposal, next to the pure DCTS07 helpers.
- `settings-surface.tsx` and `sidebar-surface.ts`: the Settings workspace and left-sidebar
  projections.

`refreshProject` and `closeProjectIds` stay hoisted runtime functions that delegate to their
owners, so the owners built earlier keep calling them.

HS2-K0VC2C moved the remaining surface-prop builders and both root components into
`features/` factories with the same `createSidebarSurface` shape: a typed
`*Dependencies` port, and a returned object holding the original function names, so
call sites are unchanged.

- `ticket-view-surface.tsx`: view titles, the view action spec, and the ticket context menu.
- `terminal-operations-surface.ts`: the terminal operations dashboard summary.
- `inspector-surface.ts`: the selected-ticket inspector, the not-working dialog props,
  and attachment items.
- `reader-layers-surface.tsx`: reader layers, gallery source lookup, and the reader stack.
- `workspace-surface.tsx`: the main workspace pane and its empty states.
- `terminal-rail-surface.tsx`: the terminal rail, including its docked inspector.
- `hot-sheet-shell.tsx` (`HotSheetShell`) and `hot-sheet-app.tsx` (`HotSheetApp`).

The ports list exactly the runtime bindings each builder reads; their types come from the
checker-inferred runtime declarations. The runtime calls each factory where the original
declaration stood (builders just before `HotSheetShell`), so no dependency is read before
its declaration. Mutable runtime `let` state crosses into `HotSheetApp` through getters
(`backgroundProjectRefresh`, `initialProjectRestoreComplete`, `ticketRenderScheduled`)
and `consumeSkipNextTicketMotion`. The render-only `renderedTicketCollectionKey` now lives
in the app factory. `app/runtime.tsx` keeps the state, owners, actions, and the factory
wiring.

Owners created before these read them through lazy accessors (saved views read
`availableSearchTags`, the project lifecycle reads `syncProjectChangeStreams`).

Owners created before the AI configuration owner read it through a lazy accessor so no
binding is touched before its declaration.

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

### 12.6.5 Web search reachability audit (HS2-GT58KA)

Trace production entry points before treating a tested export as a shipped feature.
The former global search overlay was removed in HS2-0094CQ; its old unit assertions
did not establish reachability after that change. The HS2-GT58KA inventory is:

- Removed `searchScopeChoices`, `searchFilters`, `addSearchFilter`, and the
  scope/filter branches of `filterAdvancedSearchResults`. No production caller used
  the overlay controls; the three callers passed `all` and an empty filter list.
  Local Boolean and lifecycle expression filtering remains for inline search.
- Removed `searchMatchLabel`, `isExactTicketSlug`, and the hidden-status fallback
  branch. Only the removed overlay's result-label flow or tests reached them. Exact
  ticket-reference navigation remains in the compact link chooser.
- Retained `SearchScopeToolbar`, `ticketSearchScope`, and inline token parsing.
  The workspace and ticket rail render and wire these current controls; their
  view-versus-all scope is separate from the deleted overlay scope choices.
- HS2-YQF129 traced and removed seven test-only helpers:
  `fromTokenSearchTokens`, `endAttachmentLabelEdit`, `duplicateOutcomeLabel`,
  `reconcileRememberedProjectRoots`, `discardProjectSettings`,
  `formatTicketLinkReference`, and `isAppRegionId`. Their surrounding production
  modules and live behavior remain.
- HS2-BVA3RQ removed the test-only `startProjectChangePoll` wrapper after moving
  its long-poll transition tests to the shipped `startProjectChangeStream` fallback.
  `runProjectChangePollTransport` remains the shared transport for the main
  thread and worker paths.

The earlier global-overlay promise in [06-clients.md](06-clients.md) now agrees with
the later single-surface decision. The search feature row in
[TEST-COVERAGE.md](TEST-COVERAGE.md) no longer claims overlay-only match labels.

## 12.7 Testing strategy

Follows the project's philosophy (double coverage; transition-matrix testing for
stateful modules; adversarial pass; a manual test plan) — adapted to Rust +
distributed + git. **Coverage is a floor, not a ceiling:** the behavior / property /
transition-matrix tests are the real assurance.

**Risk tiers (HS2-GC3D2X, HS2-EJ8JTG).** Per-ticket testing effort scales with one
authoritative `tier:T0`, `tier:T1`, or `tier:T2` ticket tag. Set it when declaring
risk; explain the reason in `## Preliminary thoughts`, or the claim note for
trivial work. A reviewer raises the tier by changing the tag:

- **T0** (trivial): lint, typecheck, and directly related tests.
- **T1** (normal): affected tests only, plus one regression test per fix and double coverage
  for new features.
- **T2** (high-risk): analyze the state and failure paths, record the relevant
  checks and reasons, then run that analyzed selection without a time cap.

The T2 surfaces are the merge driver, claims and leases, index reconcile, the sync engine,
the file-format parser, the terminal-sizing arbiter, auth/credentials/TLS, data migration,
shared components, and the shared guidance templates. The list is kept in `AGENTS.md`.
This HS2-specific tier-tag and analyzed-selection policy supersedes the shared
Testing Philosophy rule's note-only tier and full-affected T2 wording. T0/T1 use
the checked-in five-minute planning budget; deeper tags add checks. CI covers
locally deferred checks for every tier. A missed defect becomes a prioritized
follow-up bug that identifies the local selection that missed it. The bundled
testing guidance (`plugins/shared/testing-philosophy.md` v6) otherwise carries the same
tiers for every project; each project names its own T2 surfaces.
The completion `## Confidence` verification factor is judged against the tier.

**Targeted reruns (HS2-AEE51Y).** After a failure, agents rerun only the failing tests, for
example `nextest -E 'test(<name>)'`, `vitest <file> -t '<name>'`, or `playwright -g '<title>'`,
until they pass. They then run the ticket's affected set once. A rebase onto commits that do
not touch the ticket's area needs no rerun; an overlapping rebase reruns only the affected
tests of the overlap. A passing suite is never rerun just to double-check. The bundled
instructions (v61) carry the same rule.

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

Per the AGENTS.md mandate, enumerate states **and** transitions and walk realistic
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

- **CLI launch fixture artifact (HS2-B14HYW):** before package-scoped CLI tests,
  build once with `cargo build -p hotsheet-server -p hotsheet-cli --bins --features
hotsheet-aitools/test-permission-vault`, then run `cargo nextest run -p hotsheet-cli`.
  A temporary home does not isolate the OS credential store. The actual sibling
  server must report `isolated_permission_vault: true` through the hidden read-only
  `--test-fixture-capabilities` probe; CLI launch fixtures refuse a default or stale
  artifact before starting it. Fixture-owned server startup also fails before opening
  stores or native credentials when the compiled isolation capability is absent.
  The feature is debug-only; a test-owner environment value never enables a backend
  or changes normal release/native credential behavior. A later default-feature build
  can replace the sibling server, so repeat the matching fixture build before tests.
- **Desktop fixture AI inventory (HS2-T9TGKQ):** attachment/session fixtures use the
  actual empty-inventory contracts (`GET /ai-tools` → `200 []`, `GET /ai-settings` →
  `404` with the server's error shape). Held/faulted API responses remain available;
  ticket, storage and attachment operations still use the isolated real server. This
  includes an HTTP-only isolation probe: stop the owned synthetic upstream before any
  page launch or project open, then verify canonical responses and idempotent cleanup.
  The actual-parent recovery/byte flow remains a separate test. This avoids installed-tool
  discovery consuming the browser's origin connections during rapid project changes. Production discovery/restore blocking remains HS2-XZ3TTH;
  fixture isolation does not fix that product risk.
- **Desktop fixture byte transport (HS2-ZMVQR4):** the desktop draft fixture's owned
  Hono listener forwards project API requests to its isolated real ticket server with the
  original raw body, metadata and explicit fixture authentication. Keep its held-response
  gates, injected failures and bodyless HTTP statuses intact. Playwright's WebKit routed
  request exposed no File post data in the measured flow, so `route.fetch` forwarded zero
  bytes despite the browser File and recovery bridge both holding the expected bytes.
  Do not reconstruct fixture upload bytes from expected test data. Direct Create and
  recovery-relaunch regressions assert exact real stored binary/Unicode-name bytes;
  the shared fixture's full parent suites run in Chromium and WebKit. This is a fixture
  transport correction, not evidence of a production browser File or recovery defect.
- **Unattended credential fixtures (HS2-SFVWN5):** automated tests never require an
  interactive OS credential prompt. The explicit debug-only
  `hotsheet-aitools/test-permission-vault` artifact includes both the permission vault
  and a synthetic native credential port. Its server fixture hook activates both;
  feature-enabled CLI fixtures additionally require `--test-fixture-credentials`.
  Ordinary default debug builds and all release builds retain native credentials;
  environment values cannot activate fixture mode in production. Never install or
  run a feature-enabled fixture server as a personal host. Synthetic native items
  live privately under the fixture's explicit `HOTSHEET_HOME/test-native-credentials`,
  retaining real machine-envelope encryption and cross-process recovery behavior.
  Server probes must report both `isolated_permission_vault: true` and
  `isolated_native_credentials: true`; CLI probes report the latter. The read-only
  probe opens no store or credential backend. Old permission-only artifacts fail
  preflight before fixture state is created. Browser commands, quiet Rust/browser
  tiers, CI, and Vitest's build-once setup request the matching feature; freshness or
  `HOTSHEET_VITEST_BUILD=skip` never substitutes for the actual capability check.
  Default `desktop:test` exercises the staged bridge/runtime with synthetic debug
  backends. `desktop:test:native` explicitly opts into staged release backends and
  may require native credential access; it is excluded from unattended suites.
  The portable CLI permission-discard flow now runs on macOS too. Linux's mocked
  `secret-tool` adapter remains default coverage. The real Windows Credential
  Manager roundtrip is ignored by default and runs only through explicit
  `cargo test -p hotsheet-ticketing windows_credential_manager_round_trip -- --ignored`
  on the dedicated disposable Windows CI runner.
- **Native CRT directive evidence (HS2-1R2RGQ):** after every native step, the
  Windows Credential Manager job runs `node scripts/windows-crt-directives.mjs`, even when
  an earlier step failed. It runs `dumpbin /ARCHIVEMEMBERS /DIRECTIVES` over bounded cached candidates:
  build-script `.lib` outputs under `target/debug/build/*/out`, the selected vcpkg
  `x64-windows-static-md` dav1d archives (release and debug), and native `-sys` crate rlibs.
  It reports candidate archive members requesting `MSVCRT`, `MSVCRTD`, `LIBCMT` or `LIBCMTD`,
  plus its `RuntimeLibrary` mismatch tag, using relative artifact identities, counts and
  timing. Cached outputs can include obsolete build hashes: this is not the current linked
  Cargo graph. Current-link membership must be confirmed with native link evidence before
  naming the active warning owner. The report explicitly records that link confirmation is
  absent, and includes validated GitHub source SHA/run id/attempt when available; unavailable
  provenance is listed explicitly. Only the intended report file is cleared before probing,
  then a fresh started/completed/failed report replaces it, including missing-tool/no-input
  failures, so a cached earlier report cannot be uploaded as this run. The JSON report is uploaded as the `windows-crt-directives` artifact. A
  missing `dumpbin`, an empty artifact set, or every artifact being unreadable fails the
  step explicitly. This step diagnoses the LNK4098 `MSVCRTD` warnings for HS2-PN8D9F; it
  changes no CRT, linker or warning setting. Whether candidates request `MSVCRTD` or not, the
  next native step is `LINK /VERBOSE:LIB` on one affected link.
- **Current-link CRT correlation (HS2-9E879K):** after the directive report, the same job runs
  `node scripts/windows-crt-link-correlation.mjs`. It relinks only one affected target,
  `cargo rustc -p hotsheet-ticketing --test file_lock_process -- -C link-arg=/VERBOSE -W linker_messages`.
  Only that unit's fingerprint changes, so no dependency or cache entry is rebuilt or deleted.
  Plain `/VERBOSE` is used because `/VERBOSE:LIB` lists only the libraries searched, not the
  `Loaded <archive>(<member>)` selections. The script reads the linker output from rustc's
  `linker_messages` diagnostics in Cargo's JSON stream and records:
  - the libraries searched, the members loaded per archive, the `/DEFAULTLIB` libraries processed,
    and the LNK4098 emissions;
  - the join of the loaded members with the HS2-1R2RGQ candidates, matched `exact`, `basename` or
    `ambiguous`. A bare archive name is attributed to the `Searching` block that loaded it, and two
    cached archives sharing a file name stay `ambiguous`.

  `currentLinkConfirmed` requires complete validated source SHA, run id and attempt matching the
  directive report, plus that same link emitting an `MSVCRTD` LNK4098 and loading an exact normalized
  archive/member match for an `MSVCRTD` requester. Archive identity must retain a recognized
  `target/`, `vcpkg/` or `repo/` root; outside-root basename sanitization cannot establish identity.
  Case, separators and dot segments are normalized. Unique basename and ambiguous matches remain
  advisory, never positive confirmation. Stale, missing, malformed or explicitly unavailable
  provenance fails before cargo runs, replacing any earlier positive report. Both local and CI
  root Node gates run the ticket-named regression suite and its registration guard.
  Cargo package ids are opaque: an offline `cargo metadata --no-deps --format-version=1 --offline`
  lookup resolves the exact requested package and test target before the one relink. Missing or
  nonunique metadata produces a fresh explicit failure. Confirmation requires a matching
  `compiler-artifact` with `fresh: false` in that invocation and one attributed linker diagnostic
  containing both the warning and selected member. Diagnostics from other packages, targets or
  sources, cached artifacts, missing attribution and multiple possible confirming diagnostics cannot
  establish a current link. Separate compiler diagnostics are never pooled for confirmation.
  `candidateMatches` summarizes all captured Cargo linker output as advisory. Only the bounded
  `currentLinkMatches` list identifies members supporting a positive fresh same-diagnostic result;
  it is empty when that authority is unavailable. A cached target is explicitly unconfirmed; the
  verbose rustc arguments alone do not guarantee a fresh relink on every repeated invocation.
  The directive producer retains the existing sanitized member display and adds
  `memberIdentity: {version: 1, sha256: "<64 lowercase hex digits>"}` to each scanned request and
  summarized requester. The digest uses the complete member spelling before privacy reduction,
  removing the archive offset prefix and normalizing separators, dot segments and case. The consumer
  hashes the full selected member before sanitizing its display. An exact match requires both digests;
  exact match records also retain the allowlisted digest so the two reports can be audited without a raw path;
  distinct directories with the same member basename stay distinct. Legacy, absent or malformed
  identities remain advisory. No raw member path, Cargo package id or target source path is persisted.
  The report is identifier and metric only,
  with relative identities, bounded lists with totals and validated run provenance, and no raw
  linker log. It follows the same fresh lifecycle: an explicit failure for a missing, unreadable or
  incomplete directive report, an unstartable `cargo`, or no verbose linker output. It is uploaded
  as the `windows-crt-link-correlation` artifact. It names candidates for HS2-PN8D9F; it is not a
  CRT fix or a warning-free result.

- **Native Windows coordination (HS2-9TD8Q5):** the Windows Credential Manager job
  explicitly runs `cargo test -p hotsheet-ticketing --test file_lock_process` and
  `cargo test -p hotsheet-cli --test coordination --test coordination_portable`.
  These synthetic process tests exercise busy/nonblocking locks, release on abrupt
  process exit, duplicate wait refusal, filesystem wake/reconnect, retained handoff,
  and refill while earlier CI is pending. Pipe/channel readiness and native child
  process handles avoid Unix signal assumptions and fixed readiness delays. Each
  synthetic repository sets `core.autocrlf=false` locally so worktree and cherry-pick
  byte assertions retain LF regardless of the developer or Windows runner defaults.
  Native
  Windows evidence requires the actual Windows job to pass; a macOS run or the
  credential-adapter tests alone do not establish it.
- **Shared fixtures ("helpers to always use"):** a `TempStore` builder (temp git repo
  - seeded tickets), a `TestServer` harness (real server + temp store on an ephemeral
    port), and an in-memory adapter set for pure unit tests.
- **Machine-home isolation (HS2-T9VXEP):** tests and repros must never write the real
  `~/.hotsheet2` (`checkouts.json`, `settings.json`, `settings-locks`). Hot Sheet-launched
  agent shells export `HOTSHEET_HOME` as that real home, so inheriting the environment is
  not isolation. `.cargo/config.toml` forces `HOTSHEET_HOME=<repo>/target/test-hotsheet-home`
  for every process cargo or nextest starts (a test needing a fresh home still passes its
  own temp directory), and
  `hotsheet-plugins`'s `test_processes_use_the_repository_test_home_not_the_real_machine_home`
  fails if that guard is removed. The same config forces
  `HOTSHEET_CACHE_DIR=<repo>/target/test-hotsheet-cache`, so a server test that keeps the
  default cache root never writes posters into the real platform cache (HS2-SDPD95). The
  forced home is **shared by every test binary running in parallel**, so tests that write
  or depend on machine-home state inject their own: setup tests pass an explicit plugin
  search path (`run_setup_in` / `refresh_setup_in` / `find_in`, or
  `AppState::with_plugin_dirs`) instead of the `<HOTSHEET_HOME>/plugins` default, and
  `crates/hotsheet-plugins/tests/setup_isolation_guard.rs` fails when test code calls a
  default-path setup wrapper, sets `HOTSHEET_HOME` with `set_var`, or posts `/setup/<tool>`
  without an injected plugin path (HS2-SDPD95). The migrator suite's `globalSetup`
  (`migrator/test/isolated-home.mjs`) gives its spawned binaries a throwaway home, guarded
  by `migrator/test/isolated-home.test.mjs`; the web real-server fixtures already pass
  temp homes. Manual repros that run `target/debug/hotsheet-cli` or the server must export a
  temporary `HOTSHEET_HOME` first; running from a temp directory alone still registers the
  checkout in the real registry.
- **Global Git config isolation (HS2-M269HC):** `.cargo/config.toml` forces
  `GIT_CONFIG_GLOBAL=<repo>/.cargo/test-gitconfig` for every process cargo or nextest
  starts. That file gives tests a synthetic identity like CI's, and sets
  `maintenance.auto=false` and `gc.auto=0`. Without it, Git starts detached
  `git maintenance run --auto` children after fetch, push, commit and receive-pack (788 in
  one full suite), which can outlive a test and surface as a nextest `LEAK`, and tests read
  each developer's global settings (signing, hooks, credential helpers).
  `GIT_CONFIG_GLOBAL` survives `git::command`'s repository-environment scrub and Git's local
  transports, unlike `GIT_CONFIG_COUNT`. `hotsheet-ticketing`'s
  `hs2_m269hc_tests_use_the_repository_global_git_config` fails if the guard is removed.
- **Installed AI-tool isolation (HS2-7W3D7V):** a test that starts a server or CLI with
  the inherited `PATH` would otherwise let AI-tool discovery execute the developer's real
  tools (`codex --version` and the app-server `model/list`, `opencode models`,
  `agy models`) with their real credentials and network access. `.cargo/config.toml`
  forces `HOTSHEET_AI_TOOL_PROBES=off`, so discovery still detects installed tools on
  `PATH` but keeps their bundled offline catalogs and executes nothing.
  `crates/hotsheet-aitools/tests/probe_isolation.rs` fails if the guard is removed. Tests
  that probe their own fixture tools (`antigravity_discovery.rs`, the `graceful_shutdown`
  slow-discovery fixtures) remove the variable explicitly. Browser and Node fixtures do not
  receive cargo's `[env]`, so `clients/web/vitest.config.ts`, `clients/web/playwright.config.ts`
  and the migrator's `globalSetup` (`migrator/test/isolated-home.mjs`) set it before any
  worker starts, through `clients/web/scripts/test-ai-tool-isolation.mjs` on the web side.
  `clients/web/scripts/test-ai-tool-isolation.test.mjs`, the real-server browser fixture
  (`tests/real-ticket-server.ts`) and `migrator/test/isolated-home.test.mjs` fail without
  it (HS2-F6R2HQ).
- **Session-identity isolation (HS2-ZWBSH1):** the same launched shells also export their
  session identity (`HOTSHEET_PERMISSION_PROJECT`, `HOTSHEET_PROJECT`, `HOTSHEET_SERVER`,
  `HOTSHEET_SECRET`, `HOTSHEET_TERMINAL_ID`, `HOTSHEET_AGENT`) and the agent harness exports
  its own (`CLAUDE_CODE_MESSAGING_*`, `CLAUDE_CODE_EXECPATH`, `AI_AGENT`). `.cargo/config.toml`
  cannot remove a variable, and forcing an empty value would change the hook's fallbacks,
  so permission-hook fixtures in `crates/hotsheet-cli/tests/cli.rs` build their child with
  `hook_cli()`, which removes those variables before the fixture sets its own.
  `hs2_zwbsh1_hook_fixtures_ignore_a_poisoned_hot_sheet_session` reruns every hook fixture in
  a child test process with synthetic poisoned values (the parent environment never
  changes), and `hs2_zwbsh1_every_hook_fixture_uses_the_isolated_child` fails when a new
  hook fixture bypasses the helper or the rerun list. Toolchain and home settings stay
  inherited.
- **Local push gate (HS2-CD3D2T):** Run `scripts/install-git-hooks.sh` once per
  checkout, then `scripts/push.sh origin main` for each verified ticket commit.
  The checked-in pre-commit hook runs staged whitespace and format checks.
  `scripts/check.sh` runs the Rust, root Node, web, migrator, and spike checks
  before `git push` opens a remote connection; the pre-push hook runs the same
  check for direct pushes. A clean tree that passed is cached under Git's local
  metadata by tree id. `scripts/push.sh` passes its verified tree to the hook so
  the gate runs only once even when unrelated local files are dirty. The root
  gate requires `cargo-nextest`, `git-filter-repo`, and installed npm packages.
  Permission integration tests use an in-memory vault under the test-only dependency
  feature, so the same full Rust suite runs on macOS and Linux (`HS2-R8S894`).
- **CI (GitHub Actions) — built:** the `check` job runs `fmt --check`, `cargo lint`,
  `nextest`, web client lint/typecheck/unit/build, spike lint/typecheck/build, the plugin
  **conformance test** (HS2-64), the CLI
  build, and the **migrator vitest + coverage** (now `test:coverage`, gated on the
  per-language thresholds in `migrator/vitest.config.mjs`) + lint + the cross-language
  conformance. Ubuntu AVIF build dependencies are installed using Ubuntu package
  sources only, so unrelated runner-image apt mirrors cannot block these jobs
  (HS2-EFZZE8). A separate **`coverage` job** collects Rust coverage once
  (`cargo llvm-cov nextest --no-report`), uploads an lcov artifact (a _separate_
  per-language summary, not one merged lcov), and **gates** on a conservative line
  floor (`report --fail-under-lines`). A scheduled, creds-gated **`Live tier`**
  workflow (`live.yml`, nightly + manual dispatch) runs the `#[ignore]` live
  codex/claude turns (`HOTSHEET_CODEX_LIVE`/`HOTSHEET_CLAUDE_LIVE`) only on a runner
  flagged `HOTSHEET_LIVE_RUNNER`, keeping the default tier fast. GitHub attachment
  live tests (`crates/hotsheet-extsync/src/github_live_tests.rs`: upload round trip and
  crop-manifest CAS) run there when `HOTSHEET_GITHUB_LIVE_REPO` and
  `HOTSHEET_GITHUB_LIVE_ASSETS_REPO` name approved non-production repositories, and skip
  otherwise; the 1/10/100 batched-manifest measurement also needs
  `HOTSHEET_GITHUB_LIVE_MANIFEST_SCALE=1`. Locally the token comes from the keychain key
  `github-live`. Opt-in live mutation-latency benchmarks
  (`crates/hotsheet-extsync/tests/provider_latency.rs`, `HOTSHEET_REAL_PROVIDER_BENCH=1`)
  may only edit designated throwaway issues: the Jira ones are `HS2T-1`..`HS2T-3`
  (summary prefix `[HS2 benchmark]`, label `hs2-benchmark`), named in the local `.env` as
  `HOTSHEET_BENCH_JIRA_ISSUES` / `HOTSHEET_BENCH_JIRA_ISSUE`.
- **Local live-provider settings (HS2-5PC0WA):** live-provider tests read their settings
  from a gitignored `.env` at the repo root whose secret values are 1Password references
  (`op://…`), resolved only at run time with
  `op run --env-file=.env -- cargo nextest run -p hotsheet-extsync --all-features --run-ignored only -E '<filter>'`.
  Never commit resolved values. The variables, which mirror the `live.yml` Actions
  secrets, are: Jira `HOTSHEET_JIRA_LIVE_BASE_URL`, `HOTSHEET_JIRA_LIVE_EMAIL`,
  `HOTSHEET_JIRA_LIVE_PROJECT` (the `HS2T` test project), `HOTSHEET_JIRA_LIVE_TOKEN`
  (classic API token, Basic auth), and the operator-prepared reopen pair
  `HOTSHEET_JIRA_LIVE_REOPENED_ISSUE` / `HOTSHEET_JIRA_LIVE_REOPENED_EXPECTED` (a score or
  `none`), and the attachment pair `HOTSHEET_JIRA_LIVE_WRITE=1` /
  `HOTSHEET_JIRA_LIVE_ATTACHMENT_ISSUE` (an operator-prepared issue that receives test
  attachments, HS2-KE18XX); GitLab `HOTSHEET_GITLAB_LIVE_TOKEN`, `HOTSHEET_GITLAB_LIVE_PROJECT`,
  `HOTSHEET_GITLAB_LIVE_WRITE`; benchmark targets `HOTSHEET_BENCH_JIRA_ISSUES`,
  `HOTSHEET_BENCH_JIRA_ISSUE`, `HOTSHEET_BENCH_GITLAB_ISSUE`. In CI the same names come
  from repository Actions secrets; a test whose variables are unset skips.
  The reopen pair is prepared once by the opt-in helper `jira_live_prepare_reopened_issue`
  (HS2-JNM42S), which needs `HOTSHEET_JIRA_LIVE_WRITE=1` and refuses any issue outside
  `HOTSHEET_JIRA_LIVE_PROJECT`. It creates an `[HS2 benchmark] reopened-history` issue
  (label `hs2-benchmark`; optional `HOTSHEET_JIRA_LIVE_ISSUE_TYPE`, default `Task`), posts a
  Hot Sheet scored comment, takes the issue's own workflow transitions to Done, back to a
  not-done status, and to Done again, optionally posts a later scored comment
  (`HOTSHEET_JIRA_LIVE_REOPENED_LATER_SCORE`), and prints the two `.env` lines. Without a
  later score the expectation is `none`, because the pre-reopen score must not survive:
  `op run --env-file=.env -- cargo nextest run -p hotsheet-extsync --all-features --run-ignored only -E 'test(jira_live_prepare_reopened_issue)'`.
  The normal `check` job
  runs a bounded Chromium Playwright smoke group covering deep links, gallery save retry,
  image/video freehand strokes, feedback replies, popup anchoring, concurrent autosave,
  bulk provider projection, and ticket context menus (HS2-28MBS9, HS2-ZHN7XS). Locally, agents run
  only the specs a ticket affects; the full browser suite runs in CI (below, HS2-D95TXJ).
  **Pending (HS2-FPXSD0):** raise measured coverage
  floors, close remaining web coverage gaps, and add the macOS matrix leg for
  terminal/native-client surfaces.
- The web gate installs Playwright Chromium and its Linux dependencies after `npm ci`
  with `npx playwright install --with-deps chromium`. The smoke job bounds an
  attempt at 10 minutes, and the full-suite shards bound one at 12 minutes;
  ordinary failures retry, while a timeout stops the job before an orphan apt
  child can lock subsequent attempts (HS2-47BAAC, HS2-EN9D1C). Ubuntu jobs prefer
  Canonical's archive for these dependency downloads, with the runner's Azure
  archive as fallback (HS2-M983JG). The Vitest suite includes real-browser
  local-host and stable-dev tests, so browser setup is required before `test:unit` as well
  as before the Playwright smoke group. The `binaries` Vitest project's `globalSetup`
  (`clients/web/scripts/vitest-build-binaries.mjs`) runs
  `cargo build -p hotsheet-cli -p hotsheet-server --bins` once before any worker starts,
  unless the binaries are already fresh (§12.7.9).
  Test files never build these binaries themselves: a build inside one file relinked them
  while parallel files were spawning them, causing intermittent `ENOENT`/"not built"
  failures (HS2-A0M8CM). A cargo build run outside the suite against the same `target/`
  can still swap binaries mid-run, so do not build concurrently with a full
  `test:unit`. `npm run test:ci:browser` starts its own local
  Vite server, selects tests marked `@ci-smoke`, and uses two browser workers with a
  12-minute CI timeout. The video scrub smoke case uses the checked-in four-second,
  30-fps H.264 fixture at `clients/web/tests/fixtures/video-scrub.mp4` to check decoded
  frames across paused seeks and held-pointer movement (HS2-DE781D). It waits for the
  final requested frame because a multi-step drag also decodes intermediate positions.
  `npm run test:e2e` still runs the full browser suite locally when a change warrants it.
- **CI as the shared full-suite batch gate (HS2-QCBEQF).** Each ticket runs the local
  tests its own diff affects at its declared risk tier, then commits and pushes to
  `main`. A ticket waiting for remote verification stays Started in `final_testing`;
  its note names the run and includes `CI gate commit: <full 40-character SHA>`.
  `node scripts/ci-gate.mjs status --covers <full-sha>` reads the latest successful
  `main` push run of the `CI` workflow. It reports whether that run contains the given
  commit and lists waiting tickets with explicit markers. The helper is read-only:
  GitHub Actions is the shared source of truth, and a missing local commit yields
  `unknown` until the checkout has that commit. A green run covers all ancestor
  ticket commits, even when it followed several pushes. A red run triggers targeted
  attribution across commits since the last green run; repair the ticket responsible
  for its own failure or file a prioritized bug for an independent failure. No local
  last-green file or repeated per-ticket full-suite run is needed.
- **Full browser suite in CI (HS2-D95TXJ).** The `browser-full` job runs all Playwright
  specs (Chromium plus the WebKit-launching specs) in four parallel shards. Each shard
  runs `--shard=<i>/4 --fully-parallel --workers=2 --reporter=blob`. Test-level sharding
  keeps the large `providers.spec.ts` from serializing one shard; the suite has no serial
  or `beforeAll`-coupled tests. `browser-full-report` merges the shard blobs into one HTML
  report (artifact `playwright-report-full`) and prints the slowest browser tests via
  `scripts/test-profile.mjs`. The shards install only Chromium and WebKit, cached under
  `~/.cache/ms-playwright` per Playwright version, with three bounded install attempts.
  An attempt that times out ends the job because a privileged `apt-get` child can survive
  `npx` and hold the dpkg lock; ordinary failures still retry. The full-shard timeout is
  12 minutes to cover the slow Ubuntu mirror observed in CI run `39745789604`, and the
  Chromium-only check uses a 10-minute bound (HS2-EN9D1C).
  An opt-in profiler that needs another browser (Firefox, `profile:pty:firefox`) skips at
  file level so the default suite never launches it. The shards share one Vite dev server per
  shard; its project WebSocket bridge owns every upgraded socket's errors, so a browser
  reset mid-handshake cannot crash the server and fail the rest of the shard with
  `ERR_CONNECTION_REFUSED`. The shards pin the browsers' UI font: the runner's default
  `system-ui` is DejaVu Sans, about 15% wider than the macOS and Windows UI fonts the layout
  specs are calibrated against, so `.github/fontconfig/fonts.conf` maps `system-ui`,
  `ui-sans-serif`, and `sans-serif` to the metric-compatible Liberation Sans (HS2-6H4M85;
  the Linux-default layout itself is HS2-85H820). Specs stay platform-neutral: a label the
  client derives from the platform comes from `tests/platform-labels.ts`, and shortcuts use
  `ControlOrMeta`. To reproduce a shard locally, run it in
  `mcr.microsoft.com/playwright:v<version>-noble` with that `fonts.conf` mounted at
  `/root/.config/fontconfig/fonts.conf` and `--cpus=2`. The job verifies the full suite during a ticket's
  `final_testing` handoff, so the local requirement is the affected specs only. The
  `@ci-smoke` group stays as the fast early signal. The job is required (HS2-6H4M85): a
  failing shard fails the CI run, so a ticket in `final_testing` completes only after it
  passes.
- **Bounded, visible retries in `browser-full` only (HS2-8YHY6V).** The shards set
  `HOTSHEET_PLAYWRIGHT_RETRIES=2`, which `playwright.config.ts` reads through
  `playwrightRetries()` in `clients/web/src/test-quarantine.ts` (clamped to at most 2; unset or
  unparsable means none). Local runs and the required `@ci-smoke` and `@ci-webkit` groups never
  retry, so a flake still fails them. A test that fails and then passes on a retry is
  reported `flaky`: it does not fail the job, but it is never silent. The merged HTML report
  marks it flaky, and `browser-full-report` runs `scripts/playwright-retries.mjs`, which writes
  a "Browser tests that needed a retry" table to the job summary and one warning annotation per
  test. Every test that needs a retry gets a follow-up bug ticket for its root cause; a test
  that keeps flaking is quarantined ([test-quarantine.md](test-quarantine.md)). Retries never
  replace a fix.
  A separate bounded WebKit job runs the inspector popup-anchor regression marked
  `@ci-webkit` (HS2-HASXWP), covering the Safari engine without delaying the main
  Chromium gate.
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

### 12.7.8 Quiet test runs for agents (HS2-CP63FG)

Verbose runner output costs agent tokens without adding assurance. Agents run tests
through `node scripts/test-quiet.mjs <tier>[,<tier>...] [-- runner args]`, where the tier
is `rust` (`cargo nextest run`), `web-unit` (`clients/web` Vitest), `web-e2e` (the
binary build, then `clients/web` Playwright), or `migrator` (migrator Vitest). Arguments
after `--` go to the single named runner, so targeted runs keep their filters, for
example `node scripts/test-quiet.mjs rust -- -p hotsheet-cli -E 'test(claim)'`.

- Runners omit routine successful-test output: nextest uses live `--status-level leak`
  with final `--final-status-level fail` (there is no final `leak` level), Vitest uses
  `--reporter=dot`, and Playwright uses `--reporter=line`. The root gate also uses live
  `leak` status (HS2-ET5FF1).
- The wrapper prints one line per tier with PASS/FAIL, wall time, the runner's own count
  lines, and the log path. The complete output goes to
  `target/test-quiet/<tier>-<timestamp>.log` (`HOTSHEET_TEST_QUIET_LOG_DIR` overrides).
- Named nonfailing nextest `LEAK` statuses remain in the full log and compact summary,
  preserving diagnostic evidence even on a green run. They do not change the runner's
  exit code or become failure excerpts; `LEAK-FAIL` remains a failure. Test payloads are
  not included with these named statuses. A classification alone does not establish
  a surviving process or explain historical unnamed reports (HS2-JH6KV5, HS2-ET5FF1).
- On a red run it prints each distinct failure block, capped at `--max-failures` (8)
  blocks of `--max-lines` (30) lines. A nextest block keeps its first third and its end,
  where the panic message sits. A failure with no recognizable test block, such as a
  compile error, prints the last 30 lines instead. Excerpt lines are clipped at 240
  characters; the log keeps them whole.
- The exit status is the runner's. A failed build step stops the tier before its tests run.
- Inside the Claude Code local sandbox (`SANDBOX_RUNTIME=1`), a failed `rust` or `web-e2e`
  tier and a failed `scripts/check.sh` stage end with a notice to rerun outside the sandbox
  (HS2-5PTT2Y). Seatbelt denies AF_UNIX bind/connect, PTY spawn, and file-system watcher
  events, so about 150 broker, terminal, server, session-push, launch, and coordination
  tests fail there, and coordination waiters can block until their timeout, without any
  regression. Those tests are not skipped in the sandbox, because a skipped suite would
  report success while proving nothing; run them, and the local gate, outside it. The notice
  never changes an exit code or records a receipt (`scripts/sandbox-notice.mjs`).

A primary agent may hand a test run to a cheaper background agent that returns only the
wrapper's summary. The wrapper's parsing is covered by `scripts/test-quiet.test.mjs`,
which uses real captured runner output and spawns real processes; CI runs it in the
`check` job.

### 12.7.9 Test speed: binary builds, profiling, and quarantine (HS2-3PX1E4)

- **Binary builds only when needed.** `clients/web/vitest.config.ts` splits the suite into
  `eslint-policy`, `unit`, and `binaries` projects. The `binaries` project holds the files that
  launch real `target/debug` binaries, listed in `clients/web/scripts/vitest-binary-suites.mjs`.
  Only `binaries` carries the cargo-build `globalSetup`, and Vitest runs a project's global
  setup only when that project has files in the run. A targeted run such as
  `npx vitest run src/foo.test.ts`, or `npm run test:unit:fast` (`--project eslint-policy --project unit`), never
  starts cargo. When the setup does run, it skips the build if every binary is newer than
  every build input: crates, manifests, the toolchain file, and embedded `plugins/` and
  skill files. `HOTSHEET_VITEST_BUILD=always|skip` overrides that check. The build still
  happens once, before any worker starts, so HS2-A0M8CM's no-relink-mid-run guarantee
  holds. A guard test fails when a test file launches a binary but is not in the list.
- **Measured (2026-10-16, loaded 10-core machine).**

  | Run                                                    | Before               | After  |
  | ------------------------------------------------------ | -------------------- | ------ |
  | Full `npm run test:unit`                               | 64.4 s               | 57.3 s |
  | `test:unit:fast`: 2334 of 2350 tests, no binary suites | —                    | 16.4 s |
  | One web-only file after a `hotsheet-cli` source edit   | 9.3 s (cargo relink) | 1.9 s  |

  The full-suite gain is the build check, and the binary suites set the floor:
  `local-host.e2e` took 47 s and `stable-dev.e2e` 30 s in this profile. A later two-file
  measurement on the same loaded machine measured 43.1 s and 23.2 s respectively. The
  independent local-host cases now run concurrently after their shared production build;
  independent stable-dev process fixtures overlap in bounded pairs. In the comparable
  two-file run, those file times fell to 27.3 s and 15.6 s, respectively (HS2-XSP0GJ),
  a combined 42.9 s versus 66.2 s before. These are observed file times on a loaded
  machine, not a guaranteed CI wall-time reduction.

- **Slow-test profiles.** `node scripts/test-profile.mjs --top N` lists the slowest tests
  and files from runner reports. It reads `--nextest target/nextest/default/junit.xml`,
  which nextest writes on every run per `.config/nextest.toml`; `--vitest <json>`, from
  `--reporter=json --outputFile.json=<path>`; and `--playwright <json>`, from
  `--reporter=json`. CI prints the Rust and web-unit profiles after the tests.
- **Gate timing history (HS2-TCCKRZ).** `scripts/check.sh --timings` reads bounded,
  machine-local p75 stage and test-file estimates, with sample count and local/CI
  provenance. Only a fully passing gate adds executed stages; reused receipts and
  failed or interrupted runs add none. Passing CI runner reports can seed missing
  local file estimates via `--import-ci-timings <directory>`. See
  [29-affected-local-verification.md](29-affected-local-verification.md).
- **Full-suite bottlenecks (HS2-XSP0GJ).** A recent full nextest JUnit report covered 965
  cases; the longest was the terminal shell-history isolation case at 51.4 s
  (`HS2-5YC1E5`), followed by CLI permission-hook cases at 21.8–46.5 s
  (`HS2-KV8H01`). A targeted rerun of all five permission-hook integration cases with
  loopback access on 2026-10-17 passed in 1.181 s total (1.139–1.180 s per case),
  without changing the fixtures. The long full-suite durations were not reproduced
  and should be treated as host or suite contention rather than a fixed hook wait.
  A merged four-shard Playwright JSON report from CI run 39000860513
  covered 857 cases. `providers.spec.ts` accounted for 2,389 of 4,226 summed test
  seconds, far more than any other file; `HS2-JS8Q2K` tracks splitting it so shards
  can balance its flows. These sums are test durations, including retries, rather than
  elapsed suite wall time.
- **Flaky-test quarantine.** A known flake is skipped through a marker that names its fix
  ticket and is tracked in [test-quarantine.md](test-quarantine.md). CI's
  `scripts/check-test-quarantine.mjs` keeps markers and rows one-to-one. Nobody silently
  reruns a known flake, and every quarantined test has an owner.
- **Shared compilation cache (HS2-HRE9G7).** Keep `sccache` optional at the machine
  level; do not enable it in the repository or share `CARGO_TARGET_DIR` between
  worktrees. On 2026-10-19, two isolated checkouts at the same commit ran
  `cargo check -p hotsheet-server --lib --quiet -j 4` with separate cold target
  directories. The unwrapped build took 142.43 s (warm rerun 1.01 s); the first
  `RUSTC_WRAPPER=sccache` build took 101.02 s (warm rerun 0.66 s), and the second
  checkout's wrapped build took 156.98 s. The shared sccache daemon recorded 616
  Rust cache misses and **zero Rust hits** after both wrapped builds, though it did
  reuse C and assembler outputs. Host load varied sharply, so these wall times do not
  establish a speedup. The default wrapper is therefore unproven for the intended
  cross-worktree Rust reuse. Sandboxed builds previously failed when a native build
  invoked sccache with `Operation not permitted`; the measured builds required an
  outside-sandbox run. Keep the separate target directories that protect test binaries
  (HS2-A0M8CM).
- **Cross-worktree Rust cache investigation (HS2-ZJXAXY).** In the two measured
  sccache target trees, Cargo's registry-crate fingerprints for `serde_core` and
  `serde_json` are identical, while the `hotsheet-server` fingerprint differs through
  dependency hashes. The server build-script outputs contain different absolute
  `HOT_SHEET_LOCAL_SOURCE_ROOT` values, as intended. These artifacts alone do not
  isolate every Rust cache-key input or prove that copying a target tree would make
  Cargo rebuild all path-sensitive outputs. Upstream confirms that sccache 0.17.0
  hashes Rust working directories and path-bearing arguments without applying
  `SCCACHE_BASEDIRS` ([issue #2652](https://github.com/mozilla/sccache/issues/2652));
  its proposed [Rust key normalization](https://github.com/mozilla/sccache/pull/2813)
  was still open on 2026-10-19 and explicitly warns that key-only normalization may
  return an artifact with another checkout's embedded path. Cargo's separate
  [`build.build-dir`](https://doc.rust-lang.org/stable/cargo/reference/build-cache.html)
  holds intermediate artifacts, including build-script output, so sharing it would
  reintroduce the same correctness question even with private final target dirs.
  No new wrapper, shared directory, or snapshot restore is recommended from this
  evidence. HS2-26PHYJ tracks a controlled, path-checked snapshot experiment once
  machine load permits; HS2-29RAR3 tracks an artifact-safe upstream Rust cache release.

### 12.7.10 Concurrent browser runs (HS2-BJ00D1)

Every Playwright run in `clients/web` starts its own Vite dev server on
`HOTSHEET_WEB_TEST_PORT` (default 4176), with `reuseExistingServer: false` and a fresh
Vite cache directory. Workers that verify in separate worktrees at the same time give each run
a distinct port:

```sh
cd <worktree>/clients/web
HOTSHEET_WEB_TEST_PORT=<unused port> npx playwright test <specs> --retries=0
# or: HOTSHEET_WEB_TEST_PORT=<unused port> node ../../scripts/test-quiet.mjs web-e2e -- <specs>
```

Each run also passes its own random `HOTSHEET_WEB_TEST_SERVER_ID` to its Vite child and waits
on `/__hotsheet-test-server/<id>`. Only that run's server answers 200 there, and any other
server, including one with no id, answers 404. Before this check, a run that lost the port race
to another run's starting server tested that server's worktree and lost its connection when the
other run ended (`ERR_CONNECTION_REFUSED`).

A run on a taken port now refuses with `[WebServer] Error: Port <port> is already in use` and
`Process from config.webServer was not able to start`. Choose another port and rerun the whole
affected set. A `--last-failed` pass after such a refusal is not a clean run. Runs never stop
another worker's or the user's server.

`scripts/playwright-server-identity.e2e.test.mjs` proves this with two real Vite servers:

- each answers only its own identity
- a second server on the same port exits
- stopping one leaves the other serving

`src/playwright-config.test.ts` pins the readiness URL and `reuseExistingServer: false`.

## 12.8 Cross-references

- Core / server / CLI split: [04-core-server-cli.md](04-core-server-cli.md)
- Storage + merge driver (the property-test target): [02-ticket-storage.md](02-ticket-storage.md) §2.7
- Index + reconcile (a transition-matrix target): [03-indexing-and-query.md](03-indexing-and-query.md)
- Git-native claim (integration-test target): [08-distributed-and-remote.md](08-distributed-and-remote.md) §8.5
- Terminal-sizing arbiter (a transition-matrix target): [06-clients.md](06-clients.md) §6.7
- Migrator conformance test: [07-migration.md](07-migration.md) §7.2.1
- AI-tool plugin interface + its testability rule: [05-ai-tool-plugins.md](05-ai-tool-plugins.md) §5.10

### Real ticket-server browser regression

`npm run test:e2e` in `clients/web` calls the shared `npm run test:fixtures` prerequisite
to build isolated CLI, server and terminal fixture binaries before Playwright (HS2-63ZK8W).
The required WebKit job installs pinned Rust/native build dependencies and runs that same
prerequisite in a separate step before the unchanged `npm run test:ci:webkit` command.
Focused `npx playwright test` runs can reuse those binaries after this build at the repository root:

```sh
cargo build -p hotsheet-cli -p hotsheet-server --bins --features hotsheet-aitools/test-permission-vault
```

The feature compiles the isolated permission vault and synthetic native credentials, so
unattended browser fixtures never reach the macOS Keychain (HS2-SFVWN5). A default build
does not have them, and a temporary `HOTSHEET_HOME` does not isolate native credentials.
`tests/real-ticket-server.ts` therefore checks each binary's `--test-fixture-capabilities`
before it creates any fixture state: the server must report `isolated_permission_vault` and
`isolated_native_credentials`, and the CLI must report `isolated_native_credentials`. A
default or older artifact is refused with the build command above. The harness then creates
an isolated store, checkout registry, ephemeral loopback server, and process cleanup for
browser flows that must prove real storage/classification/index behavior. Optional
`HOTSHEET_TEST_CLI_BIN` and `HOTSHEET_TEST_SERVER_BIN` paths select frozen verified
artifacts: copies of binaries built with the same feature, kept outside a shared `target/`
so another worktree's rebuild cannot replace them during a run. They pass the same
capability check. Tests that intentionally use real native credentials stay explicit opt-ins
(`desktop:test:native`).

### Local Playwright parallelism and timeouts

Responsive inspector captures wait for the shipped rail's new `data-presentation`
and intended collapsed state before clicking Show Ticket Inspector. Desktop return
also waits for the restored open preference. `setViewportSize` completing or an empty
animation list alone does not establish those states (HS2-ZV9M6R, HS2-3HJP95).
The separately named controlled-delivery regression holds one actual resize event
until the first real readiness read, then invokes the registered application callbacks
and verifies a positive delivery count. It changes no product state or attributes;
the ordinary uninstrumented real-server flow remains required in both engines.

Every Playwright worker drives a browser against the one Vite dev server, so the suite is
CPU-bound on a single machine. Locally, `playwright.config.ts` starts from half the cores and gives
back one worker per core of existing load beyond that half, down to three workers
(`scripts/playwright-workers.mjs`, HS2-MHPHZB). CI keeps Playwright's default, and `--workers`
or `HOTSHEET_PLAYWRIGHT_WORKERS` overrides the count. Tests get 60 s and assertions 10 s so a
busy machine slows a run down instead of failing it. Tests still must wait for events or
state, never for fixed delays.

Local runs also lower their own CPU priority to nice 10 before Playwright starts its workers,
browsers, and web server, which inherit it, so several parallel suites cannot starve the
machine (HS2-FVT34A). `PLAYWRIGHT_NICE` sets another niceness (`0` opts out). CI keeps its
priority unless that variable is set, and an already lower priority is never raised.

Playwright rotates its managed screenshots, traces, and failure evidence under
`clients/web/target/playwright-test-results`. Kerf UI doctor excludes this generated
directory, so lint and browser tests can run together without a directory-scan race
(HS2-ZJJ4K6). New screenshots should use `testInfo.outputPath(...)` to remain portable
and attach to the test result. Historical named captures and videos use the portable,
Git-ignored `clients/web/target/visual-captures` directory, which Playwright prepares
without rotating it (HS2-SRGAH3).

When a browser test asserts an intermediate state that only exists while a request is
outstanding, the route fixture must hold that response behind an explicit gate (a promise the test
resolves after asserting). Examples are a pending spinner, an optimistic projection, an in-flight
cancellation, or a restore screen. A fixed `setTimeout` in the route handler is not enough: a busy
machine can let it expire before the assertion runs. Route delays may remain only to simulate
latency that no assertion depends on (HS2-ZZX0CV).

HS2-AVXYCB exercises the actual reader's quoted inline answer against that server,
checks the persisted ordinary note and cleared indexed/full-ticket review state, then
reloads to confirm persistence. Provider discovery stays deterministic; ticket reads and
writes cross the real HTTP and filesystem boundaries.

### Affected local verification (HS2-RS0FFT, HS2-FCCRCK)

The required local gate is `scripts/check.sh`, backed by the dependency planner,
bounded relevant browser checks, and exact-input receipts. Inspect it with `--plan`;
use `--full` for the full package gate. Full browser/platform regression remains
in CI, with an additional full local browser run only for a concrete unresolved risk.
See [the policy, input boundaries, and measurement limits](29-affected-local-verification.md).
HS2-GN06JT adds an explicit T0/T1 five-minute planning target based on measured
stage history, never a kill timer. The mandatory floor and direct tests always run;
HS2-VQ5ND5 resolves provider-neutral ticket tags for the normal profile: an
untagged or unresolved ticket gets the T1 target, deeper tags select affected
browser/performance checks or all affected stages, and `tier:T2` stays uncapped
with an explicit analyzed selection. HS2-A4HD9X adds synthetic local ticketing
and PGLite export performance commands for non-web `testing:perf` tags; unsupported
paths fail explicitly. See the profile vocabulary and bounds in the policy.
