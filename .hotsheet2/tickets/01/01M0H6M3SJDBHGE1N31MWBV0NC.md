---
id: 01M0H6M3SJDBHGE1N31MWBV0NC
slug: HS2-ZQ41KY
title: 'CLI: remaining commands — query/search, claim/lease, ops (serve/reindex/doctor/merge-driver)'
category: feature
priority: default
status: completed
created_at: 2026-08-20T02:11:05.892Z
updated_at: 2026-08-20T02:56:15.636Z
completed_at: 2026-08-20T02:56:15.636Z
closed_at: 2026-08-20T02:56:15.636Z
close_reason: completed
legacy_number: HS2-83
schema: 1
---

HS2-80 delivered E2E tests + edit/close. The rest of the CLI surface (toward HS2-8 / docs/04 §4.4) remains: (1) **query/search** — filter/sort/text over tickets (`hotsheet ls` filters, or a `query` subcommand); needs the SQLite/FTS index (HS2-5) or an in-memory scan for v1. (2) **claim/lease ops** — claim-next / release / renew, honoring the distributed claim/lease design (docs/05 §5.7, docs/08). (3) **ops commands** — `serve` (HS2-7 server), `reindex` (HS2-6), `doctor`, `migrate` (wraps the migrator), `merge-driver` (HS2-18). Add E2E coverage for each. Follow-up of HS2-80 / HS2-8.

## Notes

<!-- note: 01M0H6M3T3QYGMX7CF8BDE6V4J -->
2026-08-20T02:56:15.636Z — **TL;DR:** Delivered the CLI surface that doesn't depend on unbuilt subsystems — **query/filter, `doctor`, and local `claim-next`/`release`/`renew`** — with E2E + unit coverage. The rest is deferred to its owning tickets.

### Done
- **Query/search** — `ls` gains `--status`/`--priority`/`--category`/`--tag`/`--text` (case-insensitive, across title+details+notes)/`--up-next`/`--open` filters and `--sort id|created|updated|priority|status|title`. In-memory scan (the SQLite/FTS index is HS2-5).
- **`doctor`** — store health: metadata, parse errors (surfaced by listing), duplicate slugs, dangling `blocked_by`/`duplicate_of`, `duplicate` close reason without a target, invalid timestamps; **non-zero exit** on issues.
- **Local claim/lease** — `claim-next` (picks an open, unblocked, unclaimed-or-lease-expired ticket, preferring up_next > priority > creation order), `release` (holder-only unless `--force`), `renew` (holder-only). This is the *local* primitive.

### Tests
7 new E2E (filters, doctor, full claim→renew→release flow + rejections) + 4 unit tests for the claim helpers (blocked/lease/open/priority). **48 tests pass; fmt + clippy clean.**

### Deferred (with owners)
- **`serve`** → the server, **HS2-7**. **`reindex`** → the index/watcher, **HS2-5/HS2-6**. **`merge-driver`** → **HS2-18**. Each adds its CLI entry point when the subsystem lands.
- **`migrate`** (wrap the Node exporter + `import`) → a convenience for **HS2-14** (packaging/bundling concern — how the migrator ships).
- **Distributed claim/lease** (git-ref CAS for multi-worker safety) → filed as **HS2-84** (builds on the local primitive; docs/08 + the HS2-63 spike).
