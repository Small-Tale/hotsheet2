# File watcher start cost on macOS (2026-11-12, HS2-N1QDZW)

Multi-process coordination tests failed with `wait process readiness: Timeout` when
several test selections ran on one machine. A spawned `hotsheet-cli coordination wait`
did not print its readiness line within ten seconds. This records what was measured,
what the trace showed, and what changed. The behavior that shipped is described in
[33-jobs.md](33-jobs.md) §33.10.

Everything here was measured on one macOS machine (Apple Silicon). Linux and Windows
were not measured; their code path did not change.

## Method

The test selection was the orchestration selection of 1,720 tests (1,735 after the
change), run once, twice at once and four times at once from one worktree build. Other
agents ran their own selections on the machine during some runs; the tables say where.

Timing came from a temporary build that was never committed: copies of `notify` 6.1.1
and 8.2.1 that timed each call into the system file-event service, and a timer around
the wait start-up in `Coordinator::next`. Each process appended its numbers to one file.

## Trace

On macOS a file watcher is one event stream for all of its paths. `notify` 6.1.1 stops
the stream and starts a new one on every `watch` call. A waiter over four paths
therefore makes four starts and three stops.

| Step inside one `watch` call                      | Cost while one selection ran     |
| ------------------------------------------------- | -------------------------------- |
| Wait for the stream thread to idle before a stop  | 0 ms; at most 46 yields          |
| `FSEventStreamStop`                               | median 0.2 ms, p90 51 ms, 89 ms  |
| `FSEventStreamInvalidate`, `FSEventStreamRelease` | under 0.2 ms                     |
| `FSEventStreamStart`                              | median 61 ms, p90 167 ms, 330 ms |

- All of the cost is inside the two calls that start and stop a stream. Both are
  synchronous calls into the system file-event service.
- The cost grew with the number of selections running, and processor load alone does
  not explain it. With two selections the slowest start took 1.4 s while the machine was
  57% idle and the service used 8% of one core in the one sample taken. The ticket's
  author found that an unrelated build and twenty busy-loop processes changed nothing.
- A stand-alone process that started streams in a loop during the same selection saw
  starts of 0.2 ms at the median. This suggests that the slow starts belong to
  newly spawned processes that start their first streams together, which is what a
  burst of waiters is.
- That the service handles stream starts one at a time is an inference from these
  numbers. Tracing inside the service needs root and was not done.

## Before and after

`Watch set-up` is the time from creating the watcher until every path is registered.
`Ready` is the time until the readiness line, which adds registration and the first
snapshot. `Stream starts` counts every start in the run, including the watchers this
change did not touch.

| Runs at once | Build  | Stream starts | `FSEventStreamStart` median / p90 / max | Watch set-up median / max | Ready median / max |
| ------------ | ------ | ------------- | --------------------------------------- | ------------------------- | ------------------ |
| 1            | before | 176           | 61 / 167 / 330 ms                       | 36 / 261 ms               | 113 / 420 ms       |
| 1 (a)        | after  | 101           | 78 / 169 / 793 ms                       | 6 / 97 ms                 | 99 / 636 ms        |
| 2            | before | 352           | 71 / 485 / 1,417 ms                     | 83 / 273 ms               | 461 / 1,542 ms     |
| 2 (b)        | after  | 202           | 74 / 181 / 1,575 ms                     | 8 / 123 ms                | 632 / 1,260 ms     |
| 4            | after  | 395           | 119 / 420 / 4,649 ms                    | 22 / 362 ms               | 913 / 2,871 ms     |

(a) Another agent's selection ran during part of this run. (b) A third selection, from
another agent, joined during this run.

In the after rows the start timings are those of the migrated waiters (92, 185 and 363
of the starts). The other starts, from watchers this change did not touch, had medians
of 314, 428 and 1,449 ms.

- Watch set-up for a waiter fell from 36 ms to 6 ms at the median with one run and
  from 83 ms to 8 ms with two.
- The number of stream starts fell by 43%. The starts that remain are one per waiter,
  plus the server's and the usage followers' own watchers (HS2-654VBY).
- One start costs what it did before. The change removes starts; it does not make the
  service faster.
- Readiness as a whole did not get faster in these runs. Once the watcher is set up
  in a few milliseconds, registration and the first snapshot are nearly all of it, and
  they depend on how busy the machine is. The after runs had more load than the before
  runs.
- With four runs at once a single stream start took up to 4.6 s. A waiter that makes
  four starts and three stops in that condition can use up its ten seconds on them;
  with one start the slowest watch set-up was 0.4 s and the slowest
  readiness 2.9 s. There is no four-run measurement of the old build from this session:
  the ticket reported watch set-up of up to 3.3 s with four runs.
- No run in this session reached the ten-second failure, before or after. In the
  four-run case one test failed in one of the four runs,
  `hs2_42tara_server_routes_and_stdio_mcp_share_the_plan`, on a run event that had not
  been recorded yet; it reads the event feed without waiting and involves no file
  watcher (HS2-RZQFG1).

## Decision

- **Shipped.** `hotsheet_jobs::watch::PathWatcher` registers every path of a waiter
  before one start. On macOS it uses `notify` 8.2.1, which can batch path changes. Job
  store followers keep one waiter across their steps instead of starting one per step.
- **Not chosen: move the whole workspace to `notify` 8.** Version 7 added open events to
  the Linux watch mask. The worker view treats an event of any kind on a watched path as
  relevant, so its own read of `state.json` could wake it again. That cannot be checked
  on a macOS machine, so Linux and Windows stay on version 6 (HS2-7JKZET). The macOS
  backends of the two versions filter and translate events with the same code.
- **Not chosen: watch one common parent directory.** The machine home and a project
  usually share only the user's home directory. A recursive stream there delivers every
  file event of the account to every waiter.
- **Not added: a nextest group that caps waiting tests.** The measurements above do not
  show a need for it after the product change. The readiness timeout stays at ten
  seconds.
