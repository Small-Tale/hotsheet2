# Supervised Nextest discovery — 2026-10-31

HS2-2N8324 compared one bounded serialized discovery observation with the retained HS2-WJYRE6 default observation. The serialized observation reached its 30-second diagnostic budget without losing process identity proof, but did not produce a complete test list. This is an incomplete diagnostic result, not a gate fix or evidence that concurrency caused the earlier delay. Product and gate defaults are unchanged.

All observation times below are UTC on 2026-10-30. The report date is the following local day.

The preceding [startup process-query attribution](nextest-startup-attribution-2026-10-31.md) (HS2-WJYRE6) records the retained gate-stop comparison, ten-check diagnostic selection and post-spawn wait evidence, including the unrun fixture suites and unresolved cause.

## Observations

| Observation             | Default LIST (WJYRE6)                                     | Serialized LIST (2N8324)                                                        |
| ----------------------- | --------------------------------------------------------- | ------------------------------------------------------------------------------- |
| Closed interval         | 23:07:52.399295–23:08:15.961562                           | 23:49:13.048910–23:49:44.874223                                                 |
| Driver scope            | `cargo nextest list --all-features --message-format json` | Same command and feature scope                                                  |
| Driver budget           | 30 seconds                                                | 30 seconds                                                                      |
| Listing starts observed | 27 unique binaries                                        | 40 unique binaries                                                              |
| Complete list JSON      | None                                                      | None                                                                            |
| Periodic process proof  | First query 642.780 ms; next query remained unknown       | Three queries: 4.840, 4.556 and 4.543 ms, all known and reaped                  |
| Outcome                 | Cached process proof expired; outer exit 125              | Driver budget expired after 30.006935 seconds; outer exit 125, no resource stop |

Both drivers observed Cargo finish. Start events do not establish concurrent occupancy, completed binaries or total inventory. Neither run produced an inventory checksum. The serialized run's driver captured zero stdout bytes and 5,532 stderr bytes, persisted only through its scalar allowlist.

In the default observation, the delayed query's owner thread entered after 0.252 ms and `Popen` returned after 4.617 ms. The query then produced no output. The initiating stop occurred while it was pending for 1.991491 seconds, when the previous proof reached its unchanged 12-second age bound. Its final cancelled, killed and reaped result was unknown after 2.003231 seconds. The observed delay therefore lies after process creation, while waiting for the query; this does not identify an OS or load cause.

Independent memory queries in that interval succeeded in 2,067.621 and 1,114.813 ms against their unchanged 5,000 ms timeout. In the serialized interval, three independent rows reported about 19.754 GB free disk and 61% free memory; memory queries succeeded in 2.520, 2.672 and 2.985 ms. These are different observation classes and do not substitute for process identity proof. Time, cache state, interpreter overhead and OS scheduling were not controlled between the two observations.

## What serialization changed

A private Cargo target runner opened one pinned, existing advisory-lock inode and held its inheritable file descriptor across `execve` into a listing binary. The helper admitted only the exact LIST phase and arguments, with a regular executable directly inside the approved existing target directory. It preserved the binary's PID, process group, output and native exit status. There was no wrapper parent or second signal controller.

This mechanism intends to serialize cooperating binary execution. Nextest scheduling, waiting runners and process creation remain concurrent. A native binary closing inherited descriptors could defeat that assumption; a descendant retaining the lock descriptor could delay release. Actual native descriptor retention was not measured.

Six direct synthetic tests passed in 2.345 seconds before the real observation. They covered lock inheritance and exclusion, both LIST argument variants, exit 7 and native termination, acknowledged lock-wait cancellation, holder and unrelated-child survival, control-pipe EOF, malformed admission and later recovery. The cancellation test acknowledged arrival immediately before the real lock call, after default signal dispositions were established. Pure checks covered loader-variable restoration and the driver's schema/filter/launch contract. They did not execute Nextest or prove native Rust loader behavior.

## Safeguards and limits

Admission required 12 GiB free disk; the runtime floor remained 5 GiB, with 20% memory and 30-second observation freshness. Process queries retained their request-based two-second deadline, ten-second cadence and twelve-second cached-proof bound. Normal owned-group INT, TERM and KILL cleanup remained unchanged. Both real observations completed that cleanup; the serialized observation also ended with a known, reaped final query in 4.817 ms. No diagnostic supplied release-gate receipt authority.

The short synthetic interval contained no independent memory row; its healthy bracketing rows were 10.002 seconds apart. It is not continuous in-window coverage. Hard native I/O stalls, parent crashes, escaped process groups and unknown descendants remain HS2-F6F8MA limitations. HS2-PC2GJ4 owns the independent observations; HS2-RMK8B8 owns memory-query diagnostics, and HS2-XW38CB owns worker-control fingerprint coverage.

Installed cargo-nextest reported version 0.9.114 and commit `ff4ee9ac9e7feb312e2ba6ea2d5eec40142a2ce0`, which could not be resolved upstream. Published same-version source at `9daab1c50cd0389236508435f71434f01aa2eca1` supports the runner mechanism and separates discovery scheduling from test execution threads. It is qualified source evidence, not proof of the installed binary's exact source. See the upstream [target runner](https://github.com/nextest-rs/nextest/blob/cargo-nextest-0.9.114/nextest-runner/src/target_runner.rs) and [test listing](https://github.com/nextest-rs/nextest/blob/cargo-nextest-0.9.114/nextest-runner/src/list/test_list.rs) implementations.

## Evidence and next step

Ticket attachments retain the exact configs, events, filtered driver logs and source manifests. The closed serialized evidence manifest is `7a7561fbf2ff77a4cbe502e6c85887093216bcd8399593d914303975700bc60d`; its config is `51783a54d501c23e3e66ea7b4f44598cbf268b0b980f426f3c0c7c4fe5fc8c44`. The tested runner is `7ae19dc407010a05166f59d15b6a023cbc12dd0cb489f0985e61b964f9bf72f0`. No customer content was copied.

HS2-53E5F2 owns a separately reviewed, finite complete-list observation and a gate-cap recommendation based on its outcome. The 30-second observation was not extended or repeated. A complete result must include the expected nonempty scope, consistent full JSON, successful child and final driver statuses, successful outer cleanup and fresh final identity proof. A partial result or healthy memory row cannot grant completion credit.

The current helper is deliberately LIST-only and would reject a test execution phase. It cannot be inserted unchanged into `cargo nextest run`. Any gate adaptation requires its own implementation and verification. HS2-9JJZ4J still needs a fresh successful full local gate and successful covering main CI; these observations replace neither.

## Complete follow-up observation (HS2-53E5F2)

A separately reviewed and freshly admitted 300-second opportunity completed one serialized LIST on 2026-10-31. Its closed UTC interval was 00:31:12.333293–00:32:18.591330; the driver ran for 64.417289 seconds. The earlier 30-second observation was already closed and was not extended. No resource deadline, admission floor, freshness bound or cleanup grace changed.

The complete JSON contained 80 suites and 2,509 cases, agreeing with its reported test count and the unchanged Rust selection's 2,483 runnable plus 26 skipped tests. The driver received 361,212 stdout bytes with SHA256 `85af4a23528f9041b9f6857cee5def6e84e1e07c2370a885beb06dcdc320bcc2`, and 9,841 stderr bytes. Raw inventory stayed in memory. The final driver result was zero with reason `complete`, the direct child exited zero, and the outer supervisor exited zero with `stopped: null` after normal owned-group cleanup. All eight process proofs were known and reaped, taking 4.877–22.303 ms; the final proof took 19.775 ms. No resource stop or release-gate receipt authority was recorded.

Six independent in-window observations reported at least 19,585,249,280 free disk bytes and 57% free memory. Memory-query duration peaked at 7.849 ms; the bracketing sample gap peaked at 10.010 seconds. No new sampler reason or crash-report metadata appeared. These sampled observations supplement process proofs; they are not continuous monitoring. The complete closed evidence manifest is `1eb53bf02629b0fc977d7dfb34dbe9ca195c1e9ad94d64b896fae62f02a76f67`; the exact config is `0f71b4531187e51fa1d3f32d02e64169d6938538d5e9edef7ee86d7bfb56c372`.

This supports validating an optional discovery-execution cap of one in HS2-YK109D, with HS2-XW38CB covering effective worker-input fingerprints. It does not establish concurrency causality, an optimal cap, native descriptor retention, test-body correctness or a passing full gate. The LIST-only helper still requires a separately reviewed gate adaptation with run-phase passthrough and meaningful cancellation, lock and descriptor tests. Existing source-provenance and HS2-F6F8MA containment qualifications remain.
