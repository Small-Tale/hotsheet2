---
id: 01M0H6M3SJA4ADNRJ4JP3A1HGE
slug: HS2-GPA959
title: 'hotsheet-cli: direct-to-disk ticket ops + init/serve/reindex/migrate/doctor'
category: feature
priority: default
status: not_started
created_at: 2026-08-19T00:23:14.438Z
updated_at: 2026-08-19T11:16:33.698Z
legacy_number: HS2-8
schema: 1
---

Thin CLI binary wrapping the core for direct-to-disk ticket ops (new/ls/search/show/edit/claim-next) usable with or without a running server, plus ops commands (init, serve, reindex, migrate, doctor, merge-driver). `hotsheet init` git-inits a new local store when the target isn't already in a repo (every store is a git repo; local-only = no remote) and installs the merge driver (HS2-18). Shares the one core so behavior matches the server exactly. See docs/04-core-server-cli.md §4.4, docs/02 §2.8.

## Notes

<!-- note: 01M0H6M3SMQCR69F153E54M6WA -->
2026-08-19T11:16:33.698Z — **Progress (2026-08-19):** the `hotsheet` binary now exists (`crates/hotsheet-cli`) with `init` / `new` / `ls` / `show` / `import` — direct-to-disk read+write through the core writer, best-effort `git init` + initial commit. `-C/--path` selects the store. Verified end-to-end + unit-tested.

Still open for HS2-8: `serve`, `reindex`, `migrate`, `doctor`, `merge-driver` subcommands, and edit/status/close/claim ops + query. Automated E2E (spawn the binary) + command expansion filed as **HS2-80**. Note: `import` doubles as the migrator's write step (docs/07 shape B, HS2-14).
