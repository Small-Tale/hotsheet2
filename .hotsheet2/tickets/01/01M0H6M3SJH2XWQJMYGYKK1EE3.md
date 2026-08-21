---
id: 01M0H6M3SJH2XWQJMYGYKK1EE3
slug: HS2-CHS3MD
title: Filesystem watcher + incremental (git-diff-aware) reindex
category: feature
priority: default
status: completed
created_at: 2026-08-19T00:23:05.100Z
updated_at: 2026-08-20T04:58:46.989Z
completed_at: 2026-08-20T04:58:46.989Z
closed_at: 2026-08-20T04:58:46.989Z
close_reason: completed
legacy_number: HS2-6
schema: 1
---

Watch store paths; debounce; detect real changes via content hash / git blob OID; re-parse only changed ticket files and upsert/delete index rows; git-aware fast path diffing old..new HEAD on commit/pull/checkout; emit change events on the WebSocket bus. See docs/03-indexing-and-query.md §3.4.

## Notes

<!-- note: 01M0H6M3SM5Y1T2R0Z9D9QFQDQ -->
2026-08-20T04:58:46.989Z — **Built (2026-08-20): the filesystem watcher.** `hotsheet_server::spawn_watcher` — a `notify` watcher over `<store>/tickets/`, debounced (150ms). On a change it hashes the file and compares to the index's `content_hash`, reindexing (upsert) + emitting a WS ChangeEvent **only on a real change** — so the server's own writes (already indexed with that hash) don't double-emit (docs/03 §3.4). Deletions (file gone; ULID from the filename) drop the index row + emit. Runs on a background std thread; a `WatchHandle` keeps it alive for the server's run.

This closes the "a CLI/git edit doesn't show up live" gap: an external edit reindexes + broadcasts. **Verified end-to-end** over a socket (external CLI create + status edit both reindex ~300ms) + an automated server test (external `ops::create` → reindex → visible via HTTP).

Remaining (follow-up **HS2-90**): the git-diff fast path (old..new HEAD on commit/pull/checkout — O(changes) not O(tickets)) and derived `worklist.md`/`open-tickets.md` regeneration (docs/03 §3.6). The current watcher is stat/hash-based, which is correct but re-hashes touched files.
