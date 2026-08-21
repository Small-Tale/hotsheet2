---
id: 01M0H6M3SJFG915DREBNNKQCAW
slug: HS2-CE1AET
title: 'DECIDE (area 28): Remote access & multi-client — phase after local floor?'
category: investigation
priority: default
status: completed
created_at: 2026-08-19T01:31:30.755Z
updated_at: 2026-08-19T05:01:17.498Z
completed_at: 2026-08-19T05:01:17.498Z
closed_at: 2026-08-19T05:01:17.498Z
close_reason: completed
legacy_number: HS2-50
schema: 1
---

Recommend: keep-with-changes, carried over as-is (docs/04 §4.6, docs/08); phase after the local floor. mTLS + per-client certs, WS push sync, request hardening, remote-client tab mounting. See docs/11 area 28. Epic: HS2-22.

## Notes

<!-- note: 01M0H6M3SWFHB7PXXNS0Y2JGT8 -->
2026-08-19T05:01:17.498Z — **DECIDED: keep-with-changes, phase after the local floor.** mTLS + per-device certs + enrollment/QR carried over as-is; orchestration = live-mount only (no auto-clone); remote terminals deferred past v1. docs/04 §4.6, docs/08.
