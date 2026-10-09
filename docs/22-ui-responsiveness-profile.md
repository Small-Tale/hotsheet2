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
latency. Terminal data describes session presence, not a high-volume PTY
output stream.

## Baseline observations

The warm main project costs much more to reveal than the one-ticket project.
The 138-ticket transitions usually contain around 115–136 ms of inclusive
`FunctionCall` time and 43–50 ms of `Layout`; one first narrow switch was
lighter at 94 ms and 19 ms. Switches to the small project contain around
55–66 ms of `FunctionCall` and 1–3 ms of `Layout`.
The list's rendered row count is therefore a useful explanatory variable.
Blink list rows use `content-visibility: auto` with a 72px intrinsic estimate,
and the list stack uses block layout (`HS2-WRY9Q2`). WebKit keeps normal row
visibility because skipping row contents clamps deep scroll restoration there.
In the same 138-ticket Chromium desktop and narrow profile, warm-return
layout fell to about 16–21 ms and search-clear
layout to about 19–22 ms. All rows still mount through the existing progressive
render path; deep scroll, project/view restoration, and mutation flows remain
covered by Chromium and WebKit browser tests. Ticket selection still exceeds
the painted budget in some runs and remains in `HS2-KCXMAT`.
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

Thirty AI output events over roughly 770 ms accumulated 413–494 ms of
renderer-main task time and 149–192 ms of style updates, but no single
task in that stream exceeded 50 ms. The message path deserves a bounded
worker experiment after rendering improvements; this trace does not show
it dominating an individual frame. The follow-up PTY benchmark
(`HS2-K7FJVK`) will measure output traffic absent from this fixture.

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
