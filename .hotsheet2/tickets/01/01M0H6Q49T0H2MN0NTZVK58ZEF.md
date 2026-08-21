---
id: 01M0H6Q49T0H2MN0NTZVK58ZEF
slug: HS2-9YZJCH
title: 'Dogfood: HS2 tickets migrated into in-repo .hotsheet2 store'
category: task
priority: default
status: completed
created_at: 2026-08-21T03:44:30.011241Z
updated_at: 2026-08-21T03:44:30.035744Z
completed_at: 2026-08-21T03:44:30.035309Z
schema: 1
---

Migrated this project's 118 tickets off the HS1 dev instance into a committed HS2 store at .hotsheet2/ (git-native, legacy_number preserved). Placed at .hotsheet2/ rather than canonical .hotsheet/tickets/ because HS1's live datadir squats .hotsheet/. HS2 now tracks its own development. Follow-up: full cutover (retire HS1 for this project; point sessions/agents at .hotsheet2) when the work loop lands.

## Notes

<!-- note: 01M0H6Q4AK5Z20X6057EP8AB8V -->
2026-08-21T03:44:30.035744Z — Done + verified: hotsheet-cli -C .hotsheet2 ls reads all 119. This ticket is the first write to HS2's own store.
