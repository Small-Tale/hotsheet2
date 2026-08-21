---
id: 01M0H6M3SJQ83VDTM88JE7AHY6
slug: HS2-WEGYFG
title: 'DECIDE (area 26): Announcer (AI narration + TTS) — part of the product?'
category: investigation
priority: default
status: completed
created_at: 2026-08-19T01:31:25.318Z
updated_at: 2026-08-19T06:27:13.925Z
completed_at: 2026-08-19T06:27:13.925Z
closed_at: 2026-08-19T06:27:13.925Z
close_reason: completed
legacy_number: HS2-48
schema: 1
---

Recommend: reconsider, defer (HS2-17). Digest PIP, live mode, mid-task narration, cross-project reel, multi-provider TTS, diff visuals. Beta, key-consuming. Decide if it's part of product identity or an optional add-on. See docs/11 area 26. Epic: HS2-22.

## Notes

<!-- note: 01M0H6M3SWRW8E1S5HTMR56G2J -->
2026-08-19T06:24:45.450Z — announcer is a great feature but it could definitely be improved and better-generalized to work across ai tools.  think about a common interface to support this sort of thing across ai tools.

<!-- note: 01M0H6M3SWKWJ4X054DCQQR3DN -->
2026-08-19T06:27:13.925Z — **DECIDED (maintainer, 2026-08-19): keep the Announcer, but generalize it across AI tools.** HS1's is Claude-only. Add a common cross-tool `activity` capability (tool-agnostic progress/summary events) the Announcer + timeline consume (docs/05 §5.3) — design filed as HS2-70. The Announcer build (PIP/live/TTS/diff visuals) is post-floor (HS2-17), consuming that interface. docs/11 area 26.
