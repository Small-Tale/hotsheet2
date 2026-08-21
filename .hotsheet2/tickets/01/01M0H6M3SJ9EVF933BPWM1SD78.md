---
id: 01M0H6M3SJ9EVF933BPWM1SD78
slug: HS2-7GWRCP
title: Ticket notes via CLI + MCP (hotsheet edit --note / hotsheet_update note)
category: feature
priority: default
status: completed
created_at: 2026-08-20T08:12:54.756Z
updated_at: 2026-08-20T08:48:51.813Z
completed_at: 2026-08-20T08:48:51.813Z
closed_at: 2026-08-20T08:48:51.813Z
close_reason: completed
legacy_number: HS2-101
schema: 1
---

Surfaced during HS2-98. The ticket model has inline notes (hotsheet_model Note), but neither the CLI `hotsheet edit` nor the MCP `hotsheet_update`/`hotsheet_close` can add one. The Claude plugin instructions originally told the agent to run `hotsheet edit <slug> --note "..."` — a flag that doesn't exist — so I removed note usage from plugins/claude/instructions.md + SKILL.md for now.

Recording a progress note when completing work is valuable for the worklist workflow, so add it:
- ops: an append-note operation (mint a timestamp-ordered note id, kind=regular) — one impl in the engine.
- CLI: `hotsheet edit <slug> --note "..."` (and/or a `hotsheet note <slug> "..."` subcommand).
- MCP: a note argument on hotsheet_update, or a dedicated hotsheet_note tool (mirror the HS1 hotsheet_edit_note surface).
- Once shipped, restore the note guidance in the Claude plugin instructions/skill.

Note kinds/feedback (feedback_needed/feedback_draft) can be a later extension.

## Notes

<!-- note: 01M0H6M3T6F7RTG8275BGQTE4Q -->
2026-08-20T08:48:51.813Z — Done. Ticket notes end to end, one engine op: ops::add_note (caller mints a timestamp-ordered ULID note id, kind=regular, bumps updated_at). Surfaced on all three surfaces without drift:
- CLI: hotsheet edit <id> --note "..." appends alongside the field update.
- MCP: hotsheet_update takes an optional `note` (CoreBackend applies it serverless; server UpdateReq applies it too, then reindexes/broadcasts).
Restored the note guidance in the Claude plugin instructions + SKILL.md (which I'd removed in HS2-98 when the flag didn't exist).
Tests (double coverage): ops add_note (append order + persist), CLI edit --note (## Notes + text), MCP update-with-note assertion in the CoreBackend loop, server PATCH-with-note over HTTP + persisted on GET. 88 Rust tests pass; fmt+clippy clean. Committed d572872 (local).

Deferred: note kinds beyond regular (feedback_needed/feedback_draft), edit/delete-note, attachments — later tickets.
