# Durable write-behind for ticket providers (proposal; HS2-BH3CSK)

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
