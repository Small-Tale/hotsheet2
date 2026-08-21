---
id: 01M0H6M3SJBM5NVQHYG5B1V2T0
slug: HS2-ZBT6ED
title: Set up test tooling + CI per the agreed strategy (docs/12)
category: task
priority: default
status: not_started
created_at: 2026-08-19T00:24:02.457Z
updated_at: 2026-08-19T04:48:24.374Z
legacy_number: HS2-16
schema: 1
---

Wire up the test stack agreed in docs/12-code-organization-and-testing.md §12.7 when the first code lands: cargo-nextest (Rust unit+integration with injected-fake adapters + shared TempStore/TestServer fixtures), proptest (merge driver), cargo-fuzz (parser), insta (merge snapshots), deterministic bare-repo integration tests for the git-native claim (+ opt-in GitHub-live), Playwright (Solid web E2E against a real server), vitest (Node migrator) + the cross-language conformance test (real hotsheet-model round-trips migrator output). Transition-matrix + adversarial tests for stateful modules (claim/lease, index reconcile, terminal-sizing arbiter, sync engine). Coverage = per-language gates (cargo-llvm-cov / Playwright-istanbul / vitest) + aggregate summary (no merged lcov). CI = GitHub Actions, fast tier + full/live tier (GitHub-remote + creds-gated). The CLAUDE.md test-setup marker is filled with this plan; refine it to match reality as code lands. NOTE: AI-tool integration testing gets additional dedicated design (in discussion) — a fake-agent harness + plugin conformance suite; will extend this.
