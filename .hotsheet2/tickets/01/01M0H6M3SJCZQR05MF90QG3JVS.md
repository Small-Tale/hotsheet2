---
id: 01M0H6M3SJCZQR05MF90QG3JVS
slug: HS2-9DXP16
title: hotsheet setup claude (headless) — write Claude plugin artifacts into a project
category: feature
priority: high
status: completed
created_at: 2026-08-20T07:51:22.470Z
updated_at: 2026-08-20T08:15:36.150Z
completed_at: 2026-08-20T08:15:36.150Z
closed_at: 2026-08-20T08:15:36.150Z
close_reason: completed
legacy_number: HS2-98
schema: 1
---

Part of HS2-95; minimal subset of HS2-91. A `hotsheet setup <tool>` CLI command that, with NO server and NO client, prepares a target project directory for an AI tool by writing the plugin's one-shot artifacts.

Scope:
- `hotsheet setup claude [--project <dir>]` and `hotsheet setup --detect` (detect installed tools via the plugin manifest's detection.binaries; set up each).
- Uses the core `plugins` loader (HS2-97) to fetch the Claude plugin, then host helpers write: the managed CLAUDE.md section (merge-safe: insert/refresh a delimited Hot Sheet block, don't clobber user content), the worklist skill into the tool's skills location, and the MCP-config entry registering `hotsheet-mcp --path <store>` in Claude's config format (merge-safe into existing config).
- Idempotent: re-running refreshes managed blocks in place.
- Runs entirely in the CLI (one-shot/host-agnostic bucket, docs/05 §5.1a) — no server required.

Tests: run `setup claude` against a temp project; assert CLAUDE.md has the managed block, the skill file exists, the MCP config registers hotsheet-mcp; re-run is idempotent; existing unrelated CLAUDE.md/config content is preserved.

docs: docs/04 §4.4 already lists the command; note completion + any config-format specifics.

## Notes

<!-- note: 01M0H6M3T6VSR0J5ZKR6EJY5PK -->
2026-08-20T08:15:36.149Z — Done + verified. Added `hotsheet setup <tool>` (CLI) + hotsheet_cli::setup, driven by the HS2-97 plugin loader. Headless — no server, no client. Writes: (1) a merge-safe managed instruction block into the tool's instruction file (CLAUDE.md) between delimiters, preserving user content; (2) the worklist skill to the tool's skills path; (3) a merge-safe .mcp.json entry registering serverless hotsheet-mcp --path <abs-store>, preserving other servers. Idempotent; --detect sets up every tool whose manifest binary is on PATH; --project overrides the write dir (defaults to the store).

Verified end-to-end: hotsheet init + hotsheet setup claude on a project with pre-existing CLAUDE.md → user content kept, block injected, skill written, .mcp.json points hotsheet-mcp at the absolute store path.

Tests (4): all three artifacts written; idempotent + preserves user CLAUDE.md and an unrelated .mcp.json server; unknown-tool + no-selection errors. 83 Rust tests pass; fmt + clippy clean. Committed 4e87a12 (local).

Follow-up HS2-101 (note support CLI+MCP): removed a non-existent edit --note reference from the Claude plugin content; restore once it ships.

Next: HS2-99 (real-tool E2E smoke).
