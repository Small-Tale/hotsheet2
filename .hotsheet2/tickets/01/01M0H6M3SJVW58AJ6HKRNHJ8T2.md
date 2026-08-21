---
id: 01M0H6M3SJVW58AJ6HKRNHJ8T2
slug: HS2-74G7HQ
title: Automatic repo sync engine (aggressive fetch/push/rebase/merge)
category: feature
priority: high
status: not_started
created_at: 2026-08-19T01:24:44.964Z
updated_at: 2026-08-19T01:24:44.964Z
legacy_number: HS2-19
schema: 1
---

Maintainer requirement: syncing the tickets repo(s) must be almost entirely automatic. Hot Sheet aggressively fetches, pushes, and rebases/merges on its own; users CAN do these manually but should almost never need to. Design + build a background sync engine per git-remote store: periodic + event-driven fetch, auto-rebase/merge through the semantic merge driver (HS2-18), auto-commit local changes, auto-push, with backoff, offline tolerance, and conflict surfacing only when the driver truly can't resolve. Coordinate with the fs watcher/reindex. See docs/02-ticket-storage.md §2.12, docs/08.
