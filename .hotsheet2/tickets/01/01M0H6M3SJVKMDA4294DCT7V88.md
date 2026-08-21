---
id: 01M0H6M3SJVKMDA4294DCT7V88
slug: HS2-W6JHT1
title: Tauri desktop client to feature floor (API consumer; auto-starts local server)
category: feature
priority: default
status: not_started
created_at: 2026-08-19T00:23:36.706Z
updated_at: 2026-08-19T03:58:06.327Z
legacy_number: HS2-12
schema: 1
---

Tauri client: Rust shell + web UI, a PURE API CONSUMER (does NOT embed the core — maintainer decision 2026-08-19). The Rust shell launches + supervises an independent local hotsheet-server (see HS2-59) and holds the Rust-side loopback mTLS proxy for remote projects; the web UI talks HTTP/WS to that server. Web UI on a small reactive framework (recommend Solid/Svelte, not a hand-rolled runtime). Feature floor: ticket list/columns, categories/priorities/statuses, up_next, tags, notes, attachments, live WS updates, AI-drive surface (launch/trigger/permission popup/busy/connection count), multi-project tabs (each a server origin), FTS search. See docs/06-clients.md §6.3, §6.6.

## Notes

<!-- note: 01M0H6M3SNDC97YXH4W5HWRJ5G -->
2026-08-19T03:58:06.327Z — **Decided (maintainer, 2026-08-19):** the Tauri web UI uses **Solid**. See docs/09 §9.5.
