---
id: 01M0H6M3SJ0GQHWBMZHF8SQQF1
slug: HS2-Q6ATY0
title: SQLite + FTS5 index and query engine
category: feature
priority: default
status: completed
created_at: 2026-08-19T00:23:00.741Z
updated_at: 2026-08-20T04:58:34.327Z
completed_at: 2026-08-20T04:58:34.327Z
closed_at: 2026-08-20T04:58:34.327Z
close_reason: completed
legacy_number: HS2-5
schema: 1
---

Build the disposable machine-local index: SQLite (WAL) schema (tickets, tags, blocked_by, tickets_fts, index_meta), upsert-from-file, and the query surface (filter/sort/text/paging) consumed by server + CLI. Tickets keyed by (store_id, id) — full identity is store + ULID; store_id is DERIVED from file location (store membership is positional, no frontmatter store field). Carry store_id on every row so the UI filters/groups by store and shows the store-prefixed slug. Handle move tombstones (status=moved, moved_to_store): excluded from lists; a ULID reference resolves to the single live instance following the redirect. Rebuildable from git; schema-version mismatch triggers full rebuild. See docs/03-indexing-and-query.md, docs/02 §2.2.1/§2.13.

## Notes

<!-- note: 01M0H6M3SK1936H6FF4DFYGXC7 -->
2026-08-20T04:58:34.327Z — **Built (2026-08-20): the SQLite + FTS5 index.** New crate `crates/hotsheet-index` (rusqlite bundled + FTS5): `Index` with schema (tickets + provenance file_path/content_hash, a tags table, tickets_fts(title,details,notes), index_meta), `upsert`/`delete`/`rebuild_from_store`/`query(&TicketQuery)→TicketRow[]`/`content_hash` (SHA-256), and schema-version-mismatch → drop+rebuild. Reuses the engine's `TicketQuery`/`SortKey` so the surface matches the CLI/server; text is FTS5 prefix-AND (vs the file-scan's substring — documented divergence).

**Wired into the server:** `GET /tickets` serves from the index (fast + FTS); writes upsert it; the watcher (HS2-6) keeps it fresh. 5 index tests incl. a conformance test that structured filters match `ops::query` (the file scan), + FTS prefix search + schema-rebuild.

Remaining (follow-ups): file-backed at `~/.hotsheet/index/` + `reindex` CLI + no-server maintenance → **HS2-88**; facet tables (blocked_by/assignees/reviews) + expanded filters + keyset paging → **HS2-89**.
