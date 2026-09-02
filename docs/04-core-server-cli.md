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
`model`, `providers` (normalized contract, routing/capabilities, default git-backed
implementation), `index` (SQLite/FTS5), `watch`, `query`, `plugins` (AI-tool
**plugin loader + registry**, setup/instruction/skill/
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

### Checkout identity and discovery

A checkout is Hot Sheet's machine-local identity for one canonical working-directory
path; it is not a git object, ticket-store identity, or credential. Its readable id is
`<folder>-<12 path-hash hex>`, so nearby projects can discover it without receiving a
secret. `${HOTSHEET_HOME}/checkouts.json` records optional repository identity and zero or
more ticket stores, permitting many-to-many mappings. CLI `checkout
register|list|resolve`, authenticated server `/checkouts` routes, MCP list/resolve tools,
and `setup` registration share this registry. Only server instance records contain a
bearer token; those files are user-readable only on Unix.

Checkout-qualified `/checkouts/{id}/tickets` routes aggregate reads across every linked
hosted store. Get/update/close require the ticket to resolve uniquely; create selects the
only linked store or requires an explicit store id when several are linked. MCP ticket
tools accept the same optional `checkout` target (and `store` for ambiguous creates) in
both HTTP and serverless modes.

Checkout corrupt diagnostics also expose a safe repair-ticket action. It revalidates the
reported path, routes the generated work item to the affected store, and returns the
existing open repair item on repeated requests. The API schedules recoverable work for an
AI worker; it does not synchronously rewrite or delete an unreadable source file.

`POST /projects/open` is the client onboarding transaction: it validates a code checkout,
accepts explicit git-store paths or discovers an exact sibling `<checkout>.hs2` store,
hosts those stores, and registers the checkout links. Discovery only accepts a directory
containing `hotsheet-store.json`; it never creates a store or assumes a checkout has only
one source. An empty discovered source set remains valid at the core layer so a richer
client can present provider setup.

### Headless platform APIs

The authenticated server exposes checkout-scoped repository snapshots at
`/checkouts/{id}/repository/status`, current ticket-flow aggregates at
`/analytics/tickets`, and settled-plus-live usage totals at `/analytics/usage`.
Historical cumulative-flow data is explicitly reported unavailable because current ticket
files do not preserve status transitions.

Checkout-scoped code review is exposed at
`GET /checkouts/{checkout}/tickets/{ticket}/code-review`. Discovery walks at most the
newest 2,000 reachable commits and associates a commit only when the ticket slug is a
bounded token in its subject; body-only mentions are cross-references, not ownership.
Adjacent matching commits form reviewable ranges without spanning unrelated work. The
response also reports the checkout's Git `diff.tool`. The matching `POST` accepts only a
single commit or adjacent range returned by a fresh discovery, rejects arbitrary refs,
and starts `git difftool --no-prompt` with an argument array in the registered checkout.
The client never receives a program path or executes a review command itself.

Project command settings use exact `program` + `args` arrays. `/commands` only starts a
configured id; requests cannot supply arbitrary shell text. Bounded run history includes
cursor-addressable stdout/stderr lines and cancellation. `/notifications` persists a
bounded server-process routing feed with checkout/store/ticket targets, deduplication, and
acknowledgement while also publishing live events. `/tts/synthesize` accepts text,
provider id, and voice only; provider adapters and their credential resolution remain in
the server process.

> **Status: v1 built (HS2-7).** `crates/hotsheet-server` — axum HTTP REST
> (`/health`, `/tickets` list/create, `/tickets/{id}` get/patch,
> `/tickets/{id}/close`) + `/ws/sync` live push, over the shared engine
> `ops`, with **Tier 0** auth (`X-Hotsheet-Secret`) on loopback and **Tier 1 mTLS**
> (per-project CA + per-device client certs, HS2-VT3JMF) on off-loopback binds — see
> §4.6. It serves reads from the **SQLite/FTS index** (HS2-5)
> and owns the **filesystem watcher** (HS2-6) that reindexes changed files + emits
> change events, so a CLI/git edit shows up live. Terminal hosting, detached broker
> integration, lifecycle/discovery, and the long-poll fallback are also built. MCP is a
> separate shim, not served here (§5.8).

`GET /health` is intentionally unauthenticated and carries a non-secret protocol identity:
`generation: "hs2"`, `api_version`, ticket prefix, and store schema. An HS2 MCP HTTP
backend that receives 401/403 probes this marker so it can distinguish a bad HS2 secret
from an HS1/wrong-service endpoint instead of giving generic credential advice
(HS2-8H8BQM).

A thin binary that wraps the core and is the **always-on service** every GUI talks
to — **local use included**. It runs completely independently of any client (a
chartered goal, made absolute by the maintainer 2026-08-19: there is no
embedded-in-client mode). One instance per machine serves all local projects.

Responsibilities:
- **HTTP REST** for CRUD + query (JSON, not HTML — the client/service split).
- **WebSocket** (`/ws/sync`) for live push: index changes, claim/lease events,
  terminal streams, permission prompts. `GET /ws/poll` is the cursor/replay
  fallback for environments without WS; it accepts the loopback secret through
  either the native query form or `X-Hotsheet-Secret`, allowing the web bridge to
  authenticate without exposing credentials to browser JavaScript. The bridge uses
  both forms on its loopback-only hop so a newer client can still long-poll a running
  pre-header-auth server during an unsynchronized rollout. Watcher events enter the
  same replay ring as API mutations.
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

**Built (HS2-59, server-side; HS2-5A01DC, CLI wrapper):**
`hotsheet-server::lifecycle` implements the
machine-local **instance registry** (`${HOTSHEET_HOME:-~/.hotsheet2}/instances/
<project-id>.json` — not `~/.hotsheet`, which HS1 owns, HS2-104), **discovery**
(`find_instance`, validating the recorded pid is alive so a crash leaves no false
positive), the **per-store index-writer lock** (a second server is refused / told to
attach; a stale lock from a dead server is reclaimed), and **stop** (`hotsheet-server
--stop` → SIGTERM). `serve` takes the lock, writes the instance file (removed by a
guard on **graceful shutdown**: SIGTERM/Ctrl-C), and if a live server already serves
the store it **prints how to attach and exits** instead of duplicating. E2E-verified.
`hotsheet-cli serve` resolves the sibling `hotsheet-server` first, falls back to PATH,
requires its version to match the CLI, and forwards foreground/stop arguments with
clear missing-binary and mismatch diagnostics. The **client-side** half — a client
spawning the server *detached* and *supervising* it — lands with the clients
(**HS2-4072GM**); the server is already a separate process,
so a detached spawn makes it outlive the client by construction.

Clients negotiate the protected `GET /compatibility` contract before relying on the
application API. It reports the HS2 generation, semantic application version, optional
build revision, inclusive API-protocol and store-schema ranges, server start time when
available, and lifecycle capabilities. Hard compatibility is based on protocol-range
intersection, not exact version or revision equality, so rolling compatible builds can
coexist. Local development builds also hash their build-relevant server source at compile
time and cheaply monitor that same source tree at runtime. The handshake reports the
built and current source revisions plus `source_stale`; the client can therefore warn
that the detached server needs a rebuild/restart without treating unrelated monorepo Git
commits as staleness. Release/explicit-revision builds omit local source probing, and an
unavailable source tree is not reported as stale. Missing or invalid metadata is an
explicit unknown state. The server currently
advertises lifecycle restart and quiescence as unsupported: clients must not offer an
automatic restart until the server can account for active commands, AI work, mutations,
terminals, and other connected clients. Remote restart likewise requires a future explicit
authenticated capability.

Protocol ranges assume unsynchronized rollout. A non-intersecting range stops project API
use and identifies which side requires an update; exact build differences remain
informational. Persisted-format compatibility is independently governed by
[19](19-format-compatibility.md): released readers permanently accept older released
ticket/store/project/settings fixtures, while newer incompatible markers produce a
specific upgrade-required result.

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
hotsheet attach HS-7f3k9q ./proof.png       # stable id + RFC3339 created_at
hotsheet edit HS-7f3k9q --status completed --note "fixed the pre-theme paint"
hotsheet edit HS-7f3k9q --blocked-by HS-abc123 --blocked-by HS-def456   # set blockers (slug|ULID)
hotsheet edit HS-7f3k9q --clear-blocked-by                              # remove all blockers
hotsheet claim-next --worker worker-1                       # self-select + claim
hotsheet claim HS-7f3k9q --worker orchestrator-1 --label Codex # exact assigned ticket
hotsheet renew HS-7f3k9q --worker orchestrator-1             # extend active lease
hotsheet release HS-7f3k9q --worker orchestrator-1           # stop advertising work
hotsheet key set openai                    # hidden terminal prompt
printf '%s' "$PROVIDER_KEY" | hotsheet key set openai  # automation via stdin; never argv/settings
hotsheet key list                         # provider names only, never values
hotsheet providers --json                 # git + configured external connections/capabilities
hotsheet provider-ls github-main
hotsheet provider-get github-main 42
hotsheet provider-new github-main "Bug title"
hotsheet provider-edit github-main 42 --expected-token <opaque> --status started
hotsheet provider-close github-main 42 --reason completed
```
The server equivalent is `POST /tickets/{id}/attachments` with raw file bytes and
an `x-hotsheet-filename` header. Browser clients percent-encode Unicode filenames and
declare `x-hotsheet-filename-encoding: percent`; the server decodes them before normal
filename sanitization. The returned `ApiTicket.attachments` carries the
stable id, sanitized filename, and creation timestamp. Checkout-scoped clients use
`POST /checkouts/{reference}/tickets/{id}/attachments`, which resolves the ticket's
linked store and returns the same ticket plus its store identity.
`--blocked-by` (repeatable, on `new` and `edit`) takes a slug **or** ULID and is
resolved to a ULID, rejecting unknown tickets and self-references; on `edit` a present
`--blocked-by` **replaces** the set and `--clear-blocked-by` empties it. The same edge
is settable over the API and MCP: `blocked_by` (an array of slug/ULID strings) on
`hotsheet_create` / `hotsheet_update` and the server's `POST` / `PATCH /tickets` — on
update a present `blocked_by` replaces the set (`[]` clears), absent leaves it. All
surfaces share one resolver (`ops::resolve_blockers`), mirroring how `duplicate_of` is
resolved on close.
Create requests may also carry an initial `status` of `not_started`, `started`, or
`backlog`; omitting it remains backward-compatible and defaults to `not_started`.
Backlog creation clears `up_next`, and terminal/archive statuses must be reached through
an update so lifecycle timestamps and close behavior remain coherent. The server,
serverless MCP backend, and git-backed provider boundary preserve the same initial-state
contract; capability-aware GitLab/Jira behavior is tracked separately.
These write ticket files directly and **auto-commit** each mutation to the store's git
repo, then publish without holding the user-facing mutation open for network latency
(HS2-VJD1W4, HS2-0RDWSW). Headless CLI/MCP writes launch a reaped best-effort push in the
background; server-owned stores defer publication to the server's kicked, coalescing sync
loop. Thus a headless `work` run remains clean and shareable while browser/server writes
return after local durability rather than waiting several seconds for a remote. The shared
`ops` layer routes every mutation through `FsStore::write_ticket_committing`, so CLI + MCP
+ server all commit. It's a no-op when the store isn't a git repo, and
`HOTSHEET_NO_AUTOCOMMIT` disables it for batch work.
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
hotsheet init --standalone [--at <path>] [--remote <url>]
                       # create a separate git store + link this code repo in one shot
hotsheet serve         # run the server
hotsheet reindex       # drop + rebuild the index from disk
hotsheet doctor --project .  # store health + read-only tool/HS1 onboarding guidance
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

**Headless first run (HS2-MNHGT3):** `init` prints the same read-only onboarding
report available from `doctor --project <code-repo>`. It detects installed plugin
tools and recommends explicit, idempotent `setup` commands. It recognizes HS1 only
when `<project>/.hotsheet/db/PG_VERSION` exists, warns the user to close HS1, and
prints the exact one-project `hotsheet-migrate` command. Neither command silently
changes tool configuration or starts migration; interactive prompting and optional
continuous config sync remain client work (HS2-8B0YZX).

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
  > server leaf + every device's client cert; `hotsheet-cli cert init|issue|renew|role|revoke`
  > manages it (material under `${HOTSHEET_HOME}/tls/<project-id>/`, machine-local —
  > the CA key is a secret, never committed). An off-loopback `serve` bind loads a
  > rustls `ServerConfig` and serves over a manual `tokio-rustls` acceptor (axum 0.8
  > has no built-in TLS), **requiring** a client cert that chains to the CA and isn't
  > on the `revoked` fingerprint list; loopback stays Tier-0 plaintext. Chain
  > validation is delegated to rustls's `WebPkiClientVerifier` (not hand-rolled), with
  > a revocation gate layered on. The verifier **re-reads the revocation list per
  > handshake** (HS2-MPC0QF), so `cert revoke` applies **live** — no server restart. A
  > CA is valid for ten years, server leaves for 397 days, and device leaves for 90 days;
  > `cert renew` revokes the previous device leaf, issues a fresh one, and carries its ACL
  > role forward. ACLs are opt-in: without `acl.json`, CA membership retains read-write
  > access; once a role is assigned, unknown fingerprints are denied and each device may be
  > `read-only`, `read-write`, or `deny`, reloaded per request. Full live mTLS E2E proves
  > valid-in / no-cert-out / revoked-out-live plus read-only/write/unknown authorization.
  > **Deferred:** `.p12` bundling + QR enrollment (client work) and intermediate-CA UX.

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

Rich-activity distillation is a stricter case: consent is read from the **local scope
only**, even though normal effective settings permit shared/global fallback. A committed
project must not opt another collaborator into summarization. Enable the built-in,
no-network deterministic adapter with:

```sh
hotsheet settings set activity_distillation \
  '{"enabled":true,"adapter":"deterministic","deterministic_fallback":true}' \
  --scope local
```

The failure fallback defaults to `false`; the example opts into it explicitly. This is
separate from selecting the deterministic adapter itself.

Set `enabled` to `false` (or unset the local key) to stop it immediately; pending
in-memory windows are discarded. Native clients can select a client-owned adapter such
as `apple_foundation_models`. The server records and broadcasts the normalized local
stream but never loads Apple Foundation Models or any other client model.

Shared settings are versioned by the same git as tickets (diffable, mergeable via
the same driver where sensible); local settings are disposable machine state. The
`plugins` module reads the enabled-plugin set from settings to decide what `hotsheet
setup` writes ([05](05-ai-tool-plugins.md) §5.1a).

### 4.7.1 Secure provider keys

> **Built (HS2-M1XMSX):** `hotsheet_ticketing::secrets` provides an injected
> `SecretStore` port, native macOS Security.framework Keychain and Linux Secret Service
> (`secret-tool`) adapters, and a global provider registry. `hotsheet key
> set|get|list|delete` is the headless CLI surface; `set` uses a hidden prompt when
> stdin is a terminal and accepts piped stdin for automation. `${HOTSHEET_HOME}/keys.json` contains provider names and fallback environment
> variable names only, is mode `0600` on Unix, and never contains secret values.

Settings refer to a secret as `{ "secret": "provider-id" }`; consumers call the
shared resolver, which fetches the value from the OS credential store. If no credential
exists, the only fallback is the explicit read-only environment variable
`HOTSHEET_API_KEY_<NORMALIZED_PROVIDER>`. There is deliberately no plaintext file
fallback: an unavailable keychain makes writes fail closed. Secret values must not be
placed in project/global settings, ticket files, logs, diagnostics, or command arguments.

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
