---
id: 01M0H6M3SJA4SE90A4VHZB8TX4
slug: HS2-SRTW4M
title: External plugin loader + manifest format (data-only plugins, no ABI)
category: feature
priority: high
status: completed
created_at: 2026-08-20T06:37:18.405Z
updated_at: 2026-08-20T10:51:27.985Z
completed_at: 2026-08-20T10:51:27.985Z
closed_at: 2026-08-20T10:51:27.985Z
close_reason: completed
legacy_number: HS2-92
schema: 1
---

Design decided 2026-08-20 (docs/05 §5.11). Plugins are external + loadable; the common case has no ABI because it is pure data.

Scope:
- Core loader in `plugins` reading a search path: bundled built-ins → ~/.hotsheet/plugins/ (machine) → project .hotsheet/plugins/.
- Manifest-only plugin format: a directory with a manifest (id, detection {binaries,paths}, preferences, tier, transport id, launch command, MCP-config format) + template files (instruction file, skills/rules tree). No code, no sandbox needed for the data itself.
- Built-in tools (Claude, Codex, Gemini) ship as first-party plugins loaded through this same loader — anti-drift discipline (docs/05 §5.10).
- Registry is loaded identically by CLI and server so `hotsheet setup <third-party-tool>` works headless.
- CLI: `hotsheet plugin list | info <id> | install <path|url> | remove <id>` (install is trust-gated — see HS2-93).

Acceptance: a manifest-only plugin dropped into the search path is detected and usable by `hotsheet setup` with no recompile and no running server. Built-ins resolve through the same code path as third-party ones.

Relates to: HS2-91 (setup consumes the registry), HS2-93 (behavioral boundary + trust gate).

## Notes

<!-- note: 01M0H6M3T5VSKKGVYPB9Q1PRAF -->
2026-08-20T06:47:08.943Z — Sequencing decided (maintainer, 2026-08-20): the first-party plugins live IN THIS REPO from day one and load through the loader — NOT compiled-in-then-extracted later. The initial set (Claude, Codex, …) ships as first-party plugin directories in the HS2 repo, bundled as the built-in search-path entry, loaded by the same loader a third party uses. Third-party plugins are a post-release capability (developers add their own under ~/.hotsheet/plugins/ or project .hotsheet/plugins/, no fork, no recompile). One loader, one plugin shape; "first-party" is only a provenance/trust label. Acceptance additions: (1) built-in tools resolve via the loader's built-in search-path entry, not a special-case branch; (2) a plugin dropped into a user search path loads identically to a built-in.

<!-- note: 01M0H6M3T595QJCWS5V94ZC55V -->
2026-08-20T10:51:27.985Z — Done + verified. One loader path for bundled (include_dir) and on-disk plugins: Plugin::from_fs_dir, load_dir, all_plugins(search_dirs), find_in. Machine search dir ${HOTSHEET_HOME:-~/.hotsheet2}/plugins/<id>/ (off HS1's ~/.hotsheet). First-party id wins a collision; bad on-disk plugins skipped. Plugin carries PluginSource (BuiltIn|Disk) provenance. hotsheet-cli plugin list|install|remove; setup + --detect use the full registry so `hotsheet-cli setup <third-party-id>` works with no recompile / no server.

Acceptance met: verified end to end — installed an on-disk `acme` plugin, `plugin list` showed it (with its path), `setup acme` wrote its managed instruction block + .mcp.json into a project, `plugin remove` cleaned up. Built-ins resolve through the same code path.

Deferred to HS2-93: behavioral subprocess/WASM boundary + trust gate + `plugin verify`.
Tests (13 across plugins+cli plugin module): on-disk==built-in, first-party-wins-collision, skip bad dirs, home avoids ~/.hotsheet, install/remove roundtrip + rejects built-in id/non-plugin dir. 98 Rust tests pass; fmt+clippy clean. Committed 323748a. docs/05 §5.11, CODEBASE-MAP, README updated.
