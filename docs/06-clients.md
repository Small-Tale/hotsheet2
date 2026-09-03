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
  menu. Production handlers cover reader opening, category/status/priority changes, batch
  Up Next, add/remove tag, duplication, archive, and confirmed soft deletion. Every bulk
  metadata operation is submitted as one checkout-scoped atomic batch request (never one
  request per selected ticket), with all concurrency tokens validated before any write.
  metadata/tag/delete write is provider-capability gated and carries the freshly read opaque
  concurrency token; a stale ticket fails instead of overwriting a collaborator's edit.
  The complete selection remains one field-aware Undo transaction. A capture-phase,
  composed-path-aware outside pointer-down dismisses the menu reliably across native and
  Web Awesome shadow-DOM controls (including an ordinary click on another ticket row),
  while interactions inside the menu remain open; Escape also dismisses it. A
  single completed selection also exposes Verified and Not Working. The latter accepts
  notes and/or attachments and submits them through one provider-neutral operation that
  atomically appends the note, publishes all evidence, records an attributed timeline
  activity with a concise single-line summary, and returns the ticket to Not Started +
  Up Next. The actor comes from the store's git `user.name`; when no name is configured,
  the timeline uses an unattributed `Reported as not working` label. The explicit
  `not_working_report` capability hides the action for
  providers that cannot guarantee all-or-nothing behavior; the client never emulates it
  with uploads, patches, or compensating deletes. Completed/verified selections never
  offer Up Next.

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

## 6.3 Web client and Tauri desktop host

- **Web first.** Build and iterate on the Kerf web client in a normal browser before
  adding its Tauri host. A `/ux-demo` route renders the real production components
  against deterministic mock service adapters for isolated and composed UX review.
  Keep the platform-neutral component responsibilities close to the planned macOS
  SwiftUI architecture; share concepts and API contracts, not rendering primitives.

- **Stable local development by default.** `npm run dev` in `clients/web` copies the
  package into a temporary snapshot and starts Vite there. The running app retains the
  development bridge and `/ux-demo`, but concurrent edits in the checkout cannot trigger
  HMR or expose a partially edited multi-file state; restart the command to load a new
  snapshot. The launcher passes the original repository root into the snapshot so the
  project bridge still resolves the real `target/debug/hotsheet-server` rather than a
  nonexistent temporary `target` directory. Use `npm run dev:hot` only when actively developing the web UI and immediate
  HMR is desired. Browser tests use `dev:hot` on a separate default port and never reuse
  an already-running maintainer server. On shutdown, the stable launcher first waits for
  its Vite child to exit, then removes the private snapshot before the launcher itself
  exits; this prevents concurrent Vite writes from racing snapshot cleanup.

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
  transitions and no-op interactions. In particular, activating an already-selected,
  fully loaded ticket again performs no detail request, render pass, or DOM mutation and
  preserves focus plus draft state in an active editor. This makes broad Kerf render
  dependencies fail loudly instead of becoming
  focus, scroll, or animation regressions. Development builds enable Kerf's
  value-only-render and list-rebind warnings plus throwing list invariants.

- **Field-aware live editing.** A ticket refresh merges fields that the user is not
  editing immediately. An active text draft adopts a remote-only update when still
  untouched, preserves a local-only edit, and stays quiet when both sides converge.
  Only divergent changes to that same active field open a reconciliation surface with
  the remote and local versions plus an editable merged value. Whole-ticket concurrency
  token failures use the same comparison: unrelated field drift retries once against
  the fresh token instead of presenting a false conflict. Background refresh also leaves
  an in-flight or queued autosave draft alone; the write response and token retry path
  distinguish this client's earlier partial save from a genuinely competing edit.
  The freeform blocked reason uses the same silent blur-flush path: a non-empty reason
  persists, an empty edit sends `null` to clear it, and the authoritative response exits
  editing without making the text disappear. An existing blocked-reason surface enters
  that editor on double-click, matching details and ordinary notes. Existing blocked
  reasons and ordinary notes do not add redundant per-item Edit buttons; their content
  surfaces support double-click plus Enter/Space keyboard entry instead.

- **Active ticket work.** Ticket rows show a slow yellow two-dot activity animation
  directly after status only while a worker holds a non-expired claim lease. Started
  tickets without a lease remain visually idle, and old `claim_count` values never
  imply presence. A local one-shot expiry timer removes stale indicators without issuing
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
  Right-sidebar toolbars are divider-free in every state—ticket, loading, multi-selection,
  Settings, and Notifications—so content sections, not the shell toolbar, own separators.

- **Persistent shell splitters.** The project sidebar and ticket inspector are
  independently resizable by pointer or keyboard. Dragging updates only splitter
  geometry until release, then persists the bounded width locally so a reload restores
  the layout without creating broad render churn.

- **Real local web entry point (initial implementation, HS2-0P1MDG).** `/` renders the
  production AppShell over checkout-scoped server APIs; `/ux-demo` remains the isolated
  development catalog. The project-tab `+` action opens a code-checkout dialog. On first
  open, the server conservatively discovers a valid sibling `<checkout>.hs2` git ticket
  store, hosts it, and records the many-to-many checkout/store link. The dialog accepts
  an explicit git-store path when the convention does not apply. The Vite-only bridge
  discovers or detached-starts the local server and keeps its bearer credential out of
  browser state; Tauri will replace that bridge with its native lifecycle layer.
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
  an old client offers reload/update, while an old server is surfaced without an unsafe
  restart action unless both restart and quiescence capabilities are explicitly present.
  Every compatibility warning's **View details** action opens an accessible build-details
  dialog. It reports the running server application version, build revision, current local
  source revision, client revision, both protocol ranges, and server start time when the
  authenticated handshake supplied them. Missing values are labeled rather than guessed.
  Recovery guidance distinguishes safe compatible skew, stale local source, old client,
  old server, and unavailable metadata; it never offers automatic restart without the
  same explicit restart plus quiescence capability gate.
  Ticket-provider connections are not stored in `hotsheet-settings.json` or
  `hotsheet-settings.local.json`: those remain shared/local preferences. Git sources are
  checkout/store links in the machine registry, while external provider connections are
  non-secret records in the ticket store's `providers.json` (credentials remain keychain
  references). Full multi-source editing in the real Settings view is tracked separately;
  the initial view reports the active source paths and their storage model.

  Workspace search delegates to the checkout index rather than filtering compact rows
  in the browser. It therefore matches slug, title, tags, Markdown details, and note text
  while retaining the full local ticket collection for project counts, mutations, and an
  immediate return to the unfiltered view when search is cleared. Search semantics do not
  inherit the selected sidebar view: ordinary queries cover the normal working statuses,
  exact-slug lookup can reveal Backlog/Archive/Deleted matches, and explicit scope/filter
  chips opt into normally excluded lifecycle states. Reference-mention matches must say
  why they matched. HS2-383D6K owns the remaining advanced-search client work.

  Ticket details and notes share one Markdown rendering boundary in the inspector, reader,
  and UX demos. Every link emitted by that renderer opens in a new browser tab and carries
  `noopener noreferrer`; raw HTML remains escaped and unsafe URL protocols remain inert.
  Blockquotes follow email-reply semantics: a compact neutral rail, smaller quiet text,
  and inherited heading size de-emphasize quoted context instead of presenting it as a
  literary pull quote.

  The shared left project sidebar presents a centered `M open, N up next` summary
  immediately above Drive. Both counts derive from the existing checkout ticket collection:
  open means exactly Not Started + Started (Backlog is not active work), and Up Next
  additionally requires the Up Next flag. Mutations and long-poll-driven collection refreshes
  update the summary reactively; the summary itself performs no polling or network request.

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
  When the selected provider advertises attachment support, each attachment exposes
  icon actions to open, download, copy its checkout-qualified reference, or remove it;
  every icon action has an action-and-filename accessible name, matching hover title,
  and visible hover/focus feedback. Double-clicking the attachment row invokes the same
  open path as its Open icon, while double-clicks on the other action buttons remain
  scoped to those buttons. Upload/removal progress and failures remain visible in the
  attachment panel. Browser clients use download where a native Tauri host can later
  offer Reveal in Finder.

  The inspector includes a Code Review segment for ticket-associated code history. It
  lists each matching commit subject, abbreviated SHA, and date even when no review tool
  is configured. When the checkout has a Git `diff.tool`, each commit has an Open action
  and each adjacent multi-commit run has its own bundle action with explicit oldest/newest
  boundaries. Multiple disjoint runs therefore remain separately reviewable; interleaved
  unrelated commits are never silently included. The inspector tab uses the Lucide
  `message-square-code` icon while individual commit and range actions retain their
  established icons. Commit rows are flush with the review list instead of inheriting
  the component library's native list-item indentation. Loading and launch errors stay in
  the segment and do not replace ticket content or use the foreground project-loading
  indicator. All discovery, target validation, and process launch remain server-owned.

  Project refresh loads healthy tickets and checkout-scoped corrupt-ticket diagnostics
  independently. Live diagnostics supersede any stale indexed row with the same recovered
  slug, so selecting that visible ticket always opens recovery instead of retrying a doomed
  ticket fetch. The server watcher emits the existing replayable `changed` event even
  when a newly malformed file cannot be reindexed, so this replacement happens through
  the normal long poll without manual reload. A malformed file therefore cannot suppress healthy rows: the workspace
  remains usable and renders each unreadable file as a selectable warning row with the
  recovered slug/id or filename and failure state. Selecting it opens the normal
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
  the new ticket to backlog status. Column presentation always gives TicketRows a
  rounded visible border, including the wide single-column Backlog and Archive boards.
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

  Workspace chrome preferences are device-local browser state. The client restores the
  last view mode, sort field and direction, project-sidebar and inspector visibility,
  project-command group expansion, and independently clamped sidebar/inspector widths
  across reloads. Missing, partial, malformed, or unknown enum values fall back per
  field to safe defaults rather than preventing project open. Selecting a ticket still
  reopens the inspector and persists that explicit state transition.

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
- Multi-project tabs (local + remote).
- Search (FTS) and filtered views.

The web client now implements the permission portion of that floor for Claude and
Codex. Every open project has a replay-safe long poll; a `permission_asked` event
triggers one fetch of that project's authenticated permission and connection state. A
replayable `permission_resolved` event triggers the same reconciliation, so decisions made
by another client or transport become history entries instead of silently disappearing;
empty-action generic requests such as `ToolSearch` follow the same lifecycle.
There is no fixed-interval network polling. Pending counts appear in the main segmented
control and project tabs, and a non-modal popup appears even when another project is
selected. The global Notifications view
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

The long tail of HS1 UI (custom views/query builder, stats, Announcer, telemetry
dashboards, print) remains **deferred**, each its own ticket after the floor lands. The
terminal dashboard is now active work: HS2-946EQG settled its interaction contract from
the updated project/drawer wireframes, and HS2-2ZCN7K implements it.

## 6.7 Terminal display & multi-viewer PTY sizing

> **Status: the server-side arbiter is built (HS2-BD7Q74).** The cross-device
> generalization of HS1's terminal "borrow-stack" (docs/54), which worked locally but was
> never designed for remotes. `hotsheet-terminals::SizeArbiter` implements the model below —
> leased viewport claims, focus-follows (default) + smallest/largest/pinned, the
> `SIZE_FOCUS_HOLD`/`MIN_DELTA`/`RESIZE_MIN_INTERVAL` guards, and disconnect self-heal — wired
> into the WS attach (Text `{resize}` claims in, `{pty_size, driven_by}` decisions out). The
> **client-side** viewport rendering (§6.7.4 letterbox/scale-to-fit, "tap to resize") lands
> with the client work.

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
- `SIZE_RESIZE_MIN_INTERVAL_MS` (~750 ms) — rate-limit actual PTY resizes; coalesce
  bursts.

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
no history path or command content is committed to a ticket store. Implementation is
tracked by HS2-A5V801.

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
- **`regular` and `status` notes, and the ticket `details`, use the same direct editing
  affordances in the reader as they do in the inspector.** Rendered Markdown owns its
  own whitespace; note containers must not preserve the renderer's HTML formatting
  whitespace, which would create artificial blank lines between list items.
- **`activity` notes render as a chronological timeline** ordered by `created_at`
  (ULID tie-breaker). Never collapse duplicate, repeated, or reversed transitions;
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

**Feedback needed is needs review.** These are one user-facing concept, not competing
ticket states. A `feedback_needed` note and an explicit review request both project to
the same "Needs review" badge and purple leading rail in list and column presentations.
The inspector/reader uses that same rail and "Needs review" banner; the underlying note
still carries the specific question and feedback editor. The unified needs-review rail
takes precedence over blocked and Up Next rails so the outstanding decision is never
hidden. The server's compact row continues to expose the source `feedback_needed`
boolean (mirrored in the index), while the client normalizes it at presentation time.
For note-driven feedback, only an unanswered ask is active: among regular and
`feedback_needed` notes, the most recent one controls the state. A later regular note is
the response and clears Needs review; activity/status notes are neutral, and a later
`feedback_needed` note opens it again.
For compatibility with early HS2 automation, a regular note whose trimmed text begins
with the historical exact marker `FEEDBACK NEEDED:` is normalized to the
`feedback_needed` kind at the wire boundary and participates in the same exchange.
New core writes promote that marker to the first-class kind. Rebuilding an older
disposable index must therefore recompute the compact flag for unchanged ticket files.

**Reader mode is a directly editable focus surface.** Opening reader mode shows the
ticket's details + notes on one large scrollable surface with no separate top-level edit
mode. The reader uses the available browser height with exactly 24px of backdrop above
and below; it has no desktop-height cap that leaves unnecessary vertical space. Details
and ordinary notes expose their normal edit interactions immediately.
Leaving the details editor flushes its pending autosave and returns that surface to
preview. Closing the reader performs the same save-and-exit transition, so its shared
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

CI protects the deterministic contract (one PATCH and no follow-up collection/status
GET) and the projection/reconciliation logic. `npm run test:performance` is the stricter
local browser gate: after warm application startup, its click-to-next-frame projection
must remain below 100 ms; 33 ms is the aspirational two-frame target. Network/disk/git
completion is reported separately and does not delay acknowledged visual feedback.

## 6.10 Cross-references
- UX component inventory and `/ux-demo` contract: [ux-components.md](ux-components.md)
- Server-side PTY manager that hosts the arbiter: [05-ai-tool-plugins.md](05-ai-tool-plugins.md) §5.4
- The server clients talk to + its auto-start lifecycle: [04-core-server-cli.md](04-core-server-cli.md) §4.3.1
- Remote/mTLS + mobile pairing: [08-distributed-and-remote.md](08-distributed-and-remote.md)
- Leased-claim pattern reused for size claims: [05-ai-tool-plugins.md](05-ai-tool-plugins.md) §5.7
- Why clients don't embed the core / language rationale: [09-technology-decisions.md](09-technology-decisions.md) §9.2
