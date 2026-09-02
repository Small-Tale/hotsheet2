# 15. Cross-Tool Activity / Narration Interface

> **Status: Rich native stream built (HS2-KP31ZE/HS2-4C68Y8/HS2-SW655F).** A **tool-agnostic activity event stream** — the common
> interface the **Announcer** (narration + TTS) and a **timeline** consume, so
> narration isn't Claude-only like HS1's (which rode Claude's OTLP stream). Part of
> the "generalize a concern across every tool via a capability" theme (with `drive`
> [13](13-drive-transport-interface.md) and `metrics`
> [14](14-metrics-interface.md)). This is the event spec + mapping + build plan. The
> Announcer *build* is a later feature (post-floor); this is the interface it needs.

## 15.1 The idea

Every AI tool "does things" while working — starts a ticket, edits a file, runs a
command, makes a decision, finishes. HS1 got these as narratable events only from
Claude. HS2 defines **one `activity` plugin capability** ([05](05-ai-tool-plugins.md)
§5.3): each tool maps its native signals into a common **activity event**, and any
consumer (Announcer, a live timeline, a "what happened" digest) reads that one stream.

## 15.2 The activity event

```jsonc
{
  "id": "01J9ZK…",              // ULID (ordering + de-dup)
  "ts": "2026-08-19T14:03:11Z",
  "tool": "codex",
  "project": "01J9Z…",
  "ticket": "01J9ZK…",          // ULID when attributable, else null
  "kind": "edit",               // see the small vocabulary below
  "summary": "Edited resolveCommand.ts — added the {{aiCommand}} token",
  "detail": { "path": "src/…", "…": "…" },   // kind-specific, optional
  "importance": "normal"        // low | normal | high  (narration emphasis)
}
```

A **small, closed `kind` vocabulary** (not free-form, so consumers can style/emphasize
consistently):
`turn_start` · `plan` · `edit` · `command` · `tool_call` · `decision` · `blocked` ·
`permission` · `note` · `ticket_status` · `turn_end`.

- **`summary`** is the one-line, human/narration-ready string (the tool or the host
  composes it). `detail` carries structured extras a timeline can expand.
- **`importance`** lets the Announcer pick what to speak (high always; normal in the
  full timeline; low is trace-ish) — the narration "emphasis" HS1 had, generalized.
- **`ticket` attribution** reuses the active-ticket tag (same mechanism as metrics,
  [14](14-metrics-interface.md) §14.2).

## 15.3 How each tool maps into it (the per-tool part)

The `activity` capability turns native signals → events; the host owns the stream:

- **Claude** — channel/hook events + the turn transcript: `PreToolUse` → `tool_call`/
  `command`/`edit`, `UserPromptSubmit` → `turn_start`, `Stop` → `turn_end`,
  `FEEDBACK NEEDED` → `note`/`blocked`.
- **Codex** — the app-server **turn transcript** (docs/121 already emits
  start/item/end) maps directly: items → `edit`/`command`/`decision`, end → `turn_end`.
- **ACP** (OpenCode/Goose) — `session/update` payloads carry tool-use + text → the
  same kinds; `stopReason` → `turn_end`.
- A tool that exposes **nothing** narratable omits the capability (absence = the
  signal). It can still show coarse **busy/idle** from the drive's `TurnEvent`
  ([13](13-drive-transport-interface.md) §13.4) — see §15.5.

## 15.4 Live vs. digest (two consumer modes over one stream)

- **Live** — the Announcer/timeline subscribes to the event stream as it arrives
  (mid-task narration, the live PIP). Rides the same WebSocket bus as everything else.
- **Digest** — an after-the-fact summary of a window (a completed ticket, "since I
  last looked") is **computed from the stored events**, not a second pipeline. So live
  and digest are the same data, different read.
- **Storage:** activity events are **derived/ephemeral-ish** — persist a bounded
  recent window (the event day plus the preceding 13 calendar days, like metrics raw JSONL
  [14](14-metrics-interface.md) §14.3) so a digest can look back; recording a new day
  automatically ages out older files. The *durable* record of "what
  happened" is the ticket's **notes + git history**, not this stream.

## 15.5 How it composes with the other seams

- **Drive ([13](13-drive-transport-interface.md)):** `TurnEvent` (Busy/Idle/Done) is
  the *coarse* signal; `activity` is the *rich* one. A tool with a drive but no
  activity capability still narrates at the coarse level ("Codex is working / finished
  ticket X"). Activity enriches when available.
- **Metrics ([14](14-metrics-interface.md)):** a `turn_end` activity pairs naturally
  with a usage event; the Announcer can say "finished — 24k tokens, $0.04."
- **Connection registry ([05](05-ai-tool-plugins.md) §5.6):** events are tagged by
  connection, so a multi-connection / worker setup narrates per source.

## 15.6 Consumers (built later)
- **Announcer** — PIP, live mode, multi-provider TTS, code-diff visuals (post-floor,
  HS2-17) — subscribes live + requests digests.
- **Timeline** — a per-ticket / per-session "what happened" view, reads the stored
  window. (A natural, cheaper first consumer than full TTS.)

## 15.7 Durable milestone distillation

> **Built (HS2-3GRNZW):** `ticketing::activity_distillation` selects meaningful,
> bounded windows from the normalized stream and may turn them into shared `activity`
> notes. It is disabled by default and consent comes exclusively from the machine-local
> `activity_distillation` settings object; global/shared settings cannot enable it.

Candidate selection is deterministic: meaningful decisions, explicitly changed plans,
blocked/unblocked transitions, three edit/command events by default, a substantive turn
ending, or a ticket-status change after substantive work. Start/end chatter, duplicate
events, lone status events already represented by the status timeline, and ordinary
low-signal plans do not create candidates. Windows are capped (64 events by default),
and a bounded 2,048-id cache prevents completed-window replay from seeding another note.

The `LocalActivitySummarizer` boundary receives only `DistillationRequest`: event ids,
closed kinds, sanitized tool ids, and allow-listed ticket statuses. Raw event summaries,
prompts, command strings/output, paths, and file contents are absent from the type. A
local adapter can return a concise note, suppress a candidate, or fail; failure may use
the deterministic count-based fallback only when `deterministic_fallback: true` is
explicitly set (it defaults false). Returned notes are
single-line-normalized and bounded to 500 characters.

Each candidate hashes version + ticket + session + every ordered event id + count. That
provenance produces both an embedded marker and the caller-generated note ULID passed
through the provider-neutral `TicketProvider` boundary. Retries find the marker/id;
concurrent git clients converge through the existing note-by-ULID semantic merge.
Provider adapters whose remote service chooses note ids must persist/recognize the
caller-generated id as their idempotency key.

The built-in `deterministic` adapter runs locally beside the activity sink. An Apple
client selects `apple_foundation_models`, consumes the same live/digest stream, maps the
serializable `DistillationRequest` to its on-device Foundation Models session, and sends
the returned text through `distill`/`write_distilled_note`. Apple frameworks are thus a
client adapter, never a server dependency; other clients can inject another
`LocalActivitySummarizer` or leave the feature disabled.

### 15.7.1 Remaining stream considerations

- **Importance heuristic** — default mapping from `kind` → `importance`, overridable.
- **Volume/rate** — cap events/sec per turn so a chatty tool can't flood the stream.

## 15.8 Build plan (follow-ups)
- HS2-70 (this) = the spec.
- **Shipped (HS2-KP31ZE) — the interface + storage + first consumer:**
  - The **event model** — `ticketing::activity::{ActivityEvent, ActivityKind, Importance}`
    with the closed kind vocabulary, a `kind → default_importance` heuristic, and a host
    `default_summary` composer (a tool may override, §15.7).
  - The **bounded rolling store** — `record` / `read_recent` / `prune_before` over
    `activity/recent/<YYYY-MM-DD>.jsonl` (per-device, gitignored — like metrics raw,
    §15.4); recording automatically removes files outside the 14-day calendar window.
  - The **timeline** consumer — `activity::timeline(store, TimelineFilter)` (by
    ticket/session/min-importance, most-recent-capped), exposed as `GET /activity`; plus
    `POST /activity` to ingest an event (server stamps id/ts, defaults summary/importance).
  - The **`activity` plugin capability** — `Manifest.activity = ActivitySpec{source}`;
    codex declares `source="codex-transcript"`, claude `source="claude-hooks"`.
  - The **mappers** — `activity::claude_activity` (from the PreToolUse/UserPromptSubmit/Stop
    hook JSON — the real shape the permission hook already receives) and `codex_activity`
    (from transcript items, lenient — the exact codex item vocabulary wants live
    confirmation, like the usage mappers). Both sample-tested.
- **Shipped (HS2-4C68Y8) — core live stream:** every recorded event is published as
  `ChangeEvent { kind: "activity", activity: <full event> }` over `/ws/sync` and the
  long-poll fallback. The server-hosted drive emits coarse `turn_start`, `permission`,
  and `turn_end` events attributed to the active store, ticket, tool, and session; the
  same event is persisted before broadcast, so live and digest consumers cannot drift.
- **Shipped (HS2-SW655F) — rich native drive events:** Codex 0.152.1
  `item/completed` notifications stream their completed `commandExecution`, `fileChange`,
  `plan`/`reasoning`, MCP/dynamic/collaboration tool, web-search, and image-view items.
  Claude Code 2.1.258 runs with hook lifecycle events enabled and projects authoritative
  assistant `tool_use` blocks into the same `PreToolUse` payload contract consumed by
  `claude_activity`. Both become tool-neutral `NativeActivity` turn events, then the
  server maps, attributes, persists, and broadcasts them through the existing activity
  sink. Sanitized version-pinned cassettes replay in ordinary CI; credentialed drift
  checks remain explicitly ignored and environment-gated. Native payload capture does
  not create durable ticket notes or summarize content (HS2-3GRNZW owns that policy).
- **Shipped (HS2-3GRNZW) — opt-in durable distillation:** bounded deterministic
  milestone windows, privacy-safe serializable summarizer requests, a pluggable local
  summarizer boundary, deterministic fallback, and provenance-derived idempotent
  activity-note writes. The server supplies only the optional deterministic adapter;
  Apple Foundation Models remains an on-device client adapter.
- **Deferred:** the full Announcer UI/TTS remains the post-floor HS2-17 consumer.

## 15.9 Cross-references
- The `activity` plugin capability: [05-ai-tool-plugins.md](05-ai-tool-plugins.md) §5.3
- Coarse busy/done it enriches: [13-drive-transport-interface.md](13-drive-transport-interface.md) §13.4
- Pairs with usage events: [14-metrics-interface.md](14-metrics-interface.md)
- Announcer decision: docs/11 area 26 (HS2-48)
