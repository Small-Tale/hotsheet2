---
id: 01M0H6M3SJJ37A7ZYCZB8RBD43
slug: HS2-CARMDM
title: Formalize the external-sync plugin interface (GitHub Issues first)
category: feature
priority: high
status: completed
created_at: 2026-08-19T06:29:08.044Z
updated_at: 2026-08-19T07:08:57.402Z
completed_at: 2026-08-19T07:08:57.402Z
closed_at: 2026-08-19T07:08:57.402Z
close_reason: completed
legacy_number: HS2-71
schema: 1
---

Maintainer (2026-08-19, HS2-53): formalize a DEDICATED external-sync-system plugin interface instead of HS1's broad general plugin system (docs/18). Purpose-built for syncing HS2 git-tickets with external ticketing systems. GitHub Issues is especially important (a lot of user-facing tickets come in through it). Distinct from git-native store sharing (stores-as-GitHub-repos): this syncs with GitHub's Issues tracker. Define the interface: identity/auth (PAT/OAuth), field mapping (category/priority/status ↔ labels/state), bidirectional pull (import external → HS2 files) + push (HS2 changes → external), incremental sync (since cursor), comments↔notes, attachments, conflict handling, scheduled + event-driven sync, and how imported tickets land in a store/project. First implementation: GitHub Issues. Reuses HS1's sync-engine concepts (docs/18/19/88) but as a specific interface, not a general plugin loader. Output: the interface spec + a GitHub Issues sync plugin. See docs/11 area 31.

## Notes

<!-- note: 01M0H6M3T1GX8WM7FFA6EXYC3R -->
2026-08-19T06:31:13.864Z — should plan to support github, gitlab, and jira, in terms of generalizing the interface

<!-- note: 01M0H6M3T14F8JMM38TBHRRQCC -->
2026-08-19T07:08:57.402Z — **Spec delivered: docs/16-external-sync-interface.md.** A dedicated external-sync interface (not HS1's general plugin loader): the `external` frontmatter block anchors a ticket to its remote counterpart; one `ExternalSyncProvider` trait (declarative auth + field-mapping + pull/push, host owns the sync engine); configurable field mapping (status/priority/category/tags ⇄ labels/state) generalized across **GitHub, GitLab, Jira**; host-owned sync loop (pull-cursor → reconcile → push → commit → reindex); external-vs-local conflict handling (field-level last-writer-wins, notes/comments union, visible status-note on clash); imported tickets land in a configured import store; tokens in keychain. Follow-ups filed: **HS2-73** (engine + trait + GitHub provider), **HS2-74** (GitLab + Jira). Open questions in §16.9.
