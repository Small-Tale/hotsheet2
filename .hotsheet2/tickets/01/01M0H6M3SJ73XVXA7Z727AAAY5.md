---
id: 01M0H6M3SJ73XVXA7Z727AAAY5
slug: HS2-XH8M48
title: Drive trait + ProcessSpawner port + spawn-shape drive (first slice of hotsheet-aitools)
category: feature
priority: high
status: completed
created_at: 2026-08-20T12:00:00.546Z
updated_at: 2026-08-20T12:05:40.378Z
completed_at: 2026-08-20T12:05:40.378Z
closed_at: 2026-08-20T12:05:40.378Z
close_reason: completed
legacy_number: HS2-106
schema: 1
---

First vertical slice of the AI-tool DRIVE capability (docs/13 spec, HS2-67), in the new hotsheet-aitools crate (docs/12 §12.2 "AI-tool plugin host: drive/instructions/skills/command"). Builds the interface + the simplest shape, testable with a fake — no real tool, no server, no fake-agent yet.

Scope:
- ProcessSpawner + SpawnedProcess ports (injected; docs/05 §5.10) + SpawnSpec.
- Drive trait: info()/run(target, content, ctx) required; optional caps by presence. Transport tag (ClaudeChannel|Spawn|AppServer|Acp), Target, DriveCtx (injected spawner), TurnHandle (is_busy/wait/interrupt), DoneReason.
- SpawnDrive (spawn-per-run, Codex `exec` shape) + SpawnDrive::codex(); builds a SpawnSpec from the prompt (content via arg or stdin).
- SystemSpawner (real std::process impl).
- Tests (docs/13 §13.7 conformance, minus the fake-agent parts): run with a fake spawner reports the exact command it would run; TurnHandle is_busy then Done; interrupt kills; info().transport == Spawn.

Deviations from docs/13 to note: v1 TurnHandle is synchronous (is_busy + wait), not the async TurnEvent stream — that lands with the persistent-channel (Claude) drive (HS2-9) which genuinely needs async. Permission sink + connection registry are later additions to DriveCtx.

Relates to: HS2-67 (spec), HS2-66 (Codex drive), HS2-9 (Claude drive), HS2-64 (fake-agent conformance).

## Notes

<!-- note: 01M0H6M3T7T43D0N397BNSYX18 -->
2026-08-20T12:05:40.378Z — Done + verified. New crate hotsheet-aitools (behavioral half of the plugin system). Drive trait (info + run required; supports_interrupt optional) with Transport tag, Target, DriveCtx (injected spawner), TurnHandle (is_busy/wait/interrupt), DoneReason, DriveError. SpawnDrive (spawn-per-run, Codex exec shape) + SpawnDrive::codex(). ProcessSpawner/SpawnedProcess ports + SystemSpawner (real std::process).

Conformance-tested (docs/13 §13.7, minus fake-agent): run reports the exact command via a fake spawner (program=codex, args=[exec, <content>], cwd); TurnHandle busy -> Done; nonzero exit -> Failed(code); interrupt kills -> Interrupted; absent interrupt cap = false; content-via-stdin mode; SystemSpawner runs a real /bin/sh process (exit codes). 113 Rust tests pass; fmt+clippy clean. Committed 4df8d7d.

First-slice simplification (noted in docs/13): TurnHandle is synchronous (is_busy/wait), not the async TurnEvent stream — that + the permission sink + connection registry land with the persistent-channel (Claude) drive (HS2-9). docs/13 status + CODEBASE-MAP + README updated.

Next drive slices: persistent-channel (Claude) drive + async TurnEvent stream (HS2-9), connection registry + busy (docs/05 §5.6), then wire drive into the server + a trigger, then fake-agent conformance (HS2-64).
