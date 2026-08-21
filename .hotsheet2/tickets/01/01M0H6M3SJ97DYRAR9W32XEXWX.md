---
id: 01M0H6M3SJ97DYRAR9W32XEXWX
slug: HS2-TAEMZN
title: 'Headless work loop: `hotsheet-cli work` — trigger until Up Next is drained'
category: feature
priority: default
status: not_started
created_at: 2026-08-21T03:27:21.756Z
updated_at: 2026-08-21T03:27:21.756Z
legacy_number: HS2-118
schema: 1
---

Follow-up from HS2-109 / the bootstrap north star. HS2-109 drives ONE turn. For real headless bootstrap, add a loop that keeps driving until the Up Next queue is empty (or a max-iterations/budget cap):
- `hotsheet-cli work <tool> [--max N] [--worker]` — repeatedly: check Up Next (via ops/query); if empty, stop; else trigger one turn (reusing/resuming the same session where the transport supports it, e.g. claude --resume <session_id> / codex thread resume), report per-ticket outcome.
- Resume continuity: thread the session_id/thread_id from the first turn into subsequent triggers so it's one persistent instance, not a fresh process per ticket.
- Guard against thrash: stop if a ticket stays not_started/started across N turns (the agent isn't making progress), and surface it.
- Belongs on the critical path to HS2 bootstrapping its own dev. Apply the baked-in launch safety (sibling follow-up).

Relates to: HS2-109, HS2-9 (channel resume), HS2-112 (codex thread resume), the bootstrap north star.
