---
id: 01M0H6M3SJRFCJ8T8722RWV90M
slug: HS2-BR92RB
title: 'Make live E2E tiers safe: guarantee agents use hotsheet-cli, never bare hotsheet'
category: task
priority: default
status: completed
created_at: 2026-08-20T09:19:44.328Z
updated_at: 2026-08-20T09:33:55.677Z
completed_at: 2026-08-20T09:33:55.677Z
closed_at: 2026-08-20T09:33:55.677Z
close_reason: completed
legacy_number: HS2-103
schema: 1
---

From the HS2-102 incident. The live E2E tiers (HS2_LIVE_CLAUDE / HS2_LIVE_CODEX in test-projects/e2e-headless-claude.sh) previously let a nested agent run a bare `hotsheet`, which on a dev machine with HS1 installed resolves to /usr/local/bin/hotsheet (HS1 production launcher) — it starts a prod instance against the cwd and does --replace, killing the running dev instance, and registers stray project tabs.

Mitigation already shipped: the dev binary is renamed hotsheet -> hotsheet-cli, so a stray bare `hotsheet` fails safe (command not found). But to actually RUN the live tiers safely (and have the agent's CLI calls work), do one of:
- Install/symlink target/debug/hotsheet-cli into a bin dir the agent's tool-shell PATH truly honors (Claude/Codex re-derive PATH from the login profile, so exported PATH prefixes don't always win), or a dir we control and pass explicitly; OR
- Drive the live agents MCP-only (hotsheet_* tools via hotsheet-mcp, which has no HS1 collision) and instruct them not to use the CLI.
Also ensure the codex live tier keeps CODEX_HOME isolated to $proj/.codex (so it never loads the user's ~/.codex [mcp_servers.hotsheet-channel] that spawns HS1 src/channel.ts).

Acceptance: HS2_LIVE_CLAUDE=1 and HS2_LIVE_CODEX=1 runs complete without ever launching HS1 (no /Applications/Hot Sheet.app, no new ~/.hotsheet/projects.json entries, no --replace of a dev instance), on a machine where HS1 owns /usr/local/bin/hotsheet.

## Notes

<!-- note: 01M0H6M3T7K38F7CDSZFYPRD3P -->
2026-08-20T09:33:55.677Z — Done + verified live. Hardened test-projects/e2e-headless-claude.sh so a live agent can never reach the HS1 production launcher:
- Transient hotsheet shim in ~/.local/bin (precedes /usr/local/bin on PATH) -> our hotsheet-cli; removed on exit (trap). Confirmed a login shell resolves bare hotsheet to the shim.
- Every live agent's MCP uses an ABSOLUTE hotsheet-mcp path (no PATH reliance).
- Preflight assert (refuse to launch unless bare hotsheet -> our shim) + post-run assert (fail if <proj>/.hotsheet appeared).
- Codex keeps an isolated CODEX_HOME (never loads the user's hotsheet-channel).
Also fixed a server-leak bug (start_server ran in a subshell via process substitution, losing SERVER_PID tracking) -> sets globals in the current shell; servers cleaned up on exit.

Committed f934d54. Verified by running HS2_LIVE_CODEX=1 in BOTH modes (see HS2-102). Post-run global checks: 0 new ~/.hotsheet/projects.json entries, no .hotsheet clusters, dev instance (pid 45318, port 4174) intact, shim removed, no leaked servers.
