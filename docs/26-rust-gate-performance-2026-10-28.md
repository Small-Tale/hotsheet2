# Rust gate test profile — 2026-10-28 (HS2-Z1NY93)

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
