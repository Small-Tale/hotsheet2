---
id: 01M0H6M3SJMKG5NHQDSKD2FZ65
slug: HS2-0RK4YC
title: 'External-sync providers: GitLab Issues + Jira (on the same interface)'
category: feature
priority: low
status: not_started
created_at: 2026-08-19T07:08:53.551Z
updated_at: 2026-08-19T07:08:53.551Z
legacy_number: HS2-74
schema: 1
---

After the engine + GitHub provider land, add GitLab Issues and Jira providers implementing the same ExternalSyncProvider trait (docs/16). Jira maps a sensible subset (issue type/status-category/priority/assignee/comments); don't mirror epics/sprints/custom-fields fully at first. Validates that the interface generalizes across all three (maintainer's ask). See docs/16 §16.4.
