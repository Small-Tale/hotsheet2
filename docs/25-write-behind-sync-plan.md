# Durable write-behind for ticket providers (HS2-BH3CSK)

## What is slow now

The original 100-ticket Completed-to-Verified Git batch spent roughly 15 seconds
persisting because the server performed a Git commit, index refresh, event, and
worklist update for every ticket. HS2-0JZDQ8 replaced that path with one commit per
store and one refresh. Two real Git-store runs persisted in 722 and 808 ms. The latest
`Server-Timing` sample was 71.5 ms admission, 115.8 ms file writes, 310.6 ms Git,
77.3 ms index, 1.7 ms events, 0.5 ms worklist, and 601.6 ms total server work; a
subsequent two-page query took 14 ms. The board projected the move and refill in
61 ms. There is no current local-Git evidence that a new log is needed for this
100-ticket action.

The web client already projects single and bulk changes before the request finishes.
Bulk Git updates use `/checkouts/{reference}/tickets/batch`; sources without atomic
batch support use one awaited PATCH per ticket and show progress. GitHub, GitLab,
and Jira adapters currently read the remote issue, check its concurrency token,
then write it. Their `offline_mutation` capability is false. With 100 external
tickets, serial network round trips and rate limits can dominate. This is a path
analysis, not a measured external-provider latency claim. The live GitHub adapter
has a ten-second issue-list read snapshot, but no durable pending-write overlay.

## Proposed contract

Keep the native provider authoritative. A separate **durable outbox** records local
intent until the provider confirms it. Do not put pending writes in the disposable
search index or rely on browser memory. A write-behind endpoint acknowledges only
after its outbox transaction is durable, returning an operation ID and projected
ticket(s) marked `pending`. It must not claim that the remote provider has committed.
Existing synchronous PATCH behavior remains available until every caller understands
that distinction.

Each intent stores provider connection and native ticket ID, operation ID and
idempotency key, actor, creation time, ordered per-ticket sequence, base remote
token and field values, patch, retry state, and last error. Credentials remain in
the existing keychain, never in the outbox. A small SQLite database using its own
WAL mode is a reasonable first implementation: one transaction can durably accept
a 100-ticket batch; the queue is distinct from the rebuildable index. Set a bounded
queue size and return explicit backpressure when full. Startup recovery must resume
accepted intents; compaction removes only confirmed and safely retained audit
records. A rotating file log is an alternative only if it provides the same atomic
batch, crash recovery, and compaction guarantees.

Reads merge the latest remote snapshot with pending intents in per-ticket order,
including list rows, counts, full ticket reads, search, and change events. The
overlay is always labeled provisional. A remote refresh never erases local intent
merely because the remote response is older. Pending updates survive browser and
server restart. On confirmation, replace the projection with the provider response
and advance the remote token; on rejection, keep the intent and show `needs
attention` until retry or explicit discard. Undo creates a compensating intent rather
than pretending an already dispatched write can be cancelled.

## Dispatch and contention

Use bounded concurrency per provider, serialize writes per ticket, honor provider
rate-limit reset and `Retry-After`, and retry transient failures with jitter. Distinct
fields can be rebased onto a fresh remote version. For a same-field conflict, retain
the local intent in the projection while the worker obtains a fresh remote version;
ordinary ticket fields should apply the latest local intent by default and record
the overwritten remote value for review. Destructive actions, cross-provider moves,
claims, and attachment mutations need explicit operation-specific conflict rules
before entering this queue. Never infer that a timed-out create or note append failed:
retry only with a provider idempotency key or a searchable operation marker, otherwise
pause it as an uncertain outcome for reconciliation.

The worker reports `queued`, `sending`, `rate_limited`, `needs_attention`, and
`confirmed` with operation IDs. The client shows pending counts and per-ticket
state, provides retry/discard where safe, and keeps the existing optimistic board
response. A successful transport response is not enough if its returned version
does not contain the intent; read back or use provider acknowledgement semantics.

## Delivery and evidence gates

1. Instrument external-provider GET, token check, PATCH, remote acknowledgement,
   queue wait, and projection latency with redacted `Server-Timing` and client timing.
   Run 1, 20, and 100-ticket fake-transport tests with controlled latency/rate limits
   plus an opt-in real-provider benchmark. Compare this with the current Git baseline.
2. Add the durable outbox and overlay behind a capability and feature flag for one
   provider. Preserve synchronous endpoints. Prove atomic batch acceptance, restart
   replay, idempotent retry, read-your-writes, and bounded backpressure.
3. Add provider-specific dispatch and conflict handling, then the pending/failed UI.
   Test remote update races, out-of-order events, partial batches, offline/reconnect,
   rate limits, timeout-after-success, and multi-client reads. Extend to the other
   providers only after the first provider passes those gates.

The local Git path stays synchronous for now: its measured persistence is under one
second for this case and gives immediate file durability. Revisit it only if a new
profile shows a concrete remaining bottleneck.

### Phase 1 measurement (HS2-ZHN7XS)

Provider ticket PATCH now reports redacted read, token-check, write, acknowledgement,
queue-wait, and total durations in `Server-Timing`. Queue wait is zero while there is
no durable provider outbox. The web client emits optimistic projection, request, and
single-ticket sequencer wait durations through `hotsheet:mutation-timing`; bulk events
include their count. These measurements preserve synchronous mutation semantics.

The controlled GitHub, GitLab, and Jira fake transports wait 2 ms for each GET
and 3 ms for each PATCH or PUT. Jira's current update path also reads the issue
and comments after PUT to acknowledge the result. GitLab and GitHub decode the
write response as their acknowledgement.
One local run of `cargo test -p hotsheet-extsync --test provider_latency -- --nocapture`
measured serial writes as follows; the numbers include test runner scheduling and are
diagnostic, not a real provider service-level claim:

| Provider | Tickets | Batch elapsed | Per-ticket p50 / p95 | Read p50 / p95 | Write p50 / p95 |   Ack p50 / p95 |
| :------- | ------: | ------------: | -------------------: | -------------: | --------------: | --------------: |
| GitHub   |       1 |        9.0 ms |         9.0 / 9.0 ms |   3.7 / 3.7 ms |    4.2 / 4.2 ms | response decode |
| GitHub   |      20 |      147.4 ms |         7.5 / 8.4 ms |   3.2 / 3.5 ms |    4.4 / 4.9 ms | response decode |
| GitHub   |     100 |      785.3 ms |        7.6 / 10.4 ms |   3.1 / 3.7 ms |    4.5 / 6.3 ms | response decode |
| GitLab   |       1 |        9.5 ms |         9.5 / 9.5 ms |   3.5 / 3.5 ms |    5.1 / 5.1 ms |    0.0 / 0.0 ms |
| GitLab   |      20 |      153.4 ms |         7.8 / 8.4 ms |   3.2 / 3.6 ms |    4.6 / 4.9 ms |    0.1 / 1.0 ms |
| GitLab   |     100 |      790.3 ms |        7.6 / 10.1 ms |   3.1 / 4.2 ms |    4.6 / 5.1 ms |    0.0 / 0.1 ms |
| Jira     |       1 |       14.7 ms |       14.7 / 14.7 ms |   3.5 / 3.5 ms |    4.6 / 4.6 ms |    6.6 / 6.6 ms |
| Jira     |      20 |      285.6 ms |       14.3 / 15.9 ms |   3.2 / 3.8 ms |    4.6 / 5.1 ms |    6.5 / 7.7 ms |
| Jira     |     100 |     1451.2 ms |       14.0 / 19.1 ms |   3.2 / 4.0 ms |    4.6 / 5.1 ms |    6.2 / 8.7 ms |

In a controlled 100-ticket run that returned a 429 for every twentieth GET, five
mutations failed explicitly; the current synchronous path did not queue them. The
roughly 0.8-second fake GitHub and GitLab 100-ticket results exceed the 601.6 ms
measured local Git 100-ticket server work above; Jira takes about 1.5 seconds with
its acknowledgement readback. These are controlled adapter costs, not live-provider
latency claims. Real provider round trips and rate limits should be measured before
committing to a write-behind implementation. The strongest phase 2 candidate is
Jira serial 20- and 100-ticket field edits because the required read, PUT, and
readback amplify latency. GitHub and GitLab multi-ticket field edits follow closely;
GitHub status updates use the same read/write path. Single-ticket edits and the local
Git batch do not yet justify an outbox. Jira status transitions use a separate action
and were not measured in this field-edit fixture.

An ignored opt-in test, `opt_in_real_github_mutation_latency`, performs three same-title
PATCH requests against a designated issue. It requires
`HOTSHEET_REAL_PROVIDER_BENCH=1`, `HOTSHEET_BENCH_GITHUB_REPOSITORY`,
`HOTSHEET_BENCH_GITHUB_ISSUE`, and `HOTSHEET_BENCH_GITHUB_TOKEN`; run it only against
an issue meant for benchmarking. It was not run for this measurement.

### Phase 2 durable Jira admission (HS2-056R8P)

An explicit server flag, `HOTSHEET_JIRA_WRITE_BEHIND=1`, opens a separate Jira
field-edit route. It stores the operation ID, ticket base snapshot and concurrency
token, patch, and per-ticket sequence in `${HOTSHEET_HOME}/outbox/<store-id>.sqlite`.
SQLite WAL with full synchronous commits makes batch admission atomic and replayable
after restart. The initial queue limit is 1,000 unconfirmed operations; a full queue
returns HTTP 429. Credentials are never stored in this database.

Only title, details, category, priority, and tags can enter this queue. The 202
response and read DTOs identify provisional tickets with `pending_operation_ids`.
The direct provider and checkout full/list/search paths apply pending operations
before search, sorting, and cursor selection. Summary counts do not change for
these fields. A pending read uses a complete native Jira list to preserve search
and order; this opt-in path's full-list cost must be measured before broader
rollout. Native PATCH routes retain their existing
synchronous behavior. Phase 3 still owns dispatch, confirmation, conflict handling,
retry/attention states, events, and the pending-state UI.
