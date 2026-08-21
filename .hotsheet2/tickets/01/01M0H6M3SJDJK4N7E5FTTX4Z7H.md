---
id: 01M0H6M3SJDJK4N7E5FTTX4Z7H
slug: HS2-6K8065
title: 'DECIDE (area 1): Ticket model & CRUD — port/change?'
category: investigation
priority: default
status: completed
created_at: 2026-08-19T01:30:21.363Z
updated_at: 2026-08-19T05:00:50.925Z
completed_at: 2026-08-19T05:00:50.925Z
closed_at: 2026-08-19T05:00:50.925Z
close_reason: completed
legacy_number: HS2-23
schema: 1
---

Recommend: core-keep (already covered by build tickets HS2-3/HS2-4). Change for HS2: ULID ids + all-caps slug, git-file backing, blocked-by/blocked-reason retained. Confirm no field is dropped. See docs/11-hs1-feature-inventory.md area 1. Epic: HS2-22.

## Notes

<!-- note: 01M0H6M3SQVCAXX18EC9E46M9E -->
2026-08-19T05:00:50.925Z — **DECIDED: core-keep.** Ticket model + CRUD ported onto git files + ULID/all-caps slug; blocked-by, blocked-reason, and close-reason retained. Specified in docs/02 (model + §2.5 format) and docs/03 (index). Build: HS2-3/HS2-4.
