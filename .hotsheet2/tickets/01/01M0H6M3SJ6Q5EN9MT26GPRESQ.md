---
id: 01M0H6M3SJ6Q5EN9MT26GPRESQ
slug: HS2-02PMCW
title: 'DECIDE (area 33): CLI, server & isolation — port on Rust?'
category: investigation
priority: default
status: completed
created_at: 2026-08-19T01:31:41.839Z
updated_at: 2026-08-19T05:01:23.501Z
completed_at: 2026-08-19T05:01:23.501Z
closed_at: 2026-08-19T05:01:23.501Z
close_reason: completed
legacy_number: HS2-55
schema: 1
---

Recommend: core-keep (build HS2-7/8). CLI flags, server hosting, isolated test instance. Rust server/CLI over the core; PGLite-specific cluster-memory items drop. See docs/11 area 33. Epic: HS2-22.

## Notes

<!-- note: 01M0H6M3SXTRV0R4RGSPNHMFNE -->
2026-08-19T05:01:23.501Z — **DECIDED: core-keep.** Rust server + CLI over the core; one server per machine (topology), auto-start + join lifecycle; isolated test instance retained; PGLite cluster-memory items dropped. docs/04 §4.3/§4.4.
