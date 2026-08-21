---
id: 01M0H6M3SJNVQ661GSVQHP2CZC
slug: HS2-ZV972D
title: 'Watcher: git-diff fast path + derived worklist.md regeneration'
category: feature
priority: default
status: not_started
created_at: 2026-08-20T04:58:28.736Z
updated_at: 2026-08-20T04:58:28.736Z
legacy_number: HS2-90
schema: 1
---

The v1 watcher (HS2-6) stat-based: on any file event it hashes the file and reindexes if changed. Add the docs/03 §3.4 git-aware fast path — when HEAD moves (commit/pull/checkout/worktree switch), diff old..new HEAD to get the exact changed paths (O(changes), not O(tickets)). Also regenerate the derived `worklist.md`/`open-tickets.md` (debounced) from the index (docs/03 §3.6) as the file-based AI-tool contract. Follow-up of HS2-6.
