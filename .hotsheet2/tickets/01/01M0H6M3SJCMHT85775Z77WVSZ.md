---
id: 01M0H6M3SJCMHT85775Z77WVSZ
slug: HS2-NG4TGN
title: Copy & move tickets between stores (copy = new ULID; move = copy + source tombstone)
category: feature
priority: high
status: not_started
created_at: 2026-08-19T02:13:03.541Z
updated_at: 2026-08-19T02:23:31.920Z
legacy_number: HS2-60
schema: 1
---

Cross-store copy/move. COPY: new ticket (new ULID) in the destination store with same content + copied attachments, records copied_from provenance; original untouched. MOVE (no true git move — content stays in the source repo history forever): copy-to-destination KEEPING THE SAME ULID (so references keep resolving; slug takes the dest prefix) + attachments, and leave a tombstone/redirect in the source (status: moved, moved_to_store, moved_at) that the UI hides. Index keys (store_id, id) so tombstone + live coexist; ULID resolves to the single live instance following moved_to_store. RETENTION CAVEAT (security): moving OUT of a shared/remote store does NOT remove content from that repo's history or remote — Hot Sheet never force-rewrites history automatically; true purge is a manual git filter-repo/BFG + force-push. WARN before any move that changes exposure (private<->shared). Surfaces: hotsheet copy/move --to <store> (CLI), MCP tools, UI drag-onto-store / "Move to store…". See docs/02 §2.13, docs/03 §3.3/§3.5.

## Notes

<!-- note: 01M0H6M3SY5W4YGTQERHKJ5G3H -->
2026-08-19T02:23:31.920Z — **Design confirmed (maintainer, 2026-08-19):** move keeps the same ULID (source becomes a tombstone/redirect) so `blocked_by`/mentions survive the move — chosen over minting a new ULID. Docs/02 §2.13 marked confirmed.
