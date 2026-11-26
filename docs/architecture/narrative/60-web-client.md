# Browser web client

Status: written by HS2-AYKHVV. Audited source
`8a9d46d3972aa65ebafe0a25d8a60a0ca4f76774`. Per-module rows, all seven
depth-standard fields, and the full module lists are in
[coverage/web-client.md](../coverage/web-client.md). Paths below are
repository relative; unit tests are sibling `*.test.ts` files and Playwright
specs live in `clients/web/tests/`.

## Overview

The browser web client is the `web` container: one TypeScript page built with
Kerf (`kerfjs` and `@kerfjs/ui` 5.0.0-beta.135) and xterm.js 6. The same page
runs in an ordinary browser, in a remote browser signed in to a host, and in
the Tauri desktop WebView. It talks only to the Node web host and project
bridge (`bridge`, area HS2-T6SD67) over same-origin HTTP and WebSocket. The
browser never holds a server secret: `Api` sends an empty
`X-Hotsheet-Secret` and the bridge injects credentials
(`clients/web/src/api.ts:2165`). Test: `clients/web/src/api.test.ts`.

`clients/web/src` also contains Node modules (the project bridge, the
terminal WebSocket proxy, the desktop storage server). The split follows the
runtime import graph; the web host coverage file publishes it, and this
chapter covers the 732 browser modules.

What surprises a first reader:

- **One composition closure.** `startHotSheetWebClient`
  (`clients/web/src/app/runtime.tsx:417`) is 8,355 lines. It declares about
  233 signals as local variables and creates every feature controller
  (`create*Controller(dependencies)` factories in `clients/web/src/features`),
  passing signals in through dependency objects. Kerf stores are not used.
  Test: `clients/web/src/features/runtime-split-controllers.test.ts`.
- **No router.** The only path branch is `/ux-demo`
  (`clients/web/src/main.tsx:37`). Projects are tabs inside one mounted tree.
- **Interactions are delegated, not inline.** About 500 Kerf `delegate`
  registrations in `clients/web/src/interactions` use selectors from the
  `action()` tables in `clients/web/src/interaction-attrs`, installed in a
  fixed order (`clients/web/src/app/wire-interactions.ts:28-42`). Test:
  `clients/web/src/interaction-wiring.test.ts`.
- **The change-stream worker is off by default.** Streams run on the main
  thread unless `localStorage['hotsheet.change-stream-worker']` is `true`
  (`clients/web/src/features/project-change-streams.ts:193-199`). Test:
  `clients/web/tests/change-stream-worker.spec.ts` ("keeps the main-thread
  stream when the worker setting is off").
- **The 150 ms debounce is local.** Typing writes only a localStorage
  recovery copy; the server write happens once on blur, page hide or finish
  (`clients/web/src/debounced-autosave.ts:89-155`). Test:
  `clients/web/src/debounced-autosave.test.ts`.

## Components

The container has 16 components. Views `HS2-WebClient-Core`,
`HS2-WebClient-LiveData`, `HS2-WebClient-Editing`, `HS2-WebClient-Features`
and `HS2-WebClient-FeatureServices` show them in groups; the baseline
`HS2-WebComponents` view keeps the original four.

| Component          | Owns                                                    | Main entry                                               |
| ------------------ | ------------------------------------------------------- | -------------------------------------------------------- |
| `webShell`         | boot, runtime closure, interactions, Kerf UI wiring     | `clients/web/src/app/runtime.tsx:417`                    |
| `webWorkspace`     | open projects, tabs, open/close/teardown                | `clients/web/src/features/project-lifecycle.ts:333`      |
| `webApi`           | `Api` request and response validation, busy tracking    | `clients/web/src/api.ts:2118`                            |
| `webChanges`       | change streams, workers, read gaps, refresh coalescing  | `clients/web/src/features/project-change-streams.ts:401` |
| `webTickets`       | rows, board, groups, search, readers, optimistic writes | `clients/web/src/features/ticket-workflows.ts:604`       |
| `webDrafts`        | drafts, recovery copies, merge, staged files            | `clients/web/src/debounced-autosave.ts:89`               |
| `webPersistence`   | localStorage records, desktop mirror, caches            | `clients/web/src/desktop-storage.ts:10`                  |
| `webTerminal`      | xterm viewports, attach, replay, resize                 | `clients/web/src/terminal-viewport-runtime.ts:505`       |
| `webAttachments`   | attachments and gallery                                 | `clients/web/src/features/gallery.tsx:53`                |
| `webAi`            | conversations, drive, permissions, halted sessions      | `clients/web/src/features/drive-conversations.ts:89`     |
| `webOrchestration` | orchestration view, plan, timeline, settings            | `clients/web/src/features/orchestration-view.ts:359`     |
| `webCommands`      | commands and schedules                                  | `clients/web/src/features/commands.tsx:72`               |
| `webNotifications` | inbox, system notifications, toasts, announcer          | `clients/web/src/client-notifications.ts:9`              |
| `webRemote`        | remote sign-in, grants, devices, pairing                | `clients/web/src/features/remote-devices.ts:46`          |
| `webHostOps`       | host health, stats, repository, migration, standby      | `clients/web/src/features/standby.tsx:101`               |
| `webUxDemo`        | the `/ux-demo` catalog                                  | `clients/web/src/ux-demo-entry.ts:30`                    |

### State ownership

- **Owner.** Shared cross-feature state is local to the runtime closure, for
  example `projects`, `selectedProjectId`, `tickets`, `ticketRowsByProject`
  (`clients/web/src/app/runtime.tsx:427-458`). Feature factories own their
  own signals: the project lifecycle controller (44), the gallery (23), the
  announcer (20), the orchestration view (16), saved views and commands (14
  each). Module-level singletons are limited to server-busy tracking
  (`clients/web/src/server-busy.ts:9-14`). Test:
  `clients/web/src/features/controllers.test.ts` ("feature owners retain live
  state across transitions (HS2-DHYGXJ)").
- **Derivation.** Rendered rows derive from `ticketRowsByProject`; a project
  switch restores `tickets` from that record (the warm path) inside one
  `batch` (`clients/web/src/app/runtime.tsx:3436`). Change events never patch
  rows directly: they trigger an authoritative re-read
  (`clients/web/src/features/project-refresh.ts:136-308`). Test:
  `clients/web/src/features/project-refresh.test.ts`.
- **Reset.** Project switch resets composer, board pages and progressive
  rendering in `activateOpenProject`; project close prunes 13 per-project
  records, connection records, autosaves, pending-created rows and the warm
  cache entry (`clients/web/src/app/runtime.tsx:5632-5647`,
  `clients/web/src/features/project-teardown.ts:95`). Test:
  `clients/web/src/features/project-teardown.test.ts`. There is no policy test
  that every signal has a reset (see Discrepancies).

### Kerf public API dependencies

- Core: `signal`, `computed`, `effect`, `batch`, `mount`, `delegate`, `each`
  from `kerfjs`; `createScope` from `kerfjs/scope`; `action` from
  `kerfjs/actions`; `kerfjs/dev` only in development
  (`clients/web/src/app/development-bootstrap.ts:4`).
- UI wirings at page lifetime: `wireScrollDividers`, `wireCornerStatus`,
  `wireResizableRegions`, `wireChoiceChips`, `wireTextFields`, `wireForms`,
  `wireOverlaySheets`, `wirePinnablePopover` (`clients/web/src/main.tsx:14-65`).
  Web Awesome button and dialog are registered at the entry
  (`clients/web/src/main.tsx:6-7`).
- App component catalog: `clients/web/kerf.components.json` has 173 entries
  (152 components, 21 compositions); `npm run catalog:check` verifies it.
  `clients/web/.kerf-ui-profile.json` holds 48 doctor exceptions, pinned by
  `clients/web/src/kerf-ui-profile.test.ts`.
- Kerf tickets the code is waiting on (comments naming an upstream gap):
  KF-8CDT8B SearchField `resultText`
  (`clients/web/src/components/terminal-focus-search.tsx:103`), KF-8QBJWN
  submenu stays open
  (`clients/web/src/components/orchestration-settings-interactions.ts:126`),
  KF-89C223 multiline row (`clients/web/src/components/desktop-remote-dialog.tsx:124`),
  KF-ND665K compact arrow (`clients/web/src/components/note-card.css:133`),
  KF-CS729Y busy bars (`clients/web/src/components/server-busy-bars.css:3`),
  KF-E2J9ND raw Web Awesome modal parts
  (`clients/web/src/kerf-ui-profile.test.ts:116`). Their upstream status is
  not verifiable from this repository (Unresolved U3).

### The /ux-demo route

`shouldBootUxDemo` requires both the `ux-demo` host feature and the path
(`clients/web/src/host-features.ts:94-96`). `bootUxDemo` replaces `#app` with
a `noindex` catalog root and dynamically imports `ux-demo/main.tsx`
(`clients/web/src/ux-demo-entry.ts:11-36`), which mounts production
components with fixtures from the demo registry
(`clients/web/src/ux-demo/registry.ts:91`). The entry does not await the
import, to avoid a circular top-level await (comment at
`clients/web/src/main.tsx:33-36`). Test: `clients/web/tests/ux-demo.spec.ts`,
`clients/web/src/ux-demo-entry.dom.test.ts`.

## Durable state and file formats

The client writes no files. Everything that survives a reload is browser
storage, the server, or (in the desktop shell) the desktop storage endpoint.

| Store            | Keys or name                                                                                                                                         | Writer                        | Survives reload                        | Citation                                                                                                                                                                               |
| ---------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------- | -------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| localStorage     | `hotsheet.ticket-draft:<project>:<qualifiedId>:<field>[:<noteId>]` `{base, draft, at, pendingIntent?}`                                               | `webDrafts`                   | yes                                    | `clients/web/src/ticket-draft-store.ts:10-33`                                                                                                                                          |
| localStorage     | `hotsheet.workspace.project-session.v1.<id>`, `hotsheet.workspace.active-project-root.v1`                                                            | `webWorkspace`                | yes                                    | `clients/web/src/workspace-session.ts:47-48`                                                                                                                                           |
| localStorage     | `hotsheet.layout.workspace-preferences.v1`, `hotsheet.layout.*` sizes                                                                                | `webPersistence`, `webShell`  | yes                                    | `clients/web/src/workspace-preferences.ts:46`, `clients/web/src/app-region-resize.ts:20-22`                                                                                            |
| localStorage     | `hotsheet.ai-conversations.v1`, `hotsheet.ai-conversation-reads.v1`                                                                                  | `webAi`                       | yes                                    | `clients/web/src/conversation-persistence.ts:3`, `clients/web/src/conversation-unread.ts:13`                                                                                           |
| localStorage     | `hotsheet.terminals.*`                                                                                                                               | `webTerminal`                 | yes                                    | `clients/web/src/terminal-visibility.ts:6`                                                                                                                                             |
| localStorage     | `hotsheet.theme`, `hotsheet.keyboard-shortcuts`, `hotsheet.notifications-paused`, `hotsheet.announcer.settings`, `hotsheet.open-projects` and others | several                       | yes                                    | listed per component in the coverage file                                                                                                                                              |
| sessionStorage   | `hotsheet:advisory-worker`, `hotsheet:advisory-operation:<json>`                                                                                     | `webOrchestration`            | this tab only                          | `clients/web/src/app/runtime.tsx:2278-2331`                                                                                                                                            |
| IndexedDB        | database `hotsheet-draft-files-v1`, store `files`, key `<scope>:<id>`, bytes as ArrayBuffer                                                          | `webDrafts`                   | yes                                    | `clients/web/src/workspace-session.ts:51-52`, `clients/web/src/browser-draft-file-contract.ts:9-19`                                                                                    |
| Desktop endpoint | `PUT /__hotsheet/desktop-storage` mirror of every `hotsheet.*` key with a revision; `/__hotsheet/desktop-draft-files?key=`                           | `webPersistence`, `webDrafts` | yes, in the desktop app across origins | `clients/web/src/desktop-storage.ts:88-110`, `clients/web/src/desktop-draft-files.ts:8-38`                                                                                             |
| Memory           | warm-project LRU (8 ids), ticket detail LRU (50), scroll memory, pending created rows (30 s), local change acknowledgements (30 s, 2048)             | various                       | no                                     | `clients/web/src/project-warm-cache.ts:6`, `clients/web/src/ticket-detail-cache.ts:14`, `clients/web/src/pending-created-tickets.ts:10`, `clients/web/src/local-ticket-changes.ts:8-9` |

Saved views are server records (`clients/web/src/api.ts:3655`), not browser
storage. Tests: `clients/web/src/ticket-draft-store.test.ts`,
`clients/web/src/workspace-session.test.ts`,
`clients/web/src/desktop-storage.test.ts`,
`clients/web/tests/desktop-draft-files.spec.ts`.

Crash behavior: a closed tab loses at most the last 150 ms of typing from the
recovery copy and any unflushed desktop mirror writes. Desktop mirror
failures stay pending and are retried on the next write or on `pagehide`
with `keepalive` for bodies under 60 KB
(`clients/web/src/desktop-storage.ts:36-73`). A native desktop close first
dispatches the flush event, captures recovery, and acknowledges with
`desktop_flush_complete` (`clients/web/src/desktop-close.ts:50-57`). Test:
`clients/web/src/desktop-close.test.ts`, `clients/web/tests/desktop-native-close.spec.ts`.

## Concurrency, tasks and locks

The page is single threaded. Background work:

- **Change-stream worker (optional).** One module SharedWorker per origin
  named `hotsheet-change-stream`, or one dedicated module Worker per page
  when SharedWorker is missing or `hotsheet.change-stream-shared-worker` is
  `false` (`clients/web/src/features/project-change-streams.ts:202-275`).
  Owner: the worker host (`clients/web/src/change-stream-worker-host.ts:73`).
  Shutdown: a dedicated worker is terminated on dispose
  (`clients/web/src/change-stream-worker-host.ts:273-282`); a tab posts
  `dispose` on non-persisted `pagehide`, and the shared hub sweeps ports idle
  for 90 s every 30 s (`clients/web/src/change-stream.shared-worker.ts:11-13`,
  `clients/web/src/change-stream-shared-hub.ts:75-82`). Test:
  `clients/web/src/change-stream-shared-hub.test.ts`.
- **Multi-tab.** Without the worker, each tab runs its own streams. With the
  SharedWorker, tabs share one bridge WebSocket and one poll pool per
  endpoint, while each port keeps separate project streams
  (`clients/web/src/change-stream-shared-hub.ts:8-47`). Direct polls across
  tabs are bounded by four Web Locks named `hotsheet-direct-change-poll-0..3`
  (`clients/web/src/change-poll-slots.ts:2`,
  `clients/web/src/change-poll-slots.ts:35`). Tabs do not share drafts or
  signals; they converge through the server and the change stream. Test:
  `clients/web/tests/change-stream-shared-worker.spec.ts` ("two tabs share
  one bridge socket but keep independent project streams (HS2-NP7R42)").
- **Refresh barrier.** A local mutation holds `createRefreshBarrier`
  (`clients/web/src/app/runtime.tsx:894`) and change refreshes wait for it
  (`clients/web/src/refresh-barrier.ts:6-27`). Test:
  `clients/web/src/refresh-barrier.test.ts`.
- **Per-ticket write sequencer.** Writes to one ticket are serialized
  (`clients/web/src/features/ticket-workflows.ts:638-640`). Test:
  `clients/web/tests/ticket-edit-sequencing.spec.ts`.
- **Timers.** Draft debounce (150 ms), refresh coalescing (180 ms quiet, 900
  ms max, `clients/web/src/project-change-refresh.ts:7-8`), repository refresh
  debounce (180 ms), worker heartbeat (20 s), terminal resize heartbeat and
  ping (5 s), and turn clocks that only re-render
  (`clients/web/src/app/runtime.tsx:4985-5003`). None polls the network on
  a fixed interval except the direct-poll fallback described under
  Discrepancies.
- **Long polls.** Change streams, the device list
  (`clients/web/src/features/remote-devices.ts:87-107`), worker views per Git
  source (`clients/web/src/worker-view-stream.ts:8-55`) and versioned long
  polls (`clients/web/src/versioned-long-poll.ts:6-50`); each backs off to a
  30 s cap. Test: `clients/web/src/versioned-long-poll.test.ts`.

## Failure and recovery

- **Server or stream down.** Change streams back off 500 ms to 30 s
  (`clients/web/src/project-change-poll.ts:242-243`); the `online` event and
  user activity wake them (`clients/web/src/app/runtime.tsx:5151-5154`).
  Stream health drives standby liveness after 20 s
  (`clients/web/src/features/standby.tsx:290-305`) and, for remote projects,
  the reconnecting banner after 3 s
  (`clients/web/src/app/runtime.tsx:1714-1735`). Test:
  `clients/web/tests/project-startup.spec.ts` ("keeps an open remote tab with
  a reconnecting state while its host drops mid-session (HS2-WNK73S)").
- **Unavailable project.** An open or restore failure renders the
  unavailable shell kinds (`clients/web/src/project-unavailable-shell.ts:8`)
  with an explanation (`clients/web/src/project-unavailable-cause.ts:94`).
- **Request errors.** `ApiHttpError` carries status, code and request id
  (`clients/web/src/api.ts:2103`); page-level errors render in the dismissible
  `AppError` banner (`clients/web/src/components/app-error.tsx:7`). Test:
  `clients/web/src/components/app-error.test.ts`.
- **Write failure.** The optimistic row and selection roll back
  (`clients/web/src/features/ticket-workflows.ts:795-808`); the draft and its
  refusal stay visible. Test: untested at unit level.
- **Read gap.** Overflow or a failed replay forces a resync and a full
  re-read (dynamic view `HS2-WebClient-ReplayOverflow`).
- **Terminal.** Stale or corrupt sockets reattach with `resume_from`; input
  typed meanwhile is kept up to 64 KiB (dynamic view
  `HS2-WebClient-TerminalAttach`).
- **Worker failure.** Crash restarts with 250 ms doubling delay, then
  permanent main-thread fallback after 3 crashes in 60 s
  (`clients/web/src/change-stream-worker-host.ts:74-76`,
  `clients/web/src/change-stream-worker-host.ts:187-202`). Test:
  `clients/web/tests/change-stream-worker.spec.ts` ("a crash-looping worker
  falls back").
- **Storage failure.** Reads tolerate unavailable storage; several writes do
  not catch (`clients/web/src/workspace-session.ts:161-167`,
  `clients/web/src/workspace-preferences.ts:164-166`,
  `clients/web/src/conversation-persistence.ts:104-108`). Test: untested.

## Configuration and environment

The browser reads no environment variables at run time. Inputs:

- Host-stamped page metadata: `hotsheet-features` (`ux-demo`,
  `clients/web/src/host-features.ts:84`) and the remote auth mode
  (`clients/web/src/components/remote-auth-screen.tsx:130`).
- Window markers: `__HOTSHEET_DESKTOP_STORAGE__`
  (`clients/web/src/main.tsx:41-42`), `__TAURI_INTERNALS__`
  (`clients/web/src/native-notifications.ts:14`).
- Build-time: `import.meta.env.HOTSHEET_WEB_STABLE_DEV`
  (`clients/web/src/app/development-bootstrap.ts:9`).
- localStorage toggles: `hotsheet.change-stream-worker` (default off),
  `hotsheet.change-stream-shared-worker` (default on), and
  `hotsheet.change-stream-worker-ready-timeout-ms` (development only)
  (`clients/web/src/features/project-change-streams.ts:156-176`).
- User settings stored in the browser: listed in the coverage file per
  component. Server-side settings arrive through `Api`.

## Dynamic views

### HS2-WebClient-OpenProject

1. The user picks a project; the `choose-project` delegate calls `openProject`
   (`clients/web/src/features/project-lifecycle.ts:333`).
2. `openProjectFetch` opens through the bridge and reads providers and the
   project type in parallel (`clients/web/src/project-startup.ts:12`). Test:
   `clients/web/src/project-startup.test.ts`.
3. `wireOpenedProject` inserts the tab and calls `syncProjectChangeStreams`
   (`clients/web/src/features/project-lifecycle.ts:208`).
4. `activateOpenedProject` activates the tab, refreshes tickets, commands,
   views and drive connections, then restores the stored session
   (`clients/web/src/features/project-lifecycle.ts:285`,
   `clients/web/src/workspace-session.ts:97`).
5. `refreshProject` waits on the barrier and writes rows, counts and the
   projection (`clients/web/src/features/project-refresh.ts:136-308`); failure
   sets `refreshError` and the unavailable shell.
6. The stream handshakes with a zero-cursor poll and opens the WebSocket
   (`clients/web/src/project-change-poll.ts:349-358`). A ticket read stamped
   below the first cursor replays the gap once
   (`clients/web/src/change-read-gap.ts:80-131`). Test:
   `clients/web/tests/change-stream-worker.spec.ts` ("applies a change made
   between the ticket load and its handshake (HS2-SQKJYG)").

### HS2-WebClient-CoalescedEdit

1. Keystrokes update the controlled draft; `coalescedDraft` publishes it once
   per typing pause (`clients/web/src/draft-input-coalescing.ts:13-51`).
2. 150 ms after the last keystroke the recovery copy with its edit-start base
   is written; no server request is made
   (`clients/web/src/debounced-autosave.ts:132-138`). Test:
   `clients/web/src/debounced-autosave.test.ts` ("coalesces rapid edits and
   saves the latest value after 150 ms").
3. A focus move flushes the field
   (`clients/web/src/interactions/inspector-and-editor.ts:684-689`).
4. `applyTicketPatch` projects the change, begins the barrier, records the
   write as this tab's own change and sends it through the per-ticket
   sequencer (`clients/web/src/features/ticket-workflows.ts:604-724`). A stale
   token rebases and retries up to 4 times
   (`clients/web/src/features/ticket-workflows.ts:487`).
5. An edit typed while the save runs waits and saves next
   (`clients/web/src/debounced-autosave.ts:107-110`); the recovery copy is
   cleared only if nothing newer replaced it
   (`clients/web/src/debounced-autosave.ts:123`).
6. Failure path: the row rolls back and a toast reports it
   (`clients/web/src/features/ticket-workflows.ts:795-808`).
7. Reload recovery: `restoreTicketDraft` drops a copy equal to its base or the
   current value, merges the rest, and discards a conflicting copy with a toast
   (`clients/web/src/features/ticket-workflows.ts:917-937`). Test:
   `clients/web/src/ticket-draft-store.test.ts`; the wiring is untested.

### HS2-WebClient-OtherTabChange

1. Tab A writes as above. Tab B's stream receives the event: on its main
   thread by default, or through its own port of the SharedWorker.
2. `deliver` waits on tab B's barrier, skips writes tab B acknowledged
   itself, and requests a refresh
   (`clients/web/src/project-change-poll.ts:213-237`,
   `clients/web/src/features/project-change-streams.ts:422-426`).
3. The refresh coalesces for 180 ms quiet, at most 900 ms, active tab first
   (`clients/web/src/project-change-refresh.ts:7-8`,
   `clients/web/src/project-tab-refresh.ts:21`).
4. The selected ticket is re-read and reconciled. An active draft adopts a
   remote-only change, merges, or shows a conflict
   (`clients/web/src/ticket-field-reconciliation.ts:45-52`), and a focused
   control is updated only if it still holds the saved text
   (`clients/web/src/focused-draft-sync.ts:22-39`). Test:
   `clients/web/src/ticket-field-reconciliation.test.ts`,
   `clients/web/src/focused-draft-sync.test.ts`.

### HS2-WebClient-ReplayOverflow

1. After a drop, the stream long-polls the replay from the last applied
   cursor (`clients/web/src/project-change-poll.ts:467-486`).
2. An `overflow` answer, or a worker queue collapsed past 1,000 events
   (`clients/web/src/change-stream-worker-protocol.ts:174-187`), calls
   `resync('overflow')`. A failed replay resets the cursor and a fresh
   handshake resyncs as `reconnect` with a forced refresh
   (`clients/web/src/project-change-poll.ts:487-492`).
3. Resync re-reads permissions, notifications, live state, host health,
   settings, grants, the terminal dashboard and, except on `initial`, drive
   connections (`clients/web/src/features/project-change-streams.ts:463-482`).
4. The ticket projection is rebuilt by a full authoritative refresh; nothing
   is patched incrementally (`clients/web/src/project-change-poll.ts:110`).
   Test: `clients/web/src/project-change-poll.test.ts` ("handshakes,
   coalesces batches, ignores unrelated events, and refreshes overflow").
5. Stream health returns to true and clears the reconnecting banner
   (`clients/web/src/features/project-change-streams.ts:465`).

### HS2-WebClient-TerminalAttach

1. The drawer lists sessions over REST (`clients/web/src/features/terminal-dashboard.ts:174-230`).
2. The viewport opens `{apiPath}/terminals/<id>/attach` with
   `replay_protocol=2`, a byte cap of (scrollback + rows) x 192, optional
   `deflate-raw` and `resume_from`
   (`clients/web/src/terminal-replay-stream.ts:51-82`).
3. Replay bytes are written to xterm in one write after reset sequences, so
   the screen does not flash; live output follows in order behind any inflate
   (`clients/web/src/terminal-viewport-runtime.ts:1019-1048`). Test:
   `clients/web/tests/terminal-replay.spec.ts`.
4. The viewport claims a size; the server broadcasts `pty_size` naming who
   drives and who answers queries; other viewers scale their view
   (`clients/web/src/terminal-viewport-runtime.ts:589-632`). Test:
   `clients/web/tests/terminal-size-authority.spec.ts`.
5. Failure path: a corrupt replay or 15 s of silence abandons the socket and
   reattaches with `resume_from` (`clients/web/src/terminal-viewport-runtime.ts:944-948`,
   `clients/web/src/terminal-viewport.ts:108-142`); typed input is kept
   (`clients/web/src/terminal-viewport.ts:166-176`). Test:
   `clients/web/tests/terminal-liveness.spec.ts`.

## Discrepancies

Each discrepancy has a follow-up ticket filed from HS2-AYKHVV.

1. **Fixed-cadence polling fallback.** `AGENTS.md:53` forbids fixed-interval
   network polling. For bridges without change multiplexing, direct polls
   with no Web Locks run as zero-timeout requests paced 250 ms apart
   (`clients/web/src/change-poll-slots.ts:3`,
   `clients/web/src/change-poll-slots.ts:44-49`); with Web Locks they are
   1 s capped polls rotated across projects. Follow-up: HS2-V24KEA.
2. **No "every signal has a reset" policy test.** `AGENTS.md:200` requires
   one; no test in `clients/web` checks it. Follow-up: HS2-K40115.
3. **Baseline model wording.** The earlier `webChanges` description said
   "Change-stream workers" with Web Worker and SharedWorker technology,
   implying they always run. The worker is opt-in and off by default
   (`clients/web/src/features/project-change-streams.ts:193-199`), as
   `docs/06-clients.md` and `docs/CODEBASE-MAP.md` already say. Corrected in
   this model; no follow-up needed.
4. **Uncaught storage writes.** `saveProjectWorkspaceSession`,
   `saveWorkspacePreferences` and `saveConversationStates` do not catch
   storage exceptions (see Failure and recovery), while the draft and theme
   writers do. Follow-up: HS2-XXP9MM.
5. **Untested wiring on the HS2-RE1PS6 path.** Blur and `pagehide` flush
   wiring, reload draft restore, and single-ticket rollback have unit tests
   only for their primitives. Follow-up: HS2-YHNKK0.

Unresolved claims for this area are in the [register](../unresolved.md) (WC-U1 to WC-U4).
