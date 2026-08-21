---
id: 01M0H6M3SJJM2X845VH7EMYRD4
slug: HS2-XP5QNA
title: 'DECIDE (area 13): Embedded terminals — port broker + busy inference?'
category: investigation
priority: default
status: completed
created_at: 2026-08-19T01:30:51.056Z
updated_at: 2026-08-19T05:01:02.237Z
completed_at: 2026-08-19T05:01:02.237Z
closed_at: 2026-08-19T05:01:02.237Z
close_reason: completed
legacy_number: HS2-35
schema: 1
---

Recommend: core-keep (build HS2-10). Signature feature: persistent PTYs, multiple/dynamic terminals, themes/fonts, OSC titles+bell, find, quit-confirm, checkout, multi-client, PTY broker survival. See docs/11 area 13. Epic: HS2-22.

## Notes

<!-- note: 01M0H6M3SS6CFCBEQEY0TNPXZW -->
2026-08-19T05:01:02.237Z — **DECIDED: core-keep.** Persistent PTYs + detached broker + multi-viewer attach; sizing = server-arbitrated focus-follows (HS2-62); own crate with process-split deferred. docs/05 §5.4, docs/06 §6.7, docs/12 §12.5. Build: HS2-10.
