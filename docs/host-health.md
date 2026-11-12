# Host health sampler

Status: **partial**. The server sampler, push channel, REST and CLI configuration, and a client
resource have shipped (HS2-DTAVQA), and the Stats **This Mac** cards render them (HS2-NS0AYR). The
project sidebar's Live section callouts shipped with HS2-F788GW; the grid sidebar callouts are a
follow-up (HS2-E1AQE8). The approved
design is HS2-CT5EWR.

Related: [04 Core server and CLI](04-core-server-cli.md) (change bus, `/ws/sync`, `/ws/poll`),
[06 Clients](06-clients.md), and the change-event contract
[`contracts/v1/change-event.schema.json`](contracts/v1/change-event.schema.json).

## Purpose

When agents work unattended (overnight, or with the user away from the desk), the machine running
the server is a single point of failure. If it fills its disk, swaps itself to a crawl, sleeps
on battery, loses its network, or throttles, every agent on it stops. The sampler watches that
machine and tells clients when a threshold is crossed, so the glance surfaces stay a **dark
cockpit**: one "all normal" line until something is wrong, then one full-width callout per
condition. Likely-critical callouts keep their full width in compact mode.

Every value describes **the server's host**, which may not be the device showing it (for example
a phone). The snapshot therefore carries the host's name, OS and architecture, and surfaces label
the stats with that name.

## Signals

| Signal    | Reading                                                                                                                                                                                               | Warning                                                                         | Critical                                                                          | Reason codes                                                               |
| --------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| `disk`    | Free and total bytes of the worst monitored volume (the store's, plus [chosen disks](#monitored-disks)). Consumption is a robust trend (see [Disk trend](#disk-trend)), with projected hours to full. | free < `disk_warning_free_gib`, or hours to full < `disk_warning_hours_to_full` | free < `disk_critical_free_gib`, or hours to full < `disk_critical_hours_to_full` | `low_free_space`, `projected_full`                                         |
| `memory`  | OS memory-pressure level and swap in use. Swap growth is a least-squares fit over the last hour.                                                                                                      | OS pressure _warn_, or swap growth ≥ `swap_warning_growth_mib_per_hour`         | OS pressure _critical_                                                            | `pressure`, `swap_growth`                                                  |
| `cpu`     | Whole-machine busy percent, the busiest process's executable name and percent (never its arguments or path), and the last hour's history (see below).                                                 | busy ≥ `cpu_warning_percent` continuously for `cpu_sustain_minutes`             | —                                                                                 | `sustained_cpu`                                                            |
| `power`   | On battery, battery percent, lid closed, sleep-prevention assertion held, idle system-sleep timer.                                                                                                    | on battery; or idle sleep enabled with no assertion held                        | lid closed on battery; battery ≤ `battery_critical_percent` on battery            | `on_battery`, `idle_sleep_allowed`, `lid_closed_on_battery`, `battery_low` |
| `network` | Whether a default route exists, and its interface. No packet is sent.                                                                                                                                 | —                                                                               | no default route                                                                  | `no_default_route`                                                         |
| `thermal` | Whether the OS records thermal or performance throttling (macOS `CPU_Speed_Limit` below 100, or a recorded warning level).                                                                            | throttled                                                                       | —                                                                                 | `throttled`                                                                |

Each signal reports one status: `normal`, `warning`, or `critical`, or one of two non-alerting
states:

- `unsupported`: the platform has no reader for that signal.
- `unavailable`: the reader failed this tick, for example because a tool is missing or its output
  could not be parsed.

A non-alerting state never produces a callout and never shows a guessed value. Its metric fields
are `null`.

## Platforms

| Platform   | Disk                  | Memory                                                                                                                                  | CPU                                                     | Power                                                                                                                                                                       | Network                                                               | Thermal                                                                                    |
| ---------- | --------------------- | --------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ |
| macOS      | `statvfs`             | `sysctl kern.memorystatus_vm_pressure_level`, `vm.swapusage`                                                                            | `ps -A -o %cpu=,comm=`                                  | `pmset -g batt`, `pmset -g assertions`, `pmset -g`, `ioreg AppleClamshellState`                                                                                             | `route -n get default`                                                | `pmset -g therm`                                                                           |
| Linux      | `statvfs`             | `/proc/pressure/memory` (`full avg60` ≥ 10 is critical, `some avg60` ≥ 20 is a warning), `/proc/meminfo` swap                           | `/proc/stat` delta, plus `ps` for the top process       | `/sys/class/power_supply`, ACPI lid, blocking `systemd-inhibit` sleep requests, and `loginctl` idle action when available. Missing tools or policy remain unknown (`null`). | `/proc/net/route`                                                     | CPU thermal-throttle counter deltas when exposed; otherwise `unsupported` or `unavailable` |
| Other Unix | `statvfs`             | `unsupported`                                                                                                                           | `unsupported`                                           | `unsupported`                                                                                                                                                               | `unsupported`                                                         | `unsupported`                                                                              |
| Windows    | `GetDiskFreeSpaceExW` | `GlobalMemoryStatusEx`; pagefile usage from `Win32_PageFileUsage` when CIM is available. Windows memory-pressure level remains unknown. | `GetSystemTimes` delta; busiest process remains unknown | `GetSystemPowerStatus`, `powercfg /requests` and active idle-sleep policy. Lid state remains unknown.                                                                       | `GetBestRoute2` default-route verdict; interface index when available | `unsupported`                                                                              |

HS2-MEQ46T adds these readers. A missing native API, system utility, counter, or recognizable
policy output remains `unavailable` or `null`; it never implies a healthy reading. Windows
thermal and lid state have no reader. Linux throttling needs two samples to compare cumulative
counters; the first sample is `unavailable`.

## CPU history

`signals.cpu.history` (HS2-RBFR94) is the last hour of CPU as twelve five-minute averages of the
sampled busy percent, kept in server memory only:

- Buckets are aligned to the epoch (every client agrees on the boundaries), oldest first, ending with
  the bucket that holds the newest sample; `ends_at` is that bucket's end (`null` before the first
  sample).
- A bucket with no sample, because the server was down or the reader failed, is `null`, never a
  guessed value. A restart starts an empty history.
- A wall clock stepped backwards restarts the window rather than misplacing buckets.
- An unsupported or unavailable CPU reader keeps the existing history and adds nothing.

The history rides the snapshot reads below; it adds no change event.

## Disk trend

The disk consumption rate must not swing with one build's burst or an idle stretch
(HS2-Y2HGY9). It is the **Theil–Sen** slope of free space: the median of every pairwise slope
between samples. A burst that is written and later deleted, or one noisy sample, moves only the
pairs that touch it, so the median stays on the trend.

- **Window.** Up to six hours of samples, at most one point per minute whatever
  `sample_seconds` is (at most 360 points). A rate needs at least 3 points spanning 10 minutes.
- **Idle.** A flat or growing volume consumes `0` and projects nothing.
- **Sustained fast fill.** The recent rate is the smaller of the last hour's and the last 20
  minutes' robust slopes. When it is faster than the six-hour trend and alone projects the
  volume full sooner than `disk_critical_hours_to_full`, it replaces the trend, so a real
  emergency still calls out within minutes.
- **Swap growth** keeps the one-hour least-squares fit.

## Monitored disks

The `disk` signal used to describe only the volume that holds the primary store. Host health now
also watches other disks the user chooses (HS2-NHK8V8):

- **Detection.** Each sample lists the other local, writable, user-visible volumes:
  - macOS: mounts under `/Volumes` that are local, writable, and not hidden. Network shares,
    read-only images, and local Time Machine snapshots are skipped.
  - Linux: read-write block-device file systems, without boot, snap, or loop mounts, and each
    device once.
  - Other platforms detect nothing yet; only the store's volume and added disks count.

  The store's own volume is recognised by its device number and never listed twice.

- **Identity.** A volume is keyed by its OS volume UUID (macOS `diskutil info`, looked up once per
  mount; Linux `/dev/disk/by-uuid`), as `uuid:<UUID>`, else by its mount point, as
  `mount:<path>`. The key survives a removable drive's unplug and reconnect.
- **Choices.** A choice is a machine-wide setting, `host_health.disks` in
  `${HOTSHEET_HOME}/settings.json`. It maps each volume id to
  `{ "monitor": bool, "label", "mount", "manual"? }`, and a missing or unreadable value means no
  choices. A detected volume without a choice is `pending`: it shows its size, never calls out,
  and the Stats This Mac cards ask once whether to monitor it ("Monitor" or "Ignore").
  `PUT /host-health/disks` with `{ "id", "monitor" }` saves the choice and answers with the
  updated snapshot. The store's volume is always monitored (409), and an unknown id is 404.
- **Absence.** A saved volume that is not mounted stays in the list as not present, keeps its
  choice, and reports no values. When it reconnects under the same id it is monitored, or
  ignored, again without asking.
- **Evaluation.** Each monitored volume has its own robust trend (see [Disk trend](#disk-trend)),
  thresholds, margins, and three-sample recovery. The `disk` signal takes the worst committed
  volume, the store's on a tie, and shows that volume's values with `volume_id` and
  `volume_label`. Combining volumes adds no second recovery delay. A monitored volume that is
  unplugged takes its callout with it.
- **Snapshot.** `disks` lists the store's volume first, then each present volume, then each absent
  saved one. Each entry has `id`, `label`, `mount`, `store`, `manual`, `decision`
  (`included`, `excluded`, or `pending`), `present`, `status`, `reason`, and its values.
- **Events.** When a volume appears, leaves, or its decision changes, the sampler emits one
  `host_health` change event with `id: "disks"` and `slug: "changed"`. A saved choice emits the
  same event at once, so every client re-reads the snapshot. The event never carries a volume name
  or path.

## Hysteresis

The sampler is built so that callouts do not flap:

- **Escalation is immediate.** A sample that crosses a higher level commits at once, including a
  jump from normal straight to critical.
- **Recovery needs two things:**
  1. A margin. While a level holds, its free-space line moves up 10%, its projection line moves
     out 25%, the CPU line drops 5 points, swap growth must fall below 80% of the line, and the
     battery line rises 5 points.
  2. Three consecutive samples below the level (`RECOVERY_SAMPLES`). A relapse resets the count.
- **A failing reader** cannot clear a live callout until the same three samples have passed.

## Delivery

Sampling runs on the server only, at `sample_seconds` intervals. It reads local tools and files
and makes no network requests. Clients never poll.

- **Change events.** Each committed change into, out of, or between callout levels emits one
  durable, cursored change event on the existing bus. `/ws/sync` streams it and
  `/ws/poll?since=` replays it:

  ```json
  { "cursor": 812, "store": "", "kind": "host_health", "id": "disk", "slug": "warning", "message": "low_free_space" }
  ```

  The fields are:

  - `id`: the signal.
  - `slug`: the new status.
  - `message`: the reason code, absent when there is none.
  - `store`: empty, because the event belongs to the server, not to a store.

- **Snapshot notices.** A coarse snapshot notice (`id: "snapshot"`, `slug` = the overall level)
  goes out on the first sample. After that, it goes out only when the rounded values change:

  - free GiB
  - hours to full
  - swap in 256 MiB steps
  - CPU in 10-point steps
  - battery in 5-point steps
  - power source
  - reachability

  Notices are sent at most once every 5 minutes and never together with a crossing. An idle,
  healthy server therefore adds almost nothing to the event ring.

- **`GET /host-health`** returns the full snapshot (`HostHealthSnapshot` in the OpenAPI contract).
  A client reads it in three cases: on start, on each `host_health` event, and on change-stream
  resync (initial, reconnect, or overflow). The event payload stays minimal so that `ChangeEvent`
  needs no new field.
- **Client resource.** The web client resource is `clients/web/src/host-health-resource.ts`. The
  runtime creates one per client (every project reaches the same machine server through the
  bridge) and feeds it every project stream's events and resyncs (HS2-NS0AYR). It
  coalesces bursts into one follow-up read. It dedupes the same event cursor arriving through
  several project streams of one server, and it resets that cursor on resync, because a
  restarted server restarts its cursors.

Event values are metrics and stable codes only. No ticket, path, or customer content appears in
them.

## Live CPU

The snapshot's CPU reading refreshes only with the sampler and the snapshot notices above, so it
cannot feel live. While a client shows the CPU card, a separate live feed (HS2-CPFBWP) samples
per-core CPU every 2 seconds:

- **Readers.** macOS reads `host_processor_info` (no subprocess). Linux reads the `cpuN` lines of
  `/proc/stat`. Other platforms, Windows included, answer `unsupported`, and the client stops asking.
  The host health sampler being off (`HOTSHEET_HOST_HEALTH_SAMPLER=off`) also answers `unsupported`.
- **Long poll.** `GET /host-health/cpu?after=<seq>&timeout_ms=<n>` (`HostCpuFrame` in the OpenAPI
  contract) returns as soon as the server holds a frame newer than `after`. With no newer frame it
  waits up to `timeout_ms` (default 10 seconds, at most 25) and returns the current frame. A
  missing `after`, or one ahead of the server's (a restarted server), returns the current frame at
  once. The client sends its next request when the answer arrives, so the server's sampling, not a
  client timer, paces the requests.
- **Lifecycle.** The first request starts the fast sampler. The first frame compares two readings
  half a second apart, and later frames compare readings 2 seconds apart. The sampler stops 15
  seconds after the last request, so nothing samples while no one watches.
- **Frame.** A frame carries `seq`, `status`, `sampled_at`, `busy_percent` (the whole machine),
  `core_percent` (each logical core, in OS order), and `interval_ms`. Its `status` is:
  - `live`: the frame carries values.
  - `starting`: the sampler has one reading and waits for the next.
  - `unavailable`: the reader failed, so the values are empty, never guessed.
  - `unsupported`: there is no reader.
- **No ring traffic.** Frames are not change events. They never enter the cursored ring or
  `/ws/poll`, so the threshold-crossing replay is unchanged.
- **Client.** The web client (`clients/web/src/host-cpu-feed.ts`) long-polls only while Stats shows
  on a visible page. Leaving Stats or hiding the page aborts the request in flight. After a
  failure, or an `unavailable` frame, the client waits before asking again, starting at 1 second
  and doubling to at most 30 seconds. A good frame resets the wait. The This Mac CPU card shows the
  live busy percent, the core count, and a per-core meter (`CpuCoreMeter`). It falls back to the
  snapshot whenever the feed has no live frame.

## Thresholds

Thresholds are machine-wide settings in `${HOTSHEET_HOME}/settings.json`, and each has a default.
Use either interface:

- REST: `GET`/`PUT /host-health/thresholds`. The response holds `defaults`, `configured`, and
  `effective`. A `null` or missing field clears that threshold back to its default; an unknown
  name or an out-of-range value returns 422.
- Headless: `hotsheet-cli settings set host_health.<name> <n>`. The scope defaults to `global`, a
  project scope is refused, and the value is range-checked. Unreadable or out-of-range stored
  values fall back to the default.

The next sample applies a change.

| Setting (`host_health.` prefix)    | Default | Range        | Meaning                                           |
| ---------------------------------- | ------- | ------------ | ------------------------------------------------- |
| `sample_seconds`                   | 60      | 10 to 3600   | Server-local sampling cadence                     |
| `disk_warning_free_gib`            | 20      | 0 to 100000  | Warning below this many GiB free                  |
| `disk_critical_free_gib`           | 5       | 0 to 100000  | Critical below this many GiB free (checked first) |
| `disk_warning_hours_to_full`       | 24      | 0 to 8760    | Warning when projected full sooner (0 disables)   |
| `disk_critical_hours_to_full`      | 2       | 0 to 8760    | Critical when projected full sooner (0 disables)  |
| `swap_warning_growth_mib_per_hour` | 1024    | 0 to 1048576 | Warning at this swap growth (0 disables)          |
| `cpu_warning_percent`              | 90      | 1 to 100     | Busy line for sustained CPU                       |
| `cpu_sustain_minutes`              | 10      | 1 to 1440    | How long CPU must stay busy                       |
| `battery_critical_percent`         | 20      | 0 to 100     | Critical at or below this charge on battery       |

## Code

- `crates/hotsheet-ticketing/src/host_health.rs`: threshold keys, defaults, ranges, validation.
- `crates/hotsheet-server/src/host_health/`:
  - `mod.rs`: the evaluator, hysteresis, the sampler task, and `AppState::spawn_host_health_sampler`.
  - `source.rs`: the injected `HostSource`, platform readers, and pure parsers.
  - `routes.rs`: the routes.
  - `tests.rs`: the tests.
- `crates/hotsheet-cli/src/dispatch/setup.rs`: machine-wide validation for `settings set`.
- `crates/hotsheet-server/src/host_health/live_cpu.rs`: the live CPU feed and its long poll (HS2-CPFBWP).
- `clients/web/src/host-health.ts` and `clients/web/src/host-health-resource.ts`: the client
  contract and resource; `clients/web/src/host-cpu-feed.ts`: the live CPU long-poll loop.

## Tests

Test processes never sample the machine they run on. Each runner sets
`HOTSHEET_HOST_HEALTH_SAMPLER=off`, the same way it sets `HOTSHEET_AI_TOOL_PROBES`:

- `.cargo/config.toml` for every cargo and nextest process.
- `clients/web/scripts/test-ai-tool-isolation.mjs`, called by the Playwright and Vitest configs.
- `migrator/test/isolated-home.mjs` for the migrator suite.

A spawned server therefore never emits host-dependent events into a fixture's change stream.
Unit and route tests drive the sampler through an injected `HostSource`.
