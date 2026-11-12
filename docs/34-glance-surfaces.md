# 34 · Glance surfaces: standby, sidebars, and stats (HS2-3AA1N7)

**Status: partial.** Each section states its own status; §34.7 (grid sidebar pages) is shipped. It records the design the maintainer
approved on the planning ticket HS2-CT5EWR over four rounds of annotated mocks, so the implementation
tickets reference one requirements source instead of ticket notes. Each requirement names the ticket that
builds it (§34.11). When a ticket ships, update its section's status here and in
[README.md](README.md).

A **glance surface** answers "is everything fine, and if not, which project needs me?" without the user
opening a ticket or a terminal. There are five: standby, the project sidebar, the workspace grid sidebar,
the Stats page's Right now section, and the away digest shown on leaving standby.

All mocks named below are attachments on HS2-CT5EWR. They were made by injecting the proposed regions into
the real `/ux-demo?component=app-shell` screens with synthetic fixtures; the mock generator scripts
(`hs2-ct5ewr-mock-generator*.mjs`) are attached beside them.

## 34.1 Dark-cockpit principle

When everything is fine, a glance surface is quiet. Brightness, tint, and motion mean that something needs
attention, and nothing else earns them.

- Normal states render dim or neutral. A problem state brightens only the row, chip, or callout that
  carries it; the rest of the surface stays as it was.
- Standby is dark red on solid black. It encodes severity by brightness and glyph shape, and uses **amber**
  (low in blue light) for alerts. Amber was approved with mock `m5`.
- **Likely-critical conditions keep a full-width callout**, even on a compact surface (for example a disk
  about to fill). This size was approved with mock `m1`.
- Host conditions show one "all normal" line until a threshold is crossed, then one callout per crossed
  condition (§34.8).

## 34.2 Live-state vocabulary

Every glance surface uses the project states that notifications already use
([32-notifier-plugins.md](32-notifier-plugins.md) §32.8.1), plus one new state. Standby, the sidebars,
Stats, and phone notifications therefore always agree.

| State          | Meaning                                                                                                                                                                                      | Source                 |
| -------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------- |
| Active         | At least one ticket in the store holds a live claim.                                                                                                                                         | §32.8.1 (shipped)      |
| Stopped        | The last claim was released and no new claim appeared within the grace. Stopped-as-expected rows render dimmed.                                                                              | §32.8.1 (shipped)      |
| May be stuck   | Claims are held but nothing progressed for the idle window.                                                                                                                                  | §32.8.1 (shipped)      |
| Stalled        | A claim's lease expired without renewal.                                                                                                                                                     | §32.8.1 (shipped)      |
| Halted         | A terminal whose worker holds a claim reported a halt.                                                                                                                                       | §32.8.1 (shipped)      |
| Waiting on you | A Started ticket waits on an unanswered feedback request. Review flags, review requests, and tickets outside the Started column never count (HS2-JGW031). Needs you counts the same tickets. | HS2-JTTQMY, HS2-JGW031 |

Two **stuck causes** refine May be stuck and Halted so a surface can say why nothing is happening
(HS2-EGKKA0; the data ships in the live state as `activity.causes`, §32.8.4, and no surface renders it yet):

- **Usage-limit pause:** an AI tool reported a usage or rate limit. When the tool reports a reset time, the
  surface shows the resume time in place of a stuck state. When it does not, the reset is unknown and the
  surface says "paused at a usage limit" without a time. This extends the `terminal_halted` signal.
- **Awaiting input:** a terminal session waits at a permission or approval prompt. The surface links
  straight to that terminal.

The state word does not change for a cause, so notifications and clients keep one vocabulary; a surface
shows the cause's text in place of the word it refines.

## 34.3 Live values, rates, and gauges

The per-store live state model (HS2-JTTQMY, design only) is the one data source for every glance surface.
For each store it carries the state above and:

- the time of last progress, using the idle detector's progress definition;
- live claims with ticket, phase, and ETA overrun;
- workers live versus expected;
- `main` CI status;
- two rates, each with a rolling 24-hour peak (HS2-7CS5A4; designed as 30 days).

Queue runway (the ready Up Next count and an ETA-based estimate) was removed from the model and every
glance surface (HS2-5A83MR).

The rates are:

- **Tokens per minute**, from the metrics usage events ([14-metrics-interface.md](14-metrics-interface.md)) that each
  project's terminals record in that project's own store (HS2-B4EQ87). Claude and Codex
  terminals record per model response while a turn runs (HS2-SSMDQ1, HS2-CYHAWB). A turn the server's
  opt-in driving loop runs moves its ticket store's tokens per minute when the turn's usage is recorded (HS2-P37AH7).
- **Tickets updated per hour:** distinct tickets with a progress write in the last 60 minutes. **Claim
  renewals do not count**, matching the idle detector (§32.8.1), so a session that only renews looks idle.

**Gauge semantics.** One glance gauge component (HS2-CVE3SK, design only) implements the speedometer HS2-PZPD83
asked for: a needle rotating over an arc, scaled to that value's own 24-hour peak, and pinned at the end of
the arc above the peak.

- **Blue = tokens/min, green = tickets updated/h**, from Kerf tone tokens. On standby both are dark red.
- **Full variant:** value and unit inside the dial. Used on Stats Right now and, very large, in standby.
- **Bare variant:** arc, fill, and needle only. Value, unit, and 24-hour peak are in the title and the
  accessible name. Used at chip height (about 38 px) in the compact project sidebar and at about 84 px on
  the grid sidebar's gauge page.
- The needle animates only when the value changes and respects `prefers-reduced-motion`.

**Scope** follows HS2-PZPD83: on a project tab a surface shows that project; on All Projects or the
workspace grid it shows all projects. **All projects** always means the projects whose tabs are open in
the app, never every store the server hosts or has ever loaded (HS2-JGW031). Standby always shows all
projects (§34.5).

**Delivery.** The model is pushed over the existing WebSocket change stream with cursor replay. There is no
fixed-interval client polling. The server rebuilds every project's model at least once a minute and pushes it
when it changed, so a quiet project decays to zero rather than freezing at its last rate: no progress is an
important signal (HS2-MHW3SX). Clients compute ages locally from timestamps, and local display timers never
issue requests.

**Restarts.** A server restart does not zero the model. Tickets updated, the active period, and halts are
kept per store in machine-local `metrics/local/live-state.json` and restored best effort,
falling back to what the tickets record themselves (HS2-MHW3SX, [32-notifier-plugins.md](32-notifier-plugins.md)
§32.8.3).

## 34.4 Compact-surface rule

Compact glance surfaces show **glyphs, gauges, and raw numbers only**. Every explanation, unit, and
duration goes in the hover title and the accessible name, never on the face. For example, "no progress for
34 minutes" is the title of a May be stuck glyph, not visible text. Chips and rows appear only when they
have something to say. The full-width callout for likely-critical conditions (§34.1) is the one exception.

## 34.5 Standby (HS2-EQVZTZ, shipped)

The original request on HS2-CT5EWR. Mocks: `m4-standby-all-clear-desktop`,
`m5-standby-attention-desktop`, `m6-standby-phone-project`.

- **Entry:** after no direct human interaction for a threshold. An app-level setting turns it on or off and
  sets the thresholds; the setting has CLI and REST parity like other app settings. A **moon** button
  beside the left sidebar rail's collapse toggle forces entry (approved with `m1`).
- **Exit:** any click, tap, or key.
- **Display:** solid black background; a very large dark-red full gauge for tokens/min; the count of
  tickets updated in the last 60 minutes; a clock; a dim per-project state strip; and a freshness line that
  says when the change stream has dropped, so a frozen screen never looks healthy.
- **Alerts:** Stalled, Halted, and Waiting on you show an amber callout and light that project's row in
  the strip (`m5`). Nothing else brightens.
- **Scope:** always all open projects, whichever tab or page it was entered from (HS2-JGW031). The
  approved `m6` phone mock showed one project; the maintainer replaced that rule.
- **Devices:** request a Screen Wake Lock on phones and tablets, drift content slowly against OLED burn-in,
  and respect `prefers-reduced-motion`.
- **Requests:** standby issues no requests of its own; it renders the pushed live state model.

**As shipped.**

- **Settings.** `standby.enabled` (default on) and `standby.idle_minutes` (default 15, 1–1440) are
  machine-wide settings on the server's host. Clients read and write them with `GET`/`PUT
/standby-settings`. `hotsheet-cli settings set standby.<name> <value>` validates the value and
  always writes the global scope. In the app they are App Settings → General → Standby.
- **Settings changes reach open clients (HS2-A5BTTK).** Every change pushes a cursored
  `standby_settings` change event (`store` empty; it carries no values). A `PUT /standby-settings`
  emits it itself; `hotsheet-cli settings set standby.*` announces its write to every running server
  on the machine with `POST /standby-settings/changed`, which emits the same event. Clients re-read
  `GET /standby-settings` on each new event (cursor-deduped across project streams, coalesced) and
  on change-stream resync, then re-arm the idle timer from the last activity with the new threshold.
  Nothing polls. A REST write on one server does not reach clients of another server process on the
  same machine until their next resync.
- **Idle entry.** Idle means no pointer, key, wheel, or touch input. One local timer counts it and
  never makes a request.
- **Moon button.** It sits directly before the left rail's collapse toggle. While the rail is
  collapsed, it moves with the toggle into the workspace toolbar.
- **Exit.** A click or tap anywhere on the display exits. Any key also exits, and the app beneath
  never receives that key.
- **Scope.** Always every open project, entered from any tab or page (HS2-JGW031).
- **Freshness.** The freshness line reads the health of each project's change stream. A dropped
  stream turns the line amber ("connection lost · last update …") once it has stayed down past a
  20-second reconnect grace. A failed refresh after an event is not a dropped stream.
- **Layout.** Strip rows stay in name order, so a problem lights up in place and never moves.
- **Devices and motion.**
  - On touch devices, standby holds a Screen Wake Lock and takes it again when the page becomes
    visible.
  - The layout drifts a few pixels over 20 minutes, unless the user prefers reduced motion.
  - Portrait screens stack the gauge over the count.
- **Rendering.** The display is a manual-popover `<dialog>`. The top-layer watcher keeps it above
  every surface, including an open modal.

## 34.6 Project sidebar Live section (HS2-F788GW shipped; HS2-6B9S7P design only)

A **Live** section under the completion chart. Mocks: `r3-project-sidebar-compact-{light,dark}` (compact,
the default) and `m1-project-sidebar-live-{light,dark}` (expanded).

- **Compact:** one line with two bare gauges (blue tokens/min, green tickets updated/h), then icon chips:
  Needs you count, ETA overrun, `main` CI. Chips appear only when non-trivial; the queue runway chip was
  removed (HS2-5A83MR, TWD45K #6). A
  likely-critical callout (for example disk, §34.8) keeps its full width, 16px below the gauges and badges in
  both modes (HS2-0EQMZZ, the Kerf `List` `m` gap; several callouts stay 8px apart).
- **Expanded:** the two gauges centred horizontally (HS2-80SWPT, through the Kerf `Row` `hAlign` prop), then the detailed rows of `m1`: work state and last-progress age, Needs you, overdue ETA, `main`
  CI, and a machine warning only past a threshold. The queue runway row was removed (HS2-5A83MR, TWD45K #5). The section has no
  header bar (HS2-9J6PPR, TWD45K #1/#2): clicking anywhere on its gauges, chips, or rows switches modes.
  That content is one native button, so Tab and Enter or Space reach it too, and its accessible name
  states the mode it switches to ("Show live details" or "Show compact live summary") followed by the
  work-state sentence. Gauge and chip hover titles still show. The choice persists per viewer. Host
  callouts are not part of the toggle.
- **Shipped (HS2-F788GW).** `ProjectLiveSection` renders the compact line and the expanded rows from the
  live state of the project's linked stores and the host health snapshot, both pushed and resynced on the
  change stream. Ages and decaying rates re-render from a local 30-second render clock that runs only while
  live state exists and never issues a request. The compact problem-state chip (May be stuck, Stalled,
  Halted) is a glyph whose title gives the state and the last-progress age. The `main` CI chip and row wait
  for a CI source (HS2-M8HHEA).
- **Workers badge (HS2-6B9S7P):** the live worker count is a badge on the **Workers** view button, hidden
  at 0. The button's title and accessible name carry the worker count and the time since last progress.
  The Workers button is a choice of the toolbar `SegmentedControl` view switcher, which Kerf prescribes
  for an exclusive view switch, so the `ToolbarButton` badge from `KF-HDQTVV` does not apply. The
  accessible name (`Workers View, 2 live`) and title (`2 workers live · last progress 2m ago`) shipped
  with HS2-6B9S7P; the visible badge waits on the Kerf gap `KF-170258` (a badge on a `SegmentedControl`
  choice), and no app CSS positions one (§34.10).

## 34.7 Workspace grid sidebar pages (HS2-E1AQE8, HS2-EZSQDF, shipped)

**Status: shipped.** The shared group and the three page bodies (HS2-E1AQE8,
`clients/web/src/components/terminal-operations-sidebar.tsx`, `grid-sidebar-*.tsx`,
`clients/web/src/grid-sidebar-live.ts`) page in production through Kerf `PagedView` (HS2-EZSQDF,
`clients/web/src/features/terminal-operations-surface.ts`), fed by the pushed live state.

The grid sidebar keeps today's completion charts and becomes swipeable pages. The maintainer chose these
pages (option A) over a single integrated line.

1. **Completion** (default, unchanged): `r3-grid-sidebar-page1`.
2. **Gauges:** `r4-grid-sidebar-page2-{light,dark}`. The same list header per group as page 1 (All
   projects, then each project) and the same divider under All projects. Each group's body is a pair of
   bare gauges (tokens/min, tickets updated/h) with the project's state glyph at the row end. When All
   projects shows (two or more open projects), each project's gauges read against the All projects scale
   (the sum of the open stores' 24-hour peaks) so their needles compare, and their titles name that
   scale; All projects and a lone project use their own (HS2-VFQM4D). The completion charts already
   share the All projects maximum (HS2-C9JM65).
3. **Triage:** `m2-grid-sidebar-projects`. One row per project with state glyph and state word, current
   ticket and phase, sorted by need, stopped-as-expected rows dimmed, durations on hover only (§34.4).
   The All projects group lists Needs you (only when non-zero) and workers live versus expected. Both count
   only the stores of loaded projects whose tabs are shown, each store once (HS2-0ZVS5W). Expected is each
   store's coordination worker plan when a dispatcher pool runs one, so a registered worker that never
   claimed reads as missing (HS2-BHPPAS); otherwise its distinct workers that held a claim in its current
   active period (§32.8.3), so finished workers still count.

Before a project's live state arrives, its gauges rest at zero without a glyph and its triage row
reads "No live state". Every body opens that project's statistics, as the completion chart does.

All pages share one group component (header, divider, and a per-page body slot), so project order,
alignment, and scroll position match across pages (HS2-E1AQE8).

**Paging (HS2-EZSQDF):** swipe, horizontal trackpad scroll, dot click, and arrow keys change pages. The page
dots float **fixed at the bottom of the sidebar** above scrolling content (approved with
`r3-grid-sidebar-page1`). The selected page persists per viewer; Completion is the default. Paging uses
Kerf `PagedView` (`KF-0QJ2K3`), which jumps without animation under `prefers-reduced-motion`.

The page is stored in the viewer's `localStorage` (`hotsheet.terminals.sidebar-page`); an unknown or
unreadable value shows Completion. Each page scrolls on its own beneath the dots. Gauge rates decay
with time, so a local minute clock re-renders them while any store reports live state; it never
issues a request. A store linked by several open projects counts once in All projects.

## 34.8 Stats Right now (HS2-ERTNJ7, HS2-NS0AYR, HS2-9AN4DN shipped; HS2-DTAVQA partial)

A **Right now** section above the existing "Ticket flow and usage" on the Stats page, approved as
`m3-stats-right-now-{light,dark}`. Scope follows §34.3.

- **Tiles (HS2-ERTNJ7):** full tokens/min gauge; tickets updated in the last 60 minutes with a sparkline;
  completed since a period start; Needs you (tinted when non-zero); workers live versus expected; `main` CI.
- **Projects table (HS2-ERTNJ7):** state glyph and word, project, current work (ticket and phase, including
  final-testing tickets waiting on CI), last progress, ETA overrun, a 12-hour token sparkline (the queue runway column was removed by
  HS2-5A83MR),
  and the one action that unsticks it (Open terminal, Answer). Sorted by need, with a "data as of"
  freshness line.
- **Shipped (HS2-ERTNJ7).** `StatsRightNow` and `StatsProjectsTable` render from the pushed live
  state; a 30-second local display clock ages values only while Stats is shown and never requests.
  "Data as of" is when the client last applied a snapshot. Completed counts today's UTC calendar day
  from the analytics the page already loads, labelled with that midnight's local time. One part waits
  on the live state: the `main` CI tile is omitted until HS2-M8HHEA adds CI. The Tokens 12h column draws
  a `GlanceSparkline` of the snapshot's 12 hourly token buckets (HS2-TRZGT3), aligned to the display
  clock, with the current tokens/min beside it.
- **Overnight timeline (HS2-9AN4DN):** one swimlane per project over a selectable window (default: since
  standby was entered, or the last 12 hours), segments coloured by state (Active, Waiting on CI, May be
  stuck, Waiting on you, Stopped, Stalled, Halted), and a tick per completed ticket. Hover shows state,
  start, end, and duration. It needs persisted per-store state transitions: compact append-only records
  with bounded retention, served over REST with CLI parity.
- **Shipped (HS2-9AN4DN).**
  - **Record format.** Each time a store's published live state changes its timeline state, the server
    appends one line `{"v":1,"at_ms":…,"state":…}` to the store's machine-local, gitignored
    `metrics/local/live-history.jsonl` (`hotsheet_ticketing::live_history`).
    - The state is the live word, or `waiting_on_ci` while every live claim of an active store is in
      `final_testing`. That is the stand-in for Waiting on CI until a CI source exists (HS2-M8HHEA).
    - An unchanged state writes nothing. A time before the last record is clamped to it.
    - Past 4096 records, the next append rewrites the file. It drops records older than 7 days but
      keeps the newest older one, so a window's opening state stays known.
    - Readers skip torn lines and unknown versions.
  - **REST.** `GET /live-history?since_ms&until_ms` replays every hosted store's records into
    segments clipped to the window, merging equal neighbours. Time before the first record is left
    empty. It also adds the completion ticks inside the window, taken from each ticket's own history.
    The default window is the last 12 hours, clipped to now and to the retention.
  - **CLI.** `hotsheet-cli live-history [--hours N]` prints the same lane for the selected store
    straight from its files, with no server.
  - **Card.** The card sits under Right now, above This Mac.
    - The window select offers Since Standby (once standby was entered in this session, and then the
      default), Last 12 Hours (otherwise the default), and Last 24 Hours.
    - The history is read when Stats opens, when the window changes, and on a change-stream resync.
      Pushed snapshots extend the last segment between reads; the display clock never requests.
    - Lanes are in name order. Each segment's and tick's title gives the state or ticket, start, end,
      and duration. The legend lists the states, and shows Stalled and Halted only when a lane has them.
- **This Mac cards (HS2-NS0AYR):** disk free with rate, time to full, and the largest growing directory
  when known; memory pressure with swap trend; CPU with a sparkline and top process; power and sleep (kept
  awake or at risk); network and thermal only when degraded. Cards past a threshold take the warning tone.
  The group is labelled with the server's host name, because the viewer may be remote.
- **Shipped (HS2-NS0AYR).** `StatsThisMac` sits below the Projects table and above Ticket flow and
  usage, headed by the host name with "Server host · platform · sampled time". Disk, memory, CPU,
  and power always show; network and thermal only while warning or critical. Crossed cards take
  the warning or danger tone; `unsupported` and `unavailable` signals say so and show no value. The
  runtime creates one host-health resource fed by every project stream's `host_health` events and
  resyncs. The CPU card draws a `GlanceSparkline` of the snapshot's last hour of five-minute
  averages on a fixed 0 to 100 scale, from the first sampled bucket (HS2-RBFR94; `docs/host-health.md`).
  While Stats shows, the CPU card follows the live CPU feed (HS2-CPFBWP): the busy percent every two
  seconds, the core count, and a per-core LED meter (`CpuCoreMeter`), long-polled only while Stats is
  visible.
  The largest growing directory waits on a bounded, private measurement (HS2-KK02QB); the left column
  beside the cards is reserved for the Overnight timeline.
- **Host health sampler (HS2-DTAVQA):** a server-side sampler of those signals pushes threshold crossings
  and coarse snapshots over the existing WebSocket; sampling cadence is server-local and clients never
  poll. Thresholds are configurable with CLI and REST parity, and use hysteresis so they do not flap. macOS
  is first; other platforms report unsupported signals explicitly. The sampler also feeds the sidebar
  callouts (§34.6) and the away digest.

## 34.9 While-you-were-away digest (HS2-JJ9RYN, shipped)

Shown on leaving standby, approved as `m7-away-digest-light`, composed with Kerf's dialog component.

- The time range and duration spent in standby.
- Three tiles: Completed, Need you (tinted when non-zero), and tokens with cost.
- One row per event since standby began, each with the single action that resolves it: a project stalled
  (Open), waited on you (Answer), paused at a usage limit with its resume time, a host threshold crossed
  (for example a disk dip), a queue finished.
- Actions: Dismiss and "Show what needs me".
- An uneventful standby exits silently, with no digest.
- Sources: the live state transitions (HS2-JTTQMY), host health events (HS2-DTAVQA), and the activity
  narration digest ([15-activity-narration-interface.md](15-activity-narration-interface.md)) where useful.

**As shipped (HS2-JJ9RYN).**

- **Recording.** While standby shows, the standby controller diffs each pushed live-state snapshot and
  host-health snapshot it already receives (`clients/web/src/away-digest.ts`). It issues no request.
  It notes:
  - a newly stalled claim (Open);
  - a ticket that newly waits on you (Answer);
  - a usage-limit pause, with its reset time, or how long it lasted once it cleared;
  - a host signal that rose to warning or critical, with free disk, battery, or CPU busy where known;
  - a store that went Active to Stopped, every claim released ("finished its run", with the number of
    tickets claimed during that run). The live state no longer carries queue runway (HS2-5A83MR), so the
    row does not say whether Up Next still holds ready work.
    A store first seen during standby counts from its first snapshot. The activity narration digest is
    not used: these rows already cover what it would add.
- **Tiles.**
  - Completed counts the completion events inside the standby window, deduplicated across projects
    that share a store. It comes from each open project's `GET /checkouts/{id}/confidence-report`,
    read once when standby exits; a failed read counts none.
  - Need you is the waiting count at the moment standby exits.
  - Tokens sums the distinct per-minute token buckets the pushed snapshots carried during the window.
    A gap of more than the snapshot's 5-minute window between snapshots undercounts. The live state
    carries no cost, so the tile shows tokens only; the cost follow-up is HS2-9V4CGN.
- **Silent exit.** The digest shows only when some row, completion, or token was recorded; otherwise
  standby exits silently. Re-entering standby while the exit's completions load drops that digest.
- **Dialog.** Kerf `DialogSurface`, named "While you were away" and described by the time range. Need
  you takes the warning appearance while non-zero, as on Stats. Rows are oldest first, each with its
  Lucide glyph from `live-state-icons.ts`. Open and Answer open the ticket in its project through the
  linked-ticket search. "Show What Needs Me" (the mock's "Show what needs me" in Kerf's title-case button style) opens cross-project Stats and appears only while
  something needs you. Dismiss, the close button, and Escape close it.

## 34.10 Icons and Kerf gaps

**State icons (HS2-CSPKQX).** Glyphs are official Lucide assets through the shared icon component. The
maintainer standardized this set on 2026-11-06 (May be stuck chosen over `clock-alert` and `timer-off`; the
rest kept as in the approved mocks). Every glance surface takes them from
`clients/web/src/live-state-icons.ts`, pinned by `live-state-icons.test.ts`.

| State                | Lucide icon     |
| -------------------- | --------------- |
| Standby button       | `moon`          |
| Active               | `activity`      |
| Waiting on you       | `hand`          |
| May be stuck         | `hourglass`     |
| Stalled              | `octagon-alert` |
| Halted / usage limit | `zap-off`       |
| Stopped              | `circle-pause`  |

**Kerf capabilities.** Both gaps filed for these surfaces shipped in Kerf 5.0.0-beta.124, adopted by
HS2-GAJSX9:

- `KF-0QJ2K3`: PagedView, swipeable panes with bottom-floating page dots (HS2-EZSQDF). A styled
  `wa-carousel` is not a substitute; it also sits behind the Web Awesome license decision.
- `KF-HDQTVV`: a count badge on `ToolbarControlGroup` buttons. The Workers view button turned out to be a
  `SegmentedControl` choice, so HS2-6B9S7P needs `KF-170258` (open): the same badge on a
  `SegmentedControl` choice. It would also retire the hand-positioned Notifications count
  (`view-mode-switcher__badge`, HS2-X9EMBF).

## 34.11 Ticket map

| Requirement                                                   | Section    | Ticket     | Depends on              |
| ------------------------------------------------------------- | ---------- | ---------- | ----------------------- |
| This requirements document                                    | all        | HS2-3AA1N7 | none                    |
| Per-project live state model, rates, 24-hour peaks, push      | §34.2–34.3 | HS2-JTTQMY | none                    |
| Live state kept across restarts, minute rebuild and decay     | §34.3      | HS2-MHW3SX | HS2-JTTQMY              |
| Glance gauge, full and bare variants                          | §34.3      | HS2-CVE3SK | none                    |
| Standby mode                                                  | §34.5      | HS2-EQVZTZ | HS2-JTTQMY, HS2-CVE3SK  |
| Standby settings pushed to running clients                    | §34.5      | HS2-A5BTTK | HS2-EQVZTZ              |
| Project sidebar Live section                                  | §34.6      | HS2-F788GW | HS2-JTTQMY, HS2-CVE3SK  |
| Workers view button badge                                     | §34.6      | HS2-6B9S7P | HS2-JTTQMY, `KF-170258` |
| Grid sidebar shared groups, gauge and triage page bodies      | §34.7      | HS2-E1AQE8 | HS2-JTTQMY, HS2-CVE3SK  |
| Grid sidebar paging with floating dots                        | §34.7      | HS2-EZSQDF | HS2-E1AQE8, `KF-0QJ2K3` |
| Stats Right now tiles and projects table                      | §34.8      | HS2-ERTNJ7 | HS2-JTTQMY, HS2-CVE3SK  |
| Host health sampler and push channel                          | §34.8      | HS2-DTAVQA | none                    |
| Stats This Mac cards                                          | §34.8      | HS2-NS0AYR | HS2-DTAVQA, HS2-ERTNJ7  |
| Stats overnight timeline with persisted transitions (shipped) | §34.8      | HS2-9AN4DN | HS2-JTTQMY, HS2-ERTNJ7  |
| While-you-were-away digest (shipped)                          | §34.9      | HS2-JJ9RYN | HS2-EQVZTZ, HS2-JTTQMY  |
| Stuck causes: usage-limit resume time, awaiting input         | §34.2      | HS2-EGKKA0 | HS2-JTTQMY              |
| Standardize the state icons                                   | §34.10     | HS2-CSPKQX | none                    |
| Speedometer gauge request (intake)                            | §34.3      | HS2-PZPD83 | covered by HS2-CVE3SK   |
