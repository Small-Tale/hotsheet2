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
`query`, `plugins` (AI-tool **plugin loader + registry**, setup/instruction/skill/
MCP writers, terminal manager, permission bridge — [05](05-ai-tool-plugins.md) §5.1a,
§5.11), `settings` (shared/local project settings — §4.9), `coord` (claim/lease).

Because `plugins` and `settings` are in the core, **both binaries** drive them: the
CLI runs the one-shot half headless (`hotsheet setup`, `hotsheet plugin …`,
`hotsheet settings …`) with no server, and the server runs the same code for
client-driven flows and hosts the persistent half (terminals, drive, busy). This is
the §4.5 no-duplication rule applied to setup + settings, not just ticket ops — and
it's the reversal of HS1, where the *app layer* owned tool setup and project config.

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
> `ops`, with **Tier 0** auth (`X-Hotsheet-Secret`) on loopback and **Tier 1 mTLS**
> (per-project CA + per-device client certs, HS2-VT3JMF) on off-loopback binds — see
> §4.6. It serves reads from the **SQLite/FTS index** (HS2-5)
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

**Built (HS2-59, server-side):** `hotsheet-server::lifecycle` implements the
machine-local **instance registry** (`${HOTSHEET_HOME:-~/.hotsheet2}/instances/
<project-id>.json` — not `~/.hotsheet`, which HS1 owns, HS2-104), **discovery**
(`find_instance`, validating the recorded pid is alive so a crash leaves no false
positive), the **per-store index-writer lock** (a second server is refused / told to
attach; a stale lock from a dead server is reclaimed), and **stop** (`hotsheet-server
--stop` → SIGTERM). `serve` takes the lock, writes the instance file (removed by a
guard on **graceful shutdown**: SIGTERM/Ctrl-C), and if a live server already serves
the store it **prints how to attach and exits** instead of duplicating. E2E-verified.
The **client-side** half — a client spawning the server *detached* and *supervising*
it — lands with the clients (**HS2-4072GM**); the server is already a separate process,
so a detached spawn makes it outlive the client by construction.

## 4.4 The CLI (`hotsheet-cli`)

A thin binary that wraps the same core for **direct-to-disk** operations, usable
with **or without** a running server. AI tools can use the CLI *or* the MCP; humans
use it in a terminal.

> **Binary name (dev):** the compiled binary is currently **`hotsheet-cli`**, not
> `hotsheet`, to avoid colliding on `PATH` with a separately installed Hot Sheet 1
> `hotsheet` launcher on developer machines. The examples below use the conceptual
> `hotsheet` name; substitute `hotsheet-cli` when running against this repo's build.
> (A final shipped name is a release-time decision.)

Two families of commands:

**Ticket ops (direct to disk; server not required):**
```
hotsheet new "Fix dashboard flicker" --category bug --priority high --up-next
hotsheet ls --up-next --status started
hotsheet search "flicker"                 # FTS via the local index
hotsheet show HS-7f3k9q
hotsheet edit HS-7f3k9q --status completed --note "fixed the pre-theme paint"
hotsheet edit HS-7f3k9q --blocked-by HS-abc123 --blocked-by HS-def456   # set blockers (slug|ULID)
hotsheet edit HS-7f3k9q --clear-blocked-by                              # remove all blockers
hotsheet claim-next --worker worker-1     # coordination primitive
```
`--blocked-by` (repeatable, on `new` and `edit`) takes a slug **or** ULID and is
resolved to a ULID, rejecting unknown tickets and self-references; on `edit` a present
`--blocked-by` **replaces** the set and `--clear-blocked-by` empties it. The same edge
is settable over the API and MCP: `blocked_by` (an array of slug/ULID strings) on
`hotsheet_create` / `hotsheet_update` and the server's `POST` / `PATCH /tickets` — on
update a present `blocked_by` replaces the set (`[]` clears), absent leaves it. All
surfaces share one resolver (`ops::resolve_blockers`), mirroring how `duplicate_of` is
resolved on close.
These write ticket files directly and **auto-commit** each mutation to the store's git
repo, then best-effort push (HS2-VJD1W4) — so a headless `work` run (or any CLI/MCP edit)
never leaves the store dirty or unshared; the shared `ops` layer routes every mutation
through `FsStore::write_ticket_committing`, so CLI + MCP + server all commit. It's a no-op
when the store isn't a git repo, and `HOTSHEET_NO_AUTOCOMMIT` disables it for batch work.
Aggressive fetch/rebase/merge-on-conflict is the sync engine (`docs/03`; HS2-19); the
semantic merge driver (§2.7) resolves concurrent edits. The CLI reads via a
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

**AI-tool setup + plugins (core-owned, headless — [05](05-ai-tool-plugins.md) §5.1a, §5.11):**
```
hotsheet setup claude          # write CLAUDE.md/skills/MCP config for a tool   [built: HS2-98]
hotsheet setup --detect        # set up every AI tool detected on this machine   [built: HS2-98]
hotsheet plugin list           # installed + detected AI-tool plugins            [HS2-92]
hotsheet plugin install <path|url>   # add an external plugin (trust-gated)      [HS2-93]
hotsheet plugin verify <id>    # run the conformance suite against a plugin      [HS2-93]
hotsheet plugin remove <id>                                                    # [HS2-92]
```
These run **with no server and no client** — the loader + setup writers live in the
core (§4.1), so a purely terminal workflow prepares a project for its AI tools on
its own. When a client is in play it asks the *server* to run the same code.
`setup <tool>` is **built (HS2-98)**: it writes a merge-safe managed block into the
tool's instruction file (e.g. `CLAUDE.md`), the worklist skill, and an `.mcp.json`
entry registering the serverless `hotsheet-mcp --path <store>` (an **absolute**
`hotsheet-mcp` path when one sits next to the CLI, so it works without PATH munging —
HS2-117); re-running refreshes the managed pieces in place. The permission-bridge
install + the `hotsheet plugin` management commands are still to come.

**Drive a tool (the headless "play") + the work loop:**
```
hotsheet trigger <tool> [--prompt …] [--project DIR] [--mcp-config F] [--env K=V]
hotsheet work <tool> [--max 50] [--max-stall 3] [--project DIR] [--worker]
```
`work` is the **headless loop (HS2-118)**, the north-star bootstrap step: it drives the
tool one turn at a time — each turn takes the single highest-priority Up Next ticket —
until Up Next is drained, a turn cap (`--max`) is hit, or the queue stops changing for
`--max-stall` turns (a **thrash guard**, so a stuck tool doesn't spin forever). It reuses
`trigger`'s HS2-103 launch safety and exits cleanly (no setup required) when nothing is
Up Next. Each turn currently spawns a fresh process; cross-turn session resume is
**HS2-3C1XK3**.

`trigger` launches/injects one turn into an AI tool and streams it (HS2-109). It is
**safe by default (HS2-117 launch safety):** it prepends a `hotsheet` → `hotsheet-cli`
shim (plus the CLI's own dir) to the tool's PATH so a bare `hotsheet` can't hit an HS1
launcher and kill the dev instance (§4.4); refuses to run when the project holds an
HS1 store (`assert_no_hs1`) or when the tool isn't set up; and defaults `--mcp-config`
to the tool's project config so the tool can reach **only** the Hot Sheet MCP (Claude
via `--strict-mcp-config`). Codex reads its MCP servers from `$CODEX_HOME`, so `trigger`
**auto-builds a throwaway MCP-free `CODEX_HOME`** for it (HS2-YRDQNX): a copy of the
user's `auth.json` plus a `config.toml` whose only server is the Hot Sheet shim — so a
bare `trigger codex` can't load the user's global MCP servers (pass `--env CODEX_HOME=…`
to override). The safety primitives live in `crates/hotsheet-cli/src/launch_safety.rs`.

**Project settings (core-owned; shared + local scopes — §4.9):**
```
hotsheet settings get <key> [--scope shared|local]
hotsheet settings set <key> <value> [--scope shared|local]
hotsheet settings list [--scope shared|local]
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
  certs**, or the server refuses to start (never serves plaintext).
  See [08-distributed-and-remote.md](08-distributed-and-remote.md).

  > **Built (HS2-VT3JMF):** a **per-project CA** (`hotsheet-tls`, rcgen) signs the
  > server leaf + every device's client cert; `hotsheet-cli cert init|issue|revoke`
  > manages it (material under `${HOTSHEET_HOME}/tls/<project-id>/`, machine-local —
  > the CA key is a secret, never committed). An off-loopback `serve` bind loads a
  > rustls `ServerConfig` and serves over a manual `tokio-rustls` acceptor (axum 0.8
  > has no built-in TLS), **requiring** a client cert that chains to the CA and isn't
  > on the `revoked` fingerprint list; loopback stays Tier-0 plaintext. Chain
  > validation is delegated to rustls's `WebPkiClientVerifier` (not hand-rolled), with
  > a revocation gate layered on. A full mTLS-handshake E2E proves valid-in /
  > no-cert-out / revoked-out. **Deferred:** revocation **hot-reload** (a running
  > server snapshots the list at start — revoke then restart), `.p12` bundling + QR
  > enrollment (client work), and richer per-identity **ACLs** beyond CA membership.

## 4.7 Project settings (shared / local / client) — core-owned

> **Built (HS2-94, HS2-34):** `hotsheet_ticketing::settings::Settings` — a flat
> `key -> JSON` map per scope: **global** `${HOTSHEET_HOME}/settings.json`
> (machine-wide, store-independent), **shared** `hotsheet-settings.json` (committed
> beside the store) and **local** `hotsheet-settings.local.json` (auto-added to
> `.gitignore`). The effective value resolves in precedence **global < shared < local**
> (most specific wins). Driven headless by `hotsheet-cli settings get|set|list
> [--scope global|shared|local]`. Client/device-only settings still never enter core.
>
> **Decided (maintainer, 2026-08-20):** project settings are **core-owned and
> CLI-manageable**, not app-only. The client owns *only* device-specific settings.
> Build: **HS2-94**.

Settings split by **scope**, which maps directly onto the already-decided
shared-vs-local on-disk model ([README](README.md); [02-ticket-storage.md](02-ticket-storage.md)
§2.11). Each scope has a clear owner and a clear on-disk home:

| Scope | Examples | On disk | Managed by |
|---|---|---|---|
| **Global** | cross-project personal defaults (default AI tool, editor) set once per machine | **`${HOTSHEET_HOME}/settings.json`** (machine-wide, not tied to a store) | core → **CLI + server** |
| **Shared** | auto-context guidance (HS2-25), categories, per-category instructions, custom views, enabled-plugin set for the *project* | **committed** in the store repo (travels with the project) | core → **CLI + server + client** |
| **Local** | which tools are enabled *on this machine*, index location, machine paths | **gitignored** overlay beside the store (machine-local, not device-app-local) | core → **CLI + server**; client via API |
| **Client / device-only** | window geometry, theme, per-viewer PTY size prefs (§6.7) | the client's own app storage | **client only — never enters core** |

The dividing test: *does a headless CLI or the server ever need this value?* If yes,
it's shared or local and lives in core-owned settings. If it only means something to
a running GUI on one device, it's client-only and the core never sees it. This is
why `hotsheet settings` (§4.4) can manage the first two scopes with no client at all,
while a window position stays out of the core entirely.

Shared settings are versioned by the same git as tickets (diffable, mergeable via
the same driver where sensible); local settings are disposable machine state. The
`plugins` module reads the enabled-plugin set from settings to decide what `hotsheet
setup` writes ([05](05-ai-tool-plugins.md) §5.1a).

## 4.8 Open items
- **Language confirmation** (Rust vs Go) — [09-technology-decisions.md](09-technology-decisions.md) §9.2.
- **MCP transport for the new server** — HS1 spawns a separate `channel.ts` MCP
  process per project. In HS2 the plugin host may let the server itself expose MCP,
  or keep a small per-project MCP shim; decided in [05-ai-tool-plugins.md](05-ai-tool-plugins.md) §5.8.

## 4.9 Cross-references
- Storage: [02-ticket-storage.md](02-ticket-storage.md) · Index: [03-indexing-and-query.md](03-indexing-and-query.md)
- AI-tool plugins (loader, setup ownership, external plugins): [05-ai-tool-plugins.md](05-ai-tool-plugins.md) §5.1a, §5.11
- Clients (API consumers) + server auto-start lifecycle: [06-clients.md](06-clients.md) §6.2
- Language/runtime decisions + server-separation ADR: [09-technology-decisions.md](09-technology-decisions.md) §9.1e, §9.2
