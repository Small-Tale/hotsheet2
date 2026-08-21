---
id: 01M0H6M3SJTPV8A7XTDCBW1A38
slug: HS2-QJAPFJ
title: Semantic git merge driver for automatic ticket conflict resolution
category: feature
priority: high
status: not_started
created_at: 2026-08-19T00:55:20.574Z
updated_at: 2026-08-19T00:55:20.574Z
legacy_number: HS2-18
schema: 1
---

Maintainer requirement: conflict resolution must be almost entirely automatic. Implement `hotsheet merge-driver` (CLI subcommand, shared core logic) as a format-aware 3-way merge for tickets/**/*.md, registered per store via .gitattributes (merge=hotsheet-ticket) written by `hotsheet init`. Merge rules: frontmatter field-by-field (last-writer-wins by updated_at; tags/blocked_by set-union), notes unioned by their timestamp-ordered UUID and sorted, body prose only 3-way-merged when both sides changed it. Fall back to git text merge on driver error (never silent data loss). `hotsheet doctor` flags stores missing the registration. See docs/02-ticket-storage.md §2.7, docs/04-core-server-cli.md §4.4, docs/09 §9.1a.
