---
id: 01M0H6M3SJDT75BGX76TAGEZB9
slug: HS2-AB09K6
title: 'MCP shim serverless mode: CoreBackend (direct-to-disk via ops)'
category: feature
priority: high
status: completed
created_at: 2026-08-20T07:51:05.157Z
updated_at: 2026-08-20T08:04:32.400Z
completed_at: 2026-08-20T08:04:32.400Z
closed_at: 2026-08-20T08:04:32.400Z
close_reason: completed
legacy_number: HS2-96
schema: 1
---

Part of HS2-95. Today `hotsheet-mcp` only has HttpBackend (requires a running server). Add a direct-to-disk backend so MCP works headless with NO server — symmetric with the CLI's direct-to-disk path.

Scope:
- New `CoreBackend` in crates/hotsheet-mcp implementing the existing `Backend` trait (get/send) against `hotsheet_ticketing::{FsStore, ops}` directly — no HTTP. Maps the REST-shaped paths the dispatcher already uses (/tickets, /tickets/{id}, POST /tickets, PATCH /tickets/{id}, POST /tickets/{id}/close) onto ops calls.
- Binary selection: `hotsheet-mcp --path <store>` → serverless CoreBackend; `hotsheet-mcp --server <url> --secret <s>` → existing HttpBackend. Exactly one required.
- Reads use ops::query file-scan (no index needed when serverless, consistent with docs/04 §4.4). When a server IS running, prefer HttpBackend for index-backed reads + instant broadcast; a serverless write is still picked up by the server's watcher.
- Injected Clock/Rng for mint (deterministic tests).

Tests: reuse the handle_message tests against CoreBackend over a TempStore; assert create/get/query/update/close round-trip on disk with no server. Keep FakeBackend tests too.

docs: update docs/05 §5.8 (shim runs direct-core OR proxies a server).

## Notes

<!-- note: 01M0H6M3T541Z9FCM1RXBKHPJF -->
2026-08-20T08:04:32.400Z — Done + verified. Added CoreBackend (direct-to-disk over ops) to hotsheet-mcp; binary selects --path <store> (serverless) vs --server <url> --secret (HTTP proxy). Both implement the one Backend trait so the hotsheet_* surface is identical.

Bonus (wire SSOT, docs/04 §4.2): lifted the wire DTOs into hotsheet_ticketing::wire — ApiTicket/ApiNote/TicketRow + From<&Ticket> — now shared by the server and both MCP backends, so the JSON an agent sees can't drift between modes. Index re-exports TicketRow; server dropped its local ApiTicket copy.

Tests: CoreBackend drives full create/get/query/update/close over a temp store with no server; not-found + duplicate-without-target surface as tool errors. Verified end-to-end over real stdio (initialize/query/create) — the MCP-created ticket landed on disk (confirmed via CLI ls), no server running. All 74 Rust tests pass; fmt + clippy clean. Committed c334f5e (local, not pushed).

docs updated: 05 §5.8 (dual-backend shim), CODEBASE-MAP (wire.rs + shim modes), README summary.

Note: the sandboxed test runner can't receive FS-notify events, so watcher_reindexes_an_external_write only passes with the sandbox disabled (not a code issue).
