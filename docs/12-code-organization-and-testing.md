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
    hotsheet-plugins/             # AI-tool plugin host + permission bridge. Depends on ticketing.
    hotsheet-terminals/           # PTY manager + broker client + busy inference. Nearly standalone
                                  #   (needs project cwd/config, not the ticket index).
    hotsheet-types/               # wire/API types (serde) + client codegen (ts-rs → Solid; later Swift)
    hotsheet-server/  (bin)       # axum/tokio — HTTP/WS/MCP + watcher + terminal host
    hotsheet-cli/     (bin)       # clap — ticket ops + init/serve/reindex/doctor + merge-driver
    hotsheet-ptybroker/ (bin)     # the detached PTY broker
  clients/
    web/                          # Solid SPA (the Tauri webview UI and the browser build)
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
  on `plugins` or `terminals` — that keeps `hotsheet-cli` a tiny binary
  (maintainer's requirement: ticketing is separable from terminals/AI-hosting because
  the CLI needs none of the latter).
- **`hotsheet-plugins`** depends on `ticketing` (it reads the worklist, claims,
  updates tickets).
- **`hotsheet-terminals`** is nearly standalone (project cwd/config, not the ticket
  index) — which is what keeps the future **process split** (a separate durable
  terminal server) cheap. See §12.4.

## 12.3 Async model — sync core, async at the edges

**Decision:** the domain crates are **synchronous**; async lives only where it's
inherent.

- **`model` + `ticketing` are plain synchronous code** — easy to read and test
  (no `#[tokio::test]`, no runtime), and matching the reality that **rusqlite and
  local `gix` are synchronous**. Making them async would only hide `spawn_blocking`
  inside them while taxing pure logic with async "coloring" — no real concurrency
  win, since the disk/SQLite work blocks either way.
- **The server wraps ticketing in one thin async facade** that centralizes
  `spawn_blocking`, so route handlers read cleanly (`ticketing.get_ticket(id).await`)
  while the blocking-boundary boilerplate lives in exactly one place.
- **SQLite concurrency maps onto our "one index writer" rule** ([03](03-indexing-and-query.md)
  §3.8): **WAL mode + a pool of read connections + a single serialized writer.** Both
  dispatched through the facade's `spawn_blocking`.
- **Async only at the inherent edges:** the axum HTTP/WS server, the
  `notify`→channel watcher bridge, terminal byte streams, and **git-remote via
  `tokio::process`** (subprocess) so a slow push never ties up a blocking-pool thread.

## 12.4 Git access — gix local, git CLI network

**Decision:** two backends behind one `GitLocal` / `GitRemote` adapter split.

- **Local ops (`gix`, pure Rust):** commit-on-write, `diff old..new HEAD` for the
  incremental-reindex fast path ([03](03-indexing-and-query.md) §3.4), history. No
  subprocess overhead on hot paths, no native C dependency.
- **Network ops (shell out to the `git` binary):** fetch, push, the claim
  compare-and-swap (`--force-with-lease`), `ls-remote`. The CLI uses the user's real
  git config / credential helper / SSH agent automatically — exactly what the HS2-63
  spike proved for the claim CAS ([08](08-distributed-and-remote.md) §8.5). gix's
  network/push side is less mature, so this is also the pragmatic choice.
- **Per-op fallback to the `git` CLI** for any local op that is thin in gix.

## 12.5 Terminal process topology — separable crate, split deferred

**Decision:** terminals are their **own crate** now (so a full process split is cheap
later), but we **start with one ticket+terminal server + the detached PTY broker**
([05](05-ai-tool-plugins.md) §5.4). PTYs live in the broker and **survive a server
restart**; a restart only briefly drops terminal WebSockets (clients reconnect). A
fully separate, independently-restartable **terminal server** is revisited when
survivability needs are concrete — the crate boundary makes it a later, cheap change.

## 12.6 Conventions

- Errors: `thiserror` in libraries, `anyhow` at binary edges; `Result` throughout.
- Logging: `tracing` structured logs.
- Workspace-level dependency versions; `rustfmt` + `clippy -D warnings` enforced in CI.
- Wire types in `hotsheet-types` derive serde + `ts-rs` (→ TypeScript for Solid;
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
- **Client E2E:** the Solid web UI via **Playwright** against a real running server;
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

## 12.8 Cross-references
- Core / server / CLI split: [04-core-server-cli.md](04-core-server-cli.md)
- Storage + merge driver (the property-test target): [02-ticket-storage.md](02-ticket-storage.md) §2.7
- Index + reconcile (a transition-matrix target): [03-indexing-and-query.md](03-indexing-and-query.md)
- Git-native claim (integration-test target): [08-distributed-and-remote.md](08-distributed-and-remote.md) §8.5
- Terminal-sizing arbiter (a transition-matrix target): [06-clients.md](06-clients.md) §6.7
- Migrator conformance test: [07-migration.md](07-migration.md) §7.2.1
