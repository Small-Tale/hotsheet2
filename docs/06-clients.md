# 06. Clients

> **Status: Confirmed** (maintainer, revised 2026-08-26). Sequencing:
> **1) browser web UX → 2) Tauri host → 3) native SwiftUI macOS → 4) native SwiftUI iOS → 5) Android
> (Kotlin/Compose).** **Every client is a pure API consumer — no client embeds the
> core.** The server is always a separate process (local included), which the
> client auto-starts if needed and which outlives it.

## 6.1 Principle: clients are views over the service

Every client is a consumer of the same service API (HTTP + WebSocket + MCP). No
client holds authoritative state and **no client embeds `hotsheet-core`** — they
all talk to a running `hotsheet-server`. Two clients on one project show the same
thing because the server is the single authority.

The web client keeps a separate undo/redo history per checkout. Inspector field
edits, Up Next toggles, and sidebar drops are individual transactions. Undo changes
only fields that still equal the local operation's after-state, so an interleaved
remote update wins while untouched fields can still be restored; redo uses the same
field-aware rule, and a new edit after undo clears redo.

Following HS1, Cmd/Ctrl+C and X retain structured selected-ticket data while writing
readable text to the system clipboard. Cmd/Ctrl+V copies the ticket content and uses
` (Copy)`, ` (Copy 2)`, and so on for case-insensitive title collisions. Every pasted or
cross-project-dropped copy starts in Not Started regardless of its source status while
retaining its source Up Next choice; cut originals
are deleted only after destination creation, note creation, and provider-qualified
attachment copying all succeed. A paste is one history transaction: undo archives every
created copy and restores cut originals, while redo reapplies the complete transfer.
Failed transfers archive partial destination tickets and leave cut originals intact.

The local Open project dialog derives its folder value from the selected checkout. Before
any project is selected it uses the portable current-directory value (`.`), never a
developer-specific clone path. Local and remote project variants retain 16 px
dialog spacing and 8 px field/action clusters. Remote choices use shared multiline ListItem
rows with their native padding and interaction treatment, 4 px connected text/list spacing,
and character-wrapped full checkout paths within a width-constrained scrollable list (HS2-XX5Y2X).

Ticket copy/cut/paste shortcuts run only while the ticket work area owns focus and a
ticket list or board is present. The work area shows one continuous focus outline around
its composer and ticket surface. Pointer interaction outside it releases that ownership,
including non-focusable inspector text; an ordinary non-collapsed text selection, native
or Web Awesome editable control, or open dialog always retains native Cmd/Ctrl+C/X/V.
Dragging an unselected ticket moves only it, while
dragging a selected ticket moves the selection; Queue, Backlog, Archive, and (when shown)
Trash sidebar destinations apply the corresponding status — Trash soft-deletes — and
visibly highlight during dragover. A newly created ticket is inserted into both the active
collection and its project cache before it is selected, so that first selected presentation
can be dragged immediately without a deselect/reselect workaround (HS2-6E9RRS).
Right-clicking either a list or board TicketRow preserves an existing multi-selection
(or selects the clicked ticket when necessary) and opens the shared icon-bearing ticket
menu. Pointer-opened ticket menus retain the raw viewport pointer anchor and delegate
all measured popup flipping and shifting to Web Awesome; the app does not pre-clamp
against an estimated menu size that can vary with ticket state. Production handlers cover reader opening, category/status/priority changes, batch
Up Next, add/remove tag, duplication, archive, and confirmed soft deletion. A provider
advertising atomic batch support receives one checkout-scoped request with every
concurrency token validated before any write. Other update-capable providers degrade to
visible, best-effort per-ticket progress; successful writes remain applied and each failed
ticket is restored and reported. Every metadata/tag/delete write is provider-capability
gated and carries the freshly read opaque
concurrency token; a stale ticket fails instead of overwriting a collaborator's edit.
In the inspector and reader, Add tag is a distinct button that opens its own anchored,
viewport-contained popover with a labeled autocomplete field. Enter or comma can add
repeated tags, Escape restores focus to the trigger, and read-only providers omit the
action entirely instead of presenting a disabled input/button hybrid. The editor uses a
16px outer/title/input rhythm, 8px within the chip/editor group, and 4px between connected
label/input content (HS2-4Y6SM9).
The complete selection remains one field-aware Undo transaction. A capture-phase,
composed-path-aware outside pointer-down dismisses the menu reliably across native and
Web Awesome shadow-DOM controls (including an ordinary click on another ticket row),
while interactions inside the menu remain open; Escape also dismisses it. A
single completed selection also exposes Verified and Not Working. The latter accepts
notes and/or attachments and submits them through one provider-neutral operation that
atomically records one attributed timeline activity before the user's regular note,
publishes all evidence, and returns the ticket to Not Started + Up Next. The activity
subsumes the implied status transition rather than adding a second status event and
carries a concise single-line summary. The actor comes from the store's git `user.name`; when no name is configured,
the timeline uses an unattributed `Reported as not working` label. The explicit
`not_working_report` capability hides the action for
providers that cannot guarantee all-or-nothing behavior; the client never emulates it
with uploads, patches, or compensating deletes. A single Verified or Archive ticket
instead exposes **Reopen Ticket** when its provider supports updates. Reopening is one
undoable update that returns the ticket to Not Started and places it in Up Next; its
terminal lifecycle timestamps are cleared by the service. Completed/verified selections never
offer Up Next. Changing between Queue, Backlog, Archive, Trash, or ticket-error views clears
the complete ticket selection and its editing state; changing only the list/column or
other presentation mode preserves that selection.
A Trash view appears directly below Archive while the project has soft-deleted
(`deleted`) tickets, and stays while it is the selected view; Archive holds only archived
tickets and moved tombstones, never Queue-owned Verified tickets. The board projection
enforces that boundary even if a provider returns a mixed-status page. When every selected ticket is in Trash, the ticket menu
offers Restore from Trash, which returns each ticket to the status recorded before it was
deleted (Not Started when that is unknown). The Trash header also offers Empty Trash when
the checkout has a git-backed ticket source. Its confirmation states the number of tickets
that will be permanently removed from the active store and that git history retains the
files. Confirming closes the dialog immediately, projects an empty Trash, and returns to a
loading Queue while the request runs; success reconciles that Queue and removes the now-empty
Trash destination, while failure restores Trash and presents a persistent server error. The
header action keeps its icon and label on one line at every supported size.
The server
purges Trash tickets after the project's shared retention period (30 days by default);
git history still holds every purged file. `hotsheet-cli restore` and
`hotsheet-cli purge-trash` provide the same recovery and cleanup headlessly (HS2-MWDR19).

This is the clean client/service split the rewrite is chartered to create, made
**absolute**: the server is a standalone process even for local use, so the client
is only ever a view. Sharp contrast with HS1, where the server rendered HTML via a
custom JSX runtime, the client re-derived logic in a hand-rolled `kerfjs` SPA, and
the Tauri app _owned_ a Node sidecar that died with it.

## 6.2 One access model: talk to a server (local or remote)

There is **one** way a client gets data — over the API to a server. "Local" vs.
"remote" is only _which_ server:

|             | **Local project**                                                       | **Remote project**                 |
| ----------- | ----------------------------------------------------------------------- | ---------------------------------- |
| Server      | A **localhost** `hotsheet-server` (the client auto-starts it if absent) | A server on another device/machine |
| Transport   | HTTP/WS on loopback (+ secret; mTLS optional)                           | HTTP/WS over **mTLS**              |
| Who runs it | This machine's one shared server instance                               | That device's server               |

A client can show local and remote projects side by side (tabs); each tab carries
its server's `(origin, secret)` — carried from HS1's multi-server remote-client
design (§112). A local tab's origin is simply `https?://127.0.0.1:<port>`. There is
no `dataDir`/embedded-core tab kind any more.

**Auto-start + independence (the key behavior).** On launch a desktop client
resolves the local server via `${HOTSHEET_HOME:-~/.hotsheet2}/instances/`; if none is running it
**spawns one detached** and connects. The server then **keeps running after the
client quits** (in-flight AI work and terminals survive). Full lifecycle:
[04-core-server-cli.md](04-core-server-cli.md) §4.3.1.

**Opening before ticket setup.** A valid project folder opens even when it has no sibling
`.hs2` store and no configured external source. The local bridge uses a machine-local
bootstrap store only to reach the standalone server; that store is never linked to the
project. Once the empty checkout is visible, the client asks whether to create and link a
standalone `<project>.hs2` git repository or configure another provider in Sources. The
prompt can be dismissed and returns on a later open until a source is configured.
Global operation failures use a high-contrast alert toast with a keyboard-accessible
Dismiss error action. Dismissing clears only the presented client error; it does not
retry, undo, or otherwise mutate the failed operation, and the rest of the project stays
available for a deliberate retry.
When that folder contains an HS1 PGLite marker, the ordinary empty-source prompt is
replaced by a one-time import prompt that identifies the exact detected `.hotsheet`
source folder, database path, and PostgreSQL version, then asks only for the destination
ticket repository. Choosing Not now is persisted for that checkout and detected source,
so the modal does not return on every launch; a non-blocking project banner retains the
source path and an Import action. A changed source identity may prompt again.
After a successful import and remote backup, a non-blocking project banner offers explicit
cleanup of the old live HS1 data or source-scoped persistent dismissal. Cleanup is
allowlist-only, preserves backups, snapshots, the HS2 store link, and unknown files, and
refuses to delete anything while a registered HS1 channel process for that checkout can
still recreate it. Current registry entries carry the project-derived channel identity;
live entries that explicitly identify another project do not block cleanup, while matching
and identity-less legacy entries remain conservatively blocking.

## 6.3 Web client and Tauri desktop host

- **Web first.** Build and iterate on the Kerf web client in a normal browser before
  adding its Tauri host. A `/ux-demo` route renders the real production components
  against deterministic mock service adapters for isolated and composed UX review. Its
  shell uses `@kerfjs/ui`'s `Catalog` and `wireCatalog`, with URL-addressable selection,
  native responsive navigation/relationships/theme controls, focus-preserving desktop
  selection reveal, Kerf-native component bounds/margin inspection, app-owned review
  tools, and a machine-readable Hot Sheet component metadata extension alongside Kerf's.
  Keep the platform-neutral component responsibilities close to the planned macOS
  SwiftUI architecture; share concepts and API contracts, not rendering primitives.

- **Responsive mobile layout.** The web client has a desktop size floor (the app-shell
  `min-width`, 1024px). At and above it the two side panels — the left project sidebar and
  the right ticket inspector — sit beside the main column and resize/persist as usual. Below
  it the client switches to a single-column mobile layout: the app-shell drops its min-width
  and both panels become absolute overlays that slide in over the main column instead of
  taking horizontal space. Only one overlay is open at a time (opening one closes the other),
  both start closed, and a viewport-fixed click-away scrim dismisses whichever is open —
  standard mobile-drawer behavior. Crossing the breakpoint back to desktop restores the
  side-by-side layout and clears the ephemeral mobile-open state. The breakpoint is applied
  from JavaScript (a `data-mobile` attribute) so the layout switch and the overlay behavior
  stay in sync (HS2-ZK51WP). AppShell expresses that switch through Kerf
  `ResizableRegion.presentation` rather than reimplementing overlay geometry; the same
  component policy owns its hidden resize handle, width cap, shadow, and collapsed hit testing.
  The shell and viewport roots clip offscreen panels without becoming scroll containers: opening,
  clearing, and typing in search must not pan the whole workspace, including across desktop/mobile
  resize. Embedded AI conversations also opt into flex shrink containment so their header,
  transcript, and composer remain inside the mobile center column (HS2-KTW27J). The workspace and
  inspector keep independent scrolling, and
  overlay controls remain accessible (HS2-JBTPNR). The production root uses the dynamic viewport height rather than
  iOS Safari's larger layout viewport. Mobile project and inspector overlays paint through the
  full dynamic viewport—including over an open terminal drawer—instead of inheriting Kerf's generic
  85vh popover cap. Their white surfaces extend into device safe areas while padding interactive
  content away from the top, side, and bottom insets (HS2-3BVWME). The terminal-drawer restore and workspace-grid zoom controls
  use Kerf `FloatingToolbar` with dark `ToolbarControlGroup` controls, adding the device safe-area
  insets so they remain above the browser's bottom chrome and home indicator as those appear or
  retract (HS2-43N9ZB, HS2-W3GPHW). On mobile the
  ticket workspace is also list-only: the column/board
  view does not fit a single narrow column, so the Columns view toggle (and its overflow entry)
  is hidden and a persisted board preference renders as a list without being overwritten, so it
  is restored when the viewport grows back to desktop (HS2-1XCHZT). Because there is no persistent
  side inspector on mobile, a plain tap on a ticket in the list auto-opens the right inspector
  overlay (range/toggle multi-select taps and the terminal ticket rail are excluded); tap-away on
  the scrim returns to the list, and the selection persists so tapping reopens it (HS2-N7RPFP). The
  left/right sidebar keyboard shortcuts use the same mutually exclusive mobile overlays without
  changing the persisted desktop sidebar preferences (HS2-KN79XP). Crossing from desktop into mobile closes the desktop panels;
  reopening the inspector positions it fully within the viewport, including at 940px.
  Resize checks measure the settled open overlay, since a closing panel remains in the
  DOM during its exit animation (HS2-5JKNGS). The horizontal project tab strip
  and the page-header view title also do not fit a narrow column, so on
  mobile the project tabs are replaced with a project Select (the dashboard mode switcher and
  Add-project action remain) and the view title is replaced with a view Select that switches ticket
  views — both reusing the workspace-grid rail's Select controls and wiring (HS2-4C5RM7). To compact
  the mobile toolbar (HS2-0SARDD): the redundant project name is dropped from the main toolbar (the
  project Select already carries it), both the project and view Selects are borderless and sized to
  their selected label rather than stretching, and while search is open the view-mode segmented
  control is hidden so the search field gets the full toolbar row. The project combobox has the
  stable accessible name `Project`, independent of selected project text and migration status;
  Kerf owns forwarding that name to the actual Web Awesome shadow control without adding a
  visible label or changing the compact geometry (HS2-Q6EM0B). The name survives project
  changes and switching between desktop tabs and the mobile Select.
  On non-mobile project tabs, the current ticket view, notification view, or settings
  category replaces the redundant project name in the compact main-toolbar title. The
  separate large page-header row is omitted, and the ticket-view action moves to the far
  trailing edge of ProjectTabBar. Add project stays immediately after the last project tab;
  only when the tabs overflow and the strip shrinks and scrolls does it end up beside the
  ticket-view action (HS2-NE8JBS). Workspace Grid and Cross-project
  Stats retain their existing main-toolbar titles. Mobile retains its two compact project
  and view rows, including the New ticket action in the view row (HS2-9R1F91).

- **Installable web identity.** Every web route publishes a web app manifest, the
  exported Hot Sheet flame favicon, square installed-app icons at 192 and 512 pixels,
  a maskable 512-pixel icon, and a 180-pixel Apple touch icon. Browser chrome and the
  installed launch surface use the same lowered-surface `#f2f2f7` color as the client
  shell. The manifest launches at the application root in standalone display mode.
  These static identity assets are bundled into production; Hot Sheet does not use a
  service worker to cache live project/API responses or introduce a second client
  version lifecycle.

- **Stable local development by default.** `npm run dev` in `clients/web` copies the
  package into a snapshot and starts Vite there. Snapshots live in the per-user cache
  directory (`~/Library/Caches/hotsheet-web-stable` on macOS, `$XDG_CACHE_HOME` or
  `~/.cache` on Linux, `%LOCALAPPDATA%` on Windows; `HOTSHEET_WEB_STABLE_TEMP_ROOT`
  overrides it), never the OS temporary directory. macOS's daily `$TMPDIR` sweep deletes
  files not accessed for three days, and cloned snapshot files keep their source's old access
  time, so modules Vite had not read yet (such as the lazily imported Dev Review entry)
  disappeared from a running snapshot and were served as `index.html` (HS2-ZJ6VN3). Each
  snapshot records its owning process, and startup prunes snapshots whose owner has exited. The launcher
  also exits, stopping Vite, when the process that started it dies without signalling it
  (it notices the reparenting with a local check, no network request), and force-kills a
  Vite child that ignores the shutdown signal after five seconds (HS2-4SSWV5), so an
  interrupted test runner or terminal never leaves orphaned dev servers behind. The running app retains the
  development bridge and `/ux-demo`, but concurrent edits in the checkout cannot trigger
  HMR or expose a partially edited multi-file state; restart the command to load a new
  snapshot. Generated package-local `target` output is excluded from the snapshot just
  like `dist`, test results, and dependency output. Each stable process also owns a private Vite dependency cache inside its
  snapshot and disables runtime dependency discovery. A later route may therefore load a
  previously unseen ESM dependency without Vite optimizing it and forcing a document
  reload. Playwright and Vitest use separate disposable Vite caches, so a test run cannot
  mutate the cache of a maintainer's running stable client. Because a dev server can still
  full-reload for other reasons (a watched file with no HMR boundary changing, a
  self-invalidating module, or a server restart) and a reload wipes the console, a dev-only
  diagnostic records every Vite reload trigger — the event kind and the module path Vite
  blamed — to `sessionStorage` and re-surfaces the most recent one after the page returns, so
  an otherwise unreproducible "the client just refreshed" report captures its own cause. It also
  captures `vite:ws:disconnect`, because a dev-server restart or crash reloads the client on
  websocket reconnect via a direct `location.reload()` that dispatches **no** HMR event (and can
  start a fresh document that clears `sessionStorage`); the last trigger is therefore also mirrored
  to `localStorage`, and a navigation-type `reload` with no session-log trace is surfaced as that
  connection-loss cause — which points at a dev-server restart/crash (for example under memory
  pressure) rather than an HMR update (HS2-8JV12R). The launcher
  passes the original repository root into the snapshot so the
  project bridge still resolves the real `target/debug/hotsheet-server` rather than a
  nonexistent temporary `target` directory. The stable launcher serves no Vite websocket/reconnect
  client (including on the root index route); Vite's transformed CSS modules use a local style-only
  runtime so startup CSS still renders without an HMR connection. A transient dev-server connection
  loss therefore cannot reload the application; reload manually after restarting the snapshot. Use `npm run dev:hot` only
  when actively developing the web UI and immediate
  HMR is desired. Browser tests use `dev:hot` on a separate default port and never reuse
  an already-running maintainer server. Signal handling is active before snapshot creation:
  an interrupt during startup prevents Vite from launching, awaits the in-flight snapshot,
  and removes it before returning the conventional signal exit status. After Vite starts,
  shutdown first waits for the child to exit and then removes the private snapshot before
  the launcher itself exits; every path removes its signal listeners and prevents concurrent
  Vite writes from racing snapshot cleanup. Stable-dev browser checks allow the snapshot
  copy a bounded 30-second readiness window under full-suite I/O load, fail immediately if
  the launcher exits, and include its captured output in timeout and early-exit diagnostics
  (HS2-2D5CXN).

- **Startup delivery budget.** Vite development intentionally serves the source module
  graph as separate requests: a cold local profile on 2026-09-03 loaded 175 scripts
  (176 requests including the document), with first contentful paint at 148–348 ms.
  This request count is development tooling, not the desktop delivery shape. The same
  code from `vite build` loaded as one JavaScript bundle and one stylesheet: three
  requests including the document, with local first contentful paint at 20–72 ms.
  `npm run build` rejects a production entry point requiring more than four initial
  assets. The future Tauri host must embed this production output and must never ship
  or connect to Vite; Tauri startup itself remains to be measured once that host exists.

- **Render budgets.** Development builds expose root render-pass and DOM-mutation
  counters to browser tests. Polling responses that do not change observable state
  must cause zero render passes and zero DOM mutations; tests also budget intentional
  transitions and no-op interactions from an explicitly loaded, quiescent baseline so
  late project initialization is not attributed to the event under test. A permission
  request announced by long poll produces exactly one render with visible DOM mutations.
  In particular, activating an already-selected,
  fully loaded ticket again performs no detail request, render pass, or DOM mutation;
  capture-phase pointer handling prevents the click from starting the editor's blur
  lifecycle, preserving focus plus draft state. This makes broad Kerf render
  dependencies fail loudly instead of becoming
  focus, scroll, or animation regressions. Development builds enable Kerf's
  value-only-render and list-rebind warnings plus throwing list invariants. They also
  emit `hotsheet:interaction-timing` after the next painted frame for project/view/mode
  navigation, ticket selection, Up Next and status changes, bulk changes, and permission
  decisions. Each event reports state-update and painted-UI latency against a 100 ms
  budget and is copied into the bounded, value-free UI-stability event log. Ticket selection
  measures the immediate row response; its asynchronous detail fetch and inspector presentation
  are a follow-up rather than blocking the interaction sample. Optimistic list and inspector
  ticket mutations are committed in one reactive batch. The
  138-ticket browser profile enforces that budget for the primary paths. Render events include cumulative
  counters plus per-pass deltas so a captured storm distinguishes reactive rerenders
  from unrelated DOM activity. Three unexpected quick select dismissals within ten
  seconds or twelve root renders within two seconds after startup create a rate-limited
  diagnostic ticket automatically. A root render-storm signature creates at most one
  ticket per page lifecycle; quiet intervals clear stale pass history without rearming
  an already-reported signature. Dev Review is opt-in in
  development (HS2-TCACFR): `?dev-review` enables both the overlay and automatic
  stability-ticket reporting, and both stay off without it, and its ticket
  dialog offers a checked diagnostic-log attachment so a manually reported transient
  failure carries the same context. Its adjacent utilities disclosure also offers CSS Live
  Edit: two complete in-memory CSSOM snapshots bracket DevTools styling changes and are
  attached to a directly created implementation ticket (HS2-X36S5N). Automatic
  render-storm reporting remains suppressed while
  remembered projects are restoring, a foreground operation owns the app's loading
  state, the ticket collection is appending a scheduled progressive chunk, or a
  multi-step UI transition is still within five seconds of its initiating pointer or
  keyboard action; those
  intentional renders stay in the bounded diagnostic log with their
  suppression reason and cannot seed a later background storm. A render loop that
  persists after the interaction grace is detected from a fresh window, so suppression
  does not permanently mask it. This uses lifecycle state rather than a fixed startup
  duration, so slow project opens do not become false-positive tickets. Project refreshes remain deferred while any
  Web Awesome select is open, including the new-ticket composer, so a background
  ticket column move cannot replace the control or dismiss its popup.

- **Server-busy indicator.** A decorative full-width strip pinned to the very top of the app
  ripples a row of yellow (the Up Next star color) bars while the server is busy, so activity
  is apparent without hunting for a spinner (HS2-MW1V3M). It is a `position:fixed` overlay that
  allocates no layout space, is inert to the pointer and assistive technology, and is driven by
  the count of in-flight authenticated server requests — **idle long-poll event streams are
  excluded** so a quiet app reads as idle, as is silent background revalidation of an
  already-painted warm project (see _Warm project tabs_, HS2-AZZ9TF), and a short linger after the last request settles
  keeps rapid bursts from flickering. Each bar is 3px wide with a 2px gap and scales from 1px to
  4px on a staggered cycle; the bar count fills the viewport width and is recomputed only on an
  actual (debounced) window resize. Animation honors `prefers-reduced-motion`.

- **Ticket collection motion.** A status change lifts the moving ticket into a
  fixed, workspace-level overlay so it can cross column scroll and clipping
  boundaries while the source and destination siblings close and open their space
  together. A newly created ticket first lets its siblings slide apart by exactly
  the new row height, then fades into the reserved space, including when it is the
  first visible ticket. Ticket-owned scrollports disable browser scroll anchoring so
  native anchor correction cannot cancel this measured movement; Hot Sheet's explicit
  scroll restoration remains authoritative. A departing ticket fades
  out before only its source siblings close the gap. Overlay identity and temporary
  hiding survive reactive DOM morphs without duplicate cards. Visual-only ghosts
  stay on a fixed layer clipped to the live ticket workspace and, while the bottom drawer
  is open, to the drawer's top boundary; filtering or moving tickets therefore cannot paint
  transition clones over terminal tabs or content (HS2-29T4D8). The layer remains below every
  modal/dialog backdrop, so opening a dialog during a move cannot paint the moving card over
  the dialog. Ghost layers are removed with their animations. They expose
  motion-specific identifiers rather than ticket-row roles, actions, or slugs,
  and isolate their visual text from document text queries, so ordinary ticket
  selectors, text search, and assistive technology continue to see one real row.
  Pure inspector, sidebar, or viewport layout shifts never trigger collection motion.
  Queue, Backlog, Archive, and ticket-error views have distinct motion scopes, so
  replacing a whole collection never creates per-row transition ghosts; within-view
  ticket arrivals, departures, and moves retain their normal motion. Collections above
  100 rendered tickets also bypass geometry capture and transition ghosts, keeping
  search responsive when it replaces a fully rendered large queue (HS2-E76C4K).
  Reduced-motion users get the final layout immediately.

- **Field-aware live editing.** A ticket refresh merges fields that the user is not
  editing immediately. An active text draft adopts a remote-only update when still
  untouched, preserves a local-only edit, and stays quiet when both sides converge.
  Only divergent changes to that same active field open a reconciliation surface with
  the remote and local versions plus an editable merged value. Whole-ticket concurrency
  token failures use the same comparison: unrelated field drift retries once against
  the fresh token instead of presenting a false conflict. Single-ticket edits are
  serialized per ticket: each edit bases off the previous edit's committed concurrency
  token, so a user's own rapid sequential edits (e.g. setting a priority then the
  description right after creating a ticket) are last-write-wins and never self-conflict,
  while a genuine concurrent external write to the same field still surfaces the
  reconciliation surface (HS2-K9SG2R). Background refresh also leaves
  an in-flight or queued autosave draft alone; the write response and token retry path
  distinguish this client's earlier partial save from a genuinely competing edit.
  The freeform blocked reason uses the same silent blur-flush path and is the single source
  of truth for blocked presentation: a non-empty reason persists and shows the badge/rail,
  an empty edit sends `null` to clear it, and the authoritative response exits
  editing without making the text disappear. An existing blocked-reason surface enters
  that editor on double-click, matching details and ordinary notes. Existing blocked
  reasons and ordinary notes do not add redundant per-item Edit buttons; their content
  surfaces support double-click plus Enter/Space keyboard entry instead.

- **Ticket claims.** Ticket rows show a yellow spinner directly after status only
  while a worker holds a non-expired claim lease. Started tickets without a lease remain
  unclaimed, and old `claim_count` values never imply presence. A local one-shot expiry timer removes stale indicators without issuing
  polling requests; claim/release changes otherwise arrive through the shared live-update
  channel.

- **Custom project commands.** The sidebar renders machine-local typed command
  definitions as collapsible groups with running feedback, stop confirmation, latest
  outcome, and press-and-hold output history. Definitions are edited in Project
  Settings through a responsive master-detail editor with add, reorder, delete, typed
  target fields, validation, and debounced autosave; raw command JSON is not exposed as the
  primary editing interface. Definitions persist to `<project-root>/.hotsheet2/settings.local.json`. Native
  `program` definitions execute an exact program plus argument array; portable `shell`
  definitions store command text and resolve the current machine's shell only at run time.
  Running one creates a terminal named for the command, selects it, and opens the bottom
  drawer so output and subsequent interaction remain visible. A terminal-list refresh never
  disables or discards that launch action; only a create already in flight suppresses a
  duplicate click. If the detached broker has
  exited during its empty five-minute grace, the server relaunches it transparently before
  opening that terminal. `ai` definitions store the prompt plus an optional provider/model/
  effort override, never a hard-coded Hot Sheet executable or CLI argv. Their shared
  hierarchical selector starts at **Project Default**, follows later project-default changes
  while inherited, and offers the same provider catalog, compatible effort choices, and
  exact-id `Other…` dialog as Drive. The optional override round-trips through project-local
  settings and is included in the best-effort `$hotsheet` turn notification.
  Clicking one creates an urgent Up Next task whose title is the command label and whose
  details are the configured prompt; it does not execute that prompt directly. When an
  idle, sendable connection for the configured (or default) AI tool already exists, the
  client also sends `$hotsheet` so that connection re-reads and works the prioritized
  queue. Ticket creation remains the authoritative action and succeeds when no connection
  is available or the optional signal fails. Optional icon/color metadata controls the sidebar presentation.
  Run transitions use the shared WebSocket/
  long-poll event channel and never introduce client interval polling. Press-and-hold
  remains reserved for output/history. A context or overflow menu will provide “Run in
  new terminal” for shell commands (HS2-NT3F3Q).

- **Project settings navigation.** Entering Settings replaces the ticket-oriented
  project sidebar with a persistent category navigator, following the HS1 settings-tab
  pattern. The navigator groups the project-scoped categories under a **Project Settings**
  heading and the device-local categories under an **App Settings** heading (HS2-QT6PGR).
  Ticket sources, Commands, Lifecycle, Permissions, and Column view each render as a
  separate workspace so unrelated controls do not become one long settings page.
  The settings workspace and ticket-source setup flow are component-owned surfaces:
  `components/settings-workspace.tsx`, `ticket-sources-settings.tsx`,
  `ticket-source-setup-dialog.tsx`, and `provider-setup-form.tsx` own their markup and
  colocated styles. The application root retains reactive signals, API mutations, and
  delegated event wiring and passes typed render data into those surfaces (HS2-HTB5RR).
  The remaining extracted project-open, terminal-rename, and notification-inspector
  surfaces likewise own their stylesheets. `style.css` is reserved for document/app-shell,
  empty/loading/toast, and shared pagination rules; a source test enforces that boundary
  so component selectors cannot drift back into the global sheet (HS2-JH0112).
  The primary project and terminal layouts follow the same ownership boundary:
  `components/workspace-composition-surfaces.tsx` owns the sidebar, ticket workspace,
  terminal ticket rail, global terminal/statistics workspace, project terminal drawer,
  and terminal-operations compositions. `main.tsx` derives typed props from application
  state and retains effects and event handling, but no longer owns those surfaces' render
  branches (HS2-KB7ZA4). Their constituent production components remain the cataloged
  review units, so the component catalog does not duplicate internal composition wrappers.
  Reader layers and app-level transient surfaces use the parallel
  `components/reader-overlay-surfaces.tsx` boundary. It owns the reader stack and the
  permission, AI conversation, repository/evidence, ticket/attachment menu, gallery,
  command, connection, compatibility, app-tab, and not-working render adapters while
  `main.tsx` derives their typed models and retains lifecycle/event wiring (HS2-BMASD4).
  Those adapters are catalog-exempt because they compose production components whose
  standalone and in-context states already have catalog coverage.
  Together with the setup/settings extraction, these boundaries complete the inline
  component migration tracked by HS2-VBRC6A: `main.tsx` contains application-state
  derivation and event/effect wiring, while exported PascalCase UI components live under
  `components/` with their ownership tests and catalog accounting.

- **Keyboard shortcuts (App Settings).** The App Settings → Keyboard category is a complete,
  grouped reference of every documented client keyboard shortcut (HS2-QT6PGR). The global
  command chords (open search, undo, redo) plus the app-level ticket clipboard (copy, cut,
  paste) and select-all are rebindable: record a new chord, reset one to its default, or reset
  all, with conflict warnings when two editable shortcuts collide. A **Views & panels** group
  adds rebindable chords to toggle the left sidebar (⌘/Ctrl+B), right sidebar/inspector
  (⌘/Ctrl+⌥+⇧+B), and bottom terminal drawer (⌘/Ctrl+J); switch the workspace to the list
  (⌘/Ctrl+⇧+L), column (⌘/Ctrl+⇧+B), notifications (⌘/Ctrl+⇧+M), and settings (⌘/Ctrl+⌥+S)
  views; and toggle the all-project terminal grid (⌘/Ctrl+⇧+G) or stats dashboard
  (⌘/Ctrl+⇧+D) against the last selected project. A **New ticket** chord (a bare `c`, in the
  spirit of GitHub/Linear "create") opens the composer, and **Navigation & tabs** adds
  previous/next cycling for the project tabs (⌘/Ctrl+⌥+⇧+←/→) and drawer tabs
  (⌘/Ctrl+⌥+⇧+↑/↓). Defaults follow VS Code where it has an equivalent except when
  Safari reserves the chord: HS2-Q1BH0V moved the inspector, settings, and tab-cycling defaults
  away from Safari's bookmark editor, application settings, and browser tab/tab-group commands.
  Every shortcut remains rebindable, and the bare
  `c` (like other non-modifier chords) never fires while a text field is focused (HS2-9SHYWD).
  The settings surface follows Kerf's spacing rhythm: 24px between major groups, 16px
  between peer regions, 8px within rows and controls, and 4px inside tight metadata clusters
  (HS2-4Y6SM9).
  Overrides are stored
  device-locally and resolved through the shared registry (`matchesShortcut`) at the central
  keydown dispatcher, the ticket clipboard policy, and ticket-row selection, so a rebinding takes
  effect immediately (HS2-9PR10F). Recording preserves physical Control separately from
  Command on Apple, and Meta separately from Control elsewhere, including chords that hold
  both along with Shift or Alt. Modifier-only presses keep recording; Escape or Cancel leaves
  the previous binding unchanged. Matching and conflict warnings compare the exact physical
  modifiers on the current platform, while existing primary-modifier bindings retain their
  platform mapping. Control is shown as ⌃ on Apple and Ctrl elsewhere (HS2-835BZD).
  While a modal dialog is open the central keydown dispatcher
  suppresses these background app shortcuts — search focus, ticket undo/redo, and the ticket
  clipboard — so, for example, Cmd-K cannot focus the workspace search from inside a dialog; they
  resume once no modal remains, and the modal keeps its own text-field editing and shortcuts
  (HS2-FW4PYZ). Fixed ARIA structural affordances — list-arrow navigation, tab
  navigation, media-gallery keys, control activation, and dismissal — remain non-rebindable
  **System** entries, since rebinding ARIA navigation would break screen-reader and platform
  expectations. Chords display platform-correctly (⌘ on Apple, Ctrl elsewhere). The
  selected category names the shared compact toolbar heading; the workspace does not repeat that
  heading, and the right region uses the same divider-free empty inspector placeholder
  as Notifications. Entering Settings preserves the ticket selection for returning to
  list/board, but that retained selection never changes the Settings placeholder semantics.
  The selected category, unsaved command-editor draft, selection, and validation result belong to
  the active project rather than the shared shell: a newly opened project starts on Ticket
  sources, while returning to another project restores that project's category and draft.
  Project activation refreshes the visible settings data and ignores late provider responses
  from a project that is no longer selected.
  Lifecycle exposes the shared Trash retention period as a positive whole-day field, shows
  the 30-day default, explains that git history remains available after cleanup, and saves
  through the checkout-scoped core settings API.
  Right-sidebar toolbars are divider-free in every state—ticket, loading, multi-selection,
  Settings, and Notifications—so content sections, not the shell toolbar, own separators.
  The ticket inspector uses an 8px horizontal content gutter. Details, Tags, Notes,
  Block ticket, and Add note reuse the shared sidebar `ListHeader`/`ListItem`
  primitives; their text and icons share one inset while section surfaces stay flush
  beneath their headers instead of accumulating another indentation level. Its shell uses
  16px between header/status regions, 8px within status surfaces and outer gutters, and 4px
  for connected title/tab details (HS2-4Y6SM9).

- **Persistent shell splitters.** The project sidebar and ticket inspector are
  independently resizable by pointer or keyboard. Dragging updates only splitter
  geometry until release, then persists the bounded width locally so a reload restores
  the layout without creating broad render churn.

- **Stable scrolling across mutations.** List, board, each board column, and each ticket
  row expose stable Kerf `data-key` identities. Status changes and other ticket mutations
  therefore morph the existing scroll owners instead of replacing them; list position and
  every independent column position survive moves between statuses (apart from the browser's
  normal one-pixel scroll anchoring correction). During the current app session, the main
  workspace also remembers positions separately for every project, mode, and view
  (HS2-PDYXYJ). Switching back restores the list, independent board columns, or terminal
  grid after loading and progressive rendering. A new combination starts at the top;
  changed contents clamp the saved position to the available scroll range. Inspector
  and drawer scrolling remain independent of this workspace memory.
  Scrolling during a pending loading or progressive-render pass takes precedence over
  the older destination for that owner and axis (HS2-Q9Z4KM). Untouched columns and axes
  retain their desired destinations through temporary layout clamping and empty/refill
  transitions. Leaving a scope before loading settles still remembers those user edits;
  later mutation renders or stale restoration callbacks must not reset them.

- **Real local web entry point (initial implementation, HS2-0P1MDG).** `/` renders the
  production AppShell over checkout-scoped server APIs; `/ux-demo` remains the isolated
  development catalog. The project-tab `+` action invokes the host-native code-checkout
  folder chooser immediately and sends a selected folder directly to project open. The
  initial empty-state Open project action retains the roomy 48rem dialog so an explicit
  git-store path can be supplied when convention-based discovery does not apply; both
  dialog paths have host-native chooser buttons, and Cancel/Escape are controlled by
  durable state so unrelated renders cannot reopen or strand the surface. On open, the
  server conservatively discovers a valid sibling `<checkout>.hs2` git ticket store,
  hosts it, and records the many-to-many checkout/store link.
  Retrying a failed project open clears the prior failure immediately. During startup,
  remembered roots are deduplicated and their project-open/metadata requests run concurrently.
  Failed opens receive one bounded parallel retry; successful projects are then registered
  serially in remembered order. Only the remembered active project loads its ticket list,
  commands, views, and workspace session before the shell is revealed. Other project tabs
  load those resources when selected (including selection after closing the active tab).
  If the remembered active root remains unavailable, its error tab stays selected; if no
  remembered active root matches, the last successful project is selected. A visible
  aggregate terminal workspace still restores its projects' terminal and AI-chat resources.
  Entering Workspace grid or cross-project statistics loads missing inactive ticket summaries
  in the background without activating those projects or loading their commands and views.
  Startup never persists a partial session over a saved draft. Onboarding for an inactive
  restored project waits until activation, and setup/migration dialogs take precedence over
  reopening the saved composer while retaining its draft.
  A project that remains unavailable stays in the tab bar with a red title and error icon;
  selecting it presents the exact failure, known stale-server context, its remembered root,
  likely recovery steps, and an in-place retry action. Failed tabs are not draggable or
  closable as live projects, and every tab-persistence path retains their roots for the next
  launch. Restore failures must never leave stale compatibility or connection diagnostics
  over a different project that reopened successfully.
  A checkout with existing sources opens directly. Source-less project setup uses a
  bounded dialog with shared multiline menu items whose title and explanation remain
  inside one selectable row at compact sizes. When opening from the initial dialog it
  waits for that dialog's completed close event, so the two modal surfaces never overlap;
  the direct `+` flow presents setup as soon as project open identifies zero sources.
  Once a source is attached, later close/reopen operations use the persisted checkout
  source link and do not repeat onboarding.
  Forward and backward setup navigation uses the shared stable A/B `ContentTransition`:
  the outgoing and incoming content areas move together for an iOS-style push/pop, while
  crossfade and motion-free replacement remain reusable variants in the UX catalog.
  Multi-screen dialogs use the same in-content chevron `FlowBackButton`; the conversation
  save flow applies these conventions to push from message scope to bundle contents and
  pop back without placing a competing Back action in the dialog footer.
  The Vite-only bridge discovers or detached-starts one bootstrap machine server and
  attaches every discovered or explicit project store through the server's multi-store
  open path; it never starts one server per project. Discovery is health checked and the
  bridge re-supervises after transport failures and terminal-WebSocket reconnects. The first
  `/health` check of each supervision waits up to 3 s, so a busy but healthy server is not
  mistaken for a dead one, while retries keep a 750 ms limit so a hung registered process is
  still reported promptly. An instance (same pid, URL, and start time) verified healthy in the
  last 10 s is reused without another request, so a burst of project opens or reconnects does
  not re-probe; a failed forward forgets that verification before re-supervising (HS2-TANE0V). Safe
  GET/HEAD requests retry after recovery, while ambiguous writes return an explicit 503
  instead of risking duplicate mutation. Compatible old servers may be upgraded through
  their authenticated quiescence/restart capabilities; the bridge waits for the old
  registration to disappear before starting or joining its replacement. A live but
  health-check-unresponsive local server is not replaced automatically. The project dialog
  explains that active work cannot be verified and offers explicit recovery, which matches
  the registered PID, URL, and start identity before each signal, tries graceful
  termination, then force-stops only the still-matching process before reconnecting. This
  route exists only in the local bridge. The bridge keeps
  the bearer credential out of browser state; Tauri will replace it with its native
  lifecycle layer.
  Creating a ticket selects it and immediately opens and focuses its Details editor so
  the user can continue writing without another pointer action. Creation from Backlog
  sends and persists `status=backlog`; an authoritative refresh therefore keeps the new
  ticket in Backlog instead of moving it into Queue as `not_started`.

  The bridge performs the authenticated compatibility handshake when attaching to a
  discovered server. The client distinguishes compatible skew, client-too-old,
  server-too-old, and unknown metadata. Compatible revision differences do not block use;
  explicit non-intersecting protocol ranges stop before project API use and name the side
  that must be updated. Rollout is always assumed unsynchronized; no client behavior may
  depend on a server or app-store release becoming available simultaneously. Newer
  incompatible ticket diagnostics are presented as “Hot Sheet 2 update required,” never
  as corrupt files (see [19](19-format-compatibility.md)).
  Before graphical Git-store creation, the bridge compares the current CLI's created
  store schema with the server already serving that project. It refuses before creating
  any repository when the detached server is older, explicitly says no repository was
  created, identifies both schema boundaries, and directs the user to finish active work
  before stopping/restarting that project server. The warning is project-scoped: another
  tab connected to a different, current server correctly remains unbadged.
  an old client offers reload/update, while an old server is restarted automatically only
  when both restart and quiescence capabilities are explicitly present; otherwise it is
  surfaced without an unsafe restart action.
  Every compatibility warning's **View details** action opens an accessible build-details
  dialog. It reports the running server application version, build revision, current local
  source revision, client revision, both protocol ranges, and server start time when the
  authenticated handshake supplied them. Missing values are labeled rather than guessed.
  The dialog uses the same shared divider-free icon/title/subtitle header and borderless
  inset-separator value table as repository status. Every metadata entry is a canonical
  `ValueTableRow`, so shared row semantics, wrapping, and separator geometry apply in production,
  migration, and catalog surfaces. Because this metadata is safely recoverable, the
  native popover is dismissed by clicking outside or pressing Escape and has no redundant
  Close button. Every compatibility and recovery state is represented in `/ux-demo`.
  Recovery guidance distinguishes safe compatible skew, stale local source, old client,
  old server, and unavailable metadata; it never offers automatic restart without the
  same explicit restart plus quiescence capability gate.
  Ticket-provider connections are not stored in project
  `.hotsheet2/settings.json` or `.hotsheet2/settings.local.json`: those remain
  shared/local preferences. Git sources are
  checkout/store links in the machine registry, while external provider connections are
  non-secret records in the ticket store's `providers.json` (credentials remain keychain
  references). The real Sources settings view lists active git and external connections
  without embedding setup forms. Its Add data source action opens the same dialog used by
  source-less project onboarding: the first screen selects GitHub, GitLab, or Jira (and,
  during initial setup, a standalone Hot Sheet git store), then pushes the provider-specific
  configuration screen within that dialog. Its shared multiline menu rows use intrinsic
  height, so wrapped paths and descriptions retain vertical padding and cannot cross the
  inset separators; shared value-table rows likewise reserve block padding when values wrap.
  Multiple connections of one provider type are
  allowed because connection identity is independent from provider kind. Clicking an
  existing connection row opens that same dialog with its editable non-secret values.
  After creating a standalone git ticket store, the dialog asks for a clone URL and can
  add `origin` plus perform the first push without exposing shell commands. A failed first
  push rolls back that newly added origin so the same form remains retryable. Inline help
  links to [host-specific remote setup guidance](ticket-repository-remotes.md), including
  the distinction between Git hosts and issue-only providers such as Jira.
  Remote failures keep Git's diagnostic text and add specific recovery guidance for an
  existing origin, missing/inaccessible repository, authentication, SSH host trust,
  network/DNS failure, non-empty remote, or missing initial commit. Unknown failures
  retain their operation context and original stderr rather than showing only an exit code.
  The local bridge creates or reopens that store through the same idempotent headless
  `hotsheet bootstrap` workflow available in an ordinary terminal, so checkout linking,
  detected AI-tool skills/MCP setup, and preservation of existing config cannot drift.
  Creation and editing validate identifiers and locators, persist the connection record,
  update its checkout link, and can select it as the default creation target. Credential
  fields accept only an existing keychain reference; secret values are never returned to
  browser JavaScript.

  Workspace collection refreshes retain one bounded compact page and expose a visible
  **Load more tickets** cursor continuation below lists and after the final loaded row in its column instead of
  downloading every ticket. Continuation pages append without duplicating overlapping
  provider rows, and the action disappears when the cursor is exhausted. SQL aggregate counts keep the
  sidebar and background project tabs authoritative even when most rows are not resident.
  Every initial and continuation request carries the active list/board sort field and direction;
  switching sort or switching between independently sorted list and board modes refreshes the bounded
  page. The server applies that same total order before the page boundary, so a newer or higher-priority
  ticket cannot disappear merely because the browser locally sorted an arbitrary ID-ordered subset
  (HS2-X23ME4). For checkouts with multiple local or hosted-provider sources, the opaque cursor tracks
  every source and the server k-way merges their bounded heads; initial and continuation pages therefore
  remain globally ordered, including recent-first categorical ties (HS2-2BDSRK).
  The aggregate also carries the exact Verified total, allowing Queue columns to derive
  absolute Not Started, Started, Completed, and Verified counts rather than capping their
  headings at the currently loaded page; Backlog and Archive use their absolute aggregates.
  The **column (board) view paginates per column** (HS2-8NBGBX): each status column loads more with its
  own status-filtered query and cursor, so a long column (e.g. Completed) never starves a short one and
  every column that has fewer loaded rows than its absolute total offers its own Load more control —
  rather than a single global cursor whose one continuation landed in whichever column happened to hold
  the last loaded row. The loaded rows still live in one flat union, so selection, the inspector,
  mutations, and cross-column drag are unaffected. **Columns also load independently from the start**
  (HS2-HNZZHC): the board never splits one global page across columns. Every status column reads its own
  first page of 100 rows in the active sort (only the first request carries counts; the rest pass
  `counts=false`), so a short column shows all of its tickets immediately and never a lone Load more,
  and each Load more reads that column's next 100. A refresh (live change, tab reactivation, list↔board
  switch) reloads each column back to its loaded length in one commit, in pages of at most 500, so an
  external change does not reset a column's pagination. Warm project tabs keep their column cursors with
  their rows. Crossing the mobile breakpoint on the board reloads, because mobile lists one global page.
  Single-collection Backlog/Archive/Trash boards and search keep the global cursor.
  A column pages an _ordered list_ of statuses, not just one: when the **Hide Verified column** setting
  merges Verified into Completed, that column exhausts its `completed` stream and then continues into
  `verified`, so verified rows beyond the initial global page stay reachable through its own Load more
  and the column can reach its full done total (HS2-F2N4ZN).
  Workspace search delegates to the checkout index rather than filtering Markdown bodies
  in the browser. It therefore matches slug, title, tags, Markdown details, and note text.
  Search is scoped to the selected sidebar view, so Queue, Backlog, and Archive results do
  not bleed into one another. Column mode retains only that selected view's normal columns:
  a Queue search does not append Backlog or Archive columns for results that its scoped
  request cannot contain. Once the selected view's indexed search completes, bounded
  background searches update the other active collections and shared custom-view counts.
  Trash remains outside this live-search fan-out and keeps its canonical collection count.
  Pending searchable-view counts use a compact spinner;
  settled search-derived counts use a small magnifying-glass marker and remain inside the
  selected item's blue bounds. Moving focus away from an unchanged search preserves those
  settled results and counts without issuing another request. The workspace's Kerf-managed
  collapsible search adopts the project-persisted open signal: its canonical magnifier moves
  focus into the editor, and an empty blur or Escape collapses it without app-owned focusout
  bookkeeping; populated searches and marked suggestion/date/help surfaces remain open.
  Select All followed by Backspace or Delete clears ordinary text and filter chips
  without collapsing the focused editor, including repeated empty/refill sequences
  on narrow screens. Focus may still leave normally: a deliberate keyboard handoff
  collapses the empty field, and the search launcher reopens it ready for typing.
  This behavior comes from the published Kerf managed-focus implementation
  rather than an application reopen workaround (HS2-GRAQ2K).
  Clear search also preserves focus across controlled editor replacement before the
  next input task, so immediate typing stays in search even if an animation frame
  has not run. Repeated clear/refill and desktop/mobile crossings retain this behavior;
  a deliberate focus handoff still owns focus, and old Clear work cannot move a newer
  selection. This contract is owned by Kerf (KF-E1DHC6; HS2-NNNFFR).
  When composed inside a toolbar control group, the group retains its border, padding,
  and focus ring in both collapsed and expanded states. The ordinary workspace header
  and workspace-grid ticket rail share that package-owned treatment (HS2-TNSD4K).
  Explicit lifecycle expressions and filter chips narrow the
  selected collection. Boolean expressions that cannot be represented as one provider query
  walk every compact cursor page for that collection, retain only client-side matches, and
  cancel cleanly when the query changes, so matches after the first 200 rows remain discoverable.
  Structured duplicate searches send
  `close_reason=duplicate` to the provider before bounded pagination, rather than hoping
  duplicate rows happen to occur in the first unfiltered page. Reference-mention matches say why
  they matched, every result names its provider, and the global overlay can hand its
  current query/scope/filter payload to the separately owned saved-view editor without
  replacing the compact workspace search. The overlay is available from the toolbar and
  the platform Search shortcut (`Command-K`/`Control-K`).

  Workspace view choices use Kerf `SegmentedControl` with toolbar appearance inside
  the existing toolbar group. List, Columns, Notifications, and Settings retain native
  sequential focus, Enter/Space activation, selected pressed state, and notification
  count labels (the visible badge caps at 99+). Mobile omits Columns without overwriting
  the desktop preference. The explicit rail presentation offers only List and
  Notifications as equal-width rounded segments; the ordinary toolbar uses content-width
  pill segments. Overflow commands retain their existing mode action contract (HS2-F29QAT).

  The workspace and grid-rail selection toolbar uses native group
  buttons and an empty, yellow half-filled, or yellow filled Up Next star for none,
  some, or all selected tickets queued, with matching false/mixed/true pressed state.
  None/mixed toggles add the eligible selection; all toggles remove it. Completed,
  Verified, other ineligible statuses, empty selections, and provider-disabled
  selections remain disabled. Narrow overflow retains the same star state (HS2-WP15AF).
  Empty ticket collections use the shared `TicketEmptyState` adapter in both list
  and board modes. A project with no tickets invites its first ticket, a populated
  project's empty view names that view, an in-flight search reports that it is still
  searching, and a settled empty search repeats the query and suggests changing it.
  When every board column is empty, the board renders one board-wide message beneath
  the retained column headings. Individual empty columns remain blank rather than
  repeating per-column placeholders. While the initial ticket collection is unresolved,
  the same list/board content area instead shows a centered animated **Loading tickets**
  state beneath the retained column headings; it never flashes premature empty-project
  copy or duplicates the corner activity indicator.

  Reusable web-client presentation primitives come from the published `@kerfjs/ui`
  package through explicit subpath imports. Hot Sheet owns only domain compositions and
  product behavior: connection-state mapping, ticket-specific empty-state copy,
  menu action metadata, resize action wiring, and tab identity. The shared package owns their generic anatomy and CSS,
  along with toolbars, toolbar text/control groups, page headers, loading indicators,
  and Lucide rendering; the client does not carry local copies of those primitives.
  The web client pins `kerfjs`, `@kerfjs/ui`, and `eslint-plugin-kerfjs`, and its
  Kerf/Web Awesome spike pins the runtime and lint plugin, exactly at 5.0.0-beta.49.
  The web package's workspace-scoped Kerf UI profile and doctor configuration are gated
  in CI by `npm run ui:doctor`: catalog, TypeScript, Kerf ESLint, and static analysis run
  against an exact no-regression error/review budget, while executable browser evaluation
  stays opt-in and requires an explicitly supplied trusted URL (HS2-HD1SCC).
  The client follows the package's stricter component
  contracts: every shared `Select` supplies exactly one accessible naming mode, and
  `ListHeader` callers render an explicit passive, disclosure, or trailing-action mode
  instead of passing partial optional action props. The browser test runtime is kept at
  the package's Playwright 1.63 peer floor so package and application browser checks use
  one compatible installation. Token-search input state is consumed through beta 22's
  `wireTokenSearchFields.onEdit` callback in both the application and UX catalog rather
  than through a second competing delegated input listener (HS2-HJ585K).
  Managed token search owns clear-action focus across controlled editor replacement:
  clearing chips keeps the current editor open for immediate continued typing, while
  subsequently focusing another control collapses an empty search normally. The app
  no longer queues its own reopen after clear (HS2-M4BNX5).
  App-owned token removal and editing restore the workspace/saved-view caret after
  the current render, before the next input task. Restoration coalesces repeated
  requests and respects newer focus handoffs; delayed animation frames must not
  collapse a replacement selection and duplicate surrounding query text (HS2-PR5TNA).

  Shared Kerf layout primitives own common shell geometry. Project, settings,
  notification, and terminal-operations sidebars use `Pane` for their
  header/content/footer structure; settings and notification groups use `ListHeader`
  instead of local heading imitations; and the terminal ticket rail uses `SunkenPanel`
  for its lowered content surface, with its explicit square shape at the rail edges.
  Beta.24's `List` owns vertical row layout in settings and notification navigation,
  command groups, connected provider rows, ticket-note cards, and terminal operations
  summaries. It supplies their gap without adding semantic list roles, padding, or
  another scroll owner:
  named navigation landmarks and existing pane/workspace scrolling remain intact.
  Semantic `ul`/`li` and ticket `listbox` collections retain their native structure.
  Toolbars select physical divider edges through `dividerSides` (empty for none).
  ListItem's upstream 18px icon geometry and managed token-search focus/disposal
  fixes come from the package. The optional StateBanner badge needs no additional
  consumer mapping because existing banners have no separate badge content.
  Product CSS is limited to placement and the tokens those components expose
  (HS2-ZMN977). The web client and spike also pin the beta.24 ESLint plugin,
  retaining the existing recommended rules while adopting its safe, explicit-only
  bundled assistant-configuration update behavior.

  Beta.49 begins the next configuration-first pass (HS2-737H3X). The terminal rename
  form delegates its vertical and action-row geometry to `List` and `Row`, removing its
  component stylesheet. Code Review composes its heading from canonical
  `ToolbarText` entries and its equal evidence columns from the new `Grid` primitive;
  only its application-owned narrow one-column policy remains in CSS. Nested command
  headings select `ListHeader`'s compact density instead of forcing a private height,
  terminal operations uses `ListHeader.inline` instead of overriding header margin and
  padding tokens, and note empty-state insets use the shared physical `sides` contract.
  The package's new `Text`, `Spacer`, Row baseline/inset configuration, Grid, and
  contrast-safe Web Awesome surface/badge contracts are the preferred vocabulary for
  subsequent migrations; product-specific semantics, responsive policy, and
  asymmetric layout remain application-owned.

  Ticket details and notes share one Markdown rendering boundary in the inspector, reader,
  and UX demos. Every link emitted by that renderer opens in a new browser tab and carries
  `noopener noreferrer`; raw HTML remains escaped and unsafe URL protocols remain inert.
  Paragraphs use a full shared spacing step so separate thoughts remain visually distinct
  in both compact inspector notes and the larger reader. Inspector typography is a
  low-specificity default: nested Markdown headings and paragraphs always retain the
  renderer's own spacing regardless of stylesheet evaluation order.
  Blockquotes follow email-reply semantics: a compact neutral rail, smaller quiet text,
  and inherited heading size de-emphasize quoted context instead of presenting it as a
  literary pull quote.

  Successful action acknowledgements use the shared transient toast and disappear
  without changing the owning surface's layout. Persistent inline status text is
  reserved for work that is still pending, actionable warnings, and failures. This
  applies consistently to diff-tool launches, settings saves, attachment operations,
  and corrupt-ticket recovery actions. Corrupt-ticket rows use the canonical 8px
  within-row rhythm and 16px inline inset; their recovery inspector separates major
  diagnostic/action regions by 24px while retaining 8px within each region.

  The shared left project sidebar presents a centered `M open, N up next` summary
  immediately above Drive. Both counts derive from the existing checkout ticket collection:
  open means exactly Not Started + Started (Backlog is not active work), and Up Next
  additionally requires the Up Next flag. Mutations and long-poll-driven collection refreshes
  update the summary reactively; the summary itself performs no polling or network request.

  Switching to an already-open project is a local projection change, not a loading gate. The
  tab click atomically restores that project's most recent ticket rows, repository summary,
  corrupt-ticket diagnostics, and command state from memory within the next frame, without
  showing the global loading indicator. An authoritative refresh follows in the background,
  but is not started until a task after that first browser paint, so request setup cannot
  delay the cached projection becoming visible. Project activation also resets the progressive
  ticket-row boundary, preventing a previously expanded large project from rebuilding every
  cached row before its first paint. Ticket motion is scoped to the project and view, and a
  queued animation rechecks that live scope before running, so an outgoing project's card
  clones cannot animate over the incoming board or temporarily change tag/card geometry.
  Ticket-detail presentation batches all related editor,
  inspector, duplicate, and code-review state into one reactive transition rather than
  remounting the application once per field.
  Every open project's replay-safe live-update stream also refreshes its cached ticket rows,
  even while another project is selected. Project-tab Up Next counts and live-claim activity
  therefore remain authoritative without activating each tab. Repeated invalidations for one
  project coalesce, distinct projects are retained, open metadata controls defer the batch
  until their popup closes, and a project that becomes active mid-refresh is promoted to the
  complete active-project reconciliation path. Activating a project supersedes every older
  background snapshot for that project, even if the user switches away again before the old
  request settles; this keeps its newly refreshed cached rows, exact navigation counts, and
  completion trend atomic. Closing a tab also invalidates its pending work.
  Project-activation and request generations reject late A→B→A responses, including delayed
  workspace-session draft restoration, so cached immediacy cannot introduce cross-project
  state or stale network writes. Opening a different project crosses this same activation
  boundary before provider or ticket I/O begins, so its sidebar never renders with the prior
  project's counts, completion history, view, or repository state. A first visit with no cache
  uses an empty target projection during the normal loading state. Closing a project evicts its
  rows, page cursor, and exact count summary so reopening cannot flash a stale snapshot.

  **Warm project tabs (HS2-AZZ9TF).** Recently used projects stay _warm_ so frequent tab
  switching is instant. A bounded least-recently-used set (default 8 projects, including the
  active one; `clients/web/src/project-warm-cache.ts`) keeps each warm project's ticket rows,
  page cursor, repository/corrupt/command projection, and AI tool inventory + defaults resident.
  Switching to a warm project:

  - paints entirely from memory, with no ticket loading placeholder;
  - restores its cached AI tool inventory rather than refetching it (only the open Settings → AI
    page forces an authoritative reload);
  - skips the all-project terminal dashboard reload when that project's terminal group is
    already loaded (terminal create/close still refresh it explicitly);
  - revalidates tickets, commands, shared views, and the single-project settings projection
    _silently_: those requests do not drive the global busy bars or the optional
    loading-activity label, so a warm switch never announces "Loading AI tools", "Preparing
    terminals", or similar.

  Background tab refreshes driven by each project's live change stream are silent too, and load
  the collection of the project's remembered view (not always the queue), so the cached rows are
  the ones it will show. Startup stays active-only (see the remembered-project startup rules
  above): a project becomes warm on its first visit, or when a live-refreshed background
  snapshot fits spare warm capacity; that first visit is an ordinary cold load. Activating a
  project beyond the bound evicts the least recently used warm project's resident projection
  (its tab counts and live stream stay); that project's next activation is a normal cold load
  with the loading state. Leaving a cold project before its first load finishes does not cache
  its empty placeholder as a warm projection.

  Drive is a production control, not demo-only state. Its split-button label reflects the
  machine-local default provider discovered from drivable plugin manifests. The arrow opens
  hierarchical Default/provider/model/effort overrides without a client-owned provider table.
  Project activation starts catalog discovery without blocking the rest of project startup, and the
  server prewarms the catalog in a background task at start so the first client typically finds it
  already discovered rather than paying cold subprocess discovery on its startup path (HS2-MYDN7C /
  HS2-10R4VV); discovery runs on the blocking pool so it never stalls concurrent clients (HS2-S66BZZ). If
  discovery is pending or fails, the arrow reports active discovery or the bounded error and retries
  on its next opening instead of presenting a transient failure as a confirmed empty installation.
  Parent rows rely on the shared menu's single disclosure marker; child provider, model, and
  effort choices use aligned semantic icons plus one highlighted current value with compact
  submenu insets, including at the supported narrow width. The menu floats 8 px above its Drive
  control anchor and separates connected submenu groups by 4 px (HS2-4Y6SM9). Choosing Default,
  provider, model, or effort keeps the menu open so related overrides can be set in sequence;
  its toggle and an outside pointer action dismiss it (HS2-S010QF). Machine defaults, Drive overrides,
  and command overrides keep the catalog as the primary model chooser and place manual entry
  behind an `Other…` action. The machine-default settings surface uses 24 px between major regions,
  16 px between provider/model/effort fields, and 8 px within its empty state (HS2-4Y6SM9);
  the focused entry dialog accepts an exact nonblank id, shows that custom id in the chooser only
  while it remains selected, and forgets it after a catalog selection. Live conversations retain
  their compact editable catalog-backed control. These paths keep detected models convenient
  without preventing an older or otherwise undiscovered model id.
  Drive prepares a stable dedicated connection scoped to that checkout and tool, opens and
  selects its AI-chat tab in the bottom drawer, then sends the `$hotsheet` workflow turn; later
  activations reuse that tab, connection, and retained session. The server resolves the checkout id to its code
  root before preparing the tool—ticket-store paths are never used as the working directory.
  The sidebar derives running
  state from `GET /connections`, refreshes it only from replayable `drive_updated` events,
  and disables a second Drive activation while that turn is busy; interruption remains in the
  selected chat when the connection advertises it. The Views add action opens a compact
  create dialog for a readable name and any ordinary search expression. The query editor fills
  the dialog content width on desktop and mobile, independently of the collapsed toolbar search. Saved views live in
  the code project's shared settings, appear in both the project sidebar and terminal ticket
  rail, and apply their query through the same inline text/token search pipeline. Their
  `custom:<id>` selection restores per project, follows replayable `views_updated` events,
  and falls back to Queue if a selected shared view is removed. Creating a view rejects empty,
  overlong, or case-insensitively duplicate names and empty or overlong queries before saving.
  Every saved-view row exposes labeled rename and delete actions. Rename keeps the stable view
  identity and search query, including while selected; delete confirms that tickets are not
  affected and returns a deleted active view to Queue. Both mutations preserve shared-setting
  ordering and reject case-insensitive name collisions. The delegated Views add action runs in
  capture phase and rejects events whose composed path belongs to an editable control or open
  dialog. It therefore resolves the original target before another action can synchronously morph
  that node, so submitting AI chat or closing a modal cannot queue a latent Create View dialog.
  Saved-view opening uses the controlled dialog open state and native name autofocus. No delayed
  application callback may reclaim focus after the user selects the query or reopen a cancelled
  dialog. Create and Edit transitions synchronously project their initial name into the live
  Web Awesome input property as well as rendered state, so a dirty name from an earlier opening
  cannot survive cancel/reopen or create/rename/create sequences (HS2-ZQNW62). Query token caret
  restoration remains independent of the opening lifecycle.

  Search has one primary surface: the project toolbar. The former global search overlay and
  its separate scope, suggestions, result rows, and saved-view handoff were removed because
  they duplicated the ordinary inline search flow without a distinct navigation role. Exact
  cross-project ticket references continue through the compact link-resolution chooser.
  Page and dialog titles compose a divider-free Kerf Toolbar with extra-large ToolbarText;
  page titles expose a level-one heading and dialog hosts retain their title/summary naming
  relationships. Supporting copy belongs to the application below the toolbar.
  Ticket readers are persistent Web Awesome dialogs opened through the native modal lifecycle,
  so focus is trapped by the platform, Escape closes only the top reader after nested controls,
  backdrop clicks do not dismiss it, and focus returns to the live opener or workspace fallback.
  The client commits the complete reader state before presenting the native dialog, preventing an
  empty modal frame during startup or open transitions; the same render-before-show rule applies to
  the quick-ticket composer. The accessibility host mirrors the visible modal bounds while the
  native shadow dialog alone owns pointer input, allowing a newer top-layer surface to receive it.
  Linked ticket readers keep their own qualified provider identity, provider capabilities,
  and text-edit sessions instead of borrowing the workspace selection. Details, note, and
  blocked-reason drafts autosave independently; refresh reconciliation preserves a dirty
  local draft, and closing a reader flushes its pending writes through the owning project's
  checkout before the dialog is allowed to hide and the stack unwinds. Failed flushes veto
  dismissal and preserve the visible reader and its drafts.

  The MessageSquare action is available before Drive and opens the production
  `AIConversation` dialog after preparing the default tool without sending a workflow turn.
  Project Chat and Drive use different stable connection ids: Chat is a general project
  conversation, while Drive is the explicit `$hotsheet` automation shortcut in the drawer.
  Kerf retains one receipt-ordered transcript and composer draft
  per connection; each submit appends a user message and one assistant message whose Markdown
  content grows in place from attributed `turn_event` output. The assistant result receives its
  transcript position when output first arrives, so activity emitted before a permission pause
  remains above the result that resumes afterward; output already shown before later activity
  keeps its earlier position. Replayed events are deduplicated without changing that order, and
  legacy saved conversations normalize to their previous messages-then-activity presentation
  before a new turn is appended. Native activity and permission
  events provide specific progress text, and connection-matched permission requests reuse the
  standard decision card inline. Completed, failed, and interrupted outcomes remain on their
  turn. Transcript and activity state is validated and persisted device-locally after every
  transition, so a client reload restores the exact received history. On server restart, the
  client pairs that history with the server's durable newest-first session catalog and recreates
  each latest connection/session before restoring eligible drawer tabs; corrupt local entries or
  one unavailable provider are isolated instead of discarding other conversations (HS2-YHQCS2).
  Stop appears only for a busy connection advertising `interrupt`; Enter sends and
  Shift+Enter adds a line. Plugins may advertise live model and effort changes, which the
  conversation applies to subsequent turns without changing provider. Closing a nested
  model or effort popup does not dismiss the owning conversation; only the conversation
  surface's own hide lifecycle closes it. The active effort is revalidated against every
  selected model's declared levels; unlisted models clear the inferred effort, and models with
  no effort support hide that control and
  omit effort entirely from subsequent turn requests. Connection refresh and transcript updates share the existing
  replay-safe WebSocket/long-poll stream—this surface adds no timer or simple polling.
  Each open project's primary transport is the credential-free, project-scoped
  `/ws/sync` browser bridge. One long-poll handshake establishes the replay cursor and
  one zero-wait replay after the socket opens closes the subscribe race; an idle healthy
  socket issues no further requests. Disconnects, rejected upgrades, and older servers
  fall back through `/ws/poll`, replay the missing cursor span, and retry WebSocket with
  capped exponential backoff. Replay refreshes ticket state only when it carries a ticket
  invalidation or overflow; an empty replay after a rejected upgrade does not wake the
  workspace. The existing local-mutation barrier,
  acknowledgement suppression, overflow recovery, and event consumers receive the same
  `PollResponse` batches regardless of which transport delivered them.
  While the dialog is closed, streamed transcript/activity state remains retained but the
  conversation surface is not mounted and does not subscribe the application root to those
  high-frequency signals. Opening it projects the accumulated transcript in one pass; background
  tool output cannot cause a root-render storm or disturb unrelated controls.
  Usage events attach token/cost metadata to the active assistant turn and derive a
  conversation total without a second counter; unknown cost is labeled unavailable. The same
  stream's normalized activity events are session/connection matched into the shared bounded
  transcript sequence, grouping adjacent activity while preserving their position between
  messages. Each Activity card carries one persistent, accessible AI-generated/may-contain-errors
  cue instead of repeating the disclosure on every row; row feedback retains its originating tool
  identity. The cue uses 8px between attribution and feedback, 4px inside each connected cluster,
  and keeps its 32px feedback targets plus 12px glyphs as explicit geometry (HS2-4Y6SM9).
  Long summaries remain contained, and backtick-delimited commands render as wrapping
  monospace code (HS2-4NYP8C, HS2-3NG164, HS2-0YES4Y). The
  dialog uses the shared compact dialog header instead of stacking a second application header
  beneath the platform dialog title. Its secondary line reports useful ready/working/message-count
  state instead of continuously exposing the opaque session id. Plugin-provided model and effort
  choices carry visible labels, and user-authored Markdown keeps loud-surface contrast throughout
  its nested content.
  Its full-width composer floats above the message stream as one rounded, elevated surface:
  the auto-growing textarea has no separate border, a small inner toolbar carries the keyboard
  hint and round Lucide up-arrow send action, and the transcript reserves enough bottom padding
  that its latest message remains readable above the overlay at wide and narrow sizes. In the
  bottom drawer, the embedded conversation remains height-bounded and the transcript alone
  scrolls, so a long conversation cannot push the floating input below the drawer viewport.
  Retrying preserves the earlier transcript and activity
  while clearing the stale failure. A launch failure is presented as a contained alert and keeps
  the underlying Codex daemon diagnostic, so failures such as an invalid control-socket path are
  actionable instead of collapsing to an unexplained exit status. Opening a populated transcript
  starts at its latest message; streamed growth remains pinned while the reader is already at the
  bottom, but never pulls them away from older messages they intentionally scrolled back to read.
  Rerenders that do not add content — selecting, extending, or clearing a message range — keep the
  reader's scroll position, so a range can be picked while scrolled back through history. Sending a
  message is an explicit continuation, so it returns the transcript to its latest edge and pins it.

  Dialog and embedded conversation headers use native Save/Stop buttons in Kerf
  `ToolbarControlGroup`; the group owns button geometry and keyboard focus treatment.
  Save is disabled for empty or active transcripts, and Stop appears only for an
  interruptible active response. Both controls preserve their production actions and
  keyboard activation. At narrow widths the header omits its repeated usage summary so
  tool identity and actions fit; per-message usage remains available (HS2-WXVAF3).

  Completed transcripts can be saved from either conversation presentation. Messages remain
  directly selectable in the live transcript: one pick anchors a range, a second includes every
  message between the boundaries, and the selected range can be copied to the system clipboard.
  Save reuses that selection instead of asking for it again. With a selection, the compact wizard
  first offers only Entire conversation or Selected range; with no selection it skips that scope
  step because the full transcript is the only possible result. The bundle-contents step presents
  optional contents as one vertical list and opens the host folder picker from Save, without a
  redundant review step or internal bundle-file inventory. Popup lifecycle events from
  controls inside the wizard never dismiss the wizard itself. Its major regions, body, within-group
  controls, and connected labels use Kerf's canonical 24/16/8/4 px spacing relationships
  (HS2-4Y6SM9). It creates a portable `.hotsheet-chat`
  directory bundle. Every bundle contains `manifest.json`, a
  readable `transcript.md`, and lossless `conversation.json`; optional `summary.md`, attachment,
  and original-media entries are explicit. Structured file references emitted with assistant
  messages retain stable ids, filenames, MIME types, and source URLs in conversation state; only
  references from the selected message range and enabled bundle categories are fetched byte-for-byte
  and sent to the already validated asset writer. The browser receives only an opaque destination token,
  while the trusted local bridge validates sizes and identities and owns all filesystem reads and
  writes. Selecting an existing bundle requires an explicit overwrite or same-conversation
  re-export, with revision lineage recorded in the manifest. Saved conversation bundles reopen
  from the terminal-drawer New menu. A selection that ends before the live transcript tail opens
  read-only; a tail-ending selection with a compatible project/session reconnects to that session
  and can continue. Continued turns remain live-only until the user deliberately saves again.

  The repository row is also the checkout's compact status chip. It distinguishes clean,
  dirty, ahead, behind, conflicted, and unavailable states from the checkout-scoped status
  snapshot. Activating it opens a repository popover with branch/upstream identity and the
  complete ahead, behind, staged, unstaged, untracked, and conflicted counts. Repository
  failures remain local to this surface instead of hiding the project, and its explicit
  Refresh action performs one request. The server monitors each open checkout off the
  project-open request path. Linux and Windows use their native recursive filesystem
  watcher and coalesce event bursts. macOS fingerprints Git's porcelain-v2 status between
  750 ms and a two-second idle backoff instead of using notify's recursive polling watcher;
  Git excludes ignored build output and dependencies, and unchanged fingerprints emit
  nothing. Both paths send checkout-scoped `repository_changed` invalidations over the
  existing WebSocket/long-poll stream. The active client then fetches one authoritative
  snapshot; the status surface never introduces simple polling.

  The real inspector's attachment surface materializes ordinary-sized browsed and
  dropped files before upload, so a macOS promised screenshot cannot disappear while
  `fetch` lazily reads it. Empty, unreadable, and short-read files are rejected with
  actionable guidance while valid siblings continue. Accepted files go to the selected
  ticket's checkout-scoped attachment endpoint. Unicode filenames—including the narrow
  no-break space macOS inserts into screenshot names—are percent-encoded into an
  ASCII-safe transport header and decoded by the server before sanitization. The client then refreshes both the
  selected ticket and project rows from the authoritative response.
  Attachment upload endpoints accept bodies up to 100 MiB so ordinary screen recordings
  are not rejected by the framework's smaller default body limit; the larger allowance is
  route-specific and does not loosen JSON request limits.

  Files selected in one browse/drop gesture share a durable attachment batch id and human
  actor role. Later human upload gestures reuse that batch until a durable ticket-status
  activity begins the next workflow round; an upload gesture alone never advances the round.
  The tab groups both its compact rows and gallery previews into friendly human/AI rounds
  without persisting round numbers, and labels missing provider/legacy metadata as Legacy /
  Uncategorized. Each transparent group reads as a titled section: its label switches to an
  inline editor on double-click, while its purpose remains a compact tag-sized control. Files
  move between groups by dragging either the file row or its media-grid preview (without a
  separate handle glyph). Preview drags use the existing attachment identity and never fall
  through to the upload path or create a copied attachment; only
  the current group target receives a focus outline, and a New group target appears while a
  file is being dragged. The item menu also exposes rename. The presentation remains usable at
  narrow widths without turning purpose tags into full-width fields.

  Notes resolve `attachment:filename` and cross-ticket
  `attachment:[TICKET-SLUG]filename` references. Inline-code references preserve spaces;
  their CommonMark delimiter expands beyond the longest run when a filename contains backticks,
  so every valid attachment filename remains representable.
  ordinary no-space references and standard Markdown link/image destinations are also
  accepted. Matching prefers the longest real attachment filename and leaves any trailing
  sentence punctuation in the prose, so `attachment:proof.png.` resolves `proof.png`.
  When that filename is already present on the loaded ticket, previews use the attachment's
  immutable id route; the by-name route remains the fallback for references whose target
  metadata has not been loaded. This keeps an inline preview and its gallery action on the
  same concrete attachment even when filenames or trailing prose are ambiguous.
  Missing ticket or filename targets do not reject a note (attachments may be uploaded next),
  but mutation callers receive prominent actionable warning feedback. Attach output leads with
  this filename syntax rather than its opaque storage id. Git-backed
  note writes also translate unambiguous bare attachment ids in ordinary prose into resolvable
  same-ticket or cross-ticket references, while retaining literal ids in code, URLs/paths, and
  ambiguous filename cases. Browser-compatible bare
  references and explicit image destinations render inline; explicit Markdown links
  remain compact links even when their target is an image, while other references
  ask the host to open the file with its default application. Right-click actions can
  download, copy the durable reference or host path, and reveal the file using the host
  platform's file manager. Inline image controls remain intrinsic-height block content,
  so later Markdown cannot overlap an image while it loads or after it is scaled. Copying
  a reference or resolved host path reports success through the shared transient toast,
  never by inserting a persistent status label into the attachment panel. The attachment
  context menu flips inward from every viewport edge, retains an 8px safe margin, and
  becomes internally scrollable when the viewport is smaller than the menu.

  Plain uppercase ticket references such as `HS2-BD09B6` in details and notes render
  as accessible links. Activating one searches exact slugs across every open project:
  one match opens an exact, read-only reader layer above the current inspector or reader,
  no match reports a transient toast, and multiple matches open a compact source chooser
  rather than the advanced-search surface. The chooser separates its major regions by 16 px,
  keeps adjacent result rows connected at zero gap, uses 8 px row insets with a 72 px minimum
  row height, and gives connected ticket/source metadata 4 px of air (HS2-4Y6SM9). The explicit `@<project-id>/<ticket-slug>`
  form, for example `@product-docs/HS2-BD09B6`, limits resolution to one open project and
  makes cross-project links unambiguous. A linked reader identifies its owning project and
  stack depth, and links inside it may push further layers without changing the workspace's
  selected project, ticket, or list/column view. Only the top reader is modal and interactive.
  Imported HS1 references resolve through each ticket's retained legacy number, including
  single-digit `HS-1` through `HS-9`; other one-character suffixes such as `AB-1` remain
  ordinary text so the legacy exception does not broaden the general slug grammar.
  Close or Escape removes one layer and restores focus to the link that opened it; unwinding
  the final linked layer returns to the unchanged inspector or editable workspace reader.
  References already inside Markdown links, inline/fenced code, or attachment controls
  remain unchanged.

  The Attachments tab keeps the complete file list and adds a responsive, wrapping
  grid of 160px square contained previews for browser-compatible image and video
  formats, including SVG, MP4, MOV, M4V, OGV, and WebM. Grid videos do not preload
  media and never autoplay; once their poster is ready, the client removes and reloads
  their source so later application renders cannot leave a hidden decoder or request alive.
  Their poster uses one predictable attachment `thumbnail`
  GET/PUT endpoint backed by a SHA-256 content-addressed host cache. Web clients seek and
  draw a frame with native video/canvas APIs and upload the JPEG during browser uploads;
  the first capable browser viewing an older or CLI-created video lazily backfills a
  missing poster. Existing posters short-circuit generation, while concurrent idempotent
  writes are last-write-wins. Missing posters, unsupported codecs, and generation or
  upload failures leave the original video playable and never prevent opening its ticket.
  A headless host may opportunistically use an already-installed `ffmpeg` executable
  (`HOTSHEET_FFMPEG` can name one outside `PATH`), but HS2 neither requires nor bundles it
  for posters and its absence is only a cache miss, not a setup failure. The original video response uses its native media MIME type,
  advertises byte-range support, and streams only the bounded file span selected by a
  valid single-range `206` response instead of loading the whole attachment into memory,
  so Safari and other media engines can discover duration and seek normally. The browser-native
  flow and server cache contract are identical on macOS, Linux, and Windows.
  A preview or inline image opens the same full-screen native modal media gallery. It occupies a
  newer top-layer position when launched from a ticket reader, and Escape consumes only the gallery
  before returning interaction to the still-open reader. Videos remain paused initially and
  use native `preload="auto"` so the browser presents the decoded first frame rather than
  carrying the grid thumbnail poster into the full-screen player. Scrubber input assigns the
  requested precise time directly to `HTMLMediaElement.currentTime`; the browser owns seek
  coalescing and frame decoding exactly as it does for an ordinary native media control. Hot
  Sheet does not start hidden playback, serialize seeks, gate input on frame callbacks, or
  require a user-visible play/pause cycle. They expose only Hot Sheet's custom play/pause, scrubber, time, and
  volume controls, never a second native browser control strip. The volume icon opens
  a click-persistent popup containing both the slider and mute action; only clicking
  outside that popup dismisses it. Playback ticks and scrub input update the live gallery
  imperatively and commit state only at interaction boundaries, avoiding application-wide
  Kerf renders for every media event. While a paused scrubber owns focus, delayed `timeupdate`
  events cannot overwrite its live value or reset an in-progress scrub to `00:00`; playback
  updates resume normally as soon as the video plays. When the video canvas has focus, Space or K toggles playback,
  Left/Right step one 30-fps frame, Shift+Left/Right and J/L jog one second, and Home/End seek
  to the media boundaries; video jogging never activates the image-gallery navigation path.
  Closing or changing gallery media explicitly pauses the prior video, removes its URL,
  clears any stream source, and reloads the source-free element before unmount so decoding,
  network, and media events cannot survive repeated gallery sessions. The playback footer occupies layout space below the media
  stage, so both contain and cover scales are calculated from the space that remains.
  Full-screen media preserves the source image or video's square outer geometry: the
  gallery does not add corner rounding to either the media or its sizing wrapper.
  The gallery retains its dark light-mode appearance under both device and explicit app
  dark preferences, including annotations, playback, volume popup, and zoom controls.
  Its dialog scopes `color-scheme: only light` so the shared inverse-neutral palette
  stays stable without duplicating colors or changing the surrounding app's theme.
  Images and video retain their original colors; no theme filter is applied (HS2-1CACB4).
  Its filename uses inverse toolbar text, while navigation, action, close, and zoom
  controls all use the shared dark ToolbarControlGroup tone so translucent backgrounds,
  borders, icons, and hover states retain contrast over arbitrary images. Gallery chrome
  uses canonical Kerf spacing: 24px between the media and viewport, 16px between peer
  toolbar groups and around the volume popup, and 8px within toolbars, timeline controls,
  footer controls, and connected popup actions; media-control dimensions remain explicit
  geometry rather than spacing tokens (HS2-4Y6SM9).
  Note-referenced images resolve to the same gallery identity as their attached-file
  thumbnail. Known Markdown images carry their immutable attachment id, and the gallery
  resolves that id (then ticket plus filename) before consulting potentially shared
  by-name URL aliases. Each image in a multi-image note therefore opens the media item
  the user actually selected; button, keyboard-arrow, and horizontal-swipe navigation
  continue from that item. Each cyclic navigation commits the new media identity and all
  per-item gallery reset state in one batched render, so refreshes or intermediate reset
  signals cannot restore the image that was just left.
  Swipe navigation arms only from a primary-pointer gesture on non-interactive media-stage
  space while markup and horizontal zoom panning are inactive. It follows one pointer and
  requires a 48px horizontal-dominant movement; controls, scrubbers, vertical motion, and
  cancelled gestures cannot change the selected attachment. Native video-slider drags
  therefore update the playhead and `currentTime` without resetting the media or annotations.
  Full-screen markup mode follows the exported image/video gallery wireframes: normalized
  rectangles can be drawn, selected, moved, resized from edges/corners, labeled, edited,
  and confirmation-deleted. Video and animated-SVG annotations can be points or inclusive
  time ranges. New timed rectangles span five percent of the media duration before and after
  the playhead, clamped at either media boundary. Rectangles appear over the media only while
  the playhead is inside their range or its review tolerance (the larger of one second or one
  percent of the media duration), while persistent
  white wireframe-style indicators spanning every point or range remain over the scrubber
  regardless of selection. Selecting a visible annotation rectangle
  adds high-contrast white square-bracket range handles to the timeline; those endpoints can
  be dragged with a real pointer or adjusted with the arrow keys, replacing ambiguous toolbar chevrons. Only the
  selected annotation exposes adjustable range brackets, and clicking empty image or video
  canvas space clears the rectangle selection and its resize/range handles.
  The gallery annotation action carries the current annotation-count badge, and media-grid
  cards with annotations carry a lower-right annotation marker so review work is visible
  before opening the media. The gallery action menu includes Remove so a user can verify
  the full-size image or video before deleting it; removal closes the deleted media view.
  Annotation geometry and time are stored on attachment metadata, so zooming, resizing,
  reopening, and git synchronization do not change their meaning. Full-ticket REST, MCP,
  CLI, and AI worklist reads all use the canonical ticket attachment shape, including the
  complete annotation ids, rectangles, time ranges, and text; annotations are therefore
  part of the ticket context presented to an AI rather than client-only state.
  Markup edits remain local while the annotation mode is open. Finishing markup or closing
  or navigating away from the gallery persists the complete batch once. A changed batch
  atomically adds one activity note that links the attachment and lists each added, updated,
  or removed annotation with normalized percentage bounds, optional time range, and caption;
  an unchanged session performs no write and adds no note.
  New-ticket attachment evidence follows the same safety policy before a ticket exists:
  users can drop files on the collapsed New ticket launcher or anywhere on the expanded
  composer, inspect and remove the staged filenames, and cancel to discard the entire
  pending set. Creation first persists the ticket and then uploads each staged file in
  order. A create failure leaves the draft and evidence available to retry; partial upload
  failures keep the created ticket, continue valid siblings, and direct the user to retry
  failed files from that ticket's Attachments tab. Providers must advertise both create
  and attachment capabilities before the composer accepts evidence.
  The title also accepts leading tag shorthand: `[client] [Needs Review] Fix selection`
  creates `Fix selection` with `client` and `Needs-Review` tags. The client sends the
  original title and renders the authoritative normalized ticket returned by the server,
  keeping Git and external-provider creation behavior identical (HS2-CHZKR5).
  When the selected provider advertises attachment support, each attachment exposes one
  accessible Lucide ellipsis button. Activating it or right-clicking anywhere on the row
  opens the same shared ListItem-based menu for Open, Download, Copy reference,
  host-normalized reveal (Finder, File Explorer, or file manager), and Remove.
  Double-clicking the attachment row remains a direct Open shortcut, while
  activating the ellipsis never opens the file. Upload/removal progress and failures remain visible in the
  attachment panel.

  The inspector includes a Code Review segment for ticket-associated code history. It
  begins with a server-derived change-evidence summary: unique documentation, test,
  source, and other file counts plus separate counts for newly added and modified existing
  test files. The summary is an action that opens a repository-browser-style master/detail
  dialog, with Docs, Tests, Source, and Other views and Git-letter file rows. The summary
  card grows to contain every count and test-change line even in the narrow inspector.
  The shared review surface uses 16px between evidence/comparison/history regions, 8px
  within cards and commit/range rows, and 4px for connected headings, icon-label pairs,
  commit body details, and Git ref clusters; fixed graph, action, icon, and row dimensions
  remain explicit geometry (HS2-4Y6SM9).
  Clicking a file row selects it; only its visible ellipsis or a right-click opens the
  action menu. Platform-additive and range selection permit batch Show Diff and path-copy
  operations, while single-file-only Open and host-native reveal actions are disabled for
  a multi-selection. Show Diff opens the selected file or files in the configured difftool across the complete span from the parent of the
  oldest associated commit through the newest associated commit. The server derives
  and revalidates the exact path and range; arbitrary browser-supplied files are rejected.
  Classification runs against that committed range (not the browser's working tree) and is configurable through the effective project setting
  `code_review_file_classes`, whose JSON object contains `docs`, `tests`, and `source`
  glob arrays. The defaults recognize `docs/**`, Markdown, conventional test/spec paths,
  and the common `src`, `crates`, `clients`, and `apps` source roots. For example,
  `hotsheet-cli settings set code_review_file_classes '{"docs":["docs/**"],"tests":["**/*.test.*"],"source":["src/**"]}'`
  commits a shared project override; local overrides use `--scope local`.
  The segment lists each matching commit subject, up to two lines of its Markdown-formatted message
  body, abbreviated SHA, and date even when no review tool is configured. A commit that a Git ref
  points at also shows those ref decorations as small labels beside its subject, classified and
  color-coded by kind — the current `HEAD → branch`, other local branches, remote-tracking
  branches, and tags — mirroring `git log --decorate` (HS2-SFJ5TE). Clicking the
  commit summary toggles its complete Markdown body. When the checkout has a Git
  `diff.tool`, each commit has an Open action
  and each adjacent multi-commit run has its own bundle action with explicit oldest/newest
  boundaries. Multiple disjoint runs therefore remain separately reviewable; interleaved
  unrelated commits are never silently included. The inspector tab uses the Lucide
  `message-square-code` icon while individual commit and range actions retain their
  established icons. Commit rows are flush with the review list instead of inheriting
  the component library's native list-item indentation. Loading and launch errors stay in
  the segment and do not replace ticket content or use the foreground project-loading
  indicator. Reader and sidebar presentations render the same `TicketInspector` tab and
  receive the same review result, loading, launch-message, and expanded-commit state; the
  larger reader therefore cannot fall back to an empty, separately implemented panel.
  All discovery, target validation, and process launch remain server-owned.

  The ticket context menu also exposes the server's structured close operation when the
  provider advertises `close` and `close_reasons`. The close dialog records Completed,
  Not planned, Duplicate, or Obsolete rather than approximating those outcomes with a
  status patch or note. Duplicate closure searches every open project, labels each result
  with its owning project, excludes and rejects the exact source identity, requires an
  explicit canonical target, and sends its `(project_id, connection_id, native_id)` tuple.
  The server validates that tuple against the target checkout and persists it as
  `@project/connection:native-id`, so same-slug tickets cannot become ambiguous. The
  inspector resolves the saved identity through an exact ticket lookup rather than
  exposing its internal ID. The forward “Duplicate of” relationship uses the same
  project-plus-slug, title, icon, and full-row action treatment as reverse “Duplicates”
  backlinks. The reverse relationship passes its result count through the shared
  `ListHeader` count contract, matching the neutral count badge used by Notes and
  Attachments, then switches projects to open the exact canonical target even when it is
  outside the current list filter or loaded page.
  Checkout-scoped self-reference
  rejection compares all three identity fields; compatibility provider/store routes lack
  a source project and intentionally treat the same connection/native pair as the same
  underlying ticket. A canonical ticket also lists every reverse duplicate relationship
  discoverable across the machine server's registered checkouts and provider sources.
  Each backlink always includes its project name and slug, so same-slug results remain
  distinct, and opens the exact project/connection/native reference. Legacy bare-ULID
  relationships remain discoverable. An inaccessible registered project does not suppress
  healthy backlinks; the inspector names a still-present project whose additional results
  could not be checked. Remembered registrations for deleted checkout roots, and git-source
  directories recreated without an HS2 `hotsheet-store.json`, are ignored: neither can contain
  a usable backlink, and neither may create a permanent warning on every ticket. A recognizable
  HS2 store that fails during enumeration remains a transparent partial-result warning.

  Attachment references in ticket details use the same owning-ticket context as notes:
  same-ticket references prefer immutable attachment IDs, while
  `attachment:[HS2-…]filename` resolves through that explicitly named ticket.

  AI turn events carry the server event-log cursor through every project WebSocket or long-poll stream.
  The client applies each cursor-addressed turn event once globally, so multiple open
  projects cannot append the same provider-independent assistant output repeatedly.

  Project refresh loads healthy tickets and checkout-scoped corrupt-ticket diagnostics
  independently. Live diagnostics supersede any stale indexed row with the same recovered
  slug, so selecting that visible ticket always opens recovery instead of retrying a doomed
  ticket fetch. The server watcher emits the existing replayable `changed` event even
  when a newly malformed file cannot be reindexed, so this replacement happens through
  the normal long poll without manual reload. A malformed file therefore cannot suppress healthy rows: the workspace
  remains usable and renders each unreadable file as a selectable warning row with the
  recovered slug/id or filename and failure state. Its selected state uses a continuous,
  uniform brand outline through all four rounded corners. Selecting it opens the normal
  inspector region with the complete error, exact file path, a platform-specific reveal
  action, and an **Attempt AI repair** action. Linked-store diagnostics retain
  server-provided store attribution. The local bridge revalidates that exact path against
  authenticated live diagnostics before launching an argument-array OS command. AI repair creates
  an idempotent, high-priority Up Next repair ticket in the affected store with preservation
  and validation instructions. It does not edit the corrupt file immediately. A ticket from
  a newer schema offers reveal plus update guidance, not unsafe automatic downgrade.

  While a project is selected, the browser keeps a cursor-based long poll open through
  the credential-hiding bridge. Ticket create/update/claim/move/delete events and replay
  overflow coalesce into an authoritative project refresh, including reconciliation of
  the selected inspector ticket. Background reconciliation is silent: it does not toggle
  the foreground loading surface. While a metadata select popup is open, ticket refreshes
  are coalesced and deferred until the popup closes, so the application never hides and
  reopens the user's active chooser. The per-project coordinator also closes the drain-tail
  handoff atomically: an invalidation arriving after the current batch empties but before its
  runner settles always starts a successor drain, and every request promise settles only after
  its coalesced authoritative refresh. This prevents a ticket from remaining absent until an
  unrelated search merges it into client state (HS2-SJGRTK). Project switches abort the previous poll. Network failure retries
  with a fresh cursor and bounded exponential backoff without refreshing on every
  failure; the first successful reconnect reconciles once. The server-side project
  bridge retains legacy query authentication for `/ws/poll`, so a newer browser client
  does not spin on immediate authentication failures from an older running server.
  Activity/presence events are deliberately outside this ticket-refresh lifecycle.

  The default `Queue` view is the active working set and intentionally excludes both
  Backlog and every terminal/archive status. Backlog and Archive are disjoint explicit
  views with counts derived from those same predicates. The new-ticket composer is
  available in Queue and Backlog but hidden in Archive; creation from Backlog defaults
  the new ticket to backlog status. A persistent Web Awesome dialog host opens from the
  live New ticket trigger with native modality, focus confinement, topmost Escape ordering,
  and trigger restoration. Nested selects consume their own Escape first, backdrop clicks
  do not dismiss the composer, and an in-flight create vetoes dismissal. Cancel/Escape
  reset drafts only after native hide completes; project disposal still clears staged work,
  while successful creation moves focus into the new ticket's Details editor. Relaunch
  restores saved draft fields and staged evidence but starts the composer closed, preventing
  remembered projects from racing modal show/hide while the initial project set loads.
  Its expanded first row keeps the title beside the
  category and an immediately trailing star toggle; the star creates directly in Up Next
  (and therefore overrides a Backlog-view default to active Not Started). A full-width
  Details textarea follows on its own row, starts one text line tall, and resizes vertically.
  The form uses 16 px between sibling regions, 8 px within metadata/evidence/action groups,
  and 4 px for connected labels and icon details (HS2-4Y6SM9).
  Its chosen height is a device-local preference. The live textarea is morph-protected so a
  controlled-value rerender cannot replace the browser-resized element, while
  cancellation/reopening and later new-ticket sessions restore the persisted height.
  Switching among Queue, Backlog, and Archive requests a bounded compact page scoped to that
  collection before pagination; it never fetches full ticket bodies or filters an arbitrary
  mixed-status page in the browser. The selected sidebar item commits immediately while the
  workspace reports that exact view as loading until its request resolves. Only a successful
  response may present authoritative “No tickets in…” feedback; a failed request names the
  view it could not load instead. The loader also accepts the legacy bare ticket-array response
  from compatible older servers, treating it as one complete page while pagination deployments converge.
  collection refresh reconciles the first progressive row tranche. Counts render within
  that same selectable item rather than outside its selected background. Large views initially
  render 40 rows, continue in idle chunks, and expose an explicit continuation control when
  the selected collection exceeds the server page size.
  Column presentation leaves idle TicketRows borderless, including the wide
  single-column Backlog and Archive boards, while selection supplies the rounded blue
  outline. Pointer preview never changes the row fill: a quiet temporary blue outline
  marks the hovered row instead.
  The composer owns its bottom spacing; while it is rendered, both list and edge-to-edge
  board workspaces remove their otherwise-normal top inset so only one vertical gap is
  present. Composer-free views retain the workspace inset. Likewise, inspector tabs own
  the gap above their content instead of stacking it with content padding.

  Ticket selection follows the native HS1 interaction model in both presentations:
  plain click replaces the selection, Command/Ctrl-click toggles one ticket, and
  Shift-click selects a contiguous range. Board ranges are deliberately column-local;
  Shift-clicking into another column becomes a single selection. Clicking unused list
  or column space clears the selection. A plain click on the one already-selected,
  fully loaded ticket is a no-op; when an inspector editor owns focus, that click also
  leaves the editor focused instead of triggering a redundant blur/refetch cycle.
  Modifier clicks and unloaded selections still follow their normal selection paths.
  Every selected row uses the same blue border
  and background component state in list and column layouts. The inspector remains
  available in both layouts: it shows ticket details only for exactly one selection,
  otherwise showing the HS1-style zero- or multi-selection guidance placeholder. The
  zero-selection placeholder keeps its close toolbar visually open to the guidance
  area without an unnecessary divider; transitional loading and multi-selection
  placeholders retain their intentional toolbar separator.
  The main toolbar's star applies the same bulk Up Next toggle as the row menu, while
  its ellipsis opens that shared menu without duplicating the star action; both disable
  with no selection or unsupported providers. After a successful status mutation, any
  ticket no longer present in Queue, Backlog, or Archive is removed from selection, and
  a hidden single-ticket inspector is cleared.
  The compact sort `Select` draws exactly one focus ring on its enclosing control group:
  the inner Web Awesome combobox ring is suppressed, the outer ring remains visible while
  the popup is open, and a true pill radius keeps the expanded outline from reading as a
  rounded rectangle around the 44px group (HS2-M1DF1D).
  When the center column contracts around an open sidebar/inspector or at 200% zoom,
  every control removed from the toolbar remains available from its keyboard- and
  touch-operable overflow menu. Utility actions relocate first, followed by sort,
  search, and finally the view switcher. The overflow uses the same action/state
  contracts as the visible controls; opening Search temporarily gives the narrow
  toolbar to the editable field so it remains focused and contained.

  Workspace chrome preferences are device-local browser state. The client restores the
  last view mode, sort field and direction, project-sidebar and inspector visibility,
  outer project-command expansion, each named command group's independent per-project
  collapsed state, and independently clamped sidebar/inspector widths
  across reloads. Missing, partial, malformed, or unknown enum values fall back per
  field to safe defaults rather than preventing project open. Selecting a ticket still
  reopens the inspector and persists that explicit state transition. List or column mode
  remains selected while visiting the cross-project terminal dashboard. Ticket references
  in details or notes open layered readers without changing the workspace view at all.
  Status, priority, and title sorts use most-recently-updated first as their secondary
  order, regardless of the selected primary direction; exact remaining ties use the
  stable ticket slug. Updated-date sorting continues to follow its selected direction.

- **Rust shell + web UI, no embedded core.** The Rust shell's job on the server
  front is to **launch and supervise the local `hotsheet-server`** (spawn it
  detached if `instance.json` shows none, watch its health) — not to run the core
  in-process. The web UI talks HTTP/WS to that server exactly like the remote case.
  Remote projects use the Rust-side mTLS proxy already designed and scaffolded in
  HS1 (§112.5.1 — a loopback proxy that presents the device cert, sidestepping
  every WebView's broken client-cert handling).
- **UI framework: Kerf (`kerfjs`)** (maintainer, 2026-08-22 — revises the earlier
  "small mainstream framework / not kerf" lean). The web UI uses **Kerf**, the
  maintainer's own fine-grained-signals + JSX framework (~12 KB, no vDOM, no
  compiler): `signal`/`array-signal` for live WS-driven ticket lists, `ref`/`scope`
  for imperative widgets like the xterm terminal, tree-shakable list virtualization
  (kerf 4.2), and a planned tree-shakable router. Since HS1 the framework has matured
  into a published, well-tested v4 — so the "don't re-hand-roll a runtime" concern
  that pointed away from it no longer applies, and dogfooding HS2 on kerf keeps the
  whole self-hosting loop (agents included — kerf ships an AI skill + `llms.txt`) in
  tooling the maintainer owns. Client-local, revisitable, and it does not affect the
  service. (Standing caveat: single-maintainer bus factor, mitigated by it being
  dogfooded by that same maintainer.)
- **Component library: Web Awesome Core on top of Kerf** (validated 2026-08-25).
  Kerf owns state, routing, lists, API resources, and delegation; framework-neutral
  Web Awesome custom elements own accessible controls, dialogs, drawers, menus, and
  related UI primitives. Components are pinned npm dependencies, cherry-picked, and
  bundled locally for offline Tauri use. The executable spike in
  `spikes/kerf-webawesome/` proves custom-element identity, value, and focus survive
  Kerf morphs; theme tokens work; lifecycle events delegate; and the production
  bundle makes no external requests. Web Awesome 3.11 form controls emit host-level
  standard `input` / `change` events—not `wa-input` / `wa-change`; component lifecycle
  events retain names such as `wa-show` / `wa-hide`.
- **Iconography: Lucide only** (maintainer, 2026-08-26). Decorative and symbolic
  UI never uses emoji or font glyphs as stand-in icons. All clients share the Lucide
  metaphor and render official Lucide assets through a platform-appropriate shared
  component. Adjacent text owns accessibility meaning; icon-only controls have an
  explicit accessible name. Ask before choosing when multiple Lucide metaphors are
  materially plausible.
- **Platforms:** macOS primary; Linux/Windows via the same Tauri pipeline as HS1
  (best-effort, community-tested).

## 6.4 Native SwiftUI client (macOS + iOS)

- **Native API client** talking HTTP/WS to a `hotsheet-server` — **not** an
  embedded-core app. No `uniffi` bindings are needed (they were only for the retired
  embedded-core plan).
- **Why native (not just Tauri on iOS):** a first-class iOS experience — real
  navigation, share sheet, notifications, widgets — and macOS menu-bar integration.
  The identical domain behavior across surfaces comes from every surface talking to
  the _same server_, not from sharing a linked library.
- **macOS:** auto-starts + supervises the local server (like Tauri, §6.3).
- **iOS is remote-first — and structurally so.** A phone can't run an independent
  background server (iOS background-execution limits), and it rarely hosts the git
  repos or drives AI tools anyway. Its job is to view/triage tickets and answer
  permission prompts against a server running on a Mac — the **remote-server** path
  (mTLS, QR pairing — §112.6). There is no local-server-on-iOS mode. See
  [08-distributed-and-remote.md](08-distributed-and-remote.md).
- **Terminals/AI-drive on mobile:** out of first scope. Mobile watches and
  triages; driving AI tools stays on the desktop/server. Answering permission
  prompts and reading busy state _do_ work on mobile (they're just API events).

## 6.5 Android (last)

A Kotlin/Compose **API client** (HTTP/WS), fourth and last in the sequence. Like
iOS it's a pure server consumer — no core embedding, no JNI bridge needed. Only a
view layer is new work.

## 6.6 What every client must render (feature floor for v1)

- Bullet-list ticket entry + the list/column views, categories, priorities, the
  7 statuses, up_next, tags, notes, attachments.
- Live updates over WebSocket (index changes, claims, busy state).
- The AI-drive surface: launch/trigger a tool, the **permission popup**, the
  **busy indicator**, the connection count.
- Multi-project tabs (local + remote), with pointer drag reordering, remembered
  device-local project order, and an accessible per-project Up Next count. The count
  is omitted at zero, shows its exact value through 99, and renders `99+` above that
  while retaining the full count for assistive technology. Any live ticket claim turns
  the yellow count circle into a half-speed rotating activity ring split into one segment per
  simultaneous active claim (capped at 8 drawn segments for legibility; the accessible
  label still reports the true active count), while the center label always projects the Up Next count
  (so it can read `0` while work is active); the ring's segment count is the only axis that
  tracks active claims (HS2-3TGYER). Each open project's replay-safe WebSocket-first stream
  reconciles those cached rows after ticket, claim, renew, and release events even when the
  project is not selected; the indicator never waits for a tab activation to become current.
  Because lease expiry is a passage of time rather than a server mutation, the client also
  decrements the known aggregate at the expiry boundary and immediately requests an authoritative
  background snapshot (HS2-MV7S1Y). A
  closeable local tab's empty trailing reserve subtracts
  the selector gap already present beside the label, keeping that label geometrically
  centered in the complete pill rather than balancing the close control twice.
- Search (FTS) and filtered views.

Closing a project tab always requires confirmation, including when no live resources are
running; the empty-resource form is compact, names the destructive action directly, and
does not present the absence of running resources as a warning.
The close flow first inventories live terminals and AI chats. When any are running, the
confirmation dialog uses the shared menu navigation to select an item and
shows either its live, read-only terminal renderer or the exact shared `AIConversation`
in a live read-only mode. The chat preview carries the complete retained messages,
activity, usage, progress, and error state without exposing its composer or Save action;
provider/model/effort context remains visible. Stable per-resource preview identities and
the absence of an unused asynchronous terminal-snapshot rewrite prevent terminal/chat
remounts from leaving duplicated or ghosted transcript content in the wide dialog. The
dialog uses 24px detail/terminal insets, 16px warning and consequence regions, 8px resource-pane
and action spacing, and gapless adjacent resource rows; constrained layouts use a 16px preview
inset and 8px viewport gutter while preserving all preview/control geometry (HS2-4Y6SM9).
**Keep Running** removes only the local project tab. Reopening the project reconciles
eligible drawer Chat, Drive, and resumed-saved-chat connection ids from the server into
their original AI-chat tabs, including provider, model, effort, busy, action, and session
state (HS2-D34C2V). Transcript state already received in the same app window remains keyed
to that connection and returns with the tab; after an app or server restart, validated
device-local transcript state is paired with the latest durable provider session and returns
with the reconstructed tab (HS2-YHQCS2). **Stop & Close** explicitly deletes
every listed terminal and AI connection before removing the tab. Cancel and switching
away from a borrowed terminal preview make the surviving interactive drawer terminal
reclaim its fitted geometry after the preview disconnects. Cancel and native dialog
dismissal preserve both the project and all resources. Multi-tab close actions
apply the same decision project by project instead of silently terminating background
work.

The web client now implements the permission portion of that floor for Claude and
Codex. Every open project has a replay-safe long poll; a `permission_asked` event
triggers one fetch of that project's authenticated permission and connection state. A
replayable `permission_resolved` event triggers the same reconciliation, so decisions made
by another client or transport become history entries instead of silently disappearing;
empty-action generic requests such as `ToolSearch` follow the same lifecycle. That
authoritative resolution immediately removes the popup even while this client's Allow or
Deny HTTP response is still in flight; a delayed response is idempotent and cannot
resurrect or duplicate the resolved request.
For an externally launched interactive Claude session, that event is emitted only from
Claude Code's `PermissionRequest` hook after its native modes and allow/deny/ask rules
determine that a dialog is actually required. Ordinary reads and other inherently or
previously allowed operations therefore remain silent in Hot Sheet, matching the
terminal session instead of presenting advisory-looking prompts (HS2-N4R6F3).
The local Allow/Deny path has the same immediate behavior: it optimistically removes the
request and records the presumed decision before awaiting transport. Only a communication
failure rolls that history back and restores the popup with an inline error, so network
latency never invites repeated clicks.
There is no fixed-interval network polling. The main segmented control reflects only the
selected project's pending count, while every project tab keeps its own badge; a non-modal
popup can still surface an urgent request even when another project is selected. When a
standalone AI conversation is open, the active permission popup is
promoted into that dialog's top layer so it remains visible and interactive instead of
being trapped beneath the modal; resolving it uses the same authoritative permission
path. Standalone conversation dialogs use native light-dismiss and Escape behavior and
do not duplicate that dismissal with a header close button. The Notifications view, its
Pending/24 Hours/7 Days counts, and newest-first machine-local client history are scoped to
the selected project and switch immediately with its tab; a request that
disappears without this client resolving it is labeled “Decision made outside Hot
Sheet.” Responded history cards retain the same full bottom inset when their action is
empty and no decision-button footer is rendered, so the final summary cannot sit against
the card edge. The right inspector region remains present and manually collapsible in this
view rather than changing the workspace width.

Ignore is client-only and hides the popup without answering. When the server advertises
durable Always Allow support, actions are Ignore, Deny, Always Allow, and Allow Once;
otherwise the final action is simply Allow. Per-project localStorage settings can turn
on auto-Allow or auto-Deny after 15 seconds or 1/2/5/15/60 minutes. The timer accumulates
only while that request's popup is visibly presented, updates once per second, pauses when
hidden or ignored, and appears as flat text aligned with the decision buttons plus an
icon-only pause control whose accessible label and tooltip name the automatic outcome.
Stopping automation completely removes both the countdown and pause control for that
request while leaving it open for a manual decision. Timer ticks update only that text
node—not the application root—so an unrelated open Web Awesome select or popup retains
its live element, open state, focus, and selection. Automatic decisions use the same
authenticated route as clicks and are distinguished in client history.

The long tail of HS1 UI (rule-oriented query builders, saved-view management, stats, Announcer, telemetry
dashboards, print) remains **deferred**, each its own ticket after the floor lands. The
user-facing **Workspace grid** (internally `TerminalDashboard`) is active work:
HS2-946EQG settled its interaction contract from the
updated project/drawer wireframes. HS2-2ZCN7K shipped the global dashboard shell,
flow layout, magnification/hiding, and independent persisted width/high zoom
controls. HS2-ZTYJKD completes that visibility action: the eye opens a shared-component
Manage Workspace Visibility dialog, its badge counts terminals and AI chats hidden by the active group, and the
adjacent Select switches among device-local named groups. Default is permanent; named groups
can be created, renamed, and removed, and each group records workspace-item inclusion without
destroying sessions. The compact selector's open menu sizes to its option content instead of
the narrow closed control, so checkmarks and complete group names remain visible.
Production imports the canonical `@kerfjs/ui/select/register` boundary at boot.
The contract for that shared lifecycle keeps the latest open/close request authoritative
across interrupted animations and viewport changes; a previous close cannot hide a
reopened menu or return its geometry to the origin. Kerf beta.49 retains the lifecycle
installer shipped in beta.34's immutable npm artifact, and the production regression
exercises repeated interrupted close, resize, and reopen sequences at wide and narrow
widths. Consumers do not add popup delays or positioning repairs. The dialog's
tab toolbar remains transparent against the white dialog
surface rather than introducing a separate gray band. Visibility groups apply only to the
global dashboard; the project drawer
always shows its project's terminals and has no visibility controls. Newly created terminals
and newly created AI chats appear in Default and start hidden in existing named groups.
The dialog lists both terminal and chat tiles and filters rows through a normal multi-select
for Shell Terminals, AI Terminals, and AI Chat. Select All and Deselect All affect the
filter only, keep the popup open, and never change visibility. Hide listed / Show listed
apply only to matching rows in the selected visibility group. No selected types yields an
explicit empty result and disabled bulk actions. Switching groups retains the filter;
reopening the dialog resets it to all supported types. Web Browsers is disabled until
browser tiles ship under HS2-7VS6SF. Terminal kind comes from the creation request:
`connect` marks AI, ordinary/default-shell launches remain shell, and the kind persists
through PTY reattachment and broker-backed server restart. Legacy sessions without kind
remain shell; titles, command output, and OSC8 hyperlinks never determine kind (HS2-SE3RVM).
The dashboard always uses one
ungrouped flow, so there is no redundant project/none grouping selector. Terminal and AI-chat
cards share that flow in both the cross-project Workspace grid and the drawer's **Project
grid**. AI cards show provider, working/ready state, and the latest conversation summary.
Their preview bodies use a fixed natural canvas and the terminal card's one-pass uniform
scale, so increasing either grid's fit count shrinks the complete chat presentation instead of
clipping normal-size typography inside a narrow tile. The shared footer remains ordinary card
chrome, matching terminal cards. AI cards
retain their mixed terminal/chat drawer order, and reopen the owning project and embedded
conversation by click or keyboard. The global Workspace grid launcher uses the Lucide
`grid-3x3` icon, while the fixed Project grid tab in the bottom drawer keeps the distinct
Lucide `layout-grid` icon. The broader name leaves room for browser and other workspace
surfaces later.
The dashboard keeps a resizable left operations sidebar open by default. It presents one
`ListHeader` and the shared seven-day `ProjectSummary` for every open project. With multiple
projects, a leading `All projects` group sums each aligned trend day plus the completed-today
and in-progress counts. Every chart in that cross-project list uses the maximum of the summed
`All projects` trend as its shared scale, so equal bar heights mean equal activity across the
aggregate and each project; a single-project summary continues to use its own maximum. An inset
divider below that aggregate keeps it distinct from the individual project list. Each constituent
chart also draws the aligned aggregate values as slightly wider neutral-gray bars behind its blue
project values, so the total scale remains visible in-place; the aggregate row and single-project
case do not add that comparison layer. Every bar is scaled proportionally to that shared maximum
(a bar is `value / maximum` of the chart height, with only a tiny floor to keep a non-zero bar
visible), so a project's blue fill never exceeds its gray aggregate bar and a small day's fill
stays proportional instead of clamping up to a fixed minimum that would read as the whole day
(HS2-C9JM65). The project summaries open project statistics and the aggregate opens
cross-project statistics. The same bounded ticket-page response carries an exact seven-day
completion summary using browser-local calendar boundaries, so archived completions and rows
beyond the retained page remain represented without another request or polling. It remains usable beside
the grid and ticket rail at the supported 1024×600 floor, and can be hidden and restored from
the leading edge of the dashboard toolbar.
The dashboard keeps a resizable right ticket rail open by default and allows it to be hidden
and restored from the dashboard toolbar. That rail reuses the selected project's list and
notification views, compact workspace actions, content-sized project selector, and quick-ticket
launcher; board and settings modes are deliberately absent. Its view heading ends with the same
quiet separator used at the top of ticket collections, clearly dividing the white header from the
lowered scrolling ticket surface. The ticket collection keeps its intrinsic height inside that
surface so the rail remains the vertical scroll owner and every row stays reachable even when the
collection exceeds the viewport (HS2-8J0378). The launcher retains the shared blue pill presentation used by
list and column views but shortens its rail label to `Ticket…` so it stays on one line. The
rectangular list/notification
segmented control owns its full first row, while sort, selection actions, and the compact search
launcher share the second row. Activating search animates it onto a dedicated full-width third
row, where tag-autocomplete options stretch across the popup with consistently left-aligned
labels. The rail uses 8px within its project/header/content groups, a 4px top inset for the
connected control cluster, and an 8px search-row transition offset; its toolbar, selector,
button, and transition dimensions remain explicit geometry (HS2-4Y6SM9). This is the same
advanced search surface and state as the main workspace, including chips,
tag completion, attachment/presence filters, relative or local dates, and syntax help. A
well-formed structured value becomes a chip only after an explicit impossible continuation
such as trailing whitespace or Enter; incidental focus loss never commits a partial value.
Whitespace used only to commit a trailing chip is consumed with the token, so the caret is
immediately adjacent and one Backspace removes that chip instead of an invisible separator.
Quoted filters also remain editable until the closing quote and explicit commit delimiter.
Committed chips stay at their exact positions inside the editable expression, with caret stops
before, between, and after them; pressing Right Arrow from Chromium's before-chip, element-boundary,
or chip-descendant selection shapes reaches the trailing editable suffix instead of trapping the caret.
Queries such as `NOT tag:client AND parser` retain readable
boolean order. Compact 1.25rem chips do not make an otherwise single-line field taller or
misalign its icons. Ordinary text wraps at character boundaries while each chip wraps atomically;
the toolbar grows downward without clipping or moving peer controls. A chip can return to
editable text through its labeled edit action or a double-click, while removal leaves the search
focused at its former position. The expanded
search field keeps its single-line 1.5428125rem corner radius as content wraps onto additional
lines, instead of changing to a different radius. Both workspace and global
ticket search also accept case-insensitive
`AND`, `OR`, and `NOT`, with parentheses and the conventional `NOT` → `AND` → `OR`
precedence. `is:` predicates cover Up Next, an unexpired active claim, open (`not_started` or
`started`), closed (`completed`, `verified`, or `archive`), structured duplicate close reasons,
and each named lifecycle state (including `backlogged`/`backlog` and `archived`). Expressions
are evaluated against the complete checkout rather than only the currently visible queue, so
an explicit archived or backlogged predicate can find those rows. Queue, Backlog, and Archive
switch from a content-sized view selector in the page
heading without a redundant separator above it. Selecting one ticket pushes the shared inspector
into the rail. Its larger blue Back action is vertically centered with the independently centered
ticket slug and pops to the stable list through the shared `ContentTransition`. Terminal sessions
and the dashboard grid remain mounted throughout rail navigation, visibility changes, and project
switching.
Every dashboard tile presents a read-only terminal card, while only visible and near-visible
cards progressively mount xterm runtimes in bounded batches. Offscreen cards keep their
lightweight text placeholder until intersection observation reaches them. Entering the dashboard
therefore paints the complete card layout before terminal initialization, and leaving it queues
detached runtime disposal in bounded post-paint batches rather than blocking project navigation.
This progressive boundary supports dozens of sessions without creating dozens of xterms or
WebSockets at once; dedicated and magnified interactive terminals still mount immediately.
Each mounted dashboard xterm uses an exact 80×24 character grid at a
stable 1280×768 natural geometry. The resulting 5:3 invariant belongs only to the black PTY
viewport: the surrounding card adds the measured spacing-token inset and footer height outside
that viewport, without another outer border. One canonical font geometry is established when the 80×24 xterm
is constructed, then a single uniform physical scale fits it to the available preview without
changing rows, columns, or glyph proportions. Magnified interactive terminals first fit their font
metrics and row/column count to the available modal frame. Magnifying a grid tile therefore
leaves the grid-only 80×24 contract and becomes an available-space interactive terminal. While
that terminal is magnified, its matching grid card replaces the live preview with a solid
terminal-background placeholder and disposes the preview viewer. Dismissing the magnified view
remounts the grid preview at 80×24. This keeps exactly one local sizing claimant for that terminal
through the transition instead of letting the preview and magnified viewport resize the PTY back
and forth.
Dedicated project-drawer terminals are likewise fitted to their actual interactive viewport
and reserve one physical containment row;
server size echoes cannot restore the edge row that would otherwise be clipped. Abrupt drawer
changes such as maximize explicitly publish a post-layout resize boundary on the next animation
frame, with a settled follow-up, instead of depending on an observer that can remain one resize
behind the container. Fixed-grid preview surfaces reveal after one physical-scale pass. The preview
and its inset frame use the terminal background token, so unused space
cannot expose an unrelated gray surface. The computed tile height derives the 5:3 preview
from the card width, then adds the tokenized frame/footer chrome, so repeated viewport changes
cannot push the terminal outside its card. Dashboard
previews never accept terminal input. Click opens and focuses a separate interactive viewport
centered over a full-browser dimming layer; click-away restores the grid. Its footer exposes
an external-open action, and both that action and a footer double-click open the terminal in
its project's maximized drawer. A grid-tile double-click does the same, while right-click
exposes shared Open/Hide menu items. A Lucide ellipsis in the shared grid/magnified card footer
opens that exact same menu from the keyboard or pointer. The project drawer's grid (including its
magnified tile) renders the same menu with **Open** only, because terminal visibility is scoped to
the workspace dashboard and the drawer never applies it; only the grid that shows the targeted tile
renders the menu (HS2-V2CCN6). The dedicated drawer consumer re-fits after both
the immediate and settled layout passes, avoiding clipped cells and cross-surface resize races.
While that magnified viewport is open, its containing workspace is promoted above adjacent
shell regions, both side-region separators are suppressed, and the workspace suppresses its own
focus presentation immediately, so neither shell chrome
nor a transitioning focus outline can paint over the modal. On the first replay payload for a
fixed 80×24 dashboard consumer, the client removes only zsh's exact reverse-video partial-line
`%` marker when it leads the bounded replay. Ordinary percent signs, later output, and the
dedicated drawer stream are preserved unchanged.
When an attach WebSocket reconnects, the first server replay is authoritative: the client sends
the reset and replacement bytes through xterm in one parser write instead of clearing the visible
emulator while the replay is still in flight or appending the same bounded transcript again. An
empty replay remains an explicit binary attach boundary. This prevents a blank flicker, repeated
recent output, and stale ANSI modes from the disconnected stream leaking into replayed text
(HS2-0V2DYR, HS2-BQR774).
On a mobile layout, keyboard focus in the active dedicated drawer terminal promotes that
terminal to a fixed, chrome-free focus surface with only an accessible Exit action. Its bounds
follow `VisualViewport` offset and size changes rather than the layout viewport, so Mobile Safari's
virtual keyboard shrinks and repositions the terminal instead of covering its bottom rows. The whole
viewport behind the focused terminal is blacked out with the terminal background, so the app never
shows in the gap around it — e.g. below the terminal and above the iOS keyboard (HS2-JQPRXV). Exit
restores focus to the selected drawer tab; a later terminal focus may re-enter. A new terminal's
initial auto-focus retries stop once that focus has landed, so tapping Exit right after opening a
terminal cannot be undone by a pending retry reclaiming focus (HS2-Y9VK3C). Drawer hiding,
tab/project replacement, and crossing to desktop invalidate stale focus-mode ownership
(HS2-GMTQZM).
The browser regression follows the complete user path with a newly created terminal: enter
Nano, resize the drawer up and down, abruptly maximize, move to the dashboard grid, magnify
and dismiss, then double-click back into the drawer. Every boundary asserts the current
claimed/grid geometry and visible xterm-screen containment. It samples the post-paint frames
through maximize, the fixed-grid preview, and the fitted magnified surface, so a stale or malformed intermediate frame cannot
pass on a correct final state alone. Returning from the dashboard to an already-open drawer is
idempotent and settles geometry without replaying the drawer's show animation.
The UX catalog mounts the same xterm frontend over deterministic ANSI fixtures rather than
substituting a text placeholder. Its preview is constrained to a realistic grid-card width,
the magnified variant receives the remaining stage width; the preview demonstrates the canonical
80×24 grid while the magnified terminal fits that available stage. Its initial terminal focus is one-shot: later
xterm paints, including cursor blinking, never steal focus from catalog controls or close an
open Web Awesome popup.
HS2-PD4MZ9 replaced its snapshot-only panes with xterm-backed interactive
viewports over the existing terminal attach WebSocket. HS2-586BVQ ships the project-only
bottom drawer over that same viewport boundary.

When a launch restores remembered projects, the client holds a single project-restoration
surface until parallel project registration, the active project's tickets and workspace session,
visible terminal resources, and each failed project's bounded retry have settled. It then reveals the complete healthy and error-tab
set in one render boundary and restores the remembered active healthy or failed tab. A refresh
therefore never exposes a partially restored board beside terminal content from a different
stage of startup.

The terminal service is host-wide, while drawers are project-specific. The client assigns
each terminal to the most-specific open project root containing its reported working
directory before it builds drawer tabs or the workspace grid. A terminal therefore appears
once in the global grid and only in its owning project's drawer, including when several open
projects share the same server process. The terminal service reports the launch request's
working directory immediately, before a shell emits OSC 7, and replaces it when a later OSC 7
update arrives. Short-lived configured commands and shells without OSC integration therefore
remain attributable to the correct project from their first response.

The project terminal drawer occupies only the center AppShell column, leaving the project
sidebar and ticket inspector at full height. It belongs to ticket views only: the
Notifications and Settings views hide the drawer entirely — neither the drawer nor its
"Show terminal drawer" restore affordance appears while either is open — and the user's
open/closed drawer preference is preserved so it returns unchanged on the next ticket view
(HS2-EQEJC7). Kerf's `fade-slide` collapse snaps the drawer track in one reflow while moving
and fading its fixed-size content on the compositor. Its 40 px restore action uses the
region's safe-area-aware bottom-end placement (HS2-4Y6SM9). Its compact rail switches between the decorated
grid, one undecorated interactive xterm session, or one embedded AI conversation that fills
the content area. While switching directly among terminal tabs, the drawer keeps every
dedicated session element that belongs to that presentation mounted and hides the inactive
ones. Returning to a terminal therefore preserves its xterm, WebSocket, fitted grid, scroll
position, and input state instead of constructing a fresh 80×24 xterm (HS2-V93PYF). Its grid tab
never shrinks when terminal tabs consume the available width. The shared Kerf tab strip sizes to
its tabs until the rail is exhausted, then scrolls horizontally; its growing trailing slot keeps
the explicit quiet pill-shaped plus action immediately after the last tab and the drawer action
at the far edge. A newly created selected tab is revealed without stealing the dedicated xterm's
one-shot input focus; plus
opens a direct shared-menu choice of Default shell, AI shell, or AI chat. The menu has no
redundant heading, and leaf actions do not display submenu chevrons. The rail and terminal inset
use Kerf's canonical 8 px within-group rhythm, the tab-strip focus gutter and icon-label clusters
use 4 px, and connected create-menu rows use no extra gap (HS2-4Y6SM9). Option/Alt on either AI choice prompts for a
plugin-discovered provider, model, and compatible effort; AI shells use the real plugin-backed
`connect` launch path rather than treating the provider id as a shell command. A dedicated
xterm viewport receives focus as soon as it
mounts, allowing immediate typing without an extra click; this is a one-shot request that
does not make later refreshes steal focus. Opening the drawer also focuses its remembered
terminal or writable AI-chat composer, and explicitly activating any live drawer tab returns
focus to that surface, including an already-mounted terminal, unless the user moves focus
elsewhere before the deferred render settles. Project and terminal tabs share one pill-tab primitive, with
the close button before the label and optional leading/trailing state icons. Right-clicking
project, terminal, and AI-chat tabs offer Close Tab, Close Other Tabs, directional close,
and Close All Tabs. Drawer close ranges use the complete remembered mixed order, so each
action closes both terminal and AI-chat targets; terminal tabs additionally offer Rename.
Human-readable defaults replace generated
ids, and device-local rename overrides survive refresh/reopen without renaming the PTY
identity. Project tabs reorder among projects; terminal and AI-chat tabs reorder together in
one mixed drawer strip by dragging across either kind. Both strips use Kerf's controlled
`TabBar`/`AppTab` composition and one `wireTabBars` delegation. Both use manual activation:
Left/Right/Home/End move focus without replacing the focused controlled tab node, and
Enter/Space explicitly selects the project or live drawer surface. Project order is stored with the
open-project roots and restored without changing the remembered active project. Each
project's mixed drawer order is stored device-locally and remains stable across refresh and
drawer reopen. Holding Option/Alt when opening the menu changes the directional action to the
left. Alt+Shift+Left/Right reorders the focused drawer tab without losing focus. Deferred
post-reorder focus restoration yields when the user has already focused a different control,
so a pointer reorder cannot steal the following keyboard shortcut. Closing a
selected terminal or chat chooses the nearest remaining tab to its right, then left, across
both kinds before falling back to the grid.
Closing a terminal is idempotent: if its process has stopped and the terminal host has
already reaped it, Close Tab still removes the stale client tab instead of surfacing a
not-found failure (HS2-DPTG65).
Closeable tabs reserve the same trailing state slot even when it is empty, balancing the
leading close control and preventing labels from shifting when status appears. Terminal tabs
use that shared tab surface directly rather than layering a second selected background inside
it. Every selected and unselected tab is reachable in sequential Tab order, while
Left/Right/Home/End traverse the current tablist and Delete/Backspace closes the focused
closeable tab. The segmented dashboard and view controls likewise expose each choice in Tab
order. The horizontally scrolling tab strips reserve a canonical 4 px inset on every edge so pill shadows
and focus rings remain complete at either end. The project strip places the dashboard-mode
buttons in `TabBar`'s leading slot and the add-project action in its trailing slot. The drawer
strip keeps its Project grid tab sticky at the start of the shared horizontal scroller and
places create/hide actions in the trailing slot. A dedicated terminal uses a canonical 8 px inset and shares
one domain background token between its session container, viewport, and xterm theme, so
the edge area is symmetric and visually continuous with the terminal canvas.
Double-clicking the rail, grid tab, or any terminal tab toggles drawer maximization while
preserving the last manual height. Closing the selected terminal follows HS1's
nearest-neighbor behavior (right first, then
left, then the grid). The rail deliberately omits global visibility/group controls and collapses
to one bottom-end Kerf floating toolbar containing the restore action. Its Kerf restore wrapper is
positioned against the center main column rather than the whole viewport, so an open inspector does
not displace the action into inspector content (HS2-3ZGWMN). The same Kerf floating-toolbar
composition owns the bottom-end zoom pair in both the global workspace grid and the drawer's Project
grid, while the application continues to own visibility, actions, and safe-area-adjusted positioning
(HS2-W3GPHW). Dedicated sessions use xterm's WebGL renderer by default on
non-Apple engines, fall back when WebGL is unavailable, and deliberately use xterm's DOM
renderer on Apple WebKit as a conservative compatibility policy. They refit only after a drawer resize gesture settles. While
the splitter is held, neither dedicated xterms nor grid-tile geometry is recomputed and no
intermediate PTY size claims are sent; this avoids the old debounce behavior that still fired
during a slow drag.
Their library-owned DOM is protected from application morphs, so committing a resize keeps
the same xterm instance and WebSocket instead of reconnecting the shell.
Global project/terminal jump actions open this drawer on the matching project and terminal.
Drawer visibility, height, selected terminal per project, and independent short-container
zoom are device-local. Switching projects applies the destination project identity and its
remembered drawer surface atomically, so a dedicated terminal never passes through a transient
project-grid render or emits an unintended 80×24 sizing claim. Terminal metadata refreshes do
not activate a grid; PTY resizing remains lazy and follows only the surface actually presented.
Drawer scale always uses the 1–3 short-container model. Level 1 makes
one 160px-minimum row fit the available drawer height and flows additional terminals
horizontally. Levels 2 and 3 instead mean columns across the available width (treating widths
below 240px as 240px), wrap left-to-right into additional rows, and scroll vertically. Its
accessible vertical splitter persists heights from 228px
through the live boundary immediately below `PageHeader`, so it can consume the full
ticket work area on taller windows instead of stopping at the former 520px cap. At the minimum,
continued shrink resists for a 48px overshoot; releasing within that range keeps the 228px
drawer, while persisting beyond it is treated as an intentional collapse. Grid zoom is not
shown for a dedicated full-size terminal.
Double-clicking non-interactive space in the drawer rail toggles that measured maximum
and the last manually resized height (or 320px before the first resize); tab and action
buttons do not trigger the toggle. Maximizing is temporary and does not overwrite the
persisted restore height. Explicit hide/show changes animate the drawer's height and
content for 200ms (and honor reduced motion); manual pointer/keyboard resizing and rail
maximize/restore remain immediate. The terminal content stays mounted only through the
closing transition, then releases its viewport resources. Magnification remains a
temporary viewer over the same PTY.

## 6.7 Terminal display & multi-viewer PTY sizing

> **Status: the server arbiter and browser viewport are built (HS2-BD7Q74,
> HS2-PD4MZ9).** The cross-device
> generalization of HS1's terminal "borrow-stack" (docs/54), which worked locally but was
> never designed for remotes. `hotsheet-terminals::SizeArbiter` implements the model below —
> leased viewport claims, focus-follows (default) + smallest/largest/pinned, the
> `SIZE_FOCUS_HOLD`/`MIN_DELTA`/`RESIZE_MIN_INTERVAL` guards, and disconnect self-heal — wired
> into the WS attach (Text `{resize}` claims in, `{pty_size, driven_by}` decisions out). The
> client consumes those decisions through the credential-hiding local bridge described
> below.

Each rendered pane owns a stable random viewer id and one WebSocket attachment to the
existing PTY; opening another pane or magnifying the terminal attaches another viewer and
never calls terminal creation. The browser URL is same-origin and secret-free. The local
Vite bridge performs the upgrade and adds the loopback server credential only to its
server-side upstream. Vite loads that bridge from its config graph and the project-open
HTTP handler from its SSR graph, so both resolve authenticated project sessions through
one process-scoped registry; a successful project open must make terminal upgrades
immediately attachable rather than leaving the viewport indefinitely connecting. A future
Tauri host must provide the same bridge boundary rather
than exposing the server secret to web content.

The dashboard's metadata refresh reads only the terminal list. It does not fetch and retain
a second full REST scrollback snapshot for every tile: each xterm's attach WebSocket is the
single source for live output and its initial replay. The PTY host captures that replay and
subscribes to future output under one lock, so output racing an attach cannot be duplicated or
lost. If a viewer falls behind, both direct-server and detached-broker paths send an explicit
`{"terminal_replay":"replace"}` control before the atomic replacement snapshot; the browser
submits the emulator reset and snapshot as one parser write, then continues from the paired fresh live receiver
(HS2-5W0V9M). The PTY host trims its byte-capped replay only at ANSI-ground and UTF-8 character
boundaries, so eviction cannot expose a control sequence's parameter tail as ordinary terminal
text (HS2-BQR774). Read-only grid previews keep no xterm
history, temporary magnified dashboard viewers keep 1,000 lines, and dedicated interactive
terminals retain the full 5,000-line client history. Disposing a viewport cancels its frames,
timers, observers, xterm subscriptions/addons, and socket; repeated magnify/dismiss cycles are
covered as a stable-resource lifecycle rather than allowing detached viewers to accumulate.

The viewport renders ANSI/VT output with xterm, forwards typed input as terminal text, and
sends `{viewer_id, cols, rows, focus, visible}` claims on connection, geometry/focus/
visibility changes, and a five-second lease heartbeat. The heartbeat renews server state;
it is not request polling. Scaled dashboard previews observe their untransformed viewport
frame for visibility, so CSS scaling cannot turn an onscreen terminal into a false hidden
sizing claim. Disconnects retry with bounded exponential backoff, while
dispose closes the socket so the server removes that viewer and self-heals its chosen size.
Client claims normalize transient non-finite geometry to bounded integer dimensions. The server
drops any JSON frame carrying a malformed `resize` member instead of forwarding that protocol
text to the PTY, so a hidden or transitioning viewport cannot echo control JSON into the shell.

Interactive magnified and dedicated terminals register an xterm link provider for the same
ticket-reference grammar used by rendered ticket details: current slugs, retained `HS-N`
legacy numbers, and explicit `@project-id/SLUG` references. Activation uses the owning
terminal project as the preference for an unqualified match, then opens the existing stacked
ticket reader or its not-found/ambiguity feedback. The provider reads xterm's parsed buffer
and ranges rather than terminal DOM or HTML, so ANSI styling, wrapped rows, WebGL rendering,
selection, focus, and normal input remain intact. Dragging to select text within a ticket
reference does not activate the link or reopen a dismissed reader; after clearing the
selection, ordinary clicks activate references again (HS2-H6ZXNM). Scaled dashboard previews deliberately do
not register the provider. Magnified desktop terminals fit their real xterm row/column grid to
the available frame, keeping glyph proportions, pointer hit-testing, selection, and visible cells
aligned with the input surface
(HS2-2DW829).

Renderer choice follows the proven HS1 split rather than forcing one backend everywhere.
Full-size dedicated drawer terminals use xterm's WebGL addon on non-Apple engines (with DOM
fallback after load failure or context loss). Apple WebKit uses the DOM renderer as a
conservative compatibility policy; renderer selection alone is not proof of painted glyphs. The fixed 80×24 dashboard grid and magnified surfaces use xterm's
DOM renderer. Read-only previews are uniformly CSS-scaled, while interactive magnified
surfaces fit their row/column grid to available space; scaling a WebGL raster makes
the terminal blurry and can produce misleading intermediate canvas geometry. Retina browser
coverage therefore checks the dedicated WebGL canvas backing-store size separately from the
scaled DOM surfaces instead of treating `.xterm-screen` bounds as proof of a completed paint.

Terminal viewers must initialize on ordinary LAN HTTP origins as well as HTTPS/localhost.
Viewer identities use native `crypto.randomUUID()` when exposed, otherwise a UUID built with
`crypto.getRandomValues()`; only environments without Crypto use a time/random/counter identity.
Each mounted viewer has a distinct identity, retained through reconnect. `randomUUID()` is
[secure-context-only](https://www.w3.org/TR/WebCryptoAPI/#crypto-interface), while
`getRandomValues()` remains available on LAN HTTP. Calling the former unconditionally reproduced
black surfaces on ordinary LAN HTTP before xterm, renderer selection, or the socket
started (HS2-3ZBQDG). This reproduces the reported appearance in actual WebKit; confirmation
on the maintainer’s exact physical-device origin remains a manual check. The previous renderer policy did not address that startup failure.
Lazy-module or runtime initialization failure now renders a readable terminal-local alert
instead of leaving an empty black surface. Disposed pending mounts never initialize. Partial
setup failures dispose allocated xterm, observers, listeners and scheduled work; a fresh mount
removes its owned error alert and restores the read-only preview accessibility state.
Real WebKit coverage uses a non-loopback HTTP origin, real PTY replay/live bytes, and painted
light glyph pixels against the dark row background across preview, magnified, dedicated,
resize, and reconnect paths. Actual device rotation/background checks remain in the manual
plan.

The same shared browser identity helper covers chat connections and turns, saved-chat
restoration, linked ticket readers, attachment drafts/batches/annotations, workspace visibility
groups, provider transfer operations, and the attachment metadata demo (HS2-76ZR5P). These
flows must work without `randomUUID` on ordinary LAN HTTP. A rename or reuse retains the
existing object/batch identity; a new object, drawing, transfer invocation, or attachment
batch gets a fresh identity, including after cancellation, deletion or failed execution.
Later eligible upload gestures reuse the latest human batch until a durable status boundary;
separate new tickets receive independent attachment batches.
Node-side migration/export temporary files continue using Node's native crypto API.
Consumer regressions pair isolated transitions with actual insecure-origin WebKit flows and
real ticket/attachment persistence; AI execution and native archive selection remain fixture
boundaries in that browser test.

A visible mounted fixed 80×24 surface is an active sizing claimant even though its grid preview is
read-only and never accepts keyboard input. This ensures entering the dashboard actually
resizes the PTY to the promised 80×24 contract rather than merely drawing an 80×24 xterm over
output that the TUI emitted for the drawer's previous size. Conversely, offscreen dashboard
cards do not mount a viewport or open a socket until intersection observation reaches them, so
an unpainted or hidden fixed-grid card cannot take sizing control. The matching preview is also
temporarily unmounted while its magnified interactive viewport owns the terminal, then restored
after dismissal. Interactive magnified and dedicated terminals claim their fitted dimensions
whenever they are visible, independently of which app control owns keyboard focus. Finishing a
drawer drag or maximize publishes its final claim; clicking or focusing another control cannot
revert the terminal to a stale 80×24 size.

### 6.7.1 The fundamental constraint

A PTY has **exactly one size** (cols × rows) at any instant. Resizing it sends
`SIGWINCH`, and the program inside (claude, vim, a TUI) **reflows to that size** —
so resizing is disruptive and must be rare and deliberate. Meanwhile many
**viewports** may show the same terminal at once, each a different size:

- several views on **one** device (the drawer terminal, a dashboard tile, a
  magnified view — HS1's borrow-stack case), **and**
- views on **different** devices at once (a macOS window _and_ an iPhone).

You cannot give each viewport its own native size of the _same_ session: a single
PTY emits one size's worth of output, and an alternate-screen TUI was drawn for one
grid — it can't be losslessly re-flowed to another (only line-wrapped scrollback
can). So the model is **one arbitrated PTY size + graceful handling in every other
viewport** — the same reality tmux lives with. (If per-viewer native size is ever
truly needed, that's a _separate PTY per viewer_ — a different shell, not this
shared session — see §6.7.5.)

### 6.7.2 The model: the server arbitrates, viewports make _claims_

The server owns the PTY, so it is the single arbiter of its size — matching the
"server is authoritative" principle and, crucially, giving **one** coordination
point for local _and_ remote viewers. Each viewport registers a **size claim** over
the terminal WebSocket and keeps it alive with a heartbeat:

```
viewer → server:  { viewerId, cols, rows, focus: bool, visible: bool, interacting: bool }
server → viewers: { ptySize: {cols, rows}, drivenBy: viewerId }   // broadcast on change
```

- `interacting` distinguishes a **genuine user interaction** (a tap/click, a focus gain, or a
  keystroke) from the steady heartbeat/geometry claim every viewport streams. Only an
  interacting claim advances the server's per-viewport interaction recency; a plain heartbeat
  keeps the prior value. Without this, two sizing-eligible devices' interleaved 5-second heartbeats
  would ping-pong "most recent focus" and thrash the PTY size, leaving the device the user
  isn't touching (e.g. a phone) rendering the other device's size (**HS2-3ZBQDG**).
- The protocol retains the historical `focus` field name, but the web client uses it as
  **sizing eligibility**. Visible grid previews and visible interactive terminals are eligible;
  keyboard focus only controls input and never changes a terminal's local geometry
  (**HS2-KKE1PM**).

- `viewerId` is **per viewport, not per device** (`<clientId>:<paneId>`), so
  intra-device and cross-device viewports arbitrate uniformly — this _is_ the
  borrow-stack, generalized to every viewport everywhere.
- Claims are **leased** (reusing the claim/lease pattern, [05](05-ai-tool-plugins.md)
  §5.7): a viewport heartbeats; on disconnect (a phone that drops off Wi-Fi) its
  claim **expires** and the server recomputes size from the survivors — so a gone
  viewer never pins the PTY to its size forever. This is the piece HS1 never had.
- The server broadcasts the resulting `ptySize` to **all** viewers, so everyone
  agrees on the real size and each renders within its own viewport (§6.7.4).
- A client treats a repeated `ptySize` that matches its current local grid as metadata,
  not a resize request. A genuine local-grid change keeps a marker on the reader's visible
  top line and restores that line after xterm reflows. Animated command output and steady
  size heartbeats therefore cannot pull a user who scrolled into history back to the live
  cursor (**HS2-CJBZPW**).

### 6.7.3 The sizing policy: interaction-follows, with hysteresis

Default policy (= tmux `window-size latest`, which is exactly the maintainer's ask —
"right-sized based on whichever device and view area had most recent focus"):

- **The PTY follows the size of the viewport the user most recently _interacted_
  with.** When interaction moves from the big macOS pane to the small iPhone
  view, the PTY resizes to the iPhone (after the guards below); when it returns, it
  resizes back. The interaction-recency tiebreak (advanced only by `interacting`
  claims — see §6.7.2) decides between two viewports that both believe they're focused,
  so one device's background heartbeats can never steal control (**HS2-3ZBQDG**).
- Activating the read-only dashboard counts each visible fixed 80×24 tile as that
  PTY's local sizing claimant. A grid tile cannot accept keyboard input, but entering
  the terminal-specific surface is still a deliberate request to render its TUI at
  the grid contract rather than at an obsolete hidden-drawer size.
- **An actively-used viewport's size is locked in** — a background device
  cannot resize the PTY out from under someone mid-keystroke. To change the size,
  interact with its terminal (which transfers the size).
- **When no eligible viewport remains, hold the current size** (don't resize on mere
  visibility changes) — glancing at a terminal from a second device must not reflow
  it.

**Anti-thrash guards** (named so implementation has targets; tune later):

- `SIZE_FOCUS_HOLD_MS` (~500 ms, retaining the protocol's historical name) — a newly
  eligible viewport must remain eligible this long before its size is applied.
- `SIZE_MIN_DELTA` (≥2 cols/rows) — ignore sub-threshold differences.
- `SIZE_RESIZE_MIN_INTERVAL_MS` (~100 ms) — rate-limit actual PTY resizes to ten per
  second. Browser viewports coalesce layout work to animation frames and send a final
  claim 120 ms after resizing settles, so a suppressed in-window update cannot leave the
  terminal at an obsolete size until its heartbeat.

**Alternative policies (configurable per terminal), for when interaction-follows isn't
wanted:**

- `smallest` — size to the smallest _visible_ viewport so everyone sees the whole
  screen without scroll (tmux's default; good for "we're both watching").
- `largest-visible` — one big screen drives; small screens observe (scroll/scale).
- `pinned` — a fixed size the user sets; all viewports letterbox/scroll. Good for
  recording or maximum stability.

Recommend **`focus` as the default** (it's the described need) with the guards
above, and expose the alternatives as a per-terminal setting.

### 6.7.4 Rendering when a viewport ≠ the PTY size

Every interactive viewport keeps its xterm fitted to the space it occupies, including while
keyboard focus is in a toolbar, editor, or another app control. A broadcast naming another
driver records the PTY's authoritative dimensions but does not replace the local fitted grid or
show a misleading focus-to-resize badge. Genuine interaction with that terminal advances its
recency claim and lets the server move the shared PTY to those fitted dimensions. Fixed 80×24
rendering and physical scaling belong only to workspace and drawer grid tiles.

**Mobile 80×M (HS2-Z84F78, HS2-S708S3).** On a phone-width viewport (`isMobileViewport`, the same
1024px breakpoint as the single-column layout) a magnified or dedicated interactive terminal keeps a
fixed column count — the canonical **80 columns** by default, so line wrapping matches every other
device — and chooses **M rows to fill the available height**, scaling the whole grid to fit the
phone width. The text ends up small (80 columns on a ~390px screen), which is the deliberate
default trade for consistent width; the phone text-size control below trades columns for larger
text.
The magnified/full-screen terminal drops its 5:3 aspect and fills the screen. The dedicated
drawer terminal applies the same fixed-80-column DOM render path within the drawer's available
height. Both resize the PTY to 80×M, width-fit the physical grid, and reclaim sizing with the
final measured row count rather than the provisional 80×24 size. The xterm layout root expands by
the inverse of that physical scale, keeping its vertical scrollbar on the visible right edge instead
of stranding it inside the terminal (HS2-QBMVFQ). Read-only grid preview tiles
deliberately remain uniform 80×24, 5:3 cards: they are glanceable non-input surfaces, not the
phone's interactive terminal.

**Phone magnified terminal chrome (HS2-WMN626).** On a phone the magnified terminal overlay is
positioned from the live `VisualViewport` (offset and size), so presenting the virtual keyboard
shrinks it — and its M rows — to the visible area above the keyboard instead of leaving the
terminal behind it. It is full-bleed on the terminal background color (no inset ring or dimmed
scrim), and a full-viewport backdrop in that color blacks out the app behind it, so nothing shows in
a gap between the shrunken terminal and the keyboard. Unlike the desktop scrim, tapping the overlay
outside the terminal never dismisses it; Close does (HS2-SB1FSQ). It pads for the device safe areas (dropping the bottom inset while the keyboard covers it),
and clips rather than scrolls its scaled xterm root so a focus scroll or touch cannot pan the
terminal sideways off screen. The card footer becomes a **top toolbar** carrying **Close**
(Lucide `x`), the terminal identity, a **text-size** button (Lucide `a-large-small`; the
column count is not shown inline — a toast reports the new size on change, HS2-89JZSN), the
actions menu, and open-in-drawer. The toolbar is shown only while the
keyboard is hidden; keyboard visibility is inferred when the unscaled visual viewport is more than
120px shorter than the layout viewport (pinch zoom is not mistaken for a keyboard). Text size
cycles **80 → 70 → 60 → 50 → 40 → 80** columns; the choice is persisted per browser
(`hotsheet.terminals.mobile-columns`), applies to every phone-width 80×M terminal (magnified and
dedicated drawer), and immediately refits the grid and PTY claim to the new *columns*×M. Wider
viewports never render this chrome. The phone **dedicated drawer** terminal's focus mode carries
the same text-size control (`a-large-small`) at the top-left, mirroring the top-right Exit pill and
hidden while the keyboard is presented, so the size is adjustable there too, not only from a
magnified terminal (HS2-ZSFAHF).

### 6.7.5 Escape hatch: a per-viewer _separate_ terminal

When someone genuinely needs a natively-sized terminal on each device
simultaneously, that's **not** one shared session — it's **separate PTYs** (the
multi-terminal model, HS1 §22.17). Each is its own shell/program at its own size,
no arbitration needed. Hot Sheet supports both: _share this terminal_ (arbitrated,
this section) vs _open my own terminal_ (independent). The arbitration only governs
the shared case.

### 6.7.6 Why this beats HS1

HS1's protocol (§22.9) let any client send `{resize, cols, rows}` and took
"max-of-attached or last-resized" — an implicit, race-prone consensus with **no
notion of focus, no leases, and no remote testing**. A small remote either lost to
a bigger local viewport or won by a last-write race and shrank the desktop
unexpectedly, and a dropped remote left a stale size. Moving to **server-arbitrated,
leased, focus-follows** claims fixes all three: intent (focus) drives size,
disconnects self-heal, and one arbiter means local and remote behave identically.

### 6.7.7 Per-terminal shell history

Interactive bash, zsh, and fish sessions default to machine-local history isolated by
checkout/project and terminal id. Two terminals therefore do not silently share recall,
and one project's commands do not enter another project's history. A local-only terminal
preference may explicitly inherit the user's normal global shell history when that is
more useful. The terminal host owns shell-specific environment wiring and persistence;
no history path or command content is committed to a ticket store. The built host hashes
the checkout path and terminal id into stable machine-local identities: bash and zsh use
separate `HISTFILE` paths under the Hot Sheet home, while fish uses a separate durable
`fish_history` session name. Project Settings → Terminals exposes the local-only **Use my
global shell history** opt-out; it affects newly created terminals and persists as
`terminal.inherit_global_shell_history` in
`<project-root>/.hotsheet2/settings.local.json` (HS2-A5V801).

## 6.8 Notes, reader mode & editing

> **Web implementation shipped** (HS2-F3SS63). Keep HS1's reader mode + feedback
> concepts, but unify and enlarge them. Native-client parity and the local feedback-draft
> overlay/submission lifecycle remain separately tracked.

When the provider supports notes, the Notes section always presents a visible **Add
note** action—even when the ticket has no existing notes. Creating the first note must
not depend on recognizing an icon-only section-header shortcut. Activating it appends
the focused composer after the existing note list, beside the bottom action that opened it.

**Five note kinds, one rendering rule.** A note's `kind` ([02](02-ticket-storage.md)
§2.6 — `regular` / `activity` / `feedback_needed` / `feedback_draft` / `status`) determines how
it's shown, **not how the view was opened** (HS1's inconsistency: the same note
rendered as an editable feedback form when opened via "Provide feedback" but
read-only when opened via the reader icon). In HS2 there is **one reader mode**, and:

Note cards use the canonical spacing ladder: a 16px card inset, 8px between ordinary
card regions, and 4px inside connected icon/metadata and inline-feedback clusters. The
quieter Activity variant uses an 8px block / 16px inline inset with 4px between its
regions. Fixed action targets, choice indicators, and glyphs remain explicit geometry
(HS2-4Y6SM9).

- **`feedback_needed` and `feedback_draft` notes render in the feedback-editor style in
  reader mode** (you can answer the ask / continue your draft). A new response starts
  empty; a saved draft is prefilled.
- **Explicitly editing an existing note is a different action from responding.** Direct
  editing is prefilled with that note's complete Markdown—including for
  `feedback_needed`—and autosaves the replacement by note id.
- **Feedback prompts support user-chosen inline reply points.** In reader mode, the
  client maps a click in the rendered Markdown to the corresponding source-character
  boundary and inserts a focused reply field at that exact point. Enter/Space on the
  keyboard-accessible prompt adds a reply at the end of the focused segment. Each reply
  field has a remove action that deletes the field and rejoins its surrounding prompt
  segments. On submission, the complete prompt is emitted as
  Markdown blockquotes with non-empty replies interleaved at the selected points; an
  optional general response follows at the end. If the user only enters general
  feedback, the response remains plain Markdown without redundantly quoting the prompt.
  The optional catchall response starts at half the ordinary note-editor minimum height
  so it does not dominate the feedback prompt, while remaining vertically resizable.
  A compact, text-only secondary **No response needed** action appends that exact text
  as a regular note and closes the feedback exchange without mutating or deleting the
  ask. It uses an outlined button rather than an ambiguous acknowledgement icon. Clients
  keep it in the same action row, height, and centerline as the adjacent Respond action,
  and render the exact acknowledgement as a subtle ordinary note rather than another
  warning surface.
- **Feedback prompts may offer explicit choices.** An uppercase `CHOICE` or `CHOICE:`
  line immediately followed by a Markdown list becomes a set of rounded selection
  controls. A normal click selects one option or clears the sole selection; Command on
  macOS and Ctrl elsewhere toggles additional options, while Shift selects a range.
  Zero or multiple selections are always valid. Selected controls use a green border,
  quiet green fill, and checkmark. Option content uses the same Markdown and attachment
  reference rendering as notes, including image references. Submission records the
  selected option text in the regular response note and may include the existing
  freeform and inline responses; the choice list never makes an answer mandatory.
- **`regular` and `status` notes, and the ticket `details`, use the same direct editing
  affordances in the reader as they do in the inspector.** Rendered Markdown owns its
  own whitespace; note containers must not preserve the renderer's HTML formatting
  whitespace, which would create artificial blank lines between list items.
- **`activity` notes render in both Notes and the chronological Timeline** ordered by
  `created_at` (ULID tie-breaker). Notes retains the complete Markdown record with a
  quieter, unfilled, smaller Activity presentation so regular notes retain primary
  reading emphasis; Timeline is a compact index containing only the
  optional durable `summary` headline. It never repeats the full body or a subtitle.
  New AI-authored activity notes supply a plain-text, one-line, outcome-oriented
  `note_summary` in the same write. Legacy notes and providers without structured
  summary metadata use a Markdown-stripped, word-boundary-truncated first-line fallback;
  clients never invoke AI while rendering or opening a ticket. Never collapse duplicate,
  repeated, or reversed transitions;
  each entry is historical context. Every actual status change appends one of these
  durable activity notes. For tickets created before transition recording, clients also
  show the lifecycle timestamps the ticket still carries (`created_at`, `completed_at`,
  and `verified_at`) so the timeline is never blank. Show `edited_at` when it differs
  from creation. Render status-transition entries as the concise destination label
  as past-tense actions (`Started`, `Completed`, `Moved to backlog`, `Moved out of
backlog`, `Re-enqueued`) while retaining the full durable note text and using the
  source state where it changes the action's meaning.
  Rich native tool events and distilled background/subtask milestones remain tracked by
  HS2-SW655F and HS2-3GRNZW respectively.

AI-authored artifacts use the shared `AIContentLabel`; attribution is persistent rather
than hover-only and is repeated in the containing response, narration item, or note's
accessible name. Narration and distilled summaries explicitly say that they may contain
errors. Thumbs feedback appears only when the composition has a selected ticket whose
provider accepts notes. The optional explanation and consequence-oriented rating are
then appended as an ordinary ticket note, so feedback is syncable and auditable rather
than trapped in browser state. Legacy notes without durable AI provenance are not
guessed from their prose or author-like display text.

**Feedback needed is needs review.** These are one user-facing concept, not competing
ticket states. A `feedback_needed` note, a description containing the case-sensitive
`FEEDBACK NEEDED` marker, and an explicit review request all project to
the same "Needs review" badge and purple leading rail in list and column presentations.
The sidebar inspector uses the "Needs review" banner without adding a redundant full-height
purple rail; the modal reader retains the rail and banner. The underlying note still carries
the specific question and feedback editor. The unified needs-review row/reader rail
takes precedence over blocked and Up Next rails so the outstanding decision is never
hidden. The server's compact row continues to expose the source `feedback_needed`
boolean (mirrored in the index), while the client normalizes it at presentation time.
In the sidebar inspector, the active feedback note or marked description has a full-width
**Respond to Feedback** action directly below its body. It is omitted from answered or
superseded asks and from reader mode itself. Activating it opens reader mode on Info,
scrolls the active source into view, and focuses its response editor. Reader descriptions
reuse the same Markdown choice selection, inline reply, optional general response, and
No response needed controls as feedback notes; submitting adds a regular response note.
Large reader text applies to active feedback prompts and choices as well as ordinary,
activity, and status notes, including their Markdown paragraphs, lists, code, and headings.
An active feedback description uses the same warning border/fill and content insets as a
feedback note in both inspector and reader. Reader mode also gives it the standard
circle-alert icon and "Feedback needed" heading so its meaning is explicit in context.
For note-driven feedback, only an unanswered ask is active: among regular and
`feedback_needed` notes, the most recent one controls the state. A later regular note is
the response and clears Needs review; the answered ask then uses the ordinary note
presentation rather than retaining feedback styling or an editor. Activity/status notes
are neutral, and a later `feedback_needed` note opens it again.
When a description opens the exchange, activity/status notes remain neutral and the first
regular note answers it. A later first-class feedback request supersedes the description.
For compatibility with HS1 and early HS2 automation, a regular note containing the
case-sensitive all-caps phrase `FEEDBACK NEEDED` is normalized to the `feedback_needed`
kind at the wire boundary and participates in the same exchange. The phrase may appear
after introductory context and its colon is optional; lowercase prose does not match.
The marker is ignored inside Markdown blockquote lines (those beginning with `>`), so a
reply that quotes the original request back — which the inline-reply composer produces —
is not mistaken for a new request and does not reopen the exchange (HS2-HG7FZ0); an
unquoted marker the author adds in the same reply still opens one.
New core writes promote the same marker to the first-class kind. Rebuilding an older
disposable index must therefore recompute the compact flag for unchanged ticket files.

**Reader mode is a directly editable focus surface.** Opening reader mode shows the
ticket's details + notes on one large scrollable surface with no separate top-level edit
mode. The reader uses the available browser height with exactly 24px of backdrop above
and below; it has no desktop-height cap that leaves unnecessary vertical space. The Web
Awesome dialog owns the overlay geometry; a mounted but closed reader host has no fixed
viewport box, so ordinary ticket selection cannot paint an inert reader surface over the
workspace. Details
and ordinary notes expose their normal edit interactions immediately.
The workspace reader remains the editable surface for its selected ticket. Ticket links
open read-only reader layers above it (or above the sidebar inspector) so each frame can
hold an exact project-qualified ticket without sharing the workspace's edit drafts or
selection. Linked readers use the same responsive inspector composition; their compact
project/depth header remains visible at wide and narrow widths while covered frames are
inert and hidden from the active accessibility tree.
Its **A Large Small** action toggles a user-global preference that renders every
Details and note-content size from its own ordinary semantic size, including paragraphs,
lists, headings, quotations, code, tables, activity text, and edit fields, at exactly 1.5×.
Its enabled state uses the shared pressed-toolbar treatment around the icon. It does not
enlarge the ticket title, tabs, note metadata, or other reader chrome. The preference is
remembered when the reader closes and applies the next time any ticket opens.
The sidebar inspector and reader each own and persist their selected tab for the project.
Changing one surface's tab never changes the other; reopening or restoring the project
returns each surface to its own last selection. Feedback-response entry intentionally
opens the reader's Info tab without disturbing the sidebar selection. The Info, Timeline,
Code Review, and Attachments panels use Kerf's controlled `TabBar`/`AppTab` composition in
the sidebar, reader, and terminal ticket rail. Exactly one tab is selected and keyboard
focusable; Arrow, Home, and End keys move focus and activate the corresponding panel, while
narrow inspectors retain the same accessible tab names when their visible labels collapse
to icons. Attachments keeps its visible and accessible count badge.
Leaving the details editor flushes its pending autosave and returns that surface to
preview. When a pointer action outside the editor causes that blur, the save begins
immediately but the editor remains mounted until the originating click has reached its
target; Add note, tab changes, and other controls therefore act on the first click.
Closing the reader performs the same save-and-exit transition, so its shared
editing state never leaks into the sidebar inspector; stale save completions cannot close
a newer editor generation or a different selected ticket.
While editing details/notes **in the detail panel**, the **reader button stays
available**; clicking it **launches directly into the larger reader/editing mode**,
carrying the in-progress edit — so you can escalate from the tight inline editor to
the roomy one without losing your place.

Net: one consistent reader mode, kind-driven rendering, and a smooth path from the
constrained detail-panel editor to a spacious full-surface editor.

## 6.9 Mutation feedback and reconciliation

Local ticket mutations project their renderable fields into the current list and
inspector immediately, then reconcile from the authoritative PATCH response. A
single-ticket mutation must not synchronously reload the ticket collection, selected
ticket, or repository status. Responses carry a per-ticket generation: late responses
are ignored, while the current failed request restores its captured projection and
shows the error. The client emits `hotsheet:mutation-timing` with optimistic and request
phase durations for local profiling.

Bulk mutations hold event-driven collection refreshes until their authoritative atomic
batch or best-effort request sequence settles. Their optimistic rows therefore cannot
disappear, reappear from an intermediate refresh, and disappear again while a multi-ticket
status move is in flight. Back-to-back bulk actions for the same project also run in the
order requested: each action derives its patch, inverse, and expected concurrency tokens
only after the preceding action has committed or rolled back. A successful second action
therefore uses the first response's fresh token, while a genuine external conflict still
restores that action's captured rows, reports the error, and does not poison later queued
work. Each queued action retains its owning project and applies optimistic, authoritative,
rollback, and undo state to that project's cached projection even if the user switches tabs
before it starts or settles; another selected project never cancels the requested mutation or
receives its error state. Attaching a ticket source, including after HS1 import, refreshes provider descriptors
before exposing the imported tickets for mutation.

Ticket creation follows the same immediate-authority rule: as soon as the create
response returns, the new ticket is inserted, selected, and opened for Details editing.
Attachment uploads continue afterward and update the selected ticket in place. Creation
does not block presentation on a collection, selected-ticket, or repository refresh.
A long-poll event caused by that same creation waits behind the local projection and its
ticket motion before performing one authoritative reconciliation, so it cannot replace
the collection while existing rows make room or while the new row fades in.

CI protects the deterministic contract (one PATCH and no follow-up collection/status
GET) and the projection/reconciliation logic. `npm run test:performance` is the stricter
local browser gate: after warm application startup, its click-to-next-frame projection
must remain below 100 ms; 33 ms is the aspirational two-frame target. Network/disk/git
completion is reported separately and does not delay acknowledged visual feedback. The
same warm 100 ms ceiling applies from the parsed authoritative new-ticket response to
its first DOM projection. The parallel suite asserts projection within the first animation
frame opportunity; the performance command runs one worker and additionally enforces the
100 ms ceiling, keeping machine contention separate from application scheduling. The
creation regression deliberately delays the request and reports transport separately,
so network or fixture latency cannot be mistaken for rendering work (HS2-126KNQ).

## 6.10 Repository status browser

The project sidebar repository summary opens a viewport-bounded master/detail dialog.
When the selected project folder has not been initialized as a Git repository, the same
dialog presents a typed recovery state instead of a raw Git diagnostic. Its explicit
Initialize action runs `git init` only at that checkout root, leaves every existing file
untracked, and never stages, commits, or pushes. A second step can add an `origin` remote
or be skipped; it is idempotent for the same URL and refuses to replace an existing
origin. Both mutations require the server secret, resolve only a registered checkout,
and start repository monitoring after initialization succeeds.
The master column uses value cells for branch, upstream, ahead, and behind, followed by
counted Staged, Unstaged, Untracked, Conflicted, and Commits views. The detail column
scrolls independently. File views preserve porcelain-v2 change kinds (including rename
origins) as compact rounded Git-letter badges (`M`, `A`, `R`, `?`, and so on), and
middle-truncate long paths while keeping their beginning and filename-visible ending.
Every file row has a visible ellipsis. Single-click and keyboard activation select a row;
platform-additive and Shift range gestures build a multi-selection without opening a menu.
Only the ellipsis and right-click open the shared action menu. Its Show Diff action opens exact staged,
unstaged, or conflicted working-tree file diff in the configured difftool after fresh
server-side status validation (and is disabled where no meaningful diff exists). A batch
selection opens each selected diff and copies relative or absolute paths as newline-delimited
text; Open and reveal are disabled because they are single-file operations. Double-click
remains a direct shortcut asking the host to open a currently reported file. The menu also
copies relative or absolute paths, opens the file, or reveals it with host-specific
Finder/File Explorer/file-manager wording. The server re-reads status and validates repository containment before any host
launch. At roomy viewport heights the dialog is exactly tall enough for the complete
master column; at constrained heights it caps to the viewport and scrolls that column.
Value groups render each fact through the shared `ValueTableRow` contract, producing the
canonical bordered surface, aligned label/value columns, and inset row separators while
retaining a visible gap between repository identity and synchronization groups. Compare
and Refresh remain separate sibling toolbar groups with the toolbar's standard gap rather
than nesting inside one fused group (HS2-72Z7CB). The master/detail
surface uses 24px roomy pane and viewport insets, 16px between homogeneous value groups
and for constrained pane insets, 8px inside empty/menu-row groups, and 4px for connected
path metadata and menu framing. Shared adjacent `ListItem` view rows remain gapless below
their 4px-separated `ListHeader`; fixed dialog, badge, row, and icon dimensions remain
explicit geometry (HS2-4Y6SM9). In `/ux-demo`, the example stack and repository-status
fixture can shrink with the catalog detail pane, so the embedded dialog plus its Compare
and Refresh header actions remain fully contained at both the 1280px catalog viewport and
the existing constrained layout. This catalog-only containment does not alter the
production popover's full-width or viewport-bounded geometry (HS2-MCHTAW).
The status snapshot contains counts and repository metadata rather than every detail
row. Each file view and the commit view request an initial 50-row cursor page, then an
intersection sentinel fetches further pages as the independently scrolling detail pane
approaches its end. The observer is rebound to the current rendered detail pane after
reactive updates, so replacing a view cannot strand a visible Load more sentinel. Server page sizes are capped at 100, and switching views or
explicitly refreshing resets the active cursor, so very large histories and working
trees do not create an unbounded response or DOM.

The Commits view embeds the same commit graph, configured difftool actions, and
multi-commit range presentation as ticket Code Review. Both individual commits and the
unpushed range are rediscovered and validated by the server immediately before launch;
arbitrary browser-supplied paths or revisions are rejected. Its Git Compare toolbar
action sits first in its own contained group, with Refresh last; it switches to Commits and toggles
a light-purple selection banner without a redundant Cancel action. While enabled, its
shared push-button appearance uses a darker semantic background, matching border, and
inverse icon so the mode remains visible independently of the banner. Compact A/B segmented
controls choose which side the next commit click sets and remain separated from Open;
selecting A advances to B, both
commits receive visible side labels, and Open remains disabled until two distinct sides
are selected. Open uses the same external-difftool affordance as a single commit. The
shared component's complete comparison state is represented in `/ux-demo`. Repository
status remains explicit-refresh/event driven and introduces no polling. The UX demo's
settings select switches among clean, dirty, ahead, behind, diverged, conflicted, and
error fixtures so every headline icon and subtitle can be reviewed deterministically.

Every server-provided multi-commit bundle is placed in history immediately above its
newest (`to`) commit, so the bundle action stays attached to the change it concludes
even when multiple disjoint ticket ranges are present.

## 6.11 Cross-references

- UX component inventory and `/ux-demo` contract: [ux-components.md](ux-components.md)
- Server-side PTY manager that hosts the arbiter: [05-ai-tool-plugins.md](05-ai-tool-plugins.md) §5.4
- The server clients talk to + its auto-start lifecycle: [04-core-server-cli.md](04-core-server-cli.md) §4.3.1
- Remote/mTLS + mobile pairing: [08-distributed-and-remote.md](08-distributed-and-remote.md)
- Leased-claim pattern reused for size claims: [05-ai-tool-plugins.md](05-ai-tool-plugins.md) §5.7
- Why clients don't embed the core / language rationale: [09-technology-decisions.md](09-technology-decisions.md) §9.2
