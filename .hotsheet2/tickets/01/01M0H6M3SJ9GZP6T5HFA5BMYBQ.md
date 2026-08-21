---
id: 01M0H6M3SJ9GZP6T5HFA5BMYBQ
slug: HS2-6ADR28
title: Permission bridge + claim/lease coordination primitive
category: feature
priority: default
status: not_started
created_at: 2026-08-19T00:23:31.272Z
updated_at: 2026-08-19T04:08:51.288Z
legacy_number: HS2-11
schema: 1
---

Host-side permission bridge (FIFO queue, WS push to clients, non-modal popup, allow-once/always → persisted allow-rules, route answer back to originating connection) with per-plugin transport adapters. Claim/lease primitive — TWO regimes: (1) single shared server (default): atomic claim-next over the index, renewable lease, write-chokepoint conflict guard, lazy reclaim, poison quarantine; claim state persisted to ticket frontmatter (survives index rebuild). (2) MULTIPLE INDEPENDENT MACHINES over a shared git remote (no single server): git-native decentralized self-claim — a per-ticket claim marker (hs-claim/<ulid>) claimed by an atomic-push CAS (first push wins, second rejected), lease expiry in the marker payload, --force-with-lease renew/steal, reserved-namespace sweep for cleanup. No central coordinator. Marker type (custom ref vs tag) pending spike HS2-63. See docs/05 §5.7, docs/08 §8.5.

## Notes

<!-- note: 01M0H6M3SNQJ9QYM77ZDV3KXWQ -->
2026-08-19T04:08:51.288Z — **Spike HS2-63 resolved (2026-08-19):** git-native marker = **custom ref `refs/hotsheet/claims/<ulid>`** (primary; validated working on GitHub), tags as fallback. CAS-on-push, `--force-with-lease` renew/steal (two-stealer race → exactly one wins), `ls-remote` enumerate, sweep for cleanup — all proven. See docs/08 §8.5.3.
