# Feature Coverage Matrix

This is the durable report for Hot Sheet 2's **double coverage** goal: every shipped
feature should have both focused unit/logic coverage and a realistic E2E user-flow test.
Line/branch reports remain useful floors, but they cannot prove this behavioral pairing.

Update this matrix in the same change that ships, changes, defers, or adds automated
coverage for a feature. Evidence entries are semicolon-separated repository paths; an
optional `# test_name` suffix documents the most relevant test. `scripts/check-test-coverage.mjs`
validates the rows and evidence paths in CI.

Status meanings:

- `double-covered`: both unit and E2E evidence exist.
- `unit-only` / `e2e-only`: one automated layer is missing; this is visible debt.
- `manual`: automation is not currently practical and the manual plan covers it.
- `planned`: the product surface itself is not shipped yet.

<!-- coverage-matrix:begin -->
| ID | Requirement | Feature | Unit evidence | E2E evidence | Manual evidence | Status |
|---|---|---|---|---|---|---|
| storage-format | docs/17-ticket-file-format.md | Ticket Markdown parse/render and forward compatibility | `crates/hotsheet-model/src/format.rs`; `crates/hotsheet-model/tests/proptest_format.rs` | `crates/hotsheet-cli/tests/cli.rs` | — | double-covered |
| filesystem-store | docs/02-ticket-storage.md | File-backed create/read/update/list | `crates/hotsheet-ticketing/src/store.rs`; `crates/hotsheet-ticketing/src/ops.rs` | `crates/hotsheet-cli/tests/cli.rs` | — | double-covered |
| semantic-merge | docs/02-ticket-storage.md | Three-way semantic ticket merge | `crates/hotsheet-ticketing/src/merge.rs`; `crates/hotsheet-ticketing/tests/merge_proptest.rs` | `crates/hotsheet-cli/tests/cli.rs` | — | double-covered |
| claims-leases | docs/05-ai-tool-plugins.md | Claim, renew, release, blockers, and expiry | `crates/hotsheet-ticketing/src/ops.rs` | `crates/hotsheet-cli/tests/cli.rs`; `crates/hotsheet-mcp/src/lib.rs` | — | double-covered |
| assignment | docs/10-assignment-and-collaboration.md | Assignees/reviews/“me” views, server attention events, recipient notifications, GitHub roster seed | `crates/hotsheet-ticketing/src/ops.rs`; `crates/hotsheet-ticketing/src/roster.rs`; `crates/hotsheet-index/src/tests.rs` | `crates/hotsheet-server/tests/http.rs`; `crates/hotsheet-cli/tests/cli.rs` | — | double-covered |
| close-outcomes | docs/02-ticket-storage.md | Structured close/reopen outcomes | `crates/hotsheet-ticketing/src/ops.rs`; `crates/hotsheet-model/src/format.rs` | `crates/hotsheet-cli/tests/cli.rs`; `crates/hotsheet-mcp/src/lib.rs` | — | double-covered |
| sqlite-index | docs/03-indexing-and-query.md | Rebuildable SQLite/FTS index and structured filters | `crates/hotsheet-index/src/tests.rs` | `crates/hotsheet-server/tests/http.rs`; `crates/hotsheet-cli/tests/cli.rs` | — | double-covered |
| filesystem-watch | docs/03-indexing-and-query.md | External ticket changes trigger reindex/events | `crates/hotsheet-server/src/lib.rs` | `crates/hotsheet-server/tests/http.rs` | — | double-covered |
| automatic-sync | docs/02-ticket-storage.md | Fetch/rebase/push synchronization | `crates/hotsheet-ticketing/src/sync.rs` | `crates/hotsheet-cli/tests/cli.rs` | — | double-covered |
| mcp-serverless | docs/05-ai-tool-plugins.md | Serverless MCP ticket lifecycle | `crates/hotsheet-mcp/src/lib.rs` | `crates/hotsheet-mcp/src/lib.rs` | — | double-covered |
| auto-context | docs/05-ai-tool-plugins.md | Category/tag defaults, overrides, suppression, worklist and ticket surfaces | `crates/hotsheet-ticketing/src/auto_context.rs`; `crates/hotsheet-ticketing/src/worklist.rs` | `crates/hotsheet-cli/tests/cli.rs`; `crates/hotsheet-server/tests/http.rs`; `crates/hotsheet-mcp/src/lib.rs` | — | double-covered |
| secure-keys | docs/04-core-server-cli.md | OS credential-store provider registry and secret-reference resolution | `crates/hotsheet-ticketing/src/secrets.rs` | `crates/hotsheet-cli/tests/cli.rs` | `docs/manual-test-plan.md` | double-covered |
| checkout-discovery | docs/04-core-server-cli.md | path-derived ids, many-to-many registry, resolution, checkout-qualified ticket CRUD/MCP targeting | `crates/hotsheet-ticketing/src/checkouts.rs`; `crates/hotsheet-mcp/src/lib.rs` | `crates/hotsheet-cli/tests/cli.rs`; `crates/hotsheet-server/tests/http.rs` | `docs/manual-test-plan.md` | double-covered |
| repository-status | docs/04-core-server-cli.md | porcelain-v2 parsing and checkout-scoped real-git snapshots | `crates/hotsheet-ticketing/src/repository_status.rs` | `crates/hotsheet-server/tests/http.rs` | — | double-covered |
| ticket-flow-api | docs/04-core-server-cli.md | current status/category, throughput, cycle time, usage summary | `crates/hotsheet-ticketing/src/analytics.rs`; `crates/hotsheet-ticketing/src/metrics.rs` | `crates/hotsheet-server/tests/http.rs` | — | double-covered |
| configured-commands | docs/04-core-server-cli.md | typed argv schema, configured-only execution, streaming cursor, cancel/history | `crates/hotsheet-ticketing/src/commands.rs`; `crates/hotsheet-server/src/commands.rs` | `crates/hotsheet-server/tests/http.rs` | — | double-covered |
| notification-routing | docs/04-core-server-cli.md | targets, dedupe, acknowledgement, live event | `crates/hotsheet-server/src/notifications.rs` | `crates/hotsheet-server/tests/http.rs` | — | double-covered |
| server-tts-boundary | docs/04-core-server-cli.md | injectable provider, input limits, binary audio response, secret-free request | `crates/hotsheet-server/src/tts.rs` | `crates/hotsheet-server/tests/http.rs` | — | double-covered |
| plugin-registry | docs/05-ai-tool-plugins.md | Built-in/external plugin loading and setup | `crates/hotsheet-plugins/src/tests.rs`; `crates/hotsheet-plugins/tests/no_tool_id_branches.rs` | `crates/hotsheet-cli/tests/plugin_conformance.rs` | — | double-covered |
| drive-transports | docs/13-drive-transport-interface.md | Spawn, channel, app-server, and live ACP stdio drive/session abstractions | `crates/hotsheet-aitools/src/tests.rs`; `crates/hotsheet-aitools/src/acp.rs` | `crates/hotsheet-aitools/tests/protocol_cassettes.rs` | `docs/manual-test-plan.md` | double-covered |
| usage-metrics | docs/14-metrics-interface.md | Usage mapping, storage, pricing, and rollups | `crates/hotsheet-ticketing/src/metrics.rs`; `crates/hotsheet-aitools/src/acp.rs` | `crates/hotsheet-aitools/tests/protocol_cassettes.rs` | — | double-covered |
| activity-stream | docs/15-activity-narration-interface.md | Attributed activity event storage and delivery | `crates/hotsheet-server/src/dist_work_loop.rs` | `crates/hotsheet-server/tests/http.rs` | — | double-covered |
| terminal-hosting | docs/06-clients.md | PTY lifecycle, stream, input, and busy inference | `crates/hotsheet-terminals/src/lib.rs` | `crates/hotsheet-terminals/tests/fake_agent.rs`; `crates/hotsheet-server/tests/terminal_ws.rs` | — | double-covered |
| terminal-sizing | docs/06-clients.md | Multi-viewer sizing arbitration | `crates/hotsheet-terminals/src/sizing.rs` | `crates/hotsheet-server/tests/terminal_ws.rs` | `docs/manual-test-plan.md` | double-covered |
| server-multistore | docs/04-core-server-cli.md | Hosted-store discovery, routing, and isolation | `crates/hotsheet-server/src/multistore.rs` | `crates/hotsheet-server/tests/http.rs` | — | double-covered |
| mtls | docs/04-core-server-cli.md | Device certificates, ACLs, renewal, and revocation | `crates/hotsheet-tls/src/lib.rs` | `crates/hotsheet-server/tests/mtls.rs` | `docs/manual-test-plan.md` | double-covered |
| hs1-migration | docs/07-migration.md | HS1 export, deterministic/idempotent HS2 identity, normalized close state, retired legacy fields, and import conformance | `crates/hotsheet-cli/src/import.rs`; `crates/hotsheet-model/src/format.rs`; `migrator/test/export.test.mjs` | `crates/hotsheet-cli/tests/migrate.rs`; `crates/hotsheet-cli/tests/cli.rs` | — | double-covered |
| ticket-provider-foundation | docs/16-external-sync-interface.md | Provider identity/capabilities/config/aggregation and default git adapter across CLI/server/MCP | `crates/hotsheet-ticketing/src/provider.rs` | `crates/hotsheet-cli/tests/cli.rs`; `crates/hotsheet-server/tests/http.rs`; `crates/hotsheet-mcp/src/lib.rs` | — | double-covered |
| provider-transfer | docs/16-external-sync-interface.md | Idempotent copy/move, concurrent retries, field preservation/rejection, identity collision, and destination-created/source-close-failed recovery | `crates/hotsheet-ticketing/src/provider.rs` | `crates/hotsheet-cli/tests/cli.rs`; `crates/hotsheet-server/tests/http.rs`; `crates/hotsheet-mcp/src/lib.rs` | — | double-covered |
| github-ticket-provider | docs/16-external-sync-interface.md | Direct authoritative GitHub Issues mapping, pagination/incremental reads, auth/rate-limit/concurrency, webhook invalidation, and no-mirror CRUD | `crates/hotsheet-extsync/src/github.rs` | `crates/hotsheet-cli/tests/cli.rs`; `crates/hotsheet-server/tests/http.rs`; `crates/hotsheet-mcp/src/lib.rs` | `crates/hotsheet-extsync/src/github.rs # github_live_crud_against_dedicated_test_repository` | double-covered |
| gitlab-jira-ticket-providers | docs/16-external-sync-interface.md | Direct authoritative GitLab/Jira providers | — | — | — | planned |
| client-ui-stack | docs/06-clients.md | Kerf morph/custom-element integration, Web Awesome events/focus/theme, and offline bundle | — | `spikes/kerf-webawesome/tests/integration.spec.ts` | — | e2e-only |
| web-client | docs/06-clients.md | Browser client workflows | — | — | — | planned |
| native-clients | docs/06-clients.md | macOS/iOS client workflows | — | — | `docs/manual-test-plan.md` | planned |
| github-roster-seed | docs/10-assignment-and-collaboration.md | Seed people roster from GitHub collaborators | — | — | — | planned |
<!-- coverage-matrix:end -->

## Coverage report layers

- Rust line coverage: `cargo llvm-cov`; CI uploads `rust-coverage-lcov` and enforces
  the current line floor.
- Migrator line/branch/function coverage: `npm run test:coverage` in `migrator/`; CI
  uploads `migrator-coverage-lcov` and enforces Vitest thresholds.
- Feature double coverage: this matrix, gated by `npm run`-free
  `node scripts/check-test-coverage.mjs` in CI.
- Manual-only behavior: [manual-test-plan.md](manual-test-plan.md).

The matrix reports whether both behavioral layers exist; it does not claim every
requirement is fully asserted merely because a file is listed. Reviews should still
inspect the named tests and add transition/adversarial cases for stateful changes.
