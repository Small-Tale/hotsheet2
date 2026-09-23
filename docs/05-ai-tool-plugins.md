# 05. AI-Tool Plugin Interface

> **Status: Proposed.** This carries over HS1's hard-won plugin design (docs/132)
> as a _starting point_, not an endpoint. HS1 spent an eight-phase epic clawing
> per-tool `if (tool === …)` branches back into one interface; HS2 begins there.
>
> **Ownership + extensibility decided (maintainer, 2026-08-20).** Two changes from
> HS1: (1) the plugin registry and all setup/instruction/skill/MCP/settings
> _management_ lives in the **core**, driven by **either the CLI (headless — no
> client, no server) or the server**, not the app layer (§5.1a, §5.12). (2) plugins
> are **external and loadable**, layered as manifest-only (data, no ABI) → behavioral
> by capability (**subprocess** for process-shaped behaviors, **WASM** for
> pure-compute), with built-ins shipped through the same loader and a trust gate
> (§5.12). Build tracked in HS2-91/HS2-92/HS2-93.

## 5.1 The principle

**AI-tool integration is entirely plugin-based. No tool is first-class — not even
Claude.** Every tool (Claude, Codex, Gemini, OpenCode, Antigravity, Goose, and
the editor tools Cursor/Copilot/Windsurf) is one implementation of a single
interface. The interface must fit the tool it was _not_ designed around, or it is
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

For a completely clean checkout, `hotsheet bootstrap` composes store initialization,
checkout linking, detected (or explicit) tool setup, and optional existing-remote
configuration into one idempotent headless workflow. Graphical project setup invokes
that workflow instead of maintaining a client-only implementation (HS2-J90FXF).

The **client never implements setup**; consistent with "clients never embed the
core" ([04](04-core-server-cli.md) §4.1), it _requests_ setup through the server API
and renders the plugin's declared `preferences`. What moved is _authorship of the
artifacts_, from the app down into the shared core.

Freshness uses the same ownership boundary (HS2-40HZMB): the headless
`hotsheet setup --refresh` command and the server's non-blocking project-open hook both
invoke the core merge-safe writers. They migrate only settings files that already exist
and refresh detected or previously managed enabled plugins, which repairs stale or
partial setup without opting a clean project into an absent tool. Identical output is a
byte-level no-op, and managed instruction markers remain the boundary around content
Hot Sheet may replace. Claude's `.claude/skills/hotsheet/SKILL.md` and Codex's
`.agents/skills/hotsheet/SKILL.md` are fully managed plugin artifacts kept synchronized
with their canonical shared workflows; refresh replaces a stale Hot Sheet adapter while
preserving unrelated user-authored skills and instruction content. Instruction blocks and
skills carry numeric workflow-version markers. Before refreshing either artifact, setup
re-reads both installed targets and compares each marker with its bundled counterpart. If
either installed half is newer, setup preserves both as one workflow bundle while
continuing merge-safe MCP and hook maintenance. An equal-version installed half is also
preserved when its bytes differ from the bundled representation: equal versions identify
one semantic generation, but do not prove that a project-specific adapter or formatting
variant is owned by the current writer. Semantic bundled changes therefore require a
version bump. This prevents a stale or same-generation long-running server or CLI from
producing a mixed, downgraded, or de-customized workflow. Unversioned and older managed
artifacts still upgrade normally. Bundled skill versions are checked against
the current shared adapters, and Windows detection honors command wrappers from `PATHEXT`,
so freshness does not silently skip npm-installed tools or replace a newer workflow with an
older bundle.
In the source-backed web development bridge, the compiled CLI reports a digest of all
embedded setup assets and the bridge independently hashes the live `plugins/` tree. A
missing or mismatched digest refuses project setup with a rebuild command before any
managed file can be written; this covers instruction and manifest changes in addition to
the explicit skill-version check.
Machine-specific integrations never alter the shared ignore policy: Claude's permission
hook uses `.claude/settings.local.json`, Codex's uses `.codex/hooks.json`, and both are
added only to that checkout's `.git/info/exclude`. A newly created, wholly Hot Sheet-owned
MCP config is excluded the same way. A pre-existing config with unrelated user content
remains visible to git while the Hot Sheet entry is merged into it.

All bundled instruction and ticket-workflow artifacts teach **portable durable
references** (HS2-ERKA8N). AI-authored documentation, ticket text, notes, and completion
summaries use repository-relative paths inside the current checkout. References to another
repository use its stable name and canonical URL when helpful, or a technical placeholder
such as `<repo-root>/path`; a developer's home directory, username, or absolute clone
location is not presented as shared project structure. An exact local path is retained only
when the path itself is indispensable evidence for a machine-local diagnostic.

AI-authored ticket notes are also optimized for **human scanning** (HS2-MBJX7C). The
bundled instructions, Hot Sheet skills, and derived `worklist.md` tell agents to lead with
the outcome rather than a chronological transcript; substantial notes use short Markdown
sections for results, verification, and follow-ups, with bullets for parallel facts and
tables only when they clarify a dense comparison or timeline. Simple updates stay brief,
empty sections are omitted, and raw logs or multi-part results are not left as one dense
paragraph. Multiline CLI notes use `--note-file` so the intended Markdown structure reaches
the ticket intact.

Beyond the ticket-command reference, each bundled instruction block (`plugins/<tool>/
instructions.md`) carries a compact, **project-neutral** default policy so a brand-new
project that never customizes its `CLAUDE.md`/`AGENTS.md` still gets strong defaults
(HS2-3JMMAZ): ticket direct-terminal work by default (not only queue-driven work), create
every follow-up immediately, double (unit + E2E) test coverage with transition-matrix/
adversarial tests for stateful code, keeping requirements/docs in sync in the same change,
and per-ticket commit hygiene — while explicitly **leaving the push/PR decision to each
repository** rather than mandating it. The four first-party blocks share one body; the
`every_builtin_carries_the_full_default_guidance` test pins that content.

**Which set of artifacts** to write is determined by **which plugins are active** —
so "core-owned setup" and "external loadable plugins" (§5.12) are the same
capability seen from two sides: the loader decides _what_ tools exist, the setup
capability decides _what each writes_, and either binary can drive it.

Capabilities divide by lifetime, and that division is what makes headless work:

| Bucket                      | Capabilities                                                                                  | Runs in                                                                     |
| --------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------- |
| **One-shot, host-agnostic** | `setup`, `instructions`, `skills`, `mcp`-config, `permissions`-install, settings read/write   | **CLI or server** — idempotent filesystem writes, no persistent host needed |
| **Persistent, server-only** | terminals/PTY, `drive`/trigger, busy tracking, connection registry, runtime permission bridge | **server** — needs the always-on host (§5.4–§5.7)                           |

The one-shot bucket is exactly the headless case. The persistent bucket is
unchanged — it always needed the server.

## 5.2 The general interface the ticket asks for

The ticket enumerates what the AI-tool interface must cover. Mapped to the design:

| Requirement                                                  | Where it lives                |
| ------------------------------------------------------------ | ----------------------------- |
| Initialize AI tools in terminals (MCP + similar connections) | §5.3 `setup` + §5.4 terminals |
| List AI-tool connections                                     | §5.6 connection registry      |
| Trigger commands to a target connection                      | §5.5 drive/trigger            |
| Permission checks & other user prompts                       | §5.7 permission bridge        |
| Track AI-tool busy-ness                                      | §5.6 busy tracking            |
| Carry over other useful concepts from HS1                    | §5.9                          |

## 5.3 A plugin's shape

A plugin is split into a **declarative half** (plain data — client-safe, no
filesystem) and a **behavioral half** (needs the host: fs, processes) — the split
HS1 discovered is unavoidable because the registry is reachable from client code.

**Declarative (data):**

- `id`, `displayName` (short, for running text: "Codex finished"), `productName`
  (full, for menus: "Codex CLI").
- `tier`: `cli-agent` (Hot Sheet drives it) vs `editor` (Hot Sheet only supplies
  context files).
- `maturity`: `stable | beta | unreleased` — a property of the _integration_, not
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
  artifact answers "is this tool prepared?" The shared Hot Sheet workflow asks agents
  doing user-visible UI work to attach representative real-browser captures across the
  changed states and relevant viewport sizes, then reference those attachments by name
  in the ticket's development and completion notes.
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

> **Cross-cutting theme:** `drive`, `metrics`, and `activity` are all _seams that
> generalize one concern across every tool_. Designing these capability interfaces
> well (and early, HS2-67/HS2-69/HS2-70) is what keeps adding a tool cheap — the
> §5.10 testability rule + conformance suite hold each to its contract.

**Absence is the signal.** A missing capability means "not supported" — there is
no `supportsDrive: false` boolean to drift from reality. Gemini has no drive;
Goose is only identity + a command; editor tools have no runtime. A missing method
can't be called by mistake.

**The host carries the machinery.** If two plugins would write the same code, that
code is a host helper, not per-plugin: the merge-safe hooks-file writer, managed
instruction sections, the adapter skill-tree writer, the MCP-config primitive, the
permission bridge, PTY/stdio framing, the commands-log emitter. A plugin _declares
what's specific_ and _calls host helpers for the rest_ — a common-shape tool
(AGENTS.md + a skills tree + a hooks file + a spawn drive) is nearly declarative.
That "nearly" is the whole external-plugin story (§5.12): for most tools a plugin
is _pure data_, which has no ABI problem and needs no code boundary.

**"The host" is whichever binary runs the capability** (§5.1a), not "the server."
The one-shot helpers above (instruction sections, skill-tree, MCP-config,
permission-bridge _install_) are plain filesystem writes and run inside the **CLI**
headless just as well as inside the server; only the persistent machinery (PTY,
drive runtime, busy, the _live_ permission bridge) requires the always-on server.

## 5.4 Terminals & initialization (init AI tools in terminals)

The **terminal/PTY manager** (in the core, hosted by the server) provides:

- One or more PTYs per project, spawned lazily, keyed by `(project, terminalId)`.
- A bare terminal-create request launches the host user's default shell (`SHELL` on
  Unix or `COMSPEC` on Windows, with `/bin/sh` and `cmd.exe` fallbacks), matching
  HS1's explicit new-terminal behavior.
- A **scrollback ring buffer** and multi-client attach (many viewers see one
  stream, tmux-style). Retained scrollback and the live broadcast share one atomic
  handoff: output concurrent with attach appears exactly once in either the snapshot or
  the new subscriber. A lagged subscriber atomically takes a replacement snapshot and a
  fresh live receiver rather than appending an overlapping replay (HS2-5W0V9M).
- Survival across server restarts via a **detached PTY broker process** (carried
  from HS1 `src/terminals/broker/`) — terminals aren't killed when the server
  recycles or is explicitly stopped. Detached hosting is the server default; ordinary
  `hotsheet serve --stop` preserves the project's terminals, while
  `hotsheet serve --stop --kill-all-terminals` deliberately clears them. The broker
  answers a protocol-level Ping/Pong health probe and exits
  cleanly (removing its socket) after a five-minute grace with no terminals and no
  connected clients; any activity resets the grace. A server that outlives that idle
  exit reconnects through one shared restart lock, launches a fresh broker when the
  socket is gone, and retries the connection before serving the next terminal request.
  New broker sockets use `/tmp/hotsheet2-brokers-<effective-uid>/<home-project-hash>.sock`:
  the short path stays within Unix socket address limits even when `HOTSHEET_HOME` is
  under macOS's long temporary-directory root. The private directory is owned by the
  effective user with mode 0700; sockets and persistent ownership-lock files use 0600.
  The key includes the canonical configured home and project, so separate homes do not
  share terminal brokers. Indexes, instance registrations, settings, and other state
  remain under the configured home. An owned legacy socket is adopted under a shared
  namespace-selection lock and pinned by its persistent ownership-lock inode, even
  after idle socket cleanup. Retained and fresh server handles therefore reuse the
  same legacy address across exits/restarts; failed health probes never select a
  second namespace. This preserves terminals across upgrades. Symlink/shared directories and
  non-socket stale paths are rejected with a diagnostic; exclusive no-follow locks
  serialize detached ownership, and shutdown removes only the socket inode it bound.
  Lock files remain in place to avoid unlink/recreate races; they hold no durable
  application data and the operating system releases their lock on process exit.
- Environment scrubbing (drop tool-marker vars like `TSX_*`/`npm_*` that leak into
  child shells — HS1 §22.13.1).
- **Server-arbitrated PTY sizing.** A PTY has exactly one size, but many viewers
  (across devices) attach at once and want different sizes. The **server is the
  sole arbiter** of the size: viewers send _size claims_, the server picks the size
  by a focus-follows policy with leases + hysteresis, and broadcasts the result.
  Full design: [06-clients.md](06-clients.md) §6.7. (This replaces HS1's ad-hoc
  "largest-or-last-writer" consensus, which never worked for remotes.)

**Initializing a tool** in a terminal = the plugin's `setup` composed from host
helpers: write the tool's MCP config (so `hotsheet_*` is available), write its
instruction file + skills, install its permission bridge if opted in, and resolve
its `[launch]` command. `[launch]` is explicitly separate from `[drive]`: the former
starts the interactive REPL, while the latter may start a headless protocol such as
Codex app-server or Claude stream-json. `POST /terminals` with `connect:<tool>` and no
`command` runs setup, safely resolves `[launch].program`, injects the server permission
route, and spawns it in the PTY. Launching then spawns that command in a PTY. This is exactly
the ticket's "initializing AI tools in terminals including setting up MCP and
similar connections."

## 5.5 Drive / trigger (send a command to a target connection)

> **Status: first slices built (HS2-106/107/108/110).** `crates/hotsheet-aitools` — the
> `Drive` trait; the **`AppServerDrive`** (Codex, **persistent** app-server daemon — a
> `turn/start` on a resumed thread, not a fresh process); the spawn-per-run `SpawnDrive`
> (agy, `codex exec` fallback) over an injected `ProcessSpawner`; the `ConnectionRegistry`
> (§5.6); and `host::trigger` which builds a `Drive` from a plugin's manifest `[drive]`
> and registers a `Connection` — all fake-tested (`docs/13` §13.7). **Drivability is the
> integration priority** (see the current-tool matrix, `docs/13` §13.0). **Still to
> build:** the live Codex daemon connection, the Claude channel drive + async `TurnEvent`
> stream (HS2-9), the ACP drive (OpenCode), the permission bridge (§5.7), and the real
> CLI/server trigger injecting `SystemSpawner`.
>
> **`hotsheet-cli trigger` launch safety is baked in (HS2-117).** A bare `trigger <tool>`
> is safe by default: it prepends a `hotsheet` → `hotsheet-cli` shim (+ the CLI's own dir)
> to the launched tool's PATH so a bare `hotsheet` can't hit an HS1 launcher; refuses to
> run in a project that still holds a live HS1 store unless the selected HS2 store has a
> schema-valid durable import receipt whose canonical `sourceProject` matches that checkout;
> fails closed for any absent, malformed, relative-path, or unrelated receipt; refuses
> when the tool isn't set up; and defaults `--mcp-config` to the tool's project config
> so it reaches **only** the Hot Sheet shim (Claude via `--strict-mcp-config`). `setup`
> now writes an
> **absolute** `hotsheet-mcp` path so the config works without the shim. Codex reads its
> MCP servers from `$CODEX_HOME`, so `trigger` **auto-builds a throwaway MCP-free
> `CODEX_HOME`** for it (HS2-YRDQNX) — a copy of `auth.json` plus a `config.toml` whose
> only server is the Hot Sheet shim — so a bare `trigger codex` can't reach the user's
> global MCP servers (`--env CODEX_HOME=…` overrides). See
> `crates/hotsheet-aitools/src/launch_safety.rs` and `docs/04` §4.4.
>
> **`hotsheet-cli work` is the headless loop (HS2-118)** built on this: it drives one
> safe turn at a time — each turn takes the top Up Next ticket — until the queue drains,
> a `--max` cap, or a thrash stall (`--max-stall` turns with no queue change). It's the
> north-star bootstrap step (HS2 driving its own dev). Cross-turn session resume (one
> persistent instance rather than a fresh process per turn) is **HS2-3C1XK3**.

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
_which_ connection when several exist (e.g. a git-worktree worker's channel vs the
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

> **Status: registry + busy wired (HS2-107/34X6BW/4M67VN).** `hotsheet_aitools::ConnectionRegistry`
> — register/unregister/get/list/count over `Connection { id, project, tool, role
(Main|Worker|DriveSpawned), transport, pid, started_at }`, plus **busy as a derived
> sliding-window view**: `note_activity(id, now)` is one heartbeat both hooks and
> byte-stream/spinner inference feed, `is_busy`/`busy_count` read the window, and
> `set_idle` drops it on a `Done`. The clock is injected (deterministic). Live
> `TurnHandle` signals feed it in the server, and hosted terminals supply OSC-133/spinner
> inference.

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

**Ticket-level active work.** Connection busy state answers whether a tool connection is
doing something, while a non-expired ticket claim lease answers which ticket a worker is
actively holding. The ticket list projects that lease immediately after status with a
yellow, one-third-speed adaptation of the MIT-licensed svg-spinners `gooey-balls-2`
indicator. This is deliberately independent of the durable `started`
status, and `claim_count` remains retry history rather than presence. The client schedules
the nearest lease expiry locally so the indicator clears on time without network polling.
Self-claim workers acquire through `claim-next`; general orchestration and delegated
workers acquire and atomically start their assigned slug/ULID through exact `claim`, renew before expiry or
lengthy work, and release on completion, handoff, error, or feedback. A same-worker exact
claim retry is idempotent and does not inflate `claim_count`; another live holder is a
conflict, while an expired lease can be acquired as a new attempt. Every successful claim
advances Not Started to Started in the same durable write; later status values are preserved.

## 5.7 Permissions & user prompts (permission checks and other prompts)

A **host-side permission bridge**: "ask the user, get a decision," with each
plugin supplying only the transport-specific adapter (an ACP option-response, a
permission hook CLI, a hooks.json entry). When a tool wants approval to run a
command:

1. The tool's adapter routes the request to the bridge.
2. The bridge enqueues it (FIFO — concurrent requests preserved, not overwritten,
   an HS1 bug fixed in §12.10) and pushes it over the WebSocket to every client.
3. The UI shows a non-modal permission popup anchored to the owning project; the
   user allows/denies (with allow-once/always mapping onto persisted allow-rules).
4. The answer routes back to the connection that raised it.

Claude's interactive adapter listens to `PermissionRequest`, which fires only when
Claude Code's own permission modes and rules would display a prompt. Its earlier broad
`PreToolUse` integration ran before those native checks and incorrectly turned every
otherwise-safe Read/Edit/tool call into a Hot Sheet prompt (HS2-N4R6F3). Headless `-p`
sessions do not support that lifecycle event, so their launcher explicitly marks the
same installed adapter to retain `PreToolUse` bridging. Unmarked interactive
`PreToolUse` events emit no decision and preserve Claude's native permission flow.

Codex uses the same documented `PermissionRequest` input and nested allow/deny output
contract. Its plugin declaratively installs the shared adapter in `.codex/hooks.json`;
there is no tool-id setup branch. Codex loads the project hook from launch subdirectories
through the repository config layer and runs the absolute `hotsheet-cli permission-hook`
command installed by setup. Codex still owns the trust boundary: a new or changed project
hook is skipped until the user reviews its hash with `/hooks`. Hot Sheet does not pass
`--dangerously-bypass-hook-trust`. Before trust, or whenever route-back is absent or
unreachable, the adapter emits no decision and Codex presents its normal native prompt.
An unanswered request that reaches the generic bridge uses its existing eventual safe-deny
timeout; the generated provider hook timeout is slightly longer so that denial can be
returned rather than the hook process being killed first. Allow and deny use Codex's native
result; retries remain independent.

The client applies a user's decision optimistically: the popup and its clickable actions
disappear in the same render that begins the network request, preventing latency from
looking like a missed click or allowing duplicate answers. The presumed history entry is
kept when delivery succeeds. A communication failure removes that presumed history and
restores the request with an inline retryable error; authoritative resolution still wins
if another client answered while the request was in flight (HS2-66TBWX).

Both enqueue and resolution publish replayable event nudges. Resolution nudges matter when
another client or transport answers: every attached client refetches pending requests and
retains the disappeared request in notification history, including generic tools such as
`ToolSearch` whose action/details string is empty.

`Always Allow` rules are personal machine state, stored per primary project under
`${HOTSHEET_HOME}/permissions/<store-id>.json`; they are never written to the code or
ticket repository. `GET /permissions` advertises `always_allow_supported` on each
pending request so clients only render that action when durable rule storage is active.
The server retains an eventual safe-deny guard of 24 hours. This is intentionally much
longer than client-side automation windows: an ignored or otherwise hidden popup does
not advance a client's visible-presentation countdown and must not disappear after the
old five-minute transport timeout.

This is also the seam for **other prompts** the ticket mentions — the bridge is a
generic "the tool needs a human decision" channel, not just tool-permissions.

**External interactive terminals.** `hotsheet-cli launch <tool>` runs the plugin's
interactive `[launch]` command in the caller's existing terminal after installing its
setup artifacts and injecting the running server's `HOTSHEET_SERVER`/`HOTSHEET_SECRET`
route-back. From a linked code checkout, the ordinary `.hotsheet2/store` machine-local
link resolves the ticket store, so no `-C` is needed. This path is capability-gated:
Claude and Codex both declare native `PermissionRequest` adapters, so
`hotsheet-cli launch claude` and `hotsheet-cli launch codex` are supported. A tool without
that declared hook remains rejected rather than receiving misleading, unused route-back
environment. Codex permissions also remain supported through Hot Sheet's app-server drive
(`trigger`/`work`).

**The claim/lease primitive** (`coord`) is what keeps distributed work sane, and it
underpins the git-storage concurrency story ([02-ticket-storage.md](02-ticket-storage.md)
§2.7). Two regimes:

- **Single shared server (this section's default):** a self-directed worker atomically
  claims the top Up Next ticket (`claim-next`), while an orchestrated worker claims its
  exact assigned slug/ULID (`POST /tickets/{id}/claim`); either holds a renewable lease,
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
> exposing ticket CRUD plus git-backed Trash recovery through `hotsheet_restore`,
> `hotsheet_claim`, `hotsheet_claim_next`,
> `hotsheet_renew`, and `hotsheet_release`. It runs in
> **two modes over one `Backend` trait**, so the tool surface is identical either
> way — this is what lets a headless agent work **with or without a server**:
>
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
> `hotsheet_restore` calls the same `ops::restore` lifecycle as the CLI and web client;
> it accepts an optional checkout target, while explicit non-git provider connections
> return a capability error because their deletion lifecycle is provider-owned (HS2-GTNZ2Q).

AI tools reach tickets two ways, both over the one core:

- **MCP** — the `hotsheet_*` tool surface (create/update/get/query/claim/etc.).
  **Decided (maintainer, 2026-08-19): a small per-project MCP shim** spawned into
  each tool's config (as HS1 does with `channel.ts`), _not_ the server exposing MCP
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

- **Worklist-as-file** (`worklist.md`) — keep. The file-based contract lets _any_
  tool participate without the API.
- **Auto-context** (HS1 docs/4 §4.18) — **keep; critical (HS2-25).** Per-category and
  per-tag guidance the user configures is **injected into the generated worklist**
  so the AI tool gets the right context automatically for each ticket. It rides the
  worklist-as-file contract (the guidance is composed into `worklist.md` during
  generation — [03](03-indexing-and-query.md) §3.6), so it works for every tool with
  no per-tool code. **Built (HS2-BZBVAS):** HS1-compatible read-time category
  defaults plus global/shared/local overrides are resolved once in
  `hotsheet_ticketing::auto_context`; an explicit empty entry suppresses a default,
  category matching is exact, and tag matching is case-insensitive with matched tags
  sorted by key. The computed structured `{source,key,text}` blocks are returned by
  REST and both MCP modes for get/query/claim and rendered beneath each generated
  worklist row. They are never persisted in a ticket or the index.
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
  spawner reports the content it _would_ send"; here it's non-negotiable.) This is
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
_data_, and data has no ABI problem. Plugins are therefore layered:

- **Manifest-only plugins — the bulk.** A directory with a manifest (id,
  `detection`, `preferences`, `tier`, `transport` id, launch command, the
  MCP-config _format_) plus template files (instruction file, skills/rules tree).
  **No code, no ABI, no code sandbox** (there is no code). A common-shape tool
  (§5.3) ships as _just this_. Loaded identically into the CLI and the server, so
  `hotsheet setup <third-party-tool>` works headless.
- **Behavioral plugins — manifest + code**, only for the custom bits (a persistent
  channel, an app-server drive, a bespoke permission bridge). The execution boundary
  is chosen **by capability**:
  - **Subprocess protocol (stdio JSON-RPC)** for the **process-shaped behaviors** —
    `drive`/trigger, terminals, MCP. These are _already_ subprocess-shaped in HS2
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
(machine `${HOTSHEET_HOME:-~/.hotsheet2}/plugins/` or project `.hotsheet2/plugins/`) with no fork and no
recompile. So there is exactly one loader and one plugin shape; "first-party" is a
provenance/trust label (§ trust gate), not a separate code path.

**The loader lives in core** (`plugins`, [04](04-core-server-cli.md) §4.1) and reads
a search path: **bundled built-ins → `${HOTSHEET_HOME:-~/.hotsheet2}/plugins/` (machine) → project
`.hotsheet2/plugins/`**. Both binaries load the same registry, which is what lets a
headless CLI set up a project for a plugin the user dropped in.

**Trust gate (mandatory, not optional).** A manifest is inert data, but what it
_writes_ is a supply-chain surface: a plugin's instruction template steers an agent,
and its launch command _executes_. So:

- **Install-time consent** shows exactly what a plugin will write and what it will
  launch, and its **provenance** (first-party / signed / unsigned third-party).
- **`hotsheet plugin verify`** runs the §5.10 conformance suite against a plugin
  (against `hs-fake-agent`) — the acceptance test a third-party plugin must pass,
  since we can't gate someone else's plugin in our CI.
- Subprocess/WASM behavior runs under the least-privilege boundary above; a
  manifest-only plugin can _write_ but never _executes host code_.

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

### Provider-owned model discovery and defaults

Drivable plugin manifests declare model labels, effort levels, defaults, live-session
selection capabilities, and interactive-launch argument templates. A drive can also
expose the optional `RuntimeModelCatalogSource` capability. Generic hosts query that
capability without branching on a provider id: runtime ids, ordering, effort choices,
and an advertised default are authoritative, while manifest labels remain stable for
known ids. A valid manifest default (then the first live model) is the fallback when the
runtime does not advertise one.

Unsupported or unavailable providers use the complete manifest catalog. Successful and
failed runtime lookups are cached per plugin id and runtime version; a version change
invalidates the old result, while a transient version-probe failure retains the last good
catalog. `GET /ai-tools?refresh=true` explicitly retries same-version discovery, retaining
the last good catalog if that refresh fails. The web client uses that refresh path when
loading AI settings, and `hotsheet-cli ai-tools --json` uses the same capability/merge
core. Neither client maintains provider/model tables. Model catalogs are suggestions rather
than allowlists: settings, Drive overrides, command overrides, live conversations, and headless CLI settings may
name a nonblank model id that discovery did not return. Known models retain their model-specific
effort validation; an unlisted model may use an effort value already declared by that provider.
In the web client's Settings, Drive, and command-editor menus, `Other…` opens a focused exact-id dialog instead of
permanently occupying the main layout with a second model field. The selected custom id appears as
an ephemeral choice and is removed as soon as a catalog model is selected.
Machine-local defaults still validate the installed provider and are stored through
`GET`/`PUT /ai-settings` (or `hotsheet-cli ai-settings get|set`) in the global Hot Sheet 2
settings file.

The bundled Codex manifest therefore remains a useful offline fallback, while a reachable
Codex app-server supplies its current paginated `model/list` catalog at runtime.
OpenCode and Antigravity declare their installed-runtime catalog commands (`opencode models`
and `agy models`) instead of freezing account/configuration-dependent ids in the registry.
OpenCode applies a selected model through ACP `session/set_config_option`; Antigravity passes
the selected model and effort through its declared `--model`/`--effort` spawn flags.
Bundled provider manifests also describe offline fallback catalogs. The Claude manifest
tracks the CLI aliases (including `fable`) and advertises its supported effort levels so
settings, Drive, conversations, and interactive terminal launches expose the same choices
even though the channel transport does not provide a live model-catalog endpoint.
Connection creation accepts optional model/effort selections. A live turn may override
them only when the descriptor advertises `change_model` and/or `change_effort`. Interactive
AI terminals use the same plugin declarations to expand model/effort launch arguments. Each
expanded value remains one literal process argument, so spaces, quotes, dollar signs, and shell
metacharacters in a manually entered model id are never reparsed as shell syntax.

## 5.12 Cross-references

- Storage concurrency the claim primitive protects: [02-ticket-storage.md](02-ticket-storage.md) §2.7
- The core that hosts the plugin registry + settings model: [04-core-server-cli.md](04-core-server-cli.md) §4.1, §4.9
- Clients that render permission prompts / busy state / plugin preferences: [06-clients.md](06-clients.md)
- AI-tool integration testing (fake agent, conformance gate, drift layer): [12-code-organization-and-testing.md](12-code-organization-and-testing.md) §12.7.7
