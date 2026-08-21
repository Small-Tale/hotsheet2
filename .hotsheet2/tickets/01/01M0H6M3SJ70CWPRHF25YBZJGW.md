---
id: 01M0H6M3SJ70CWPRHF25YBZJGW
slug: HS2-P3P3CC
title: 'hotsheet-server: HTTP REST + WebSocket + MCP, tiered auth'
category: feature
priority: default
status: started
created_at: 2026-08-19T00:23:10.068Z
updated_at: 2026-08-20T04:16:37.903Z
legacy_number: HS2-7
schema: 1
---

Thin server binary wrapping the core: JSON REST for CRUD/query, WebSocket (/ws/sync) live push + long-poll fallback, MCP endpoint for hotsheet_* tools, hosts the fs watcher + terminal manager, tiered auth (loopback secret / mTLS off-box), instance-join lifecycle. Runs independently of any client. See docs/04-core-server-cli.md §4.3.

## Notes

<!-- note: 01M0H6M3SM262AY4AB73AJREZG -->
2026-08-19T03:58:07.842Z — **Confirmed (maintainer, 2026-08-19):** server topology = **one server per machine** serving all local projects (HS1 instance model); clients/CLI join it. Lifecycle detail in HS2-59. See docs/04 §4.3.

<!-- note: 01M0H6M3SMKMBJ89HQVYPYA81W -->
2026-08-20T04:16:37.903Z — **Progress (2026-08-20): server v1 + MCP shim built and verified end-to-end.**

### Shared engine ops first
Extracted the one implementation of every ticket operation into `hotsheet-ticketing::ops` (query/create/update/close/claim), so CLI + server + MCP share it with no drift (docs/04 §4.5). Refactored the CLI onto it.

### Server (`crates/hotsheet-server`)
axum HTTP over `ops`: `GET /health` (open), `GET/POST /tickets`, `GET/PATCH /tickets/{id}`, `POST /tickets/{id}/close`; `GET /ws/sync` live push (broadcast bus; create/update/close emit ChangeEvents). **Tier 0 auth** (`X-Hotsheet-Secret`); off-loopback binds refused until mTLS. `ApiTicket` wire DTO (full ticket incl. body + notes). 4 in-process HTTP E2E + a real-socket smoke (CLI + HTTP share one store).

### MCP shim (`crates/hotsheet-mcp`)
Per-project stdio JSON-RPC shim (docs/05 §5.8) exposing `hotsheet_query/get/create/update/close`, proxying the server. Verified the FULL stack: MCP stdin → shim → server → store. 7 protocol tests.

**61 tests pass; fmt + clippy clean.**

### Still open on HS2-7 (own tickets)
- Watcher → live reindex on file change: **HS2-6**. SQLite/FTS index: **HS2-5** (server scans in-memory for now).
- Detached lifecycle / auto-start / instance-join: **HS2-59**. Multi-store (one server, all projects): **HS2-87**.
- mTLS Tier 1 / off-loopback: **HS2-85**. Long-poll WS fallback: (small, unfiled).
- Terminals/PTY manager: **HS2-10**. MCP config-writing + fuller tool surface: **HS2-86**.
