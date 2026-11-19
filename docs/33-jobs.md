# 33 — Jobs

**Status:** Partly shipped. The core model, state machine, and local store are
shipped in `crates/hotsheet-jobs` (HS2-GCHX8V, §33.7), with the watch monitor and
the `job list`, `job show`, and `job watch` commands (HS2-5TC1GQ, §33.8) and the
GitHub Actions CI watch (HS2-3R9V0R, §33.9). The other producers, the remaining
CLI commands, the server, MCP, the result summary, the notification, and the
client surfaces remain design only and are tracked by the tickets in §33.6
(design: HS2-M78N64, from HS2-46YX92 annotation #1).

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

## 33.8 Watch monitor as shipped (HS2-5TC1GQ)

A watch Job is hosted by the **monitor**, a background `hotsheet-cli` process. It
owns no work. It observes an external operation through a watch source, renews
the Job's lease, and writes what it sees to the Job store. It needs no server.

**One monitor per machine home.** One process serves every watch Job in every
checkout under `${HOTSHEET_HOME}/jobs/`, instead of one process for each Job. It
holds a file lock for its whole life, so a second monitor cannot start, and the
operating system frees the lock however the process ends. The reasons for this
choice are a fixed cost and one place to apply backoff: the number of watched
operations does not change the number of processes or directory watchers, and a
provider's rate budget is spent by one poller. The cost of the choice is that a
monitor crash stalls every watch Job at once; the next start recovers them all.
Measured on macOS arm64 with a debug build: about 18 MB resident, and about 10 ms
of processor time in 45 seconds while watching one waiting Job, which made 3 polls
and 2 lease renewals in that time. A monitor with no Job to serve exits after 30
seconds.

**Commands.** `hotsheet-cli job watch start` starts the monitor when none is
running and prints the running monitor either way. `job watch stop` stops it and
is a no-op when none is running. `job watch status` reports it. Stopping is
cooperative: the command leaves a request addressed to that monitor's owner id,
and no process id is signalled. A stopped monitor releases its leases, so its
Jobs read as stalled at once and the next start resumes them without waiting.
`job list` and `job show` print Jobs as JSON with their derived liveness,
progress, and ETA; `job list` first turns expired producers into `lost` and
applies retention. The launch of the monitor is recorded in the process journal
with the purpose `job_monitor`. The same start is made by the server and the MCP
backend when an orchestration CI gate waits on a watch (HS2-VVHVF0,
[26](26-orchestration-projects.md) §26.13); it is one shared workflow,
`DetachedJobMonitor`, which finds `hotsheet-cli` beside the running binary.

**Serving a Job.** The monitor serves a watch Job when it has a source for the
Job's kind and the Job records its external operation. It attaches to a `queued`
Job, reattaches to a `lost` one, and takes over one whose lease has expired, each
time under a new lease generation. It wakes on a file change under the Job
directory, or when a renewal or poll is due; it does not rescan on a timer. A
step with nothing due writes nothing. It renews each lease at a third of the
lease length, whether or not a poll is due.

**Polling.** A source may answer a poll with "not modified" when the provider
supports conditional requests. The first poll is immediate. The delay is 15
seconds after a poll that saw a change, grows by half after each unchanged poll
up to 2 minutes, and returns to 15 seconds on the next change. A failed poll
doubles the delay up to 5 minutes, or waits as long as the provider asks when
that is longer; the Job stays live and shows `latest.watch = retrying` until a
poll succeeds. An operation that no longer exists fails the Job. Polling stops
when the Job finishes, is cancelled, or is no longer held by this monitor.

**Cancel and stop watching.** For a cancel request the monitor asks the source
once. If the provider accepts, the Job becomes `cancelled` when a later poll
reports it. If the source cannot cancel, or the provider refuses, the request is
cleared and the reason is recorded; the Job keeps its state. For a stop-watching
request the monitor ends the Job as `cancelled` with
`refs.cancel = stopped_watching` and never contacts the provider.

**Recovery.** If the monitor dies, its Jobs keep their state, read as stalled
when the lease runs out, and become `lost` after the grace window. A new monitor
reattaches to the same external operation; a message from the old monitor is
then refused by its lease generation. A monitor that was paused past its
lease, for example on a sleeping machine, renews its leases before it reconciles,
so it keeps its Jobs when no other monitor took them.

## 33.9 GitHub Actions CI watch as shipped (HS2-3R9V0R)

A CI watch is a watch Job of kind `github-actions` that follows the **CI gate**
of one commit. Its target is a repository, a full commit SHA, a workflow name
(default `CI`), and a shipping branch (default `main`). The gate has the meaning
of `scripts/ci-gate.mjs status --covers <sha>`: only push runs of that workflow on
that branch count, and a run **covers** the commit when its head is the commit or
a descendant of it.

| Gate         | Meaning                                                                                                                                        | Job status  |
| ------------ | ---------------------------------------------------------------------------------------------------------------------------------------------- | ----------- |
| `pending`    | No covering run exists yet, or the newest covering run is queued or in progress.                                                               | active      |
| `green`      | A successful covering run exists.                                                                                                              | `succeeded` |
| `red`        | The newest covering run failed, timed out, failed to start, or needs action.                                                                   | `failed`    |
| `cancelled`  | The newest covering run was cancelled and nothing newer covers the commit, or watching was stopped.                                            | `cancelled` |
| `superseded` | The newest run on the branch is on a line that does not contain the commit, so no run can cover it.                                            | `failed`    |
| `unknown`    | No watch Job exists for the target, its observer was lost, the provider never learned the commit, or the watch failed for an unrelated reason. | any other   |

A run that the provider cancelled because a newer push replaced it keeps the gate
`pending` while the newer covering run is running. A covering run that was
skipped or neutral is not a failure and not the green run the gate needs, so the
gate stays `pending`.

**Steps, ETA, and links.** The steps are the jobs of the newest covering run;
an in-progress job counts its completed steps. The producer ETA of a run in
progress is its start plus the median duration of at least 3 of the last 10
successful runs of the same workflow in the listing; the history estimate of
§33.7 applies otherwise. The Job publishes the run's page as its URL. The Job
carries the qualified ticket that waits on the gate, when the caller names one,
and the commit. The outcome holds identifiers only: the gate, the commit,
repository, branch, and workflow, and the run's id, attempt, head commit,
conclusion, and URL. `ci_outcome` returns that record for the ticket result
summary (HS2-BDH12X), the notification (HS2-FCBYC8), and orchestration.

**Provider access.** The monitor lists the branch's push runs and the jobs of the
run it follows, both with conditional requests, so an unchanged answer costs no
rate limit and writes nothing. Whether a newer run's head contains the commit
comes from the provider's compare endpoint; an answer is kept for the life of the
monitor, and one poll starts at most 5 such lookups. A commit the provider does
not know yet waits for its run. When the compare endpoint still does not know the
commit 30 minutes after the watch Job was created (a mistyped SHA, or a commit
that was never pushed), the Job fails with gate `unknown` and the summary
`<short sha> is not known to <repo> after 30 minutes`, so it stops polling
(HS2-XGV2J9). `--rewatch` starts a new watch once the commit is pushed. A repository that cannot be read fails the Job
with gate `unknown`. The HTTP client takes its token from `GH_TOKEN` or
`GITHUB_TOKEN`, otherwise from `gh auth token`; the token is never written to a
Job, a log, or the process journal. A failed `gh auth token` is not tried again
for 15 minutes. `HOTSHEET_GITHUB_API_URL` selects a GitHub
Enterprise Server API root; a root that is not HTTPS is refused unless it is on
this machine.

**Cancel and stop watching.** Cancel asks the provider to cancel the run only
when the newest covering run is the commit's own run and is not finished. A
covering run that belongs to a newer commit is never cancelled. When the
provider refuses, the reason is recorded and the Job keeps running. Stop
watching ends the Job as `cancelled` and never contacts the provider. The CLI
declares the cancel capability unless `--no-cancel` is given; the library
default does not declare it.

**Commands.** `hotsheet-cli job watch ci --commit <sha>` starts or finds the
watch and makes sure the monitor runs; repeating it changes nothing. `--repo`
defaults to the checkout's `origin` remote. `--ticket <connection_id>:<native_id>`
links the waiting ticket. `--rewatch` watches again after a `red` or `cancelled`
verdict in a new Job that is linked to the old one and ignores the run already
judged, so a pushed fix or a re-run can turn the gate green; without it, a
retained verdict is returned unchanged. `job watch gate --commit <sha>` prints the
gate from the local store without contacting the provider, and
`--wait-seconds <n>` blocks until a verdict or the timeout by waiting on the
store's file changes.

**Use from another crate.** `hotsheet_jobs` exports `CiTarget`,
`ensure_ci_watch` and `rewatch_ci` (find or create under the checkout lock, so
concurrent callers get one Job), `gate_state` (returning `GateState` with the Job
id and the store revision), `wait_for_gate_change`, `JobStore::wait_for_change`,
`JobStore::wait_for_change_keeping` with its `KeptWaiter` (§33.10),
`ci_outcome`, and `ensure_monitor_running`. A caller that creates a watch must
make sure the monitor runs; the CLI does, and other callers are HS2-DVJTQN.

## 33.11 GitLab CI watch as shipped (HS2-TPAHYA)

A `gitlab-ci` watch Job follows one commit's CI gate exactly as §33.9 describes for
GitHub Actions, through the same machine monitor and the same CI gate API
(`ensure_ci_watch`, `gate_state`, `gate_of`); `CiTarget` carries the provider, and the
Job kind and external system are `gitlab-ci`.

- **Target.** The project's full path (two to twenty segments), the commit, a pipeline
  `name` or `*` for any, and the branch. The external reference has the same parts
  as GitHub's.
- **Provider calls** (relative to `/api/v4`): the branch's push pipelines, newest first
  (`projects/:path/pipelines?ref=&source=push`, conditional by ETag); the pipeline's
  jobs as steps (`stage: name`); ancestry by `repository/merge_base` of the commit and
  a pipeline head (the head covers the commit when the merge base is the commit),
  cached, at most five per poll; cancel by `POST .../pipelines/:id/cancel`, only for
  the commit's own unfinished pipeline.
- **Verdicts.** `success` is green; `failed` red; `canceled` cancelled; `skipped` and
  `manual` are no verdict; the rest wait or run. A retried pipeline keeps its id, so
  a rewatch ignores a judged pipeline by id. Outcome refs have the same keys as
  GitHub's, with `run_attempt` 1; no artifact digest exists to record.
- **Credentials.** `GITLAB_TOKEN`, sent as `PRIVATE-TOKEN`, kept in memory only;
  without it, the signed-in `glab` CLI's token for the API root's host
  (`glab config get token --host <host>`, one journaled launch, a failure remembered
  for 15 minutes, read again after a 401; HS2-RTEQ7R). `job watch status` reports
  `gitlab_token` as `GITLAB_TOKEN`, `glab` or `null`, never the token. The API root is
  `HOTSHEET_GITLAB_API_URL` (default `https://gitlab.com/api/v4`), HTTPS except
  loopback HTTP for tests.

## 33.11a Buildkite and CircleCI watches as shipped (HS2-17HVNP)

`buildkite` and `circleci` watch Jobs follow one commit's CI gate like §33.9 and §33.11,
through the same machine monitor, CI gate API and REST port. Neither provider's API
exposes repository ancestry, so these gates attest **exact coverage only**: a build or
pipeline counts only when it was built from the watched commit itself on the target
branch. A later commit's green never covers it, and the gate is never superseded; it
stays pending until the commit's own build is judged.

- **Buildkite** ([REST builds API](https://buildkite.com/docs/apis/rest-api/builds)).
  The target names the pipeline as `organization/pipeline` slugs, and the workflow is
  always `*`. Each poll lists `organizations/:org/pipelines/:pipeline/builds?branch=&commit=`
  and ignores pull request builds. A `passed` build of the commit is green; otherwise
  the newest build decides: `failed` is red, `canceled` is cancelled, and other states
  wait or run (`running`, `failing`, `canceling`). Non-wait jobs are the steps. Cancel
  is `PUT .../builds/:number/cancel`, sent only for the commit's own unfinished build.
  A rebuild is a new build number, so a rewatch ignores a judged build by number.
- **CircleCI** ([API v2](https://circleci.com/docs/api/v2/index.html)). The target names
  the project slug `vcs/organization/project` (`gh`, `github`, `bb`, `bitbucket` or
  `circleci`) and either one workflow name or `*` for every workflow. Each poll reads
  `project/:slug/pipeline?branch=` and keeps the pipelines whose `vcs.revision` is the
  commit. It then reads `pipeline/:id/workflow`, where the newest workflow of each name
  counts, a finished set is cached, and an errored pipeline is red. The counted
  workflows' jobs (`workflow/:id/job`) are the steps. Every counted workflow succeeding
  is green; any failure is red, and a cancellation is cancelled. Cancel is
  `POST workflow/:id/cancel` for the commit's unfinished counted workflows. The run
  floor is the pipeline number.
- **Outcome.** Refs have GitHub's keys, with `run_attempt` 1 and `run_head_sha` the
  commit itself. No artifact digest is recorded.
- **Credentials.** `BUILDKITE_API_TOKEN` is sent as `Authorization: Bearer`, and
  `CIRCLECI_TOKEN` as `Circle-Token`. Both are kept in memory only. `job watch status`
  reports `buildkite_token` and `circleci_token` as the variable name or `null`. The API
  roots are `HOTSHEET_BUILDKITE_API_URL` (default `https://api.buildkite.com/v2`) and
  `HOTSHEET_CIRCLECI_API_URL` (default `https://circleci.com/api/v2`), HTTPS except
  loopback HTTP. A 429 waits for `Retry-After`, or for Buildkite's `RateLimit-Reset`.

## 33.12 Validation and deployment runners (HS2-BWAPSX)

A hosted validation step or deployment that runs in a detached runner
([26](26-orchestration-projects.md) §26.12, HS2-M7EV18) is shown as an owned Job of
kind `validation-step` under the hosted checkout, found or created by its execution id
(`external.system = validation-step`). The runner holds its lease, renews it at a third
of the lease length while the step runs, and ends it as `succeeded` for exit 0,
`failed` otherwise, or `cancelled`, with `end`, `exit_code` and `duration_ms` refs. The
hosted-validation registry stays the record that keeps an execution from running
twice; the Job only mirrors it. No output enters the Job, which declares no actions.

## 33.10 File watchers and change waiters (HS2-N1QDZW)

Every wait in the command-line runtime that wakes on a file change starts its
platform file watcher through `hotsheet_jobs::watch::PathWatcher`. It takes all of
the waiter's paths, each with its own depth, registers them, and then starts the
watcher once. `PathWatcher::add` registers one more path later; the worker view
uses it for a settings directory that did not exist when the wait began.

The reason is the macOS backend. A watcher there is one event stream for all of
its paths, and starting or stopping a stream is a call into the system file-event
service. The watcher library restarted the stream for each path, so a
`coordination wait` over four paths made four starts and three stops before it
reported readiness. The measurements and the trace are in
[file-watcher-start-cost-2026-11-12.md](file-watcher-start-cost-2026-11-12.md).

- **Callers.** `coordination wait` (`Coordinator::next`), the worker view watch,
  and `ChangeWaiter`, which the run event feed, the scheduler follower, the
  monitor and the Job store waits use.
- **Long-lived watchers (HS2-654VBY).** The server's repository watcher, its
  orchestration-settings watcher, the Claude transcript follower and the Codex
  rollout follower start through `PathWatcher` too, so `hotsheet-aitools` depends
  on this crate; a separate watcher crate was not worth its build plumbing for one
  module. `PathWatcher::change` stops and starts paths in one registration, which
  the Codex follower uses as it moves from its home to `sessions/` to the rollout.
  Platform registrations, before and after, for one start of each:

  | Watcher                                                          | Before            | After |
  | ---------------------------------------------------------------- | ----------------- | ----- |
  | Repository, linked worktree (root, Git dir, common dir)          | 3                 | 1     |
  | Repository, ordinary checkout                                    | 1                 | 1     |
  | Claude transcript, session directory present                     | 2                 | 1     |
  | Codex rollout, home then `sessions/` then rollout                | 5                 | 3     |
  | Orchestration settings, every directory present                  | one per directory | 1     |
  | Orchestration settings, each event while `.hotsheet2` is missing | one more          | 0     |

  The server keeps a native watcher of its own only for a ticket store, over one
  path, and polling watchers, which have no stream to restart; a policy test pins
  this.

- **Platforms.** macOS uses `notify` 8.2 (`notify-batched` in the workspace
  manifest), whose `paths_mut` registers several paths before one start. Linux
  and Windows watch each path separately and have no stream to restart, so they
  keep `notify` 6 and its per-path call; their code path did not change. The
  event types `hotsheet_jobs::watch` exports are those of the library in use.
- **`ChangeWaiter`.** It watches one or several directories recursively and wakes
  on a content change; reads and lock files are not changes. A change that
  arrives between two waits ends the next wait at once, and several events of one
  write are one wake. Every wait also ends at a one-second local re-check, which
  reads files and sends no request. A waiter whose platform watcher could not
  start waits out that re-check instead of returning at once.
- **Followers keep their waiter.** `JobStore::wait_for_change` starts a waiter
  for one call. A caller that waits step after step passes a `KeptWaiter` to
  `JobStore::wait_for_change_keeping`, which keeps the waiter of that checkout
  directory between calls: `wait_for_gate_change`, the orchestration gate
  follower (`GithubActionsGates`) and the speculation gate probe do. The kept
  waiter is replaced when the directory was removed and created again or when
  another checkout is asked for, and a second thread that waits through the same
  `KeptWaiter` at the same moment uses a waiter of its own. Windows has no
  directory identity to compare, so nothing is kept there.
- **Never waiting on the platform (HS2-F6F42W).** On a loaded Mac the file-event
  service delivered events in 3 to 14 ms but took up to 7 s to start a stream, 4
  to 19 s to restart one (adding or moving a path) and up to 46 s to stop one; the
  `notify` 8 backend's stop also calls `FSEventsPurgeEventsForDeviceUpToEventId`, a
  device-wide call that serializes every stop on the machine. So the waits and
  followers start through `hotsheet_jobs::watch::BackgroundWatcher`, which starts,
  changes and stops the stream on its own thread and returns at once. It reports
  `WatchStatus::Live` once a start or change has taken effect, and the owner reads
  again then, because the stream did not report what changed before. A refused
  start or change is retried with a bounded backoff (100 ms doubling to 5 s),
  reporting `WatchStatus::Refused` at each attempt so the owner keeps reading;
  nothing gives up silently. Until the stream is live, `BackgroundWatcher::recheck`
  asks the owner to read again every second (local files only), as `ChangeWaiter`'s
  safety re-check does, since no event reports a change meanwhile. Every owner reads once before the stream exists:
  - `ChangeWaiter::starting` (HS2-WD475E) and `ChangeWaiter::watching`, which
    still returns only once the stream is live or refused once, share it; going
    live ends the next wait, and dropping a waiter never waits for the stop;
  - `coordination wait` and the worker view watch read at once and treat going
    live (or a refusal) as a rescan event (`watch::rescan_event`);
  - the Codex rollout follower moves its subscription without waiting, and a
    dropped follower releases its callback at once;
  - the Claude transcript follower (HS2-PTK0CK) keeps its single recursive macOS
    stream and leaves retries to the shared watcher;
  - the orchestration-settings watcher queues new directories on the shared
    watcher, which replaced its own restart thread;
  - a target-store watch records every ticket file modified since it was asked
    for once its stream is live.
    The server's repository watcher already starts on a thread of its own behind its
    Git-status bridge and keeps that design. Avoiding the device-wide purge on stop
    would mean patching or forking `notify`, which was not done; the measurements
    and the options are recorded on HS2-F6F42W.
- **Not covered.** The server's ticket watcher and the Claude and Codex usage
  followers still call the library per path (HS2-654VBY). The workspace
  carries two versions of the library until it can move as one (HS2-7JKZET).
