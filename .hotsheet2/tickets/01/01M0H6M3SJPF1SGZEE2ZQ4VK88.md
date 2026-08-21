---
id: 01M0H6M3SJPF1SGZEE2ZQ4VK88
slug: HS2-BYXA4V
title: Decide release / distribution strategy (npm likely wrong; GitHub releases too rough)
category: investigation
priority: low
status: not_started
created_at: 2026-08-19T07:01:55.153Z
updated_at: 2026-08-19T07:01:55.153Z
legacy_number: HS2-72
schema: 1
---

Deferred (maintainer 2026-08-19): figure out a better release/distribution strategy — later, as scaffolding + real artifacts come together. HS2's artifacts are Rust binaries (server + CLI), the Tauri desktop app (macOS/Linux/Windows, with auto-update), the Solid web build, native SwiftUI (macOS/iOS via App Store / TestFlight), later Android, and the bundled Node migrator — so HS1's npm-package distribution likely does NOT make sense, and plain GitHub Releases feels too unpolished. Investigate: how the CLI/server binaries are distributed (Homebrew tap? cargo-dist? signed installers?), Tauri auto-update channel, Apple notarization/App Store vs. direct, versioning across the workspace, and a polished download/update surface. Not scoped now. License = MIT (© Small Tale Inc.).
