---
id: 01M0H6M3SJ0SPQD11WK94VEGCA
slug: HS2-B01JZB
title: 'Migrator + importer: attachments (copy files + write attachments/&lt;id&gt;/)'
category: feature
priority: default
status: completed
created_at: 2026-08-19T11:16:05.894Z
updated_at: 2026-08-20T02:06:39.038Z
completed_at: 2026-08-20T02:06:39.038Z
closed_at: 2026-08-20T02:06:39.038Z
close_reason: completed
legacy_number: HS2-78
schema: 1
---

Attachments are not yet migrated. Add: (exporter) read the HS1 `attachments` table, emit `attachments: [{original_filename, stored_path}]` per ticket in hotsheet-export.json, and stage the files alongside the JSON (docs/07 §7.2 export shape). (importer, hotsheet-cli/src/import.rs) copy staged files into `attachments/<new-ulid>/<original_filename>` under the store and reference them. docs/02 §2.5 (attachments dir), docs/07 §7.2/§7.4. Follow-up of HS2-14 / HS2-4.

## Notes

<!-- note: 01M0H6M3T24DYJ1R6EM54WJFJH -->
2026-08-20T02:06:39.038Z — **TL;DR:** Attachments now migrate end to end — the exporter stages the files, the importer copies them into `attachments/<new-ulid>/`.

### Exporter (`migrator/src/export.mjs`)
- Reads the `attachments` table, **promoted only** (`draft_id IS NULL`; column-tolerant — the column/table didn't exist on the oldest schemas), emits `{original_filename, stored_path}` per ticket.
- `exportDatadir` **stages** each file beside the export JSON under `attachments/<n>/<name>` and rewrites `stored_path` to that JSON-relative path. Source files resolve **by basename** under `<.hotsheet>/attachments/` (HS1 stored an absolute path), so a project copied to another machine still resolves. Missing files are dropped with a warning.

### Importer (`hotsheet-cli/src/import.rs`)
- Resolves each `stored_path` against the export JSON's directory and copies into the store; `ImportSummary` now reports the attachment count.

### Store (`hotsheet-ticketing`)
- `FsStore::attachment_dir` + `write_attachment`, which reduces the filename to its **basename** (path-traversal guard).

### Tests
- Rust: store traversal-strip + import staged-file→store.
- vitest: `exportFromDb` promoted-vs-draft filtering + `exportDatadir` real staging + `stored_path` rewrite.
- **35 Rust + 11 vitest pass**; fmt + clippy clean. docs/07 updated.

No follow-ups — attachments are covered. (Note: this project's own corpus has 0 attachments, so validation is via synthetic fixtures, matching the exporter/importer path convention.)
