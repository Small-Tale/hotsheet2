---
id: 01M0H6M3SJCQYJHPJSA4E1F8H1
slug: HS2-FQ5EWB
title: 'EPIC: survey every HS1 feature → a build/skip decision ticket per feature area'
category: investigation
priority: high
status: completed
created_at: 2026-08-19T01:25:02.343Z
updated_at: 2026-08-19T01:32:09.489Z
completed_at: 2026-08-19T01:32:09.489Z
closed_at: 2026-08-19T01:32:09.489Z
close_reason: completed
legacy_number: HS2-22
schema: 1
---

Maintainer ask: do a thorough survey of high- and low-level features in ~/Documents/hotsheet and make a decision-making ticket for each — "should we implement this in Hot Sheet 2 and, if so, with specific changes to how it works/operates?" A comprehensive feature inventory is being produced as docs/11-hs1-feature-inventory.md. From it, create one decision ticket per coherent feature AREA (sub-features enumerated inside each), each posing the port/port-with-changes/reconsider/drop question with a recommended default. Granularity is per-area to avoid flooding the worklist with 188 micro-tickets; finer granularity on request. This epic tracks that set.

## Notes

<!-- note: 01M0H6M3SQZDFNHFD1GB6XE4CN -->
2026-08-19T01:32:09.489Z — **Survey complete.** Produced `docs/11-hs1-feature-inventory.md` — a 36-area inventory of every HS1 high/low-level feature with a recommended disposition (core-keep / keep-with-changes / reconsider / likely-drop) and doc refs.

**Per-area decision tickets created: HS2-23 … HS2-58** (area N → HS2-(22+N)). Each poses "build in HS2? with what changes?" with a recommended default, for the maintainer to resolve. They are backlog (won't clutter Up Next).

Granularity is **per-area** (36 tickets) rather than per-micro-feature (~150+) to keep the decision backlog reviewable; sub-features are enumerated inside each ticket + the inventory doc. Ask if finer granularity is wanted on any area.
