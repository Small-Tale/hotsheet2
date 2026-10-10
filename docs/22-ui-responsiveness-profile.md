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
Blink and WebKit list rows use `content-visibility: auto` with a 72px intrinsic
estimate, and the list stack uses block layout (`HS2-WRY9Q2`, `HS2-AHRFBW`).
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
