---
id: 01M0H6M3SJTPF91TJ61ZB39MD5
slug: HS2-J7VDBZ
title: Design the AI-tool drive/transport interface early (capability-based, all tools)
category: investigation
priority: high
status: completed
created_at: 2026-08-19T06:20:16.909Z
updated_at: 2026-08-19T07:11:09.078Z
completed_at: 2026-08-19T07:11:09.078Z
closed_at: 2026-08-19T07:11:09.078Z
close_reason: completed
legacy_number: HS2-67
schema: 1
---

Maintainer (2026-08-19): investigate the drive/transport interface design SOONER RATHER THAN LATER — it's the seam every AI tool and the whole test harness (HS2-64) hang off. Goal: ONE interface with OPTIONAL capabilities that each tool conforms to as applicable (absence = not supported); no single universal transport. Nail down: the required core (run(target, content) — sync spawn OR async POST-to-running-session) + optional sub-capabilities (interrupt, reset, prestart, isBusy, long-lived backing service) + how transport identity, the target selector, permission-request surfacing, busy signaling, and done-detection are expressed uniformly across the shapes (persistent channel = Claude, spawn/app-server = Codex, ACP = OpenCode/Goose). Validate the abstraction against at least those three shapes so it isn't Claude-shaped (the docs/132 lesson). Output: the trait/interface spec + which optional caps exist + a conformance checklist the fake-agent suite verifies. Feeds HS2-9 (plugin host) and HS2-66 (Codex). See docs/05 §5.5.

## Notes

<!-- note: 01M0H6M3T05PS6BQB4TZWT8A0J -->
2026-08-19T07:11:09.078Z — **Spec delivered: docs/13-drive-transport-interface.md.** One `Drive` = required `run(target, content) -> TurnHandle` (sync spawn OR async post-to-running-session) + optional sub-capabilities probed by presence, not bool flags (interrupt / reset / backing-service). The four cross-cutting concerns expressed once: transport = a declarative data tag (identity, no client mirror); target = pick a connection from the registry; permissions = a per-transport adapter into the host bridge; busy/done = a unified `TurnEvent` stream on the TurnHandle (each transport derives it from its native signal — hooks+spinner / process-exit / stopReason). Validated non-Claude-shaped against Claude (async), Codex (interrupt + daemon), ACP (permission-as-response). Conformance checklist for the fake-agent suite included (§13.7). No new build ticket — implementation is HS2-9 (host + Claude drive) + HS2-66 (Codex) + HS2-64 (conformance). Open questions in §13.8.
