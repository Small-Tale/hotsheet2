# 13. AI-Tool Drive / Transport Interface

> **Status: Design (HS2-67); first slice built (HS2-106).** The seam **every AI tool
> and the whole test harness (HS2-64) hang off**, so it's designed early. **One
> interface with optional capabilities each tool conforms to as applicable** — no
> single universal transport (maintainer, 2026-08-19). This is the trait spec + a
> conformance checklist. **Built (`crates/hotsheet-aitools`):** the `Drive` trait +
> `Transport` tag + `Target` + `DriveCtx` + `TurnHandle` (`is_busy`/`wait`/`interrupt`)
>
> - `DoneReason`, the injected `ProcessSpawner`/`SpawnedProcess` ports + the real
>   `SystemSpawner`, and the **spawn-per-run `SpawnDrive`** (Codex `exec` shape),
>   conformance-tested against a fake spawner (§13.7, minus the fake-agent parts).
>   **Streaming view added (HS2-116):** `TurnHandle` now also has `next_event() ->
Option<TurnEvent>` (`Output`/`PermissionAsked`/`Done`) — additive, so the sync
>   spawn/app-server drives keep the default (`None` → use `wait`), while the Claude channel
>   drive streams. The permission sink is still a later addition to `DriveCtx`; the
>   connection registry already landed (HS2-107).
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
> `turn/started`, streaming `item/agentMessage/delta` as user-visible `Output`, then
> `turn/completed` (busy→done), `turn/interrupt`. A completed `agentMessage` is the
> fallback when a transport omits deltas and is deduplicated when deltas were received.
> Threads open with the chat's selected
> `approvalPolicy` (default `"never"`; `on-request`/`on-failure`/`untrusted` route approvals to
> the permission bridge, HS2-NHRDMW) + `sandbox:"workspace-write"` on both `thread/start` and
> `thread/resume`; a Claude permission-mode value is never forwarded (it maps to `never`). The bytes ride an
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
> auth token for the _local_ socket; the `Authorization: Bearer` check is only on the
> network `ws://IP:PORT` remote-control path), carrying the _same_ `initialize`→`thread/*`→
> `turn/*` JSON-RPC as WebSocket **text frames** on `ws://localhost/rpc`. The prior probe
> drew zero bytes because it wrote raw newline JSON where an HTTP/WebSocket upgrade was
> expected (and `codex app-server proxy` is a dumb `stdio_to_uds` byte relay that does _not_
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

| Tool                        | Persistent driving? | Mechanism                                                                                                                                                                                                                              | Transport       | Priority |
| --------------------------- | ------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------- | -------- |
| **Codex** 0.148             | **Yes (daemon)**    | `app-server daemon` (JSON-RPC/control socket; `thread/start\|resume` + `turn/start\|interrupt`; `remote-control`+pairing); `exec-server`; `exec resume <id> "<prompt>"` no-daemon                                                      | `AppServer`     | 1        |
| **Claude** 2.1.238          | **Yes (channel)**   | MCP-channel injection into a running session; `-p --input-format stream-json`; `--resume`/`--continue`                                                                                                                                 | `ClaudeChannel` | 2        |
| **OpenCode** 1.17.18        | **Yes (ACP/HTTP)**  | `opencode acp` (live stdio client: initialize, new/load, prompt/update/cancel); `opencode serve` + `attach`; session mgmt                                                                                                              | `Acp`           | 3        |
| **Antigravity (agy)** 1.1.7 | **No daemon**       | `agy --conversation <id> --print "…"` (spawn-per-turn, resumed thread). `agy-mcp` community bridges wrap this for _delegation_; Antigravity only **consumes** MCP (`.agents/mcp_config.json`), it is not exposed as a driveable server | `Spawn`+resume  | 4        |

MCP-config setup targets differ per tool: Claude `.mcp.json`, Codex `.codex/config.toml`,
Antigravity `.agents/mcp_config.json` (Gemini `mcpServers`), OpenCode its own config.

## 13.1 The problem the interface must absorb

"Drive" = the app tells a running AI tool to do something and observes it. The three
shapes differ deeply:

|             | **Persistent channel** (Claude)                        | **Spawn / app-server** (Codex)                                                     | **ACP** (OpenCode, Goose)    |
| ----------- | ------------------------------------------------------ | ---------------------------------------------------------------------------------- | ---------------------------- |
| trigger     | inject a `<channel>` event into a long-lived session   | `codex exec --json` per turn, _or_ a JSON-RPC call to the app-server daemon        | `session/prompt`             |
| done        | Stop hook / channel idle                               | process exit / turn-end message                                                    | `stopReason`                 |
| busy        | hooks + spinner                                        | process alive / turn in-flight                                                     | `session/update`             |
| permissions | native `PermissionRequest` hook / channel notification | app-server approval request or interactive `.codex/hooks.json` `PermissionRequest` | `session/request_permission` |
| lifecycle   | one persistent process                                 | one-shot **or** a warm daemon                                                      | one process per session      |

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
   right implementation; it is _identity, not behavior_.
2. **The target selector** — `run(target, …)`: which live connection to hit when
   several exist (the main channel vs. a git-worktree worker's channel). The host's
   **connection registry** ([05](05-ai-tool-plugins.md) §5.6) enumerates connections;
   `Target` picks one (or "the default").
3. **Permission-request surfacing** — a driven turn that needs approval calls the
   host's **permission bridge** ([05](05-ai-tool-plugins.md) §5.7) through a
   per-transport adapter (channel notification / hooks CLI / ACP option response).
   The `DriveCtx` hands the drive a `PermissionSink`; the drive never talks to the UI.
   External interactive Claude and Codex sessions use native `PermissionRequest` hooks:
   the plugin declares the provider config target/event/command, setup installs the same
   normalized adapter, and a hook transport failure preserves the provider's own prompt;
   the bridge's eventual unattended timeout remains safe-deny. Codex keeps its documented
   hash-review trust gate via `/hooks`.
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

**Observing a TUI's thread (HS2-MN865E):** a second `UdsWsTransport` connection that sends
only `initialize` and `thread/resume {threadId}` rejoins a thread the interactive TUI is
running, and receives that thread's `turn/started`, `error {willRetry}`, and
`turn/completed {turn.status, turn.error}` notifications. The server uses this read-only
rejoin for Codex halt detection ([05](05-ai-tool-plugins.md) §5.7). It never starts the
daemon or a turn. The managed daemon may run a newer build than the CLI: on 0.162.1 it
installed and ran 0.163.0 under `$CODEX_HOME/packages/app-server-daemon/`.

**Wired into the trigger (HS2-B7C66H, live-verified 2026-08-21):** `hotsheet-cli trigger
codex --shared-daemon` builds a **daemon-ready isolated `CODEX_HOME`** (the HS2-YRDQNX
MCP-free home, but under a short root so the control socket fits `sun_path`, with the managed
standalone install symlinked in), starts the daemon for _that_ home, and drives the turn over
`UdsWsTransport` — so MCP isolation holds _and_ one codex instance is reused. Available on
both `trigger` and `work` via `--shared-daemon` (off by default: a fresh `app-server` process
per connection). **Lifecycle handled (HS2-9M6T68):** the isolated home stops its daemon on
drop (`ensure_codex_daemon_in` starts it, `stop_codex_daemon_in` tears it down), so a run
never orphans a codex process; the `work` loop is the best case (one daemon reused across all
turns, torn down at loop end — live-verified 2026-08-22). Still open: folding
`ensure_codex_daemon` behind a `Drive::service()` accessor, and a _stable_ per-project home if
cross-invocation reuse is ever wanted.

## 13.6 Why this isn't Claude-shaped (the acceptance test)

> **Claude channel drive built (HS2-116).** `ClaudeChannelDrive` + `ClaudeChannel`
> (`src/claude.rs`) — a play is **one user message injected into a running, persistent
> `claude` stream-json session** (the HS1 play-button model), observed as an **async
> `TurnEvent` stream** (`Output` … → `Done`), not a single terminal wait. Verified
> `claude 2.1.238` protocol: `claude -p --input-format stream-json --output-format
stream-json [--resume <id>]`; input `{"type":"user",…}`; output `system`/`init`
> (session id), `assistant` (output), `result` (turn done). Same NDJSON framing as codex,
> so it reuses the injected `RpcTransport`/`StreamChild` plumbing: `ClaudeStreamTransport`
> spawns real `claude`, tests inject a **scripted claude**. This is what forced the
> `TurnHandle::next_event()` streaming view (additive; sync drives keep the default).
> Claude may repeat an identical complete `assistant` record during one turn, including
> retries that change or omit the provider message id. The adapter suppresses exact
> consecutive text projections independent of that unstable identity, and also remembers
> identified projections across the turn. Distinct fragments, repetition after intervening
> different output, and every other provider retain ordinary append semantics.
> **Per-turn processes (HS2-M5GN19):** the live drive spawns one `claude --resume` process
> per turn and attaches it with `ClaudeChannel::connect_single_turn`. In that mode a
> `result` does not end the turn. The channel closes stdin and keeps streaming until the
> process exits, so assistant text, tool use, and usage produced after an early or extra
> `result` reach the turn. Before this, the child was orphaned unobserved and the client
> showed an empty `0 tokens` turn while the session kept acting. The last `result` decides the
> outcome, and an exit with no result is a failure.
>
> A live 2.1.302 capture (HS2-4JPDJA, cassette `claude-2.1.302-resume-background.json`)
> pinned the condition behind the early `result`. A turn that leaves background work running
> (a `run_in_background` command or an `Agent` sub-agent, which backgrounds by default) gets
> its first `result` while that work runs. When the work finishes, Claude self-starts a
> follow-up turn: a new `system init`, then a `result` with
> `origin.kind = "task-notification"`. That turn belongs to the same process, so the channel
> drains it. No zero-usage result was observed. The scripted `PrematureResult` shape stays
> covered.
>
> Result `usage` tokens are per result (main agent only) and add across the turn.
> `total_cost_usd` is cumulative for the whole **session**, not just this process: it grows
> across results in one process, and a `--resume` process continues from the resumed
> session's total. A turn keeps the highest cost it sees and subtracts a baseline. In a
> persistent process the baseline is the total the process had already reported. For a
> resumed process it is the host's recorded total: `TurnDone::session_cost_usd` is stored on
> the client-drive session catalog entry and passed back as
> `LiveTrigger::session_cost_baseline_usd`. A total below the baseline means the baseline
> belonged to another session, and the raw total stands. Without this, every resumed turn
> re-reported all earlier turns' cost. A result reporting no model work (zero tokens and no
> cost) contributes no usage. When a successful turn streamed
> no assistant text, its final `result` text becomes the turn's output. The persistent
> multi-turn mode (`ClaudeChannel::connect`) still ends a turn at its first `result`.
> **Stopping a turn (HS2-V0SWSM):** Claude's stream-json channel has no interrupt message,
> so the drive stops a turn by ending its process. `StreamChild` keeps the spawned `Child`
> shared between the reader half and a `ProcessSignals` handle
> (`RpcTransport::process_signals`); `procio::stop_process` sends SIGTERM, waits a 5 s grace
> period, then SIGKILLs a process that ignored it. Exit is observed with `try_wait`, which
> reaps the child, so a signal can never reach a recycled pid. `ClaudeChannelDrive` therefore
> reports `supports_interrupt`, and the plugin manifest declares `interrupt = true`.
> `TurnHandle::interrupt` stops the process and reports `Done(Interrupted)`. A Stop request
> also wakes a turn blocked waiting for Claude's next event: the live drive registers a
> `TurnControl::on_interrupt` callback that only wakes the turn, which then stops the process
> on its owner thread. When no event arrives within the 600 s turn timeout, the turn stops the
> process the same way and fails with a timeout diagnostic instead of leaving it working
> unobserved. This stays the decision for a single-turn process still running at the
> timeout (HS2-4JPDJA): Hot Sheet stops it rather than detaching it, because nothing would
> observe its output, tool use, or cost. The timeout is per event. When Claude reports a
> nonempty `system/background_tasks_changed.tasks` list, the silence bound becomes 30 minutes;
> an empty list restores 600 s. Malformed task lists leave the latest valid state intact.
> Every observed event resets the applicable bound. Even after an early successful `result`,
> exceeding the bound stops the process and fails the turn (HS2-PZMKXR). The polite SIGTERM
> reaches only the `claude` process, which cleans up its own
> tools. When it has to be force-killed, its descendant tree (Bash tool commands, MCP
> servers) goes too: each descendant is frozen with SIGSTOP, the tree is re-walked until no
> new process appears, and then every frozen process and `claude` get SIGKILL. The turn
> process deliberately stays in the caller's process group, so a foreground
> `hotsheet-cli trigger` still delivers terminal Ctrl-C to it (HS2-M08WMC).
> **Live-verified (2026-08-21, isolated temp cwd + strict empty MCP config → nothing else
> reachable):** a real turn streamed `Output("pong")` then `Done(Completed)` with the
> session id captured and the HS1 dev instance untouched; gated ignored test
> (`HOTSHEET_CLAUDE_LIVE=1`). Follow-up: the permission bridge (§5.7, HS2-113 — the drive
> emits `PermissionAsked` but runs a safe mode).

The interface is only real if the tool it was **not** designed around fits. Checks:

- **Claude** needs `run` to be **async** (a turn on a running session) → `run` returns a
  `Result<TurnHandle>` whose `next_event()` streams `Output`/`PermissionAsked`/`Done`; the
  spawn/app-server drives are sync under the same signature (they return `None` from
  `next_event` and callers use `wait`). **Verified by construction (HS2-116).**
- `TurnEvent::NativeActivity` carries an exact narratable protocol payload plus its
  declared activity-source id. Codex emits non-message completed app-server transcript
  items as activity while agent-message deltas become user-visible `Output`; Claude emits
  tool-use payloads in its verified hook contract. Mapping and ticket/project/session
  attribution remain host responsibilities, so the drive crate stays independent of the
  ticket activity model.
- **Codex** needs **`interrupt`** and a **backing service**; Claude declares no backing
  service, and nothing breaks — absence is the signal. Claude's interrupt is implemented by
  ending its per-turn process rather than by a protocol message (HS2-V0SWSM), which the
  interface absorbs without a new method.
- **ACP** needs permission-_as-a-response_ (not a hook) → the `PermissionSink` adapter
  is per-transport; the bridge sees a uniform `PermReq`.
- If a **fourth** shape needs a _new required_ method or a third `run` allowance, the
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

## 13.8 Resolved transport semantics

- **`Target` when zero connections exist:** spawn drives start a process; channel,
  ACP, and app-server drives surface `NotConnected` when their backing service/session
  is unavailable.
- **Streaming granularity:** `Output` preserves transport output at this layer;
  protocol-specific activity parsing is a consumer/adapter concern.
- **Cancellation:** capable drives expose explicit `TurnHandle::interrupt`; dropping a
  handle is not the cancellation contract.

## 13.9 Client-owned drive lifecycle (HS2-5DGFG2)

The server retains a prepared drive and its isolated tool home across sequential client
turns. A client creates/attaches it with `POST /drive/connections`, supplying the checkout
id along with the tool and stable connection id, then sends arbitrary text
with `POST /drive/connections/{id}/turns`, and resumes from either the retained session id
or an explicit one. Only one turn may run per connection. A thread-safe `TurnControl`
delivers interruption back to the concrete turn on its owner thread; the wire advertises
`interrupt` in `actions` only when that drive actually implements it.

The server resolves that checkout through its project registry and prepares the tool with
the checkout's code root as both its process directory and `HOTSHEET_PROJECT`. The ticket
store is deliberately not a substitute for the checkout: UI connections are scoped by
checkout plus tool so Codex and Claude conversations cannot leak across either boundary.
The connection also retains its selected linked ticket-source identity. The checkout default
is used when present; otherwise the caller must provide `source`. A local Git source supplies
the `SafeTrigger` store boundary and receives usage, native activity, and turn-event identity;
non-local providers reject this activity capability explicitly instead of falling back to the
machine server's default store.

Connection changes emit replayable `drive_updated` events. Clients refresh from that
signal over the existing WebSocket or long-poll fallback; simple periodic polling is not
part of this contract. Preparation and running remain plugin-routed through `SafeTrigger`,
including HS1 refusal, an isolated Codex home, permission bridging, and the cross-platform
`hotsheet` shim resolving the real sibling `hotsheet-cli` rather than the server process.
Unix hosts reuse the shared Codex daemon; Windows uses the direct app-server transport
because the daemon control channel is Unix-domain-socket based.
The HTTP lifecycle is fake-backend E2E tested and has a credentials-gated real Codex test.
Separately, `hotsheet-cli launch codex` uses the ordinary interactive CLI with a
project-local `.codex/hooks.json` `PermissionRequest` adapter and running-server route-back;
it is intentionally not the isolated app-server drive and does not bypass Codex hook trust.

**Raw client stream (HS2-060HQJ):** every `TurnEvent` is projected into a stable tagged
`turn_event` envelope carrying connection id, the originating `turn_id`, and, for
autonomous ticket work, ticket id. **Turn correlation (HS2-14X4AF):** a client may name its
turn with `turn_id` in `POST /drive/connections/{id}/turns` (1-128 ASCII letters, digits,
`-`, `_`, `.`, `:`; anything else is `400`), otherwise the server assigns a ULID; dist-loop
worker turns always get a server ULID. A busy connection reports the running turn's
`turn_id` and an idle one omits it. Clients apply a correlated event only to the matching
turn, so a delayed or repeated event from an earlier turn on the same connection cannot
finish or fill a newer one; events from older servers omit `turn_id` and fall back to the
client's single active turn. **Turn timing (HS2-XV1VCR):** every envelope carries the server's
`at_ms` (Unix milliseconds) and, when a tool call of the turn is open after the event,
`in_flight_tool` `{name, started_at_ms, count}` (the oldest open call and how many are open); an
envelope with `at_ms` and no `in_flight_tool` means no call is open. A busy connection also
reports `turn_started_at_ms`, `last_event_at_ms`, and `in_flight_tool`, so a reload or a second
client seeds the same state; an idle one omits all three. Tool lifecycle edges are read
provider-neutrally by `hotsheet_aitools::tool_progress::tool_transition`: a Claude
`PreToolUse` record (with its `tool_use_id`) opens a call and `PostToolUse` /
`PostToolUseFailure` closes it; sub-agent records (`parent_tool_use_id`) never open their own
call, and `Done` closes every call. Codex driven turns map a tool-like `item/started`
(command execution, MCP or dynamic tool call, web search, file change, sub-agent call, image
generation) to native activity with source `codex-item-started`, which opens a call keyed by the
item id; that item's `item/completed` transcript record (`codex-transcript`) closes it
(HS2-90FMR2). Message and reasoning items are never in flight, and items of another thread or
turn are ignored. Older servers omit these fields. It rides the existing WS and genuine-long-poll replay ring; each replayable WS event carries
the same monotonic cursor a reconnecting client supplies to `/ws/poll`. Output,
permission requests, usage, native activity, a coalescing notice, and terminal done are
additive event variants. Clients retain unknown variants without failing their live loop.
Per turn, the server admits 128 ordinary events, then up to eight late permission/usage
events per kind, coalesces exact dropped counts immediately before an always-preserved
`done`, and truncates a pathological individual output chunk at 65,536 characters.
Native activity payloads above 65,536 encoded bytes become a small typed truncation marker.

**Driven turn failures (HS2-AZVE3P):** failed `done` events include optional provider
`message` text, bounded to 65,536 Unicode characters with an ellipsis when shortened;
completed and interrupted events omit it. Provider failures and transport
errors save the diagnostic in the client connection's `last_error`. Accepting a new turn
clears it immediately. Client reconnect reconciles a persisted active assistant against an idle
server failure without treating an optimistic request still being sent as complete. Codex retryable errors remain in progress until a terminal outcome;
a later successful completion or interruption clears the pending diagnostic. Claude result
errors, ACP prompt errors and unexpected stop reasons preserve their provider message, with
a stable fallback when no nonblank diagnostic is available. Interrupted turns are not failures.

**Durable resume (HS2-KH8FBA):** completed client turns update a machine-local session
catalog under `${HOTSHEET_HOME}/drive/<project>/sessions.json`; `/drive/sessions` returns
that project's most-recent-first choices. Recreating a connection with an explicit session
targets Claude `--resume` or Codex `thread/resume`. Client-owned Codex records the opaque
home identity with each session and reuses that durable, MCP-isolated home even when a
different client connection resumes it after server restart. On Unix,
the persistent directory itself uses a stable short `${HOTSHEET_HOME}/d/<hash>` path so
Codex cannot canonicalize a symlink back into a control-socket path beyond `sun_path`.
Existing data migrates atomically from the historical
`${HOTSHEET_HOME}/drive/<project>/homes/<home>` location, which remains as a compatibility
symlink. Windows uses the durable directory directly with app-server. Two connections may
attach to one session, but a manager-level session gate permits only one simultaneous turn;
interrupt releases the gate for a later resume. Autonomous ticket driving similarly retains
a per-ticket session id across Continued passes and uses a stable isolated home for each
ticket.

The web client also persists its validated receipt-ordered conversation projection locally.
After a client or server restart it reads the catalog newest-first, recreates at most the latest
session for each stable connection id, and attaches the retained messages/activity to that
connection. Recovery is project-root scoped and failure-isolated, so stale/corrupt local data or
one unavailable provider cannot hide another resumable conversation (HS2-YHQCS2).

## 13.10 Build plan (follow-ups)

- HS2-67 (this) = the spec. Implementation lands in **HS2-9** (plugin host + Claude
  drive) and **HS2-66** (Codex drive); the conformance checklist is built in **HS2-64**.
  No new ticket needed — those three own the build.

## 13.11 Cross-references

- Drive/trigger overview + optional caps: [05-ai-tool-plugins.md](05-ai-tool-plugins.md) §5.5
- Connection registry + busy: [05-ai-tool-plugins.md](05-ai-tool-plugins.md) §5.6
- Permission bridge: [05-ai-tool-plugins.md](05-ai-tool-plugins.md) §5.7
- Testability + the fake agent: [12-code-organization-and-testing.md](12-code-organization-and-testing.md) §12.7.7
- Messaging a live interactive session: §13.12 below

## 13.12 Messaging a live interactive session (session push, HS2-PEJ962)

**Status: Partial**. Shipped: the operation, the OpenCode adapter, the `codex queue` adapter
(HS2-FKM97G), the Claude inbox-socket adapter (HS2-M15CHN), and terminal-drawer and headless CLI
message callers (HS2-NN6EXP). Automatic ticket-note delivery is tracked separately (HS2-25TP3N).

Hot Sheet can push a message into an _interactive_ AI session that one of its terminals
launched (Connect / `POST /terminals {connect}`), so an attached agent hears about a new note or
an answered question without a human typing it. Each tool reaches its live session differently,
so the host keeps one small trait, `hotsheet_aitools::session_push::SessionPush`, with one
adapter per **mechanism**. The plugin manifest selects it; core code never branches on a tool id.

```toml
[session_push]
mechanism = "opencode-http"          # the adapter
port_args = ["--port", "{port}"]     # appended to [launch]; {port} = a free loopback port
password_env = "OPENCODE_SERVER_PASSWORD"  # carries a per-session random password
```

A tool **without** `[session_push]` (or with a mechanism this host has no adapter for) is
reported as unsupported. Antigravity declares none: a user-started `agy` TUI has no attach
point, and only Hot Sheet-owned `stream-json` processes could be messaged.

### Operation

- `POST /connections/{id}/message` with exactly one of `{"text": "…", "banner"?: "…"}` (a prompt;
  `banner` also shows a short visible notice when the mechanism has one) or
  `{"command": "…"}` (a tool UI command). The connection id is the terminal id.
- `200 {"delivered": true, "mechanism", "session_id"?, "confirmed"}`. `confirmed: false` means the
  tool accepted the request but cannot confirm it acted (OpenCode answers `true` to every TUI
  command, including unknown ones).
- Errors carry a stable `code`: `404 connection_not_found`, `422 session_push_unsupported`
  (named with the tool's display name), `400` for a malformed or empty message,
  `502 session_unreachable` (nothing listening), `502 session_rejected` (an error status).
- `GET /connections` lists `"message"` in a push-capable connection's `actions` and reports the
  tool-native `session_id` once it is known.
- The adapter lives while its terminal does; the terminal's exit drops it with the connection.
- For a detached-broker terminal, the broker retains the launch mechanism, loopback port,
  password, directory, launch time, model, and resolved session id beside the live PTY. A
  restarted server restores the adapter when it resumes that terminal. This state travels only
  over the machine-local broker socket; HTTP terminal and connection responses never include
  the password. A broker predating this state reports the session as unsupported after restart.

### OpenCode (`opencode-http`)

Pinned against opencode 1.17.18 by a spike (TUI under a PTY) and a live Hot Sheet run; the
sanitized capture is `crates/hotsheet-aitools/tests/fixtures/opencode-1.17.18-session-push.json`.

- The TUI is a client of its own HTTP server. Launched with `--port <p>` and
  `OPENCODE_SERVER_PASSWORD`, it serves `127.0.0.1:<p>` behind HTTP Basic auth (user
  `opencode`); a wrong password answers `401`. Every request carries `directory=<cwd>`.
- **Session id.** The TUI creates no session until the first prompt. On the first message the
  adapter picks the newest root session in the launch directory created since the launch
  (`GET /session?roots=true`), else creates one titled "Hot Sheet" (`POST /session`) and shows it
  (`POST /tui/select-session`). It then remembers the id; a `404` (session deleted) re-resolves
  once. A user who later switches the TUI to another session keeps receiving pushes in the
  remembered one; the banner is visible either way.
- **Prompt.** Optional `POST /tui/show-toast` (best effort), then
  `POST /session/:id/prompt_async` → `204`. The prompt renders in the attached TUI and a turn
  starts. Without `model`, `prompt_async` runs on the config default model rather than the TUI's
  `--model`, so the adapter passes the launch model (`provider/model`); the OpenCode `[launch]`
  now also applies a chosen model with `--model`.
- **Command.** `POST /tui/execute-command {"command"}` (verified: `agent_cycle` switched Build to
  Plan). Unconfirmed, as above.
- The port is allocated by binding `127.0.0.1:0` and releasing it before launch; if another
  process takes it first the tool fails to bind and the push reports `session_unreachable`
  (the per-session password keeps it from reaching another process's API).

### Codex (`codex-queue`, a reported mechanism)

Pinned against codex 0.162.1 by a live TUI spike and a live Hot Sheet run; the sanitized
capture is `crates/hotsheet-aitools/tests/fixtures/codex-0.162.1-session-queue.json`.

- **Address.** A fresh Hot Sheet-owned TUI gets a one-shot PTY bootstrap adapter. The Codex
  `SessionStart` hook then reports `session_id`, which is the thread id, and
  `codex_home` when `CODEX_HOME` is set (unset means the user's default home).
- **Before the first turn.** `SessionStart` fires lazily, on the first submitted prompt.
  The bootstrap adapter sends that first prompt as bracketed paste followed by Enter to
  the newly launched TUI's own PTY. It accepts exactly one prompt, rejects terminal
  control characters and UI commands, and reports delivery as unconfirmed. The hook's
  thread report replaces it with `codex-queue`. Reattached and shell terminals do not
  create another bootstrap adapter (HS2-8R7SN0).
- **Prompt.** `codex queue --thread=<id> --message=<text>` runs without a shell, under the
  reported home, with a 20 s timeout. It exits 0 with `Queued message <id> for thread <id>.`
  (`confirmed: true`). An idle session starts a turn; a busy one takes it as its next user turn.
  Either way the TUI fires `UserPromptSubmit`. Codex has no banner, so `banner` is ignored.
- **Command.** Not supported (`422 session_push_unsupported`).
- **Reach.** The queue is stored per `CODEX_HOME`, so a TUI started with `--no-daemon` under the
  same home is still reached. A thread the home does not know exits 1 with
  `no rollout found for thread id`, reported as `502 session_unreachable`.

### Claude (`claude-inbox`, a reported mechanism)

Pinned against Claude Code 2.1.302 by a PTY spike and a live Hot Sheet run. In that run a
temp-store `hotsheet-server` launched interactive `claude` in a terminal; its `SessionStart`
hook reported the inbox, and the posted message started a turn that the session answered. The
sanitized capture is `crates/hotsheet-aitools/tests/fixtures/claude-2.1.302-inbox-message.json`.

- **Address.** Claude Code 2.1.224+ binds a per-session Unix socket for
  [cross-session messaging](https://code.claude.com/docs/en/cross-session-messaging). It
  exports `CLAUDE_CODE_MESSAGING_SOCKET` (a plain path such as `/tmp/cc-socks/<pid>.sock`;
  `/status` shows it as `uds:<path>`) and `CLAUDE_CODE_MESSAGING_TOKEN` to hooks.
  - Hot Sheet's `SessionStart` report adds `messaging_socket`, `messaging_token`, and
    `tool_version` (from `AI_AGENT`, else the versioned `CLAUDE_CODE_EXECPATH`).
  - `reported_session` exposes these as the `messaging_socket`, `messaging_token`, and
    `tool_version` fields.
  - The token is a per-session credential. It stays in server memory on the live connection,
    and neither terminal APIs nor the last-hook-report history serve it or the socket path.
  - A permission ask from the same session keeps the inbox its `SessionStart` recorded.
  - No program is resolved; the adapter only writes to the socket.
- **Prompt.** One connection gets an optional auth line, then one `user` line:

  ```text
  {"type":"auth","token":"<token>"}
  {"type":"user","message":{"role":"user","content":"<text>"},"from":"hotsheet","session_id":"<id>"}
  ```

  - `session_id` makes Claude drop a line meant for an earlier session.
  - An idle session starts a turn (with `UserPromptSubmit` firing); a busy one reads the message
    between tool calls.
  - Claude presents it to the model as a message from "another Claude session", not as user
    input, and its permission prompts still apply.
  - The socket writes nothing back, so `confirmed` is always `false`. Hot Sheet cannot tell
    whether the session's inbound controls delivered or held the message.
  - The text limit is 100,000 characters. `banner` is ignored.

- **Version check.** A report from a known Claude older than 2.1.224 registers no adapter. An
  unknown version is accepted, because exporting a socket implies support.
- **Command.** Not supported (`422 session_push_unsupported`).
- **Bypass mode.** A session that bypasses permissions holds every inbound message for an
  approval dialog. A Hot Sheet `-p` stream child launched in `bypassPermissions` therefore also
  gets `--settings {"crossSessionInbound":"accept"}`. This bypass path is doc-sourced, not yet
  live-verified (HS2-FBR3NB). Native Windows (a named pipe) is not supported.
- **Channels.** Claude Code [channels](https://code.claude.com/docs/en/channels) remain out of
  scope. They are a research preview with hidden flags, an allowlist, and an interactive
  dialog, and they are ignored under `-p`.

### Reported mechanisms: the shared registration path

A mechanism whose address is learned after launch is **reported**
(`session_push::is_reported`). Codex is one; the Claude inbox socket is another. Such a
mechanism has no launch wiring, so `prepare_session_push` skips it. Registration then works
like this:

1. **Report.** Each `POST /terminals/{id}/ai-connection` from the tool's hook stores the
   address on the terminal's internal connection.
2. **Build.** `routes::terminals::reported_session` turns it into a
   `session_push::ReportedSession {session_id, fields}`. Each tool's hook-reported fields
   (`codex_home`; the Claude `messaging_socket`, `messaging_token`, and `tool_version`) are
   added in that function.
3. **Register.** `routes::session_messages::register_reported` builds the adapter with
   `session_push::adapter_for_reported(spec, reported, program)` and puts it in the same
   per-terminal map. The program is the plugin's `[launch] program`, resolved on `PATH`; a
   mechanism that runs no program (`claude-inbox`, see `reported_needs_program`) skips that.
   - A newer report replaces it.
   - A launch-wired adapter is never displaced.
4. **Unregister.** `unregister_reported` drops only reported adapters, on `SessionEnd`,
   terminal kill, or shell exit.

The route, error codes, and connection action stay shared. A new reported adapter needs its
mechanism in `is_reported_mechanism` and `adapter_for_reported`, plus a `[session_push]`
section in its manifest.
