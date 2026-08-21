---
id: 01M0H6M3SJJPK8YE3EXBBW86NZ
slug: HS2-F9NKJD
title: Core-owned AI-tool setup (move ownership from app layer to core)
category: feature
priority: high
status: not_started
created_at: 2026-08-20T06:37:11.640Z
updated_at: 2026-08-20T06:37:11.640Z
legacy_number: HS2-91
schema: 1
---

Design decided 2026-08-20 (docs/05 §5.1a, docs/04 §4.1). Move authorship of AI-tool integration artifacts OUT of the app/client layer and into the shared core so it runs headless (CLI, no server, no client) and through the server for client-driven flows.

Scope:
- Core `plugins` module owns the one-shot setup writers: managed instruction sections (CLAUDE.md/AGENTS.md/GEMINI.md), skills/rules tree, MCP-config-per-tool, permission-bridge install.
- Capability split by lifetime: one-shot/host-agnostic (setup/instructions/skills/mcp/permissions-install) runs in CLI OR server; persistent/server-only (terminals, drive, busy, connection registry, runtime permission bridge) stays server-hosted.
- CLI surface: `hotsheet setup <tool>` and `hotsheet setup --detect`.
- Client stops implementing setup; it requests `POST …/setup/<tool>` (clients never embed core).
- The set of artifacts written is driven by the active plugin registry (HS2-92).

Acceptance: a project can be fully prepared for a detected AI tool from the CLI with no server and no client running; identical result when the server runs it. Both go through one core impl (docs/04 §4.5).

Depends on / relates to: HS2-92 (loader), HS2-94 (settings supplies the enabled-plugin set). Testable against hs-fake-agent (docs/05 §5.10).
