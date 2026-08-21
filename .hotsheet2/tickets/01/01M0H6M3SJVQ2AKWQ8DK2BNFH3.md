---
id: 01M0H6M3SJVQ2AKWQ8DK2BNFH3
slug: HS2-TPS9D6
title: 'EPIC: Headless AI-tool loop in a test project (Claude via skills+CLI+MCP, with/without server)'
category: feature
priority: highest
status: completed
created_at: 2026-08-20T07:50:56.956Z
updated_at: 2026-08-20T08:36:17.165Z
completed_at: 2026-08-20T08:36:17.165Z
closed_at: 2026-08-20T08:36:17.165Z
close_reason: completed
legacy_number: HS2-95
schema: 1
---

Major goal (maintainer, 2026-08-20). Get a real AI tool (Claude first, Codex second) running in a test project directory that works with Hot Sheet ENTIRELY HEADLESS (no HS2 client) via skills + CLI + MCP, either WITH or WITHOUT a hotsheet-server running.

Sequencing decided: plugin-first, minimal — build the loop AS the first first-party Claude plugin over a minimal loader; defer external/third-party/WASM/trust (HS2-93) and the fake-agent conformance suite (HS2-64).

Vertical slice (children):
- HS2-96 MCP shim serverless mode (CoreBackend, direct-to-disk) — MCP works with no server.
- HS2-97 Minimal plugin loader + first-party Claude plugin (in-repo: manifest + worklist skill + CLAUDE.md sections + MCP-config template).
- HS2-98 `hotsheet setup claude` (headless) — writes the Claude plugin artifacts into a target project over the loader.
- HS2-99 Real-tool E2E smoke — test project → setup → run Claude non-interactively on a task → assert ticket create/update/close; run twice (server up / server down).

Defaults: Claude first; skill drives off `hotsheet ls --up-next` (no server-regenerated worklist.md needed); proof via real-tool smoke now, fake-agent later.

Done = in a fresh test project, `hotsheet setup claude` prepares it, and a headless Claude session lists/reads/creates/updates/closes tickets via CLI and MCP, both with and without a server running.

## Notes

<!-- note: 01M0H6M3T5V9DYCCMFHMK08BQ4 -->
2026-08-20T08:36:17.165Z — EPIC DONE + verified live. A real headless Claude, in a project prepared only by `hotsheet setup claude`, works Hot Sheet tickets via skills + CLI + MCP — with AND without a hotsheet-server. Proven end-to-end by test-projects/e2e-headless-claude.sh (deterministic tiers + live claude -p, both modes).

Children (all completed, plugin-first minimal path):
- HS2-96 serverless MCP CoreBackend (+ wire-DTO SSOT).
- HS2-97 minimal plugin loader + first-party Claude plugin in-repo (plugins/claude/).
- HS2-98 hotsheet setup <tool> — merge-safe, headless.
- HS2-99 E2E harness; live real-Claude run passed serverless + server-backed.

Deferred as planned: HS2-92/HS2-93 (external plugin search-path / third-party / subprocess+WASM / trust gate), HS2-64 (hs-fake-agent conformance for a deterministic CI stand-in). Follow-ups filed: HS2-100 (CLI `new` parity: --up-next/--tags/positional title), HS2-101 (ticket notes via CLI+MCP).

State: 83 Rust tests pass, fmt + clippy clean, tree clean, all commits LOCAL (not pushed). Next natural step toward the epic's stretch: Codex as the second tool (proves the plugin interface isn't Claude-shaped) — not yet ticketed.
