# 04. Core, Server & CLI

> **Status: Proposed.** The shared-core-in-Rust recommendation is the pivotal
> language decision — see [09-technology-decisions.md](09-technology-decisions.md)
> §9.2 and the maintainer-confirmation note there.

## 4.1 The shared core

`hotsheet-core` is a single library holding **all** domain logic, written once and
used by the **two Rust binaries** — the server and the CLI. Clients do **not**
link it; they are API consumers (§4.1 "Why a library"). This still avoids HS1's
problem of logic re-implemented per surface: there is exactly one implementation
of every operation, and the server (the only thing GUIs talk to) is the single
authority over it. The CLI shares it directly for its direct-to-disk path.

The core is **I/O-capable but policy-free**: it performs file and process I/O
through injected adapters, so hosts inject real implementations and tests inject
fakes. Nothing in the core decides *where* to bind a port or *whether* to open a
browser — those are host policy.

Core modules (mirrors [01-architecture.md](01-architecture.md) §1.2):
`model`, `store` (git-backed ticket files), `index` (SQLite/FTS5), `watch`,
`query`, `plugins` (AI-tool host + terminal manager + permission bridge), `coord`
(claim/lease).

### Why a library, not just a server
So the **CLI can operate directly on disk with no server running** (a chartered
requirement — "a cli for direct reading/manipulation of tickets on disk"), running
the exact same tested engine the server runs. The server and CLI are two thin Rust
binaries over one core; a `hotsheet new …` from the terminal and a create from the
GUI go through identical logic, and a running server's watcher reconciles whatever
the CLI wrote. The library boundary is what guarantees those two paths can't
drift.

> **Clients do not embed the core** (maintainer decision, 2026-08-19). Earlier
> drafts had Tauri/SwiftUI run the core in-process for local projects; that is
> **retired**. The server is always a separate process, even locally (§4.3), so
> the client/service split is absolute and the server can outlive the client.
> `uniffi` Swift/JNI bindings are therefore **not needed** — clients speak HTTP/WS
> like any API consumer. See [06-clients.md](06-clients.md).

## 4.2 Concepts carried over from HS1 (worth keeping)

From the current codebase, these are proven and carry forward — reimplemented on
the git+index foundation:

- **Zod-style wire SSOT** → a single schema module defining wire types + validation,
  shared across surfaces. (Rust: `serde` + a schema crate; generated TS/Swift types
  for clients.)
- **Data-driven AI-tool plugin registry with a purity boundary** — the cleanest
  abstraction in HS1; carried wholesale. [05-ai-tool-plugins.md](05-ai-tool-plugins.md).
- **Claim/lease ticket primitive** orthogonal to status/up_next, with a
  conflict-guarded write chokepoint, lazy reclaim, and poison quarantine.
- **PTY broker as a detached process** so terminals survive a server restart.
- **Byte-stream busy inference** (spinner detection) rather than tool-specific
  busy APIs.
- **Worklist-as-file** contract + MCP tools proxying the same core.
- **WS sync + long-poll fallback** for live updates.
- **Tiered auth**: loopback secret → trusted-origin → mTLS for exposed binds.

Dropped: the PGLite cluster and its whole crash-recovery apparatus (snapshots,
backups, repair, cluster eviction, lock recycling-PID guards) — obsoleted by
git + a rebuildable index ([02-ticket-storage.md](02-ticket-storage.md) §2.9).

## 4.3 The server (`hotsheet-server`)

> **Status: v1 built (HS2-7).** `crates/hotsheet-server` — axum HTTP REST
> (`/health`, `/tickets` list/create, `/tickets/{id}` get/patch,
> `/tickets/{id}/close`) + `/ws/sync` live push, over the shared engine
> `ops`, with **Tier 0** auth (`X-Hotsheet-Secret`, loopback only — off-loopback
> binds are refused until mTLS). It serves reads from the **SQLite/FTS index** (HS2-5)
> and owns the **filesystem watcher** (HS2-6) that reindexes changed files + emits
> change events, so a CLI/git edit shows up live. It does **not** yet own terminals
> (HS2-10), the detached lifecycle/auto-start (HS2-59), or the long-poll fallback.
> MCP is a separate shim, not served here (§5.8).

A thin binary that wraps the core and is the **always-on service** every GUI talks
to — **local use included**. It runs completely independently of any client (a
chartered goal, made absolute by the maintainer 2026-08-19: there is no
embedded-in-client mode). One instance per machine serves all local projects.

Responsibilities:
- **HTTP REST** for CRUD + query (JSON, not HTML — the client/service split).
- **WebSocket** (`/ws/sync`) for live push: index changes, claim/lease events,
  terminal streams, permission prompts. Long-poll fallback for environments
  without WS.
- **MCP** endpoint(s): the `hotsheet_*` tool surface for AI tools
  ([05-ai-tool-plugins.md](05-ai-tool-plugins.md) §5.8).
- **Owns the filesystem watcher** (→ incremental reindex) and the **terminal/PTY
  manager** (both need a persistent host).
- **Tiered auth** (§4.6).

Stack (Rust recommendation): `axum` + `tokio`, `tower` middleware for auth/rate
limiting, `rusqlite` for the index, `notify` for watching, `portable-pty` for
terminals. Runs as a plain HTTP server on loopback by default; `--bind` off-box
flips it into the mTLS tier or refuses to start.

### 4.3.1 Lifecycle — independent, auto-started, survives the client

The server's life is **decoupled from any client's** (maintainer requirement,
2026-08-19):

- **Started explicitly** by `hotsheet serve`, **or auto-started by a client.** When
  a client (Tauri, SwiftUI macOS) launches and finds no server, it **spawns one
  detached** — not as a child tied to the app's lifecycle, but as a background
  process (double-fork / `setsid`, or a launchd/systemd user service) — then
  connects over HTTP/WS.
- **Survives the client closing.** Because it's detached, quitting the app leaves
  the server running (in-flight AI work, terminals, and the watcher keep going).
  This is the whole point of the separation and the direct reversal of HS1's
  Tauri-sidecar model, where the server died with the app.
- **Join-don't-collide.** A per-project lock (index writer) + a global instance
  file (`~/.hotsheet/instance.json`, holding the port) let a second launch — a CLI,
  another client, a second app window — **discover and join** the running server
  instead of starting a duplicate. Carried over from HS1's instance model, minus
  the DB-lock recycling complexity (there's no DB cluster to protect, only the
  disposable index).
- **Supervised, not owned.** A client may *supervise* the local server it started
  (restart it if it crashed, surface its health) but never *owns* it — any client
  can attach to a server another client started, and none closing it takes it down.
- **Shutdown is explicit** — `hotsheet serve --stop` / a tray/menu "Quit server"
  action / OS service stop — never an implicit side effect of a client exiting.
- **On iOS** there is no local server to auto-start (background-execution limits
  make an independent daemon impractical); iOS connects to a *remote* server on a
  Mac. See [06-clients.md](06-clients.md) §6.4.

Build/design detail for this lifecycle is tracked in **HS2-59**.

## 4.4 The CLI (`hotsheet-cli`)

A thin binary that wraps the same core for **direct-to-disk** operations, usable
with **or without** a running server. AI tools can use the CLI *or* the MCP; humans
use it in a terminal.

Two families of commands:

**Ticket ops (direct to disk; server not required):**
```
hotsheet new "Fix dashboard flicker" --category bug --priority high --up-next
hotsheet ls --up-next --status started
hotsheet search "flicker"                 # FTS via the local index
hotsheet show HS-7f3k9q
hotsheet edit HS-7f3k9q --status completed --note "fixed the pre-theme paint"
hotsheet claim-next --worker worker-1     # coordination primitive
```
These write ticket files directly (and commit, configurable). The CLI reads via a
**direct store scan** — it does **not** touch the index; the index is the *server's*
read cache, and there's no reader when no server runs. If a server is running, its
watcher observes the file change and reindexes + broadcasts, so a CLI edit shows up
live in every open client; if not, the server **reconciles** the index against the
files on its next start (`Index::open_reconciled`), so offline CLI/git edits are
picked up then. A manual rebuild is just deleting the index file — the server
recreates it — so the CLI needs no SQLite dependency of its own.

**Ops / lifecycle:**
```
hotsheet init          # create/register a project + default store (+ install the merge driver)
hotsheet serve         # run the server
hotsheet reindex       # drop + rebuild the index from disk
hotsheet doctor        # diagnose: store health, merge-driver registration, index drift, plugin detection
hotsheet merge-driver  # git-invoked semantic 3-way merge for ticket files (02-ticket-storage.md §2.7)
```

The **HS1→HS2 migrator is a separate, disposable bundled tool** (may be Node),
**not** part of this long-lived CLI — it runs once per old project and is retired
once data is moved. Invoke it directly (`hotsheet-migrate <old-project>`) or via the
UI prompt; see [07-migration.md](07-migration.md). (If we ever want a `hotsheet
migrate` alias, it just shells out to that bundled tool.)

`hotsheet merge-driver` is not run by hand — it is the binary git calls for the
`merge=hotsheet-ticket` driver registered in each store's `.gitattributes`. It
lives in the CLI so the format-aware merge logic is the **same core code**, tested
once, that powers automatic conflict resolution. `hotsheet init` writes the
`.gitattributes` line and configures the local git repo to use it.

`clap` (Rust) for arg parsing. The CLI and server share the core, so an operation
behaves identically whichever path invokes it — a key maintainability win over
HS1, where the CLI and server were the same Node process but the client re-derived
logic separately.

## 4.5 How the three stay consistent

There is exactly one implementation of every operation (in the core). The server
and CLI are its only two adapters, and both call the same functions; clients call
those operations *through the server's API*, never a re-implementation. This is
enforceable the way HS1 enforces its structural rules (an ESLint-style lint / a
Rust crate boundary): **domain logic may not live outside `hotsheet-core`.** A
"client mirror" of core logic is a bug, not a pattern.

## 4.6 Auth & trust tiers (carried from HS1, simplified)

- **Tier 0 — loopback:** plain HTTP + per-project shared secret. The default; the
  single-user local case. Trusts localhost.
- **Tier 1 — exposed:** binding off-loopback requires **mTLS + per-device client
  certs + ACLs**, or the server refuses to start (never serves plaintext). The
  entire HS1 §94/§97 model (per-project CA, `.p12`/QR enrollment, revocation
  sweep) is carried over — it's shipped, proven, and orthogonal to the storage
  rewrite. See [08-distributed-and-remote.md](08-distributed-and-remote.md).

## 4.7 Open items
- **Language confirmation** (Rust vs Go) — [09-technology-decisions.md](09-technology-decisions.md) §9.2.
- **MCP transport for the new server** — HS1 spawns a separate `channel.ts` MCP
  process per project. In HS2 the plugin host may let the server itself expose MCP,
  or keep a small per-project MCP shim; decided in [05-ai-tool-plugins.md](05-ai-tool-plugins.md) §5.8.

## 4.8 Cross-references
- Storage: [02-ticket-storage.md](02-ticket-storage.md) · Index: [03-indexing-and-query.md](03-indexing-and-query.md)
- Clients (API consumers) + server auto-start lifecycle: [06-clients.md](06-clients.md) §6.2
- Language/runtime decisions + server-separation ADR: [09-technology-decisions.md](09-technology-decisions.md) §9.1e, §9.2
