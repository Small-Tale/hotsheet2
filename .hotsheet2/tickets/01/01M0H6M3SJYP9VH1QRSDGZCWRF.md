---
id: 01M0H6M3SJYP9VH1QRSDGZCWRF
slug: HS2-D4GHJQ
title: Human assignment + in-the-loop / team review on git-based tickets
category: feature
priority: high
status: not_started
created_at: 2026-08-19T01:24:49.503Z
updated_at: 2026-08-19T03:57:48.348Z
legacy_number: HS2-20
schema: 1
---

Human assignment distinct from machine claim/lease. DECIDED (maintainer 2026-08-19): person identity = git email; display roster = a COMMITTED people.json in the store (syncs to the team); ONE "People…" control sets assignees AND adds review_requests (each with a work/feedback/review/fyi kind); review requests are SOFT (attention only) — use blocked_by for hard ordering. Fields: assignees[] + review_requests[{who, kind, by:ulid, at}] in shared frontmatter, merge by set-union. Attention delivery: in-app + live WS push + on-sync (desktop notif); iOS push deferred (docs/08 O5). Small follow-ups: off-server notification transport; optionally seed people.json from GitHub collaborators. See docs/10-assignment-and-collaboration.md §10.2/§10.5, docs/02 §2.5.
