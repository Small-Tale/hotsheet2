---
id: 01M0H6M3SJWJ568P6WYWTGNTEQ
slug: HS2-1CPKK9
title: 'Index: facet tables (blocked_by/assignees/reviews) + expanded query + keyset paging'
category: feature
priority: default
status: not_started
created_at: 2026-08-20T04:58:21.441Z
updated_at: 2026-08-20T04:58:21.441Z
legacy_number: HS2-89
schema: 1
---

The v1 index (HS2-5) has tickets + tags + FTS. Add the remaining docs/03 §3.3/§3.5 surface: the blocked_by/assignees/reviews facet tables; query filters for claimed/unclaimed, blocked/unblocked, assignee/review-requested (incl. "me"), close_reason, store_id, and date ranges (the custom-view query-builder dimensions, HS2-29); and keyset pagination on (sort_key, store_id, id) for large stores. Follow-up of HS2-5.
