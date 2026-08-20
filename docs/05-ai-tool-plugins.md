# 05. AI-Tool Plugin Interface

> **Status: Proposed.** This carries over HS1's hard-won plugin design (docs/132)
> as a *starting point*, not an endpoint. HS1 spent an eight-phase epic clawing
> per-tool `if (tool === …)` branches back into one interface; HS2 begins there.
>
> **Ownership + extensibility decided (maintainer, 2026-08-20).** Two changes from
> HS1: (1) the plugin registry and all setup/instruction/skill/MCP/settings
> *management* lives in the **core**, driven by **either the CLI (headless — no
> client, no server) or the server**, not the app layer (§5.1a, §5.12). (2) plugins
> are **external and loadable**, layered as manifest-only (data, no ABI) → behavioral
> by capability (**subprocess** for process-shaped behaviors, **WASM** for
> pure-compute), with built-ins shipped through the same loader and a trust gate
> (§5.12). Build tracked in HS2-91/HS2-92/HS2-93.

## 5.1 The principle

**AI-tool integration is entirely plugin-based. No tool is first-class — not even
Claude.** Every tool (Claude, Codex, Gemini, OpenCode, Antigravity, Goose, and
the editor tools Cursor/Copilot/Windsurf) is one implementation of a single
interface. The interface must fit the tool it was *not* designed around, or it is
a hierarchy, not an interface (HS1's acceptance test — Claude was migrated last,
precisely because it's the deepest integration).

The hard rule, enforced by a lint/boundary check: **outside a plugin's own module,
no code branches on a tool id.** Generic modules ask the plugin, never
`if (tool === 'codex')`.

## 5.1a Ownership: the core, not the app

**Who prepares a project for an AI tool** — writes its instruction file, its skills,
its MCP config, installs its permission bridge — was the **app layer's** job in HS1.
That is wrong for HS2 (maintainer, 2026-08-20): it makes setup impossible headless,
and it re-splits logic the way §4.5 forbids. In HS2 the plugin **registry and all
setup/settings machinery live in the core** ([04-core-server-cli.md](04-core-server-cli.md)
§4.1 `plugins`), reachable from **both** thin binaries:

- **CLI, headless** — `hotsheet setup claude` / `hotsheet setup --detect` prepares a
  project with **no client and no server running**. This is the load-bearing case:
  a purely terminal, server-less workflow can still install instructions, skills, and
  MCP config for whatever tools are present.
- **Server** — the same core code runs when a client asks it to (`POST …/setup/<tool>`).

The **client never implements setup**; consistent with "clients never embed the
core" ([04](04-core-server-cli.md) §4.1), it *requests* setup through the server API
and renders the plugin's declared `preferences`. What moved is *authorship of the
artifacts*, from the app down into the shared core.

**Which set of artifacts** to write is determined by **which plugins are active** —
so "core-owned setup" and "external loadable plugins" (§5.12) are the same
capability seen from two sides: the loader decides *what* tools exist, the setup
capability decides *what each writes*, and either binary can drive it.

Capabilities divide by lifetime, and that division is what makes headless work:

| Bucket | Capabilities | Runs in |
|---|---|---|
| **One-shot, host-agnostic** | `setup`, `instructions`, `skills`, `mcp`-config, `permissions`-install, settings read/write | **CLI or server** — idempotent filesystem writes, no persistent host needed |
| **Persistent, server-only** | terminals/PTY, `drive`/trigger, busy tracking, connection registry, runtime permission bridge | **server** — needs the always-on host (§5.4–§5.7) |

The one-shot bucket is exactly the headless case. The persistent bucket is
unchanged — it always needed the server.

## 5.2 The general interface the ticket asks for

The ticket enumerates what the AI-tool interface must cover. Mapped to the design:

| Requirement | Where it lives |
|---|---|
| Initialize AI tools in terminals (MCP + similar connections) | §5.3 `setup` + §5.4 terminals |
| List AI-tool connections | §5.6 connection registry |
| Trigger commands to a target connection | §5.5 drive/trigger |
| Permission checks & other user prompts | §5.7 permission bridge |
| Track AI-tool busy-ness | §5.6 busy tracking |
| Carry over other useful concepts from HS1 | §5.9 |

## 5.3 A plugin's shape

A plugin is split into a **declarative half** (plain data — client-safe, no
filesystem) and a **behavioral half** (needs the host: fs, processes) — the split
HS1 discovered is unavoidable because the registry is reachable from client code.

**Declarative (data):**
- `id`, `displayName` (short, for running text: "Codex finished"), `productName`
  (full, for menus: "Codex CLI").
- `tier`: `cli-agent` (Hot Sheet drives it) vs `editor` (Hot Sheet only supplies
  context files).
- `maturity`: `stable | beta | unreleased` — a property of the *integration*, not
  a per-project setting; controls whether it ships to users at all.
- `detection`: `{ binaries, paths }` — evaluated by the host, not a closure, so the
  registry stays client-safe.
- `transport`: which drive transport this tool speaks (identity, not behavior —
  HS1 §132.11.7 learned this the hard way when a client "mirror" of transports
  drifted).
- `preferences`: declared per-tool settings (e.g. "interactive permissions
  on/off"), rendered by a shared settings renderer — no hand-written UI per tool.

**Behavioral (host-side, keyed by plugin id):**
- `instructions`: the managed instruction file (`CLAUDE.md` / `AGENTS.md` /
  `GEMINI.md` / a rules file) + frontmatter + adapter-family flag.
- `skills`: generate/refresh the worklist skill/rule artifact; report which
  artifact answers "is this tool prepared?"
- `command`: the binary + how a launch line resolves (channel flags, model
  variants).
- `drive`: the transport implementation — run one turn; optional
  interrupt/reset/prestart/busy; optional long-lived backing service.
- `permissions`: install/remove the tool's permission bridge, merge-safe.
- `mcp`: write the `hotsheet-*` MCP server into the tool's config, in that tool's
  format (JSON / TOML / ACP session field).
- `metrics`: report **usage / cost metrics** from the tool's telemetry, mapped to a
  **unified metrics interface** all plugins conform to (HS2-46/HS2-69). Only
  usage/cost is kept — the HS1 debugging telemetry (span trees / tracing / waterfalls)
  is dropped. A tool without telemetry simply omits this capability.
- `activity`: emit **tool-agnostic progress / summary events** ("started ticket X",
  "edited file Y", "finished") that the **Announcer** (narration + TTS) and a timeline
  consume — a common cross-tool interface so narration isn't Claude-only (HS2-48).
  Design: HS2-70. A tool that exposes no activity stream omits it.

> **Cross-cutting theme:** `drive`, `metrics`, and `activity` are all *seams that
> generalize one concern across every tool*. Designing these capability interfaces
> well (and early, HS2-67/HS2-69/HS2-70) is what keeps adding a tool cheap — the
> §5.10 testability rule + conformance suite hold each to its contract.

**Absence is the signal.** A missing capability means "not supported" — there is
no `supportsDrive: false` boolean to drift from reality. Gemini has no drive;
Goose is only identity + a command; editor tools have no runtime. A missing method
can't be called by mistake.

**The host carries the machinery.** If two plugins would write the same code, that
code is a host helper, not per-plugin: the merge-safe hooks-file writer, managed
instruction sections, the adapter skill-tree writer, the MCP-config primitive, the
permission bridge, PTY/stdio framing, the commands-log emitter. A plugin *declares
what's specific* and *calls host helpers for the rest* — a common-shape tool
(AGENTS.md + a skills tree + a hooks file + a spawn drive) is nearly declarative.
That "nearly" is the whole external-plugin story (§5.12): for most tools a plugin
is *pure data*, which has no ABI problem and needs no code boundary.

**"The host" is whichever binary runs the capability** (§5.1a), not "the server."
The one-shot helpers above (instruction sections, skill-tree, MCP-config,
permission-bridge *install*) are plain filesystem writes and run inside the **CLI**
headless just as well as inside the server; only the persistent machinery (PTY,
drive runtime, busy, the *live* permission bridge) requires the always-on server.

## 5.4 Terminals & initialization (init AI tools in terminals)

The **terminal/PTY manager** (in the core, hosted by the server) provides:
- One or more PTYs per project, spawned lazily, keyed by `(project, terminalId)`.
- A **scrollback ring buffer** and multi-client attach (many viewers see one
  stream, tmux-style).
- Survival across server restarts via a **detached PTY broker process** (carried
  from HS1 `src/terminals/broker/`) — terminals aren't killed when the server
  recycles.
- Environment scrubbing (drop tool-marker vars like `TSX_*`/`npm_*` that leak into
  child shells — HS1 §22.13.1).
- **Server-arbitrated PTY sizing.** A PTY has exactly one size, but many viewers
  (across devices) attach at once and want different sizes. The **server is the
  sole arbiter** of the size: viewers send *size claims*, the server picks the size
  by a focus-follows policy with leases + hysteresis, and broadcasts the result.
  Full design: [06-clients.md](06-clients.md) §6.7. (This replaces HS1's ad-hoc
  "largest-or-last-writer" consensus, which never worked for remotes.)

**Initializing a tool** in a terminal = the plugin's `setup` composed from host
helpers: write the tool's MCP config (so `hotsheet_*` is available), write its
instruction file + skills, install its permission bridge if opted in, and resolve
its launch command. Launching then spawns that command in a PTY. This is exactly
the ticket's "initializing AI tools in terminals including setting up MCP and
similar connections."

## 5.5 Drive / trigger (send a command to a target connection)

> **Status: first slices built (HS2-106/107/108).** `crates/hotsheet-aitools` — the
> `Drive` trait + the spawn-per-run `SpawnDrive` (Codex `exec`) over an injected
> `ProcessSpawner`, the `ConnectionRegistry` (§5.6), and `host::trigger` which builds a
> `Drive` from a plugin's manifest `[drive]` declaration, registers a `Connection`, and
> runs one turn — all conformance-tested against a fake spawner (`docs/13` §13.7).
> **Still to build:** the persistent-channel (Claude) + ACP drives with the async
> `TurnEvent` stream (HS2-9), the permission bridge (§5.7), and the real CLI/server
> trigger that injects `SystemSpawner` (with the HS2-103 launch-safety lessons).

> **Direction confirmed (maintainer, 2026-08-19, HS2-41):** there is **no single
> transport all tools share** — but there is **one interface with optional
> capabilities that each tool conforms to as applicable** (absence = not supported,
> §5.3). Different tools implement different subsets. **Designing this interface is
> an early priority** — investigate it sooner rather than later, since it's the seam
> every tool and the whole test harness hang off (design ticket: HS2-67).

A **drive transport** is how the app steers a running tool. HS1 has four; HS2
models them as implementations of one `Drive` trait so a fifth is additive:

- **Persistent channel** (Claude): a long-lived MCP channel; a trigger injects a
  `<channel>` event into the running session ("run the worklist"). Permissions and
  busy flow back over the same channel.
- **Spawn-per-run** (Codex `exec`, Antigravity `agy --print`): each trigger spawns
  a one-shot process; done = process exit.
- **App-server / daemon** (Codex app-server): a long-lived backing service driven
  over JSON-RPC.
- **ACP** (OpenCode, Goose, Kiro): the Agent Client Protocol —
  `session/prompt` = trigger, `session/update` = busy, `stopReason` = done,
  `session/request_permission` = the permission overlay.

`Drive::run(target, content)` sends a command/prompt to a **target connection**
(the ticket's "triggering commands to a target connection"). `target` selects
*which* connection when several exist (e.g. a git-worktree worker's channel vs the
main one). `run` may be sync (spawn) or async (POST to a running session) — both
allowed by the trait.

**Optional drive sub-capabilities** (a tool implements only what it supports —
absence = not supported): `interrupt`, `reset`, `prestart` (a daemon warm-up),
`isBusy`, and a long-lived **backing service** (Codex's app-server; Claude's channel
arguably). The host calls `drive.interrupt?()` etc. as no-ops for tools that don't
have them, so a new tool declares its subset and nothing branches on the tool id.
The v1 tools exercise two shapes: **Claude** (persistent channel) and **Codex**
(spawn / app-server) — enough to prove the interface isn't Claude-shaped. Getting
this trait boundary right early (HS2-67) is what lets a third tool be nearly
declarative.

## 5.6 Connection registry & busy tracking (list connections; track busy-ness)

> **Status: registry + busy built (HS2-107).** `hotsheet_aitools::ConnectionRegistry`
> — register/unregister/get/list/count over `Connection { id, project, tool, role
> (Main|Worker|DriveSpawned), transport, pid, started_at }`, plus **busy as a derived
> sliding-window view**: `note_activity(id, now)` is one heartbeat both hooks and
> byte-stream/spinner inference feed, `is_busy`/`busy_count` read the window, and
> `set_idle` drops it on a `Done`. The clock is injected (deterministic). Still to
> wire: live `TurnHandle` signals feeding it (with the server), and the actual spinner
> inference (with terminals, HS2-10).

- **Connection registry.** Each live tool connection registers an entry
  (`project`, `tool`, `pid`/session, `startedAt`, role: main vs worker vs
  drive-spawned). The API exposes `list connections`, and the UI shows "N
  connections active." Carried from HS1's `channel-ports.d/` + `channelRegistry`,
  generalized so it's not Claude-specific (HS1's is per-Claude-channel-server).
- **Busy tracking, two complementary sources** (both carried from HS1):
  1. **Signals from the tool** — lifecycle (process exit) or hooks
     (UserPromptSubmit/PreToolUse/PostToolUse/Stop heartbeats), extending a
     sliding busy timer.
  2. **Byte-stream inference** — the PTY manager watches for the tool's animated
     spinner glyph; recent spinner output ⇒ busy even mid-single-tool-call, N
     seconds of silence ⇒ idle. This backstops a dropped Stop hook.
  The registry exposes `isBusy(connection)`; the UI shows "X working / X idle."

## 5.7 Permissions & user prompts (permission checks and other prompts)

A **host-side permission bridge**: "ask the user, get a decision," with each
plugin supplying only the transport-specific adapter (an ACP option-response, a
PreToolUse hook CLI, a hooks.json entry). When a tool wants approval to run a
command:
1. The tool's adapter routes the request to the bridge.
2. The bridge enqueues it (FIFO — concurrent requests preserved, not overwritten,
   an HS1 bug fixed in §12.10) and pushes it over the WebSocket to every client.
3. The UI shows a non-modal permission popup anchored to the owning project; the
   user allows/denies (with allow-once/always mapping onto persisted allow-rules).
4. The answer routes back to the connection that raised it.

This is also the seam for **other prompts** the ticket mentions — the bridge is a
generic "the tool needs a human decision" channel, not just tool-permissions.

**The claim/lease primitive** (`coord`) is what keeps distributed work sane, and it
underpins the git-storage concurrency story ([02-ticket-storage.md](02-ticket-storage.md)
§2.7). Two regimes:

- **Single shared server (this section's default):** a worker atomically claims the
  top Up Next ticket (`SKIP LOCKED`-style over the index), holds a renewable lease,
  and the server's write chokepoint rejects a write to a ticket another actor holds.
  Lazy reclaim + poison quarantine carried over. `claim-next` selection runs over
  the **index** (fast), and the claim is persisted to the **ticket file**
  frontmatter (source of truth) — so a claim survives an index rebuild.
- **Multiple independent machines over a shared git remote (no single server):**
  coordination goes through **git itself** — a per-ticket claim marker
  (`refs/hotsheet/claims/<ulid>`) claimed by an **atomic-push compare-and-swap**,
  with lease expiry + a sweep for cleanup. Fully decentralized, no coordinator.
  **Validated** (custom refs work on GitHub; tags are the fallback) —
  [08-distributed-and-remote.md](08-distributed-and-remote.md) §8.5.

## 5.8 MCP & CLI access for tools

> **Status: MCP shim built (v1, HS2-7/43); serverless mode added (HS2-96).**
> `crates/hotsheet-mcp` → the `hotsheet-mcp` binary: a stdio JSON-RPC 2.0 server
> exposing `hotsheet_query` / `get` / `create` / `update` / `close`. It runs in
> **two modes over one `Backend` trait**, so the tool surface is identical either
> way — this is what lets a headless agent work **with or without a server**:
> - **`--path <store>` → serverless**, straight to disk over `hotsheet_ticketing::ops`
>   (no server, no index — reads are a file scan, symmetric with the CLI; `docs/04`
>   §4.4). The headless default. A running server's watcher still picks up its writes.
> - **`--server <url> --secret <s>` → proxy** a running `hotsheet-server` over HTTP,
>   for index-backed reads + instant broadcast.
>
> The full-ticket + list-row wire DTOs are defined once in `hotsheet_ticketing::wire`
> (the wire SSOT, §4.2) and shared by the server and both shim backends, so the JSON
> an agent sees never drifts between modes. The plugin-config writing half (the `mcp`
> capability that drops the entry into each tool's config) is HS2-98.


AI tools reach tickets two ways, both over the one core:
- **MCP** — the `hotsheet_*` tool surface (create/update/get/query/claim/etc.).
  **Decided (maintainer, 2026-08-19): a small per-project MCP shim** spawned into
  each tool's config (as HS1 does with `channel.ts`), *not* the server exposing MCP
  directly. This keeps the per-project namespacing (`hotsheet-channel-<slug>`) and
  the channel model tools already expect, and lets a tool reach the right project by
  its own config. The plugin's `mcp` capability writes whichever entry the tool's
  config format needs; the shim proxies to the core/server.
- **CLI** — `hotsheet` commands ([04-core-server-cli.md](04-core-server-cli.md) §4.4),
  for tools that shell out.

Both are thin over `hotsheet-core`; there is no duplicated handler tree (HS1's MCP
tools proxy the REST API — HS2's proxy the core directly).

## 5.9 Other HS1 concepts to carry over (evaluated)

Per the ticket's "evaluate other AI-tool interface concepts to carry over":

- **Worklist-as-file** (`worklist.md`) — keep. The file-based contract lets *any*
  tool participate without the API.
- **Auto-context** (HS1 docs/4 §4.18) — **keep; critical (HS2-25).** Per-category and
  per-tag guidance the user configures is **injected into the generated worklist**
  so the AI tool gets the right context automatically for each ticket. It rides the
  worklist-as-file contract (the guidance is composed into `worklist.md` during
  generation — [03](03-indexing-and-query.md) §3.6), so it works for every tool with
  no per-tool code.
- **Skills/instructions generation** for editor tools (Cursor/Copilot/Windsurf) —
  keep; it's the whole Tier-B story and already tool-agnostic in HS1.
- **Self-claim worker loop + worktrees** (the `/hotsheet-worker` skill) — keep the
  claim/lease + worktree isolation + "worker never merges, the main agent
  integrates" model. Drop the retired imperative worker-pool orchestration (HS1
  moved to prompt-driven, docs/90 partial retirement); keep it prompt-driven.
- **Telemetry/cost attribution & the Announcer** — defer (they ride Claude's OTLP
  stream; port later as their own tickets, tool-agnostic where possible).
- **Commands Log** transcript of triggers/permissions/shell runs — keep; plugins
  emit, the host owns the log shape.

## 5.10 Testability — injected adapters + the fake agent

> **Load-bearing rule** (maintainer, 2026-08-19): adding a tool to HS1 was a manual
> testing slog. HS2 designs that out — the plugin interface is built to be tested
> against a **fake agent**, not a real LLM.

- **Every side-effecting interaction a plugin performs goes through an injected
  adapter** — `ProcessSpawner`, config-file writer, `PermissionTransport`,
  `McpConfigWriter`, `Clock`. **No plugin touches a real process, file, or global
  directly.** (HS1 half-learned this — docs/132 §132.7's "run() with an injected
  spawner reports the content it *would* send"; here it's non-negotiable.) This is
  what makes drive / permissions / MCP-config / command all deterministically
  testable, and it's a hard rule the conformance suite enforces.
- **Tested against `hs-fake-agent`** — a scriptable test double that speaks the same
  protocols a real tool does (MCP calls, permission requests, PTY bytes/OSC/spinner,
  busy signals), so the host side is exercised end-to-end with no real tool.
- **A conformance suite parameterized over the whole registry is a hard CI gate** —
  a new tool inherits it by existing and can't merge until it passes conformance +
  the fake-agent E2E.
- **Real-tool drift** is caught by a thin, explicit layer: recorded protocol
  contracts (replayed in fast CI) + an opt-in, creds-gated live smoke per tool.

Full testing design: [12-code-organization-and-testing.md](12-code-organization-and-testing.md)
§12.7.7. Build: **HS2-64**.

## 5.11 Plugin loading & extensibility (external plugins)

> **Decided (maintainer, 2026-08-20); manifest-only loading built (HS2-92).** Plugins
> are **external and loadable** — a third party can add a new AI tool without
> recompiling the core — layered so the common case has no ABI at all. `hotsheet-plugins`
> now loads a plugin from a **bundled** first-party dir (`include_dir`) **or a real
> on-disk dir** through one code path (`Plugin::from_fs_dir` / `all_plugins(search_dirs)`);
> the machine search dir is `${HOTSHEET_HOME:-~/.hotsheet2}/plugins/<id>/` (kept **off**
> HS1's `~/.hotsheet`). `hotsheet-cli plugin list|install|remove` manages them, and
> `hotsheet-cli setup <third-party-id>` works with no recompile. A first-party id wins a
> collision (a third party can't shadow a built-in). Still to build: the behavioral
> subprocess/WASM boundary + the trust gate (**HS2-93**).

Rust has no stable ABI, so "loadable plugin" cannot mean "load a `.dylib`." The
**declarative/behavioral split (§5.3) is the escape hatch**: most of a plugin is
*data*, and data has no ABI problem. Plugins are therefore layered:

- **Manifest-only plugins — the bulk.** A directory with a manifest (id,
  `detection`, `preferences`, `tier`, `transport` id, launch command, the
  MCP-config *format*) plus template files (instruction file, skills/rules tree).
  **No code, no ABI, no code sandbox** (there is no code). A common-shape tool
  (§5.3) ships as *just this*. Loaded identically into the CLI and the server, so
  `hotsheet setup <third-party-tool>` works headless.
- **Behavioral plugins — manifest + code**, only for the custom bits (a persistent
  channel, an app-server drive, a bespoke permission bridge). The execution boundary
  is chosen **by capability**:
  - **Subprocess protocol (stdio JSON-RPC)** for the **process-shaped behaviors** —
    `drive`/trigger, terminals, MCP. These are *already* subprocess-shaped in HS2
    (ACP, Codex app-server, the `hotsheet-mcp` shim), so an external drive plugin is
    just another executable speaking the capability protocol. Language-agnostic; OS
    crash-isolation.
  - **WASM (`wasmtime`/`extism`)** for **pure-compute transforms** that want a
    tighter sandbox — the host exposes only the §5.10 adapters (`ProcessSpawner`,
    config writer, `PermissionTransport`, `McpConfigWriter`, `Clock`) as
    capability-scoped imports; ambient fs/net is denied.

**Built-ins are first-party plugins, in this repo, through the same loader — from
day one** (maintainer, 2026-08-20). Claude and Codex are not special-cased and are
not compiled-in-then-extracted later: the initial set ships as first-party plugin
directories **in the HS2 repo**, bundled into the binaries as the built-in
search-path entry, and loaded by the exact same loader a third party's plugin uses.
This is the §5.10 anti-drift discipline applied to the loader itself — our own tools
ride the external interface, so it can't rot. **Third-party plugins are a
post-release capability:** once HS2 ships, developers add their own plugins
(machine `~/.hotsheet/plugins/` or project `.hotsheet/plugins/`) with no fork and no
recompile. So there is exactly one loader and one plugin shape; "first-party" is a
provenance/trust label (§ trust gate), not a separate code path.

**The loader lives in core** (`plugins`, [04](04-core-server-cli.md) §4.1) and reads
a search path: **bundled built-ins → `~/.hotsheet/plugins/` (machine) → project
`.hotsheet/plugins/`**. Both binaries load the same registry, which is what lets a
headless CLI set up a project for a plugin the user dropped in.

**Trust gate (mandatory, not optional).** A manifest is inert data, but what it
*writes* is a supply-chain surface: a plugin's instruction template steers an agent,
and its launch command *executes*. So:
- **Install-time consent** shows exactly what a plugin will write and what it will
  launch, and its **provenance** (first-party / signed / unsigned third-party).
- **`hotsheet plugin verify`** runs the §5.10 conformance suite against a plugin
  (against `hs-fake-agent`) — the acceptance test a third-party plugin must pass,
  since we can't gate someone else's plugin in our CI.
- Subprocess/WASM behavior runs under the least-privilege boundary above; a
  manifest-only plugin can *write* but never *executes host code*.

CLI surface: `hotsheet plugin list | info <id> | install <path|url> | verify <id> |
remove <id>`, and `hotsheet setup <tool|--detect>` (§5.1a).

> **Trust gate built (HS2-93, partial).** `hotsheet-cli plugin verify` checks a plugin
> structurally — the MCP `format` is known, and every declared **write target stays
> inside the project** (no `..`/absolute escape, enforced in the setup writer too, so a
> plugin can't be tricked into writing outside the project). `plugin info` / `install`
> **disclose what a plugin writes + launches** (plus provenance: first-party vs
> unsigned third-party) and `install` requires confirmation (`--yes` to skip). **Still
> to build:** the behavioral **subprocess/WASM sandbox** (no code-bearing plugins exist
> yet — all current plugins are manifest-only data) and the **`hs-fake-agent`
> conformance** half of `verify` (HS2-64).

## 5.12 Cross-references
- Storage concurrency the claim primitive protects: [02-ticket-storage.md](02-ticket-storage.md) §2.7
- The core that hosts the plugin registry + settings model: [04-core-server-cli.md](04-core-server-cli.md) §4.1, §4.9
- Clients that render permission prompts / busy state / plugin preferences: [06-clients.md](06-clients.md)
- AI-tool integration testing (fake agent, conformance gate, drift layer): [12-code-organization-and-testing.md](12-code-organization-and-testing.md) §12.7.7
