---
id: 01M0H6M3SJ5M9E7SB6Z1G9VV2R
slug: HS2-4HS59K
title: 'DECIDE (area 4): Notes, reader mode & feedback — scope?'
category: investigation
priority: default
status: completed
created_at: 2026-08-19T01:30:27.831Z
updated_at: 2026-08-19T05:18:04.863Z
completed_at: 2026-08-19T05:18:04.863Z
closed_at: 2026-08-19T05:18:04.863Z
close_reason: completed
legacy_number: HS2-26
schema: 1
---

Recommend: core-keep notes + FEEDBACK NEEDED loop (now generalizes to human assignment, docs/10). Reader-mode overlay + note-nav = keep-with-changes / lower priority. Decide reader-mode priority. See docs/11 area 4. Epic: HS2-22.

## Notes

<!-- note: 01M0H6M3SQVWT4JD9N5JW406F1 -->
2026-08-19T05:08:17.249Z — keep these concepts.  however, for the reader and feedback needed modes (and also for writing details and notes) I want some improvements:

there are effectively 4 kinds of notes:
- regular notes
- feedback needed
- feedback drafts
- status notes (like those saying a claim expired)

currently, the reader mode shows notes in different ways depending on how it was launched.  that is, if i click provide feedback, it shows a feedback note using the feedback editor style; if i click the reader icon, it shows the same note as as read only style.  in HS2, there should be one reader mode concept that always shows feedback needed / draft notes using the feedback editor style and other notes (and also details) using the reader style.

additionally, for details and notes, there should be a way of making a larger mode when editing, since sometimes editing in the details panel feels too constrained.  i propose having an "edit" button in reader mode that turned reader mode into a larger editing surface. and then also, while editing details / notes, still showing the reader button and if clicked while editing, launching directly into the larger reader / editing mode

<!-- note: 01M0H6M3SQDT0WYXSJFRPFXYGS -->
2026-08-19T05:18:04.862Z — **DECIDED (maintainer, 2026-08-19): keep the concepts, with improvements.** (1) Four note KINDS: regular / feedback_needed / feedback_draft / status (docs/02 §2.6; draft is local/per-user, the rest shared). (2) ONE reader mode whose rendering is driven by note kind, not how it was launched — feedback_needed/draft always use the feedback-editor style; regular/status notes + details use the reader style (fixes HS1's inconsistency). (3) An "Edit" button in reader mode opens a LARGER editing surface for details/notes; while editing inline, the reader button stays available and jumps straight into the larger reader/editing mode carrying the edit. Design: docs/06 §6.8. Build: HS2-65.
