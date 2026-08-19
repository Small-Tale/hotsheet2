# 13. AI-Tool Drive / Transport Interface

> **Status: Design (HS2-67).** The seam **every AI tool and the whole test harness
> (HS2-64) hang off**, so it's designed early. **One interface with optional
> capabilities each tool conforms to as applicable** — no single universal transport
> (maintainer, 2026-08-19). Validated against three genuinely different shapes so it
> isn't Claude-shaped (the docs/132 lesson). This is the trait spec + a conformance
> checklist.

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
