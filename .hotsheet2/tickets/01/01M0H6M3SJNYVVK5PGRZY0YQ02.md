---
id: 01M0H6M3SJNYVVK5PGRZY0YQ02
slug: HS2-NM7WC6
title: Minimal plugin loader + first-party Claude plugin (in-repo)
category: feature
priority: high
status: completed
created_at: 2026-08-20T07:51:14.941Z
updated_at: 2026-08-20T08:08:39.056Z
completed_at: 2026-08-20T08:08:39.056Z
closed_at: 2026-08-20T08:08:39.056Z
close_reason: completed
legacy_number: HS2-97
schema: 1
---

Part of HS2-95; minimal subset of HS2-92. Introduce the core `plugins` module (new crate hotsheet-plugins) with just enough to load the FIRST first-party plugin (Claude) and expose its one-shot setup artifacts. Defer search-path/third-party/WASM/trust (HS2-93).

Scope:
- New crate `crates/hotsheet-plugins`: a minimal manifest type (id, displayName, productName, tier, detection {binaries}, mcp-config target/format, instruction target filename, skills target) + a Plugin loaded from a bundled directory.
- First-party Claude plugin dir IN THE REPO (e.g. `plugins/claude/`), embedded into the binary via include_dir. Contents: manifest.toml, the managed CLAUDE.md section(s) describing the Hot Sheet workflow (work the worklist via `hotsheet ls --up-next`, read with `hotsheet show`, update/close via CLI or hotsheet_* MCP), the worklist skill (SKILL.md, adapted from the HS1 skill template), and the MCP-config template registering `hotsheet-mcp` (serverless `--path` form) in Claude's config format.
- Loader API: `builtin_plugins()` / `find("claude")`, returning the artifacts a setup writer consumes (HS2-98). Built-ins resolve via the loader, not a special-case branch (per HS2-92 acceptance).

Tests: load the Claude plugin from the embedded dir; assert manifest fields + that instruction/skill/mcp templates are present and non-empty.

docs: docs/05 §5.11 (first-party in-repo, via the loader — already recorded); note the crate in docs/CODEBASE-MAP.md when built.

## Notes

<!-- note: 01M0H6M3T6W8V1BK275423FDQV -->
2026-08-20T08:08:39.056Z — Done + verified. New crate hotsheet-plugins (the core `plugins` module): Manifest + Plugin loaded from a bundled dir via include_dir; builtin_plugins()/find(id). First-party Claude plugin in-repo at plugins/claude/ (manifest.toml + instructions.md + SKILL.md), embedded into the binary and loaded through the same path a third-party plugin will use — no special-casing (docs/05 §5.11 anti-drift). MCP template registers serverless hotsheet-mcp (--path {store}); mcp_args() substitutes the store path.

Deferred to HS2-92/HS2-93 as planned: on-disk search path, third-party install, subprocess/WASM behavioral boundary, trust gate.

Tests (5): Claude loads with expected manifest + non-empty instruction/skill artifacts + declared targets; mcp args substitute store path; builtins all load; unknown id None. 79 Rust tests pass; fmt + clippy clean. Committed 5c58a24 (local). CODEBASE-MAP updated (crate + plugins/ dir + where-to-look).

Next: HS2-98 (hotsheet setup claude) consumes this loader to write the artifacts into a project.
