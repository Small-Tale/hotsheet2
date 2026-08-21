---
id: 01M0H6M3SJCG9WHPNH9W76FYTW
slug: HS2-5BJH42
title: 'DECIDE (area 29): Desktop app (Tauri) — embed core, no sidecar?'
category: investigation
priority: default
status: completed
created_at: 2026-08-19T01:31:32.817Z
updated_at: 2026-08-19T05:01:20.503Z
completed_at: 2026-08-19T05:01:20.503Z
closed_at: 2026-08-19T05:01:20.503Z
close_reason: completed
legacy_number: HS2-51
schema: 1
---

Recommend: core-keep (build HS2-12). Tauri wrapper, auto-update, install-to-PATH, menus. CHANGED per maintainer 2026-08-19: Tauri does NOT embed the core; instead it launches + supervises an INDEPENDENT local server (HS2-59) that survives app close, and is a pure API consumer. Replaces HS1's Node-sidecar-that-dies-with-the-app model. App-icon variants dropped. See docs/11 area 29, docs/06 §6.3, docs/09 §9.1e.

## Notes

<!-- note: 01M0H6M3SXHN4AEVATGW88ZG56 -->
2026-08-19T05:01:20.503Z — **DECIDED: core-keep, changed.** Tauri does NOT embed the core — it launches + supervises an independent local server (survives app close, HS2-59); Solid web UI; auto-update/menus retained; app-icon variants dropped. docs/06 §6.3, docs/09 §9.1e/§9.5.
