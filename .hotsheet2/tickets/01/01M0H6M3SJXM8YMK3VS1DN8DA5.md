---
id: 01M0H6M3SJXM8YMK3VS1DN8DA5
slug: HS2-538K5V
title: 'DECIDE (area 24): Telemetry / OTLP / cost — v1 and on-by-default?'
category: investigation
priority: default
status: completed
created_at: 2026-08-19T01:31:18.042Z
updated_at: 2026-08-19T06:25:18.121Z
completed_at: 2026-08-19T06:25:18.121Z
closed_at: 2026-08-19T06:25:18.121Z
close_reason: completed
legacy_number: HS2-46
schema: 1
---

Recommend: keep-with-changes, likely defer (HS2-17). OTLP receiver, cost widget, per-ticket attribution, tracing, retention, foreign-OTLP filter. Claude-only today. Decide v1 inclusion + on-by-default. See docs/11 area 24. Epic: HS2-22.

## Notes

<!-- note: 01M0H6M3SW3GHZG8JS2VDAXQHK -->
2026-08-19T06:20:34.078Z — we should define a unified interface for metrics based on what we actually showed in the UI for HS1.  I dont think we need to keep the debugging-oriented options. We should only keep the usage / cost metrics really.  and then all ai plugins should conform to the interface.  we should define this sooner rather than later and then also think about:
- storage (i guess keep using the jsonl mechanism we have now)
- db (maybe just periodically analyze and store rollup data in parallel to the jsonl files) and then dont use a db and just live read from the rollups + extra jsonl (for newer data) as needed?
- have a mechanism for sharing these metrics in teams via git periodically?

<!-- note: 01M0H6M3SWWFPAZ4ZJKQM73YKJ -->
2026-08-19T06:25:18.121Z — **DECIDED (maintainer, 2026-08-19):** keep ONLY usage/cost metrics (drop the debugging tracing/span/waterfall telemetry). Define a UNIFIED metrics interface based on the HS1 UI, that ALL ai plugins conform to (a `metrics` plugin capability). Storage = rotating JSONL + periodic rollup files, NO DB (live-read rollups + newer JSONL). Team sharing via git (periodic rollup sync). Design sooner rather than later → HS2-69. docs/05 §5.3, docs/11 area 24.
