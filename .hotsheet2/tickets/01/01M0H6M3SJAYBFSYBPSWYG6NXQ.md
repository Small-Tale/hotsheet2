---
id: 01M0H6M3SJAYBFSYBPSWYG6NXQ
slug: HS2-4HRKHE
title: CLI `hotsheet new` parity with MCP create (--up-next, --tags; positional title)
category: bug
priority: default
status: completed
created_at: 2026-08-20T08:04:37.142Z
updated_at: 2026-08-20T08:48:46.691Z
completed_at: 2026-08-20T08:48:46.691Z
closed_at: 2026-08-20T08:48:46.691Z
close_reason: completed
legacy_number: HS2-100
schema: 1
---

Follow-up from HS2-96. The CLI `hotsheet new` and the MCP `hotsheet_create` tool have drifted:
- `hotsheet new` supports: --title, --category (default issue), --priority (default), --details. It does NOT support --up-next or --tags.
- `hotsheet_create` (MCP) supports: title, category, priority, details, tags, up_next.

Also docs/04 §4.4 shows `hotsheet new "Fix dashboard flicker" --up-next` (positional title + --up-next), which doesn't match the impl (--title, no --up-next).

Fix: bring `hotsheet new` to parity — accept a positional title (keep --title as an alias), add --up-next and --tags <comma-list>. Route through the same NewTicket/ops::create the MCP path uses so they can't drift again. Update docs/04 §4.4 examples to match. Add a CLI test.

Matters for HS2-99 (headless E2E) where the agent may create up_next/tagged tickets via the CLI.

## Notes

<!-- note: 01M0H6M3T6RA0XJ8TNBHBD4P8J -->
2026-08-20T08:48:46.691Z — Done. hotsheet new now accepts a positional title (--title kept as an alias), plus --up-next and repeatable --tag, all routed through the same NewTicket/ops::create the MCP hotsheet_create uses (no drift). docs/04 §4.4 example already matched. Tests: positional+up-next+tags round-trips via show/ls; missing-title errors. 88 Rust tests pass; fmt+clippy clean. Committed d572872 (local).
