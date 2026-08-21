---
id: 01M0H6M3SJ2KCSPT06XYQG8HWC
slug: HS2-RT76H2
title: 'DECIDE (area 15): Shell integration (OSC) — which OSC 133 parts?'
category: investigation
priority: default
status: completed
created_at: 2026-08-19T01:30:56.374Z
updated_at: 2026-08-19T05:24:33.074Z
completed_at: 2026-08-19T05:24:33.074Z
closed_at: 2026-08-19T05:24:33.074Z
close_reason: completed
legacy_number: HS2-37
schema: 1
---

Recommend: keep OSC 7 (cwd chip) / 8 (links) / 9 (toasts+native notif). OSC 133 suite (prompt markers, jump, copy-output, hover, ask-Claude) is experimental — pick which parts are v1. See docs/11 area 15. Epic: HS2-22.

## Notes

<!-- note: 01M0H6M3STW65XF46P2YQK2TEQ -->
2026-08-19T05:23:59.675Z — keep 7,8,9 and drop everything else for now

<!-- note: 01M0H6M3ST8CKJ9YP1PDYGH3PA -->
2026-08-19T05:24:33.074Z — **DECIDED (maintainer, 2026-08-19): keep OSC 7, 8, 9; drop everything else (the OSC 133 suite) for now.** Keep: OSC 7 (cwd chip / open-in-Finder), OSC 8 (clickable hyperlinks), OSC 9 (notifications — in-app toasts + native OS notifications when backgrounded). Dropped for v1: OSC 133 prompt markers / jump / copy-last-output / hover-to-rerun / ask-Claude — revisit later if wanted. docs/11 area 15. Build with the terminal manager (HS2-10).
