---
id: 01M0H6M3SJHPHZS684FJN3M68W
slug: HS2-F3SS63
title: Notes model (4 kinds) + unified reader mode + larger editing surface
category: feature
priority: default
status: not_started
created_at: 2026-08-19T05:18:01.775Z
updated_at: 2026-08-19T05:18:01.775Z
legacy_number: HS2-65
schema: 1
---

Notes carry a `kind`: regular / feedback_needed / feedback_draft / status (docs/02 §2.6). Storage tiering: regular/feedback_needed/status are shared (committed inline notes); feedback_draft is local/per-user (gitignored overlay, §2.11 Tier B) and becomes a regular shared note on submit. UI (docs/06 §6.8): ONE reader mode whose rendering is driven by note KIND not launch point — feedback_needed/feedback_draft render in the feedback-editor style (respond/continue draft); regular/status notes + details render read-only. Reader mode gets an "Edit" button that turns it into a LARGER editing surface for details/notes; while editing in the detail panel, the reader button stays available and, if clicked, launches directly into the larger reader/editing mode carrying the in-progress edit. See docs/02 §2.6, docs/06 §6.8. Resolves HS2-26.
