---
id: 01M0H6M3SJY58MJ48B5NQMT6CZ
slug: HS2-5CEGJS
title: 'DECIDE (area 27): Backups, snapshots & repair — mostly drop (git replaces)?'
category: investigation
priority: default
status: completed
created_at: 2026-08-19T01:31:28.291Z
updated_at: 2026-08-19T05:01:15.973Z
completed_at: 2026-08-19T05:01:15.973Z
closed_at: 2026-08-19T05:01:15.973Z
close_reason: completed
legacy_number: HS2-49
schema: 1
---

Recommend: mostly drop — git history IS the backup (docs/02 §2.9). Keep only instance locking (index writer) + index rebuild as "repair" (= reindex). Confirm nothing else from tiered backups/snapshot/repair is needed. See docs/11 area 27. Epic: HS2-22.

## Notes

<!-- note: 01M0H6M3SWBMA8T66FF5AWWEN9 -->
2026-08-19T05:01:15.973Z — **DECIDED: mostly drop.** Git history IS the backup — tiered backups/snapshot/repair subsystem obsoleted. Keep only instance locking (index writer) + index rebuild as "repair" (= reindex). docs/02 §2.9, docs/03 §3.8.
