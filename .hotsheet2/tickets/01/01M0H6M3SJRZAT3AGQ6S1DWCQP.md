---
id: 01M0H6M3SJRZAT3AGQ6S1DWCQP
slug: HS2-CEHFSQ
title: Codex as the second first-party plugin (proves the interface isn't Claude-shaped)
category: feature
priority: high
status: completed
created_at: 2026-08-20T08:50:00.653Z
updated_at: 2026-08-20T09:33:58.821Z
completed_at: 2026-08-20T09:19:30.076Z
closed_at: 2026-08-20T09:19:30.076Z
close_reason: completed
legacy_number: HS2-102
schema: 1
---

Stretch of HS2-95: add Codex as the SECOND first-party plugin, exercising the plugin abstraction against a tool it wasn't designed around (docs/05 §5.1 acceptance test). Codex 0.147.0 differs from Claude in exactly the ways that matter:
- Instruction file: AGENTS.md (markdown — same merge-safe block mechanism).
- NO skills concept → the `skills` capability must be OPTIONAL (absence = not supported, docs/05 §5.3). Make manifest.skills Option; setup skips it when absent.
- MCP config: TOML at $CODEX_HOME/config.toml as [mcp_servers.<name>] { command, args } — different format + location from Claude's .mcp.json. The setup writer must pick the writer by the manifest's `mcp.format` (claude-json | codex-toml) — a host helper keyed on FORMAT, not tool id.

Scope:
- plugins/codex/ (manifest.toml + instructions.md), embedded in hotsheet-plugins alongside claude.
- hotsheet-plugins: skills optional; load codex.
- hotsheet-cli setup: format dispatch (JSON vs TOML, merge-safe), skip skills when absent. Add toml dep.
- `hotsheet setup codex` writes AGENTS.md block + .codex/config.toml; no skill file.
- Tests: codex loads (no skills, format codex-toml); setup codex writes AGENTS.md + TOML mcp_servers, no skill, idempotent.
- Optional live: codex exec run with CODEX_HOME=<proj>/.codex (gated).
- docs: 05 §5.11, CODEBASE-MAP, README.

Done = `hotsheet setup codex` prepares a project for Codex headless, and (opt-in) a real `codex exec` session works a ticket.

## Notes

<!-- note: 01M0H6M3T7XXGGR4F1WBHPNDNF -->
2026-08-20T09:19:30.076Z — Done (deterministic) + verified. Codex is the second first-party plugin, proving the interface isn't Claude-shaped:
- plugins/codex/ — AGENTS.md instructions, NO skills concept, codex-toml MCP config.
- hotsheet-plugins: skills is now Optional (absence = no skill artifact, docs/05 §5.3). Plugin::skill() returns Option.
- setup: the MCP writer is chosen by the manifest FORMAT (claude-json JSON vs codex-toml TOML), merge-safe, keyed on format not tool id.
- E2E harness: new Codex serverless mode; deterministic tier passes (setup codex + drive hotsheet-mcp + assert on disk). Verified: hotsheet-cli setup codex writes AGENTS.md block + .codex/config.toml [mcp_servers.hotsheet], no skill dir.
Tests: plugins (codex loads, no skills, codex-toml), setup (AGENTS.md + TOML + no skill + idempotent + preserves other servers/keys). 91 Rust tests pass; fmt+clippy clean. Committed a727c16.

LIVE codex tier NOT run: a prior live codex/claude run invoked a bare `hotsheet`, which on this machine resolves to /usr/local/bin/hotsheet (HS1 production launcher) and started a prod instance that --replace'd (killed) the maintainer's dev instance, and registered a stray "codex" project tab. Mitigation shipped: the dev binary is renamed hotsheet -> hotsheet-cli, so a stray bare `hotsheet` now fails safe. Live tiers remain gated (HS2_LIVE_CLAUDE / HS2_LIVE_CODEX) and I will not run them without explicit go-ahead. Stray HS1 cluster + scratch dirs cleaned up; maintainer closed the tab.

Follow-up worth filing: make the live E2E tiers guarantee the agent uses hotsheet-cli (install/symlink into a PATH the agent shell honors) or drive via MCP-only, so the live proof can run safely.

<!-- note: 01M0H6M3T76T24Q9QB7RHRB278 -->
2026-08-20T09:33:58.821Z — LIVE codex now verified in BOTH modes (safely, via HS2-103). Ran HS2_LIVE_CODEX=1: a real headless `codex exec` session, in a project prepared only by `hotsheet-cli setup codex` (AGENTS.md + codex-toml MCP), used the hotsheet_* MCP tools to create GREETING.txt and drive its ticket to completed — in serverless (Mode C, --path) AND server-backed (Mode D, a real hotsheet-server via --server/--secret). Both passed the assert_no_hs1 gate; HS1 was never launched and the maintainer's dev instance stayed up. So the full milestone goal is now proven with a real second AI tool, headless, with and without a server.
