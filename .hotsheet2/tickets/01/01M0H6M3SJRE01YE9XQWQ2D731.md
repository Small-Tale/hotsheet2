---
id: 01M0H6M3SJRE01YE9XQWQ2D731
slug: HS2-8JXDCN
title: Claude channel drive (ClaudeChannel transport + async TurnEvent stream)
category: feature
priority: default
status: completed
created_at: 2026-08-21T01:58:19.889Z
updated_at: 2026-08-21T02:09:45.516Z
completed_at: 2026-08-21T02:09:45.516Z
closed_at: 2026-08-21T02:09:45.516Z
close_reason: completed
legacy_number: HS2-116
schema: 1
---

Slice of HS2-9 / the drive frontier (docs/13). Build the ClaudeChannel transport drive: inject a turn into a RUNNING, persistent claude process (HS1 play-button model), streaming async TurnEvents — the interface's acceptance test (docs/13 §13.6, "why this isn't Claude-shaped").

Verified claude 2.1.238 protocol (ground-truthed): `claude -p --input-format stream-json --output-format stream-json [--resume <id>]` — long-lived process, NDJSON. Output events: system/init (gives session_id), assistant (message blocks = output), result (subtype success|error_*, is_error, result text = TURN DONE). Input: {"type":"user","message":{"role":"user","content":"..."}}. Same NDJSON framing as codex, so reuse RpcTransport/RpcReader/RpcWriter.

Scope (phase 1):
- Add TurnEvent {Output(String), PermissionAsked(..), Done(DoneReason)} + TurnHandle::next_event() with a default impl (existing sync drives synthesize one Done — additive, non-breaking).
- claude.rs: ClaudeChannel client over an injected RpcTransport (reader thread appends parsed events to a shared log; sequential turns scan cursor->next result). ClaudeStreamTransport (real: spawns claude stream-json). Fully fake-tested via a scripted-claude transport.
- ClaudeChannelDrive impl Drive (transport ClaudeChannel) using an injected ctx.channel; maps stream events -> TurnEvent; wait() drains to Done; session_id from system/init for Target/resume.
- Claude plugin [drive] transport="claude-channel"; host::drive_for handles it; update the claude_is_not_drivable test.
- Gated live test (isolated, empty MCP config, HS2-103 safety). Docs: docs/13 §13.4/§13.6, CODEBASE-MAP.

Out of scope (follow-ups): permission bridge wiring (§5.7, emit PermissionAsked but run a safe permission mode by default); channel interrupt (stream-json control message); partial-message deltas.

Relates to: HS2-9, HS2-112 (codex app-server client pattern), HS2-113 (permission bridge), docs/13.
