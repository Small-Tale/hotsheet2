---
id: 01M0H6M3SJTJT6EE41MVZXVKQ5
slug: HS2-DEPKP0
title: Behavioral plugin boundaries (subprocess + WASM) + trust gate / verify
category: feature
priority: default
status: completed
created_at: 2026-08-20T06:37:25.919Z
updated_at: 2026-08-20T11:36:16.264Z
completed_at: 2026-08-20T11:36:16.264Z
closed_at: 2026-08-20T11:36:16.264Z
close_reason: completed
legacy_number: HS2-93
schema: 1
---

Design decided 2026-08-20 (docs/05 §5.11). For plugins that carry code (not manifest-only), the execution boundary is chosen by capability, plus a mandatory trust gate.

Scope:
- Subprocess protocol (stdio JSON-RPC) for process-shaped behaviors — drive/trigger, terminals, MCP. These are already subprocess-shaped in HS2 (ACP, Codex app-server, hotsheet-mcp shim); an external drive plugin is another executable speaking the capability protocol. OS crash-isolation.
- WASM (wasmtime/extism) for pure-compute transforms wanting a tighter sandbox — host exposes only the docs/05 §5.10 adapters (ProcessSpawner, config writer, PermissionTransport, McpConfigWriter, Clock) as capability-scoped imports; ambient fs/net denied.
- Trust gate (mandatory): install-time consent showing exactly what a plugin writes and what it launches, plus provenance (first-party / signed / unsigned third-party). Manifest-only plugins can write but never execute host code.
- `hotsheet plugin verify <id>` runs the §5.10 conformance suite against hs-fake-agent — the acceptance test a third-party plugin must pass (we can't CI someone else's plugin).

Acceptance: an external subprocess drive plugin drives hs-fake-agent through one turn; a WASM transform runs with fs/net denied; install refuses/annotates by provenance; `plugin verify` passes/fails a plugin against the conformance suite.

Depends on: HS2-92 (loader/manifest), HS2-64 (fake-agent + conformance suite), HS2-67 (drive trait).

## Notes

<!-- note: 01M0H6M3T5NBHC8TN0DHXV7JFC -->
2026-08-20T11:36:16.264Z — Trust-gate + verify half DONE + verified (committed b67e4c3). Split from the behavioral boundary, which is now HS2-105.

Built:
- Path-safety: a plugin's write targets (instruction/skill/MCP) must stay inside the project (no absolute, no ..). hotsheet_plugins::is_safe_rel_path + Plugin::unsafe_targets; the setup writer REFUSES a plugin with an escaping target (can't be tricked into writing outside the project).
- hotsheet-cli plugin verify <id>: structural check (loads, known MCP format, safe targets). plugin info <id> + install disclose what a plugin writes + launches + provenance (first-party vs unsigned third-party); install runs verify + shows the disclosure + requires confirmation (--yes to skip).
Verified: info/verify on built-ins; install refuses an escaping-target plugin with a clear message.

Deferred (moved to HS2-105): the behavioral subprocess/WASM execution boundary — not needed yet because all current plugins are manifest-only DATA (no code). Also deferred: the hs-fake-agent conformance half of verify (HS2-64).

107 Rust tests pass; fmt+clippy clean. docs/05 §5.11, CODEBASE-MAP, README updated.
