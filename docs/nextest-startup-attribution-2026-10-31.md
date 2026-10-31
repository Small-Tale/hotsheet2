# Nextest startup process-query attribution — 2026-10-31

HS2-WJYRE6 narrowed one observed process-query delay to the wait after process creation. It did not establish an OS, load or discovery-concurrency cause. Ten direct diagnostic checks passed; the single default LIST observation stopped without a complete inventory. Neither result supplies release-gate receipt authority.

All observation times below are UTC on 2026-10-30. The report date is the following local day. The later serialized and complete-list observations remain in the separate [Nextest discovery report](nextest-discovery-2026-10-31.md); the earlier [Rust test performance profile](26-rust-gate-performance-2026-10-28.md) measures a different interval and is unchanged.

## Discovery and execution are different phases

Installed cargo-nextest reported version 0.9.114 and commit `ff4ee9ac9e7feb312e2ba6ea2d5eec40142a2ce0`. That commit could not be resolved upstream. The published same-version tag resolves to `9daab1c50cd0389236508435f71434f01aa2eca1`; it is qualified source evidence, not exact provenance for the installed binary.

In that published source, [CLI list construction](https://github.com/nextest-rs/nextest/blob/cargo-nextest-0.9.114/cargo-nextest/src/dispatch/cli.rs) passes the available CPU count into test listing, while the test-thread setting separately configures the execution runner. [Test listing](https://github.com/nextest-rs/nextest/blob/cargo-nextest-0.9.114/nextest-runner/src/list/test_list.rs) buffers concurrent binary queries and queries each binary with and without ignored tests. Installed help exposed no public listing-concurrency cap. `NEXTEST_TEST_THREADS=1` therefore was not evidence of serialized discovery; actual concurrency was not measured.

List construction precedes runner/reporter initialization in the inspected source. A missing run header is compatible with discovery or other pre-run initialization, but does not prove discovery executed or caused a process-query timeout. Built-in debug starts have no child completion/PID timing; passive read timestamps cannot establish child emission times or simultaneous occupancy.

## Retained gate attempts

Three retained HS2-9JJZ4J attempts reached Cargo compilation completion without a Nextest run header. Their unchanged process-query deadline was two seconds.

| Attempt    | Cargo compilation reported | Observed process query           | Outer outcome                                         |
| ---------- | -------------------------: | -------------------------------- | ----------------------------------------------------- |
| `2e8cc7ef` |              23.84 seconds | No output after 2.004830 seconds | Exit 125 at 20:16:12.319421                           |
| `d2e4ac67` |               0.34 seconds | No output after 2.003105 seconds | Birth-timeout stop; exit 125 at 21:51:51.175605       |
| `0a900e7e` |               0.56 seconds | No output after 2.003097 seconds | Cached-proof-expiry stop; exit 125 at 22:15:07.133307 |

The last attempt's initiating event was cached-proof expiry at 22:15:05.610250, while its next query was still unknown. Its eventual timeout/cancellation is a later observation, not a reason to relabel the initiating stop. Independent memory queries succeeded in 2.264 and 2.404 ms during that interval; those results do not restore unknown process identity.

The earlier successful foundation gate ran 18:49:05.841922–19:03:35.043034, with 2,483 tests passing. Its Cargo output has no per-line timestamps, so discovery duration cannot be recovered from it. The elapsed stage time includes more than compilation and reported test execution. Neither that success nor the aborted attempts establishes a Python-version, algorithm, kernel or load explanation.

## Diagnostic verification and the bounded LIST

The direct-only microcohort ran 23:04:28.713211–23:04:30.718062 under one enclosing supervisor. Its ten checks passed in 0.119 seconds: four direct probe-owner methods and six pure filter/scope cases. The outer and child results were zero, `stopped` was null, the owned anchor was reaped, and the final process proof was known/reaped in 14.739 ms. This is diagnostic verification only.

The complete 17-case milestone fixture suite and 30-case baseline were not run in that cohort. They include independent synthetic supervisors/groups and were excluded pending separate registered guardian/control-FD EOF and acknowledgement ownership review. A ten-case pass does not validate those broader suites or universal descendant containment.

The single default `cargo nextest list --all-features --message-format json` observation ran 23:07:52.399295–23:08:15.961562. It had a 30-second diagnostic budget but stopped earlier on process-proof expiry. The passive driver observed Cargo finish and 27 unique binary listing starts, with zero stdout bytes and no final list JSON. Starts are neither a completed inventory nor measured concurrency.

The delayed query entered its owner thread 0.252 ms after request. `Popen` returned after 4.617 ms; communicate began after 4.660 ms. It produced no output before cancellation. At 23:08:14.441604 the preceding proof reached the unchanged 12-second age limit while the new query had been pending for 1.991491 seconds. The initiating reason was `sampler_birth_cached_proof_expired`.

The eventual query result was unknown after 2.003231 seconds, cancelled/timed out, killed with signal 9 and reaped, with zero output. These milestones locate the observed delay after fast process creation, during the output/result wait. They do not identify the reason for that wait. Diagnostic bookkeeping itself has overhead and is not a real-time guarantee.

The owner sent INT, TERM and KILL to its pinned group. Anchor reaping was recorded at 23:08:15.961179; the distinct final supervisor event was at 23:08:15.961562, outer 125, child 125, no receipt authority. This establishes the recorded owned-group cleanup, not absence of every possible unknown or escaped descendant.

Independent memory queries succeeded in 2,067.621 and 1,114.813 ms against their unchanged 5,000 ms timeout. The two interval samples were separated by 12.070 seconds. These are distinct, sampled observations, not proof of the process-query cause or continuous coverage. No diagnostic changed the two-second process deadline, ten-second cadence, twelve-second cached bound, 12/5 GiB admission/runtime floors, 20% memory threshold, 30-second freshness bound or cleanup graces.

## Evidence and remaining ownership

Ticket attachments retain the configs, events, filtered logs, exact source manifests and ownership plan. The default LIST config SHA256 is `a91742f91005eeea7d8fe5eefde5445ded53ca2363b12b763f8a87f535ab58a7`; its event log is `3f015f00abc78ffbb53cd42685b88824a47c342182ef014a30e49a7e9d226fcf`. The direct microcohort config is `13392f38150eca03153d92164e58c248bd33dee63e0a5338fb9ee7a0cd44db8d`; its event log is `883ec4846e4697470d18cd855de25f41e6b6a75b99853d015d87d3286706791f`. The reviewed private source manifest is `d7efdca609f0329b60f540eeaab4ade2f9a3bebcb0218a79967b1ede08f8e60d`. Raw diagnostic output was filtered before persistence; no customer content was copied.

The existing typed follow-up chain is HS2-WJYRE6 → HS2-2N8324 → HS2-53E5F2. The latter reports record the incomplete serialized comparison and separately completed inventory; neither retroactively proves concurrency causality for this stop. Other existing owners remain distinct:

| Owner                   | Scope outside this bounded attribution result                                                  |
| ----------------------- | ---------------------------------------------------------------------------------------------- |
| HS2-9JJZ4J              | Durable supervision integration, affected fixture verification and fresh full gate/covering CI |
| HS2-F6F8MA              | Parent-death, escaped-group, unknown-descendant and native-I/O containment limits              |
| HS2-YK109D              | Optional discovery-cap gate adaptation and its real composition/descriptor tests               |
| HS2-XW38CB              | Effective worker-control receipt fingerprint coverage                                          |
| HS2-PC2GJ4 / HS2-RMK8B8 | Independent observations and memory-query diagnostics                                          |

This report answers the bounded phase-attribution question and preserves the remaining unknowns. It does not complete those owners, authorize a retry/rearm, promote aborted progress, or supply a passing gate or containment proof. No new runtime was performed to write the report.
