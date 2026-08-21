---
id: 01M0H6M3SJRSNX5DJBQ81JJQ8J
slug: HS2-9HPD84
title: Design a common cross-tool narration/activity interface (for the Announcer)
category: investigation
priority: default
status: completed
created_at: 2026-08-19T06:27:12.169Z
updated_at: 2026-08-19T07:12:11.646Z
completed_at: 2026-08-19T07:12:11.646Z
closed_at: 2026-08-19T07:12:11.646Z
close_reason: completed
legacy_number: HS2-70
schema: 1
---

Maintainer (2026-08-19, HS2-48): the Announcer is a great feature but HS1's is Claude-only (rides Claude's OTLP stream). Design a COMMON cross-tool interface — a `activity` plugin capability emitting tool-agnostic progress/summary events (started ticket X, edited file Y, finished, key decisions) that the Announcer (narration + TTS) and a timeline consume. Decide: the event shape (what/when/ticket-attribution), how each tool maps its signals (Claude channel/hooks, Codex turn transcripts, ACP session/update) into it, live-vs-digest modes, and how it composes with the busy/connection registry (docs/05 §5.6) and metrics (HS2-69). Part of the "generalize concerns across tools via capability interfaces" theme (with drive HS2-67, metrics HS2-69). The Announcer BUILD itself (PIP, live mode, multi-provider TTS, diff visuals) is a later feature (post-floor, HS2-17) that consumes this interface. See docs/05 §5.3, docs/11 area 26.

## Notes

<!-- note: 01M0H6M3T00XHFNG4PTESK9NS1 -->
2026-08-19T07:12:11.646Z — **Spec delivered: docs/15-activity-narration-interface.md.** A tool-agnostic `activity` plugin capability: each tool maps native signals → a common activity event (ULID/ts/tool/project/ticket/kind/summary/detail/importance) with a closed kind vocabulary (turn_start/plan/edit/command/tool_call/decision/blocked/permission/note/ticket_status/turn_end). Per-tool mapping shown for Claude (hooks+transcript), Codex (app-server transcript), ACP (session/update). Live + digest are the same stored data, different reads (bounded recent window; the durable record stays the ticket notes + git history). Composes with the drive TurnEvent (coarse busy/done it enriches) and metrics (turn_end + usage). First consumer = a cheap timeline; the full Announcer is post-floor (HS2-17). Follow-up build ticket: **HS2-76**. Open questions in §15.7.
