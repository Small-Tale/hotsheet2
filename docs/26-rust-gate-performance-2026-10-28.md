# Rust gate test profile — 2026-10-28 (HS2-Z1NY93)

Startup and discovery observations are recorded separately in the [2026-10-31 supervised Nextest investigation](nextest-discovery-2026-10-31.md) (HS2-2N8324).

The required `cargo nextest run --all-features` gate ran in 101–138 seconds in the prior warm profile at `cc20c7ca`, with 32 tests above five seconds accounting for 264 seconds of summed test time. This change targets four 500-row boundary tests. Their fixtures each need 501 parseable ticket files to exercise overflow, truncation, filtering, and paging. The tests still use real `FsStore` reads and the same assertions.

## Focused comparison

The baseline and changed tree used the same worktree, all-features nextest profile, and four test names. Browser gates were idle during both focused runs. The first three baseline tests ran together; the fourth ran separately after a filter typo. The changed tests ran together. Times are nextest's per-test elapsed times, so they include fixture setup and queries. The changed run started at a one-minute host load average of 32.8, within the 25–37 range of the prior warm gate profile.

| Test                                                                 |  Before |   After |
| -------------------------------------------------------------------- | ------: | ------: |
| HTTP `unpaged_checkout_arrays_are_bounded_and_flag_truncation`       | 7.421 s | 0.723 s |
| HTTP `store_level_ticket_lists_are_bounded_and_page_with_page_after` | 7.330 s | 0.657 s |
| MCP `corebackend_checkout_query_is_bounded_like_the_server`          | 7.646 s | 0.957 s |
| MCP `corebackend_store_query_is_bounded_and_pages_with_page_after`   | 7.386 s | 0.837 s |

The direct-file fixture wrote 501 tickets in about 214–216 ms per test in an instrumented run. It serializes `Ticket` values with `hotsheet_model::to_file_string` into the store's current ticket paths. This bypasses per-row writer-format and atomic-replacement checks that are irrelevant to read-path tests. A trial using `FsStore::write_ticket` did not help: its four tests took 7.8–8.1 seconds under shared load. Temporary test timing output was removed after measurement.

The focused result demonstrates lower test time, but the effect on full-gate wall time depends on nextest scheduling and other parallel tests. Lease-expiry tests were left unchanged because their real elapsed-time transitions are the behavior they verify.

## Follow-up profile (HS2-SWBHYG)

The integrated 2,218-test JUnit report at `9db63f6c` recorded 129.861 seconds of nextest execution. Its longest test, `hs2_zwbsh1_hook_fixtures_ignore_a_poisoned_hot_sheet_session`, took 59.915 seconds; the eight hook fixtures it reruns also took 8.336–8.973 seconds each as independent tests. The nested replay forced one test thread, serializing those fixture delays. Every fixture uses its own temporary home and ephemeral listener, so the nested replay now permits four threads while keeping the poisoned inherited environment, all eight selected test names, and the `8 passed` assertion.

| Measurement                               |    Before |    After |
| ----------------------------------------- | --------: | -------: |
| Poisoned-session replay, focused sample 1 |   1.704 s |  1.371 s |
| Poisoned-session replay, full nextest run |  59.915 s |  0.116 s |
| Full Rust nextest execution, 2,218 tests  | 129.861 s | 86.807 s |

An additional unchanged focused sample took 1.455 seconds. The changed full run passed all 2,218 tests. The full runs were on the same host and command but under different concurrent load, and the focused sample was taken under lighter load. The 43.054-second difference in full-run execution time cannot be assigned solely to this change. The nested replay's result still asserts that all eight named fixtures ran successfully. Remaining longest tests took about 10–14 seconds and include real lease-expiry transitions; no timeout or assertion was changed.
