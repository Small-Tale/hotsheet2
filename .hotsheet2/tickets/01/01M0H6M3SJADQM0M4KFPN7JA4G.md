---
id: 01M0H6M3SJADQM0M4KFPN7JA4G
slug: HS2-CCJ2BP
title: 'Parser hardening: proptest round-trip + cargo-fuzz target for the ticket file format'
category: task
priority: default
status: completed
created_at: 2026-08-19T11:16:12.050Z
updated_at: 2026-08-20T01:59:03.975Z
completed_at: 2026-08-20T01:59:03.975Z
closed_at: 2026-08-20T01:59:03.975Z
close_reason: completed
legacy_number: HS2-79
schema: 1
---

hotsheet-model::format claims round-trip stability and degrade-not-panic (docs/17 §17.4, docs/12 §12.7.2) and has example-based tests, but no property/fuzz coverage yet. Add: (1) a proptest generating arbitrary Tickets and asserting parse(to_file_string(t)) == t and byte-idempotent re-serialize; (2) a cargo-fuzz target feeding arbitrary bytes to parse_file, asserting it never panics. Also consider an insta snapshot of a canonical file. Pin any discovered bug as a regression test. Follow-up of HS2-3.

## Notes

<!-- note: 01M0H6M3T2SDYNT4JPWC4HBWAB -->
2026-08-20T01:59:03.975Z — **TL;DR:** Added `proptest` round-trip/no-panic properties + a `cargo-fuzz` target for the ticket parser. Proptest found and fixed a real edge (empty-text notes didn't round-trip).

### Property tests (`crates/hotsheet-model/tests/proptest_format.rs`)
Generates arbitrary `Ticket`s covering every field (inside the format's canonical domain — valid RFC3339, body without a `## Notes` line, note text/markers that don't collide with the syntax, no local-only `feedback_draft`), asserting:
- **semantic round-trip** — `parse_file(to_file_string(t)) == t`
- **byte-idempotent** re-serialize
- **never panics** — 3 properties over arbitrary text, arbitrary bytes (lossy UTF-8), and frontmatter-framed input.

Stable across repeated 512-case runs.

### Bug found + fixed
Proptest surfaced that a note with **empty text** doesn't round-trip: its `<timestamp> — ` re-parses as the text. Fixed by **omitting content-less notes on serialize** (same treatment as `feedback_draft`), pinned by `format::tests::empty_text_notes_are_omitted`.

### Fuzz target (`crates/hotsheet-model/fuzz`)
A `cargo-fuzz` `parse_file` target (its own workspace so stable `nextest` ignores it; nightly: `cargo +nightly fuzz run parse_file`) for deeper coverage-guided no-panic fuzzing. The same invariant runs offline via the proptests.

**33 tests pass; fmt + clippy clean.** CLAUDE.md test-setup note updated. No follow-ups.
