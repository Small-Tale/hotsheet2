# Browser web client: coverage

Status: written by HS2-AYKHVV against audited source `8a9d46d3972aa65ebafe0a25d8a60a0ca4f76774`.
`node scripts/architecture-coverage.mjs inventory --area web` lists the
required entries; every browser entry appears below in backticks. The
[chapter](../narrative/60-web-client.md) explains the flows.

Paths in this file are under `clients/web/`. Line numbers are at the
audited revision. Unit tests are sibling `*.test.ts` files in `src/`;
Playwright specs are in `tests/` (`clients/web/playwright.config.ts:75`).

## Runtime classification used here

`clients/web/src` mixes browser modules with Node host modules. This file maps
every module reachable only from a browser entry: `clients/web/src/main.tsx`,
`clients/web/src/ux-demo/main.tsx`, `clients/web/src/standby-saver/main.tsx`,
the two worker entries and the lazily imported terminal runtime. The
classification follows runtime (non-type) imports. The web host area
(HS2-T6SD67) maps the Node modules and the modules both sides import at run
time.

- Node modules in `src/` that the browser only talks to over HTTP:
  `clients/web/src/project-bridge.ts`, `clients/web/src/terminal-ws-bridge.ts`,
  `clients/web/src/local-host.ts`, `clients/web/src/dev-server.ts`,
  `desktop-storage-bridge.ts`,
  `migration-stream.ts`, `remote-projects/connections.ts`,
  `remote-projects/host.ts`, `remote-projects/permissions.ts`,
  `remote-projects/terminals.ts`, `remote-projects/transport.ts`.
- Modules imported at run time by both sides, mapped by the web host area and
  described here where the browser depends on them:
  `clients/web/src/server-response-parsers.ts` (response shape validation,
  `ServerResponseShapeError` at `clients/web/src/server-response-parsers.ts:123`),
  `change-ws-mux.ts`, `local-bridge-client.ts`, `desktop-storage-contract.ts`,
  `host-features.ts`, `host-only-actions.ts`, `json-value.ts`,
  `registered-projects-model.ts`, `remote-projects/identity.ts`,
  `terminal-project-scope.ts`, `client-build.ts`, `client-origin.ts`,
  `compatibility.ts`, `conversation-export.ts`, `change-mux-bridge.ts` (the
  bridge half of the poll multiplexer; the browser imports its constants).
- This split matches the web host's
  [Module runtime classification](web-host.md#module-runtime-classification)
  (732 browser modules). Two browser modules reached from no runtime import
  are mapped here: `repository-state.ts` (types and pure state for repository
  status, `webHostOps`) and `stable-vite-client.ts` (the stable-dev client stub
  served at `clients/web/vite.config.ts:102`, `webShell`).
- Directories: `clients/web/src/app` and `clients/web/src/interactions` and
  `clients/web/src/interaction-attrs` belong to `webShell`;
  `clients/web/src/features` and `clients/web/src/components` are split by
  feature across the components below (each file is listed by name);
  `clients/web/src/ux-demo` belongs to `webUxDemo`;
  `clients/web/src/remote-projects` is split: `client.ts`, `host-names.ts` and
  `reconnect.ts` belong to `webRemote`, the rest to the web host area.

## Entry, runtime composition and interaction wiring (`webShell`)

- Modules: `clients/web/src/app/runtime.tsx`, `clients/web/src/features/hot-sheet-app.tsx`, `clients/web/src/features/hot-sheet-shell.tsx`, `clients/web/src/interactions/attachments-and-gallery.ts`, `clients/web/src/interactions/commands-and-ai.ts`, `clients/web/src/interactions/inspector-and-editor.ts`, `clients/web/src/interactions/notifications-and-links.ts`, `clients/web/src/interactions/project-lifecycle.ts`, `clients/web/src/interactions/shell-and-global.ts`, `clients/web/src/interactions/terminals.ts`, `clients/web/src/interactions/ticket-selection.ts`, `clients/web/src/keyboard-shortcuts.ts`
- Small modules in `clients/web/src/`: `confirmation-interactions.ts`, `confirmation.ts`, `context-menu-position.ts`, `controlled-open.ts`, `dev-latency-entry.ts`, `dev-latency.ts`, `dev-reload-diagnostics-entry.ts`, `dev-reload-diagnostics.ts`, `drawer-tab-order.ts`, `feature-areas.ts`, `file-picker-routes.ts`, `input-dialog-interactions.ts`, `interaction-performance.ts`, `lucide-catalog.ts`, `main.tsx`, `popup-menu-state.ts`, `render-metrics.ts`, `tab-cycle.ts`, `tab-order.ts`, `theme-tile-art.tsx`, `top-layer-overlay.ts`, `tracked-size-observer.ts`, `wa-dialog-semantics.ts`
- Small modules in `clients/web/src/app/`: `development-bootstrap.ts`, `interaction-bindings.ts`, `wire-interactions.ts`
- Small modules in `clients/web/src/components/`: `app-combobox-wiring.ts`, `app-empty-state.tsx`, `app-loading-indicator.tsx`, `app-shell.tsx`, `confirmation-dialog.tsx`, `connection-details-dialog.tsx`, `content-transition.tsx`, `context-menu-dismissal.ts`, `customization-palette.ts`, `feature-settings.tsx`, `flow-back-button.tsx`, `keyboard-settings.tsx`, `lucide-icon-picker.tsx`, `lucide-popular.ts`, `main-shell.tsx`, `settings-navigation.tsx`, `settings-workspace.tsx`, `shell-banner.tsx`, `theme-picker.tsx`
- Small modules in `clients/web/src/features/`: `feature-areas.ts`, `inspector-surface.ts`, `settings-surface.tsx`
- Small modules in `clients/web/src/interaction-attrs/`: `attachments-and-gallery.ts`, `commands-and-ai.ts`, `confirmation.ts`, `input-dialogs.ts`, `inspector-and-editor.ts`, `navigation-and-tabs.ts`, `notifications-and-links.ts`, `orchestration-settings.ts`, `orchestration.ts`, `project-lifecycle.ts`, `repository.ts`, `search-and-composer.ts`, `shell-and-global.ts`, `terminals.ts`, `ticket-groups.ts`, `ticket-selection.ts`, `views-and-saved-views.ts`
- Small modules in `clients/web/src/interactions/`: `dom.ts`, `gallery-shortcut-entry.ts`, `lifetime.ts`, `navigation-and-tabs.ts`, `repository.ts`, `search-and-composer.ts`, `ticket-search-field.ts`, `types.ts`, `views-and-saved-views.ts`
- Responsibility: boots the page. `clients/web/src/main.tsx:37` loads the
  `/ux-demo` catalog instead of the app when the host feature and path match
  (`clients/web/src/host-features.ts:94`). Otherwise it checks the remote
  browser session (`clients/web/src/main.tsx:39`), hydrates desktop storage
  in the release shell (`clients/web/src/main.tsx:41-42`), calls
  `startHotSheetWebClient` (`clients/web/src/app/runtime.tsx:417`) and wires
  page-lifetime Kerf UI behaviors (`clients/web/src/main.tsx:49-65`). The
  runtime is one 8,355-line composition closure: it creates every feature
  controller (for example `clients/web/src/app/runtime.tsx:1502`,
  `clients/web/src/app/runtime.tsx:3865`) and mounts the Kerf tree with
  `mount(appRoot, withControlledOpen(appRoot, HotSheetApp))`
  (`clients/web/src/app/runtime.tsx:7151`). There is no client router: the
  only path branch is `/ux-demo`. Does not talk to the server itself.
  Test: `src/interaction-wiring.test.ts` ("keeps main.tsx as a bounded
  side-effect bootstrap"), `src/host-features.test.ts`.
- State and persistence: about 233 `signal(` calls are locals of
  `startHotSheetWebClient`, for example `projects`, `selectedProjectId`,
  `tickets` and `ticketRowsByProject` (`clients/web/src/app/runtime.tsx:427-458`).
  No Kerf stores (`defineStore`) are used. Layout keys
  `hotsheet.layout.app-*.size` (`clients/web/src/app-region-resize.ts:20-22`),
  `hotsheet.show-loading-activity` (`clients/web/src/app/runtime.tsx:676`),
  `hotsheet.keyboard-shortcuts` (`clients/web/src/components/settings-navigation.tsx:160`).
  Test: `src/features/runtime-split-controllers.test.ts`.
- Types and entry points: `startHotSheetWebClient`,
  `createHotSheetShell` (`clients/web/src/features/hot-sheet-shell.tsx:261`),
  `createHotSheetApp` (`clients/web/src/features/hot-sheet-app.tsx:264`),
  `wireHotSheetInteractions` (`clients/web/src/app/wire-interactions.ts:28`).
- Concurrency: single page thread. Interactions are delegated listeners on
  `document.body` via Kerf `delegate`, for example
  `clients/web/src/interactions/ticket-selection.ts:354`, installed in a fixed
  group order and torn down newest first
  (`clients/web/src/interactions/lifetime.ts:16`). The order is pinned in
  `src/interactions/registration-inventory.txt`. Test:
  `src/interaction-wiring.test.ts`, `src/interactions/lifetime.test.ts`.
- Errors and recovery: page-level errors render through `webApi`'s
  `AppError` banner. Development builds load diagnostics lazily
  (`clients/web/src/app/development-bootstrap.ts:4-9`). Test: untested for
  the development bootstrap.
- Configuration and environment: host feature meta `hotsheet-features`
  (`clients/web/src/host-features.ts:84`), `import.meta.env.HOTSHEET_WEB_STABLE_DEV`
  (`clients/web/src/app/development-bootstrap.ts:9`).
- Invariants: modules register no listeners or timers at import time; every
  interaction group returns a teardown
  (`clients/web/src/app/wire-interactions.ts:28`). Test:
  `src/interaction-wiring.test.ts` ("preserves every ordered event/root/selector
  and capture registration").

## Workspace and project lifecycle (`webWorkspace`)

- Modules: `clients/web/src/components/workspace-controls.tsx`, `clients/web/src/features/project-lifecycle.ts`
- Small modules in `clients/web/src/`: `app-region-resize.ts`, `mobile-layout.ts`, `mobile-view-choices.ts`, `project-activation.ts`, `project-live-summary.ts`, `project-minimization.ts`, `project-settings-state.ts`, `project-startup.ts`, `project-tab-action-fit.ts`, `project-tab-order.ts`, `project-unavailable-cause.ts`, `project-unavailable-shell.ts`, `resizable-region-commits.ts`, `workspace-toolbar-visibility.ts`
- Small modules in `clients/web/src/components/`: `grid-sidebar-gauges.tsx`, `grid-sidebar-group.tsx`, `grid-sidebar-triage-row.tsx`, `project-close-dialog.tsx`, `project-dialog.tsx`, `project-general-settings.tsx`, `project-live-section.tsx`, `project-restore-error.tsx`, `project-setup-warning-banner.tsx`, `project-sidebar.tsx`, `project-source-choice-dialog.tsx`, `project-strip-actions.tsx`, `project-summary.tsx`, `project-tab-bar.tsx`, `project-tab-context-menu.tsx`, `project-tab.tsx`, `registered-projects-settings-interactions.ts`, `registered-projects-settings.tsx`, `sidebar-panel.tsx`, `workspace-composition-surfaces.tsx`, `workspace-header-interactions.ts`, `workspace-header.tsx`
- Small modules in `clients/web/src/features/`: `project-close.ts`, `project-teardown.ts`, `registered-projects.ts`, `sidebar-surface.ts`, `workspace-surface.tsx`
- Responsibility: opens, activates, minimizes, closes and tears down projects;
  renders tabs, the sidebar and the unavailable-project shell. `openProject`
  (`clients/web/src/features/project-lifecycle.ts:333`) calls
  `openProjectFetch` (`clients/web/src/project-startup.ts:12`), then
  `wireOpenedProject` (`clients/web/src/features/project-lifecycle.ts:208`)
  and `activateOpenedProject` (`clients/web/src/features/project-lifecycle.ts:285`).
  A tab the user picks during an open wins
  (`clients/web/src/project-activation.ts:18`). Test:
  `src/project-startup.test.ts`, `src/project-activation.test.ts`,
  `tests/project-startup.spec.ts`.
- State and persistence: 44 signals in the lifecycle controller; per-project
  `Record` signals in the runtime. Project switch resets composer, board pages
  and progressive rendering in one `batch` inside `activateOpenProject`
  (`clients/web/src/app/runtime.tsx:3436`). Close prunes 13 per-project
  records (`clients/web/src/app/runtime.tsx:5632-5646`) through
  `createProjectTeardown` (`clients/web/src/features/project-teardown.ts:95`).
  Keys `hotsheet.open-projects` (`clients/web/src/app/runtime.tsx:755`),
  `hotsheet.project-tab-order.v1` (`clients/web/src/project-tab-order.ts:11`),
  `hotsheet.layout.minimized-projects.v1` (`clients/web/src/project-minimization.ts:2`).
  Test: `src/features/project-teardown.test.ts`.
- Types and entry points: `createProjectLifecycleController`
  (`clients/web/src/app/runtime.tsx:1502`), `restoreRememberedProjects`
  (`clients/web/src/project-startup.ts:63`), called at
  `clients/web/src/app/runtime.tsx:8266`.
- Concurrency: remembered projects start together; the active one is
  presented first (`clients/web/src/project-startup.ts:63`). Test:
  `src/project-startup.test.ts` ("starts every unique fetch together, then
  presents the active project before the others finish").
- Errors and recovery: a failed open retries once after 500 ms
  (`clients/web/src/project-startup.ts:63`); an unhealthy server offers
  recovery (`clients/web/src/project-startup.ts:35`). Unavailable kinds are
  restore-failure, remote-loading, remote-unavailable and opening
  (`clients/web/src/project-unavailable-shell.ts:8`). Test:
  `src/project-unavailable-shell.test.ts`, `tests/project-startup.spec.ts`
  ("HS2-H0QGK7 keeps the sidebars, toolbar, and drawer for an unavailable tab").
- Configuration and environment: none beyond the keys above.
- Invariants: closing a project removes it from every per-project record and
  cancels its autosaves (`clients/web/src/features/project-teardown.ts:95`).
  Test: `src/features/project-teardown.test.ts` ("drops only the closing ids
  from every record").

## API client (`webApi`)

- Modules: `clients/web/src/api.ts`
- Small modules in `clients/web/src/`: `clipboard-when-ready.ts`, `copy-text.ts`, `open-external.ts`, `priority-wire.ts`, `request-observer.ts`, `server-busy.ts`, `server-clock.ts`
- Small modules in `clients/web/src/components/`: `app-error.tsx`, `server-busy-bars.tsx`
- Responsibility: the `Api` class (`clients/web/src/api.ts:2118`) builds
  requests against `project.apiPath` (`/__hotsheet/project-api/<id>`, assigned
  by the Node bridge) and validates every answer with a parser
  (`clients/web/src/api.ts:2137`). It adds a human actor to actor-aware
  mutations (`clients/web/src/api.ts:2030`). It never caches. Test:
  `src/api.test.ts` ("adds the human actor only to actor-aware ticket
  mutations").
- State and persistence: module singletons `serverBusy` and an in-flight
  counter with a 300 ms idle linger (`clients/web/src/server-busy.ts:9-35`).
  `CHANGE_STREAM_CLIENT_ID` is a per-tab random id
  (`clients/web/src/api.ts:2021-2024`). Test: `src/server-busy.test.ts`.
- Types and entry points: `Api`, `ApiHttpError`
  (`clients/web/src/api.ts:2103`), `HostOnlyActionError`
  (`clients/web/src/api.ts:2152`), `requestBody`
  (`clients/web/src/api.ts:2165`), `observeRequest`
  (`clients/web/src/request-observer.ts:20`).
- Concurrency: none beyond `fetch`; `trackBusy: false` clients stay off the
  busy indicator. Test: `src/api.test.ts` ("tracks ordinary requests but
  leaves quiet background clients off the busy indicator").
- Errors and recovery: no generic retry. Partial and truncated source pages
  are read from headers (`clients/web/src/api.ts:2369`). The browser sends an
  empty secret; the Node bridge injects credentials. Test: `src/api.test.ts`.
- Configuration and environment: none.
- Invariants: every endpoint result passes a shape parser before use.
  Test: `src/server-response-parsers.test.ts`.

## Change streams and refresh (`webChanges`)

- Modules: `clients/web/src/features/project-change-streams.ts`, `clients/web/src/project-change-poll.ts`
- Small modules in `clients/web/src/`: `change-poll-mux.ts`, `change-poll-slots.ts`, `change-read-gap.ts`, `change-stream-shared-hub.ts`, `change-stream-worker-host.ts`, `change-stream-worker-protocol.ts`, `change-stream-worker-runtime.ts`, `change-stream.shared-worker.ts`, `change-stream.worker.ts`, `grid-sidebar-live.ts`, `live-state-icons.ts`, `live-state.ts`, `local-ticket-changes.ts`, `project-change-refresh.ts`, `project-tab-refresh.ts`, `project-ticket-refresh.ts`, `refresh-barrier.ts`, `search-projection-coalescing.ts`, `source-stale-notice.ts`, `versioned-long-poll.ts`, `worker-ticket-refresh.ts`, `worker-view-stream.ts`
- Small modules in `clients/web/src/components/`: `connection-state-banner.tsx`, `remote-reconnecting-banner.tsx`, `source-stale-notice-banner.tsx`, `ticket-store-sync-banner.tsx`
- Small modules in `clients/web/src/features/`: `project-refresh.ts`, `ticket-store-sync.ts`
- Responsibility: keeps one stream per open project
  (`clients/web/src/features/project-change-streams.ts:401`), on the main
  thread by default (`clients/web/src/features/project-change-streams.ts:372`).
  Transport is WebSocket first at `{api}/ws/sync`, after a zero-cursor poll
  handshake (`clients/web/src/project-change-poll.ts:349-358`), with long
  poll `{api}/ws/poll?timeout_ms=25000` (`clients/web/src/api.ts:3677-3680`).
  Bridge multiplexers carry many projects on one socket or one poll request.
  Test: `src/project-change-poll.test.ts`, `tests/change-poll-mux.spec.ts`.
- State and persistence: `LiveStateStore.states`
  (`clients/web/src/live-state.ts:153-171`), the ticket-store sync signal
  (`clients/web/src/features/ticket-store-sync.ts:15-53`). Nothing persisted
  except the opt-in keys below.
- Types and entry points: `createProjectChangeConsumer.deliver`
  (`clients/web/src/project-change-poll.ts:213-237`), worker host
  (`clients/web/src/change-stream-worker-host.ts:73`), protocol version 1
  (`clients/web/src/change-stream-worker-protocol.ts:11`), refresh coalescer
  (`clients/web/src/project-change-refresh.ts:7-8`), refresh barrier
  (`clients/web/src/refresh-barrier.ts:6-27`).
- Concurrency: SharedWorker (one per origin, named
  `hotsheet-change-stream`, `clients/web/src/features/project-change-streams.ts:224-227`)
  or dedicated `Worker` (`clients/web/src/features/project-change-streams.ts:266-275`)
  only when `hotsheet.change-stream-worker` is `true`
  (`clients/web/src/features/project-change-streams.ts:193-199`). The shared
  hub gives each tab port its own runtime and shares socket and poll pools
  (`clients/web/src/change-stream-shared-hub.ts:8-47`); ports heartbeat every
  20 s and are swept after 90 s idle
  (`clients/web/src/features/project-change-streams.ts:235-241`,
  `clients/web/src/change-stream-shared-hub.ts:75-82`). Direct polls are capped
  at four per origin by Web Locks
  (`clients/web/src/change-poll-slots.ts:2`,
  `clients/web/src/change-poll-slots.ts:35`). Test:
  `src/change-stream-shared-hub.test.ts`, `src/change-poll-slots.test.ts`,
  `tests/change-stream-shared-worker.spec.ts`. Page-side heartbeat and
  `pagehide` disposal: untested.
- Errors and recovery: backoff 500 ms doubling to 30 s
  (`clients/web/src/project-change-poll.ts:242-243`); a wake or `online`
  resets it (`clients/web/src/app/runtime.tsx:5151-5154`). A worker that does
  not answer `ready` within 2 s, reports another protocol, or crashes more
  than 3 times in 60 s falls back to the main thread from the last applied
  cursor (`clients/web/src/change-stream-worker-host.ts:73-76`,
  `clients/web/src/change-stream-worker-host.ts:128-140`). Read gaps: a
  WebSocket cursor jump (`clients/web/src/project-change-poll.ts:401-403`),
  the server `overflow` flag (`clients/web/src/project-change-poll.ts:218`),
  a worker queue over 1,000 events collapsed to one overflow page
  (`clients/web/src/change-stream-worker-protocol.ts:143`), and a ticket read
  stamped below the handshake cursor
  (`clients/web/src/change-read-gap.ts:80-131`). Test:
  `src/change-stream-worker.test.ts`, `src/change-read-gap.test.ts`,
  `tests/change-stream-worker.spec.ts`.
- Configuration and environment: localStorage `hotsheet.change-stream-worker`
  (default off), `hotsheet.change-stream-shared-worker` (default on when a
  worker is used), `hotsheet.change-stream-worker-ready-timeout-ms`
  (development builds only,
  `clients/web/src/features/project-change-streams.ts:172`).
- Invariants: no refresh runs while a local mutation holds the barrier
  (`clients/web/src/project-change-poll.ts:226`); this tab's acknowledged
  writes do not trigger refetches
  (`clients/web/src/features/project-change-streams.ts:422-426`). Test:
  `src/refresh-barrier.test.ts`, `src/project-change-poll.test.ts` ("rechecks
  an invalidation after the local mutation barrier settles").

## Tickets, board, search and reader (`webTickets`)

- Modules: `clients/web/src/components/note-card.tsx`, `clients/web/src/components/ticket-code-review.tsx`, `clients/web/src/components/ticket-inspector.tsx`, `clients/web/src/components/ticket-source-setup-dialog.tsx`, `clients/web/src/features/ticket-workflows.ts`, `clients/web/src/features/workspace-search.ts`
- Small modules in `clients/web/src/`: `advanced-search.ts`, `automatic-ticket-group.ts`, `board-pagination.ts`, `bulk-update-progress.ts`, `inline-search-caret.ts`, `inline-search.ts`, `minimized-dialogs.ts`, `minimized-readers.ts`, `new-ticket-source.ts`, `note-authorship.ts`, `note-response-link.ts`, `pending-created-tickets.ts`, `reader-minimize-motion.ts`, `saved-views.ts`, `search-collapse.ts`, `ticket-blockers.ts`, `ticket-blocking.ts`, `ticket-board-layout.ts`, `ticket-bulk-operations.ts`, `ticket-clipboard-shortcuts.ts`, `ticket-close.ts`, `ticket-completion-trend.ts`, `ticket-creator.ts`, `ticket-deep-link.ts`, `ticket-display-number.ts`, `ticket-drop-target.ts`, `ticket-editor-size.ts`, `ticket-follow-up-link.ts`, `ticket-group-state.ts`, `ticket-groups.ts`, `ticket-link-resolution.ts`, `ticket-motion.ts`, `ticket-mutation.ts`, `ticket-operations.ts`, `ticket-prefix.ts`, `ticket-reader-focus.ts`, `ticket-reader-stack.ts`, `ticket-reference-search.ts`, `ticket-row-containment.ts`, `ticket-search-model.ts`, `ticket-timeline-data.ts`, `ticket-title-disclosure.ts`, `ticket-transfer.ts`, `ticket-usage-timeline.ts`, `ticket-views.ts`, `workspace-ticket-sort.ts`
- Small modules in `clients/web/src/components/`: `bulk-ticket-dialog-interactions.ts`, `bulk-ticket-dialog.tsx`, `category-presentation.ts`, `markdown-preview.tsx`, `minimized-tickets-control.tsx`, `note-metadata-dialog.tsx`, `priority-presentation.ts`, `quick-ticket-composer-interactions.ts`, `quick-ticket-composer.tsx`, `reader-overlay-surfaces.tsx`, `saved-view-dialog.tsx`, `search-scope-toolbar.tsx`, `status-badge.tsx`, `tag-chip.tsx`, `ticket-attachments-interactions.ts`, `ticket-attachments.tsx`, `ticket-blocker-dialog.tsx`, `ticket-blockers.tsx`, `ticket-board-column.tsx`, `ticket-board.tsx`, `ticket-category-select.tsx`, `ticket-close-dialog.tsx`, `ticket-collection-interactions.ts`, `ticket-content-editor-interactions.ts`, `ticket-duplicate-backlinks.tsx`, `ticket-empty-state.tsx`, `ticket-field-conflict.tsx`, `ticket-field-editor-interactions.ts`, `ticket-follow-up-link-dialog.tsx`, `ticket-group-context-menu.tsx`, `ticket-group-dialog.tsx`, `ticket-group-interactions.ts`, `ticket-group-merge-dialog.tsx`, `ticket-group-operation-banner.tsx`, `ticket-group-settings-interactions.ts`, `ticket-group-settings.tsx`, `ticket-group.tsx`, `ticket-info-panel.tsx`, `ticket-inspector-panel.tsx`, `ticket-inspector-placeholder.tsx`, `ticket-inspector-surface.tsx`, `ticket-link-choice-dialog.tsx`, `ticket-list.tsx`, `ticket-notes.tsx`, `ticket-page-more.tsx`, `ticket-priority-select.tsx`, `ticket-reader.tsx`, `ticket-reference-picker.tsx`, `ticket-row-context-menu.tsx`, `ticket-row.tsx`, `ticket-search-field.tsx`, `ticket-selection.ts`, `ticket-source-color-picker.tsx`, `ticket-source-icon.tsx`, `ticket-source-marks.ts`, `ticket-sources-settings.tsx`, `ticket-status-menu.tsx`, `ticket-tag-editor.tsx`, `ticket-timeline.tsx`, `view-navigation.tsx`
- Small modules in `clients/web/src/features/`: `automatic-ticket-grouping.ts`, `board-column-paging.ts`, `follow-up-link.ts`, `linked-reader-subject.ts`, `minimized-dialogs.ts`, `minimized-readers.ts`, `reader-layers-surface.tsx`, `saved-views.ts`, `ticket-blockers.ts`, `ticket-group-settings.ts`, `ticket-groups.ts`, `ticket-view-surface.tsx`
- Responsibility: list and board rendering, groups, saved views (stored on
  the server at `/views`, `clients/web/src/api.ts:3655`), search, inspector,
  stacked readers, notes, blockers, bulk operations and ticket writes.
  `applyTicketPatch` (`clients/web/src/features/ticket-workflows.ts:604`)
  projects the patch at once (`clients/web/src/features/ticket-workflows.ts:629-633`).
  Test: `src/ticket-mutation.test.ts`, `tests/ticket-edit-sequencing.spec.ts`.
- State and persistence: runtime signals `tickets`, `ticketRowsByProject`,
  `ticketCountsByProject`, `boardColumnPages`, `selectedTicket`, written by
  `refreshProject` (`clients/web/src/features/project-refresh.ts:136-308`).
  Undo history of 30 transactions (`clients/web/src/ticket-operations.ts:21`).
  Created rows stay visible until the index has them
  (`clients/web/src/pending-created-tickets.ts:23`). Keys
  `hotsheet.ticket-groups.collapsed` (`clients/web/src/ticket-group-state.ts:20`),
  `hotsheet.ticket-editor-height` (`clients/web/src/ticket-editor-size.ts:4`),
  `hotsheet.minimized-readers.v1` (`clients/web/src/minimized-readers.ts:55`).
  Test: `src/features/project-refresh.test.ts`,
  `src/pending-created-tickets.test.ts`, `src/ticket-operations.test.ts`.
- Types and entry points: `applyTicketPatch`, `applyBulkOperations`
  (`clients/web/src/features/ticket-workflows.ts:1301`),
  `projectTicketPatch` (`clients/web/src/ticket-mutation.ts:17-20`),
  `loadProjectTicketRefresh` (`clients/web/src/project-ticket-refresh.ts:164`).
- Concurrency: writes are serialized per ticket by
  `singleTicketMutationSequencer.enqueue`
  (`clients/web/src/features/ticket-workflows.ts:638-640`); each write holds
  the refresh barrier. Test: `tests/ticket-concurrency-retry.spec.ts`.
- Errors and recovery: concurrency conflicts rebase and retry up to 4 times
  (`clients/web/src/features/ticket-workflows.ts:487`); failure restores the
  row and selection when the generation still matches
  (`clients/web/src/features/ticket-workflows.ts:795-808`); bulk rollback at
  `clients/web/src/features/ticket-workflows.ts:1471`. Outcome is reported as
  `hotsheet:mutation-timing` (`clients/web/src/ticket-mutation.ts:35-37`).
  Test: single-ticket rollback is untested at unit level;
  `tests/providers.spec.ts` ("keeps successful best-effort writes when another
  selected ticket fails (HS2-967BWM)") covers bulk partial failure.
- Configuration and environment: provider write-behind is used when the
  provider capability allows it
  (`clients/web/src/features/ticket-workflows.ts:652-656`).
- Invariants: optimistic patches contain only renderable fields
  (`clients/web/src/ticket-mutation.ts:4-15`). Test: `src/ticket-mutation.test.ts`
  ("projects renderable fields immediately without leaking command-only fields").

## Drafts and edit coalescing (`webDrafts`)

- Modules: none at 500 lines or more
- Small modules in `clients/web/src/`: `browser-draft-file-contract.ts`, `browser-draft-mutation-queue.ts`, `composer-restore-intent.ts`, `debounced-autosave.ts`, `desktop-draft-files.ts`, `desktop-recovery-flush.ts`, `draft-file-recovery.ts`, `draft-input-coalescing.ts`, `draft-submission-state.ts`, `feedback-response-drafts.ts`, `focused-draft-sync.ts`, `inline-editor-escape.ts`, `markdown-click-edit.ts`, `text-merge.ts`, `ticket-draft-store.ts`, `ticket-field-reconciliation.ts`, `ticket-title-editing.ts`, `trailing-task-internal.ts`, `trailing-task.ts`
- Small modules in `clients/web/src/components/`: `markdown-editor.tsx`, `note-composer.tsx`
- Responsibility: never writes to the server while the user types
  (HS2-RE1PS6). `createFocusLossAutosave`
  (`clients/web/src/debounced-autosave.ts:89-155`) writes a local recovery
  copy 150 ms after the last keystroke
  (`clients/web/src/debounced-autosave.ts:1`) and calls the server `save` only
  in `flush()` (`clients/web/src/debounced-autosave.ts:106-129`). Flush runs on
  blur (`clients/web/src/interactions/inspector-and-editor.ts:684-689`), title
  finish (`clients/web/src/interactions/inspector-and-editor.ts:575-578`),
  `pagehide` (`clients/web/src/app/runtime.tsx:8197-8203`) and before an
  update reload (`clients/web/src/app/runtime.tsx:8207-8214`). Test:
  `src/debounced-autosave.test.ts` ("persists locally while typing and saves to
  the server only on flush"); blur and pagehide wiring untested.
- State and persistence: localStorage
  `hotsheet.ticket-draft:<project>:<qualifiedId>:<field>[:<noteId>]` holding
  `{base, draft, at, pendingIntent?}`
  (`clients/web/src/ticket-draft-store.ts:10-33`); staged files in IndexedDB
  `hotsheet-draft-files-v1/files` (`clients/web/src/workspace-session.ts:51-52`)
  or desktop draft files (`clients/web/src/desktop-draft-files.ts:8-38`).
  Memory only: feedback reply owners, composer restore intent and local change
  acknowledgements (`clients/web/src/local-ticket-changes.ts:8-9`). Test:
  `src/ticket-draft-store.test.ts`, `src/browser-draft-file-contract.test.ts`,
  `tests/browser-draft-files.spec.ts`.
- Types and entry points: `coalescedDraft` and `flushDraftInputs`
  (`clients/web/src/draft-input-coalescing.ts:13-51`), `mergeText`
  (`clients/web/src/text-merge.ts:171-177`), `reconcileActiveDraft`
  (`clients/web/src/ticket-field-reconciliation.ts:45-52`),
  `syncFocusedDraftControl` (`clients/web/src/focused-draft-sync.ts:22-39`).
- Concurrency: a flush during a running save waits and then saves the newest
  value (`clients/web/src/debounced-autosave.ts:107-110`); draft-file
  mutations are ordered per key
  (`clients/web/src/browser-draft-mutation-queue.ts:2-16`). Test:
  `src/debounced-autosave.test.ts` ("serializes a newer edit behind an active
  save"), `src/browser-draft-mutation-queue.test.ts`.
- Errors and recovery: on reload a stored draft that differs from base and
  current value is reconciled; a conflict discards it with a toast
  (`clients/web/src/features/ticket-workflows.ts:917-937`). The recovery copy
  is cleared only when no newer edit replaced it
  (`clients/web/src/debounced-autosave.ts:123`). Native close capture throws
  when no destination exists (`clients/web/src/ticket-draft-store.ts:59-68`).
  Test: `src/ticket-draft-store.test.ts` ("HS2-E0H7S0 exposes
  unavailable/quota capture while ordinary browser writes stay tolerant");
  the restore wiring itself is untested.
- Configuration and environment: none.
- Invariants: a draft equal to its base is removed, not stored
  (`clients/web/src/ticket-draft-store.ts:47`); `mergeText` is a three-way
  merge with an LCS size guard of 4,000,000 cells
  (`clients/web/src/text-merge.ts:21`). Test: `src/text-merge.test.ts`.

## Browser persistence (`webPersistence`)

- Modules: none at 500 lines or more
- Small modules in `clients/web/src/`: `browser-id.ts`, `desktop-storage.ts`, `document-scroll-restore.ts`, `project-warm-cache.ts`, `session-restore-guard.ts`, `theme-preference.ts`, `ticket-category-preference.ts`, `ticket-detail-cache.ts`, `ticket-scroll-state.ts`, `workspace-preferences.ts`, `workspace-session.ts`, `workspace-view-memory.ts`
- Responsibility: localStorage records for preferences and per-project
  sessions, the desktop mirror, and in-memory caches. In the release shell
  `initializeDesktopStorage` loads `GET /__hotsheet/desktop-storage` and patches
  `Storage.prototype` so every `hotsheet.*` write is mirrored with a revision
  (`clients/web/src/desktop-storage.ts:10-31`,
  `clients/web/src/desktop-storage.ts:88-110`). Test:
  `src/desktop-storage.test.ts` ("hydrates before defaults, imports visible
  old data, and honors tombstones").
- State and persistence: `hotsheet.workspace.project-session.v1.<id>` and
  `hotsheet.workspace.active-project-root.v1`
  (`clients/web/src/workspace-session.ts:47-48`),
  `hotsheet.layout.workspace-preferences.v1`
  (`clients/web/src/workspace-preferences.ts:46`), `hotsheet.theme`
  (`clients/web/src/theme-preference.ts:12`),
  `hotsheet.ticket-composer.last-category.v1`
  (`clients/web/src/ticket-category-preference.ts:3`). Memory only: warm
  project LRU of 8 ids (`clients/web/src/project-warm-cache.ts:6`), ticket
  detail LRU of 50 (`clients/web/src/ticket-detail-cache.ts:14`), scroll memory
  (`clients/web/src/ticket-scroll-state.ts:132-133`). Test:
  `src/workspace-session.test.ts`, `src/project-warm-cache.test.ts`,
  `src/ticket-detail-cache.test.ts`, `tests/project-tab-warm-cache.spec.ts`.
- Types and entry points: `initializeDesktopStorage`, `flushForNativeClose`
  (`clients/web/src/desktop-storage.ts:126-137`),
  `loadProjectWorkspaceSession` (`clients/web/src/workspace-session.ts:97`).
- Concurrency: desktop writes that fail stay pending and are retried on the
  next mutation or on `pagehide` with `keepalive` under 60 KB
  (`clients/web/src/desktop-storage.ts:36-73`). Test: `src/desktop-storage.test.ts`
  ("retains failed writes and retries on page hide without logging values").
- Errors and recovery: reads are wrapped in try/catch; several writes are not
  (`clients/web/src/workspace-session.ts:161-167`,
  `clients/web/src/workspace-preferences.ts:164-166`), so a storage exception
  reaches the caller. No explicit quota handling. Test: untested for the
  unwrapped writes.
- Configuration and environment: `window.__HOTSHEET_DESKTOP_STORAGE__`
  selects the desktop mirror (`clients/web/src/main.tsx:41-42`).
- Invariants: values are never logged; a `null` value is a tombstone
  (`clients/web/src/desktop-storage.ts:88-110`).

## Terminal frontend (`webTerminal`)

- Modules: `clients/web/src/components/terminal-dashboard.tsx`, `clients/web/src/components/terminal-drawer.tsx`, `clients/web/src/features/terminal-drawer.ts`, `clients/web/src/features/terminal-viewports.ts`, `clients/web/src/terminal-viewport-runtime.ts`
- Small modules in `clients/web/src/`: `mobile-terminal-columns.ts`, `mobile-terminal-focus.ts`, `terminal-ai-connection.ts`, `terminal-clear.ts`, `terminal-clipboard.ts`, `terminal-feedback-primary.ts`, `terminal-grid-layout.ts`, `terminal-keys.ts`, `terminal-limits.ts`, `terminal-long-press.ts`, `terminal-names.ts`, `terminal-progressive-work.ts`, `terminal-query-replies.ts`, `terminal-question-answer.ts`, `terminal-replay-stream.ts`, `terminal-search-count-fit.ts`, `terminal-search.ts`, `terminal-snapshot-refresh.ts`, `terminal-theme.ts`, `terminal-ticket-links.ts`, `terminal-touch-scroll.ts`, `terminal-touch-selection.ts`, `terminal-viewport.ts`, `terminal-visibility.ts`
- Small modules in `clients/web/src/components/`: `terminal-clipboard-dialogs.tsx`, `terminal-component-interactions.ts`, `terminal-focus-search.tsx`, `terminal-key-bar.tsx`, `terminal-memory-settings.tsx`, `terminal-operations-sidebar.tsx`, `terminal-rename-dialog.tsx`, `terminal-ticket-rail.tsx`, `terminal-visibility-dialog.tsx`
- Small modules in `clients/web/src/features/`: `terminal-dashboard.ts`, `terminal-drawer-geometry.ts`, `terminal-limits-settings.ts`, `terminal-names.ts`, `terminal-operations-surface.ts`, `terminal-presentation.tsx`, `terminal-rail-surface.tsx`
- Responsibility: one xterm 6 instance per viewport
  (`clients/web/package.json:57-60`), attached at
  `{apiPath}/terminals/<id>/attach` without credentials in the URL
  (`clients/web/src/terminal-viewport.ts:364-371`), with replay protocol 2
  query parameters (`clients/web/src/terminal-replay-stream.ts:70-82`). The
  drawer, dashboard grid and magnified card share one instance that moves
  between hosts (`clients/web/src/features/terminal-viewports.ts:187-225`).
  Test: `src/terminal-viewport.test.ts`, `tests/terminal-replay.spec.ts`.
- State and persistence: per-viewport state is closure variables surfaced on
  `element.dataset` (`clients/web/src/terminal-viewport-runtime.ts:505-550`);
  app signals `terminalDrawerVisible`, `terminalDrawerSelected` and
  `terminalVisibility` (`clients/web/src/app/runtime.tsx:768-819`). Keys
  `hotsheet.terminals.*`, for example
  `hotsheet.terminals.visibility-groups` (`clients/web/src/terminal-visibility.ts:6`)
  and `hotsheet.terminals.mobile-columns` (`clients/web/src/mobile-terminal-columns.ts:4`).
  Test: `src/terminal-visibility.test.ts`.
- Types and entry points: `mountTerminalViewportRuntime`, imported lazily
  (`clients/web/src/terminal-viewport.ts:402`); replay sequencer
  (`clients/web/src/terminal-replay-stream.ts:154-212`);
  `refreshTerminalDashboard` (`clients/web/src/features/terminal-dashboard.ts:174-230`).
- Concurrency: resize claims are sent once per animation frame after layout
  settles, on visibility change and on a 5 s heartbeat
  (`clients/web/src/terminal-viewport-runtime.ts:614-632`,
  `clients/web/src/terminal-viewport-runtime.ts:857-877`); the server names
  the driving and answering viewer. Parked viewports keep their socket; the
  warm cap evicts the oldest
  (`clients/web/src/features/terminal-viewports.ts:204-225`). Test:
  `tests/terminal-size-authority.spec.ts`, `src/features/terminal-viewports.test.ts`.
- Errors and recovery: reconnect backoff `min(8000, 250 * 2^attempt)` ms
  (`clients/web/src/terminal-viewport.ts:166`); 15 s without inbound frames
  marks the socket stale (`clients/web/src/terminal-viewport.ts:108-142`); a
  corrupt replay abandons the socket and reattaches
  (`clients/web/src/terminal-viewport-runtime.ts:944-948`); input buffers up
  to 64 KiB while reconnecting. A failed runtime import shows a visible error
  (`clients/web/src/terminal-viewport.ts:443-468`). Test:
  `tests/terminal-liveness.spec.ts`, `src/terminal-viewport-mount.test.ts`.
- Configuration and environment: history lines and warm terminals default to
  5000 and 12, or 2500 and 4 on phones
  (`clients/web/src/terminal-limits.ts:34-60`); WebGL only on Firefox
  dedicated viewports (`clients/web/src/terminal-viewport.ts:250-254`). Test:
  `src/terminal-limits.test.ts`.
- Invariants: replayed bytes never trigger query replies
  (`clients/web/src/terminal-viewport-runtime.ts:562-569`). Test:
  `src/terminal-viewport-runtime.test.ts` ("never answers the past queries in
  replayed output, only live ones").

## Attachments and gallery (`webAttachments`)

- Modules: `clients/web/src/components/attachment-gallery.tsx`, `clients/web/src/features/gallery.tsx`
- Small modules in `clients/web/src/`: `annotation-intents.ts`, `attachment-filename-editing.ts`, `attachment-filename-interactions.ts`, `attachment-files.ts`, `attachment-grouping.ts`, `attachment-label-editing.ts`, `attachment-owner.ts`, `attachment-references.ts`, `drag-leave.ts`, `gallery-annotation-editor.ts`, `gallery-annotation-gesture.ts`, `gallery-annotation-workflow.ts`, `gallery-crop.ts`, `gallery-drag-modifiers.ts`, `new-ticket-attachments.ts`, `project-tab-file-drop.ts`, `video-posters.ts`
- Small modules in `clients/web/src/components/`: `attachment-context-menu-interactions.ts`, `attachment-context-menu.tsx`, `attachment-filename-editor.tsx`, `attachment-gallery-interactions.ts`, `pending-attachment-picker.tsx`
- Responsibility: attachment upload, rename, label, grouping and the gallery
  with crop and annotation editing, created by `createGalleryController`
  (`clients/web/src/features/gallery.tsx:53`, wired at
  `clients/web/src/app/runtime.tsx:1129`). Test: `tests/note-attachment-gallery.spec.ts`,
  `tests/gallery-shortcuts.spec.ts`.
- State and persistence: 23 gallery signals, for example
  `attachmentGalleryCropMode` (`clients/web/src/features/gallery.tsx:77-80`).
  Label and filename drafts use `webDrafts`.
- Types and entry points: `createGalleryController`,
  `gallery-annotation-workflow.ts`.
- Concurrency: none beyond request ordering in `webDrafts`.
- Errors and recovery: unreadable staged files stay referenced and block
  submit (`clients/web/src/draft-file-recovery.ts:53-69`). Test:
  `src/draft-file-recovery.test.ts`.
- Configuration and environment: none.
- Invariants: none recorded beyond `webDrafts`.

## AI conversation, drive and permissions (`webAi`)

- Modules: `clients/web/src/ai-conversation.ts`, `clients/web/src/components/ai-conversation.tsx`, `clients/web/src/components/permission-request-card.tsx`, `clients/web/src/components/permission-rules-settings.tsx`, `clients/web/src/components/provider-setup-form.tsx`, `clients/web/src/features/ai-configuration.tsx`, `clients/web/src/features/permissions.tsx`
- Small modules in `clients/web/src/`: `ai-launch-dialog.ts`, `ai-provider-defaults.ts`, `conversation-activity-disclosure.ts`, `conversation-composers.ts`, `conversation-context-menu.ts`, `conversation-history.ts`, `conversation-persistence.ts`, `conversation-render-scheduler.ts`, `conversation-scroll.ts`, `conversation-selection.ts`, `conversation-unread.ts`, `custom-ai-command.ts`, `drive-interrupt-reply.ts`, `feedback-choice-lines.ts`, `feedback-choices.ts`, `feedback-needed.ts`, `feedback-prompt-edits.ts`, `feedback-replies.ts`, `feedback-session-labels.ts`, `halted-sessions.ts`, `minimized-composer.ts`, `not-working-workflow.ts`, `note-repair-proposal.ts`, `permission-decision-workflow.ts`, `permission-notifications.ts`, `permission-rule-editor-focus.ts`, `project-drive.ts`, `turn-clock.ts`
- Small modules in `clients/web/src/components/`: `ai-content-label.tsx`, `ai-conversation-interactions.ts`, `ai-feedback-dialog.tsx`, `ai-launch-dialog.tsx`, `ai-tool-settings.tsx`, `choice-card.tsx`, `codex-hooks-notice-banner.tsx`, `conversation-export-dialog.tsx`, `drive-control.tsx`, `drive-options-menu.tsx`, `effort-gauge-icon.ts`, `embedded-composer-clearance.ts`, `halted-session-popup.tsx`, `manual-model-dialog.tsx`, `not-working-dialog.tsx`, `note-delivery-popup.tsx`, `note-repair-review.tsx`, `provider-icon.tsx`, `provider-model-effort-menu.tsx`
- Small modules in `clients/web/src/features/`: `conversation-archive.tsx`, `drive-conversations.ts`, `halted-sessions.tsx`, `minimized-composer.ts`
- Responsibility: conversation rendering and composers, AI launch and drive
  (`clients/web/src/features/drive-conversations.ts:89`), AI configuration
  (`clients/web/src/features/ai-configuration.tsx:51`), conversation archive
  (`clients/web/src/features/conversation-archive.tsx:49`), permission
  requests and rules (`clients/web/src/features/permissions.tsx:58`), halted
  sessions (`clients/web/src/features/halted-sessions.tsx:17`). Test:
  `src/features/controllers.test.ts`, `tests/orchestrator-permission.spec.ts`.
- State and persistence: transcripts in `hotsheet.ai-conversations.v1`,
  written at most every 250 ms with a 1 s maximum wait
  (`clients/web/src/conversation-persistence.ts:3`,
  `clients/web/src/conversation-persistence.ts:112-129`); read marks
  `hotsheet.ai-conversation-reads.v1` (`clients/web/src/conversation-unread.ts:13`);
  `hotsheet.permission-history` (`clients/web/src/features/permissions.tsx:62`).
  Test: `src/conversation-persistence.test.ts`.
- Types and entry points: controllers above; turn events arrive from
  `webChanges` (`clients/web/src/features/project-change-streams.ts:488-605`).
- Concurrency: permission refreshes are coalesced per project and never
  drive the busy indicator. Test: `src/features/controllers.test.ts`
  ("reconciles permissions as invisible background work that never drives the
  busy indicator (HS2-7G3C19)").
- Errors and recovery: a failed optimistic permission decision is restored.
  Test: `src/features/controllers.test.ts` ("restores failed optimistic
  permission decisions").
- Configuration and environment: `hotsheet.permission-automation-revision`
  (`clients/web/src/features/permissions.tsx:91`).
- Invariants: an orchestrator permission card is never decided
  automatically. Test: `src/features/controllers.test.ts` ("never decides an
  orchestrator card automatically").

## Orchestration UI (`webOrchestration`)

- Modules: `clients/web/src/components/orchestration-inspector.tsx`, `clients/web/src/components/orchestration-project-detail.tsx`, `clients/web/src/features/orchestration-settings.ts`, `clients/web/src/features/orchestration-view.ts`, `clients/web/src/orchestration-scheduler.ts`, `clients/web/src/orchestration-settings-model.ts`, `clients/web/src/worker-view-model.ts`
- Small modules in `clients/web/src/`: `active-ticket-work.ts`, `advisory-claim-resource.ts`, `ci-presentation.ts`, `claim-holder.ts`, `development-strategy-tags.ts`, `orchestration-built-ahead.ts`, `orchestration-capabilities.ts`, `orchestration-hand-back.ts`, `orchestration-plan-health.ts`, `orchestration-plan.ts`, `orchestration-progress.ts`, `orchestration-refusals.ts`, `orchestration-remote-labels.ts`, `orchestration-resources.ts`, `orchestration-run-checks.ts`, `orchestration-timeline-board.ts`, `orchestration-timeline.ts`, `orchestration-validation-steps.ts`, `overnight-timeline.ts`, `timeline-stage-estimate.ts`, `worker-provisional.ts`
- Small modules in `clients/web/src/components/`: `active-claim.tsx`, `advisory-claim-notice.tsx`, `confidence-badge.tsx`, `confidence-calibration.tsx`, `development-strategy-settings-interactions.ts`, `development-strategy-settings.tsx`, `orchestration-built-ahead-section.tsx`, `orchestration-decisions-dialog.tsx`, `orchestration-inspector-section.tsx`, `orchestration-limits-help.tsx`, `orchestration-overview.tsx`, `orchestration-pattern-field.tsx`, `orchestration-plan-problem.tsx`, `orchestration-plan-section.tsx`, `orchestration-policy-settings.tsx`, `orchestration-settings-interactions.ts`, `orchestration-settings.tsx`, `orchestration-timeline-interactions.ts`, `orchestration-timeline-view.tsx`, `orchestration-toolbar.tsx`, `orchestration-validation-section.tsx`, `orchestration-why-not-running-section.tsx`, `phase-estimate.tsx`, `worker-columns.tsx`
- Small modules in `clients/web/src/features/`: `claim-clock.ts`, `orchestration-settings-surface.tsx`, `orchestration-target-lines.ts`
- Responsibility: the orchestration view controller
  (`clients/web/src/features/orchestration-view.ts:359`, wired at
  `clients/web/src/app/runtime.tsx:3865`), plan, timeline, inspector,
  settings and policy surfaces, worker columns, claim clocks
  (`clients/web/src/app/runtime.tsx:1054`). Test:
  `src/features/orchestration-view.test.ts`, `tests/orchestration-view.spec.ts`,
  `tests/orchestration-golden-path.spec.ts`.
- State and persistence: controller signals such as `loading`, `busy`
  (`clients/web/src/features/orchestration-view.ts:370-374`); an actor id in
  `hotsheet.orchestrationPersonId` (`clients/web/src/features/orchestration-view.ts:74-91`).
  Advisory claim worker id and operation keys in `sessionStorage`
  (`clients/web/src/app/runtime.tsx:2278-2331`).
- Types and entry points: `createOrchestrationViewController`,
  `createOrchestrationTargetLines` (`clients/web/src/app/runtime.tsx:3903`).
- Concurrency: worker views follow one cursor long poll per Git source
  (`clients/web/src/worker-view-stream.ts:8-55`). Test:
  `src/worker-view-stream.test.ts`.
- Errors and recovery: a project not declared orchestration falls back to
  the previous view mode (`clients/web/src/features/orchestration-view.ts:439-445`).
  Test: `src/features/orchestration-view.test.ts`.
- Configuration and environment: none beyond the keys above.
- Invariants: the person id matches `^[A-Za-z0-9._-]{1,160}$` or is replaced
  (`clients/web/src/features/orchestration-view.ts:78`).

## Commands and schedules (`webCommands`)

- Modules: `clients/web/src/components/command-settings-editor.tsx`
- Small modules in `clients/web/src/`: `command-group-dialog.ts`, `command-hold.ts`, `command-order.ts`, `command-schedule-form.ts`, `command-schedules.ts`, `command-settings-workflow.ts`
- Small modules in `clients/web/src/components/`: `command-group-dialog.tsx`, `command-icon.ts`, `command-navigation.tsx`, `command-run-dialog.tsx`, `command-schedule-dialog-interactions.ts`, `command-schedule-dialog.tsx`, `command-settings-editor-interactions.ts`
- Small modules in `clients/web/src/features/`: `command-schedules.ts`, `commands.tsx`
- Responsibility: command editor, groups, ordering, press-and-hold runs
  (`clients/web/src/features/commands.tsx:72`) and schedules
  (`clients/web/src/app/runtime.tsx:1261`). Test:
  `src/features/controllers.test.ts`, `src/command-settings-workflow.test.ts`.
- State and persistence: 14 controller signals; write order key
  `hotsheet.command-write-order.v1` (`clients/web/src/features/commands.tsx:36`).
  The editor draft is per project; pending saves flush on `pagehide`
  (`clients/web/src/app/runtime.tsx:8197-8203`). Test:
  `src/features/controllers.test.ts` ("keeps a pending command save per project
  and flushes every one on page hide (HS2-25HAK3)").
- Types and entry points: `createCommandsController`,
  `createCommandSchedulesController`.
- Concurrency: none beyond autosave serialization.
- Errors and recovery: a failed group read reports loading, failure, retry
  and ready. Test: `src/features/controllers.test.ts` ("HS2-BZKTDC reports
  loading, then a failed first read, retry, and ready").
- Configuration and environment: none.
- Invariants: run transitions arrive over the change stream, not timers.

## Notifications, toasts and announcer (`webNotifications`)

- Modules: `clients/web/src/announcer-speech.ts`, `clients/web/src/features/announcer-controller.tsx`
- Small modules in `clients/web/src/`: `announcer-editorial.ts`, `announcer-layout.ts`, `announcer-reel.ts`, `announcer-settings.ts`, `away-digest.ts`, `client-notifications.ts`, `native-notifications.ts`, `notification-pause.ts`, `notification-settings-model.ts`, `toast-clearance.ts`, `toast-lifetime.ts`, `toast-modal-host.ts`, `toast-presentation.ts`
- Small modules in `clients/web/src/components/`: `announcer-overlay.tsx`, `away-digest-dialog.tsx`, `notification-card.tsx`, `notification-center.tsx`, `notification-channel-dialog.tsx`, `notification-inspector.tsx`, `notification-navigation.tsx`, `notification-rule-dialog.tsx`, `notification-settings-interactions.ts`, `notification-settings.tsx`, `notifications-paused-banner.tsx`
- Small modules in `clients/web/src/features/`: `notification-settings.ts`
- Responsibility: the server-backed attention inbox
  (`clients/web/src/client-notifications.ts:9`), refreshed only on events and
  resync; system notifications through the Tauri plugin or a granted browser
  `Notification` when the window is hidden or unfocused
  (`clients/web/src/native-notifications.ts:4-26`); toast presentation and
  lifetime (`clients/web/src/toast-presentation.ts:10`,
  `clients/web/src/toast-lifetime.ts:31`); announcer
  (`clients/web/src/features/announcer-controller.tsx:52`). Test:
  `src/client-notifications.test.ts`, `tests/announcer.spec.ts`,
  `tests/notification-settings.spec.ts`.
- State and persistence: `byProject` inbox signal
  (`clients/web/src/client-notifications.ts:13`); keys
  `hotsheet.notifications-paused` (`clients/web/src/notification-pause.ts:8`),
  `hotsheet.announcer.settings` (`clients/web/src/announcer-settings.ts:21`),
  `hotsheet.layout.announcer-*` (`clients/web/src/announcer-layout.ts:10-45`).
- Types and entry points: `createClientNotifications`,
  `sendSystemNotification` (`clients/web/src/native-notifications.ts:11`).
- Concurrency: a refresh that follows another waits for it, and a newer
  generation discards the older result
  (`clients/web/src/client-notifications.ts:21-29`).
- Errors and recovery: permission for the native plugin is requested once
  per page (`clients/web/src/native-notifications.ts:8-19`). Test: untested
  for the native path.
- Configuration and environment: `window.__TAURI_INTERNALS__` selects the
  native plugin (`clients/web/src/native-notifications.ts:14`).
- Invariants: each notification is presented at most once per page
  (`clients/web/src/client-notifications.ts:33-36`).

## Remote access and devices (`webRemote`)

- Modules: none at 500 lines or more
- Small modules in `clients/web/src/`: `device-name.ts`, `github-account-endpoint.ts`, `github-attachment-settings.ts`, `github-sign-in-origin.ts`, `remote-devices-model.ts`, `remote-host-health.ts`, `remote-live-states.ts`, `remote-session-lifetime.ts`
- Small modules in `clients/web/src/components/`: `desktop-remote-dialog.tsx`, `remote-approval-reminder-banner.tsx`, `remote-auth-screen.tsx`, `remote-devices-settings-interactions.ts`, `remote-devices-settings.tsx`, `remote-project-placeholder.tsx`
- Small modules in `clients/web/src/features/`: `browser-grants.ts`, `pairing-requests.ts`, `remote-approval-reminders.ts`, `remote-devices.ts`
- Small modules in `clients/web/src/remote-projects/`: `client.ts`, `host-names.ts`, `reconnect.ts`
- Responsibility: the remote sign-in screen
  (`clients/web/src/components/remote-auth-screen.tsx:130`), browser grants
  (`clients/web/src/features/browser-grants.ts:28`), pairing requests
  (`clients/web/src/features/pairing-requests.ts:44`), the device list
  (`clients/web/src/features/remote-devices.ts:46`) and remote reconnect
  (`clients/web/src/app/runtime.tsx:1647`). Test:
  `src/remote-auth-server-integration.test.ts`, `src/features/remote-devices.test.ts`, `tests/pairing-requests-notice.spec.ts`.
- State and persistence: `list`, `state`, `message`, `busy` signals
  (`clients/web/src/features/remote-devices.ts:59-62`); host names in
  `hotsheet.remote-host-names.v1` (`clients/web/src/remote-projects/host-names.ts:10`);
  reminder keys `hotsheet.remote-reminder.*`
  (`clients/web/src/remote-session-lifetime.ts:67`).
- Types and entry points: `createRemoteDevices`, `ensureRemoteBrowserSession`.
- Concurrency: one genuine long poll on
  `/__hotsheet/remote-auth/owner/changes?after=<revision>` with backoff 1 s to
  30 s (`clients/web/src/features/remote-devices.ts:19`,
  `clients/web/src/features/remote-devices.ts:87-107`).
- Errors and recovery: 401 or 403 stops following, drops the list and asks
  the page to re-read its grants
  (`clients/web/src/features/remote-devices.ts:96-100`). Remote stream health
  shows the reconnecting banner after a 3 s grace
  (`clients/web/src/app/runtime.tsx:1714-1735`). Test:
  `tests/project-startup.spec.ts` ("keeps an open remote tab with a reconnecting
  state while its host drops mid-session (HS2-WNK73S)").
- Configuration and environment: the auth mode the host stamps into the page
  (`clients/web/src/components/remote-auth-screen.tsx:130`).
- Invariants: the device list is never kept after a refusal.

## Host, repository and maintenance surfaces (`webHostOps`)

- Modules: `clients/web/src/components/guidance-settings.tsx`, `clients/web/src/components/repository-status-popover.tsx`
- Small modules in `clients/web/src/`: `client-update-notice.ts`, `desktop-close-contract.ts`, `desktop-close.ts`, `desktop-cursor.ts`, `desktop-window-theme.ts`, `guidance-search-model.ts`, `guidance-update-cache.ts`, `host-cpu-feed.ts`, `host-disk-choices.ts`, `host-health-resource.ts`, `host-health.ts`, `machine-settings-source.ts`, `migration-job-client.ts`, `migration-progress.ts`, `repository-file-selection.ts`, `standby.ts`, `stats-right-now.ts`, `stats-summary.ts`, `stats-this-mac.ts`, `trash-empty-summary.ts`
- Small modules in `clients/web/src/components/`: `add-host-disk-dialog.tsx`, `client-update-notice-banner.tsx`, `corrupt-ticket-row.tsx`, `cpu-core-meter.tsx`, `glance-gauge-geometry.ts`, `glance-gauge.tsx`, `glance-sparkline.tsx`, `group-registry-sync-banner.tsx`, `host-disk-settings.tsx`, `hs1-migration.tsx`, `repository-setup.tsx`, `repository-size-banner.tsx`, `repository-size.tsx`, `repository-status-interactions.ts`, `repository-summary.tsx`, `server-build-notice-banner.tsx`, `standby-screen.tsx`, `standby-settings.tsx`, `stats-dashboard.tsx`, `stats-overnight.tsx`, `stats-projects-table.tsx`, `stats-right-now.tsx`, `stats-this-mac.tsx`, `trash-settings-interactions.ts`, `trash-settings.tsx`
- Small modules in `clients/web/src/features/`: `corrupt-ticket-recovery.ts`, `repository-workflow.ts`, `repository.tsx`, `standby.tsx`, `stats-glance.ts`
- Small modules in `clients/web/src/standby-saver/`: `main.tsx`, `snapshot.ts`
- Responsibility: host health (`clients/web/src/host-health-resource.ts:22`),
  CPU and disk feeds (`clients/web/src/app/runtime.tsx:4956-4985`), stats,
  repository status (`clients/web/src/features/repository.tsx:23`,
  `clients/web/src/features/repository-workflow.ts:18`), legacy migration
  jobs (`clients/web/src/migration-job-client.ts:23`), guidance settings,
  trash, update notices, desktop close
  (`clients/web/src/desktop-close.ts:50-57`), window theme
  (`clients/web/src/desktop-window-theme.ts:9`) and standby
  (`clients/web/src/features/standby.tsx:101`). Test:
  `src/features/standby.test.ts`, `tests/standby.spec.ts`,
  `tests/repository-setup.spec.ts`, `tests/hs1-migration.spec.ts`.
- State and persistence: standby `streams` health with a 20 s grace
  (`clients/web/src/features/standby.tsx:104`,
  `clients/web/src/features/standby.tsx:290-305`); guidance update cache in
  localStorage (`clients/web/src/app/runtime.tsx:2438-2442`); dismissal keys
  `hotsheet.client-update-notice.dismissed`
  (`clients/web/src/client-update-notice.ts:29`) and
  `hotsheet.source-stale-notice.dismissed` (`clients/web/src/source-stale-notice.ts:28`).
  Test: `src/features/standby.test.ts` ("tracks stream drops by first drop
  time and clears them on recovery or close").
- Types and entry points: controllers above; `standby-saver/main.tsx` is a
  separate build (`clients/web/vite.saver.config.ts:17`).
- Concurrency: turn and CI clocks (`clients/web/src/app/runtime.tsx:4985-5003`)
  re-render labels; they make no requests.
- Errors and recovery: a failed native close acknowledgement marks the flush
  failed (`clients/web/src/desktop-close.ts:53-57`). Test: `src/desktop-close.test.ts`.
- Configuration and environment: `desktop_flush_complete` and
  `plugin:window|set_theme` Tauri commands
  (`clients/web/src/desktop-close-contract.ts:3`,
  `clients/web/src/desktop-window-theme.ts:9`).
- Invariants: neither content nor errors cross the native boundary on close
  (`clients/web/src/desktop-close.ts:50`).

## UX demo catalog (`webUxDemo`)

- Modules: `clients/web/src/ux-demo/ai-conversation-demo.tsx`, `clients/web/src/ux-demo/app-shell-demo.tsx`, `clients/web/src/ux-demo/content-components-demo.tsx`, `clients/web/src/ux-demo/orchestration-demo.tsx`, `clients/web/src/ux-demo/project-sidebar-demo.tsx`, `clients/web/src/ux-demo/setup-settings-demo.tsx`, `clients/web/src/ux-demo/terminal-components-demo.tsx`, `clients/web/src/ux-demo/ticket-collections-demo.tsx`, `clients/web/src/ux-demo/ticket-metadata-demo.tsx`, `clients/web/src/ux-demo/workspace-components-demo.tsx`
- Small modules in `clients/web/src/`: `ux-demo-entry.ts`
- Small modules in `clients/web/src/ux-demo/`: `announcer-overlay-demo.tsx`, `app-chrome-demo.tsx`, `app-dialogs-demo.tsx`, `app-loading-indicator-demo.tsx`, `away-digest-demo.tsx`, `bulk-ticket-dialog-demo.tsx`, `catalog.ts`, `choice-card-demo.tsx`, `command-run-dialog-demo.tsx`, `command-schedule-dialog-demo.tsx`, `confidence-badge-demo.tsx`, `confidence-calibration-demo.tsx`, `confirmation-dialog-demo.tsx`, `connection-details-demo.tsx`, `content-transition-demo.tsx`, `corrupt-ticket-inspector-demo.tsx`, `cpu-core-meter-demo.tsx`, `demo-actions.ts`, `demo-module.ts`, `demo-settings-form.tsx`, `desktop-remote-dialog-demo.tsx`, `development-strategy-settings-demo.tsx`, `dialog-layout-demo.tsx`, `display-preferences.ts`, `feature-settings-demo.tsx`, `fixtures.ts`, `glance-gauge-demo.tsx`, `glance-sparkline-demo.tsx`, `grid-sidebar-demo.tsx`, `group-registry-sync-banner-demo.tsx`, `halted-session-demo.tsx`, `host-disk-settings-demo.tsx`, `host-health-fixtures.ts`, `input-dialogs-demo.tsx`, `list-demo.tsx`, `list-header-demo.tsx`, `list-item-demo.tsx`, `live-section-fixtures.ts`, `live-state-fixtures.ts`, `main.tsx`, `minimized-tickets-control-demo.tsx`, `not-working-dialog-demo.tsx`, `note-delivery-demo.tsx`, `notification-card-demo.tsx`, `notification-settings-demo.tsx`, `orchestration-fixtures.ts`, `orchestration-limits-help-demo.tsx`, `orchestration-pattern-field-demo.tsx`, `orchestration-policy-settings-demo.tsx`, `orchestration-project-detail-demo.tsx`, `orchestration-settings-demo.tsx`, `orchestration-settings-fixtures.ts`, `permission-components-demo.tsx`, `project-dialog-demo.tsx`, `project-general-settings-demo.tsx`, `project-live-section-demo.tsx`, `provider-icon-demo.tsx`, `registered-projects-settings-demo.tsx`, `registry.ts`, `remote-auth-screen-demo.tsx`, `remote-devices-settings-demo.tsx`, `repository-size-demo.tsx`, `repository-status-demo.tsx`, `search-scope-toolbar-demo.tsx`, `select-demo.tsx`, `settings-controls.ts`, `settings-workspace-demo.tsx`, `standby-demo.tsx`, `stats-dashboard-demo.tsx`, `stats-overnight-demo.tsx`, `stats-projects-table-demo.tsx`, `stats-right-now-demo.tsx`, `stats-this-mac-demo.tsx`, `status-badge-demo.tsx`, `tag-chip-demo.tsx`, `terminal-demo.ts`, `terminal-visibility-demo.tsx`, `theme-picker-demo.tsx`, `ticket-group-demo.tsx`, `ticket-group-dialog-demo.tsx`, `ticket-group-settings-demo.tsx`, `ticket-row-demo.tsx`, `ticket-search-field-demo.tsx`, `ticket-store-sync-banner-demo.tsx`, `toolbar-control-group-demo.tsx`, `toolbar-demo.tsx`, `toolbar-text-demo.tsx`, `worker-columns-catalog-demo.tsx`, `worker-columns-demo.tsx`
- Small modules in `clients/web/src/ux-demo/components/`: `demo-app.tsx`, `demo-event.tsx`, `demo-state.ts`, `device-setting.tsx`
- Responsibility: `/ux-demo` replaces `#app` with the catalog root and loads
  `ux-demo/main.tsx` as its own chunk
  (`clients/web/src/ux-demo-entry.ts:11-36`), only when the host enabled the
  `ux-demo` feature. The catalog mounts production components in a Kerf
  Catalog (`clients/web/src/ux-demo/main.tsx:28-37`) from a registry of demo
  modules (`clients/web/src/ux-demo/registry.ts:91`). Test:
  `tests/ux-demo.spec.ts`, `src/ux-demo/registry.test.ts`,
  `src/ux-demo-entry.dom.test.ts`.
- State and persistence: display preferences in `hotsheet.ux-demo.display`
  (`clients/web/src/ux-demo/display-preferences.ts:4`).
- Types and entry points: `bootUxDemo`, `demoModules`, `wireDemoModules`
  (`clients/web/src/ux-demo/registry.ts:192`).
- Concurrency: none.
- Errors and recovery: none recorded.
- Configuration and environment: host feature `ux-demo`
  (`clients/web/src/host-features.ts:94`).
- Invariants: the app never downloads the catalog chunk (comment at
  `clients/web/src/main.tsx:33-36`). Test: `tests/ux-demo.spec.ts`.
  `clients/web/kerf.components.json` lists 173 app catalog entries; the
  Kerf profile `clients/web/.kerf-ui-profile.json` holds 48 doctor exceptions.
