---
id: 01M0H6M3SJJRJK72EK8DX2M9NF
slug: HS2-EXS4ZX
title: 'DECISION: confirm core implementation language (Rust vs Go)'
category: feature
priority: high
status: completed
created_at: 2026-08-19T00:22:35.752Z
updated_at: 2026-08-19T00:55:26.298Z
completed_at: 2026-08-19T00:55:26.298Z
closed_at: 2026-08-19T00:55:26.298Z
close_reason: completed
legacy_number: HS2-2
schema: 1
---

The pivotal, expensive-to-reverse decision that gates all implementation. Recommendation (docs/09 §9.2): a shared Rust core library (hotsheet-core) with thin Rust binaries for server (axum/tokio) and CLI (clap); clients embed the same core (Tauri links the crate; SwiftUI via uniffi bindings; Android later via uniffi/JNI).

Decisive argument: the mandated native SwiftUI macOS/iOS client. Rust lets ONE tested engine run in the server, CLI, Tauri process, and native apps. Go's mobile-embedding story (gomobile) is far clunkier, forcing native clients into pure remote-API mode and re-opening the logic-duplicated-per-surface problem the rewrite exists to close.

Cost stated honestly: Rust iterates slower than Go, and this app has a large glue-feature surface where Go's simplicity helps. Go is the documented fallback if the maintainer weights iteration speed over native-client core-sharing.

ACTION: maintainer confirms Rust (recommended) or Go before implementation begins. See docs/09-technology-decisions.md §9.2 and §9.11 (L1).

## Notes

<!-- note: 01M0H6M3SKVKKE0GP229QBDMA0 -->
2026-08-19T00:55:26.298Z — **RESOLVED (maintainer, 2026-08-19): Rust.**

A shared **Rust** core library (`hotsheet-core`) with thin Rust binaries for server (axum/tokio) and CLI (clap); clients embed the same core (Tauri links the crate; SwiftUI via uniffi; Android later via uniffi/JNI). Go is the documented fallback of record only.

docs/09-technology-decisions.md §9.2 updated to **Decided**. All downstream implementation tickets (HS2-3…HS2-14, HS2-18) proceed on Rust.
