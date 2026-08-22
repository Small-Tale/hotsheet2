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
    hotsheet-extsync/             # External-sync plugin host + providers (GitHub/GitLab/Jira).
                                  #   Deps: ticketing + HTTP.  NO terminals.  (docs/16)
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
- **One crate per plugin *type*** (maintainer, 2026-08-19), named `hotsheet-<type>`,
  each a registry of that type's plugins with its **own conformance suite** — they do
  not share a mega-crate, so each pulls only its own deps:
  - **`hotsheet-aitools`** (AI-tool plugins) depends on `ticketing` **+ `terminals`**
    (it drives agents in PTYs). Holds the drive/metrics/activity/permission/mcp/… host.
  - **`hotsheet-extsync`** (external-sync plugins) depends on `ticketing` **+ HTTP
    clients**, **not** `terminals` — so a `hotsheet sync` path never drags in the
    terminal/agent machinery.
  - Future plugin types get their own `hotsheet-<type>` crate the same way.
  - The **pattern** is shared across all of them (declarative identity + behavioral
    half, injected adapters for testability §12.7, a conformance gate) even though the
    *interfaces* differ; extract a shared `hotsheet-plugin-core` only if a third type
    reveals real common machinery — don't pre-abstract.
- **`hotsheet-terminals`** is nearly standalone (project cwd/config, not the ticket
  index) — which is what keeps the future **process split** (a separate durable
  terminal server) cheap. See §12.5.

> **§12.3–§12.5 are implementation-choice *decisions*.** They shape the crate APIs and
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
detached PTY broker; a fully separate terminal *process* is a later, cheap change the
crate boundary preserves. Decision + rationale: [09](09-technology-decisions.md) §9.14.

## 12.6 Conventions

- Errors: `thiserror` in libraries, `anyhow` at binary edges; `Result` throughout.
- Logging: `tracing` structured logs.
- Workspace-level dependency versions; `rustfmt` + `clippy -D warnings` enforced in CI.
- Wire types in `hotsheet-types` derive serde + `ts-rs` (→ TypeScript for the Kerf client;
  Swift generation added for the native client).

---

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

### 12.7.4 Cross-language conformance (the migrator's guard)

A CI test where the real `hotsheet-model` **parses and round-trips what the Node
migrator wrote**. That single test removes the drift risk and lets the migrator stay
a simple, disposable standalone tool ([07](07-migration.md) §7.2.1).

### 12.7.5 Coverage — per-language gates + an aggregate

A single literal merged report across Rust + TS + (later) Swift is impractical, so:

- **Per-surface gates in CI:** `cargo-llvm-cov` (Rust), Playwright/istanbul (web),
  `vitest` (migrator) — each with its own high threshold.
- **An aggregate summary dashboard** rolls them up; there is **no fake merged lcov**.
- The property / transition-matrix / behavior tests are the real bar — a green
  coverage number is necessary, not sufficient.

### 12.7.6 Fixtures & CI

- **Shared fixtures ("helpers to always use"):** a `TempStore` builder (temp git repo
  + seeded tickets), a `TestServer` harness (real server + temp store on an ephemeral
  port), and an in-memory adapter set for pure unit tests.
- **CI (GitHub Actions):** `fmt --check`, `clippy -D warnings`, `nextest`, the
  conformance test, web E2E, coverage — split into a **fast tier** and a **full/live
  tier** (GitHub-remote + creds-gated), mirroring HS1's split.
- **Manual test plan** (`docs/manual-test-plan.md`, created with the first code):
  real multi-device terminal-sizing focus handoff (iOS↔macOS), mTLS enrollment / QR
  pairing across devices, native-client UX, long-term GitHub custom-ref behavior —
  migrated to automation as we can.

### 12.7.7 AI-tool integration testing (the HS1 Codex pain, designed out)

Adding an AI tool to HS1 (Codex, when it started Claude-only) was a heavy **manual**
effort. The root cause: HS1 conflated two very different questions into one manual
test — **"does *our host* handle the protocol correctly?"** (automatable) and **"does
the *real tool* actually speak that protocol?"** (real-tool drift). Splitting them is
the whole strategy: ~95% becomes deterministic automation; the ~5% drift check is a
small, explicit layer. Build: **HS2-64**.

**1 — Testability is a plugin-interface rule.** Every side-effecting interaction a
plugin performs goes through an **injected adapter** (`ProcessSpawner`, config-file
writer, `PermissionTransport`, `McpConfigWriter`, `Clock`). No plugin touches a real
process, file, or global directly. This is the non-negotiable that makes everything
below deterministic — see [05-ai-tool-plugins.md](05-ai-tool-plugins.md) §5.10.

**2 — `hs-fake-agent`: a scriptable test double.** A workspace test binary that speaks
the *same protocols a real tool does*, but scripted and deterministic. It can be told
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
  (concurrent requests preserved), WS push, answer routed to the *originating*
  connection, allow-once/always → persisted rules; each `permissions` capability's
  install-then-remove leaves foreign hook entries intact (merge-safety).
- **Terminal integration** — the terminal manager against the fake agent: scrollback,
  multi-viewer attach, sizing arbitration, OSC parsing, **survival across a broker
  restart**; the `command` capability is a pure resolve-the-launch-line test.
- **Busy-state monitoring** — a **transition-matrix** test: feed scripted hook signals
  *and* byte-stream spinner glyphs; walk busy→sustained→idle, stale-clear, the
  spinner-liveness gate, dropped-Stop-hook recovery.

**4 — A conformance suite over every plugin, as a hard CI gate.** One suite
parameterized over the plugin registry, run against a temp fixture project — identity,
instructions, skills, command (injected spawner), drive, permissions (merge-safety),
MCP config. **A new tool inherits the entire suite by existing**, and can't merge
until it passes conformance *and* the fake-agent E2E. This is the forcing function
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
- **Opt-in live smoke** — a tiny per-tool suite that runs the *real* binary
  (creds-gated, nightly / pre-release) for the end-to-end sanity a recording can't
  give.

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
