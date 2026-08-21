---
id: 01M0H6M3SJ0HVVZ2TDX12QV8K5
slug: HS2-MA5XCG
title: 'DECIDE (area 14): Terminal dashboard & grids — v1 or later?'
category: investigation
priority: default
status: completed
created_at: 2026-08-19T01:30:54.196Z
updated_at: 2026-08-19T05:22:10.076Z
completed_at: 2026-08-19T05:22:10.076Z
closed_at: 2026-08-19T05:22:10.076Z
close_reason: completed
legacy_number: HS2-36
schema: 1
---

Recommend: keep-with-changes but likely defer (HS2-17). Global terminal dashboard, drawer grid, magnify/dedicated/jump, visibility groupings — high effort. Decide v1 vs later. See docs/11 area 14. Epic: HS2-22.

## Notes

<!-- note: 01M0H6M3STVDTMSF79TDM0XTF7 -->
2026-08-19T05:20:27.893Z — definitely need to keep the terminal dashboard and grids.  these are critical, heavily used features

<!-- note: 01M0H6M3STVQ5WKWSHTJFKNGYS -->
2026-08-19T05:22:10.076Z — **DECIDED (maintainer, 2026-08-19): keep — critical, heavily-used, v1 (not deferred).** Global terminal dashboard + drawer grid + magnify/dedicated/jump + column slider + visibility groupings. Each tile is a terminal viewport, so they compose cleanly with the multi-viewer PTY-sizing arbiter (docs/06 §6.7) — a magnified/dedicated tile = a focused viewport whose focus drives the shared PTY's size. Build with the terminal manager (HS2-10) + client (HS2-12). docs/11 area 14.
