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
or exhausts a component's practical capacity. Fixture staging and commit are also tier
results: a timeout or failure remains in `fixture_commit`, the generated ticket/byte totals
are retained, and read-only CLI, server, and browser page probes continue. Mutation probes
are explicitly skipped while the Git worktree may be partially staged, so the harness does
not accidentally commit an incomplete fixture. That makes bottlenecks visible in the JSON
report instead of losing the generated tier or earlier measurements.

Useful options:

```sh
npm run stress:scale -- --counts 10000,100000,1000000
npm run stress:scale -- --counts 1000 --skip-web --timeout-ms 60000
npm run stress:scale -- --counts 10000,100000 --skip-web --assert-cli-budgets
npm run stress:scale -- --counts 10000,100000 --skip-web --assert-cli-mutation-budgets
npm run stress:scale -- --counts 100000,1000000 --skip-web --assert-reindex-budgets --timeout-ms 700000
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
The opt-in `--assert-reindex-budgets` gate caps a full disposable-index rebuild at 60
seconds for 100K tickets and 600 seconds for 1M tickets. Full rebuild writes are grouped
in one SQLite transaction while corrupt ticket files remain skippable, so the gate measures
the required complete parse/index pass without per-row autocommit overhead. Use a timeout
above the 1M budget when exercising both tiers.
The opt-in `--assert-web-100k` acceptance gate keeps each first/continuation server page
at exactly 200 rows, at most 1 MB, and under 60 seconds on the stressed debug harness;
requires production Chromium to reach a populated Queue within 120 seconds and stay below
192 MB JavaScript heap; caps Queue/Backlog/Archive view switches at 2 seconds; requires the
rapid round trip back to the already-loaded Queue to launch no redundant collection request;
and separately caps the browser's 100K ticket-update interaction at 30 seconds. Rapid view
intent is trailing-edge coalesced before expensive collection work begins. Locally acknowledged
create and update events are consumed by exact event kind and ticket ID after the mutation
barrier, so they do not contend with their own redundant full-project reconciliation; unmatched
and concurrent external events still refresh normally. Project opening and server-owned ticket
writes regenerate checkout worklists from the indexed, active Up Next projection instead of
rescanning every ticket file. Exact short-lived server-write hashes suppress the corresponding
filesystem-watcher echo. Unmarked external filesystem changes are reindexed from their exact
changed paths before regenerating the same bounded projection. It is a manual release/capacity
gate, not ordinary CI.

The ordinary Rust suite separately protects asynchronous remote publication under sustained
host pressure. Its focused store regression occupies the available CPU workers and performs
repeated synced filesystem writes, observes the background push child through an in-process
handshake, and blocks the bare remote's receive hook through a FIFO handshake. Only the
local-mutation return remains time-bounded; push startup and final remote observation are
event-driven so a loaded host cannot consume an arbitrary startup allowance before the
nonblocking assertion begins.

## Local Git board flow

Run `npm run test:real-world-performance` from `clients/web` for a repeatable
client UI measurement against a real disposable Git ticket store and server. The
fixture helper seeds and commits 200 Completed tickets, confirms that the store
is clean and has no remote, and deletes the workspace after the run. Other UI
performance scenarios can reuse the same helper with different status/count groups.

The browser switches to Columns, selects the first 100 visible Completed tickets
through the column header, and changes their status to Verified from the context
menu. Its `real-git-board-performance.json` Playwright attachment records elapsed
milliseconds from the action to 100 rows appearing in Verified and to the next
100 rows appearing in Completed. The test asserts both final row counts. Timings
are exploratory because local CPU and disk load vary; compare runs on the same
machine and build profile rather than treating one number as a fixed CI budget.
The suite is opt-in so ordinary browser CI does not pay for the 200-ticket fixture
and 100-ticket Git mutation.

## WebKit session memory

On macOS, run the opt-in WebKit process profile from `clients/web` after building the
debug CLI and server binaries:

```sh
zsh -ic 'HOTSHEET_WEBKIT_MEMORY_PROFILE=1 HOTSHEET_WEBKIT_MEMORY_CYCLES=18 HOTSHEET_WEBKIT_MEMORY_IDLE=1 npx playwright test tests/webkit-memory.spec.ts --workers=1'
```

The scenario opens a disposable 120-ticket Git checkout through the real Rust server,
streams 200 lines through a real PTY, then repeatedly edits a ticket title and switches
through List, Columns, Notifications, Settings, and the terminal workspace grid. It
samples the newly launched WebKit WebContent process's RSS with `ps` at a consistent
grid state. The test also checks that terminal glyphs paint, the DOM row counts stay
bounded, the final edit reaches the store, and no page error occurs. Its
`webkit-memory-profile.json` attachment records each sample. Run it without other
Playwright WebKit sessions so a newly spawned process can be attributed to this test.
`HOTSHEET_WEBKIT_MEMORY_BLOCKS=2` repeats the edit/navigation block and idle period
in the same tab to distinguish warm-up from continuing retention.

In one HS2-2G23X9 sample, WebContent rose from 344 MiB to 722 MiB RSS during 18
cycles over 148 seconds, stayed at 722 MiB after 30 idle seconds, and fell to 462
MiB after 60 idle seconds. In a separate two-block sample, it rose from 313 MiB to
651 MiB during the first 12 cycles, stayed near 651 MiB through the first idle
minute, peaked at 735 MiB during the second 12 cycles, and ended at 554 MiB after
the second idle minute. The DOM held about 5,349 nodes, including 24 terminal rows
and 120 ticket rows, throughout the active cycles. These results show considerable
RSS fluctuation and delayed reclamation, but do not identify a retained-object path.
RSS includes WebKit allocator and shared pages and is not the same as JavaScript
heap or the process's physical footprint. Neither sample reproduced a tab reset or
identified the user's existing Safari tab. A real Safari session with its tab
process identified is still needed if the reset continues.
