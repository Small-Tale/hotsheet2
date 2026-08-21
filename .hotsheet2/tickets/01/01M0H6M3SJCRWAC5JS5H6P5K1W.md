---
id: 01M0H6M3SJCRWAC5JS5H6P5K1W
slug: HS2-2ZPZVH
title: 'PGLite → git migration: standalone bundled per-project migrator (+ UI prompt)'
category: feature
priority: default
status: started
up_next: true
created_at: 2026-08-19T00:23:48.156Z
updated_at: 2026-08-20T03:44:08.746Z
legacy_number: HS2-14
schema: 1
---

Maintainer 2026-08-19: migration is a STANDALONE, BUNDLED, ONE-TIME, PER-PROJECT tool — NOT part of the long-lived Rust core. It may be entirely Node (Node already speaks PGLite; writing markdown+YAML is trivial), and may be one CLI or several. Retire it once everyone's data is moved. Runs against ONE old project's .hotsheet/ at a time (users may not have all projects open) — no batch step. Two acceptable shapes: (A) one Node CLI PGLite->git-store directly; (B) Node export->JSON->importer (Rust `hotsheet import` reusing the core format writer, or Node). REQUIRED either way: a conformance test that the real hotsheet-core cleanly reads + round-trips what the migrator wrote (removes drift risk, frees us to pick A). Write step: fresh ULID + all-caps slug, preserve HS1 number as legacy_number, notes get ULID ids, map completed/verified to close fields, two-pass blocked_by/duplicate_of ULID resolution, git init + install merge driver + initial commit; server reindexes after. Idempotent (skip by legacy_number), source opened READ-ONLY, old db renamed to db.hs1-backup. UI: per-project detect + confirm prompt, or run hotsheet-migrate <old-project> by hand. See docs/07-migration.md, docs/02 §2.5/§2.6a.

## Notes

<!-- note: 01M0H6M3SN7AXW6F7WZMNJE5F4 -->
2026-08-19T11:16:42.539Z — **Progress (2026-08-19): migrator built as docs/07 shape B.** Node exporter `migrator/src/export.mjs` (`exportFromDb` → hotsheet-export.json: fields, JSON tags/notes, RFC3339 timestamps, soft-deleted→deleted, ticket_blocked_by edges best-effort; CLI opens a COPY of the datadir, reads settings.json) + Rust importer `hotsheet import` (two-pass ULID assign + blocked_by remap, preserves legacy_number, fresh note ULIDs, completed/verified→close outcome, idempotent). vitest suite over a synthetic HS1 DB + a **cross-language conformance test** (exporter JSON → real `hotsheet import`, asserts no drift). 6 JS tests + Rust import tests pass.

Deferred follow-ups: real-cluster validation + PGLite version match → **HS2-77** (the installed PGLite can't open HS1's PG17 datadir); attachments → **HS2-78**; the UI-prompted per-project flow (§7.3) is still design-only. docs/07 status updated to "Partially built".

<!-- note: 01M0H6M3SN19T43FNBFXZMSXTC -->
2026-08-20T01:32:04.194Z — **Version coverage (2026-08-20):** per maintainer, the migrator must support the 5 most recent production releases + the current beta. Confirmed from `~/Documents/hotsheet` git tags: **v0.17.2, v0.17.3, v0.18.0, v0.19.0, v0.20.0** (finals) + **v0.21.0-beta.12** (beta). These span **two PG majors** — v0.17.x = PGLite 0.3.x/PG16, v0.18.0+ (incl. beta) = 0.4.x/PG17.

The exporter now bundles one PGLite engine per major and picks by `PG_VERSION` (a PG17 engine can't open a PG16 datadir), reads columns tolerantly (schema drift across releases), and probes postgres+template1. Tested with real on-disk PG16 and PG17 clusters + the real snapshot. See migrator/README.md. Cross-major details tracked in HS2-82; attachments in HS2-78.

<!-- note: 01M0H6M3SNN0S4A4ADN70ZT31T -->
2026-08-20T03:05:47.927Z — **One-command `hotsheet migrate` built (2026-08-20).** `hotsheet -C <store> migrate <old-project>/.hotsheet` now runs the whole flow in one step — spawns the Node exporter (auto-detected / `--migrator` / `$HOTSHEET_MIGRATOR`) against a COPY of the old database, then imports. The two-step (`node export.mjs` + `hotsheet import`) remains available.

**Driving-goal acceptance passed** against a copy of this project's real HS1 snapshot: migrate (84 real tickets) → `ls` (+filters) → `show` → `edit` (status/priority/tags) → `new` → read back; `doctor` confirms every migrated file re-parses cleanly. Tests: Rust E2E (missing-migrator path) + a vitest that runs `hotsheet migrate` end-to-end.

Still open on HS2-14: the **UI-prompted per-project flow** (§7.3) — detect a migratable HS1 project on open + confirm dialog + rename old db to `db.hs1-backup/`. That's client/server work (HS2-7 + clients), not the CLI.

<!-- note: 01M0H6M3SNZCCEP59Q239M7BV3 -->
2026-08-20T03:44:08.746Z — **Migration moved to a separate `hotsheet-migrate` binary (2026-08-20, maintainer review).** Migration is rarely-used, one-time, and needs Node + the bundled exporter, so it no longer sits in the live `hotsheet` CLI. (Note: the Rust `hotsheet` binary never linked pglite/pglite-migrate — those are Node deps in `migrator/`; the CLI only shelled to `node`. The move is about command-surface + runtime coupling hygiene, not a Rust dependency.)

`crates/hotsheet-cli` now builds a small shared lib (`run_import`/`run_migrate`/git helpers — pglite-free) + two bins: `hotsheet` (live ops; keeps the generic `import`) and `hotsheet-migrate` (`hotsheet-migrate <old/.hotsheet> -C <store>`). Verified the full journey on the real snapshot via the split binaries.
