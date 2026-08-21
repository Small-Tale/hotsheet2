---
id: 01M0H6M3SJ2ZWXVYB786Z5XJG6
slug: HS2-S1397K
title: Core-owned project settings model (shared / local scopes, CLI-manageable)
category: feature
priority: default
status: completed
created_at: 2026-08-20T06:37:32.789Z
updated_at: 2026-08-20T10:56:00.207Z
completed_at: 2026-08-20T10:56:00.207Z
closed_at: 2026-08-20T10:56:00.207Z
close_reason: completed
legacy_number: HS2-94
schema: 1
---

Design decided 2026-08-20 (docs/04 §4.7). Project settings are core-owned and CLI-manageable, not app-only. The client owns only device-specific settings.

Scope — settings `settings` module in core, split by scope (maps onto the shared/local on-disk model, docs/02 §2.11):
- Shared: committed in the store repo (travels with the project) — auto-context guidance (HS2-25), categories, per-category instructions, custom views, project-level enabled-plugin set. Managed by CLI + server + client (via API).
- Local: gitignored overlay beside the store (machine-local, NOT device-app-local) — tools enabled on this machine, index location, machine paths. Managed by CLI + server; client via API.
- Client/device-only: window geometry, theme, per-viewer PTY size prefs — client app storage, NEVER enters core.
- Dividing test: does a headless CLI or the server ever need this value? yes → shared/local (core); GUI-on-one-device only → client-only.
- CLI: `hotsheet settings get|set|list [--scope shared|local]`.
- The plugins module reads the enabled-plugin set from settings to decide what `hotsheet setup` writes (HS2-91).

Acceptance: shared + local settings are read/written from the CLI with no client; shared settings are committed + diffable; a window-position-style value has no core representation.

Relates to: HS2-91 (setup reads enabled-plugin set), HS2-25 (auto-context guidance lives here).

## Notes

<!-- note: 01M0H6M3T5MBNFE7873YT51S7T -->
2026-08-20T10:56:00.207Z — Done + verified. hotsheet_ticketing::settings::Settings — flat key->JSON map per scope beside the store: shared (hotsheet-settings.json, committed) + local (hotsheet-settings.local.json, auto-added to .gitignore). Effective = local over shared. API: map/effective/get/get_effective/set/unset, Scope{Shared,Local}. CLI: hotsheet-cli settings get|set|list [--scope shared|local]; set defaults to shared, parses value as JSON when possible (arrays/numbers/bools) else stores a string; get/list default to the effective (merged) view.

Verified end to end: set shared categories/theme_hint + local index_path -> hotsheet-settings.json committed, hotsheet-settings.local.json gitignored, effective get/list merges local over shared.

Tests (6): set/get/effective override, shared-committed vs local-gitignored, no gitignore dup, unset, missing-files-empty, + CLI settings E2E. 104 Rust tests pass; fmt+clippy clean. Committed 0580df8. docs/04 §4.7 + CODEBASE-MAP + README updated.

Not yet wired (small follow-ups if wanted): setup reading an enabled-plugin set from settings (HS2-91); auto-context guidance (HS2-25) as a settings-backed feature. Client/device-only settings deliberately excluded from core.
