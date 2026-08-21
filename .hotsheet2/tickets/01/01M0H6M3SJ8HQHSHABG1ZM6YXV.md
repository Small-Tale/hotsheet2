---
id: 01M0H6M3SJ8HQHSHABG1ZM6YXV
slug: HS2-P7Y6CW
title: 'DECIDE (area 16): Claude channel & permission overlay — as one plugin?'
category: investigation
priority: default
status: completed
created_at: 2026-08-19T01:30:58.921Z
updated_at: 2026-08-19T05:01:04.493Z
completed_at: 2026-08-19T05:01:04.493Z
closed_at: 2026-08-19T05:01:04.493Z
close_reason: completed
legacy_number: HS2-38
schema: 1
---

Recommend: keep-with-changes (build HS2-9/11). Play/auto mode/backoff, permission overlay w/ edit-diff, allow-rules, commands log, busy/idle — but as ONE plugin transport + the generic host permission bridge (docs/05). See docs/11 area 16. Epic: HS2-22.

## Notes

<!-- note: 01M0H6M3STHSYAQ76Q17RST6MC -->
2026-08-19T05:01:04.493Z — **DECIDED: keep-with-changes.** Claude becomes ONE plugin transport (not first-class); permission overlay = the generic host permission bridge; MCP via a per-project shim; busy via hooks + byte-stream spinner. docs/05 §5.5/§5.7/§5.8. Build: HS2-9/HS2-11.
