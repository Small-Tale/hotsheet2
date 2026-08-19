# 05. AI-Tool Plugin Interface

> **Status: Proposed.** This carries over HS1's hard-won plugin design (docs/132)
> as a *starting point*, not an endpoint. HS1 spent an eight-phase epic clawing
> per-tool `if (tool === …)` branches back into one interface; HS2 begins there.

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

## 5.6 Connection registry & busy tracking (list connections; track busy-ness)

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
  coordination goes through **git itself** — a per-ticket claim marker (`hs-claim/
  <ulid>`) claimed by an **atomic-push compare-and-swap**, with lease expiry + a
  sweep for cleanup. Fully decentralized, no coordinator. Design + the one remote
  caveat to spike: [08-distributed-and-remote.md](08-distributed-and-remote.md) §8.5.

## 5.8 MCP & CLI access for tools

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

## 5.10 Cross-references
- Storage concurrency the claim primitive protects: [02-ticket-storage.md](02-ticket-storage.md) §2.7
- The core that hosts the plugin registry: [04-core-server-cli.md](04-core-server-cli.md)
- Clients that render permission prompts / busy state: [06-clients.md](06-clients.md)
