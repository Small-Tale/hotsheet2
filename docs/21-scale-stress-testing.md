# Scale stress testing

The local scale harness exercises Hot Sheet with real disposable Git ticket stores at
10,000, 100,000, and 1,000,000 tickets. It is deliberately outside the normal test suite:
the largest run consumes substantial time, disk, memory, and browser resources.

From `clients/web`, after building the Rust workspace and installing the web dependencies:

```sh
cargo build --workspace
npm run stress:scale
```

The harness creates one temporary checkout and sibling store, adds tickets incrementally at
each configured milestone, and records:

- generation time and dataset bytes;
- CLI reindex, bounded list, full-text query, show, create, and edit wall time plus peak RSS;
- server cold index/startup time and peak RSS, followed by compact-list, detail, create, and
  update HTTP timings and response sizes;
- Chromium Playwright initial load, Queue/Backlog/Archive switches, ticket opening, ticket
  creation, ticket mutation, and browser JavaScript heap.

Every scenario records an error and continues to the next tier when it exceeds the timeout
or exhausts a component's practical capacity. That makes bottlenecks visible in the JSON
report instead of losing the earlier measurements.

Useful options:

```sh
npm run stress:scale -- --counts 10000,100000,1000000
npm run stress:scale -- --counts 1000 --skip-web --timeout-ms 60000
npm run stress:scale -- --keep --output /private/tmp/hotsheet-scale.json
```

The default JSON report is written under the operating system temporary directory. The
generated checkout, store, indexes, isolated Hot Sheet home, and browser/server state are
deleted after the run. `--keep` retains that temporary workspace for profiling or manual
inspection; remove it when finished. The harness never configures or contacts a remote.

For comparable results, record the report's host metadata, run on an otherwise quiet
machine, use the same build profile, and compare the same milestone. This is an exploratory
capacity test, not a stable timing assertion: normal CI should continue to use the focused
unit, integration, browser, and interaction-budget gates.
