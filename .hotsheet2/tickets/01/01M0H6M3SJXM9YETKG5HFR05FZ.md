---
id: 01M0H6M3SJXM9YETKG5HFR05FZ
slug: HS2-JAXS4Z
title: 'Build: external-sync engine + provider trait + GitHub Issues provider'
category: feature
priority: default
status: not_started
created_at: 2026-08-19T07:08:49.643Z
updated_at: 2026-08-19T07:29:48.706Z
legacy_number: HS2-73
schema: 1
---

Implement the external-sync design (docs/16): the host-owned sync engine (pull-with-cursor, reconcile, field-level last-writer-wins conflict handling, note/comment union, write files + commit + reindex, scheduled + on-demand + optional webhook), the ExternalSyncProvider trait, the `external` frontmatter block on tickets, per-project connection config (import store + defaults + field mapping), and the FIRST provider: GitHub Issues (PAT/OAuth, labels⇄status/priority/category/tags, comments⇄notes). Tokens in keychain (area 32). See docs/16.

## Notes

<!-- note: 01M0H6M3T1817BFXNDX8XERFZ6 -->
2026-08-19T07:29:48.706Z — **Crate (maintainer, 2026-08-19):** external-sync lives in its own **`hotsheet-extsync`** crate (deps: ticketing + HTTP, no terminals) — separate from `hotsheet-aitools`. See docs/12 §12.2.1, docs/16 §16.3.
