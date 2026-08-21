---
id: 01M0H6M3SJVMZV41HN52WHGNM5
slug: HS2-PEGFKW
title: Bake HS2-103 launch safety into `hotsheet-cli trigger` (shim + PATH + mcp isolation)
category: feature
priority: default
status: not_started
created_at: 2026-08-21T03:27:15.769Z
updated_at: 2026-08-21T03:27:15.769Z
legacy_number: HS2-117
schema: 1
---

Follow-up from HS2-109. The trigger works end to end, but the caller currently has to assemble the HS2-103 safety by hand: prepend a hotsheet->hotsheet-cli shim dir + target/debug to the child PATH (via --env PATH=...) and pass --mcp-config for strict isolation. Bake this in so a bare `hotsheet-cli trigger <tool>` is safe by default:
- Auto-create a transient hotsheet->hotsheet-cli shim dir and prepend it (+ the dir of the running hotsheet-cli exe, so hotsheet-mcp/hotsheet-cli resolve) to the launched tool's PATH.
- Preflight: assert `command -v hotsheet` resolves to our shim, and assert no HS1 store under the project (assert_no_hs1), before launching.
- For codex, default an isolated CODEX_HOME (symlink packages + copy auth.json, strip mcp_servers) — see the HS2-112 live harness.
- Consider setup writing an ABSOLUTE hotsheet-mcp command path so the MCP works without PATH munging.

Relates to: HS2-109, HS2-103, HS2-112.
