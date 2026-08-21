---
id: 01M0H6M3SJKCPWCE1WVBZHZ9J9
slug: HS2-CX3YK2
title: 'DECIDE (area 34): Database & storage internals — replace with git+SQLite?'
category: investigation
priority: default
status: completed
created_at: 2026-08-19T01:31:44.051Z
updated_at: 2026-08-19T05:01:25.730Z
completed_at: 2026-08-19T05:01:25.730Z
closed_at: 2026-08-19T05:01:25.730Z
close_reason: completed
legacy_number: HS2-56
schema: 1
---

Recommend: replace wholesale (build HS2-4/5). PGLite/inline migrations/.hotsheet layout/telemetry clusters/WAL mitigations → git stores + SQLite index (docs/02, 03). Confirm nothing depends on PGLite semantics. See docs/11 area 34. Epic: HS2-22.

## Notes

<!-- note: 01M0H6M3SYKS3MWQJ8X8034ZPG -->
2026-08-19T05:01:25.730Z — **DECIDED: replace wholesale.** PGLite / inline migrations / .hotsheet cluster layout / telemetry clusters replaced by git stores + a rebuildable SQLite index. docs/02, docs/03. Build: HS2-4/HS2-5.
