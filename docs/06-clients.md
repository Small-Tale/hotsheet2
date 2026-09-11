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
` (Copy)`, ` (Copy 2)`, and so on for case-insensitive title collisions; cut originals
are deleted only after destination creation, note creation, and provider-qualified
attachment copying all succeed. A paste is one history transaction: undo archives every
created copy and restores cut originals, while redo reapplies the complete transfer.
Failed transfers archive partial destination tickets and leave cut originals intact.
Ticket copy/cut/paste shortcuts run only while the ticket work area owns focus and a
ticket list or board is present. The work area shows one continuous focus outline around
its composer and ticket surface. Pointer interaction outside it releases that ownership,
including non-focusable inspector text; an ordinary non-collapsed text selection, native
or Web Awesome editable control, or open dialog always retains native Cmd/Ctrl+C/X/V.
Dragging an unselected ticket moves only it, while
  dragging a selected ticket moves the selection; Queue, Backlog, and Archive sidebar
  destinations apply the corresponding status and visibly highlight during dragover.
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
  action entirely instead of presenting a disabled input/button hybrid.
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
  with uploads, patches, or compensating deletes. Completed/verified selections never
  offer Up Next. Changing between Queue, Backlog, Archive, or ticket-error views clears
  the complete ticket selection and its editing state; changing only the list/column or
  other presentation mode preserves that selection.

This is the clean client/service split the rewrite is chartered to create, made
**absolute**: the server is a standalone process even for local use, so the client
is only ever a view. Sharp contrast with HS1, where the server rendered HTML via a
custom JSX runtime, the client re-derived logic in a hand-rolled `kerfjs` SPA, and
the Tauri app *owned* a Node sidecar that died with it.

## 6.2 One access model: talk to a server (local or remote)

There is **one** way a client gets data — over the API to a server. "Local" vs.
"remote" is only *which* server:

| | **Local project** | **Remote project** |
|---|---|---|
| Server | A **localhost** `hotsheet-server` (the client auto-starts it if absent) | A server on another device/machine |
| Transport | HTTP/WS on loopback (+ secret; mTLS optional) | HTTP/WS over **mTLS** |
| Who runs it | This machine's one shared server instance | That device's server |

A client can show local and remote projects side by side (tabs); each tab carries
its server's `(origin, secret)` — carried from HS1's multi-server remote-client
design (§112). A local tab's origin is simply `https?://127.0.0.1:<port>`. There is
no `dataDir`/embedded-core tab kind any more.

**Auto-start + independence (the key behavior).** On launch a desktop client
resolves the local server via `~/.hotsheet/instance.json`; if none is running it
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
cleanup of the old live HS1 data; backups are never removed.

## 6.3 Web client and Tauri desktop host

- **Web first.** Build and iterate on the Kerf web client in a normal browser before
  adding its Tauri host. A `/ux-demo` route renders the real production components
  against deterministic mock service adapters for isolated and composed UX review.
  Keep the platform-neutral component responsibilities close to the planned macOS
  SwiftUI architecture; share concepts and API contracts, not rendering primitives.

- **Installable web identity.** Every web route publishes a web app manifest, the
  exported Hot Sheet flame favicon, square installed-app icons at 192 and 512 pixels,
  a maskable 512-pixel icon, and a 180-pixel Apple touch icon. Browser chrome and the
  installed launch surface use the same lowered-surface `#f2f2f7` color as the client
  shell. The manifest launches at the application root in standalone display mode.
  These static identity assets are bundled into production; Hot Sheet does not use a
  service worker to cache live project/API responses or introduce a second client
  version lifecycle.

- **Stable local development by default.** `npm run dev` in `clients/web` copies the
  package into a temporary snapshot and starts Vite there. The running app retains the
  development bridge and `/ux-demo`, but concurrent edits in the checkout cannot trigger
  HMR or expose a partially edited multi-file state; restart the command to load a new
  snapshot. Generated package-local `target` output is excluded from the snapshot just
  like `dist`, test results, and dependency output. Each stable process also owns a private Vite dependency cache inside its
  snapshot and disables runtime dependency discovery. A later route may therefore load a
  previously unseen ESM dependency without Vite optimizing it and forcing a document
  reload. Playwright and Vitest use separate disposable Vite caches, so a test run cannot
  mutate the cache of a maintainer's running stable client. The launcher
  passes the original repository root into the snapshot so the
  project bridge still resolves the real `target/debug/hotsheet-server` rather than a
  nonexistent temporary `target` directory. Use `npm run dev:hot` only when actively developing the web UI and immediate
  HMR is desired. Browser tests use `dev:hot` on a separate default port and never reuse
  an already-running maintainer server. Signal handling is active before snapshot creation:
  an interrupt during startup prevents Vite from launching, awaits the in-flight snapshot,
  and removes it before returning the conventional signal exit status. After Vite starts,
  shutdown first waits for the child to exit and then removes the private snapshot before
  the launcher itself exits; every path removes its signal listeners and prevents concurrent
  Vite writes from racing snapshot cleanup.

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
  keep a bounded, value-free UI-stability event log. Render events include cumulative
  counters plus per-pass deltas so a captured storm distinguishes reactive rerenders
  from unrelated DOM activity. Three unexpected quick select dismissals within ten
  seconds or twelve root renders within two seconds after startup create a rate-limited
  diagnostic ticket automatically. A root render-storm signature creates at most one
  ticket per page lifecycle; quiet intervals clear stale pass history without rearming
  an already-reported signature. Dev Review is enabled by
  default in development (`?dev-review=false` is the sole opt-out), and its ticket
  dialog offers a checked diagnostic-log attachment so a manually reported transient
  failure carries the same context. Automatic render-storm reporting remains suppressed while
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
  stay on the workspace layer below every modal/dialog backdrop, so opening a dialog
  during a move cannot paint the moving card over the dialog. They expose
  motion-specific identifiers rather than ticket-row roles, actions, or slugs,
  and isolate their visual text from document text queries, so ordinary ticket
  selectors, text search, and assistive technology continue to see one real row.
  Pure inspector, sidebar, or viewport layout shifts never trigger collection motion.
  Queue, Backlog, Archive, and ticket-error views have distinct motion scopes, so
  replacing a whole collection never creates per-row transition ghosts; within-view
  ticket arrivals, departures, and moves retain their normal motion.
  Reduced-motion users get the final layout immediately.

- **Field-aware live editing.** A ticket refresh merges fields that the user is not
  editing immediately. An active text draft adopts a remote-only update when still
  untouched, preserves a local-only edit, and stays quiet when both sides converge.
  Only divergent changes to that same active field open a reconciliation surface with
  the remote and local versions plus an editable merged value. Whole-ticket concurrency
  token failures use the same comparison: unrelated field drift retries once against
  the fresh token instead of presenting a false conflict. Background refresh also leaves
  an in-flight or queued autosave draft alone; the write response and token retry path
  distinguish this client's earlier partial save from a genuinely competing edit.
  The freeform blocked reason uses the same silent blur-flush path and is the single source
  of truth for blocked presentation: a non-empty reason persists and shows the badge/rail,
  an empty edit sends `null` to clear it, and the authoritative response exits
  editing without making the text disappear. An existing blocked-reason surface enters
  that editor on double-click, matching details and ordinary notes. Existing blocked
  reasons and ordinary notes do not add redundant per-item Edit buttons; their content
  surfaces support double-click plus Enter/Space keyboard entry instead.

- **Ticket claims.** Ticket rows show a static yellow lock directly after status only
  while a worker holds a non-expired claim lease. A claim communicates reservation, not
  proof that an AI process is currently running. Started tickets without a lease remain
  unclaimed, and old `claim_count` values never imply presence. A local one-shot expiry timer removes stale indicators without issuing
  polling requests; claim/release changes otherwise arrive through the shared live-update
  channel.

- **Custom project commands.** The sidebar renders machine-local typed command
  definitions as collapsible groups with running feedback, stop confirmation, latest
  outcome, and press-and-hold output history. Definitions are edited in Project
  Settings and persisted to `hotsheet-settings.local.json`; commands always execute
  as an exact program plus argument array. Run transitions use the shared WebSocket/
  long-poll event channel and never introduce client interval polling. Press-and-hold
  remains reserved for output/history. A context or overflow menu will provide “Run in
  new terminal” for shell commands and capability-aware “Create task from command” for
  AI commands (HS2-NT3F3Q).

- **Project settings navigation.** Entering Settings replaces the ticket-oriented
  project sidebar with a persistent category navigator, following the HS1 settings-tab
  pattern. Ticket sources, Commands, Permissions, and Column view each render as a
  separate workspace so unrelated controls do not become one long settings page. The
  selected category names the shared page header; the workspace does not repeat that
  heading, and the right region uses the same divider-free empty inspector placeholder
  as Notifications. Entering Settings preserves the ticket selection for returning to
  list/board, but that retained selection never changes the Settings placeholder semantics.
  The selected category, unsaved Commands JSON draft, and its validation result belong to
  the active project rather than the shared shell: a newly opened project starts on Ticket
  sources, while returning to another project restores that project's category and draft.
  Project activation refreshes the visible settings data and ignores late provider responses
  from a project that is no longer selected.
  Right-sidebar toolbars are divider-free in every state—ticket, loading, multi-selection,
  Settings, and Notifications—so content sections, not the shell toolbar, own separators.
  The ticket inspector uses an 8px horizontal content gutter. Details, Tags, Notes,
  Block ticket, and Add note reuse the shared sidebar `MenuHeader`/`MenuItem`
  primitives; their text and icons share one inset while section surfaces stay flush
  beneath their headers instead of accumulating another indentation level.

- **Persistent shell splitters.** The project sidebar and ticket inspector are
  independently resizable by pointer or keyboard. Dragging updates only splitter
  geometry until release, then persists the bounded width locally so a reload restores
  the layout without creating broad render churn.

- **Stable scrolling across mutations.** List, board, each board column, and each ticket
  row expose stable Kerf `data-key` identities. Status changes and other ticket mutations
  therefore morph the existing scroll owners instead of replacing them; list position and
  every independent column position survive moves between statuses (apart from the browser's
  normal one-pixel scroll anchoring correction).

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
  one failed hidden remembered project is pruned from the remembered set and reported
  non-modally; it must never leave stale compatibility or connection diagnostics over a
  different project that reopened successfully.
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
  The Vite-only bridge discovers or detached-starts one bootstrap machine server and
  attaches every discovered or explicit project store through the server's multi-store
  open path; it never starts one server per project. Discovery is health checked and the
  bridge re-supervises after transport failures and terminal-WebSocket reconnects. Safe
  GET/HEAD requests retry after recovery, while ambiguous writes return an explicit 503
  instead of risking duplicate mutation. Compatible old servers may be upgraded through
  their authenticated quiescence/restart capabilities; the bridge waits for the old
  registration to disappear before starting or joining its replacement. The bridge keeps
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
  inset-separator value table as repository status. Because this metadata is safely recoverable, the
  native popover is dismissed by clicking outside or pressing Escape and has no redundant
  Close button. Every compatibility and recovery state is represented in `/ux-demo`.
  Recovery guidance distinguishes safe compatible skew, stale local source, old client,
  old server, and unavailable metadata; it never offers automatic restart without the
  same explicit restart plus quiescence capability gate.
  Ticket-provider connections are not stored in `hotsheet-settings.json` or
  `hotsheet-settings.local.json`: those remain shared/local preferences. Git sources are
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

  Workspace search delegates to the checkout index rather than filtering compact rows
  in the browser. It therefore matches slug, title, tags, Markdown details, and note text
  while retaining the full local ticket collection for project counts, mutations, and an
  immediate return to the unfiltered view when search is cleared. Search semantics do not
  inherit the selected sidebar view: ordinary queries cover the normal working statuses,
  exact-slug lookup can reveal Backlog/Archive/Deleted matches, and explicit scope/filter
  chips opt into normally excluded lifecycle states. Reference-mention matches say why
  they matched, every result names its provider, and the global overlay can hand its
  current query/scope/filter payload to the separately owned saved-view editor without
  replacing the compact workspace search. The overlay is available from the toolbar and
  the platform Search shortcut (`Command-K`/`Control-K`).

  Empty ticket collections use the shared `TicketEmptyState` composition in both list
  and board modes. A project with no tickets invites its first ticket, a populated
  project's empty view names that view, an in-flight search reports that it is still
  searching, and a settled empty search repeats the query and suggests changing it.
  When every board column is empty, the board renders one board-wide message beneath
  the retained column headings. Individual empty columns remain blank rather than
  repeating per-column placeholders. While the initial ticket collection is unresolved,
  the same list/board content area instead shows a centered animated **Loading tickets**
  state beneath the retained column headings; it never flashes premature empty-project
  copy or duplicates the corner activity indicator.

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
  and corrupt-ticket recovery actions.

  The shared left project sidebar presents a centered `M open, N up next` summary
  immediately above Drive. Both counts derive from the existing checkout ticket collection:
  open means exactly Not Started + Started (Backlog is not active work), and Up Next
  additionally requires the Up Next flag. Mutations and long-poll-driven collection refreshes
  update the summary reactively; the summary itself performs no polling or network request.

  Switching to an already-open project is a local projection change, not a loading gate. The
  tab click atomically restores that project's most recent ticket rows, repository summary,
  corrupt-ticket diagnostics, and command state from memory within the next frame, without
  showing the global loading indicator. An authoritative refresh follows in the background.
  Project-activation and request generations reject late A→B→A responses, including delayed
  workspace-session draft restoration, so cached immediacy cannot introduce cross-project
  state or stale network writes. A first visit with no cache retains the normal loading state.

  Drive is a production control, not demo-only state. Its split-button label reflects the
  machine-local default provider discovered from drivable plugin manifests. The arrow opens
  hierarchical Default/provider/model/effort overrides without a client-owned provider table.
  Parent rows rely on the shared menu's single disclosure marker; child provider, model, and
  effort choices use aligned semantic icons plus one highlighted current value with compact
  submenu insets, including at the supported narrow width.
  Drive prepares a stable dedicated connection scoped to that checkout and tool, opens and
  selects its AI-chat tab in the bottom drawer, then sends the `$hotsheet` workflow turn; later
  activations reuse that tab, connection, and retained session. The server resolves the checkout id to its code
  root before preparing the tool—ticket-store paths are never used as the working directory.
  The sidebar derives running
  state from `GET /connections`, refreshes it only from replayable `drive_updated` events,
  and disables a second Drive activation while that turn is busy; interruption remains in the
  selected chat when the connection advertises it. The Views add action opens a compact
  create dialog for a readable name and any ordinary search expression. Saved views live in
  the ticket store's shared settings, appear in both the project sidebar and terminal ticket
  rail, and apply their query through the same inline text/token search pipeline. Their
  `custom:<id>` selection restores per project, follows replayable `views_updated` events,
  and falls back to Queue if a selected shared view is removed. Creating a view rejects empty,
  overlong, or case-insensitively duplicate names and empty or overlong queries before saving.
  Every saved-view row exposes labeled rename and delete actions. Rename keeps the stable view
  identity and search query, including while selected; delete confirms that tickets are not
  affected and returns a deleted active view to Queue. Both mutations preserve shared-setting
  ordering and reject case-insensitive name collisions.

  Search has one primary surface: the project toolbar. The former global search overlay and
  its separate scope, suggestions, result rows, and saved-view handoff were removed because
  they duplicated the ordinary inline search flow without a distinct navigation role. Exact
  cross-project ticket references continue through the compact link-resolution chooser.

  The MessageSquare action is available before Drive and opens the production
  `AIConversation` dialog after preparing the default tool without sending a workflow turn.
  Project Chat and Drive use different stable connection ids: Chat is a general project
  conversation, while Drive is the explicit `$hotsheet` automation shortcut in the drawer.
  Kerf retains an ordered transcript and composer draft
  per connection; each submit appends a user message and one assistant message whose Markdown
  content grows in place from attributed `turn_event` output. Native activity and permission
  events provide specific progress text, and connection-matched permission requests reuse the
  standard decision card inline. Completed, failed, and interrupted outcomes remain on their
  turn. Stop appears only for a busy connection advertising `interrupt`; Enter sends and
  Shift+Enter adds a line. Plugins may advertise live model and effort changes, which the
  conversation applies to subsequent turns without changing provider. Connection refresh and transcript updates share the existing
  replay-safe WebSocket/long-poll stream—this surface adds no timer or simple polling.
  While the dialog is closed, streamed transcript/activity state remains retained but the
  conversation surface is not mounted and does not subscribe the application root to those
  high-frequency signals. Opening it projects the accumulated transcript in one pass; background
  tool output cannot cause a root-render storm or disturb unrelated controls.
  Usage events attach token/cost metadata to the active assistant turn and derive a
  conversation total without a second counter; unknown cost is labeled unavailable. The same
  stream's normalized activity events are session/connection matched into a bounded activity
  sequence with persistent AI/tool attribution and an accessible may-contain-errors cue. The
  dialog uses the shared compact dialog header instead of stacking a second application header
  beneath the platform dialog title. Its secondary line reports useful ready/working/message-count
  state instead of continuously exposing the opaque session id. Plugin-provided model and effort
  choices carry visible labels, user-authored Markdown keeps loud-surface contrast throughout its
  nested content, and repeated activity-level disclaimers are shown only on their attributed rows.
  Its full-width composer keeps the send action attached
  to the input at wide and narrow sizes. In the bottom drawer, the embedded conversation is a
  height-bounded column: header, session controls, and composer remain fixed while the transcript
  alone takes the remaining height and scrolls, so a long conversation cannot push the input below
  the drawer viewport. Retrying preserves the earlier transcript and activity
  while clearing the stale failure. A launch failure is presented as a contained alert and keeps
  the underlying Codex daemon diagnostic, so failures such as an invalid control-socket path are
  actionable instead of collapsing to an unexplained exit status. Opening a populated transcript
  starts at its latest message; streamed growth remains pinned while the reader is already at the
  bottom, but never pulls them away from older messages they intentionally scrolled back to read.

  Completed transcripts can be saved from either conversation presentation. The compact,
  three-step save wizard first previews the transcript so the user can keep everything or choose
  a contiguous inclusive range directly from the messages, then chooses optional bundle contents,
  and opens the host folder picker only from the final Save action. Popup lifecycle events from
  controls inside the wizard never dismiss the wizard itself. It creates a portable `.hotsheet-chat`
  directory bundle. Every bundle contains `manifest.json`, a
  readable `transcript.md`, and lossless `conversation.json`; optional `summary.md`, attachment,
  and original-media entries are explicit. The browser receives only an opaque destination token,
  while the trusted local bridge validates sizes and identities and owns all filesystem reads and
  writes. Selecting an existing bundle requires an explicit overwrite or same-conversation
  re-export, with revision lineage recorded in the manifest. Reopen behavior is summarized quietly
  on the review step instead of presented as a separate option. Saved conversation bundles reopen
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
  ordinary no-space references and standard Markdown link/image destinations are also
  accepted. Matching prefers the longest real attachment filename and leaves any trailing
  sentence punctuation in the prose, so `attachment:proof.png.` resolves `proof.png`.
  When that filename is already present on the loaded ticket, previews use the attachment's
  immutable id route; the by-name route remains the fallback for references whose target
  metadata has not been loaded. This keeps an inline preview and its gallery action on the
  same concrete attachment even when filenames or trailing prose are ambiguous.
  Missing ticket or filename targets do not reject a note (attachments may be uploaded next),
  but mutation callers receive prominent actionable warning feedback. Attach output leads with
  this filename syntax rather than its opaque storage id. Git-backed note writes also translate
  unambiguous bare attachment ids in ordinary prose into resolvable same-ticket or cross-ticket
  references, while retaining literal ids in code, URLs/paths, and ambiguous or currently
  unrepresentable filename cases (HS2-H2PTVZ). Browser-compatible
  bare references and explicit image destinations render inline; explicit Markdown links
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
  one match switches projects when necessary and selects the ticket, no match reports a
  transient toast, and multiple matches open a compact source chooser rather than the
  advanced-search surface. The explicit `@<project-id>/<ticket-slug>` form, for example
  `@product-docs/HS2-BD09B6`, limits resolution to one open project and makes cross-project
  links unambiguous. References already inside Markdown links, inline/fenced code, or
  attachment controls remain unchanged.

  The Attachments tab keeps the complete file list and adds a responsive, wrapping
  grid of 160px square contained previews for browser-compatible image and video
  formats, including SVG, MP4, MOV, M4V, OGV, and WebM. Grid videos preload only
  metadata and never autoplay. Their poster uses one predictable attachment `thumbnail`
  GET/PUT endpoint backed by a SHA-256 content-addressed host cache. Web clients seek and
  draw a frame with native video/canvas APIs and upload the JPEG during browser uploads;
  the first capable browser viewing an older or CLI-created video lazily backfills a
  missing poster. Existing posters short-circuit generation, while concurrent idempotent
  writes are last-write-wins. Missing posters, unsupported codecs, and generation or
  upload failures leave the original video playable and never prevent opening its ticket.
  A headless host may opportunistically use an already-installed `ffmpeg` executable
  (`HOTSHEET_FFMPEG` can name one outside `PATH`), but HS2 neither requires nor bundles it
  for posters and its absence is only a cache miss, not a setup failure. The original video response uses its native media MIME type,
  advertises byte-range support, and returns valid single-range `206` responses so
  Safari and other media engines can discover duration and seek normally. The browser-native
  flow and server cache contract are identical on macOS, Linux, and Windows.
  A preview or inline image opens the same full-screen
  media gallery; videos remain paused initially but preload and present their decoded
  first frame rather than carrying the grid thumbnail poster into the full-screen player.
  A paused scrub presents the decoded frame at the selected time without requiring a
  play/pause cycle. They expose only Hot Sheet's custom play/pause, scrubber, time, and
  volume controls, never a second native browser control strip. The volume icon opens
  a click-persistent popup containing both the slider and mute action; only clicking
  outside that popup dismisses it. Playback ticks and scrub input update the live gallery
  imperatively and commit state only at interaction boundaries, avoiding application-wide
  Kerf renders for every media event without allowing an intervening render to reset an
  in-progress scrub to `00:00`. When the video canvas has focus, Space or K toggles playback,
  Left/Right step one 30-fps frame, Shift+Left/Right and J/L jog one second, and Home/End seek
  to the media boundaries; video jogging never activates the image-gallery navigation path.
  Closing or changing gallery media explicitly pauses the prior video, removes its URL,
  clears any stream source, and reloads the source-free element before unmount so decoding,
  network, and media events cannot survive repeated gallery sessions. The playback footer occupies layout space below the media
  stage, so both contain and cover scales are calculated from the space that remains.
  Full-screen media preserves the source image or video's square outer geometry: the
  gallery does not add corner rounding to either the media or its sizing wrapper.
  Its filename uses inverse toolbar text, while navigation, action, close, and zoom
  controls all use the shared dark ToolbarControlGroup tone so translucent backgrounds,
  borders, icons, and hover states retain contrast over arbitrary images.
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
  opens the same shared MenuItem-based menu for Open, Download, Copy reference,
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
  body, abbreviated SHA, and date even when no review tool is configured. Clicking the
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
  status patch or note. Duplicate closure searches checkout-wide tickets, excludes and
  rejects the source ticket, requires an explicit canonical target, and sends its durable
  identity as `duplicate_of`. The inspector renders the saved outcome and lets users open
  the canonical duplicate target even when it is outside the current list filter.

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
  reopens the user's active chooser. Project switches abort the previous poll. Network failure retries
  with a fresh cursor and bounded exponential backoff without refreshing on every
  failure; the first successful reconnect reconciles once. The server-side project
  bridge retains legacy query authentication for `/ws/poll`, so a newer browser client
  does not spin on immediate authentication failures from an older running server.
  Activity/presence events are deliberately outside this ticket-refresh lifecycle.

  The default `Queue` view is the active working set and intentionally excludes both
  Backlog and every terminal/archive status. Backlog and Archive are disjoint explicit
  views with counts derived from those same predicates. The new-ticket composer is
  available in Queue and Backlog but hidden in Archive; creation from Backlog defaults
  the new ticket to backlog status. Its expanded first row keeps the title beside the
  category and an immediately trailing star toggle; the star creates directly in Up Next
  (and therefore overrides a Backlog-view default to active Not Started). A full-width
  Details textarea follows on its own row, starts one text line tall, and resizes vertically.
  Its chosen height is a device-local preference that survives controlled-value rerenders,
  cancellation/reopening, and later new-ticket sessions.
  Switching among Queue, Backlog, and Archive is a client-only projection over the
  already-loaded compact ticket collection; it does not fetch full ticket bodies or wait
  on the database. The selected sidebar item and the first progressive row tranche commit
  in one batched Kerf render, avoiding an intermediate rerender of the previous view. Large
  views initially render 80 rows and continue in idle chunks while exposing loading state.
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

  Workspace chrome preferences are device-local browser state. The client restores the
  last view mode, sort field and direction, project-sidebar and inspector visibility,
  outer project-command expansion, each named command group's independent per-project
  collapsed state, and independently clamped sidebar/inspector widths
  across reloads. Missing, partial, malformed, or unknown enum values fall back per
  field to safe defaults rather than preventing project open. Selecting a ticket still
  reopens the inspector and persists that explicit state transition. List or column mode
  remains selected while visiting the cross-project terminal dashboard and when following
  ticket references in details or notes; if navigation begins from a non-ticket workspace,
  it restores the last explicitly selected list-or-column mode.
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
  the *same server*, not from sharing a linked library.
- **macOS:** auto-starts + supervises the local server (like Tauri, §6.3).
- **iOS is remote-first — and structurally so.** A phone can't run an independent
  background server (iOS background-execution limits), and it rarely hosts the git
  repos or drives AI tools anyway. Its job is to view/triage tickets and answer
  permission prompts against a server running on a Mac — the **remote-server** path
  (mTLS, QR pairing — §112.6). There is no local-server-on-iOS mode. See
  [08-distributed-and-remote.md](08-distributed-and-remote.md).
- **Terminals/AI-drive on mobile:** out of first scope. Mobile watches and
  triages; driving AI tools stays on the desktop/server. Answering permission
  prompts and reading busy state *do* work on mobile (they're just API events).

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
- Multi-project tabs (local + remote), with pointer drag reordering and remembered
  device-local project order. A live claim uses a static segmented claim ring and a
  “claimed” accessible label; it never presents a lease as proof of active AI execution.
- Search (FTS) and filtered views.

Closing a project tab first inventories its live terminals and AI chats. When any are
running, a confirmation dialog uses the shared menu navigation to select an item and
shows either its live, read-only terminal renderer or the AI provider, model, effort,
and latest Markdown activity without duplicating the selected item's title.
**Keep Running** removes only the local project tab. Reopening the project reconciles
eligible drawer Chat, Drive, and resumed-saved-chat connection ids from the server into
their original AI-chat tabs, including provider, model, effort, busy, action, and session
state (HS2-D34C2V). Transcript state already received in the same app window remains keyed
to that connection and returns with the tab; after an app restart, the live server session
continues but earlier messages are not retrospectively reconstructed. The close dialog
states that boundary explicitly. **Stop & Close** explicitly deletes
every listed terminal and AI connection before removing the tab. Cancel and native
dialog dismissal preserve both the project and all resources. Multi-tab close actions
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
There is no fixed-interval network polling. Pending counts appear in the main segmented
control and project tabs, and a non-modal popup appears even when another project is
selected. When a standalone AI conversation is open, the active permission popup is
promoted into that dialog's top layer so it remains visible and interactive instead of
being trapped beneath the modal; resolving it uses the same authoritative permission
path. Standalone conversation dialogs use native light-dismiss and Escape behavior and
do not duplicate that dismissal with a header close button. The global Notifications view
keeps pending requests above newest-first machine-local client history; a request that
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
Show / Hide Terminals dialog, its badge counts terminals hidden by the active group, and the
adjacent Select switches among device-local named groups. Default is permanent; named groups
can be created, renamed, and removed, and each group records terminal inclusion without
destroying sessions. The compact selector's open menu sizes to its option content instead of
the narrow closed control, so checkmarks and complete group names remain visible. The dialog's
tab toolbar remains transparent against the white dialog
surface rather than introducing a separate gray band. Visibility groups apply only to the
global dashboard; the project drawer
always shows its project's terminals and has no visibility controls. Newly created terminals
appear in Default and start hidden in existing named groups. The dashboard always uses one
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
`MenuHeader` and the shared seven-day `ProjectSummary` for every open project. With multiple
projects, a leading `All projects` group sums each aligned trend day plus the completed-today
and in-progress counts. Every chart in that cross-project list uses the maximum of the summed
`All projects` trend as its shared scale, so equal bar heights mean equal activity across the
aggregate and each project; a single-project summary continues to use its own maximum. An inset
divider below that aggregate keeps it distinct from the individual project list. The project summaries open project statistics and the aggregate opens
cross-project statistics. These values derive from ticket collections already loaded for the
open project tabs, so the sidebar adds no polling or network traffic. It remains usable beside
the grid and ticket rail at the supported 1024×600 floor, and can be hidden and restored from
the leading edge of the dashboard toolbar.
The dashboard keeps a resizable right ticket rail open by default and allows it to be hidden
and restored from the dashboard toolbar. That rail reuses the selected project's list and
notification views, compact workspace actions, content-sized project selector, and quick-ticket
launcher; board and settings modes are deliberately absent. The rectangular list/notification
segmented control owns its full first row, while sort, selection actions, and the compact search
launcher share the second row. Activating search animates it onto a dedicated full-width third
row, where tag-autocomplete options stretch across the popup with consistently left-aligned
labels. This is the same advanced search surface and state as the main workspace, including chips,
tag completion, attachment/presence filters, relative or local dates, and syntax help. A
well-formed structured value becomes a chip only after an explicit impossible continuation
such as trailing whitespace or Enter; incidental focus loss never commits a partial value.
Quoted filters also remain editable until the closing quote and explicit commit delimiter.
Committed chips stay at their exact positions inside the editable expression, with caret stops
before, between, and after them; pressing Right Arrow at the boundary before a trailing chip
reaches its editable suffix instead of trapping the caret. Queries such as `NOT tag:client AND parser` retain readable
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
changing rows, columns, or glyph proportions. Magnifying a grid tile preserves
the same exact grid and terminal-screen aspect. Changing grid fit or magnifying
never derives PTY rows or columns from tile dimensions. Dedicated project-drawer terminals
remain fitted to their actual interactive viewport and reserve one physical containment row;
server size echoes cannot restore the edge row that would otherwise be clipped. Abrupt drawer
changes such as maximize explicitly publish a post-layout resize boundary on the next animation
frame, with a settled follow-up, instead of depending on an observer that can remain one resize
behind the container. Fixed-grid surfaces reveal after one physical-scale pass, avoiding the
incremental typography loop while moving from a drawer to the dashboard or magnifying a card. The preview
and its inset frame use the terminal background token, so unused space
cannot expose an unrelated gray surface. The computed tile height derives the 5:3 preview
from the card width, then adds the tokenized frame/footer chrome, so repeated viewport changes
cannot push the terminal outside its card. Dashboard
previews never accept terminal input. Click opens and focuses a separate interactive viewport
centered over a full-browser dimming layer; click-away restores the grid. Its footer exposes
an external-open action, and both that action and a footer double-click open the terminal in
its project's maximized drawer. A grid-tile double-click does the same, while right-click
exposes shared Open/Hide menu items. A Lucide ellipsis in the shared grid/magnified card footer
opens that exact same menu from the keyboard or pointer. The focused dedicated drawer consumer re-fits after both
the immediate and settled layout passes, avoiding clipped cells and cross-surface resize races.
While that magnified viewport is open, its containing workspace is promoted above adjacent
sidebar dividers and suppresses its own focus presentation immediately, so neither shell chrome
nor a transitioning focus outline can paint over the modal. On the first replay payload for a
fixed 80×24 dashboard consumer, the client removes only zsh's exact reverse-video partial-line
`%` marker when it leads the bounded replay. Ordinary percent signs, later output, and the
dedicated drawer stream are preserved unchanged.
The browser regression follows the complete user path with a newly created terminal: enter
Nano, resize the drawer up and down, abruptly maximize, move to the dashboard grid, magnify
and dismiss, then double-click back into the drawer. Every boundary asserts the current
claimed/grid geometry and visible xterm-screen containment. It samples the post-paint frames
through maximize and both fixed-grid mounts, so a stale or malformed intermediate frame cannot
pass on a correct final state alone. Returning from the dashboard to an already-open drawer is
idempotent and settles geometry without replaying the drawer's show animation.
The UX catalog mounts the same xterm frontend over deterministic ANSI fixtures rather than
substituting a text placeholder. Its preview is constrained to a realistic grid-card width,
the magnified variant receives the remaining stage width, and both demonstrate the canonical
code font and a populated 80×24 Nano screen. Its initial terminal focus is one-shot: later
xterm paints, including cursor blinking, never steal focus from catalog controls or close an
open Web Awesome popup.
HS2-PD4MZ9 replaced its snapshot-only panes with xterm-backed interactive
viewports over the existing terminal attach WebSocket. HS2-586BVQ ships the project-only
bottom drawer over that same viewport boundary.

When a launch restores remembered projects, the client holds a single project-restoration
surface until every remembered project's tickets and terminals, plus the remembered active
project, have settled. The complete shell is then revealed in one render boundary. A refresh
therefore never exposes a partially restored board beside terminal content from a different
stage of startup.

The project terminal drawer occupies only the center AppShell column, leaving the project
sidebar and ticket inspector at full height. Its compact rail switches between the decorated
grid, one undecorated interactive xterm session, or one embedded AI conversation that fills
the content area. Its grid tab
never shrinks when terminal tabs consume the available width. The terminal tabs scroll
horizontally, with the explicit quiet pill-shaped plus action immediately after them; plus
opens a direct shared-menu choice of Default shell, AI shell, or AI chat. The menu has no
redundant heading, and leaf actions do not display submenu chevrons. Option/Alt on either AI choice prompts for a
plugin-discovered provider, model, and compatible effort; AI shells use the real plugin-backed
`connect` launch path rather than treating the provider id as a shell command. A dedicated
xterm viewport receives focus as soon as it
mounts, allowing immediate typing without an extra click; this is a one-shot request that
does not make later refreshes steal focus. Project and terminal tabs share one pill-tab primitive, with
the close button before the label and optional leading/trailing state icons. Right-clicking
project, terminal, and AI-chat tabs offer Close Tab, Close Other Tabs, directional close,
and Close All Tabs. Drawer close ranges use the complete remembered mixed order, so each
action closes both terminal and AI-chat targets; terminal tabs additionally offer Rename.
Human-readable defaults replace generated
ids, and device-local rename overrides survive refresh/reopen without renaming the PTY
identity. Project tabs reorder among projects; terminal and AI-chat tabs reorder together in
one mixed drawer strip by dragging across either kind. Project order is stored with the
open-project roots and restored without changing the remembered active project. Each
project's mixed drawer order is stored device-locally and remains stable across refresh and
drawer reopen. Holding Option/Alt when opening the menu changes the directional action to the
left. Alt+Shift+Left/Right reorders the focused drawer tab without losing focus. Closing a
selected terminal or chat chooses the nearest remaining tab to its right, then left, across
both kinds before falling back to the grid.
Closeable tabs reserve the same trailing state slot even when it is empty, balancing the
leading close control and preventing labels from shifting when status appears. Terminal tabs
use that shared tab surface directly rather than layering a second selected background inside
it. Every selected and unselected tab is reachable in sequential Tab order, while
Left/Right/Home/End traverse the current tablist and Delete/Backspace closes the focused
closeable tab. The segmented dashboard and view controls likewise expose each choice in Tab
order. The horizontally scrolling tab strips reserve an inset on every edge so pill shadows
and focus rings remain complete at either end. A dedicated terminal uses the same token-sized inset as a dashboard viewport and shares
one domain background token between its session container, viewport, and xterm theme, so
the edge area is symmetric and visually continuous with the terminal canvas.
Double-clicking the rail, grid tab, or any terminal tab toggles drawer maximization while
preserving the last manual height. Closing the selected terminal follows HS1's
nearest-neighbor behavior (right first, then
left, then the grid). The rail deliberately omits global visibility/group controls and collapses
to one floating restore button. Dedicated sessions use xterm's WebGL renderer by default, fall
back when WebGL is unavailable, and refit only after a drawer resize gesture settles. While
the splitter is held, neither dedicated xterms nor grid-tile geometry is recomputed and no
intermediate PTY size claims are sent; this avoids the old debounce behavior that still fired
during a slow drag.
Their library-owned DOM is protected from application morphs, so committing a resize keeps
the same xterm instance and WebSocket instead of reconnecting the shell.
Global project/terminal jump actions open this drawer on the matching project and terminal.
Drawer visibility, height, selected terminal per project, and independent short-container
zoom are device-local. Drawer scale always uses the 1–3 short-container model. Level 1 makes
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
single source for live output and its initial replay. Read-only grid previews keep no xterm
history, temporary magnified dashboard viewers keep 1,000 lines, and dedicated interactive
terminals retain the full 5,000-line client history. Disposing a viewport cancels its frames,
timers, observers, xterm subscriptions/addons, and socket; repeated magnify/dismiss cycles are
covered as a stable-resource lifecycle rather than allowing detached viewers to accumulate.

The viewport renders ANSI/VT output with xterm, forwards typed input as terminal text, and
sends `{viewer_id, cols, rows, focus, visible}` claims on connection, geometry/focus/
visibility changes, and a five-second lease heartbeat. The heartbeat renews server state;
it is not request polling. Disconnects retry with bounded exponential backoff, while
dispose closes the socket so the server removes that viewer and self-heals its chosen size.
Client claims normalize transient non-finite geometry to bounded integer dimensions. The server
drops any JSON frame carrying a malformed `resize` member instead of forwarding that protocol
text to the PTY, so a hidden or transitioning viewport cannot echo control JSON into the shell.

Renderer choice follows the proven HS1 split rather than forcing one backend everywhere.
Full-size dedicated drawer terminals use xterm's WebGL addon (with DOM fallback after load
failure or context loss). The fixed 80×24 dashboard grid and magnified surfaces use xterm's
DOM renderer because those surfaces are uniformly CSS-scaled; scaling a WebGL raster makes
the terminal blurry and can produce misleading intermediate canvas geometry. Retina browser
coverage therefore checks the dedicated WebGL canvas backing-store size separately from the
scaled DOM surfaces instead of treating `.xterm-screen` bounds as proof of a completed paint.

A visible mounted fixed 80×24 surface is an active sizing claimant even though its grid preview is
read-only and never accepts keyboard input. This ensures entering the dashboard actually
resizes the PTY to the promised 80×24 contract rather than merely drawing an 80×24 xterm over
output that the TUI emitted for the drawer's previous size. Conversely, offscreen dashboard
cards do not mount a viewport or open a socket until intersection observation reaches them, so
an unpainted or hidden fixed-grid card cannot take sizing control. Finishing a drawer drag or maximize
returns input focus to the selected dedicated terminal before its final claim; clicking the
drawer rail must not leave the server holding the old PTY size while only the WebGL canvas
grows around stale TUI output.

### 6.7.1 The fundamental constraint

A PTY has **exactly one size** (cols × rows) at any instant. Resizing it sends
`SIGWINCH`, and the program inside (claude, vim, a TUI) **reflows to that size** —
so resizing is disruptive and must be rare and deliberate. Meanwhile many
**viewports** may show the same terminal at once, each a different size:

- several views on **one** device (the drawer terminal, a dashboard tile, a
  magnified view — HS1's borrow-stack case), **and**
- views on **different** devices at once (a macOS window *and* an iPhone).

You cannot give each viewport its own native size of the *same* session: a single
PTY emits one size's worth of output, and an alternate-screen TUI was drawn for one
grid — it can't be losslessly re-flowed to another (only line-wrapped scrollback
can). So the model is **one arbitrated PTY size + graceful handling in every other
viewport** — the same reality tmux lives with. (If per-viewer native size is ever
truly needed, that's a *separate PTY per viewer* — a different shell, not this
shared session — see §6.7.5.)

### 6.7.2 The model: the server arbitrates, viewports make *claims*

The server owns the PTY, so it is the single arbiter of its size — matching the
"server is authoritative" principle and, crucially, giving **one** coordination
point for local *and* remote viewers. Each viewport registers a **size claim** over
the terminal WebSocket and keeps it alive with a heartbeat:

```
viewer → server:  { viewerId, cols, rows, focus: bool, visible: bool, activityAt }
server → viewers: { ptySize: {cols, rows}, drivenBy: viewerId }   // broadcast on change
```

- `viewerId` is **per viewport, not per device** (`<clientId>:<paneId>`), so
  intra-device and cross-device viewports arbitrate uniformly — this *is* the
  borrow-stack, generalized to every viewport everywhere.
- Claims are **leased** (reusing the claim/lease pattern, [05](05-ai-tool-plugins.md)
  §5.7): a viewport heartbeats; on disconnect (a phone that drops off Wi-Fi) its
  claim **expires** and the server recomputes size from the survivors — so a gone
  viewer never pins the PTY to its size forever. This is the piece HS1 never had.
- The server broadcasts the resulting `ptySize` to **all** viewers, so everyone
  agrees on the real size and each renders within its own viewport (§6.7.4).

### 6.7.3 The sizing policy: focus-follows, with hysteresis

Default policy (= tmux `window-size latest`, which is exactly the maintainer's ask —
"right-sized based on whichever device and view area had most recent focus"):

- **The PTY follows the size of the viewport that most recently held input focus.**
  When focus moves from the big macOS pane to the small iPhone view, the PTY
  resizes to the iPhone (after the guards below); when focus returns, it resizes
  back. `activityAt` breaks ties if two devices both believe they're focused.
- Activating the read-only dashboard counts each visible fixed 80×24 tile as that
  PTY's local sizing focus. A grid tile cannot accept keyboard input, but entering
  the terminal-specific surface is still a deliberate request to render its TUI at
  the grid contract rather than at an obsolete hidden-drawer size.
- **A focused, actively-typing viewport's size is locked in** — a background device
  cannot resize the PTY out from under someone mid-keystroke. To change the size,
  take focus (which transfers the size).
- **When nothing is focused, hold the current size** (don't resize on mere
  visibility changes) — glancing at a terminal from a second device must not reflow
  it.

**Anti-thrash guards** (named so implementation has targets; tune later):
- `SIZE_FOCUS_HOLD_MS` (~500 ms) — a newly-focused viewport must hold focus this
  long before its size is applied (kills ping-pong when focus flickers).
- `SIZE_MIN_DELTA` (≥2 cols/rows) — ignore sub-threshold differences.
- `SIZE_RESIZE_MIN_INTERVAL_MS` (~100 ms) — rate-limit actual PTY resizes to ten per
  second. Browser viewports coalesce layout work to animation frames and send a final
  claim 120 ms after resizing settles, so a suppressed in-window update cannot leave the
  terminal at an obsolete size until its heartbeat.

**Alternative policies (configurable per terminal), for when focus-follows isn't
wanted:**
- `smallest` — size to the smallest *visible* viewport so everyone sees the whole
  screen without scroll (tmux's default; good for "we're both watching").
- `largest-visible` — one big screen drives; small screens observe (scroll/scale).
- `pinned` — a fixed size the user sets; all viewports letterbox/scroll. Good for
  recording or maximum stability.

Recommend **`focus` as the default** (it's the described need) with the guards
above, and expose the alternatives as a per-terminal setting.

### 6.7.4 Rendering when a viewport ≠ the PTY size

Every non-driving viewport reconciles its viewport against the broadcast `ptySize`:

- **Viewport larger than the PTY** → **letterbox**: render the grid at its true
  size within the pane (centered / top-left), padded with the theme background.
  Never stretch. (HS1 already handles the gutter/padding — §22.6.)
- **Viewport smaller than the PTY** → **scale-to-fit then scroll**: shrink the font
  toward a readable floor to fit; below that floor, scroll within the pane. A phone
  glancing at a desktop-sized terminal scales to fit for reading; to *interact* it
  takes focus and the PTY resizes to it.
- Show a subtle affordance when a viewport isn't driving the size (e.g. "viewing at
  120×40 — tap to resize to this screen") so the mismatch is legible, not confusing.

The web viewport implements this with a true-size top-left letterbox when the PTY fits,
proportional scale-to-fit down to a 70% readable floor when it does not, and pane-local
scrolling below that floor. A non-driving overlay reports the broadcast dimensions and
invites focus; focusing sends a new claim and leaves the arbiter—not the browser—to decide
whether and when the PTY actually resizes.

### 6.7.5 Escape hatch: a per-viewer *separate* terminal

When someone genuinely needs a natively-sized terminal on each device
simultaneously, that's **not** one shared session — it's **separate PTYs** (the
multi-terminal model, HS1 §22.17). Each is its own shell/program at its own size,
no arbitration needed. Hot Sheet supports both: *share this terminal* (arbitrated,
this section) vs *open my own terminal* (independent). The arbitration only governs
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
`terminal.inherit_global_shell_history` in `hotsheet-settings.local.json` (HS2-A5V801).

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
  render the exact acknowledgement as a subtle ordinary note rather than another
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
New core writes promote the same marker to the first-class kind. Rebuilding an older
disposable index must therefore recompute the compact flag for unchanged ticket files.

**Reader mode is a directly editable focus surface.** Opening reader mode shows the
ticket's details + notes on one large scrollable surface with no separate top-level edit
mode. The reader uses the available browser height with exactly 24px of backdrop above
and below; it has no desktop-height cap that leaves unnecessary vertical space. Details
and ordinary notes expose their normal edit interactions immediately.
Its **A Large Small** action toggles a user-global preference that renders every
Details and note-content size from its own ordinary semantic size, including paragraphs,
lists, headings, quotations, code, tables, activity text, and edit fields, at exactly 1.5×.
Its enabled state uses the shared pressed-toolbar treatment around the icon. It does not
enlarge the ticket title, tabs, note metadata, or other reader chrome. The preference is
remembered when the reader closes and applies the next time any ticket opens.
The sidebar inspector and reader each own and persist their selected tab for the project.
Changing one surface's tab never changes the other; reopening or restoring the project
returns each surface to its own last selection. Feedback-response entry intentionally
opens the reader's Info tab without disturbing the sidebar selection.
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
work. Independent projects retain independent mutation queues. Attaching a ticket source,
including after HS1 import, refreshes provider descriptors before exposing the imported
tickets for mutation.

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
same warm click-to-DOM ceiling applies to an authoritative new-ticket response.

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
Value groups have no outer border, use text-aligned inset row separators, and retain a
visible gap between repository identity and synchronization groups.
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
