---
id: 01M0H6M3SJ2CMTKAAGA6ED3ECK
slug: HS2-SNVTVV
title: 'Index: file-backed at ~/.hotsheet/index/ + `hotsheet reindex` + CLI/no-server index maintenance'
category: feature
priority: default
status: completed
created_at: 2026-08-20T04:58:18.539Z
updated_at: 2026-08-20T06:03:33.786Z
completed_at: 2026-08-20T06:03:33.786Z
closed_at: 2026-08-20T06:03:33.786Z
close_reason: completed
legacy_number: HS2-88
schema: 1
---

The v1 index (HS2-5) is in-memory, rebuilt on each server start. Make it durable + usable without a running server (docs/03 §3.2/§3.8): (1) file-backed at `~/.hotsheet/index/<project-id>.sqlite` (WAL, machine-local, gitignored) keyed per project; (2) `hotsheet reindex` CLI (drop + rebuild); (3) the CLI upserts the index on write when no server runs (docs/04 §4.4 "then upsert the local index"), with a one-writer-per-machine lock (§3.8). Follow-up of HS2-5.

## Notes

<!-- note: 01M0H6M3T4PK31GJYKRC5MNXBH -->
2026-08-20T05:27:18.727Z — **Part 1 done (2026-08-20): file-backed index + restore/reconcile on launch** — the piece flagged as wanted "soonish".

`Index::open_reconciled(db_path, store)`: opens the file-backed SQLite index (WAL), **keeps whatever rows are still valid**, and reconciles only the delta — `Index::reconcile()` re-parses + upserts files whose `content_hash` changed and deletes rows whose file is gone. A corrupt file is deleted + rebuilt (corruption is a non-event, docs/03 §3.8). The server (`main`) now opens the index at `~/.hotsheet/index/<hash-of-store-path>.sqlite` (or `--index <file>`), reconciles on start, then runs the watcher; `AppState::new` keeps the in-memory path for tests.

**Verified across a real restart:** a ticket created via HTTP is restored from disk on reboot, and a ticket created offline via the CLI while the server was down is reconciled in — both present, no full rebuild. Tests: reconcile picks up offline edits / deletes missing rows / corrupt-file rebuilds (8 index tests).

**Still open on HS2-88:** (2) the **`hotsheet reindex` CLI** (drop + rebuild on demand); (3) the CLI **upserting the index on write when no server runs** (docs/04 §4.4) with a one-writer-per-machine lock (§3.8) — this pulls rusqlite into the CLI binary, so worth a deliberate call. Facet tables/paging are HS2-89; git-diff fast path is HS2-90.

<!-- note: 01M0H6M3T4DTAXX7QA3066A6GS -->
2026-08-20T06:03:33.786Z — **Completed (2026-08-20).** Part 1 — file-backed index + restore/reconcile on launch — is built + verified (see prior note). Parts 2 & 3 were **deliberately dropped** (maintainer, 2026-08-20):

The index is purely the **server's** read cache (fast list-draw + FTS for connected clients). The CLI is direct-to-disk and reads via a file scan (`ops::query`), so:
- **Part 3 (CLI maintains the index on write): dropped.** With no server running there's no reader, so keeping the index warm from the CLI is pure duplicated work + a cross-writer lock, and would pull `rusqlite` into the `hotsheet` binary for no benefit. The server's `open_reconciled` already absorbs offline CLI/git edits on its next start.
- **Part 2 (`hotsheet reindex`): dropped as unnecessary.** The index rebuilds on schema change, reconciles every start, and self-heals if corrupt. A manual force-rebuild is just `rm ~/.hotsheet/index/<hash>.sqlite` (the server recreates it) — no CLI SQLite dependency.

docs/04 §4.4 updated to state the CLI never touches the index. Remaining index work is elsewhere: facet tables/paging (HS2-89), git-diff fast path (HS2-90).
