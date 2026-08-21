---
id: 01M0H6M3SJBSM8RP1YFSY13SXF
slug: HS2-31APTP
title: 'DECIDE (area 2): Statuses, priorities & categories — keep all 7 statuses?'
category: investigation
priority: default
status: completed
created_at: 2026-08-19T01:30:23.318Z
updated_at: 2026-08-19T05:09:29.123Z
completed_at: 2026-08-19T05:09:29.123Z
closed_at: 2026-08-19T05:09:29.123Z
close_reason: completed
legacy_number: HS2-24
schema: 1
---

Recommend: core-keep. Open: keep all 7 statuses or trim (backlog/archive overlap)? Category presets + custom prefix + auto-cleanup semantics on git storage. DECIDED (maintainer 2026-08-19) for the close-reason coupling: an explicit OPEN/CLOSED axis with close_reason (completed/not_planned/duplicate/obsolete) as the sole close descriptor (GitHub model); verified stays as an extra human-checked flag on a completed close — NOT a separate 'closed' status. Remaining for this ticket: fit that open/closed axis to the concrete status enum (which of HS1's 7 to keep/rename/trim). See docs/02 §2.6a, docs/11 area 2. Epic: HS2-22.

## Notes

<!-- note: 01M0H6M3SQQDXA12HQVCKKD06E -->
2026-08-19T05:06:23.218Z — keep the statuses and have a separate optional field for closed reason, for tracking: completed vs duplicate of ... vs not planned

<!-- note: 01M0H6M3SQGNAW2Z48FW52FTMT -->
2026-08-19T05:09:29.123Z — **DECIDED (maintainer, 2026-08-19):** **Keep the statuses as-is** (HS1's set — no trim, no open/closed collapse, no new `closed` status) and add a **separate OPTIONAL `close_reason` field** for tracking: completed vs duplicate-of vs not-planned (+ obsolete). `close_reason` is orthogonal to status — it annotates a closure, doesn't replace the status; a ticket may have none. This supersedes the earlier "open/closed axis" framing. Docs updated: docs/02 §2.6a, docs/09 §9.11, docs/11 area 2. Build: HS2-61. (Auto-cleanup semantics on git remain an implementation detail for HS2-3/HS2-5.)
