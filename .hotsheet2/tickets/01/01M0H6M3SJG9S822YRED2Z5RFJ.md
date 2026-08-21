---
id: 01M0H6M3SJG9S822YRED2Z5RFJ
slug: HS2-DDH9S4
title: 'AI-tool testing harness: hs-fake-agent + plugin conformance suite (hard CI gate)'
category: feature
priority: high
status: not_started
created_at: 2026-08-19T04:54:56.081Z
updated_at: 2026-08-19T07:29:46.349Z
legacy_number: HS2-64
schema: 1
---

Design out the HS1 "adding Codex was a manual slog" pain (maintainer 2026-08-19). Deliverables: (1) enforce the testability rule in hotsheet-plugins — every plugin side-effect goes through an injected adapter (ProcessSpawner, config-file writer, PermissionTransport, McpConfigWriter, Clock); no plugin touches a real process/file/global directly. (2) hs-fake-agent — a scriptable workspace test binary speaking the same protocols a real tool does: MCP tool calls, permission requests through a plugin transport, PTY bytes incl. OSC 7/8/9/133 + spinner glyphs + chosen exit code, busy/idle signals. (3) Per-aspect automated E2E against it: MCP call flows + config-write validity; permission FIFO-enqueue/WS-push/route-back/allow-rules + merge-safe install/remove; terminal spawn/stream/OSC/sizing/broker-survival; busy-state transition-matrix (busy->sustained->idle, stale-clear, spinner-liveness gate, dropped-Stop recovery). (4) A conformance suite parameterized over the whole plugin registry (identity/instructions/skills/command/drive/permissions/mcp), run vs a temp fixture project, as a HARD CI GATE — a new tool inherits it by existing and can't merge until it passes conformance + fake-agent E2E. (5) Drift layer: recorded protocol contracts (cassette replay in fast CI) + opt-in creds-gated live smoke per tool (nightly/pre-release). See docs/12 §12.7.7, docs/05 §5.10.

## Notes

<!-- note: 01M0H6M3SZD7D9HTZ70XC6NFYQ -->
2026-08-19T05:40:09.027Z — **Acceptance criterion (maintainer, 2026-08-19, HS2-40):** the design succeeds if the maintainer can, *relatively unsupervised*, ask to add support for another tool (OpenCode / Cursor / Antigravity / …) and have it work **fully without constant manual testing** — the new plugin inherits the fake-agent E2E + conformance gate + recorded-contract drift checks by existing. Treat this as the north-star test for HS2-64.

<!-- note: 01M0H6M3SZA45RKRDJFN35AC11 -->
2026-08-19T07:29:46.349Z — **Crate rename (maintainer, 2026-08-19):** the AI-tool plugin host crate is **`hotsheet-aitools`** (was `hotsheet-plugins`). The testability rule + fake-agent + conformance suite live there. Each plugin TYPE is its own crate (`hotsheet-<type>`) with its own conformance suite — external-sync is `hotsheet-extsync` (HS2-73). See docs/12 §12.2.1.
