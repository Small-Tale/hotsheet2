---
id: 01M0H6M3SJJ0XTFQ2BE8DGEMF4
slug: HS2-85TEAH
title: 'Migrator: validate the exporter against a real HS1 cluster (PGLite version match)'
category: task
priority: high
status: completed
created_at: 2026-08-19T11:16:02.139Z
updated_at: 2026-08-20T01:09:41.899Z
completed_at: 2026-08-20T01:09:41.899Z
closed_at: 2026-08-20T01:09:41.899Z
close_reason: completed
legacy_number: HS2-77
schema: 1
---

The Node exporter (migrator/src/export.mjs) is verified against a synthetic HS1 DB and the cross-language conformance test passes, but it has NOT been run against a real HS1 datadir. The installed `@electric-sql/pglite` 0.3.16 aborts (`RuntimeError: Aborted()`) opening HS1's PG17 cluster — including the clean `.hotsheet/snapshot.tar.gz` — a PGLite build/version mismatch (datadirs aren't portable across PGLite builds). This project's own `.hotsheet/db` is also held open by the running server.

To close this: (1) determine the exact PGLite version Hot Sheet 1 ships (from HS1's node_modules/package-lock, or ask the maintainer), (2) pin the migrator to a compatible PGLite, (3) run `node src/export.mjs <a CLOSED HS1 project's .hotsheet>` and confirm the exporter reads real `tickets` / `ticket_blocked_by` (verify the real join-table column names — the exporter assumes ticket_id/blocked_by_id and degrades gracefully if absent), (4) round-trip through `hotsheet import` and eyeball the store. See docs/07 §7.2.1. Follow-up of HS2-14.

## Notes

<!-- note: 01M0H6M3T26QGE91K5ECPM21RM -->
2026-08-20T01:09:41.899Z — **Done (2026-08-20): the exporter is validated against a real HS1 cluster.** Root cause of the earlier abort was a cross-MAJOR PGLite mismatch, not a build quirk: I had installed 0.3.16 (PG16); Hot Sheet ships `@electric-sql/pglite ^0.4.5` (0.4.x = PG17), confirmed by reading `~/Documents/hotsheet/package.json`. After matching to 0.4.6 the datadir opens.

Read the real HS1 schema from `~/Documents/hotsheet/src/db/connection.ts` and fixed two exporter bugs: (1) the join column is `blocks_on_ticket_id`, not the assumed `blocked_by_id` (edges would have silently dropped); (2) this cluster predates PGLite 0.4.0, so its tables live in `template1`, not `postgres` — the opener now auto-probes both databases.

Validated end-to-end on this project's own `.hotsheet/snapshot.tar.gz` (consistent, read-only, live DB untouched): **81 tickets** exported (57 with notes) → `hotsheet import` → 81 ticket files with notes / legacy_number / completed→close_reason intact. `ticket_blocked_by` had 0 rows in this corpus, so edge remapping stays covered by the synthetic vitest (which now uses the real column name).

Remaining (separate tickets): attachments → **HS2-78**; cross-MAJOR datadirs (PG16/PG18) via pglite-migrate → **HS2-82** (the exporter already fails such clusters with a clear, actionable message). Validating against a *different, closed* HS1 project is nice-to-have but the mechanism is proven.
