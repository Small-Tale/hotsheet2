# 13. AI-Tool Drive / Transport Interface

> **Status: Design (HS2-67); first slice built (HS2-106).** The seam **every AI tool
> and the whole test harness (HS2-64) hang off**, so it's designed early. **One
> interface with optional capabilities each tool conforms to as applicable** — no
> single universal transport (maintainer, 2026-08-19). This is the trait spec + a
> conformance checklist. **Built (`crates/hotsheet-aitools`):** the `Drive` trait +
> `Transport` tag + `Target` + `DriveCtx` + `TurnHandle` (`is_busy`/`wait`/`interrupt`)
> + `DoneReason`, the injected `ProcessSpawner`/`SpawnedProcess` ports + the real
> `SystemSpawner`, and the **spawn-per-run `SpawnDrive`** (Codex `exec` shape),
> conformance-tested against a fake spawner (§13.7, minus the fake-agent parts).
> **Streaming view added (HS2-116):** `TurnHandle` now also has `next_event() ->
> Option<TurnEvent>` (`Output`/`PermissionAsked`/`Done`) — additive, so the sync
> spawn/app-server drives keep the default (`None` → use `wait`), while the Claude channel
> drive streams. The permission sink is still a later addition to `DriveCtx`; the
> connection registry already landed (HS2-107).
>
> **Claude channel drive built (HS2-116):** `ClaudeChannelDrive` + `ClaudeChannel` — a
> turn injected into a running `claude` stream-json session, streamed as `TurnEvent`s.
> This is the interface's acceptance test; see §13.6. It added the `next_event()`
> streaming view (additive).
>
> **Codex app-server drive built (HS2-110):** `AppServerDrive` — a play is a
> `turn/start` on a new/**resumed** thread against the running `codex app-server daemon`,
> **not a fresh process** (`Target` selects the thread; `turn/interrupt` backs interrupt).
> It's transport logic over an injected `AppServerClient` port (fake-tested). Codex's
> `[drive]` is now `app-server`, not `spawn`.
>
> **Live Codex client built (HS2-112):** `CodexAppServer` (`src/codex.rs`) is the real
> `AppServerClient` — a JSON-RPC engine (`CodexRpc`: a background reader thread
> demultiplexing responses / notifications / auto-answered approval `ServerRequest`s)
> speaking the verified `codex 0.148` protocol: `initialize`→`initialized`,
> `thread/start`|`thread/resume`, `turn/start` (text input), observing
> `turn/started`→`turn/completed` (busy→done), `turn/interrupt`. Threads open with
> `approvalPolicy:"never"` + `sandbox:"workspace-write"` (headless). The bytes ride an
> injected `RpcTransport` (`send`/`recv` line-oriented JSON — the verified framing):
> `StdioTransport` runs `codex app-server` **direct** (one persistent process per
> connection — many turns, no process per play), and `UdsWsTransport` speaks the shared
> **daemon**'s control socket so many connections reuse one codex instance. The whole
> engine is unit-tested against in-process **scripted daemons** (a loopback and a scripted
> WebSocket over a temp socket — no live `codex`). **Live-verified (2026-08-21, under
> HS2-103 safety in an isolated MCP-free `CODEX_HOME`):** a real turn via `StdioTransport`
> opened a thread and completed (`Completed`) with the HS1 dev instance untouched.
> **Shared-daemon path (HS2-115):** the earlier "does not serve JSON-RPC / undocumented
> protocol" reading was wrong. From codex 0.148 source, the daemon control socket is a
> **plain WebSocket** endpoint (server does tungstenite `accept_async` on the UDS — **no**
> auth token for the *local* socket; the `Authorization: Bearer` check is only on the
> network `ws://IP:PORT` remote-control path), carrying the *same* `initialize`→`thread/*`→
> `turn/*` JSON-RPC as WebSocket **text frames** on `ws://localhost/rpc`. The prior probe
> drew zero bytes because it wrote raw newline JSON where an HTTP/WebSocket upgrade was
> expected (and `codex app-server proxy` is a dumb `stdio_to_uds` byte relay that does *not*
> add the WS layer). `UdsWsTransport` now connects the UDS directly and frames JSON-RPC as
> WS text frames (a dedicated Tokio thread bridging to the sync reader/writer halves);
> proven end to end by a scripted-WS-daemon unit test **and live-verified (2026-08-21):**
> a real turn over the shared daemon opened a thread and completed (`Completed`) via the
> gated live test (`HOTSHEET_CODEX_LIVE=1`). Still open: selecting the shared-daemon transport
> from the live trigger (it currently always uses `StdioTransport`), and wiring approval
> `ServerRequest`s to the real permission bridge (§5.7, HS2-113) rather than auto-approving.

## 13.0 Current tool capabilities (verified 2026-08-21)

Drivability is the integration priority (a tool that can't be driven against a
persistent/continuous session is lower priority). Ground-truthed from the **installed
CLIs** + official docs, not HS1's notes:

| Tool | Persistent driving? | Mechanism | Transport | Priority |
|---|---|---|---|---|
| **Codex** 0.148 | **Yes (daemon)** | `app-server daemon` (JSON-RPC/control socket; `thread/start\|resume` + `turn/start\|interrupt`; `remote-control`+pairing); `exec-server`; `exec resume <id> "<prompt>"` no-daemon | `AppServer` | 1 |
| **Claude** 2.1.238 | **Yes (channel)** | MCP-channel injection into a running session; `-p --input-format stream-json`; `--resume`/`--continue` | `ClaudeChannel` | 2 |
| **OpenCode** 1.17.18 | **Yes (ACP/HTTP)** | `opencode acp` (live stdio client: initialize, new/load, prompt/update/cancel); `opencode serve` + `attach`; session mgmt | `Acp` | 3 |
| **Antigravity (agy)** 1.1.7 | **No daemon** | `agy --conversation <id> --print "…"` (spawn-per-turn, resumed thread). `agy-mcp` community bridges wrap this for *delegation*; Antigravity only **consumes** MCP (`.agents/mcp_config.json`), it is not exposed as a driveable server | `Spawn`+resume | 4 |

MCP-config setup targets differ per tool: Claude `.mcp.json`, Codex `.codex/config.toml`,
Antigravity `.agents/mcp_config.json` (Gemini `mcpServers`), OpenCode its own config.

## 13.1 The problem the interface must absorb

"Drive" = the app tells a running AI tool to do something and observes it. The three
shapes differ deeply:

| | **Persistent channel** (Claude) | **Spawn / app-server** (Codex) | **ACP** (OpenCode, Goose) |
|---|---|---|---|
| trigger | inject a `<channel>` event into a long-lived session | `codex exec --json` per turn, *or* a JSON-RPC call to the app-server daemon | `session/prompt` |
| done | Stop hook / channel idle | process exit / turn-end message | `stopReason` |
| busy | hooks + spinner | process alive / turn in-flight | `session/update` |
| permissions | channel permission notification | `.codex/hooks.json` PreToolUse | `session/request_permission` |
| lifecycle | one persistent process | one-shot **or** a warm daemon | one process per session |

The interface must express all of these **without the caller ever branching on the
tool id** (the §5.1 rule).

## 13.2 Shape of the interface

A `Drive` is a **required core + optional sub-capabilities** (absence = not
supported). Rust sketch:

```rust
/// Identity — DECLARATIVE, client-safe (§05 §5.3), no I/O.
struct DriveInfo { transport: Transport }          // Transport is a data tag, not behavior

/// Behavioral — server-side. A tool implements the subset it supports.
trait Drive {
    /// REQUIRED. Send one prompt/turn to a target connection. Returns a handle the
    /// host tracks. May start a process (sync) OR POST to a running session (async).
    fn run(&self, target: Target, content: &str, ctx: &DriveCtx) -> Result<TurnHandle>;

    // OPTIONAL sub-capabilities (Option-returning accessors, not bool flags):
    fn interrupt(&self) -> Option<&dyn Interrupt> { None }   // codex yes, agy no
    fn reset(&self)     -> Option<&dyn Reset>     { None }
    fn service(&self)   -> Option<&dyn BackingService> { None } // codex app-server; maybe Claude
}
```

- **`run` is the only required method.** Everything else is optional and probed by
  presence (`drive.interrupt().is_some()`), never a `supportsInterrupt: bool` that can
  drift (the §132.11.2 lesson).
- **`run` returns a `TurnHandle`** — an abstraction over "the thing now happening"
  that carries **status + done/busy signals** back (see §13.4), so the host has one
  way to observe a turn regardless of transport.

## 13.3 The four things `run` must carry uniformly

The ticket calls these out; here's how each is expressed once:

1. **Transport identity** — a **declarative data tag** on the plugin
   (`Transport::{ClaudeChannel, Spawn, AppServer, Acp}`), client-safe, so the client
   and server agree without a mirror (the §132.11.7 drift). It routes `run` to the
   right implementation; it is *identity, not behavior*.
2. **The target selector** — `run(target, …)`: which live connection to hit when
   several exist (the main channel vs. a git-worktree worker's channel). The host's
   **connection registry** ([05](05-ai-tool-plugins.md) §5.6) enumerates connections;
   `Target` picks one (or "the default").
3. **Permission-request surfacing** — a driven turn that needs approval calls the
   host's **permission bridge** ([05](05-ai-tool-plugins.md) §5.7) through a
   per-transport adapter (channel notification / hooks CLI / ACP option response).
   The `DriveCtx` hands the drive a `PermissionSink`; the drive never talks to the UI.
4. **Busy signaling + done-detection** — unified in the `TurnHandle` (§13.4).

## 13.4 `TurnHandle` — one way to observe any turn

```rust
struct TurnHandle { /* … */ }
impl TurnHandle {
    fn events(&self) -> impl Stream<Item = TurnEvent>;   // Busy | Output | PermissionAsked | Done(reason)
}
enum TurnEvent { Started, Busy, Idle, Output(Bytes), PermissionAsked(PermReq), Done(DoneReason) }
```

Each transport **produces `TurnEvent`s from its native signal**, so the host consumes
one stream:
- **Claude:** Stop/PreToolUse/PostToolUse hooks + the spinner heuristic → Busy/Idle;
  channel idle / Stop → Done.
- **Codex spawn:** process alive → Busy; exit code → Done. **App-server:** turn-start/
  turn-end messages → Busy/Done.
- **ACP:** `session/update` → Busy/Output; `stopReason` → Done.

> **Built (HS2-PEQ6Q8):** `AcpSession` speaks ACP v1 newline-delimited JSON-RPC over
> an injected transport; `AcpStdio` launches `opencode acp`. It negotiates initialize,
> creates/loads sessions, streams message/usage/tool updates, maps prompt stop reasons,
> and sends cancellation. Unknown client requests receive an empty response and permission
> requests select a reject option by default. A fast version-pinned contract cassette plus
> the gated `HOTSHEET_OPENCODE_LIVE=1` smoke test form the OpenCode drift oracle.

**Busy is thus a derived view, not a per-tool API** — matching HS1's dual
hook+spinner model, generalized. The connection registry's `isBusy` reads the latest
`TurnEvent`.

## 13.5 Optional: a long-lived backing service

Some drives have a **daemon** behind them (Codex app-server; Claude's channel
arguably). `Drive::service()` returns a `BackingService` when present — is-it-on,
health, `prestart` (warm it up), `note_terminal_launch`, must-a-terminal-wait. A tool
without one returns `None`, and no generic caller ever imports a tool's daemon module
(closing the §132.11.2 leak by construction).

**Built (HS2-112/HS2-115, Codex):** `ensure_codex_daemon(program)` is the concrete
BackingService prestart — it runs `codex app-server daemon start` (idempotent: "start …
if not already running") before a `UdsWsTransport` connects to the daemon's control
socket at `codex_control_socket_path(codex_home)`. Every `CodexAppServer` connection over
that transport talks to the **same** daemon, so plays reuse one persistent instance
rather than launching a process each time. `ensure_codex_daemon_in(program, codex_home)`
targets a specific (isolated) home.

**Wired into the trigger (HS2-B7C66H, live-verified 2026-08-21):** `hotsheet-cli trigger
codex --shared-daemon` builds a **daemon-ready isolated `CODEX_HOME`** (the HS2-YRDQNX
MCP-free home, but under a short root so the control socket fits `sun_path`, with the managed
standalone install symlinked in), starts the daemon for *that* home, and drives the turn over
`UdsWsTransport` — so MCP isolation holds *and* one codex instance is reused. Available on
both `trigger` and `work` via `--shared-daemon` (off by default: a fresh `app-server` process
per connection). **Lifecycle handled (HS2-9M6T68):** the isolated home stops its daemon on
drop (`ensure_codex_daemon_in` starts it, `stop_codex_daemon_in` tears it down), so a run
never orphans a codex process; the `work` loop is the best case (one daemon reused across all
turns, torn down at loop end — live-verified 2026-08-22). Still open: folding
`ensure_codex_daemon` behind a `Drive::service()` accessor, and a *stable* per-project home if
cross-invocation reuse is ever wanted.

## 13.6 Why this isn't Claude-shaped (the acceptance test)

> **Claude channel drive built (HS2-116).** `ClaudeChannelDrive` + `ClaudeChannel`
> (`src/claude.rs`) — a play is **one user message injected into a running, persistent
> `claude` stream-json session** (the HS1 play-button model), observed as an **async
> `TurnEvent` stream** (`Output` … → `Done`), not a single terminal wait. Verified
> `claude 2.1.238` protocol: `claude -p --input-format stream-json --output-format
> stream-json [--resume <id>]`; input `{"type":"user",…}`; output `system`/`init`
> (session id), `assistant` (output), `result` (turn done). Same NDJSON framing as codex,
> so it reuses the injected `RpcTransport`/`StreamChild` plumbing: `ClaudeStreamTransport`
> spawns real `claude`, tests inject a **scripted claude**. This is what forced the
> `TurnHandle::next_event()` streaming view (additive; sync drives keep the default).
> **Live-verified (2026-08-21, isolated temp cwd + strict empty MCP config → nothing else
> reachable):** a real turn streamed `Output("pong")` then `Done(Completed)` with the
> session id captured and the HS1 dev instance untouched; gated ignored test
> (`HOTSHEET_CLAUDE_LIVE=1`). Follow-ups: the permission bridge (§5.7, HS2-113 — the drive
> emits `PermissionAsked` but runs a safe mode) and a channel interrupt.

The interface is only real if the tool it was **not** designed around fits. Checks:
- **Claude** needs `run` to be **async** (a turn on a running session) → `run` returns a
  `Result<TurnHandle>` whose `next_event()` streams `Output`/`PermissionAsked`/`Done`; the
  spawn/app-server drives are sync under the same signature (they return `None` from
  `next_event` and callers use `wait`). **Verified by construction (HS2-116).**
- **Codex** needs **`interrupt`** and a **backing service**; Claude declares neither,
  and nothing breaks — absence is the signal.
- **ACP** needs permission-*as-a-response* (not a hook) → the `PermissionSink` adapter
  is per-transport; the bridge sees a uniform `PermReq`.
- If a **fourth** shape needs a *new required* method or a third `run` allowance, the
  interface is accreting rather than generalizing — that's the red flag to watch
  (HS1's exact criterion, §132.11.5).

## 13.7 Conformance checklist (what the fake-agent suite verifies, HS2-64)

Every plugin with a `drive` passes:
- `transport` is a valid tag; the client-side `transportFor(id)` agrees (no mirror).
- `run` with an **injected spawner/poster** reports the exact content it would send
  (no real tool) and returns a `TurnHandle`.
- The `TurnHandle` emits `Busy` then `Done` for a scripted `hs-fake-agent` turn.
- A scripted permission request surfaces as one `PermReq` through the bridge and the
  answer routes back.
- Declared optional caps behave: `interrupt` (if present) stops a turn; `service`
  (if present) reports health + prestart.
- Absent caps return `None` and are never called.

## 13.8 Open questions
- **`Target` when zero connections exist** — does `run` *start* one (spawn) or error
  (channel needs a running session)? Lean: spawn-drives auto-start; channel-drives
  surface "not connected" (HS1's behavior).
- **Streaming granularity** of `Output` events (raw bytes vs. parsed) — keep raw at
  this layer; parsing is a consumer concern.
- **Cancellation semantics** for async `run` (drop the handle vs. explicit `interrupt`).

## 13.9 Build plan (follow-ups)
- HS2-67 (this) = the spec. Implementation lands in **HS2-9** (plugin host + Claude
  drive) and **HS2-66** (Codex drive); the conformance checklist is built in **HS2-64**.
  No new ticket needed — those three own the build.

## 13.10 Cross-references
- Drive/trigger overview + optional caps: [05-ai-tool-plugins.md](05-ai-tool-plugins.md) §5.5
- Connection registry + busy: [05-ai-tool-plugins.md](05-ai-tool-plugins.md) §5.6
- Permission bridge: [05-ai-tool-plugins.md](05-ai-tool-plugins.md) §5.7
- Testability + the fake agent: [12-code-organization-and-testing.md](12-code-organization-and-testing.md) §12.7.7
