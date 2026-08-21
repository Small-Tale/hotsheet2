---
id: 01M0H6M3SJZGV82Y5KGQYM40SF
slug: HS2-42B6V3
title: 'Real-tool E2E smoke: headless Claude drives Hot Sheet (server up + down)'
category: test
priority: default
status: completed
created_at: 2026-08-20T07:51:30.844Z
updated_at: 2026-08-20T08:35:50.153Z
completed_at: 2026-08-20T08:35:50.153Z
closed_at: 2026-08-20T08:35:50.153Z
close_reason: completed
legacy_number: HS2-99
schema: 1
---

Part of HS2-95. Prove the whole loop with a REAL tool (Claude) running non-interactively in a test project directory, headless (no HS2 client), both WITH and WITHOUT a hotsheet-server.

Scope:
- A `test-projects/` scaffold + a script/harness: create a fresh store (`hotsheet init`), `hotsheet setup claude`, then run Claude non-interactively (e.g. `claude -p "…"`) pointed at a task that requires reading the worklist and creating/updating/closing a ticket.
- Assert the on-disk ticket state changed as expected (via `hotsheet ls/show`), exercising BOTH CLI use and hotsheet_* MCP use by the agent.
- Run the scenario twice: (a) with `hotsheet-server` running (MCP via HttpBackend, index + watcher live), (b) with no server (MCP via CoreBackend serverless). Both must pass.
- Creds/binary-gated + opt-in (real Claude needed), like the live-tool smoke tier in docs/12 §12.7; skips cleanly in CI without creds.

Deferred to HS2-64: the hs-fake-agent conformance suite (deterministic, CI-gated) that replaces the manual real-tool smoke for regression.

Done = a documented, repeatable smoke where a real headless Claude works a ticket through to completed via CLI + MCP, with and without a server.

## Notes

<!-- note: 01M0H6M3T6SETRPBMEE0R9EFVV -->
2026-08-20T08:35:50.153Z — Done + verified live. test-projects/e2e-headless-claude.sh (+ README). For each MCP mode: hotsheet init + setup claude on a fresh project, seed a ticket, drive the real hotsheet-mcp binary over stdio (initialize/query/update/create), assert on-disk via CLI. Deterministic tiers pass for BOTH serverless (--path) and server-backed (real hotsheet-server on an ephemeral port, --server --secret).

LIVE tier RAN (maintainer opt-in, both modes): a real headless `claude -p` session, in a project prepared only by `hotsheet setup claude`, read the Hot Sheet skill, found the ticket, created GREETING.txt containing hello, and drove the ticket not_started → started → completed — in serverless AND server-backed modes. Full milestone goal proven with a real AI tool, headless, with and without a server.

Live tier is gated (HS2_LIVE_CLAUDE=1 + claude on PATH); puts target/debug on PATH, loads .mcp.json, --dangerously-skip-permissions for non-interactive. Committed bd58342 + ff0e826.

Observation feeding HS2-100: the agent noted the seeded ticket wasn't up_next (hotsheet new can't set it yet), so it found the ticket by text/status instead of the Up Next queue. Coped fine; reinforces HS2-100.

hs-fake-agent (HS2-64) will later give the live tier a deterministic CI stand-in.
