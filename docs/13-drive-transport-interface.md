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
> **First-slice simplification:** `TurnHandle` is synchronous (`is_busy` + `wait` +
> `interrupt`), not yet the async `TurnEvent` stream — that lands with the
> persistent-channel (Claude) drive (HS2-9), which genuinely needs it. The permission
> sink + connection registry are later additions to `DriveCtx`.
>
> **Codex app-server drive built (HS2-110):** `AppServerDrive` — a play is a
> `turn/start` on a new/**resumed** thread against the running `codex app-server daemon`,
> **not a fresh process** (`Target` selects the thread; `turn/interrupt` backs interrupt).
> It's transport logic over an injected `AppServerClient` port (fake-tested); the live
> daemon connection (`codex app-server proxy` speaking `thread/*` + `turn/*` JSON-RPC) is
> a follow-up. Codex's `[drive]` is now `app-server`, not `spawn`.

## 13.0 Current tool capabilities (verified 2026-08-21)

Drivability is the integration priority (a tool that can't be driven against a
persistent/continuous session is lower priority). Ground-truthed from the **installed
CLIs** + official docs, not HS1's notes:

| Tool | Persistent driving? | Mechanism | Transport | Priority |
|---|---|---|---|---|
| **Codex** 0.148 | **Yes (daemon)** | `app-server daemon` (JSON-RPC/control socket; `thread/start\|resume` + `turn/start\|interrupt`; `remote-control`+pairing); `exec-server`; `exec resume <id> "<prompt>"` no-daemon | `AppServer` | 1 |
| **Claude** 2.1.238 | **Yes (channel)** | MCP-channel injection into a running session; `-p --input-format stream-json`; `--resume`/`--continue` | `ClaudeChannel` | 2 |
| **OpenCode** 1.17.18 | **Yes (ACP/HTTP)** | `opencode acp` (ACP server); `opencode serve` + `attach`; session mgmt | `Acp` | 3 |
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

**Busy is thus a derived view, not a per-tool API** — matching HS1's dual
hook+spinner model, generalized. The connection registry's `isBusy` reads the latest
`TurnEvent`.

## 13.5 Optional: a long-lived backing service

Some drives have a **daemon** behind them (Codex app-server; Claude's channel
arguably). `Drive::service()` returns a `BackingService` when present — is-it-on,
health, `prestart` (warm it up), `note_terminal_launch`, must-a-terminal-wait. A tool
without one returns `None`, and no generic caller ever imports a tool's daemon module
(closing the §132.11.2 leak by construction).

## 13.6 Why this isn't Claude-shaped (the acceptance test)

The interface is only real if the tool it was **not** designed around fits. Checks:
- **Claude** needs `run` to be **async** (POST to a running session) → `run` returns a
  `Result<TurnHandle>` and may be async; the spawn drives are sync under the same
  signature.
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
