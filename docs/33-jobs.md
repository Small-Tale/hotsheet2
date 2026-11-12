# 33 — Jobs

**Status:** Partly shipped. The core model, state machine, and local store are
shipped in `crates/hotsheet-jobs` (HS2-GCHX8V, §33.7). The producers, CLI, server,
MCP, and client surfaces remain design only and are tracked by the tickets in
§33.6 (design: HS2-M78N64, from HS2-46YX92 annotation #1).

A **Job** is a transient, machine-local record of long-running work such as a test
run, a project command, or a wait for CI. It is not a ticket: it has no provider,
priority, Up Next position, status workflow, or ticket sync. Its vocabulary is
**steps** for sub-progress, **feed** for streamed detail, and **outcome** for the
result. A Job may link to the qualified `(connection_id, native_id)` of the ticket
that spawned it. Completion writes a short result summary to that ticket through
its authoritative provider; the Job and its feed stay local.

## 33.1 Identity and record

Each Job has a ULID, checkout id, producer kind and stable job key, title, creation
and update times, status, optional outcome summary, producer lease identity, and
optional spawning-ticket link. The job key groups comparable runs for historical
ETA; it is not the Job id. A title describes the work without copying private
feed content. A producer may publish a URL only when its kind declares `open_url`.

The status is one of `queued`, `running`, `waiting`, `succeeded`, `failed`,
`cancelled`, or `lost`. `waiting` means an active producer is waiting on an
external condition, such as a queued CI run. `lost` means the producer stopped
reporting without an outcome. A lease is renewed by the producer; the CLI and
server can detect expiry after a restart. For an owned process, liveness is tied
to the process-journal launch id, never a bare PID.

Each Job has a dynamic tree of steps. Every step has a stable id within the Job,
label, status, optional `done` and `total` with a unit, positive weight, and
children. Producers may add steps as the plan becomes known. A parent rolls up
its children's weighted fractions; an active child without a total makes the
rollup partly indeterminate. A failed or cancelled step retains its last measured
fraction rather than displaying 100%. Invalid totals, duplicate ids, or cycles
are rejected. ETA is producer-supplied when available; otherwise it is estimated
from completed Jobs with the same job key. An insufficient history gives no ETA.

The feed is a bounded sequence of entries with monotonically increasing `seq`
values and a cursor for replay. A reader gets entries after its cursor, or an
explicit gap when retention has removed them, then resumes from the current
cursor. The producer may also report a few latest-value fields, such as the
current test or CI step. Feed data can contain local output: it is never copied
to ticket notes, notifications, telemetry, or the synced ticket store.

## 33.2 State machine

The core crate tests the following transitions and rejects every unlisted one.
Updates within the same nonterminal state may change steps, feed, ETA, and lease
without creating a status transition. A final outcome is immutable; retry makes
a new Job linked to the old one.

| From                 | To                    | Required event and rule                                                                                                                                   |
| -------------------- | --------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------- |
| New                  | `queued`              | Create a valid Job with a checkout and producer kind.                                                                                                     |
| `queued`             | `running`             | The producer starts work and takes a live lease.                                                                                                          |
| `queued`             | `waiting`             | A watch producer begins observing an external condition.                                                                                                  |
| `queued`             | `cancelled`           | A supported cancellation is acknowledged before work starts.                                                                                              |
| `running`            | `waiting`             | The producer reports a live external wait.                                                                                                                |
| `waiting`            | `running`             | The producer reports that work resumed.                                                                                                                   |
| `running`, `waiting` | `succeeded`, `failed` | The producer reports an outcome.                                                                                                                          |
| `running`, `waiting` | `cancelled`           | The owning process or supported external operation confirms cancellation. A request alone does not finish the Job.                                        |
| `running`, `waiting` | `lost`                | The lease expires and recovery cannot confirm a live owner.                                                                                               |
| `lost`               | `running`, `waiting`  | A watch producer reattaches to the same external operation with verified identity and a fresh lease. An owned or reported Job cannot use this transition. |

An unsupported cancel action is absent, not a transition. A cancel request that
fails leaves the Job in its prior state and reports the failure. Terminal states
`succeeded`, `failed`, and `cancelled` cannot resume. `lost` is terminal for owned
and reported producers; a user retry creates a new Job. The core transition tests
cover valid edges, every invalid edge, repeated and out-of-order reports, lease
expiry versus late outcomes, cancellation races, and watch reattachment. A late
message from an earlier lease cannot overwrite a newer generation or outcome.

## 33.3 Local storage, recovery, and retention

The CLI writes Jobs under `${HOTSHEET_HOME}/jobs/<checkout-id>/<ulid>/` without a
server. A Job directory contains an atomically replaced snapshot and a bounded
append-only feed with enough cursor information to detect gaps after compaction.
The directory is machine-local, outside the ticket store and source checkout; it
is never synced. Concurrent CLI, monitor, and server access must preserve one
ordered state machine and monotonic feed sequence. Readers reject an unknown
record version rather than rewriting it.

After a restart, the CLI or server reloads the snapshot and reconciles the
producer lease and launch identity. An owned Job whose process is gone becomes
`lost`; a watch Job can reattach to its external operation. A lightweight
monitor may run without a server and report through the same CLI/store path.
Finished Jobs remain for a short, bounded recovery window, then disappear.
Retention also caps count and feed bytes per checkout; dismissal may remove a
finished Job sooner. The durable trace is only the result summary on its
spawning ticket, when one exists. Summary writes are idempotent by Job id and
contain outcome and identifiers, not feed lines.

## 33.4 Producers and capabilities

One Job core supports three producer families, each with specialization plugins:

| Family        | Source of truth                                      | Examples                                                  | Cancellation                                                                             |
| ------------- | ---------------------------------------------------- | --------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| Owned process | Hot Sheet's child-process launch and process journal | Project commands; `check.sh`, nextest, vitest, Playwright | Stop the owned process group and confirm exit.                                           |
| Watch         | An external operation                                | GitHub Actions CI, later other CI systems                 | Provider cancel only when supported; otherwise stop watching is a distinct local action. |
| Reported      | An outside producer reporting through CLI or MCP     | Agent or script progress                                  | Only when it registers a verifiable launch id or cancel hook.                            |

The kind's manifest declares its family, parser, job key, and capabilities:
`cancel`, `retry`, and `open_url` when supported. A client exposes only declared
actions. A watch monitor may poll its external source with bounded backoff while
the Job is active; many clients must not each poll. Clients receive changes from
the hosting server's existing change stream with cursor replay and bounded
reconnect backoff. No server is needed to create, update, read, or monitor Jobs
through the CLI. A monitor that polls CI remains lightweight and can be started
for that Job alone (HS2-5TC1GQ).

## 33.5 Surfaces and access

Jobs appear at the top of List View and the top of the **Started** column in
Column View, separate from ticket rows. The ticket details header links to a
Job it spawned. Workers view has a **System** column for each connected system,
showing that system's Jobs. A compact summary shows title, status, progress, and
ETA; expansion shows the step tree and bounded feed. The feed is collapsed by
default. These surfaces do not make a Job draggable or editable as a ticket.

The headless CLI can list, inspect, tail, cancel, retry, and dismiss Jobs and
start owned, watch, and reported work. MCP provides equivalent observation and
reporting for agents. The server reads the same local store, serves authorized
REST and change-stream views, and mediates remote actions. A remote client sees
Jobs only through the server that owns the checkout; it cannot sync the records
to its own machine. Cancel and feed access honor that server's session role and
revocation rules. Feed content is never included in notification payloads.

## 33.6 Implementation boundaries

The core model and local store are HS2-GCHX8V; CLI commands HS2-V4Q2MA;
lightweight monitor HS2-5TC1GQ; server REST and change stream HS2-WPFWS8; MCP
HS2-B2PAVJ. Client placement is HS2-JR20NM, the ticket header link HS2-4VK4BN,
and Workers System columns HS2-3Y8HQF. Test-runner parsers are HS2-YD292M;
CI watch HS2-3R9V0R; result summary HS2-BDH12X; notification HS2-FCBYC8;
project-command consolidation HS2-YT619Z. Their implementation tickets may
refine protocol field names without changing the behavior or state transitions
above.

## 33.7 Core crate as shipped (HS2-GCHX8V)

`crates/hotsheet-jobs` is a library with no server or ticket-store dependency. It
reads time and mints ids only through injected `Clock` and `IdSource` values.

**Files.** `${HOTSHEET_HOME}/jobs/<checkout-id>/` holds a `.lock` file, a `revision`
counter that grows with every change in the checkout, a `history.json` of recent
successful durations by job key, and one `<ulid>/` directory for each Job with
`job.json` and `feed.jsonl`. Directories are created with owner-only permissions.
`job.json` carries `version: 1`; any other version is rejected and never
rewritten or removed. Feed text is stored only in `feed.jsonl`.

**Leases.** `acquire` moves a `queued` Job to `running` or `waiting` and returns a
token of owner and generation. Every producer write presents the token and renews
the lease. A token from an earlier generation, or from another owner, is refused.
A producer may also number its reports; a report at or below the last accepted
number is refused. The default lease is 60 seconds.

**Stalled and lost.** `stalled` is a derived liveness, not a stored status: the
Job is active and its lease has expired. A late renewal makes it live again.
`reconcile` turns a stalled Job into `lost` after a further 5 minutes, or at once
when the owner probe reports that the process-journal launch has ended. A probe
that confirms a live owner prevents the loss. A `lost` watch Job can be acquired
again by presenting the same external operation; a stalled watch Job can be taken
over the same way, which applies the `lost` and reattach edges in one locked step.

**Outcomes and cancellation.** An identical repeated outcome from the same lease
is accepted without a write; any different repeat is a rejected transition. A
cancel request on a `queued` Job cancels it at once, because no producer has
started. On an active Job the request is only recorded: the producer confirms it,
or reports the failure, which clears the request and keeps the prior state. A
kind without the `cancel`, `retry`, or `open_url` capability refuses that action.
Recording a cancellation that was observed needs no capability. Stop watching is
a separate request that every watch Job accepts: the Job ends as `cancelled` with
`refs.cancel = stopped_watching`, and its external operation is left alone. A
producer that stops deliberately releases its lease, so the Job reads as stalled
at once.

**Steps and ETA.** A producer replaces the whole step tree in one report, as a
nested tree or as parent-linked steps. A tree holds at most 512 steps in 8
levels. The history estimate is the median of the last 16 successful durations of
the job key and needs at least 3 of them.

**Feed.** One update appends at most 256 lines of at most 4096 bytes each. A feed
is capped at 256 KiB; reaching the cap drops the oldest entries down to half the
cap. A reader behind the retained range gets one gap, then the retained entries.

**Retention.** `succeeded` and `cancelled` Jobs are removed 15 minutes after they
finish; `failed` and `lost` Jobs after 24 hours. A checkout holds at most 64 Jobs.
A create at the cap removes the oldest finished Job, and is refused when every
Job is unfinished. Retention runs on create and on `reconcile`, which also removes
abandoned temporary files, unfinished creates, and damaged records.
