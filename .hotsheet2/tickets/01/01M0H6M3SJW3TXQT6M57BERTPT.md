---
id: 01M0H6M3SJW3TXQT6M57BERTPT
slug: HS2-21VNE8
title: 'Native SwiftUI clients (API consumers): macOS then iOS'
category: feature
priority: default
status: not_started
created_at: 2026-08-19T00:23:42.793Z
updated_at: 2026-08-19T01:56:14.108Z
legacy_number: HS2-13
schema: 1
---

Native SwiftUI apps as PURE API CONSUMERS over HTTP/WS (no core embedding, no uniffi — retired with the server-always-separate decision, 2026-08-19). Confirmed order: after Tauri+web (HS2-12) — macOS SwiftUI first, then iOS. macOS auto-starts/supervises the local server (HS2-59). iOS is remote-first and structurally so (no local server on iOS due to background limits): view/triage/search/answer-permission-prompts against a Mac's server; QR-pairing + keychain-held device cert. See docs/06-clients.md §6.4, docs/09 §9.5.
