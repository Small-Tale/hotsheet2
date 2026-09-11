# Scale stress testing

The local scale harness exercises Hot Sheet with real disposable Git ticket stores at
10,000, 100,000, and 1,000,000 tickets. It is deliberately outside the normal test suite:
the largest run consumes substantial time, disk, memory, and browser resources.

From `clients/web`, after building the Rust workspace and installing the web dependencies:

```sh
cargo build --workspace
npm run stress:scale
```

The harness creates one temporary checkout and sibling store, adds and commits tickets
incrementally at each configured milestone (so Git delta reconciliation is measured under
the same clean-store contract as normal auto-committed operation), and records:

- generation time and dataset bytes;
- CLI reindex, bounded list, full-text query, show, create, and edit wall time plus peak RSS;
- server cold index/startup time and peak RSS, followed by bounded 200-row compact first
  and continuation pages (including cursor, exact total count, and response bytes), detail,
  create, and update HTTP timings and response sizes;
- Chromium Playwright initial load, Queue/Backlog/Archive switches, ticket opening, ticket
  creation, ticket mutation, and browser JavaScript heap.

Every scenario records an error and continues to the next tier when it exceeds the timeout
or exhausts a component's practical capacity. That makes bottlenecks visible in the JSON
report instead of losing the earlier measurements.

Useful options:

```sh
npm run stress:scale -- --counts 10000,100000,1000000
npm run stress:scale -- --counts 1000 --skip-web --timeout-ms 60000
npm run stress:scale -- --counts 10000,100000 --skip-web --assert-cli-budgets
npm run stress:scale -- --counts 10000,100000 --skip-web --assert-cli-mutation-budgets
npm run stress:scale -- --counts 100000 --assert-web-100k
npm run stress:scale -- --keep --output /private/tmp/hotsheet-scale.json
```

The default JSON report is written under the operating system temporary directory. The
generated checkout, store, indexes, isolated Hot Sheet home, and browser/server state are
deleted after the run. `--keep` retains that temporary workspace for profiling or manual
inspection; remove it when finished. The harness never configures or contacts a remote.

For comparable results, record the report's host metadata, run on an otherwise quiet
machine, use the same build profile, and compare the same milestone. By default this is an
exploratory capacity test, not a stable timing assertion. The opt-in
`--assert-cli-budgets` mode fails if bounded list, full-text, or show exceeds 2 seconds at
10K or 5 seconds at 100K; it intentionally remains outside normal CI. Normal CI continues
to use the focused unit, integration, browser, and interaction-budget gates. The separate
`--assert-cli-mutation-budgets` gate caps create/edit at 5 seconds for 10K and 30 seconds
for 100K, including path-scoped Git durability and index-backed worklist refresh.
The opt-in `--assert-web-100k` acceptance gate keeps each first/continuation server page
at exactly 200 rows, at most 1 MB, and under 60 seconds on the stressed debug harness;
requires production Chromium to reach a populated Queue within 120 seconds and stay below
192 MB JavaScript heap; and caps Queue/Backlog/Archive view switches at 2 seconds. It is a
manual release/capacity gate, not ordinary CI.
