---
id: 01M0H6M3SJY6CE8P5H2Z2VB86Q
slug: HS2-VBBZ5V
title: Ticket close reasons (completed / not planned / duplicate-of / obsolete)
category: feature
priority: high
status: not_started
created_at: 2026-08-19T02:34:17.723Z
updated_at: 2026-08-19T05:09:45.188Z
legacy_number: HS2-61
schema: 1
---

Collaboration-motivated: when a ticket is closed, optionally record WHY (for tracking/filtering). DECIDED (HS2-24, 2026-08-19): HS1's STATUS SET IS UNCHANGED — no trim, no open/closed collapse, no new 'closed' status. Add frontmatter fields: close_reason (completed|not_planned|duplicate|obsolete, extensible) — a SEPARATE OPTIONAL field orthogonal to status; duplicate_of (ticket ULID, required when duplicate; resolved globally incl. cross-store); closed_at. Setting close_reason annotates a closure (typically alongside status=completed) but does NOT change/replace the status; a ticket may have no close_reason. Freeform explanation still goes in a note; close_reason is the structured/filterable tag. Merge = scalar last-writer-wins (§2.7); reopening clears the fields. UI: optional close-reason picker on close, closed tickets show their reason, duplicates link both ways via duplicate_of. Index: close_reason/duplicate_of/closed_at columns + filters. See docs/02 §2.6a, docs/03 §3.3/§3.5.

## Notes

<!-- note: 01M0H6M3SZSY87XE5PVFJ7WJZH -->
2026-08-19T03:58:09.318Z — **Decided (maintainer, 2026-08-19):** close-reason ↔ status uses an **open/closed axis** with close_reason as the sole descriptor (GitHub model); verified = extra human-checked flag; no separate 'closed' status. See docs/02 §2.6a.

<!-- note: 01M0H6M3SZ2HGMQA280YE0MBQZ -->
2026-08-19T05:09:33.023Z — **CORRECTION (maintainer, 2026-08-19, HS2-24):** supersedes the earlier "open/closed axis" note. Final: **keep HS1's status set unchanged**; `close_reason` is a **separate OPTIONAL field** (completed/not_planned/duplicate+duplicate_of/obsolete), orthogonal to status — it annotates a closure and may be absent. No `closed` status, no open/closed collapse. See docs/02 §2.6a.
