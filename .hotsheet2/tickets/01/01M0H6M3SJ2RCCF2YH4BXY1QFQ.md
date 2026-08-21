---
id: 01M0H6M3SJ2RCCF2YH4BXY1QFQ
slug: HS2-228QA1
title: 'AI-tool plugin host: registry + capability traits + first plugin (Claude)'
category: feature
priority: default
status: not_started
created_at: 2026-08-19T00:23:20.303Z
updated_at: 2026-08-19T07:29:47.883Z
legacy_number: HS2-9
schema: 1
---

Implement the plugin interface: declarative half (id/names/tier/maturity/detection-data/transport/preferences) + behavioral half (instructions/skills/command/drive/permissions/mcp), absence-as-feature-test, host-carried machinery (hooks-file writer, managed sections, MCP-config writer, adapter skill-tree), and a lint forbidding tool-id branches outside plugins. Ship Claude as the first plugin + acceptance test. See docs/05-ai-tool-plugins.md.

## Notes

<!-- note: 01M0H6M3SMVFETT0Z15Y6H28SA -->
2026-08-19T03:58:03.319Z — **Decided (maintainer, 2026-08-19):** MCP is delivered via a **per-project MCP shim** spawned into each tool's config (like HS1's channel.ts), not the server exposing MCP directly. Keeps per-project namespacing + the channel model. The plugin's `mcp` capability writes the shim entry. See docs/05 §5.8.

<!-- note: 01M0H6M3SMAC7H9QVAXBQWD1PZ -->
2026-08-19T04:54:57.789Z — **Testability rule (maintainer, 2026-08-19):** every plugin side-effect MUST go through an injected adapter (ProcessSpawner, config-file writer, PermissionTransport, McpConfigWriter, Clock) — no plugin touches a real process/file/global directly. The plugin host + Claude plugin must be exercised by the conformance suite + hs-fake-agent E2E (HS2-64). See docs/05 §5.10, docs/12 §12.7.7.

<!-- note: 01M0H6M3SMY115G90325BGKEJB -->
2026-08-19T07:29:47.883Z — **Crate (maintainer, 2026-08-19):** the AI-tool plugin host lives in **`hotsheet-aitools`** (renamed from `hotsheet-plugins`; deps: ticketing + terminals). One crate per plugin type — see docs/12 §12.2.1.
