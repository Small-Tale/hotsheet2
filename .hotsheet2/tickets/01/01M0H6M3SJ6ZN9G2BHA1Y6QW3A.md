---
id: 01M0H6M3SJ6ZN9G2BHA1Y6QW3A
slug: HS2-T9VWP5
title: 'hotsheet-model: typed timestamp (replace the String Timestamp alias)'
category: task
priority: default
status: completed
created_at: 2026-08-19T11:16:20.826Z
updated_at: 2026-08-20T01:50:14.503Z
completed_at: 2026-08-20T01:50:14.503Z
closed_at: 2026-08-20T01:50:14.503Z
close_reason: completed
legacy_number: HS2-81
schema: 1
---

Timestamps are currently `pub type Timestamp = String` (RFC3339 text) throughout the model to avoid baking a time-crate choice into the scaffold. Introduce a typed timestamp (e.g. a wrapper over `time::OffsetDateTime` or `jiff`) with a serde impl that (de)serializes to RFC3339, validate on parse (degrade-not-panic per docs/17 §17.4), and thread it through Ticket/Note/etc. Keeps `updated_at` last-writer-wins comparisons correct (docs/02 §2.7). Follow-up of HS2-3.

## Notes

<!-- note: 01M0H6M3T299N0P486GXFXJJA4 -->
2026-08-20T01:50:14.503Z — **TL;DR:** Replaced `pub type Timestamp = String` with a typed **lenient RFC3339** `Timestamp` that preserves exact on-disk text (round-trip stable) while exposing a parsed instant for chronological comparison — degrading rather than panicking on invalid text.

### Design (`hotsheet-model/src/timestamp.rs`)
`Timestamp { raw: String, parsed: Option<OffsetDateTime> }`:
- **Serde** (de)serializes as a plain string, so the file format is unchanged.
- **Invalid text degrades** to `instant() == None`, raw preserved verbatim (degrade-not-panic, docs/17 §17.4) — no parse can fail.
- **Equality is textual** (round-trip identity); `chronological_cmp` / `is_after` use the parsed instant for last-writer-wins merges (docs/02 §2.7).
- Backed by the `time` crate (`parsing` + `formatting`).

### Threaded through
- `Ticket`/`Note` fields are now `Timestamp` / `Option<Timestamp>`; `Ticket::new` takes `impl Into<Timestamp>`.
- `format.rs` note (de)serialization + `hotsheet-cli/src/import.rs` (`Option<String>` → `Option<Timestamp>`) updated.

### Verified
- 5 new unit tests (valid/invalid/fractional/offset/order/textual-equality).
- **27 tests pass**; fmt + clippy clean.
- Real snapshot (82 tickets) round-trips with millisecond timestamps preserved *and* validated.

No follow-ups — scope complete.
