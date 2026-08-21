---
id: 01M0H6M3SJ4ZCRGH2A2ZV211W9
slug: HS2-KQ7TF0
title: 'CLI: E2E tests (spawn the hotsheet binary) + expand commands (edit/status/close/claim)'
category: task
priority: default
status: completed
created_at: 2026-08-19T11:16:15.436Z
updated_at: 2026-08-20T02:11:20.088Z
completed_at: 2026-08-20T02:11:20.088Z
closed_at: 2026-08-20T02:11:20.088Z
close_reason: completed
legacy_number: HS2-80
schema: 1
---

hotsheet-cli has unit/integration coverage of import + store, and a manual smoke test, but no automated E2E driving the built binary (double-coverage per CLAUDE.md). Add assert_cmd/trycmd-style E2E over `hotsheet init/new/ls/show/import`. Also expand the command surface toward HS2-8: edit a ticket (title/details/status/priority/tags), set status transitions + close reason, up_next toggle, claim/lease ops, search/query. See docs/04 §4.4. Follow-up of HS2-8.

## Notes

<!-- note: 01M0H6M3T2PR49CVVV1WRJ028E -->
2026-08-20T02:11:20.088Z — **TL;DR:** Added `assert_cmd` **E2E tests** driving the built `hotsheet` binary, plus two new commands — `edit` and `close`.

### E2E (`crates/hotsheet-cli/tests/cli.rs`)
Drives the real binary through `init → new → ls → show → edit → close`, plus error paths: invalid status, `close --reason duplicate` without a target, unknown ticket, commands on a non-store dir, empty-store `ls`. This is the double-coverage the store/import logic already had at the unit level.

### New commands
- **`edit <id>`** — set `title`/`details`/`category`/`priority`/`status`/`--tag`/`--up-next`/`--no-up-next`; a terminal status stamps `completed_at`/`verified_at`; bumps `updated_at`.
- **`close <id> --reason <completed|not_planned|duplicate|obsolete> [--duplicate-of <id>]`** — records the close outcome (`closed_at` + `close_reason`, orthogonal to status per docs/02 §2.6a); requires the target for a duplicate.

### Verified
**41 Rust tests pass** (incl. 6 E2E); fmt + clippy clean. Codebase map updated.

### Follow-up
The rest of the CLI surface — **query/search, claim/lease ops, and ops commands (serve/reindex/doctor/migrate/merge-driver)** — is filed as **HS2-83** (several depend on not-yet-built subsystems: the index HS2-5, server HS2-7, watcher HS2-6, merge-driver HS2-18).
