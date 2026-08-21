---
id: 01M0H6M3SJ5ZXV1RME4PTHGVCZ
slug: HS2-R4GMG1
title: Shared-vs-local ticket data model + local overlay storage
category: feature
priority: high
status: not_started
created_at: 2026-08-19T01:24:53.862Z
updated_at: 2026-08-19T01:24:53.862Z
legacy_number: HS2-21
schema: 1
---

Decide what belongs in the committed (shared) ticket file vs per-user/per-machine local-only data, and where local data lives. Recommendation (docs/02 §2.11): shared fields in the ticket frontmatter/body (title/details/status/priority/tags/notes/blocked_by/assignees/attachments/timestamps); per-user/local data (read tracking last_read_at, UI/view state, feedback drafts, machine-specific prefs) NOT committed. Local durable data lives ON DISK in gitignored overlay files in the store (so it survives an index rebuild — the 'everything reconstructs from disk' principle), with the index as a cache; the DB/index is never the sole home of durable data. Also covers whole local-only stores (visibility:local). See docs/02-ticket-storage.md §2.11.
