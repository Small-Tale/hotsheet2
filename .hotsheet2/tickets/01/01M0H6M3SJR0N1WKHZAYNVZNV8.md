---
id: 01M0H6M3SJR0N1WKHZAYNVZNV8
slug: HS2-WH92PR
title: 'MCP shim: config-writing capability + expand tool surface (claim/batch/announce/signal_done)'
category: feature
priority: default
status: not_started
created_at: 2026-08-20T04:15:12.303Z
updated_at: 2026-08-20T04:15:12.303Z
legacy_number: HS2-86
schema: 1
---

The v1 MCP shim (HS2-7) exposes hotsheet_query/get/create/update/close proxying the server. Remaining (docs/05 §5.8): (1) the `mcp` plugin CAPABILITY that WRITES the per-project shim entry into each AI tool's config (the HS1 channel.ts install), with per-project namespacing (`hotsheet-channel-<slug>`); (2) expand the tool surface to match the HS1 set the worklist expects — claim_next/release/renew, batch, toggle_up_next, add/edit/delete note, add_attachment, announce, request_feedback, signal_done, query_tickets. Needs server endpoints for the ones not yet exposed (claim/notes/attachments/announce/signal). Follow-up of HS2-7 / HS2-43.
