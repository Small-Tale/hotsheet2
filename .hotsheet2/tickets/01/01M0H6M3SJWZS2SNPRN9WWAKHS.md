---
id: 01M0H6M3SJWZS2SNPRN9WWAKHS
slug: HS2-NGB37B
title: 'DECIDE (area 19): Drive transports (MCP-hooks/ACP/Codex) — minimal set?'
category: investigation
priority: default
status: completed
created_at: 2026-08-19T01:31:06.095Z
updated_at: 2026-08-19T06:20:18.873Z
completed_at: 2026-08-19T06:20:18.873Z
closed_at: 2026-08-19T06:20:18.873Z
close_reason: completed
legacy_number: HS2-41
schema: 1
---

Recommend: reconsider — pick a minimal transport set for v1 (Claude channel given; then choose among MCP+hooks, ACP, Codex app-server). docs/05 §5.5 models them as one trait. See docs/11 area 19. Epic: HS2-22.

## Notes

<!-- note: 01M0H6M3SV2211R3QZ2KSWV3T4 -->
2026-08-19T06:17:51.574Z — yes, i dont think we'll have a single one that will be compatible, but we should have a single interface, potentially with different capabilities that can be optionally conformed to, that can be applied to all the ai tools.  we should investigate the design of this interface sooner rather than later

<!-- note: 01M0H6M3SVK7H42BHT1VK7XM1Z -->
2026-08-19T06:20:18.873Z — **DECIDED (maintainer, 2026-08-19):** not a fixed minimal SET of separate transports — instead **ONE drive interface with optional capabilities each tool conforms to as applicable** (no single universal transport; absence = not supported). v1 covers Claude (persistent channel) + Codex (spawn/app-server); ACP/others plug in later as capabilities. **Design this interface EARLY** — filed as HS2-67 (investigate sooner rather than later). docs/05 §5.5, docs/11 area 19.
