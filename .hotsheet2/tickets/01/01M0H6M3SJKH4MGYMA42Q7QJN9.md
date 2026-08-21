---
id: 01M0H6M3SJKH4MGYMA42Q7QJN9
slug: HS2-SH5YBC
title: 'DECIDE (area 32): Secure storage, keychain & API keys — port?'
category: investigation
priority: default
status: completed
created_at: 2026-08-19T01:31:39.621Z
updated_at: 2026-08-19T05:01:21.251Z
completed_at: 2026-08-19T05:01:21.251Z
closed_at: 2026-08-19T05:01:21.251Z
close_reason: completed
legacy_number: HS2-54
schema: 1
---

Recommend: core-keep. OS-keychain secure storage w/ fallback, global API-key registry, transparent setting backend. Needed for mTLS certs + provider keys. See docs/11 area 32. Epic: HS2-22.

## Notes

<!-- note: 01M0H6M3SXBYF0CTR59A09GM46 -->
2026-08-19T05:01:21.250Z — **DECIDED: core-keep.** OS-keychain secure storage + global API-key registry (needed for mTLS device certs + provider keys). docs/08 §8.4.
