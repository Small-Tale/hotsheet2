---
id: 01M0H6M3SJCN9BB7Y80FQ8KB95
slug: HS2-QRSD6W
title: 'DECIDE (area 21): MCP tool surface — port proxying the core?'
category: investigation
priority: default
status: completed
created_at: 2026-08-19T01:31:10.740Z
updated_at: 2026-08-20T04:15:47.137Z
completed_at: 2026-08-20T04:15:47.137Z
closed_at: 2026-08-20T04:15:47.137Z
close_reason: completed
legacy_number: HS2-43
schema: 1
---

Recommend: core-keep (build HS2-7/9). The hotsheet_* tools (create/update/query/batch/claim/announce/signal_done/request_feedback) proxying the Rust core (not REST-over-HTTP). Per-project server naming. See docs/11 area 21. Epic: HS2-22.

## Notes

<!-- note: 01M0H6M3SVG6A160K4WKTGK9YA -->
2026-08-19T05:01:11.473Z — **DECIDED: core-keep.** hotsheet_* MCP tools proxying the Rust core, delivered via a per-project MCP shim. docs/05 §5.8. Build: HS2-7/HS2-9.

<!-- note: 01M0H6M3SV8KSV4ESPFM0REJ77 -->
2026-08-20T04:15:47.137Z — **Built (2026-08-20): the MCP shim exists.** `crates/hotsheet-mcp` → the `hotsheet-mcp` binary: a stdio JSON-RPC 2.0 server exposing `hotsheet_query` / `get` / `create` / `update` / `close`, proxying a running `hotsheet-server` over HTTP (per docs/05 §5.8 — a per-project shim, NOT the server exposing MCP directly). Verified end-to-end: MCP stdin → shim → server → store (create + query round-trip). 7 unit tests over the JSON-RPC protocol + tool dispatch.

Remaining (filed as **HS2-86**): the `mcp` plugin CAPABILITY that writes the shim entry into each tool's config with per-project namespacing, and the fuller tool surface (claim/batch/notes/attachments/announce/signal_done/request_feedback) — several need new server endpoints. This DECIDE ticket's recommendation (core-keep, shim proxying the core/server) is realized.
