# Local test dependency baseline (2026-11-02)

HS2-D461XW maps the present verification edges and proposes
[`local-verification-dependencies.proposed.yaml`](local-verification-dependencies.proposed.yaml).
The YAML is a review artifact; `scripts/check.sh` does not read it. The source tree
is commit `7d4f6bc7e162bb2eaca0284c29221390001211ed`. HS2-3ZJBRM,
HS2-55MWQ9, and HS2-K5E2KF may use this map to design an enforceable selection;
HS2-DAA8AB owns quiet-host selected-versus-full validation before any gate-policy
change.

## What actually depends on what

The direct regular dependencies of all fourteen Rust packages in the YAML come from `cargo
metadata --no-deps --format-version 1`, checked against the workspace
[`Cargo.toml`](../Cargo.toml) and crate manifests. Reverse traversal is required:
`hotsheet-model` reaches ticketing, index, extsync, plugins, CLI, MCP, and server;
`hotsheet-cli` reaches MCP and server. Test and build dependencies add edges beyond
regular Cargo dependencies. A changed private function is a narrower _candidate_
only when its exported contract and every caller remain unchanged; a package
boundary alone does not prove that.

| Input and source proof                                                                                                                                         | Consumer or verification edge                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Server [Axum routes](../crates/hotsheet-server/src/lib.rs)                                                                                                     | [OpenAPI route sync](../clients/web/scripts/sync-openapi-routes.mjs) reads the route inventory; [contract browser tests](../clients/web/tests/contracts.spec.ts) compare routes, compile schemas, and exercise real responses. A router edit therefore carries the generated [OpenAPI contract](contracts/v1/openapi.yaml).                                                                                                                                                                                                |
| Rust CLI and server binaries                                                                                                                                   | [Real browser fixture](../clients/web/tests/real-ticket-server.ts) starts both; the [migrator tests](../migrator/test/export.test.mjs) and [progress tests](../migrator/test/progress.test.mjs) also spawn built Rust binaries. A pure Rust source edit can therefore affect web and migrator flows even when their TypeScript did not change.                                                                                                                                                                             |
| [Migrator exporter](../migrator/src/export.mjs)                                                                                                                | [CLI runtime lookup](../crates/hotsheet-cli/src/lib.rs), [desktop bridge](../clients/web/src/project-bridge.ts), [desktop packaging](../clients/web/scripts/prepare-desktop.mjs), and migrator tests read or invoke it.                                                                                                                                                                                                                                                                                                    |
| Web [trailing task](../clients/web/src/trailing-task.ts)                                                                                                       | Its [direct unit file](../clients/web/src/trailing-task.test.ts) tests schedule/cancel; [app runtime](../clients/web/src/app/runtime.tsx) uses it for ticket collection refresh; the shipped [provider browser flow](../clients/web/tests/providers.spec.ts) checks a deferred refresh with an open metadata popup.                                                                                                                                                                                                        |
| [Public favicon](../clients/web/public/favicon-32.png), [shared guidance](../plugins/shared/ticket-driven-work.md), [CI workflow](../.github/workflows/ci.yml) | Rust [HTTP tests](../crates/hotsheet-server/tests/http.rs), [CLI tests](../crates/hotsheet-cli/tests/cli.rs), and [claim tests](../crates/hotsheet-cli/tests/claim_run.rs) embed these with `include_bytes!` or `include_str!`. Path prefixes alone would miss those reverse edges.                                                                                                                                                                                                                                        |
| Repository-wide sources                                                                                                                                        | [Format script](../clients/web/scripts/format-sources.mjs) covers web, migrator, spike, docs and CI; [coverage matrix checker](../scripts/check-test-coverage.mjs), [quarantine checker](../scripts/check-test-quarantine.mjs), [instruction policy](../scripts/instruction-policy.test.mjs), and [role-skill mirror check](../scripts/sync-role-skills.mjs) read across packages. The [full stage catalog](../scripts/check-full.sh) and [current path rules](../scripts/gate-plan.mjs) keep these floor checks selected. |

The present [path rules](../scripts/gate-plan.mjs) conservatively select Rust,
web, and migrator groups for every `crates/` path; `migrator/` selects those
three groups, `clients/web/public/` selects Rust and web, and unknown/gate/
configuration paths force the full package gate. The [affected verification
guide](29-affected-local-verification.md) records the exact-tree receipt and CI
boundary. A shared test fixture such as `tests/providers.spec.ts` also affects
many tests within its file; the current browser planner can select all 549 cases.

## Observed latency, not a speedup claim

The table uses the local `scripts/check.sh --timings` history in the Git common
directory on 2026-11-02. Its estimate is the nearest-rank p75 of complete
successful stage executions. These are stage wall times on a shared Mac and are
not additive across parallel CI jobs. All listed package and repository-wide
stage estimates have 20 local samples; the exact three-file browser command has
seven. Receipt hits add no sample.

| Stage                                                    |                   Local p75 | Samples |
| -------------------------------------------------------- | --------------------------: | ------: |
| `cargo fmt --all --check`                                |                     2.958 s |      20 |
| `cargo lint`                                             |                    46.792 s |      20 |
| All-features Rust Nextest                                |                   218.709 s |      20 |
| Root policy/gate Node tests                              |                    74.667 s |      20 |
| Coverage matrix, quarantine, role mirror checks together |                     3.691 s | 20 each |
| Web lint, including format and UI doctor                 |                   175.185 s |      20 |
| Web typecheck, unit suite, build                         | 12.994 s, 55.588 s, 5.748 s | 20 each |
| Migrator lint and coverage                               |           1.700 s, 13.605 s | 20 each |
| Spike lint, typecheck, build                             |   4.234 s, 3.878 s, 2.275 s | 20 each |
| Contract/provider/shell real-server browser command      |                 1,684.112 s |       7 |

The immediately preceding clean HS2-6H8ZY6 exact-tree `scripts/check.sh --full`
invocation passed in **2,164.409 s (36.1 min)**. Its browser stage took
**1,779.835 s (29.7 min, 82% of that gate wall time)**; web lint took 201.809 s
and root policy tests took 83.477 s. Unchanged Rust stage receipts were reused
after an earlier browser teardown failure. Thus this is a measured local
commit-to-gate-green _command span_ with warm receipts, not a cold full-gate
duration or an estimated saving from a new selection. The initial failed attempt
took 2,285.466 s and issued no receipt. The test teardown was fixed, then the
entire selected run passed. Its covering main CI run was
[41380269338](https://github.com/Small-Tale/hotsheet2/actions/runs/41380269338).

Two reversible, semantics-preserving private edits in this ticket's isolated
worktree measured focused edit-to-green latency. Both source files were restored
byte-for-byte before this report was written.

| Probe                                                   |                  Clean checkout observation | Limit                                                                                                   |
| ------------------------------------------------------- | ------------------------------------------: | ------------------------------------------------------------------------------------------------------- |
| Web `trailing-task.ts` direct Vitest file, before edit  | 1.91 s first, 1.51 s warm; two tests passed | Node dependencies installed locally in 4 s beforehand.                                                  |
| Web same file after private timer-initializer edit      |                    1.75 s; two tests passed | Focused unit result only.                                                                               |
| Web shipped deferred-refresh browser case               |                    10.14 s; one case passed | Real CLI/server fixture build took 59.18 s first; no browser outcome was inferred from the unit result. |
| Rust `broken_pipe.rs` direct unit slice                 |               0.81 s warm; two tests passed | Isolated Cargo target.                                                                                  |
| Rust same unit slice after private string-constant edit |                    8.53 s; two tests passed | Includes rebuilding the CLI test target.                                                                |
| Rust CLI broken-pipe integration binary                 |                     5.87 s; one test passed | First invocation compiled that test target for 3.39 s.                                                  |

The Rust cold build is **unmeasured**: its first native AVIF build failed under
the sandbox after 20.20 s, and an escalated resume then passed in 38.09 s.
Combining those numbers would fabricate a cold baseline. These small samples
show command scale, not a host-normalized throughput gain. Worktrees must keep
separate Cargo targets, as [§12.2](12-code-organization-and-testing.md) requires;
HS2-DAA8AB discarded earlier matched-run evidence that shared a target.

## Pilot boundary and safe fallback

The YAML proposes a Rust pilot for private `broken_pipe.rs` parsing changes and
a web pilot for private `trailing-task.ts` scheduling changes. Each pilot names
its direct unit and shipped-flow checks and the packages or module to compile.
The Rust CLI integration test is mandatory for the broken-pipe path; a web
composition case is mandatory for deferred collection refresh. Shared floor
checks still run. Neither pilot's focused green is a gate receipt today.

Unknown paths, changed manifests or toolchains, public signatures, wire/router
contracts, generated artifacts, shared fixtures, migration/auth/sync/index
surfaces, missing test links, and any ambiguous or failed source read must fall
back to the existing full affected selection. A path or stage that cannot be
classified must never produce an empty green plan. Preserve exact committed-tree
receipts and full browser/platform/coverage CI. HS2-3ZJBRM and HS2-55MWQ9 must
implement and validate those edges, while HS2-K5E2KF can pin the pilot module
boundary. HS2-DAA8AB must independently compare selected and full outcomes on
a quiet host with separate build targets before this proposal can affect the
required gate. No customer content was copied.
