# Browser UI responsiveness profile

This profile records why the web client can feel slow when several projects and
terminal or AI sessions are open. It is an investigation baseline for
`HS2-WDTN3W`; implementation work belongs to its linked follow-up tickets.

## Reproduce the trace

From `clients/web`, run `npm run profile:ui`. The opt-in Playwright scenario
opens two projects, keeps a 138-ticket main project warm, exposes 12 terminal
sessions, and exercises three switches in each direction before search, a
ticket update, terminal opening, AI chat creation, a sustained AI output
stream, and switches with activity present. It runs at 1440 × 900 and
760 × 900. The real application UI and state logic run in Chromium; HTTP,
WebSocket, and terminal data are deterministic, contract-shaped fixtures.
Run `npm run profile:pty` from the same directory for the `HS2-K7FJVK`
load scenario described below.
Run `npm run profile:pty:real` for the isolated local-server and OS PTY
scenario described below.
The 200-ticket real Git/server board test remains available through
`npm run test:real-world-performance` for storage and transport timing.

The run writes Chrome DevTools traces and interaction windows under
`clients/web/target/performance-traces/`. Run
`node scripts/summarize-ui-trace.mjs target/performance-traces/<trace>.json`
from `clients/web` to extract marked windows, renderer-main task time,
JavaScript calls, style update, layout, and paint. The raw JSON can also be
opened in Chrome's Performance panel. The summary categories are inclusive
event durations and should not be added together. The action windows include
Playwright dispatch and two frames, so compare renderer work and the app's
`hotsheet:interaction-timing` click-to-paint measurements rather than
treating their wall duration as user input latency.

These are development-build traces with tracing overhead on one machine.
Repeat on the same build and machine before comparing a change; use the
interaction timing events and a production build for final user-facing
latency targets. The synthetic transport keeps this profile focused on the
browser main thread and cannot establish live server, network, or disk
latency. The default `profile:ui` terminal data describes session presence;
`profile:pty` adds a binary WebSocket output stream.

## PTY output with concurrent workspace activity

`npm run profile:pty` keeps the same two projects, 138-ticket main board,
12 terminal sessions, search/update flow, and AI chat. With a terminal
viewport visible, it sends 600 binary PTY frames (1,161,600 bytes) and
30 concurrent AI events while searching, updating Up Next, and switching
projects at 1440 × 900 and 760 × 900. It writes separate
`hs2-k7fjvk-<viewport>-chromium-trace.json` and
`hs2-k7fjvk-<viewport>-interaction-windows.json` files in the trace
directory above. Run the same trace summarizer for the `pty-load` and
`pty-*` windows. The three pre-load switches in each direction provide a
same-run idle comparison.

| Single Chromium run                           |    Desktop |     Narrow |
| --------------------------------------------- | ---------: | ---------: |
| Idle switch to one-ticket project, three runs | 136–186 ms | 162–205 ms |
| Switch to one-ticket project during PTY load  |   1,518 ms |     200 ms |
| First PTY paint upper bound                   |     232 ms |      85 ms |
| Input-to-WebSocket-route upper bound          |       2 ms |      23 ms |
| Input echo paint upper bound                  |      79 ms |      96 ms |
| PTY send API time, 600 calls summed           |     137 ms |     142 ms |
| Frame gaps over 50 ms during load             |         15 |          6 |
| Longest main-thread task during load          |     562 ms |     125 ms |
| Garbage-collected retained JS heap delta      |   +1.76 MB |   +1.75 MB |

The desktop load window accumulated 1,522 ms of style updates, and its
project switch to the small project accumulated 1,134 ms. The same switch
had a 562 ms main-thread task. The narrow switch remained near its idle
range, but uses a dispatched change event while the expanded terminal
drawer covers the project picker. Its wall time therefore cannot be used
as a pointer-interaction comparison with desktop. `HS2-DM2SK3` tracks the
desktop project-switch bottleneck.

The PTY route sends the same binary frame shape as the terminal WebSocket
but has no OS PTY, server attach, or real network. The recorded
`networkTransportMs` is therefore `null`; `messageSendApiMs` sums the
600 synchronous fixture send calls, and the first-paint and echo numbers
include screenshot polling and Playwright scheduling. The narrow drawer
covers workspace controls, so the load scenario dispatches their DOM
click/change events while keeping the PTY viewport open. This exercises
application state and rendering under load, but those narrow action wall
times omit physical pointer targeting. Frame gaps come from
`requestAnimationFrame`; a gap over 50 ms is a coarse dropped-frame
signal, not a complete frame-by-frame presentation trace. The heap delta
compares `Runtime.getHeapUsage` after garbage collection before and after
the burst; it is a single-run retained-heap observation, not a leak rate.
These development-build traces include CDP tracing overhead. Repeat on
the same build and machine before drawing a performance conclusion.
The real local-server profile below measures transport and echo outside
the fixture.

### Project switching during PTY output

`HS2-DM2SK3` parks the outgoing project's keep-alive terminal viewports
before the selected project changes. In the same synthetic profile, the
desktop switch from the 138-ticket project to the small project fell from
1,786 ms to 266 and 297 ms in two runs. The reverse switch fell from
630 ms to 417 and 433 ms. The first after trace reduced desktop switch
renderer task time from 1,758 to 232 ms, style updates from 1,384 to
70 ms, and its longest task from 819 to 98 ms. A second after trace found
271 ms renderer task time, 76 ms style work, and a 102 ms longest task.
Narrow switch windows stayed near their prior 204 and 335 ms.

The PTY burst as a whole can still drop frames; this change bounds the
project transition under that load. The before/after interaction windows,
wide/narrow traces, and live-terminal return screenshots are attached to
`HS2-DM2SK3`. Trace categories are inclusive and should not be summed.

### Grid mode and full-screen switches

`clients/web/tests/terminal-grid-reuse.spec.ts` walks six mounted terminals
from a project's drawer grid into the workspace grid, into and out of full
screen, and back, timing each step until all six previews are settled with
their output. Before `HS2-AMNNBR`/`HS2-TBQQ00` every step disposed and
recreated all six emulators and sockets (503, 364, 374, and 750 ms with
instant fake sockets; a real PTY adds attach and scrollback replay per
terminal). After, no step creates an emulator or socket (208, 160, 168, and
220 ms, mostly Playwright polling), and the spec fails if one is recreated.

## Real local-server PTY transport

`npm run profile:pty:real` builds the local CLI and server binaries, starts
an isolated store and server for each viewport, and mounts the shipped
terminal viewport on its real terminal attach socket. An unbuffered Python
PTY process writes 600 timestamped output blocks with terminal line breaks.
The browser records binary WebSocket arrivals and bytes, input echo, frame
gaps, long tasks, garbage-collected heap, and a DevTools trace at 1440 × 900
and 390 × 844. The command writes
`target/performance-traces/hs2-e035f5-<viewport>-metrics.json` and
`-chromium-trace.json` under `clients/web`. It is opt-in because it runs
local binaries and a real PTY. The baseline metrics and compressed traces
are attached to `HS2-E035F5`.

| Baseline Chromium run, before HS2-36P1NP          |                           Desktop |                            Narrow |
| ------------------------------------------------- | --------------------------------: | --------------------------------: |
| Browser WebSocket data                            | 1,087,804 bytes in 1,201 messages | 1,087,804 bytes in 1,201 messages |
| Browser receive rate                              |                          354 KB/s |                          368 KB/s |
| Process write to browser arrival, p50 / p95       |                      179 / 243 ms |                        49 / 70 ms |
| Synchronous WebSocket message handling, p50 / p95 |                        0 / 0.1 ms |                        0 / 0.1 ms |
| Input to echoed WebSocket message                 |                            2.3 ms |                            2.2 ms |
| Echo paint upper bound                            |                            246 ms |                            244 ms |
| First output paint upper bound                    |                            923 ms |                            421 ms |
| Frame gaps over 50 ms                             |                                25 |                                 4 |
| Longest observed main-thread task                 |                            134 ms |                              0 ms |
| Garbage-collected retained JS heap delta          |                          +1.50 MB |                          +1.49 MB |

These are one-run development-build measurements with tracing and
screenshot-polling overhead. Process and browser timestamps share the
local machine clock. The process-to-browser interval includes PTY, server,
socket, and browser scheduling; it does not isolate network transit.
The synchronous message measure brackets terminal viewport WebSocket
listeners, not later rendering. Paint bounds include Playwright screenshot
polling, so they are deliberately conservative. The synthetic profile
uses a different output shape (1,161,600 bytes) and concurrent workspace
activity; its paint and frame-gap numbers above are context, not a direct
regression comparison.

`HS2-36P1NP` repeated the desktop Chromium WebGL profile: process-to-browser
arrival p95 was 227 ms, close to the 243 ms baseline. A bare WebSocket on the
same server and PTY had p95 1 ms at both widths. Switching only the desktop
viewport to xterm's DOM renderer reduced p95 to 5 ms with no frame gaps over
50 ms. Chromium dedicated terminals now use DOM; Firefox retains WebGL.
After that change, the full real PTY profile measured p95 5 ms desktop and
3 ms narrow, with zero frame gaps over 50 ms at both widths. These results
are local development-build samples, not a universal latency guarantee.
The bare-socket metrics, DOM comparison trace, post-change metrics, and
wide/narrow rendered terminal screenshots are attached to `HS2-36P1NP`.

`npm run profile:pty:firefox` uses the same isolated local server and
600-write OS PTY load in headless Firefox. The dedicated desktop and narrow
viewports selected WebGL and measured process-write to browser arrival p95
of 3 ms and 22 ms; a DOM-forced desktop viewport measured 5 ms. None of
the three runs recorded a frame gap over 50 ms. Park/resume switched each
WebGL viewport through DOM and back without losing its connection or
painted glyphs. This local result supports retaining Firefox WebGL; it
does not establish hardware-accelerated Firefox performance on other
machines. Metrics and before/after-resume screenshots are attached to
`HS2-527G7P`. The narrow profile mounts a standalone dedicated viewport;
the phone drawer's 80xM grid policy can still select DOM in the full app.

## Baseline observations

The warm main project costs much more to reveal than the one-ticket project.
The 138-ticket transitions usually contain around 115–136 ms of inclusive
`FunctionCall` time and 43–50 ms of `Layout`; one first narrow switch was
lighter at 94 ms and 19 ms. Switches to the small project contain around
55–66 ms of `FunctionCall` and 1–3 ms of `Layout`.
The list's rendered row count is therefore a useful explanatory variable.
Blink and WebKit list rows used `content-visibility: auto` with a 72px intrinsic
estimate, and the list stack uses block layout (`HS2-WRY9Q2`, `HS2-AHRFBW`).
`HS2-S3RGM8` removed that row `content-visibility` from lists and board columns,
because Safari painted skipped rows late while scrolling. Rows now always render,
so the layout figures below predate that change and may rise.
WebKit waits for progressive rows to settle before restoring a saved deep scroll
position, avoiding a clamp against the initial 40-row height.
In the same 138-ticket Chromium desktop and narrow profile, warm-return
layout fell to about 16–21 ms and search-clear
layout to about 19–22 ms. All rows still mount through the existing progressive
render path; deep scroll, project/view restoration, and mutation flows remain
covered by Chromium and WebKit browser tests. Ticket selection still exceeds
the painted budget in some runs and remains in `HS2-KCXMAT`.
`HS2-KCXMAT` then removed two whole-app style invalidations. First, the
measured workspace-search widths were inherited custom properties on the app
root, so every search open, close, or remeasure restyled about 5,000 elements.
They now live in one adopted stylesheet rule per toolbar, scoped to that
toolbar's open search slot, which survives Kerf morphs just as the root did.
Second, ticket motion now counts a bulk arrival from row slugs before measuring
any row. In three repeated runs each, ticket selection fell from 105–110 ms
(desktop) and 125–130 ms (narrow) to 86–94 ms at both widths. Large-project
returns with activity measured 93–102 ms, and search-clear JavaScript fell from
about 131–135 ms to 79–83 ms. What remains per click is Kerf's separate
pointer-blur collapse render pass, full-list JSX attribute serialization
(including Lucide path data), and scroll-divider remeasurement, all tracked
as upstream Kerf tickets.
Those Kerf fixes (`KF-AS24QP`, `KF-JPP0Z5`, `KF-50PA0Y`) shipped in
`@kerfjs/ui` 5.0.0-beta.95 and need no app-side adoption. On beta.95 alone,
three repeated runs measured ticket selection at 70–75 ms desktop and 77–78 ms
narrow, and every project change, including large-project returns, at 84–94 ms.
`HS2-4B8Q5D` then stopped per-render layout reads in ticket scroll memory and
motion. Once a scope's scroll destination is reached, a re-render reads no
scroll sizes unless a `MutationObserver` saw rows added or removed inside a
scroll owner, an attribute change on an owner or its ancestors, a replaced
owner, or a viewport resize. Ticket motion returns before measuring when no row
arrived, left, or changed column, and reads computed border radius once per row
class. In three repeated runs, project changes fell to 77–84 ms desktop and
69–72 ms narrow, and selection measured 65–73 ms desktop and 78–85 ms narrow.

The app's measured project click-to-next-paint ranged roughly 52–92 ms
before terminal and AI activity. Returning to the large project with
activity present took 103 ms on desktop and 100 ms on narrow screens.

Clearing search repaints the large ticket list. It produced around
131–135 ms of inclusive JavaScript calls and 48–51 ms of layout across the
two viewports. Selecting a ticket also crossed the 100 ms painted UI budget
(107 ms desktop, 130 ms narrow). The Up Next mutation's optimistic paint stayed below
100 ms in this run.

Selecting a terminal tab with 12 sessions produced the largest style work:
roughly 435–447 ms of `UpdateLayoutTree` accumulated through the action
window. Creating an AI chat and switching projects with terminal activity
also produced long main-thread tasks. These results point first to DOM and
style invalidation in terminal and ticket views. Workers cannot perform DOM
layout or paint.

Dedicated terminal views now render only the selected session; the existing
keep-alive controller parks inactive viewports and restores their live DOM on
return (`HS2-9B8QHF`). In three repeated 12-session Chromium profiles,
terminal-tab style work fell from 402–467 ms to 153–167 ms on desktop and
from 396–440 ms to 136–143 ms at the narrow viewport. Paint stayed within
3–4 ms in the after runs. Kerf beta.88's whole-tab snapping added roughly
909px of end padding at this tab count, pushing the creation action under the
inspector rail; terminal tab snapping is disabled until `HS2-3P7TZV` can
restore it with a bounded Kerf strip.

Thirty AI output events over roughly 770 ms accumulated 413–494 ms of
renderer-main task time and 149–192 ms of style updates, but no single
task in that stream exceeded 50 ms. The message path deserves a bounded
worker experiment after rendering improvements; this trace does not show
it dominating an individual frame. The opt-in PTY benchmark above measures
output traffic absent from the default fixture.

## Worker boundaries

| Boundary                                                               | Gain and constraints                                                                                                                                                                                                                                                                                                                | Decision                                                                |
| ---------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| Main-thread rendering, progressive ticket rows, and terminal viewports | Owns DOM, style, layout, and xterm/WebGL; cannot be transferred to a worker. Reducing mounted work addresses the measured hot path directly.                                                                                                                                                                                        | Optimize first.                                                         |
| One dedicated change-stream worker with a project-indexed state map    | Can own socket polling, JSON validation, cursor ordering, and bounded event coalescing for all open projects. One module instance limits idle memory and copies only compact event envelopes. It needs project generations, abort, replay on restart, and a per-project queue so one busy project cannot delay others indefinitely. | Prototype after rendering work, with before/after message-burst traces. |
| One worker per project                                                 | Strong failure isolation and natural ordering, but each project duplicates worker/module memory and connection setup. Structured-cloning full ticket collections would be costly; socket ownership still requires replay on restart.                                                                                                | Defer until a shared worker demonstrably starves unrelated projects.    |
| Search and ticket sorting worker                                       | Pure search evaluation is movable, but the measured query is modest at 138 rows and server search supplies the result set. Copying rows and synchronizing generations may cost more than the saved work.                                                                                                                            | Reconsider only at larger result sets with measured CPU dominance.      |
| Terminal emulator worker                                               | xterm rendering and input are tied to DOM/canvas APIs. Offscreen rendering would be a larger compatibility project, particularly for WebKit and viewport lifecycle.                                                                                                                                                                 | Defer.                                                                  |

The change-stream path in `project-change-poll.ts` already handles cursor
continuity, polling fallback, reconnect, and cancellation. A worker move must
preserve those semantics and send compact, ordered events to
`app/runtime.tsx`; UI signals, optimistic ticket mutations, terminal
viewports, and conversation presentation stay on the main thread. The
existing warm-project cache prevents a broad worker-owned ticket-store
rewrite from being necessary for the first stage.

## Typing latency in text editors (HS2-EQTXTJ)

`HOTSHEET_TYPING_PROFILE=1 npx playwright test tests/typing-latency-profile.spec.ts` opens a real
server with 400 tickets and types 43 characters at 60 ms intervals into the search field, title,
details editor, and new-note composer, recording Chromium Event Timing (`keydown`/`input` duration
to next paint). Each draft keystroke used to write a draft signal that re-rendered the whole
workspace; the editors now publish drafts at typing pauses. Same machine and load, before and after:

| Editor   | Keystrokes over 16 ms | `input` handlers (avg)                    |
| -------- | --------------------- | ----------------------------------------- |
| Title    | 43/43 to 3/43         | 11.2 ms to 0.2 ms                         |
| Details  | 43/43 to 2/43         | 10.7 ms to 0.1 ms                         |
| New note | 43/43 to 0/43         | 14.1 ms to 0 ms                           |
| Search   | 43/43 to 43/43        | 5.8 ms (unchanged; tracked in HS2-X72PD6) |

Headless Chromium on a fast machine understates Safari and large live projects, where the same
per-keystroke render scaled into the 50–100 ms the report describes. Run the profile on a quiet
machine; concurrent builds shift the paint component by several milliseconds.

## Rollout criteria

First reduce the measured ticket and terminal rendering cost and capture
equivalent before/after wide and narrow traces. Next prototype a single
dedicated change-stream worker behind a test-only switch. Exercise at least
two active projects, sustained AI output, out-of-order and repeated events,
disconnect/reconnect, cancellation during project closure, and worker crash
recovery. Compare click-to-paint latency, >50 ms main-thread tasks, event
delivery lag, and retained memory against the baseline. Expand to a
per-project or search worker only if that comparison isolates a remaining
main-thread processing bottleneck.

The rendering follow-ups are `HS2-KCXMAT` (ticket list) and `HS2-9B8QHF`
(terminal tab). The shared-worker experiment is `HS2-SH9179`, sequenced
after both rendering investigations.

## Shared change-stream worker prototype

`HS2-SH9179` adds an opt-in dedicated worker that owns every open project's
change transport. It is off by default. Set the local setting
`hotsheet.change-stream-worker` to `true` in the browser's `localStorage`,
then reload, to use it.

- **Split.** `project-change-poll.ts` is now a _transport_ (handshake,
  WebSocket, JSON validation, cursor gap/repeat checks, long-poll replay,
  and backoff) feeding a _consumer_ (resync, events, refresh barrier, and
  refresh). The default main-thread stream composes both, unchanged.
- **Worker.** `change-stream.worker.ts` runs one transport per project.
  Each project has a bounded delivery queue with one delivery in flight.
  Adjacent event pages coalesce while the main thread is busy. Past
  1,000 queued events, the queue collapses into one overflow page, which
  reconciles from the server. Only change events cross the boundary, never
  ticket collections.
- **Host.** `change-stream-worker-host.ts` applies each delivery through
  the same consumer and acknowledges it. Every subscription carries a
  generation token, and closing a project aborts its transport. The host
  falls back to the main-thread stream on a protocol-version mismatch, a
  missing `ready`, or an unavailable worker. A crashed worker restarts with
  bounded backoff and replays each project from its last applied cursor,
  dropping repeated events. More than three crashes within a minute fall
  back for the rest of the session.

`npm run profile:ui:worker` repeats the `HS2-WDTN3W` scenario with the worker
on. Playwright cannot route WebSockets opened inside a worker, so that
profile delivers the same events through held long polls. One run on the
same build and machine gave:

| `ai-stream` window (30 AI events) | Before | Main thread after | Worker |
| --------------------------------- | -----: | ----------------: | -----: |
| Desktop renderer task time        | 180 ms |            189 ms |  92 ms |
| Desktop JavaScript function time  |  77 ms |             86 ms |  23 ms |
| Narrow renderer task time         | 152 ms |            150 ms | 118 ms |
| Narrow JavaScript function time   |  83 ms |             75 ms |  31 ms |
| Desktop p95 click-to-paint, all   |  81 ms |             82 ms |  76 ms |
| Narrow p95 click-to-paint, all    |  86 ms |             85 ms |  79 ms |

Main-thread JavaScript time during the AI stream fell by about 60–70% and
renderer task time by 22–49%; p95 click-to-paint did not regress. The worker's narrow `ai-stream`
wall time rose to 3.5 s. In this fixture the worker's socket upgrade is
refused, so events wait for the next long-poll window. This measures
fixture delivery lag, not the real WebSocket path. `HS2-9XF60G` covers crash restart with cursor replay,
the crash-loop fallback, and the skew fallback in real-browser tests
(`clients/web/tests/change-stream-worker.spec.ts`).

### Real-server profile and default-on decision

`npm run profile:worker:real` (`clients/web/scripts/change-stream-worker-profile.mjs`,
`HS2-MH5PWD`) builds the production client and local host, supervises a real
machine server under an isolated `HOTSHEET_HOME`, and opens two projects in
Chromium at 1440×900. It alternates main-thread and worker runs. Each run
writes 40 visible-project tickets through the server while the background
project receives 40 more. It then switches project tabs 30 times while both
projects keep receiving writes. It reports:

- delivery lag, from the server write to the rendered row;
- click-to-paint, from Event Timing;
- heap after a forced GC, for the page and the worker isolate.

Both modes stayed on real WebSockets; each made only its four startup long
polls. One run on the same build and machine gave:

| Metric (two runs per mode)         |  Main thread |       Worker |
| ---------------------------------- | -----------: | -----------: |
| Delivery lag p50                   |   255–280 ms |   294–332 ms |
| Delivery lag p95                   |   409–416 ms |   457–481 ms |
| Click-to-paint p95                 |     72–96 ms |        80 ms |
| GC heap after load (page + worker) | 10.4–11.3 MB | 11.3–12.2 MB |
| Worker isolate heap                |            — |      0.36 MB |

Server writes dominate delivery lag in both modes. The worker added about
40–50 ms of delivery lag and about 0.9 MB of heap. Click-to-paint showed no
consistent difference. At realistic two-project load, the main thread is not
the bottleneck the worker was meant to relieve. **Decision:** the worker stays
off by default (opt in with `hotsheet.change-stream-worker`). Revisit only if a
profile of heavier concurrent AI output shows main-thread change processing
dominating click-to-paint.

### Cross-tab SharedWorker measurement (HS2-NP7R42)

When the worker setting is enabled, supported browsers now use a SharedWorker
across same-origin tabs. Two Chromium pages opening the same project through a
real SharedWorker and a local bridge fixture held **one browser WebSocket**;
each tab received only its own client-tagged events, and closing one tab left
the other's stream open. The long-poll fallback similarly used one version 2
browser request for both tabs, with separate per-tab answers. The dedicated
worker mode would use one socket or long poll per tab. Client ids remain
separate at the bridge, so presence does not become one shared tab.

The real-server profile script now counts `shared_worker` targets and their
heap. A short sanity run (two visible changes and two tab clicks in each of
two runs per mode) found one 0.37–0.45 MB SharedWorker isolate and no
long-poll fallback. Its worker delivery-lag p95 was 124–126 ms versus
73–98 ms on the main thread; click-to-paint p95 was 56–88 ms versus
48–80 ms. The tiny sample does not establish a latency benefit and did not
exercise two real app tabs. The measurable cross-tab benefit is connection
count under the per-origin HTTP/1.1 cap; the existing default-off worker
setting remains the adoption gate. Unsupported SharedWorker contexts keep
the dedicated worker path.
