---
id: 01M0H6M3SJFYNWY41KFJGER64E
slug: HS2-WXM1WD
title: Antigravity (agy) plugin + spawn drive with conversation resume
category: feature
priority: default
status: completed
created_at: 2026-08-20T22:35:34.372Z
updated_at: 2026-08-20T22:41:43.042Z
completed_at: 2026-08-20T22:41:43.042Z
closed_at: 2026-08-20T22:41:43.042Z
close_reason: completed
legacy_number: HS2-111
schema: 1
---

Integrate Antigravity (agy 1.1.7) as a first-party plugin driven by the SpawnDrive — the weakest tool (no daemon), so make it as good as agy allows: conversation RESUME so a trigger continues the same thread (agy --conversation <id> --print "<prompt>"), not a fresh chat each time.

Scope:
- SpawnDrive/SpawnConfig gain an optional resume_flag (e.g. "--conversation"): when the Target carries a session id, inject `<resume_flag> <id>` into the args (spawn+resume). Manifest DriveSpec gains resume_flag; host::drive_for threads it.
- plugins/antigravity/: manifest (id=antigravity, detection binaries=[agy]) + instructions.md; [drive] transport=spawn program=agy args=["--print"] content=arg interrupt=true resume_flag="--conversation"; [mcp] target=".agents/mcp_config.json" format=claude-json (Antigravity consumes MCP via a standard mcpServers object — official docs). Instruction target=AGENTS.md (verify agy's convention; correct if needed). Embed in the loader.
- Tests: agy plugin loads + drive_for builds a spawn drive; SpawnDrive resume injects --conversation <id> when Target is set, omits it when not; setup antigravity writes AGENTS.md block + .agents/mcp_config.json.

Then: a gated LIVE effectiveness run (billable) — drive real agy with a task and observe (completed/failed, session continuity), reusing HS2-103 launch safety (agy has shell tools → could hit HS1 hotsheet). Offer, don't auto-run.

Relates to: HS2-106 (SpawnDrive), HS2-108 (host), HS2-109 (real trigger), HS2-103 (safety).

## Notes

<!-- note: 01M0H6M3T8KHXW5MFNQVBBZE18 -->
2026-08-20T22:41:43.042Z — Integration DONE + verified deterministically. Antigravity (agy) is the 3rd first-party plugin (plugins/antigravity/): detection agy, AGENTS.md instructions, no skills, [mcp] .agents/mcp_config.json (Gemini mcpServers), [drive] spawn + resume_flag=--conversation. SpawnDrive/SpawnConfig gained resume_flag; SpawnDrive::agy(); when a Target session id is present the spawn injects --conversation <id> before the prompt (continuous thread, best a no-daemon tool can do). Verified: hotsheet-cli setup antigravity writes the AGENTS.md block + .agents/mcp_config.json (abs). 124 tests pass; committed 6d8ac12 (pushed).

LIVE effectiveness run BLOCKED on safety (not run): ~/.gemini/config/mcp_config.json contains a hotsheet-channel MCP → /Users/westphal/Documents/hotsheet/src/channel.ts (HS1). agy loads that global config and has NO clean config-home isolation flag (no CODEX_HOME equivalent found; only --add-dir for workspace). Running agy would spawn the HS1 channel and could kill the dev instance again (the HS2-103 class). Awaiting maintainer decision on how to isolate (temporarily neutralize the .gemini hotsheet-channel entry with backup/restore, or a config-home env var if one exists) before running the live agy effectiveness test.
