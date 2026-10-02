# UX component catalog

> **Status: initial inventory; implementation started.** The development-only Hono
> `/ux-demo` catalog shell and production `TagChip`, `StatusBadge`, and initial
> `TicketRow` components are built in `clients/web` (HS2-61XG15/HS2-H0T0MV/HS2-RP0FKP). This
> catalog is derived from the
> [list-mode wireframe](design/exports/Main%20Interface%20Wireframe%20-%20List%20Mode.png),
> the structural SVG export beside it, and the client requirements in
> [06-clients.md](06-clients.md). The wireframe establishes information architecture,
> not final color, type, spacing, or density.

The [Kerf UI design philosophy](ui-design-philosophy.md) governs quality tradeoffs,
state continuity, semantic reuse, and the review gates for these components. The
[design guidelines](design-guidelines.md) supply the concrete platform rules.

Hot Sheet will build the browser implementation first for rapid UX iteration, then
host it in Tauri and implement a closely corresponding native macOS SwiftUI client.
The browser exposes the real production components at `/ux-demo`, backed by a
deterministic mock service. Demo interactions should communicate intent even before
their server integration exists.

## 1. Shared component architecture

Web and SwiftUI should share the same **conceptual component tree**, responsibilities,
state boundaries, and user-facing vocabulary. A `TicketRow`, `TicketInspector`, or
`TerminalDrawer` should mean the same feature on both platforms. They do not need to
share rendering mechanics or reproduce one another's primitive hierarchy.

| Feature component             | Web implementation                                   | macOS SwiftUI implementation                      |
| ----------------------------- | ---------------------------------------------------- | ------------------------------------------------- |
| Application shell and regions | Kerf composition, CSS grid/split panes               | `NavigationSplitView`, split views, window scenes |
| State and API resources       | Kerf signals, array signals, resources               | observable models and async API services          |
| Standard controls             | Cherry-picked Web Awesome Core elements              | Native SwiftUI controls                           |
| Lists and selection           | Kerf virtualized list + semantic rows                | `List`/lazy containers + native selection         |
| Menus, dialogs, drawers       | Web Awesome primitives where suitable                | Native menus, sheets, popovers, inspectors        |
| Terminal viewport             | Imperative terminal widget behind a Kerf `ref`/scope | Native terminal surface wrapper                   |

Rules for both clients:

- Components receive typed view state and emit semantic actions; they do not call
  provider-specific ticket APIs directly.
- Provider capabilities determine whether an action is shown, disabled, or explained.
- Server data is authoritative. Optimistic client state must reconcile with HTTP and
  WebSocket results.
- Selection, focus, keyboard commands, accessibility labels, loading, empty, error,
  disconnected, permission-denied, and unsupported-capability states are component
  responsibilities—not afterthoughts added only to a screen.
- Platform conventions may change presentation while preserving the same feature
  boundary and outcome.
- All decorative/symbolic iconography uses official Lucide icons through a shared
  platform renderer. Never use emoji, geometric Unicode characters, dingbats, or
  other font glyphs as icons. Hide decorative icons from accessibility when adjacent
  text carries the meaning, and name icon-only controls. Ask the maintainer when
  multiple Lucide metaphors are materially plausible.
- Cursor semantics communicate the interaction under the pointer: pointer for
  clickable/selectable targets, text for editing, not-allowed for disabled controls,
  appropriate grab/resize cursors for direct manipulation, and platform default for
  non-interactive content. Style Web Awesome controls through documented CSS parts.

## 2. Application shell and navigation

### 2.1 `AppShell` — feature floor, demo built

The **built demo** composes the current production sidebar, tabs, connection banner,
header, ticket workspace, and inspector into a desktop shell. The supported AppShell
floor is **1024 × 600 CSS pixels**; native hosts must enforce the
same minimum window content size rather than asking the shell to compress below it.
The production browser shell inherits that floor from the shared AppShell stylesheet;
screen-level CSS must not replace it with a smaller minimum.
AppShell configures the components it composes through props rather than cross-component CSS
(HS2-DR549A):

- **`presentation`.** `framed` (the default, as the UX demo stages it) draws a bordered window. `viewport`
  fills its container edge to edge; `MainShell` passes it for the application root, so no global
  stylesheet restyles the shell.
- **`workAreaFocusRing`** (default `true`). The runtime turns it off while a magnified terminal, a
  top-layer overlay hosted inside the work area, owns focus, so the ring never shows through that
  overlay's scrim.
- **Project strip.** AppShell renders `ProjectTabBar` with `surface="default"`. It sets
  `divider` only in terminals mode, where no page header sits between the strip and the work area.
- **`--hotsheet-scroll-end-inset`.** The workspace sets `data-bottom-edge` from its own `mobile`
  and drawer props. It publishes this app scroll-end token: the home-indicator inset while the
  phone column reaches the screen's bottom edge, else `0px`. The workspace and each
  `TicketBoardColumn` scroller add it to their own bottom padding and scroll padding (HS2-4A29RR).

Sidebar and inspector splitters are keyboard/pointer adjustable, remain present until
the user explicitly collapses them, and never auto-hide at viewport breakpoints. Sidebar
regions never resize below 250px. Production pointer drags update splitter geometry once per animation frame and
commit a single render when released; the active region carries Kerf's `data-resizing`
state so its content motion is suppressed without application CSS. Pointer and keyboard sizes persist locally across
reloads. Both sidebars animate between visible and collapsed states; their restore
controls live at the matching leading/trailing edges of the center-column toolbar,
never in ProjectTabBar. Start-edge inspector resizing correctly mirrors end-edge sidebar resizing. Composed
WorkspaceHeader search retains its expand/autofocus/filter/empty-blur
contract. Project settings replace the project-summary sidebar with an HS1-style
category navigator for Ticket sources, Commands, Permissions, and Column view; the
selected category alone occupies the workspace and names the shared page header without
a duplicate workspace title. Settings shows the standard empty inspector placeholder and
disables ticket-view actions.
Cross-project Stats hides both project-scoped regions. Global Terminal Dashboard replaces
the project sidebar with a resizable operations sidebar and keeps the ticket rail at the
right; both regions remain explicitly hideable and restorable. It owns the top-level arrangement.
The project-scoped list/column workspace also composes the real `QuickTicketComposer`
immediately above its ticket collection, matching the wireframe; settings and global
dashboard modes omit it.
AppShell composes Kerf's `Workbench` (`id="app"`, HS2-P289N2): the sidebar is the
`leftRail`, the inspector or ticket rail the `rightRail`, and the terminal drawer the
`bottomDrawer`, each configured through the panel contract — `separator`,
`collapseMotion="slide"` (rails) or `"fade-slide"` (drawer), inline/overlay `presentation`
for the app's own mobile switch (`responsiveOverlayAt="never"`), popup-aware
`contentOverflow`, `resizable` bounds, and a `restoreControl` in the drawer's
`restorePosition="bottom-end"` corner. The Workbench derives the panel ids
`app-left-rail`, `app-right-rail`, and `app-bottom-drawer`; `wireWorkbench` drives and
persists the rails' sizes, while the drawer keeps the app's own drag for its measured
maximum and drag-past-minimum collapse. The left rail is a Workbench toolbar panel
(HS2-RWGQWN): `ProjectSidebar`, `SettingsNavigation`, `NotificationNavigation`, and
`TerminalOperationsSidebar` each export a `…Panel()` builder returning `SidebarPanelParts`
(`src/components/sidebar-panel.tsx`: toolbar, standard `toggle`, content, footer, Pane
config), the shell spreads them into the rail through `workbenchSidebarPanel()`, and the
Workbench renders the panel's toolbar with the collapse toggle and relocates that toggle into
the workspace toolbar while the rail is collapsed; the standalone components render the same
parts as a Kerf `Pane` through `SidebarPane` for the catalog. The right rail is a Workbench
toolbar panel too (HS2-QQW6CT): `ticketInspectorPanel`, `ticketInspectorSkeletonPanel`,
`ticketInspectorPlaceholderPanel`, `notificationInspectorPanel`, and `corruptTicketInspectorPanel`
return `SidebarPanelParts` with an optional fixed `header` (Kerf `KF-ZBW7MS`) and the shared `inspectorToggle()` (`toggle-ticket-inspector`), routed by
`ticket-inspector-surface.tsx`; the shell spreads them into the right rail and composes no Show
inspector control of its own. Because the Workbench owns the panel's Pane root, the inspector's
identity (`data-ticket-slug`, `data-needs-review`, `data-presentation`) and attachment drop
target live on its app-owned `ticket-inspector-header` and `ticket-inspector-body` wrappers, and
inspector CSS scopes to those wrappers. The standalone components (`TicketInspector` for the
reader modal and the catalog) render the parts inside their `[data-component]` card through
`SidebarPane`; `collapseControl` mirrors the rail toggle in the catalog. `terminalTicketRailPanel`
returns `NavigationPanelParts` instead (HS2-FY06N4, Kerf beta.66 `KF-WW33YJ`): the shell's right
rail becomes a `WorkbenchNavigationPanel` (through `workbenchRailPanel`) whose controlled NavStack
has a root view (project selector in `leading`, controls and heading as the pinned `header`, the
ticket collection on the sunken scroll surface) and, once a ticket is open, a `ticket:<slug>`
view built from that ticket's `ticketInspectorPanel` parts. The runtime wires `wireNavStack` to
the mounted stack, and its Back pops the app-owned rail screen. `mobile-side-panels.css` is gone.
Magnified terminal mode suppresses side-panel
separators through the typed policy, and terminal focus mode lifts the drawer through the
public `--kui-workbench-popup-z` token instead of descendant CSS. The main column (shell
toolbar, project tab strip, banners, page header, work area) stays app-owned as the
Workbench's `main` (HS2-4Y6SM9).
The composer wrapper owns equal top and bottom inset around the creation surface. When
it is present, the scrolling workspace removes its own top padding so list and board
presentations receive one gap rather than two; composer-free settings, Archive, and
global surfaces retain the workspace's normal top inset. The board remains edge-to-edge
on its horizontal and bottom edges.
The composed project sidebar uses the same white surface as the inspector, while the
ProjectTabBar adds no redundant background or top separator. Busy indicators preserve
their visual center throughout rotation. The composer title input and category select
share one control height.
and responsive behavior of:

- `ProjectSidebar`
- `WorkspaceHeader`
- `ProjectTabBar`
- `Workspace`
- `TicketInspector`
- `BottomDrawer`
- global overlays and notifications

Supporting components:

- `ResizableRegion` — **demo built**: horizontal and vertical accessible splitters
  with pointer/keyboard sizing, clamped ranges, and collapse/restore without losing
  the restored size. The demo wires its regions through Kerf's `wireResizableRegions`
  (HS2-KB5PJQ): pointer drags update the region's CSS geometry directly and commit
  reactive state only on release (`onCommit`), so large ticket collections are not
  rebuilt for every pointer event; arrow keys step 16 px, Shift+arrow 64 px, and
  Home/End jump to the bounds. Sidebar, inspector, and bottom-drawer sizes are local
  UI state. The catalog's tab strips likewise use the application's `wireTabBars`
  wiring (manual activation, drag and Alt+Shift+Arrow reorder, edge autoscroll).
- `ConnectionStateBanner` — **demo built**: connecting, reconnecting, offline,
  incompatible-server, and authentication variants with state-specific Lucide icons,
  live-region semantics, details, and relevant recovery actions.
- `ConnectionDetailsDialog` — **demo built**: every compatible-skew, stale-source,
  old-server, old-client, and unavailable-metadata state using the shared Toolbar heading
  and `ValueTable` presentation. Safely recoverable details dismiss through the native
  popover light-dismiss behavior rather than a redundant Close action. Its body uses the
  canonical 24 px major-region rhythm; narrow metadata rows use 8 px insets and 4 px between
  their connected key/value pair (HS2-4Y6SM9).
- `ProjectDialog` — **demo built**: local folder/store inputs and remote server-known
  checkout choices retain native Web Awesome dialog/input/button behavior while the
  app-owned composition uses 16 px dialog spacing, 8 px field/action clusters, and 4 px
  connected remote-row text/list spacing. Remote choices use multiline ListItem rows with
  shared padding and character-wrapped full paths in a width-constrained scrollable list.
  The interactive demo covers local/recovery/busy and remote populated/loading/empty/error
  transitions, reopening, and keyboard selection (HS2-XX5Y2X).
- Dialog/page heading compositions and `ValueTable` — **demo built**: direct divider-free
  `Toolbar` rows put an optional icon `ToolbarControlGroup` and extra-large `ToolbarText`
  in the leading zone, with named grouped controls in the trailing zone. Supporting copy
  is app-owned content below the toolbar; host `aria-labelledby`/`aria-describedby` point
  to the retained title and summary ids. `heading.css` owns only the icon tone and summary
  alignment, while Kerf owns row/control geometry. The framework-neutral Dev Review
  overlay emits the same Toolbar/ToolbarText anatomy and imports its canonical styles.
  `ValueTable` supplies the bordered metadata surface while `ValueTableRow` supplies aligned
  label/value geometry and inset separators; consumers do not hand-author raw definition-list
  rows; the connection-details, HS1 migration, and catalog compositions all use that row
  contract directly (HS2-1YABZ8). These compositions use APIs available in beta.24 and do
  not depend on the removed upstream heading wrapper (HS2-AGDJ6E, HS2-72Z7CB).
- `GlobalDropTarget` — routes supported ticket, attachment, and cross-store drops
  (HS2-R6P8MZ).
- `FocusCoordinator` — predictable keyboard traversal and restoration after overlays.
- `WindowChrome` — native traffic lights/titlebar accommodation in Tauri/macOS; absent
  or adapted in an ordinary browser.

### 2.2 `ProjectSidebar` — feature floor, demo built

The left region in the wireframe, scoped to the selected project/store connection.
The **built demo** composes the five production boundaries below into a full-height
sidebar with the drive action anchored at the bottom and shared controlled state. In
AppShell, its collapse control sits in a shared Toolbar exactly matching the center
and inspector toolbar height/padding, without the intentionally omitted bottom divider.
The sidebar's trailing separator remains visible. Collapsing slides the fixed-width
sidebar content offscreen while the main region resizes, avoiding compressed content,
and moves the restore control to the leading edge of the main toolbar. A
direct horizontal resize handle changes the demo height by pointer or keyboard so the
scrolling content region can be reviewed without moving the Drive control.
The collapsed terminal drawer's floating restore action is likewise positioned by the
main column rather than the viewport, keeping it aligned with the workspace when the
inspector is open (HS2-3ZGWMN).
All project-sidebar content follows two explicit rails: full-row highlight layers are
inset 8px from each sidebar edge, while visible text and icon slots use an 8px content
inset inside that layer. Transparent idle rows retain the same highlight geometry.
With Kerf beta.24, menu icons occupy an 18px slot and labels follow after 8px.
Standalone toolbar/footer icons retain a 44px highlight target even when their
background is hidden (HS2-2P8N8D).
Immediately above Drive, a centered `M open, N up next` summary is derived from the
already-loaded project tickets. Open is the active-work axis (exactly Not Started +
Started; Backlog is excluded), while Up Next counts only workflow-open
tickets carrying the Up Next flag. It updates with the same reactive ticket collection and
does not introduce polling or another network request.

- `ProjectSummary` — **demo built**: typed seven-day ticket-completion trend,
  completed-today count, and current in-progress count. The ambiguous day-over-day
  percentage from the wireframe is intentionally omitted. Zero-completion days retain
  a one-pixel neutral baseline mark so all seven day positions remain visible. Production
  derives each bar from ticket completion timestamps in the corresponding local calendar
  day. Its optional explicit chart maximum lets a comparison owner place several instances
  on one shared numeric domain while retaining safe local scaling elsewhere. Brand and success
  chart-tone variants let an aggregate read separately from its constituent projects. An optional
  aligned background trend draws a slightly wider neutral bar behind each foreground bar, so a
  project can show its contribution against an aggregate without adding another chart. A `size`
  variant is `default` or `compact`: compact is a shorter (68px minimum, 44px chart) summary with `--kui-space-s`
  padding, used for the per-project groups stacked in the terminal operations sidebar
  (HS2-4APEJP). The tones, the background trend, and both sizes appear in the catalog. Hosts inset
  the summary with their own wrapper, as the project sidebar's `project-sidebar__summary` does, and
  never restyle its root. The whole summary is an accessible action that opens the selected project's
  statistics surface; HS2-38RJMK owns the full charts behind the current placeholder.
  - `ProgressSparkline` / compact status histogram
  - completed and in-progress counts
- `TerminalOperationsSidebar` — **demo built**: the Terminal Dashboard's left rail
  reuses `ListHeader` and a `size="compact"` `ProjectSummary` for each open project. When two or more
  projects are open, an `All projects` group precedes them and sums aligned trend days,
  completed-today counts, and in-progress counts. The maximum summed daily value becomes the
  shared scale for the aggregate and every constituent chart, making cross-project bar heights
  directly comparable. The aggregate remains a standalone success-green chart; every brand-blue
  project chart repeats the aggregate trend as a neutral-gray background silhouette, keeping the
  absolute scale visible at each day and project. Each summary opens statistics for
  that project; the aggregate opens cross-project statistics. The component consumes
  the already-loaded ticket rows and does not introduce polling or network requests.
- `RepositorySummary` — **production built**: one accessible branch/status action with
  unpushed and uncommitted counts. The uncommitted count remains text-only to avoid
  crowding the already icon-rich row.
  - `BranchChip`
  - unpushed and uncommitted counts
  - opens the production repository-status master/detail dialog with value-cell branch
    metadata; scrollable staged/unstaged/untracked/conflicted file views with Git-letter
    badges and middle-truncated paths;
    double-click, keyboard, copy-path, and host-native reveal actions; and a Commits view
    that reuses Code Review difftool/range presentation. The master views and detail
    files compose the shared `ListHeader` and `ListItem` primitives, and the complete
    dialog is represented as an embedded production component in `/ux-demo`
    (HS2-RPVFA4, HS2-323XHG, HS2-Z0TSX4). Its roomy panes use 24px insets, constrained
    panes and sibling value groups use 16px, menu rows and empty-state clusters use 8px,
    and connected path/menu details use 4px. The shared view rows remain gapless below a
    4px-separated `ListHeader`; the pointer-anchored file-action menu stays app-owned
    because no catalog primitive owns that host-action positioning contract (HS2-4Y6SM9).
    Its catalog-only example stack and fixture wrapper also opt into shrinking, keeping the
    embedded dialog and both heading toolbar actions inside the detail pane at 1280px and in the
    constrained layout without changing production popover sizing (HS2-MCHTAW).
    At phone widths up to 600px, view navigation stacks above the independently scrollable
    detail pane; repository metadata remains reachable by scrolling the navigation pane,
    and the A/B comparison prompt and Open action remain visible at 390px (HS2-B2MD8Z).
- `RepositorySetup` — **production built**: initialize a project folder's Git repository and
  optionally connect its origin without staging, committing, or pushing project files. Its major
  surface and section rhythm use the canonical 24 px step, with 16 px between the icon/message
  columns and 8 px inside error and action groups (HS2-4Y6SM9).
- `ChangeEvidenceDialog` — **production built**: the actionable Code Review evidence
  summary opens a repository-style master/detail dialog whose Docs, Tests, Source, and
  Other views reuse `ListHeader`, `ListItem`, middle-truncated paths, and Git-letter
  badges. Selecting a file opens its diff across the server-validated complete ticket
  commit span. It shares the phone-width stacked navigation and detail layout with
  RepositorySummary (HS2-B2MD8Z). The dialog has a standalone interactive `/ux-demo` route (HS2-S7X4SB).
- `ListItem` — **demo built**: the shared icon, label, trailing-value, and full-row
  selection grid used by repository, view, and command actions. This keeps icons,
  labels, and interaction boundaries aligned across menu-like sidebar surfaces. Its
  canonical standalone demo route is `/ux-demo?component=list-item`.
- `ListHeader` — **demo built**: shared section-label alignment with ListItem icons,
  including an optional subtle count badge, trailing actions, popover-targeted actions,
  and whole-header disclosure variants. Notes uses the shared count slot instead of
  concatenating a bare number into its label (HS2-FYCAZC). Its canonical standalone
  demo route is `/ux-demo?component=list-header`.
- `ViewNavigation` — **demo built**: icon-bearing views, counts, attention, add-view
  action, and controlled selection through adjacent `ListItem` rows. The section uses
  Kerf's 4px tight-cluster spacing while the shared rows remain gapless.
  - section heading and add-view action
  - `ViewNavigationItem` with icon, title, count, selection, and attention state
  - built-ins: Needs Review, Queue (active tickets), Backlog, Archive
  - user-defined views when custom-view support lands
- `CommandNavigation` — **demo built**: collapsible group of palette-colored,
  icon-bearing command actions with controlled running state. Colors are constrained
  to the exact shared HS1 custom-command palette, including contrast-aware neutral.
  Configured HS1 Lucide names are preserved (`send`, `file-text`, `arrow-left-right`,
  `soap-dispenser-droplet`, `circle-check-big`, `balloon`, `git-compare`,
  `git-compare-arrows`, `wand`, and `globe`) instead of being replaced by the command-kind
  fallback; missing or unsupported names still use the deterministic send/test/build fallback.
  - `CommandButton`
  - `CommandGroup` with collapsible heading
  - running, stopping, last-run, success, and failure states
- `DriveControl` — **production + demo built**: split workflow action with explicit
  tool and running semantics, preceded by the centered open/Up Next project summary.
  The primary side opens/reuses a dedicated drawer chat and runs `$hotsheet`; the arrow
  side opens `DriveOptionsMenu` for Default or plugin-discovered provider/model/effort.
  - primary launch/resume action; duplicate activation is disabled while busy
  - active tool/connection state from the shared long-poll event stream
  - `DriveOptionsMenu` — **production + demo built**: hierarchical session override menu
    with one library-owned disclosure marker per parent, compact aligned semantic-icon child
    choices, and a single highlighted current provider/model/effort value
  - `AiToolSettings` — **production + demo built**: machine-local plugin-discovered defaults

- `AIConversation` — **production + demo built**: a project-scoped
  dialog opened from a compact MessageSquare action beside Drive, plus an embedded presentation
  for AI-chat tabs in `TerminalDrawer`. Project Chat prepares the default tool and opens an empty
  conversation without running `$hotsheet`; Drive uses a separate stable connection and explicitly
  starts that workflow in the drawer. It keeps the ticket workspace visible behind a bounded, vertically
  scrollable transcript rather than replacing the project route.
  - header: tool identity, ready/working/message-count context, conversation usage, and
    native Save/Stop buttons inside Kerf `ToolbarControlGroup` in both dialog and embedded
    presentations. Save is disabled for empty or active transcripts; Stop appears only
    while the active connection advertises `interrupt`. The group owns button geometry
    and keyboard focus treatment. The dialog uses light-dismiss/Escape without a redundant
    close action. The demo exposes both presentations and exercises save/interruption
    through its deterministic state (HS2-WXVAF3).
  - session controls: visibly labeled model and effort choices appear only for plugin-declared live-change
    capabilities and apply to subsequent turns; popup hide events stay scoped to
    the nested select and never dismiss the conversation surface; selecting a model revalidates effort and
    removes both the control and request field when that model declares no supported effort levels
  - provider change (HS2-PRBGRB): when more than one provider is configured and the chat is writable, the
    same popup adds a Provider submenu listing the configured tools. Selecting a different provider opens a
    fresh session for it (providers keep separate opaque sessions) and re-seeds it with the prior transcript
    as one framed, read-only context turn ("…do not take any actions based on it yet — wait for my next
    message"), then continues live. No earlier turn is re-executed against the new provider; an empty chat
    switches provider without seeding
  - transcript: ordered, high-contrast user messages and one progressively appended assistant response per
    submitted turn; output chunks update that response in place, unknown additive events do
    not break it, and completed/failed/interrupted terminal state remains attached to the turn;
    messages are always keyboard- and pointer-selectable as an inclusive range, with visible
    selection state plus Copy and Clear actions that preserve ordinary text selection
  - working row: an icon plus specific live text (`Reviewing the project…`, `Responding…`,
    `Waiting for permission…`, or a bounded native-activity description), never a generic
    spinner without explanation
  - permission requests: the existing `PermissionRequestCard` list presentation appears in
    transcript order for this connection and uses the same global decision handlers/history
  - composer: multiline free-form input plus an explicit Send action; Enter sends while
    Shift+Enter inserts a line break; sending is disabled for blank input and while the same
    connection is busy, with the reason exposed accessibly
  - save/reopen: the header Save action reuses the live transcript selection. If a range exists,
    a compact `ConversationExportDialog` offers Entire conversation or the already-selected range
    without presenting another picker; if no range exists it skips directly to the vertically
    stacked set of optional local summary/attachments/original-media
    contents and invokes the host `.hotsheet-chat` folder picker from Save; nested popup hide events do
    not close the wizard;
    dialog regions follow Kerf's canonical 24 px major, 16 px body, 8 px within-group, and
    4 px connected-label spacing relationships (HS2-4Y6SM9); existing
    destinations receive explicit collision handling; structured per-message file references render
    in both live/export transcripts and selected enabled assets retain bytes, MIME type, filename,
    stable id, and range scoping; `TerminalDrawer` can open a saved bundle
    read-only or resume its original session when the saved range ends at the conversation tail
  - read-only contexts: the same complete embedded component renders saved exports and live
    project-close previews. Live previews retain the real transcript/activity/usage/error
    projection, label why editing is unavailable, omit Save and composer actions, and use a
    stable preview identity while users switch between terminal and chat resources. The native
    project-close dialog composes shared `ListHeader`/`ListItem` resource navigation with 24px
    detail, 16px warning, 8px pane/action, and zero-gap connected-row spacing while retaining
    explicit live-preview geometry (HS2-4Y6SM9). The chat preview relies on the `embedded`
    presentation filling its container; the dialog never sizes `.ai-conversation` (HS2-29Q3XG)
  - state: Kerf owns per-connection transcript/draft/open state; the existing replay-safe
    WebSocket/long-poll stream is the only live update source, and no conversation timer
    issues network requests. Reopening a locally closed project reconciles eligible
    server-owned drawer chat connections back into tabs with their live provider, model,
    effort, actions, session, and busy state. Validated transcript/activity state remains
    attached to the stable connection id in device-local storage; an app or server restart
    pairs it with the newest durable provider session before restoring the chat tab
    (HS2-D34C2V, HS2-YHQCS2)
  - public demo states: empty/idle, composing, streaming output, waiting for permission,
    completed, failed, interrupted, and narrow layout; the production ProjectSidebar
    composition exercises open, send, streamed rerender, inline permission, stop, and close
  - each assistant turn ends with a compact usage line when the metrics projection reports
    tokens: input and output remain separately accessible, while the visible total stays
    glanceable. Cost is shown as an approximate USD value only when `cost_usd` exists; an
    unpriced event says “cost unavailable” rather than displaying `$0.00` or inventing precision
  - embedded drawer conversations shrink within the center column across wide-to-narrow viewport
    transitions; viewport roots use non-scrolling clipping so retained composer focus cannot pan
    the application and clip the header or transcript (HS2-KTW27J)
  - the dialog header carries a conversation-total disclosure derived only from those received
    usage events, never a parallel client counter or analytics poll
  - narratable normalized activity for the connection/session appears as a bounded “Activity”
    sequence alongside the transcript. The group carries one persistent, accessible
    “AI-generated summaries may contain errors” cue rather than repeating it on every row;
    row-specific feedback controls retain the originating tool identity. This app-specific
    disclosure/feedback adapter uses the canonical 8px group and 4px cluster rhythm while
    retaining explicit 32px feedback targets. Backtick-delimited
    commands render as wrapping monospace code, and long summaries stay inside the card. Raw
    native activity may drive the immediate working label, but it does not substitute for
    normalized activity entries in this sequence (HS2-4NYP8C, HS2-3NG164, HS2-0YES4Y)

### 2.3 `WorkspaceHeader` — feature floor

`WorkspaceControls` (`clients/web/src/components/workspace-controls.tsx`) renders no wrapper
element: it is Toolbar zone content — the view-mode, sort, and selection
`ToolbarControlGroup`s, the `TicketSearchField`, and the narrow-width overflow menu in its
own borderless nested-dropdown group — so the application shell's header Toolbar and the
demo's `WorkspaceHeader` Toolbar host them as cataloged children, and the workspace-grid
rail's own grid places the same groups (HS2-EZ1N7Z). `WorkspaceIdentity` is a real
`ToolbarText` for the leading zone. Responsive rules key off the component's own
`searchOpen` state: while it is open, the non-rail groups (view switcher, sort, selection
actions, overflow menu) render a literal `--yield` modifier, and a compact (480px or less)
toolbar hides them so the field takes the row. The header asks `TicketSearchField` for
`layout="grow"` and the rail for `layout="row"`, so no stylesheet reads the field's rendered
state or styles it from outside (HS2-8FS5BJ). The yield modifiers and the rail switcher's
full-row width wait on Kerf `KF-6WX6VK` and `KF-GM376R`, and are documented `KUI-L022`
suppressions until then (`HS2-DAMHD1`). Both wrappers are declared to Kerf's composition rule (`rendersAs`) in
`clients/web/ai/component-composition-extension.json`. The mode selector composes Kerf
`SegmentedControl` inside `ToolbarControlGroup`, with
one accessible View mode group and native sequentially focusable buttons. Kerf owns
selection, hover, focus, and segment geometry. Hot Sheet supplies Lucide icons, the
content-sized notification badge (full count in the accessible label, 99+ visual cap),
and routes `data-segment-value` through the existing `set-view-mode` action; legacy
`data-view-mode` producers and the responsive overflow menu keep their contract.
`WorkspaceControls.presentation` defaults to `toolbar` (content-width pill); `rail`
explicitly provides equal-width rounded List/Notifications choices. `listOnly` removes
Columns from the ordinary toolbar and overflow for mobile. The WorkspaceHeader,
TerminalTicketRail, and ToolbarControlGroup demos exercise these compositions
(HS2-F29QAT). The connected WorkspaceHeader demo also uses the shared `PermissionInbox`
model with two local pending requests and an initial history entry. Notifications renders
`NotificationCenter`; its badge derives from the same pending snapshot. Allow, Always Allow
(where supported), and Deny move requests into history. Ignore dismisses prompt attention while
retaining the pending request, matching production. Reset notifications restores the fixture;
counts and decisions persist across List, Columns, Settings, and Notifications transitions
without network requests (HS2-Y70MJY).

- `WorkspaceHeader` — **demo built**: responsive project identity, compact
  all-Lucide Tahoe-style toolbar groups, animated inline expanding live search, a functional
  compact shared `Select` sort control whose popup carries simple direction arrows while
  its tightly spaced trigger uses a semantic field-and-direction icon and accessible
  label without clipping the chevron. Its Web Awesome combobox ring is suppressed in favor
  of one outer-group ring that remains visible while the popup is open and uses a true pill
  radius even after the outline expands beyond the 44px group (HS2-M1DF1D). It toggles
  ascending/descending direction when reselected. Status follows workflow order, ascending
  priority runs from low through urgent, and list/column views remember independent sort
  settings. The demo connects those controls to its list/column/settings/notifications workspace. When its owning
  toolbar narrows, lower-priority utility and sort controls yield first; search and then the view
  switcher yield only at otherwise unusable widths. Every yielded action is relocated into a
  keyboard- and touch-operable overflow menu with the same selected and disabled state, including
  constrained sidebar/inspector shells and 200%-zoom-equivalent widths. Opening Search at the
  narrowest size temporarily gives the toolbar row to the focused field. Actions remain contained
  without clipping downward-opening popovers. The star applies Up Next to the current selection and the
  ellipsis opens the shared TicketRow menu without a duplicate Up Next entry; both are
  capability-aware and disabled with no selection. Settings disables sort, ticket actions,
  and search; global shell modes omit project controls.
- `Toolbar` — **demo built**: shared 56px-high leading/optional-center/trailing layout
  with consistent horizontal padding across the project sidebar, center column, and
  ticket inspector. Trailing content is edge-aligned; when center is omitted, leading
  content owns the flexible space and trailing controls remain pinned right. Its
  bottom divider is an explicit option rather than consumer CSS.
- `FloatingToolbar` — **demo built**: Kerf's accessible, forced-dark overlay toolbar for
  small control clusters that remain over scrolling content without entering the top layer.
  Hot Sheet uses its bottom-end variant for the collapsed terminal-drawer restore action and
  for zoom controls in both the workspace dashboard and drawer Project grid.
  `ToolbarControlGroup` children inherit the toolbar's dark color scheme with their default
  tone, keeping dark surfaces and light icons in both page themes. Applying the group's
  inverse `dark` tone would invert that already-dark scheme back to a light surface
  (HS2-HW02QG). The children own the control chrome; consumers own visibility,
  actions, and safe-area-adjusted positioning (HS2-W3GPHW).
- `ToolbarText` — **demo built**: vertically aligned large, default, and small toolbar
  identity text; project names use large and inspector ticket numbers use small.
- Page heading — **demo built**: current view identity below ProjectTabBar stays separate
  from project-level controls. A divider-free `Toolbar` holds extra-large `ToolbarText`
  with `headingLevel={1}` and the stable page-title id; view actions stay in its trailing
  zone. Dialog titles instead retain their host naming relationships without adding a
  document heading landmark.
- `ToolbarControlGroup` — **demo built**: shared equal-height rounded-border container for toolbar
  buttons, segmented choices, and popup triggers; child controls do not draw their
  own borders or divider lines. A single control highlights the whole group on
  hover; ordinary controls are native buttons with the package-owned 40px circular
  hover/focus geometry. Web Awesome buttons are reserved for package-specific features
  such as popup triggers. Slotted Lucide icons share explicit sizing, block layout, and
  vertical centering across native and Web Awesome buttons. Its borderless appearance
  keeps identical 44px group geometry and 40px button highlights while omitting the idle border
  and background; sidebar visibility and inspector ticket-action groups use it. Its
  orthogonal dark tone provides a shared inverse surface, exact `#353536` border, icon, and hover palette
  for overlay toolbars. Opt-in push buttons retain native `aria-pressed` semantics while
  giving a pressed single-button group the shared dark background, matching border, and
  inverse icon treatment. The demo covers resting and pressed push controls plus dark groups.

These generic primitives, plus `LucideIcon`, `ListItem`, `ListHeader`, `AppTab`,
`ResizableRegion`, `Select`, `StateBanner`, `EmptyState`, `LoadingSpinner`, `Toolbar`,
and `ValueTable` use direct explicit-subpath imports from `@kerfjs/ui`. Hot Sheet keeps
only product compositions that translate domain state or actions into that shared anatomy;
it does not carry local renderer wrappers for the package primitives.
Hot Sheet pins `kerfjs`, `@kerfjs/ui`, and `eslint-plugin-kerfjs` together at
5.0.0-beta.58 and treats the strengthened declaration unions introduced in beta.22 as
integration requirements: Select adapters
choose one accessible-name branch, while ListHeader compositions choose a complete
passive or action branch. This keeps disabled/read-only surfaces semantically passive
rather than emitting incomplete hidden actions. The production shell and UX catalog also
adopt the package-owned `wireTokenSearchFields.onEdit` path so beta 22 remains the sole
owner of editor normalization before the controlled query state rerenders (HS2-HJ585K).

- `ProjectHeading`
- `ViewModeSwitcher` — **built for list, columns, notifications, and project settings**
  with accessible pressed state. The notifications mode projects its pending count in
  the shared warning color and `3xs` typography tokens; its content-sized pill uses
  balanced vertical and horizontal padding instead of a forced height. The full `WorkspaceHeader`
  forwards and demonstrates that state. Settings replaces ticket content while active
  rather than opening a transient popover; later dashboard/analytics modes join the control.
- `SortControl` — shared compact `Select` with aligned option labels and an ascending or
  descending Lucide arrow on the current field; activating the current option again
  reverses its direction.
- `SavedOrCommandMenu`
- `SearchButton` and `SearchField` expansion — the magnifier button is replaced by
  an animated, wider field carrying the same icon and automatic focus. An empty
  field collapses on blur; a non-empty query remains expanded. Focus is drawn by
  the outer control group so the ring is never clipped by the animated field.

### 2.4 `ProjectTabBar` — feature floor, demo built

- `ProjectTab` — **demo built**: macOS Tahoe-inspired pill presentation owned by the
  component itself, with selected, remote/local, busy, disconnected, attention,
  closable, fixed, Up Next count, and active-ticket states. Every tab selector
  participates in sequential Tab order;
  arrow/Home/End keyboard navigation remains available within a composed tablist, and
  Delete/Backspace closes a focused closeable tab. Fine-pointer devices reveal close affordances on hover or
  keyboard focus; touch-oriented devices retain the visible close control. Local tabs
  omit the redundant folder/branch icon, while remote tabs retain their cloud marker.
  Keyboard focus on the tab-selection action outlines the complete compound pill,
  including its leading close affordance, rather than bisecting the pill at that action's edge;
  focusing Close retains a separate compact focus indicator for that independent action.
  The close affordance is a compact, highlight-free leading control with balanced
  trailing space
  so the tab identity remains visually centered. When that trailing balance slot is
  empty, it subtracts the selector's existing item gap instead of counting that gap a
  second time; transient trailing indicators such as
  busy, offline, attention, and ticket-work state occupy that reserved balance space.
  Ticket-work state derives from each open project's already-cached ticket rows without
  adding requests: a yellow circle is absent at zero, shows 1–99, caps visible overflow
  at `99+`, and changes to a rotating yellow half-speed activity ring for any live claim.
  It has one yellow segment per simultaneous active claim, capped at 8 drawn segments for
  legibility (HS2-7XHZY1), while its center label always
  shows the Up Next count (reading `0` when work is active but nothing is queued); the
  ring segments are the only active-count axis (HS2-3TGYER). The accessible label continues
  reporting both uncapped axes.
  Each project's replay-safe live-update connection authoritatively replaces that
  cache after ticket/claim events while the tab is inactive. Refresh coordination coalesces
  repeated work without dropping distinct projects, defers rendering while an open select
  owns focus, and preserves cache-first project switching.
  It can coexist with a permission notification. Busy uses a full-ring CSS spinner: its statically
  centered wrapper never transforms, and the ring alone rotates around its center.
  This avoids both transform-composition drift and the perceptual wobble of rotating
  an incomplete Lucide arc; browser coverage samples its center across animation frames.
- `ProjectTabBar` presentation props (HS2-DR549A):
  - `surface`: `lowered` by default, or `default` to share the surface of a column it heads.
  - `divider`: draws the bottom rule (default `true`).

  The demo shows the standalone strip and the shell-column variant (`surface="default"`,
  `divider={false}`). On desktop, Add project stays beside the last tab and the workspace action
  holds the far edge of the growing trailing zone, set through Kerf's public
  `--kui-tab-bar-trailing-flex` token. Kerf's `end` zone accepts only a ToolbarControlGroup, so the
  action moves there only after KF-A59SC4 ships. Each `ProjectTab` sets Kerf's attention-color token
  on its own AppTab root (`[data-tab-kind='project']`). It keeps Kerf's own drop-target treatment.

- `ProjectTabContextMenu` — **built** with Lucide icons for Close Tab, Close Other
  Tabs, Close Tabs to the Right, and Close All Tabs.
- `AddProjectButton` — **production + demo built** with controlled insertion and selection;
  production invokes the host-native folder chooser immediately.
- `ProjectPicker` — **production built** as the native chooser boundary; a selected
  checkout opens directly, with ticket-source setup shown only when the project reports
  no configured source.
- `TabOverflowMenu` — removed from the current design; the project strip itself is
  horizontally scrollable and does not duplicate projects in a secondary menu.
- `TerminalDashboardButton` and `CrossProjectStatsButton` — **shell navigation built**
  with controlled selected state. Their full dashboard surfaces remain tracked by
  HS2-2ZCN7K and HS2-38RJMK respectively.

Global dashboard modes precede project tabs. Add follows the project strip when there is
no ticket-view action; on desktop project views, Add and the current view action occupy the
far trailing edge together. Tabs
represent server/project connections rather than embedded stores. The strip uses
Kerf's 4px tight-cluster rhythm around and between controls, with an 8px outer
inline inset, while retaining its fixed 60px geometry. The component must tolerate
two tabs that expose the same store through different checkouts or
servers. The tab strip scrolls horizontally without truncating identities (every TabBar strip —
project tabs, terminal drawer, and inspector — clamps vertical scrolling, HS2-QG4K9W); the overflow
strip provides direct access to tabs outside the current viewport and reserves enough inset
for the complete selected shadow and keyboard focus ring at both ends. Project tabs support
same-strip pointer drag reordering. Terminal and AI-chat tabs share one project-scoped
device-local order and can drag across kinds or move with Alt+Shift+Left/Right; closing the
selected drawer tab transfers focus to the nearest live mixed-kind neighbor. Project and
drawer order persist across reloads. Opening the drawer focuses the selected terminal or
writable AI-chat composer by default; clicking a live drawer tab likewise transfers keyboard
focus into its terminal/chat input while yielding to any newer explicit focus action. The Add action remains
vertically centered with the pills.

In desktop project mode the hierarchy is Toolbar(compact current-view heading +
WorkspaceControls) → ProjectTabBar(current view action at the trailing edge) → connection
banner → workspace. The redundant project-name heading and separate large PageHeader are
absent. Global modes retain their established main-toolbar titles, while mobile retains
its project Select and separate view Select/action row. TicketInspector is a root trailing region spanning the shell's
full height and uses the same animated slide/collapse contract as the project sidebar;
its panel-right control replaces a generic close glyph. Inspector tab
icons never shrink, and compact inspectors switch to icon-only labels.

The HS1 conversion prompt and persistent migration/cleanup notices use the canonical
24 px major-region, 16 px icon/column, 8 px field/action, and 4 px connected-copy
rhythm. Both notices compose Kerf's shared `StateBanner` with polite status semantics
and info/success tones. Their surfaces use 8 px block by 16 px inline padding, and
their compact actions retain the established 28.8 px minimum height with 8 px inline
padding. The cleanup notice passes one app-owned Dismiss/Delete action group to the
primitive's single action slot, preserving responsive wrapping without duplicating the
shared banner anatomy.

## 3. Ticket workspace

### 3.1 `Workspace` — feature floor

Routes the selected project and view to one major content surface while retaining
selection where sensible.

The shell's focusable work-area wrapper owns ticket clipboard shortcuts for its list and
column presentations. Its `:focus-within` outline is continuous around the composer and
scrolling workspace instead of outlining an individual row. Clicking or selecting text
outside the work area releases shortcut ownership; selected text and editable descendants
always retain native clipboard behavior.

- `ListWorkspace`
- `ColumnWorkspace`
- `SearchResultsWorkspace`
- built: shared custom views created and edited with the standard tokenized query input and
  selected from either the project sidebar or terminal ticket rail; project-sidebar rows keep
  management controls behind a shared ellipsis/right-click menu, preserve stable identity while
  editing the name and query, and return a deleted active view to Queue after confirmation
- later: `TerminalDashboard` and `AnalyticsDashboard`
- `WorkspaceState` — loading skeleton, empty state, error/retry, offline snapshot,
  unsupported view, and no-project onboarding

### 3.2 `ListWorkspace` — feature floor and current wireframe focus

- `ViewHeading`
- `QuickTicketComposer` — **demo built**: compact launcher expands to title/category,
  an adjacent Up Next star, a one-line vertically resizable Details field, provider
  destination, required-title validation, create, and cancel states; creation
  inserts a selected mock ticket into the shared collection. The collapsed launcher and
  expanded form are attachment drop targets. Dropped or browsed files are safety-screened,
  shown as removable pending evidence, cleared on cancellation, and uploaded after the
  ticket is created when the selected provider supports both operations. Category choices use
  the shared colored/iconic picker in both its selected and menu presentations;
  the star sends explicit Up Next placement and takes precedence over Backlog-view creation;
  created mock tickets derive their category icon/color from that same choice model.
  The provider destination is plain "Creating in" text for one writable source and a compact
  Kerf source `Select` for several (HS2-NZMJBJ); the demo settings switch between the two
  variants, and the demo remembers the last source it created in, preselecting it next time.
  The demo's GitHub issues source reports no attachment support, so files browsed in while
  Hot Sheet git is selected stay listed but block Create, with an explanation and a
  **Remove all** header action, until they are removed or the user switches back
  (HS2-8HHHK3).
  Textual Cancel intentionally has no redundant icon. Production and catalog keep one
  controlled Web Awesome dialog host mounted, call `show()` while the live launcher owns
  focus, and let native modality confine Tab/Shift-Tab, order nested-control Escape, inert
  the background, and restore the trigger after completed hide. The backdrop is not a
  dismissal action; application state owns validation, drafts, create-in-flight veto,
  disposal, and the successful create destination. The app-specific evidence staging rows
  remain custom because they are form state rather than pane navigation; category selection
  continues to use Kerf `Select`. The catalog schedules `show()` only
  after expanded content has committed, and keyboard interactions outside the composer
  begin only after its native hide lifecycle completes.
  - compact “New ticket…” entry
  - expands to the minimum useful creation fields
  - respects the selected ticket provider and its capabilities
- `TicketList` — **demo built**: composes the production `TicketRow` at
  comfortable list width with platform-style replacement, Command/Ctrl toggle, Shift
  range, arrow-key range extension, and Select All semantics and no parallel row
  markup; it fills the width supplied by its host (which owns the standard workspace
  margins), and the list shell and its first/last rows share rounded outer corners. The list never
  styles its rows: it passes each healthy and corrupt row a `listEdge` (`start`, `end`, or `only`)
  prop, and the row rounds its own matching corners. The row also owns the overlap of
  adjacent selected list rows (HS2-4APEJP)
  - composes the shared `TicketEmptyState` when no healthy or corrupt row exists,
    with distinct new-project, empty-view, searching, and no-match language
  - later data integration: virtualization for exceptionally large result sets
  - incremental paging and live insertion/reordering
  - `TicketListSection` where grouping is active
  - `TicketRowDivider`
- `TicketRow` — **demo built**: a shared, horizontally responsive ticket-summary
  boundary for list and narrow column use. The comfortable list presentation is a
  square-cornered, flat, separator-led row that reads as one continuous list; the same component becomes a rounded, borderless card at
  narrow column widths. A quiet inset outline previews pointer hover without changing
  the row background, while selection supplies the persistent blue outline. Its primary
  line treats qualified slug and title as one normal
  inline formatting flow, with an explicit two-line limit in lists and three-line
  limit in board columns. The comfortable list keeps the category in its dedicated
  leading slot; compact board rows remove that empty left gutter and place a reduced
  category icon inline immediately before and vertically centered against the slug's
  first line. The remaining flow is ordered
  slug → priority → title so bounded priority remains visible before an arbitrarily
  long title (the slug is a stable-width inline block). Updated time is the first item in that identity flow and
  floats right, allowing later lines of a long title to use the space beneath it. A quieter, vertically
  centered secondary flow holds the persistent independently operable outline/filled
  Up Next star, status, short owner name, and all tags; it wraps without hiding or
  collapsing metadata at narrow widths. A completed or verified ticket with a derived
  `latest_confidence` adds the compact `ConfidenceBadge` pill (gauge icon + percentage,
  band-tinted) just before its tags in both list rows and board cards (HS2-A0Q6G6).
  Any other status never shows a score, even when the wire still carries one. The
  labeled "Confidence NN%" form stays in the inspector/reader header. It also includes blue selection, keyboard
  selection, and one shared right-click context menu. Plainly reactivating the one
  already-selected, fully loaded row is inert, including while an inspector editor owns
  focus; modifier selection and double-click reader opening remain active. The real TicketList and TicketBoard
  compositions wire that same menu to reader, direct icon-rich category/priority/status
  submenus with bulk assignment, lifecycle-eligible Up Next, duplication, archive, and
  delete handlers; the demo only substitutes deterministic effects. A single completed
  selection prepends `Verified` and `Not Working…`; completed and verified selections
  never expose Up Next.
  The left rail is reserved for special-state attention in HS1 precedence order:
  needs review (purple), blocked (dark gray), then Up Next (yellow). Up Next also uses
  the familiar yellow Lucide star with an accessible add/remove name. Blocked tickets
  additionally show a compact `Blocked` pill immediately after their status. A live,
  non-expired worker claim adds a yellow activity spinner immediately after the status badge;
  started-but-unclaimed and
  previously claimed tickets do not show it.
  When that live claim carries an ETA (HS2-XQMDQB), the agent name is followed by a compact
  estimate: a brand-blue Web Awesome progress ring showing the elapsed share of the time from
  the claim's start (`claim_started_at`) to its ETA, plus `~45m left` (`<1m`, `~1h 20m`, `~3d`
  forms). Once the ETA passes, the ring gives way to `Soon`, with the activity spinner as the
  indeterminate cue, until the worker re-estimates or the claim ends. Both states carry a
  title with the absolute time. The countdown re-renders from a local timer (at most every 30
  seconds, and just after the nearest ETA) that never makes network requests. The demo's
  **Claim ETA** setting exposes the none, on-track, and past-estimate variants.
  - category/type icon and color use a serializable Lucide name plus the HS1 custom
    command palette; a configured icon replaces category text and appears before the title.
    Neutral retains its pale fill swatch but uses a darker, still-lighter-than-gray icon
    stroke for visibility. Without an icon, the same color applies to a fixed-width,
    three-letter uppercase category abbreviation (`BUG`, `FEA`, `TSK`, etc.); icon
    and label variants occupy the same 2rem column so ticket text always aligns
  - slug/native ticket identifier
  - title
  - up-next/star toggle
  - tag chips
  - priority uses a directional Lucide scale immediately after the slug: double-up
    red, up orange, neutral minus gray, and down blue
  - assignee, active tool, or connection indicator
  - relative updated time
  - selected, unread, claimed, blocked, busy, review-needed, and provider states
  - production mutations behind row context-menu actions and drag affordance

### 3.3 `ColumnWorkspace` — feature floor

- `TicketBoard` — **demo built**: status columns stretch equally to fill
  available width down to a 250px minimum, share TicketList's multi-selection contract
  across columns, then the workspace scrolls horizontally
  edge-to-edge between its sidebar separators and reaches the workspace bottom, with
  uniform 8px outer and inter-column gutters plus 16px breathing room inside the bottom of each
  independently scrolling ticket region. The
  board has no extra framing and
  whose title and count provide sufficient grouping without an additional visual
  container around either the board or each column. Each column composes production
  flat, elevation-free, idle-borderless `TicketRow` at narrow width. The deterministic demo carries enough live tickets to
  overflow all columns; each ticket region scrolls independently while its heading and
  count remain fixed. A hosting workspace may add its own surrounding surface when
  appropriate. If the whole board is empty, it retains the column headings and places
  one shared `TicketEmptyState` across the board body; individual empty columns remain
  blank when other columns contain tickets.
  Its `layout` variant is `grid` (default, described above) or `paged` (HS2-ZYJMDP): each column
  is the board's visible width minus 24px so the next column peeks in, and mandatory horizontal
  scroll snapping settles a released scroll or swipe on the nearest column start with a smooth
  animated scroll (instant under `prefers-reduced-motion`). Trailing room lets the last column rest
  at the same 8px inset. A paged board opts each column into snapping through
  `TicketBoardColumn`'s `scrollSnap` prop rather than styling the column (HS2-4APEJP). The
  production app uses `paged` below the mobile breakpoint; the demo
  shows both variants. Changing `layout` remounts the board, so a paged scroll offset never leaves
  the grid mid-column.
- `TicketBoardColumn` — **demo built**: owns one heading, count derived from its ticket
  collection, fixed header, independently scrolling ticket region, visible scroll
  affordance, and a full-width heading control that selects every ticket in that column.
  The header, ticket gutter, row rhythm, and progressive-loading state use Kerf's 8px
  within-group spacing; the scrolling region retains 16px of bottom breathing room.
  Its semantic `h2` resets inherited browser heading typography and the selectable
  control has an explicit compact 2rem height, so native heading metrics cannot expand
  the board's header track.
  Its `scrollSnap` prop (default off) makes the column a `scroll-snap-align: start` target
  for a horizontally scrolling host such as the paged `TicketBoard`.
  It also has a standalone demo that preserves the 250px production minimum and shared
  responsive `TicketRow` composition. Loading and mutation-error variants are tracked
  by HS2-0W67Y6; an empty column intentionally retains only its heading and count.
- The real Queue board uses `Not Started`, `Started`, `Completed`, and `Verified`
  columns. A per-project setting can hide `Verified`, merging those tickets into
  `Completed`. Backlog and Archive views each use one eponymous column because the
  selected view already supplies their grouping. An active search retains that selected
  view's column set because its server request is scoped to the same collection.
- There is no separate `TicketCard`: narrow board columns activate `TicketRow`'s
  container-query card presentation while preserving identical markup and actions
- keyboard and pointer movement between columns
- explicit mutation preview/error handling when a provider lacks the target field

Comprehensive platform-aware keyboard shortcuts are planned in HS2-KTHGVE. They must
follow macOS conventions on Apple platforms and standard web/OS conventions elsewhere,
remain discoverable, avoid editable-field conflicts, and receive unit plus browser
coverage.

### 3.4 Search and filtering — feature floor

WorkspaceHeader and the workspace-grid ticket rail project selection into native
Up Next and More actions buttons. The Up Next icon is an empty full-outline star
when none are queued, a yellow half-filled full-outline star for a mixed selection,
and a yellow filled star when all are queued. Its `aria-pressed` value is respectively
`false`, `mixed`, or `true`; selection does not add a persistent background behind
the star. Hover and keyboard focus retain the canonical circular button treatment.
The narrow overflow action presents the same icon state and names the selection state
for assistive technology. Clicking none/mixed adds all eligible selected tickets;
clicking all removes them. Empty selections, Completed/Verified and other ineligible
statuses, and providers without update capability remain disabled (HS2-WP15AF).
The connected header and rail demos derive this state from the selected fixture tickets
and exercise the same selection replacement and none/mixed/all toggle transitions.

The inline search and active-filter surface is built under HS2-383D6K; the later custom
query-builder/editor is tracked separately by HS2-G7FWSS. Advanced constraints belong in
the ordinary toolbar search rather than a separate launcher and dialog. The earlier
`GlobalSearchOverlay` composition and its overlay-only scope, result-row, filter suggestion,
and saved-view handoff components were removed: they duplicated this simpler primary flow
without a clear place in the product.

The editor chrome itself is now kerf UI's `TokenSearchField` + `wireTokenSearchFields`
(HS2-88P90P), not a hand-rolled contenteditable. Kerf renders the editor, atomic chips
(`data-component="token-search-token"`, `data-token-value`), leading icon, clear, and
`role="searchbox"`, and its wire helper owns Enter-submit, caret preservation across
controlled token deletion, and the opt-in adjacent-chip keyboard (Backspace/Delete removes
the neighbouring chip; ArrowRight steps past a trailing chip). The app keeps ownership of the
token model (`clients/web/src/inline-search.ts`, adapted to kerf via `toTokenSearchToken` /
`fromTokenSearchTokens` keyed on each token's canonical `raw`), the whitespace-commit input
gating (reported by the helper's `onEdit` callback), the suggestions/date/help popovers
(marked `data-token-search-keep-open`), and the
persisted `searchOpen` signal. The workspace field enables Kerf's managed collapsible mode
and adopts that signal, so Kerf owns its canonical magnifier trigger, reveal/focus transfer,
empty-field blur collapse, and Escape collapse while the app's responsive header continues
to read the same state.

Every ticket-search surface composes the app-owned **`TicketSearchField`**
(`clients/web/src/components/ticket-search-field.tsx`, HS2-N5G6JS): Kerf's grouped
`TokenSearchField` inside a `content="search"` `ToolbarControlGroup`, driven by a Kerf-managed
`TokenSearchModel` (HS2-5JXBQY), plus the Hot Sheet helper surfaces that used to live only in the
workspace header — the lifecycle date/time helper derived from the query's trailing filter and
the syntax-help button and popover. The model comes from `createTicketSearchModel`
(`clients/web/src/ticket-search-model.ts`): its rules express the whole grammar (`tag`, `is`,
`has`, `attachment`, and the twelve hyphenated lifecycle date filters), every rule parses through
the app's own `tokenFromRaw`, the `tag` rule's `suggest(input, state)` serves the in-place
completion from the project's tags minus the chips already committed (capped at eight), and the
documented relative syntax `updated-after:4h ago` is quoted (`updated-after:"4h ago"`, the chip's
canonical value) before Kerf's whitespace-delimited grammar sees it. Kerf owns parsing, chips,
the anchored suggestion popover rows (in flow before beta.63; beta.64 adopted) (`kui-token-search__suggestion`, inside the field itself), chip
edit/remove, clear, Enter, and the keyboard; the app projects `model.state` into its
`searchQuery`/`searchTokens` signals (`inlineSearchTokens`) and reseeds the editor through
`model.replace` (`replaceTicketSearch` for a restored session or a seeded saved-view query). The
workspace toolbar, the workspace-grid ticket rail, and the saved-view dialog's non-collapsible
query field all render this one component with their own model, so the helpers cannot drift
apart or be forgotten on a new surface. Its rendered group carries the root class
`ticket-search-field` plus literal modifiers for the app's own state (`--open`) and the
toolbar sizing policy a consumer picks with the `layout` prop (HS2-8FS5BJ). A consumer never
styles the field from its own stylesheet; only literal classes are classifiable by the Kerf
analyzer, so each combination is spelled out.

- `layout="inline"` (default) keeps Kerf's own collapsed and expanded widths.
- `layout="grow"` is the workspace header policy. The open field grows into the free room on
  its row from a 19rem floor, which keeps the view, sort, and utility groups beside it in a
  640px toolbar. It takes the whole row on a compact toolbar, and its collapsed icon leaves
  toolbars of 224px or less.
- `layout="row"` is the narrow-rail policy. The collapsed icon sits at the zone's trailing
  edge, and the open field fills a row of its own through Kerf `fill`, sliding in from the row
  above.

The grow sizing, row placement, and row entrance wait on Kerf `KF-GM376R`, `KF-6WX6VK`, and
`KF-XFPJSY`. Until then they are narrow rules in `ticket-search-field.css` with documented
`KUI-L022` suppressions (`HS2-DAMHD1`).

The grouped field's quieter chip tint is set on the `ticket-search-field__query` hook Kerf
renders on its field root. The form-field presentation renders no such hook, so its chips keep
Kerf's default tint (`KF-5G8WJ0`, `HS2-RXHZVR`). Every action it renders uses
one generic name (`edit-ticket-search-token`, `remove-ticket-search-token`,
`clear-ticket-search`, `toggle-ticket-search-help`, `apply-ticket-search-date`); the shared
`wireTicketSearchFields` helper (`clients/web/src/interactions/ticket-search-field.ts`) resolves
the owning field through its `data-token-search-id` and routes each callback by that id. It is
wired before `wireTokenSearchFields({ models })`, so the chip edit/remove callbacks can read a
chip's position and restore the caret a frame after Kerf has rebuilt the editor; the date helper
commits through `model.commit` for the active prefix; Enter inside the date helper applies the
date instead of submitting an enclosing form. Every path to a lifecycle date chip — the date
helper, a typed filter, a restored query, and the model rule — labels it through
`searchDateLabel`, so a local `YYYY-MM-DD[THH:MM]` value reads in the machine locale's short
date (and time) format (`created after 9/1/26, 11:05 AM`) while relative, zoned, and
locale-typed values keep the text as written (HS2-074E0P). Kerf (5.0.0-beta.64, `KF-Q2G9QS`)
restores the caret itself after a model-mode Backspace/Delete beside a chip and after a model
field's clear, so the app no longer snapshots chip offsets or refocuses a cleared field
(HS2-45F8WW). The surfaces have two placements:
`surfaces="floating"` (default) hangs them below the group as popovers, with the component
overriding Kerf's search-group clipping while expanded; `surfaces="external"` renders none inside
the group, and the consumer places the exported `TicketSearchSurfaces` for the same `id` in its
own stacked layout, for a clipping container that still wants a toolbar field.
In a form, the sibling `TicketSearchFormField` (same module) renders Kerf's
`TokenSearchField presentation="form-field"`: a visible label, hint, and required marker with the
same inset, typography, and edges as a neighboring `wa-input`, no Toolbar, and the helper surfaces
stacked below it in an app-owned `.ticket-search-form-field` wrapper that carries the field id for
the shared wiring. Its chips keep Kerf's default tint (see above). The saved-view dialog uses it
(HS2-E40KC0); the TicketSearchField demo's "Form field" example exposes it. The remaining field
radius and required-marker color differences from `wa-input` are Kerf's (`KF-6P4NAV`). The wrapper is declared to Kerf's composition rule through
`clients/web/ai/component-composition-extension.json` (`rendersAs`
`@kerfjs/ui:toolbar-control-group`), loaded via `.kerf-ui-profile.json` `catalogs`, so a
Toolbar zone accepts it as the group it renders.

**Managed `TokenSearchModel` evaluation (HS2-HHRYP9, Kerf 5.0.0-beta.59).** Kerf's opt-in
`createTokenSearchModel` expresses the whole ticket search grammar: `tag`, `is`, `has`, and
`attachment` are plain rules with `parse`/`label`; the twelve lifecycle date filters are
hyphenated rule names (`created-after`, `updated-before`, …) whose `parse` is
`parseSearchDate`; quoted values, boolean words, and parentheses behave exactly as the
app's own `consumeSearchTokens` (one separator survives per consumed token), and a `tag`
rule's `suggest` can serve the tag completion, with `choose` committing the chip. That fit
is pinned in `clients/web/src/ticket-search-model-evaluation.test.ts`. Adoption was
deferred because the beta.59 model API had three gaps that a faithful migration needs:
`suggest(input)` could not see the committed tokens, so it could not exclude tags already
filtering the query (`KF-YBJ27D`); `choose` accepted only a current suggestion, so the
date helper could not commit its computed token for the active prefix (`KF-K3EJM5`); and only
`clear()` bumped `editorRevision`, so applying a saved view or restoring a session had no
model action that rebuilds the DOM-owned editor text (`KF-ER975X`). Working around them
would have meant double ownership of the search state (the model's `state` beside the app's
persisted `searchQuery`/`searchTokens` signals) across three surfaces. **Beta.62 closed all
three** (HS2-06Q4MG re-ran the evaluation): a rule's `suggest(input, state)` receives the
committed tokens, `model.commit(value)` commits a computed value for the active `name:`
prefix (invalid values and a missing prefix leave the query unchanged), and
`model.replace(value)` rebuilds the editor text and bumps `editorRevision`. The decision is
to adopt: `HS2-5JXBQY` (now Up Next) moves the three `TicketSearchField` surfaces onto the
managed model, with the syntax help staying app-owned.

With the published Kerf dependency, Select All followed by Backspace or
Delete removes both text and tokens while keeping the empty workspace editor open
and focused for immediate typing. Mixed text/token and token-only queries support
repeated deletion and refill at desktop and mobile widths. The package restores
focus across controlled editor replacement; the application does not reopen it in
a separate callback. A deliberate keyboard focus move out of the empty field still
collapses it, and reopening or pressing Escape retains the normal managed-focus
contract (HS2-GRAQ2K).

Managed Clear restores the replacement editor at the DOM mutation checkpoint, before
the next input task. The first typed shortcut letter therefore belongs to search even
when rendering frames are delayed. Clear observation expires without delayed focus or
caret changes, and a real outside focus handoff remains authoritative. The app only
clears its query/token state; Kerf owns restoration (KF-E1DHC6; HS2-NNNFFR).

For app-owned token edits/removals, `inline-search-caret.ts` restores the caret in
the current task's microtask checkpoint, after synchronous rendering/batching.
It coalesces requests for the same field and respects a newer focus handoff. It
does not defer across animation frames, which could collapse a later replacement
selection and duplicate surrounding text. Both workspace and saved-view fields
cover repeated Backspace/Delete, replacement, clear/refill, and continued typing
(HS2-PR5TNA).

For the collapsible toolbar composition, Kerf's `ToolbarControlGroup` owns the enclosing
border, padding, and focus ring while the child `TokenSearchField` avoids a duplicate
border. Hot Sheet only sizes and places that composition; expansion must not strip the
group's chrome in either the workspace header or workspace-grid ticket rail. The rail
demo binds the same search signals for expansion, query filtering, help, and clearing,
including populated blur and empty-collapse/reopen transitions. Its mode controls project
the shared List/Notifications state into the selected button and corresponding content,
with search disabled in Notifications and no Columns choice (HS2-TNSD4K).

- `SearchQueryInput` — **built**: one multiline editable flow containing ordinary text and
  atomic chips in their expression order, with character-level text wrapping and token-level
  chip wrapping; supports inline tag,
  attachment/media-annotation/commit presence, wildcard-filename, and lifecycle date/time
  tokens. Date chips and help examples use the client machine's locale, relative values
  such as `4h ago` resolve at search time, and ISO 8601 input remains portable and always
  accepted. Complete uncommitted filters already affect results; an explicit space or Enter
  commits a chip, while incomplete bare or quoted values survive incidental focus movement.
  Compact chips preserve the single-line field height; the magnifier, help, and conditional
  clear controls remain vertically centered on the field's first text line when content wraps.
  At narrow browser widths, an expanded production search moves below the project identity so
  the project name remains readable while the query retains the full main-column width.
  Real editable boundaries keep a keyboard caret reachable before, between, and after chips;
  Right Arrow normalizes browser-native element and chip-descendant positions at the final chip
  into its trailing editable text span. Backspace/Delete removes the immediately preceding chip,
  while forward Delete removes the immediately following chip, at both middle and end boundaries.
  Its concise placeholder remains
  ordinary search guidance while a help button
  exposes the complete syntax.
- `TicketSearchField` — **built, demo built**: the shared query editor described above. Its
  catalog demo shows the standalone (dialog-style) field with tag completion, the date
  helper, syntax help, chip commit/edit/remove, and clear; the external-surfaces dialog
  layout; the form field; a collapsible toolbar field in each `layout` (`inline`, `grow`,
  `row`, HS2-8FS5BJ); and the disabled state (HS2-N5G6JS).
- Search suggestions — **built**: typing `tag:` offers readable matching project tags in
  place through Kerf's model-managed suggestion rows (Arrow Down reaches them, Enter picks,
  Escape returns to the editor), while lifecycle prefixes expose a native date and
  optional-time helper — in the workspace header, the workspace-grid rail, and the
  saved-view dialog alike.
- Active filters and removable chips — **built** into the toolbar search; chips
  remain inline without internal truncation, expose labeled edit/remove actions, and return to
  text at the same caret position for editing by button or double-click.

Saved views are created and managed from the Views section rather than from a separate
search overlay. The controlled open state and native dialog autofocus own initial name
focus; delayed application work must not redirect immediate query typing or reopen a
cancelled dialog. Programmatic Create/Edit transitions also synchronize the name's live
Web Awesome value before native autofocus, so dirty values cannot survive reopen while
ordinary input continues updating controlled state (HS2-ZQNW62). Query token editing
retains its independent caret restoration.

Later custom-query work adds `QueryBuilder`, `FilterRule`, `FilterGroup`, and
`ViewEditor` without replacing the basic search components.

### 3.5 Selection and batch actions — partial

- `SelectionBar` — persistent affordance tracked with undo history by HS2-4CAN74.
- `BatchActionMenu` — the selected-row context menu currently supplies the shipped batch
  surface; a persistent selection bar remains later work.
- `TicketContextMenu` — **built**: shared list/board menu with Lucide icons, checked
  metadata submenus, stable field/value action contracts, capability-aware category/status/
  priority changes, add/remove tag dialogs, and confirmed soft deletion. Bulk writes use
  fresh provider concurrency tokens and participate in field-aware Undo. Capture-phase
  composed-path containment keeps shadow-DOM menu interactions open and dismisses on every
  true outside pointer-down or Escape.
- `CopyMoveTicketDialog` — tracked by HS2-77M88K.
- `UndoToast` / `UndoHistory` — tracked by HS2-4CAN74.

## 4. Ticket inspector, reading, and editing

### 4.1 `TicketInspector` — feature floor

The trailing inspector shown in the wireframe. `TicketInspector` is **demo built**
as a focused shell around separately demoed `TicketInfoPanel`, `TicketTimeline`,
`TicketCodeReview`, and `TicketAttachments` components, plus the Up Next toggle, close/reopen, and controlled
tab routing. Its AppShell composition projects the same shared active-tab state rather
than substituting a hardcoded default. When one row has been selected but its full ticket
is still loading, AppShell keeps the visible inspector region mounted and shows the
placeholder in place; it never removes and re-adds the sidebar during that transition.
The zero-selection placeholder omits the otherwise-shared Toolbar divider so the empty
navbar does not leave a stray rule above its centered guidance. Loading and
multi-selection placeholders keep the divider to preserve their intentional state boundary.
While a live claim is held, the header leads its status notices with a `LiveClaimNotice`
(holder, the shared activity spinner, and the row's ETA ring/time left or `Soon`); the demo's
**Live claim** setting exposes none, with an estimate, past its estimate, and without an ETA
(HS2-QKNQXC).
The copyable ticket number sits in the header toolbar's **leading** slot for both the sidebar
inspector (HS2-9MCJ2B) and the reader dialog (HS2-FZ5HB2); it centers only for the terminal
ticket rail, whose absolutely positioned back button occupies the leading edge. `slugPlacement`
overrides this per composition (the rail passes `center`). Clicking it copies through the
shared `copyText` helper (`clients/web/src/copy-text.ts`, also used by the repository-file
and attachment Copy actions): when Safari refuses the Clipboard API inside the click, the
helper copies through a temporary selection in the same gesture and restores focus and
selection, so a copy never fails intermittently (HS2-1A2BQR). The section strip uses Kerf
`TabBar`/`AppTab` with automatic keyboard activation and one selected, roving-focus tab.
The inspector owns one 8px outer inset and removes the TabBar toolbar padding; its auto
width includes those margins within the available space. The rail keeps a 4px inset,
and each equal-width segment has a full-width selection target with its icon, responsive
label, and optional attachment count centered together. Sidebar labels remain visually
hidden but accessible, while wide readers display them (HS2-GZN2HZ, HS2-WKGMN4).

Structured close outcomes are available from the single-ticket context menu when the
owning provider advertises both close and close-reason support. `TicketCloseDialog`
offers Completed, Not planned, Duplicate, and Obsolete; Duplicate requires searching for
and selecting a distinct canonical ticket before submission. Search results reuse
`ListItem`, the reason control reuses `Select`, and validation prevents self-reference.
The form separates peer regions by 16px and uses 8px within result/selection groups,
with a 4px inset around the result list.
Closed tickets retain a visible outcome in the inspector, and duplicate outcomes link to
the canonical ticket instead of relying on a freeform explanatory note. Canonical tickets
render reverse duplicate backlinks as shared menu rows labeled with both project and slug;
same-slug sources remain unambiguous, and a compact status message names registered
projects with recognizable sources that could not be searched without hiding results from
accessible projects. Deleted checkouts and recreated directories whose former git source no
longer contains HS2 store metadata are stale registrations, not user-facing partial failures.

- `InspectorHeader`
  - ticket identifier
  - full multi-line title with no line-count cap in the inspector sidebar
  - capability-aware inline title editing with debounced persistence
  - up-next/star toggle
  - close/collapse action
- `InspectorTabBar`
- `TicketInfoPanel` — **demo built**: metadata; safe Markdown details on a white
  basic-note-like surface; blocked reason before Details with its header outside the
  gray reason box; tags; notes with collection-derived counts; and provider/update
  provenance. The passive Details heading keeps ListHeader's inline inset while using
  Category's compact label geometry and `0.5em` gap above the field. Preview,
  editing, read-only, reader, and loading states share that spacing (HS2-S6S709).
  Status uses the same compact `ListHeader` treatment and places its existing status
  picker and optional Blocked badge in `ListInsetControl`. The metadata section owns
  the outer inset, so the control row adds no border or padding around the badges;
  their edges align with the Category field, while their own styling and read-only
  behavior remain intact (HS2-AHADNK).
  Its intrinsic-width boundaries keep both metadata columns, long
  unbroken details, and long note bodies inside the inspector at narrow widths;
  wide Markdown tables and code blocks scroll within their own content surface.
  The sidebar follows the kerf 8px-grid inset (HS2-EQEGGG, refined in HS2-R64ETQ):
  the shared `TicketInspectorPanel` column (`ticket-inspector-panel.tsx`) that every tab panel
  composes owns no inline padding, and each direct child (section, notes, provenance) sits 8px from the edge and owns no border/padding of its own. Bordered
  surfaces/cards/fields therefore sit their own 1px border on the 8px column and own their 8px
  padding, so their text lands at 17px; headers and other non-bordered content get a 1px
  transparent inline border + 8px padding to align their text to the same 17px column. There
  are **no negative margins** — nothing breaks out of a padded parent. The compact tabs use the
  same 8px margin. Since `@kerfjs/ui` 5.0.0-beta.17, Category and Priority use the selects'
  native labels in a two-column grid: the shared Web Awesome theme gives each field the canonical
  1px + 8px content inset and aligns its ListHeader-style label with its value, eliminating separate
  inset wrappers and label-suppressed controls. The Code Review and Attachments panels
  follow the same rule (heading at 17px, evidence/commit/attachment surfaces at the 8px column).
  The wider reader modal keeps its own generous inline padding through the column's
  `presentation="reader"` variant, so its sections do not add the 8px margin on top.
  **Style ownership (HS2-MGVE50):** each panel styles only its own markup in its own stylesheet —
  `TicketInfoPanel` (`ticket-info-panel__*`), `TicketTimeline` (`ticket-timeline__*`),
  `TicketAttachments` (`ticket-attachments__*`), `TicketCodeReview` (`ticket-code-review--panel`),
  and the skeleton (`ticket-inspector-skeleton__*`, non-interactive through `inert` rather than
  CSS reaching into the Kerf tab bar). The column no longer restyles descendant `h2`/`p`, so Markdown
  headings, paragraphs, quotations, and activity-note text inside the inspector use their own
  component typography; the Details editor aligns to its surface through `MarkdownEditor`
  `inset="flush"` instead of a padding override, and the reader's card-free chrome and title size are
  `TicketInspector` reader-presentation variants. The title is the inspector's own
  `.ticket-inspector__title` element (with a `--reader` modifier) rather than a header `h1`
  descendant selector. Persisted editor heights (`ticket-editor-size.ts`) differ per presentation,
  so the inspector body maps them onto its children's public height tokens instead of sizing
  their textareas: `MarkdownEditor` reads `--markdown-editor-source-height` (embedded source),
  `TicketInfoPanel` reads `--ticket-info-panel-blocked-reason-height`, and `NoteCard` reads
  `--note-card-editor-height`. Each token defaults to `auto`, and a coarse pointer resets all three
  to `auto` so touch editors grow with their content (HS2-DYAR0S). It reuses the same `ListHeader`/`ListItem`
  primitives as the left project sidebar for Details, Tags, Notes, Block ticket,
  and Add note. Headers and content align by their text/icon inset while bordered
  surfaces remain flush below their headers without a second indentation level.
  App-owned spacing follows Kerf's semantic rhythm: 4px inside tight label/action
  clusters, 8px within one surface, 16px between homogeneous attachment groups,
  and 24px between the inspector's differing major sections (HS2-4Y6SM9).
  Editable tags use the shared `ListHeader` with a trailing icon-only Add tag action,
  matching the Views header, and a uniquely targeted anchored popover in sidebar and
  reader instances. The popover contains a labeled autocomplete field,
  supports repeated Enter/comma additions, dismisses with Escape while restoring trigger
  focus, remains within the narrow viewport, and follows the canonical 16/8/4 spacing hierarchy.
  An unblocked ticket exposes a full-width shared-menu `Block ticket` action without an
  otherwise-empty `Blocked reason` heading. Its controlled editor flushes on blur,
  preserves the saved reason, and creates the adjacent status `Blocked` pill.
  - `TicketTimeline` — **demo built**: chronological activity shown as time-ago and a
    concise headline along a continuous dot/line track; durable ticket activity omits
    the full note body/subtitle, and its displayed event total is derived from the
    rendered entry collection
  - `TicketCodeReview` — **demo built**: ticket-associated commit subjects, abbreviated
    SHAs, dates, two-line Markdown body previews with click-to-expand full messages,
    configured-tool status, individual commit actions, adjacent-range actions, and the
    repository A/B comparison banner/labels. The catalog exposes configured,
    unconfigured, empty, loading, and error states; actions are disabled without a
    configured Git diff tool. Its shared sidebar/reader/repository presentation uses 16px
    between major review regions, 8px within cards and rows, and 4px for connected
    metadata/icon clusters while retaining explicit graph/control geometry (HS2-4Y6SM9).
    The A/B side toggle is a Kerf `ToolbarControlGroup` with `selectedChrome="filled"`
    and real `aria-pressed` values, and commit bodies use the inherited-tone, small,
    compact `MarkdownPreview` variants; the review stylesheet styles no other component
    (HS2-7RY5GK). Its `embedded` presentation (the repository status popover) marks the root
    `data-embedded` and owns the 16px gap below its heading, so hosts never style its header
    (HS2-4APEJP).
  - `TicketAttachments` — **demo built**: attachment rows with a subtle count badge beside
    the aligned section heading, plus native browse and drop entry points; no redundant
    total line is rendered below the collection.
    Open, download, copy-reference, and remove icon buttons have explicit accessible
    names, hover titles, and visible hover/focus states. Double-clicking the row uses
    the same Open action; action-button double-clicks do not bubble into the row action.
    The inspector and TicketRow are also attachment drop targets. `editable={false}` is the
    append-only variant for providers without `attachment_edit` (HS2-HSA64D): browse/drop stay,
    names become links, and the menus, dragging, and batch label/purpose editing are omitted;
    `enabled={false}` shows the unsupported notice. The demo shows all three variants.
- `TicketMetadataEditor`
  - `Select` — **demo built**: compact, icon-bearing Web Awesome select foundation
    shared by ticket category and priority controls, including selected-value and
    popup-option icon/color projection with the same measured `0.5rem` icon/label
    gap used by custom-command `ListItem`s;
    the light-DOM icon margin explicitly overrides Web Awesome's slotted default.
    Consumers may supply a custom selected-value renderer while retaining the shared
    option list, keyboard behavior, spacing, and typography
  - `TicketCategorySelect` — **demo built**: configured category icons and colors in
    both selected-value and popup-option presentations
  - `TicketPrioritySelect` — **demo built**: semantic priority icons in both
    selected-value and popup-option presentations
  - `TicketStatusMenu` — **demo built**: a shared `Select` whose custom selected-value
    renderer retains the `StatusBadge` presentation at its `semibold` weight. Kerf's
    `presentation="toolbar-borderless"` and `caret={false}` remove the trigger chrome and caret.
    One temporary trigger-geometry rule (zero min-height, padding, and border width) remains
    until Kerf ships a bare inline trigger (KF-V2Y51V, HS2-4APEJP);
    every normally weighted popup option carries its semantic Lucide icon, and the
    selected control intentionally hides the redundant dropdown caret. Inspector and
    row-context status menus share one canonical order: Not started, Started,
    Completed, Verified, then a separator before Backlog and Archive. Backlog uses the
    clock metaphor; Archive uses the archive-box metaphor.
  - `StatusPicker`
  - `StatusBadge` — **built**: readable status text with status-specific tone,
    optional reinforcing Lucide icon, filled/plain appearances, regular/compact
    sizing, and a bold (default) or `semibold` label `weight`; every public variant is exposed with unit and bidirectional `/ux-demo` coverage
  - `ConfidenceCalibration` — **built** (`components/confidence-calibration.tsx`,
    HS2-Q1WCCY): a project's per-band calibration table, with a reopen-rate bar and
    recent completions. Its `/ux-demo` entry exposes the report, empty, loading, and error
    states.
  - `ConfidenceBadge` — **built** (`components/confidence-badge.tsx`, HS2-A0Q6G6): the
    banded AI completion confidence. The `compact` pill is used on note cards and ticket
    list/board summaries; `labeled` is used in the inspector/reader header. Its own
    `/ux-demo` entry exposes both appearances across all four rubric bands
  - assignee/reviewer/claim fields when supported
  - capability-aware validation and unsupported-field explanation
- `TicketDetailsSection` — section header remains outside its visually distinct
  bordered Markdown surface, matching Notes hierarchy; double-click non-empty details,
  single-click the empty prompt, or use its keyboard action to begin editing. In write
  mode the textarea owns the complete bordered surface, with content inset by internal
  padding and the native vertical resize handle at the surface's outer corner.
- `TicketTagsSection` — **built**: controlled chips with capability-aware removal,
  duplicate-safe creation, and native autocomplete suggestions shared by inspector
  and reader
- `TicketAttachmentsSection` — **built**: aligned inspector gutters, a subtle heading count,
  a single accessible Lucide ellipsis per file,
  with click/right-click parity through the shared ListItem-based Open, Download, Copy
  reference, and Remove context menu, plus a
  responsive, full-width square contained image-preview grid feeding the shared full-screen
  arrow/keyboard/swipe gallery
- `AttachmentGallery` — **built**: square-cornered contained/covered image and video media,
  with a layout-owning playback footer, an initial video poster, custom play/scrub/time controls,
  and a click-persistent volume popup containing its slider and mute action. Clickable wireframe-style
  annotation ticks and draggable plus keyboard-adjustable bracket endpoints cover selected timed
  annotations. High-frequency playback and scrub updates stay inside the gallery DOM, and gallery
  teardown releases the video decoder/resource. Native video controls stay disabled so the component
  has one consistent cross-browser control surface. The `/ux-demo` state exercises popup open,
  mute, volume, click-away dismissal, timed annotations, and close/reopen transitions. Its Kerf
  `Toolbar`/`ToolbarControlGroup` chrome follows a 24px media inset, 16px peer-group/popup rhythm,
  and 8px within-control rhythm while retaining explicit media-control geometry (HS2-4Y6SM9).
- `TicketNotesSection`

### 4.2 Details and reader surfaces — feature floor

- `MarkdownPreview` — **built**: HS1-parity `marked` rendering with GFM tables and
  task lists, line breaks, links, images, fenced/inline code, blockquotes, lists, and
  headings. Raw HTML is escaped and unsafe link/image protocols are rejected. Long
  tokens wrap, while intrinsically wide tables and code blocks remain locally
  scrollable rather than widening an inspector or reader. Block rhythm uses Kerf's
  16px group spacing, headings step from 16px to 24px by level, and tight inline/list
  relationships use 4px or 8px.
  Ticket-aware note previews additionally resolve local and cross-ticket attachment
  references, inline supported images, and expose host-native actions.
  Consumers choose presentation through props rather than restyling `.markdown-preview`
  (HS2-7RY5GK): `tone` (`default`; `inherit` takes the container color but keeps link
  color; `inverse` carries the container color into links, for loud fills), `size`
  (`default`; `small` is the 12px/1.4 secondary scale; `inherit` takes the container
  font), `density` (`default` 16px block rhythm; `compact` 4px; `flush` 0), and `media`
  (`full`; `thumbnail` crops attachment images into a 192 x 112px box). The
  `MarkdownPreview` UX demo shows every variant on the surface it is designed for.
- `MarkdownEditor` — **demo built**: rendered preview by default, double-click/keyboard
  to edit non-empty content, single-click to add empty content, persistent controlled
  draft, full-surface vertically resizable embedded details with padded text and an
  outer-corner resize handle, inline/expanded presentation, and 150 ms debounced autosave without routine
  Save/Cancel actions. Internal editor controls preserve editing; external blur flushes.
  The embedded appearance reuses the same behavior in inspector and reader without a
  redundant standalone toolbar or save-status footer; `inset="flush"` (demoed beside
  `appearance` in the UX demo settings, which open from the demo's Settings button and
  reset to standalone/padded, HS2-QBR5HC) drops the preview/source padding when the host surface
  owns the inset, as the inspector Details surface does (HS2-MGVE50); the real inspector persists edits
  through its checkout. Live ticket refresh is field-aware: unrelated changes merge into
  the inspector while the draft remains mounted, remote-only changes to an untouched
  draft are adopted, and only a divergent edit to the same field opens the shared
  side-by-side `TicketFieldConflict` editor for choosing the remote text or applying an
  edited merge.
- `NotWorkingDialog` — **demo built**: an explicit completed-ticket failure report
  accepting a note and/or pending evidence. It retains input after failures, prevents
  accidental light dismissal, and invokes one capability-gated atomic provider operation
  that publishes the complete report and reopens as Not Started + Up Next.
- `PendingAttachmentPicker` — **demo built**: reusable browse/drop evidence staging
  with long-name ellipsis and per-file removal before submission.
- `InlineEditableField`
- `ReaderButton`
- `TicketReader` — **built in the demo and real web shell**: a nearly
  full-browser-height dialog with exactly 24px of backdrop above and below, presenting
  the actual `TicketInspector`, preserving its metadata editing, tabs, attachments, timeline,
  Markdown details, notes, and controlled state rather than maintaining a reduced
  parallel reader implementation. The inspector exposes a Reader action, reader content
  uses its full available width, and details/notes retain their normal direct editing
  affordances without a separate reader-wide Edit mode. In-progress inline details
  drafts carry into the larger surface without losing focus or content. Ticket-reference
  The persistent Web Awesome dialog is opened with `show()`, supplies native modality and
  focus confinement, delegates nested-control Escape handling to the platform, and is not
  light-dismissable. Its cancelable hide phase flushes pending edits before the completed
  hide updates application state and restores focus. Ticket-reference links push exact
  project-qualified, read-only `TicketReader` layers without changing the
  main project/ticket selection. Navigation may recurse; only the top layer is modal and
  interactive, and Close/Escape unwinds one frame before restoring focus to its originating
  link (or the work area if that opener disappeared). A compact project/depth header distinguishes stacked same-slug readers and remains
  usable at narrow viewport widths. Large reader text sets one unitless reading scale
  (`--hotsheet-reading-scale: var(--hs-reader-text-scale)`, 1.5) on the reader's own root; Markdown
  previews and sources, note bodies, feedback prompts, choices, and response editors multiply their
  own ordinary sizes by it, so every element is exactly 1.5× without the reader restyling them
  (HS2-MGVE50).
- `UnsavedChangesGuard`

### 4.3 Tags — feature floor

- `TagList`
- `TagChip` — **built**: Kerf's removable `Chip` primitive inside an app wrapper that carries
  the stable domain identity (`data-tag-id`), compact quiet rounded default presentation plus
  optional variants (mapped onto Chip tone, appearance, shape, and size), disabled/removable
  behavior, unit tests, and interactive `/ux-demo` coverage. Removal is Chip's delegated
  `remove-tag-chip` action; the owning feature resolves the tag from the wrapper and mutates
  the ticket (HS2-HJEHRW). Hot Sheet owns no chip chrome.

The component catalog records composition relationships. A left-aligned “Related
components” menu in the main demo footer uses the shared `Select`. It lists `Used by`
first, then a separated `Uses` section when both exist; headings carry the relationship
meaning and each option uses the same evocative component icon as the catalog sidebar.

Production web component CSS is colocated in `clients/web/src/components/` and imported
by its component module. The `/ux-demo` stylesheet owns only catalog shell, inspector,
and stage presentation, ensuring the demo exercises the same CSS the real app imports.
Application-level render boundaries follow the same ownership rule: `MainShell` and the
inspector surface live under `components/`, while `main.tsx` only derives their state and
props. Project-opening, notification-inspector, terminal-rename, initial empty/restore,
and inspector surfaces are component modules rather than inline `main.tsx` definitions;
the standalone visual states are represented in `/ux-demo`.
Application interaction registration is organized as a matching feature hierarchy in
`main.tsx`: project lifecycle, repository, navigation/tabs, terminals, ticket selection,
views, commands/AI, notifications/links, search/composer, attachments, inspector/editor,
and shell/global behavior each have a named wiring boundary. The boundaries run in their
original order and retain the single `document.body` delegation root, so organization does
not alter event ordering, propagation, or dynamically rendered target support.
Both the real app and `/ux-demo` load `@kerfjs/ui/webawesome.css`, then
`clients/web/src/hot-sheet-tokens.css`. Kerf owns the generic surface, text, brand,
success/warning/danger, spacing, radius, focus, shadow, light/dark, and Web Awesome
theme contracts. The local file defines `--hs-*` only for Hot Sheet domain concepts
that Kerf cannot name: ticket-state rails, priority colors, terminal background,
reader scaling, and the shell-divider alias.
At this shared document boundary, an explicit root `data-theme` selects a single
`color-scheme`, overriding the library foundation's system preference without replacing
its palette. Catalog light/dark selection therefore controls components independently of
the operating-system preference and survives reloads (HS2-0DD4XQ).
The production shell, UX-demo chrome, and local Dev Review overlay all consume this
same contract. Raw product palette values are defined only in
`hot-sheet-tokens.css`; component, demo, and development-tool styles select semantic
surface, text, border, status, focus, overlay, and shadow tokens. Translucent effects
derive from those tokens with `color-mix()` instead of embedding a second palette.
User/provider category colors remain local application data because they are persisted
choices, not component styling. Component font sizes likewise use Web Awesome's named
`--wa-font-size-*` scale; components do not invent intermediate sizes. For example,
the view-mode notification badge uses `--wa-font-size-3xs` rather than a one-off
`.57rem`. Unit policy scans every client-owned stylesheet and rejects raw color values
outside the product-token boundary or numeric component font sizes, so new variants cannot silently
reintroduce either kind of drift. One-off layout geometry may remain local when it
describes an actual component measurement rather than a reusable visual meaning.
Actionable context-menu entries consistently pair their text with meaningful Lucide
icons; structural separators do not require icons.

- `TicketTagEditor` — **built**: shared tag list, compact add control, autocomplete,
  normalization, duplicate prevention, removal, and unsupported-provider state

### 4.4 Attachments — feature floor

- `TicketAttachments` — the existing attachment-list surface, with browse/drop input,
  durable row identity, accessible icon actions, and row double-click-to-open behavior;
  it remains shared rather than being duplicated by a separate `AttachmentList` component
- `AttachmentRow`
- `AttachmentPicker`
- `AttachmentDropZone`
- `AttachmentProgress`
- `AttachmentActions` — open, copy reference/path, reveal, download, remove
- `AttachmentPreview` where the media type and provider permit it

### 4.5 Notes and activity — feature floor

- The empty Notes message uses `ListInsetText` with horizontal-only geometry: its
  text aligns with the Notes heading and bordered content, while the owning section
  supplies the outer inset. It remains visible for read-only tickets and yields to the
  note composer or actual notes without adding vertical padding (HS2-D4VEE8).
- `NoteList`
- `NoteCard` — **demo built** with distinct regular, status, feedback-needed,
  feedback-draft, and activity presentations sharing stable author, timestamp, vertically resizable edit body,
  an optional banded AI completion `ConfidenceBadge` (all four rubric bands shown in the
  demo; HS2-DWTJ43),
  contained long-token wrapping, and note identity;
  `density="compact"` selects the tighter 11.2px/8px inset and 9.6px radius used for every
  kind in the TicketNotes list, while the default `comfortable` density keeps the canonical
  card inset; the demo shows both (HS2-7RY5GK);
  double-click enters a controlled editor whose Save persists and Cancel restores.
  In reader mode, regular/status notes remain directly editable, while feedback-needed
  and feedback-draft notes always render their Respond/Submit editor style. An uppercase
  `CHOICE` block in a feedback-needed note becomes Markdown-capable rounded options;
  selected options use a green border/fill and checkmark, support platform-additive and
  Shift-range selection, and coexist with inline and freeform replies. Hover/focus
  reveals an explicit Edit action in both inspector and reader, while
  the inspector toolbar provides the single Reader entry point from every inspector tab.
  Its layout follows a 16px card inset, 8px region rhythm, and 4px connected-cluster
  rhythm; the compact Activity variant uses 8px block / 16px inline insets. Rich
  Markdown choices remain an app-specific zero-or-multi-selection surface rather than an
  exclusive-choice `TabBar` or `Select`; control targets and indicators retain explicit
  geometry (HS2-4Y6SM9).
- `AIContentLabel` — **built** as the shared persistent attribution and limitations cue
  for AI-authored notes, conversation responses, and narration. The factual tool name
  and “may contain errors” text are included in the containing artifact's accessible
  name. Its optional thumbs feedback uses consequence language and is only exposed by
  production compositions that have a selected ticket where feedback can be persisted
  as an ordinary note; it is never a browser-only rating counter.
  `tone="inherit"` takes the surrounding heading color (the AIConversation message
  header) instead of the default quiet secondary color; the `AIContentLabel` UX demo
  shows both tones with and without feedback (HS2-7RY5GK).
- `RegularNote`
- `StatusNote`
- `FeedbackNeededNote`
- `FeedbackDraftEditor`
- `ActivityTimeline`
- `ActivityTimelineEntry`
- `NoteComposer` — **demo built** as the shared controlled create/cancel surface used
  by both TicketInspector and TicketReader. Provider note capabilities gate create,
  edit, and delete independently; deletion is an explicit provider operation.
- `NoteEditor`
- `NoteReaderButton`

Activity notes are durable ticket history and appear in both the complete Notes list
and the compact Timeline index. NoteCard's quieter Activity presentation differentiates
them without hiding their full Markdown body. Timeline uses an optional durable summary
headline and never repeats the body; older notes fall back to a deterministic bounded
first line. The separate rich AI activity stream can
feed a live timeline and, under HS2-3GRNZW, later propose distilled activity notes;
the two sources must remain visually and semantically distinguishable. Status-change
activity uses a concise past-tense action in the timeline (for example `Completed`,
`Moved to backlog`, or `Moved out of backlog`), while the durable note keeps the complete
from/to transition for history and auditing.
Native rich-event wiring remains tracked by HS2-SW655F.

The inspector's segmented tabs own the one-rem gap below the control. The scrolling tab
content starts with zero top padding and keeps its side/bottom inset, preventing the tabs
and content container from stacking duplicate vertical space.

### 4.6 Deliberate HS1 detail-panel parity

The HS1 detail panel was reviewed before defining the HS2 inspector. HS2 currently
retains category, priority, status, Up Next, title and identity, rendered/click-to-edit
Markdown details, blocked reason, tags, attachments, notes, and provider/update
provenance. Timeline and attachments move to dedicated tabs so the narrow Info view
stays readable; the reader dialog exposes those same tabs at a comfortable width.

Attachment actions and note composition/edit/delete are shipped with provider-capability gating and shared
Inspector/Reader state. Telemetry and review
proof are deliberately not generic always-visible fields: they will appear as
capability-aware sections when their underlying features and data contracts land.

## 5. AI drive, attention, and commands

### 5.1 Drive status — feature floor

- `ToolConnectionIndicator`
- `BusyIndicator`
- `ConnectionCountBadge`
- `DriveLauncher`
- `DriveSessionMenu`
- `StopDriveDialog`
- `DriveOutcomeNotice`

### 5.2 Permission flow — feature floor

- `PermissionRequestCard` — shared list/popup presentation with project identity,
  operation details when non-empty (no blank framed details box), optional visible-time
  automation countdown rendered flat and vertically aligned with the decision buttons,
  an icon-only Lucide pause action with an outcome-specific accessible tooltip, and
  capability-aware Ignore/Deny/Always Allow/Allow Once actions. The one-second timer
  updates only its own text node so settings popups retain identity and open state;
  stopping it removes the complete automation presentation and leaves the request pending.
  Its UX demo settings preview popup/list presentations; pending, resolving, failed,
  disconnected, allowed, denied, and externally resolved states; command/edit/read/
  detail-free tool requests; allow/deny/no countdown; optional explanation; and the
  Always Allow capability, with a live 13-second countdown that reaches zero and resets
  for continued review, plus a complete settings reset round trip.
- `PermissionRequestPopup` — non-modal presentation of the shared request card. Its `layer`
  variant anchors it to its host's top-right corner (`inline`), places it in its host's layout so
  the host positions it (`flow`, the AI conversation's foreground), or lifts it into the browser
  top layer (`top`, the shell popup). The UX demo previews all three. The `flow` layer sizes to
  its host's content box (`min(704px, 100%)`), does not clip the card's shadow, and never takes
  the corner or phone placement. The phone placement (pinned 8px from both viewport edges at
  672px and below) applies only to `inline` and `top`. So the host's padding alone sets a `flow`
  popup's insets. The AI conversation's foreground covers the dialog panel (not the viewport) and
  opens the popup below the heading, with symmetric 32px insets (16px at 640px and below), fully
  inside the dialog (HS2-SH3DR7).
- `PermissionSummary`
- `PermissionDetailDisclosure`
- allow/deny/session-scope actions
- timeout, already-answered, disconnected, and competing-client states

### 5.3 Custom commands — built

- `CommandNavigation` groups locally configured commands and projects running state
  from the server event stream. A normal activation starts the command; activating a
  running command opens an explicit stop confirmation. Each named group is an accessible
  independent disclosure whose per-project collapsed state survives reload; ungrouped
  commands remain directly visible, and the outer Project Commands disclosure still
  controls the entire section (HS2-J963JF).
- Press-and-hold opens the latest bounded output/history view. The button hover title
  exposes the latest run outcome without adding permanent sidebar chrome.
- A context or overflow menu owns alternate actions: “Run in new terminal” for shell
  commands and capability-aware “Create task from command” for AI commands. These do not
  replace the history gesture (HS2-NT3F3Q).
- `CommandRunDialog` owns output and cancellation presentation. Its dialog surface uses a
  24 px major inset, 16 px between regions, 8 px within output/action groups, and 4 px between
  an action icon and its label (HS2-4Y6SM9). Completion and stop changes arrive through the
  existing long poll; the client never interval-polls. Both native dialogs style only their
  own `command-run-dialog` classes (root, `__title`, `__output`, `__actions`) and size their
  action icons through the `LucideIcon` `size` prop, never through `[data-component]` roots or
  descendant `svg` selectors (HS2-29Q3XG). The `/ux-demo` entry exposes both presentations
  through a Presentation setting (Run output, Stop confirmation). Each swap reopens the native
  `<dialog>` with `showModal`, and an Open dialog stage action reopens it after Close, Keep running,
  or Stop command, which the demo reports as fixture events (HS2-CWWX7S).
- `CommandSettingsEditor` (Project Settings → Commands) is an HS1-style WYSIWYG editor:
  a sidebar-aligned grouped list of command rows (drag handle, colored icon, name, type)
  whose group headers come from each command's `group`, with ungrouped commands at the top.
  Rows reorder by pointer drag-and-drop within and across groups (dropping into another
  group changes membership); there are no up/down arrows. Rows are multi-selectable —
  click to select, Cmd/Ctrl-click to toggle a row, Shift-click to select a range — and
  dragging any row of the selection moves the whole selection together as one block,
  preserving relative order and adopting the drop target's group (HS2-VJYQHG). Each row
  exposes an overflow "…" menu — also opened by right-click — with Edit and Delete, and
  double-clicking a row edits it. The heading pairs a secondary "Add group" with the brand-filled primary
  "Add command"; the editor styles only its own buttons (`command-settings-editor__button`), so nested
  components such as the icon picker keep their geometry (HS2-JSSMFY). An "Add group" button appends an empty group (droppable, with a delete button
  while empty). Groups are kept until deleted (HS2-EZ5KMC): an added group, and any group whose last command
  is deleted or dragged elsewhere, stays in the list as an empty, droppable group, saved immediately to the
  project's machine-local `command_groups` setting beside `commands`, so it survives a reload; only its
  delete button removes it. Editing — or adding — a command opens a native "Edit command" popover
  dialog holding the typed detail form — Button label, Type, the Program `{program,args}` /
  Shell / AI-prompt fields, an optional confirmation message, the color-swatch picker, and
  a searchable Lucide-icon picker — with Done in the dialog toolbar. The identifier (auto-generated),
  group (set by drag-and-drop), and working directory (always the project root; users `cd`
  within shell commands) are intentionally not shown. The color palette's neutral slot is
  "Transparent": a command with that color (or none) renders with no background fill and the
  default button styling, in the editor list and the sidebar alike. Field edits,
  reorders, and deletes autosave to a project-scoped draft on a debounce (no explicit Save
  button), surfacing a status line and validation errors. AI commands additionally use the
  shared Provider/Model/Effort submenus: **Project Default** stores no override, while an
  explicit provider/model/compatible-effort selection (including an exact-id `Other…` model)
  persists with the command and is sent with its `$hotsheet` notification. The popover dialog only takes its
  flex layout while open (`:popover-open`) so a closed, empty dialog stays UA-hidden rather
  than painting a stray strip. Named AI prompts use the same safe contract by invoking an
  appropriate configured CLI command. The retired worker target picker is deliberately
  absent; drive targeting remains a separate control (HS2-656XJ2, HS2-D9JBXT).
- `LucideIconPicker` (`components/lucide-icon-picker.tsx`) is the reusable searchable icon
  picker used for the command's Button icon. A search field sits above a grid: with no query
  it shows a curated set of bundled "popular" icons (`components/lucide-popular.ts`, always in
  the main bundle so defaults and legacy command icons render synchronously); typing a query
  searches the full Lucide catalog, which is lazy-loaded as a separate chunk on first use
  (`lucide-catalog.ts`, `loadLucideCatalog()`), so any of the ~2000 icons can be assigned. The
  host owns state: it passes the current icon name and query and wires the search input
  (`name="command-icon-search"`) and per-icon `data-action="select-command-icon"`
  (`data-icon-name`) buttons through Kerf delegation. `components/command-icon.ts`
  (`resolveCommandIcon`) maps a stored icon to a renderable node — resolving legacy keys
  (`test`→`test-tube-2`, `build`→`hammer`) and falling back to a default until the catalog
  loads — and the sidebar/editor read `lucideCatalogVersion` so custom icons appear once the
  catalog is ready (HS2-5VSNV3).

### 5.4 Notifications — presentation begins at feature floor

- `ToastRegion` and `Toast`
- `NotificationBell`
- `NotificationCenter` — pending requests followed by newest-first resolution history;
  externally resolved requests remain visible with a neutral outcome message. The canonical
  12px card gap applies within each group and between pending requests and history
  (HS2-D38KZF). `inset="page"` (default) pads it as a centered workspace page; `inset="flush"`
  drops that padding where the host already insets content, as the ticket rail's sunken list
  does (HS2-8FS5BJ)
- `AttentionBadge`
- `NativeNotificationRouter`
- `NotificationPreferences`

Off-server/mobile push remains a later transport; clients consume the same normalized
notification model.

## 6. Bottom drawer and terminals

### 6.1 `BottomDrawer` — production built

- `DrawerTabBar`
- `AppTab` / `DrawerTab` — shared with project tabs; close action precedes the label,
  supports leading/trailing icons, participates in sequential Tab order whether selected or
  not, supports arrow/Home/End navigation and Delete/Backspace closing, and exposes the same
  close/others/directional/all menu
- `AddDrawerTabButton`
- `DrawerVisibilityButton`
- `DrawerResizeHandle`
- tab kinds: terminal first; activity, command output, or other tools may follow

The shipped terminal drawer is the Workbench's bottom drawer (center column only) with a
compact grid/terminal tab rail, explicit new-terminal action, hidden-session recovery,
persisted 228 px-to-workspace-boundary height, Kerf-managed fade/slide collapse, popup
overflow, and a safe-area-positioned floating restore button when collapsed. The
splitter resists below 228 px and treats a continued 48 px overshoot as an intentional collapse.
The rail follows Kerf's canonical spacing relationships: 8 px within its toolbar groups and
terminal inset, 4 px for the tab-strip focus gutter and icon-label air, and no gap between the
connected rows in its create menu (HS2-4Y6SM9).
On mobile, focusing the active dedicated xterm temporarily replaces that drawer chrome with a
full visual-viewport terminal and one Exit pill. The terminal tracks the visual viewport's
offset and height while the software keyboard opens, so it never extends underneath the keyboard.
Explicit Exit restores the selected terminal tab; desktop transitions, drawer hiding, and terminal
replacement clear the ephemeral mode rather than restoring it later (HS2-GMTQZM).
Terminal tabs can be pointer-dragged into a new same-project order, which survives session
list refreshes for the current client lifetime. Grid and
dedicated tabs attach viewports to existing sessions; only the plus action creates a PTY.
Activity and command-output tab kinds remain later extensions.

### 6.2 `TerminalPane` — desktop feature

- `TerminalSessionHeader`
- `TerminalViewport`
- `TerminalStatusLine`
- `TerminalSearch`
- `TerminalActions` — focus, stop/close, delete, overflow
- `TerminalSizeMismatchNotice` — actual PTY dimensions, driving viewport, and
  “resize to this screen” action
- `TerminalViewportClaim` behavior — focus, visibility, size, heartbeat, and lease
- letterbox, scale-to-fit, readable-floor, and scroll presentation states
- `TerminalGrid` and `MagnifiedTerminal` shared by the project drawer and global dashboard

The browser mock should simulate output, focus ownership, resize arbitration, session
completion, and disconnection without spawning a PTY.

### 6.3 `TerminalGrid` and `TerminalDashboard` — desktop feature

The project bottom drawer and global Terminals screen share one tile-grid contract. The
drawer keeps its compact tab rail above the grid; the global screen removes the project
sidebar and ticket inspector, keeps the project-tab strip for navigation, and groups live
terminal tiles in one ungrouped flow. Visibility lives in the main toolbar, while zoom remains anchored to the grid's bottom
right corner. Each tile has a 4:3 preview, terminal and project identity,
busy/idle/exited state, and pending-attention treatment. An empty project is omitted from
the global grid unless it is the only available project, in which case the screen explains
how to create or open a terminal.

Styling ownership (HS2-DR549A):

- **Drawer layout.** The dashboard styles its own `drawer` layout (`layoutMode="drawer"`) with a tighter
  top inset. That layout anchors the zoom toolbar at the drawer's edge, which the Workbench has
  already padded by the home-indicator inset. The drawer never reaches into the dashboard.
- **Buttons.** Only the tile's own footer actions get app button chrome. The zoom toolbar's and
  key bar's Kerf control groups own their buttons. One temporary rule restores the not-allowed
  cursor on a disabled zoom button until KF-FTADQT ships; its CSS-ownership allowlist entry is
  tracked under KF-FTADQT itself.
- **Visibility Select.** It uses Kerf's `toolbar-borderless` compact presentation, with a
  group-owned focus ring.
- **`TerminalSession`.** `TerminalDashboard` renders and styles it; `TerminalDrawer` composes it
  with props only (HS2-YNW0B3). `mobile` (the app's phone layout) clips its scaled xterm, and
  `focus` (drawer focus mode) drops the viewport's `--kui-space-xs` inset so the terminal fills
  the focused surface edge to edge. Its dedicated rules follow the shared viewport rules so they
  win in every connection state.
- **`TerminalPreview`.** The live, non-interactive scaled terminal preview fills its positioned
  container and owns its frame, 1280×768 scaled canvas, and connecting fallback, which hides once
  the viewport connects or reconnects (HS2-148B5C). Containers tune only its public tokens:
  `--terminal-preview-inset` (frame inset, default `--kui-space-l`) and
  `--terminal-preview-radius` (frame radius, default `--wa-border-radius-m`). `ProjectCloseDialog`
  composes it and narrows the inset to `--kui-space-m` on phones; its UX demo now switches between
  the chat and terminal previews. Its canvas takes the dashboard tile's preview-scale contract:
  every `scaled-preview` viewport, with or without the dashboard's 80×24 grid policy, is scaled by
  `terminalPreviewScale(frame)` (exposed as `data-preview-scale`) in both the live and static
  runtimes, so all lines fit inside the frame at every width instead of a native-size crop
  (HS2-S7E53Q). The preview stays non-claiming, so a borrowed terminal keeps its PTY geometry.

Grid scale is a discrete fit count controlled by icon-only minus/plus buttons with visible
tooltip and accessible names. Plus zooms in (fewer terminals on the controlling axis);
minus zooms out (more terminals). The active count is announced as “N across” or “N high”:
the visibility-group label select constrains its menu to its compact trigger width so its
popup remains wholly inside the viewport.

- when the grid container is taller than 600 px, scale means how many terminal tiles fit
  across the available content width, preserving HS1's integer 1–10 column model;
- when its height is 600 px or less, scale means how many terminal tiles fit in the
  available content height, clamped to 1–3. Tiles then continue in additional horizontally
  scrollable columns rather than shrinking below the chosen height;
- the width and height modes retain independent values (defaults: 4 across and 2 high), so
  crossing the 600 px boundary does not destroy the user's prior scale on either side;
- the exact 600 px boundary uses height mode. Resize observation recomputes geometry but
  does not change either stored count. Minus/plus disable at the active range limit.

The bottom drawer specializes that scale: it always offers levels 1–3. Level 1 is one
full-height row (never shorter than 160 px) with additional terminals flowing horizontally.
Levels 2 and 3 are columns across the drawer width, wrap left-to-right, and overflow
vertically. Layout math treats a drawer narrower than 240 px as 240 px. During a held
splitter gesture, existing dedicated and grid terminal geometry remains frozen; xterm fit,
tile recomputation, and PTY size claims happen once after pointer release.

At every scale, dashboard tiles are keyboard-focusable, non-interactive previews: each live
xterm retains a fixed 1280×768 natural geometry and the complete terminal is uniformly
scaled into the tile instead of being refit to the tile. This keeps the PTY stable as grid
zoom changes and keeps the preview, inset frame, and unused terminal area on the terminal
background color. Only the black viewport follows that 5:3 aspect; the borderless tile card adds its
measured spacing-token inset and footer height outside the viewport. `FixedAspectTerminalCard`
owns this structure for both grid-preview and magnified-interactive variants. It withholds the
xterm surface until bounded font-metric fitting is stable, so transitions do not expose each
intermediate fitting pass. A plain activation
magnifies and focuses an interactive copy in place over the same grid. During magnification,
the matching grid card keeps its geometry and terminal-background fill but unmounts its preview
xterm and viewer, leaving the fitted magnified viewport as the terminal's only local sizing
claimant. Clicking the surrounding overlay or pressing Escape restores and remounts the 80×24
grid preview. Interactive magnified and dedicated variants
turn current, legacy, and project-qualified ticket references in xterm's parsed buffer into
pointer-underlined links that open the stacked ticket reader. Selecting text within a link
keeps the reader closed; clicking after clearing selection opens it normally. Preview tiles remain inert, and
the interactive magnified font fits without a CSS transform so link hit-testing and ordinary
terminal selection stay on the rendered cells (HS2-2DW829). A double
activation, or Open from the tile's shared ListItem-based context menu, jumps to that
project and selects the terminal in a maximized drawer. The same context menu offers Hide
Terminal. A permanent Lucide ellipsis footer button opens the same shared Open/Hide menu as
right-click. These actions must never spawn a
second PTY. The eye opens `TerminalVisibilityDialog`, built from the shared dialog, Select,
ListHeader, and ListItem vocabulary. Adding prompts for the name before creation; pill tabs
select groups and named-tab context menus rename or delete them, while Default has no context
menu. Rows include both terminals and AI chats with their existing stable visibility keys.
A Web Awesome native multi-select, styled with Kerf's public Select class, filters Shell
Terminals, AI Terminals, and AI Chat. Kerf's current Select API is single-valued, so this
composition uses the native multiple contract and one controlled array/property bridge;
the static native control is a morph-skipped island so its open popup and focus survive
list updates. Select All / Deselect All keep the popup open; Hide listed / Show listed
change only matching rows, with disabled bulk actions for an empty filter. Reopening resets
all supported types, while named-group changes retain the filter. Web Browsers remains
disabled pending HS2-7VS6SF. Creation kind is immutable server/broker metadata, with shell
fallback for legacy payloads (HS2-SE3RVM). An
adjacent compact label Select switches immediately, while an eye badge reports the active
group's hidden count. The project drawer always shows its local terminals and exposes no
visibility controls. Its UX demo initially presents an explicit Manage Workspace Visibility button;
the dialog opens only after activation and can be dismissed and reopened repeatedly.
New terminals and AI chats appear in Default and begin hidden in named groups. Visibility and grouping
changes never destroy sessions. The focused magnified or drawer consumer
must reclaim its fitted dimensions after leaving the dashboard. Focus, resize claims,
attention, and selection survive layout and scale changes. Magnification promotes the terminal
workspace above neighboring shell dividers and disables its enclosing focus outline without a
transition, preventing sidebar chrome from crossing the dimmed modal. Fixed dashboard consumers
also discard only an exact leading reverse-video zsh partial-line marker from each connection's
initial replay; normal `%` content and project-drawer streams remain byte-for-byte intact.

The operations sidebar separates its aggregate `All projects` summary from individual projects
with an inset divider and normalizes all displayed completion trends to the aggregate maximum.
Each individual project layers the aligned aggregate trend as wider neutral-gray bars behind its
brand-blue bars; the aggregate row itself remains the only green chart and has no redundant layer.
The aligned-series aggregation and explicit chart-domain contract are reusable for future
cross-project statistics. During remembered-project startup, the AppShell and terminal drawer remain
unmounted behind one stable restoration status until the active project's tickets, visible terminal
resources, and the bounded retry pass are ready, then healthy tabs and red error tabs appear together without a mixed intermediate layout.
Selecting an error tab uses the shared `EmptyState` primitive for exact failure context, recovery
guidance, and retry without pretending the unavailable checkout has live sidebar or inspector data.

`FixedAspectTerminalCard` is also a first-class UX catalog entry. Its catalog page renders
every supported public variant with the real xterm frontend — grid preview, magnified, and the
phone magnified variant (`mobile` prop, `data-mobile-chrome`) with its top toolbar and with the
keyboard presented (toolbar hidden; HS2-WMN626) — a deterministic ANSI fixture whose
reverse-video Nano bars explicitly paint all 80 terminal cells,
and representative preview-versus-magnified sizing. `TerminalDashboard` lists it as a related
component so the production composition is explicit rather than only inferable from source.
The card is self-contained (HS2-0X36TX): its `--terminal-tile-frame-inset` and
`--terminal-tile-footer-height` tokens and its preview styling hang off the `.terminal-tile` root,
so it renders the same with or without a `TerminalDashboard` around it. Its `fit` prop chooses the
sizing: `grid` (default) takes the tile size its grid sets, and `aspect` takes the container's
width and an intrinsic height with the viewport at 5:3. The catalog stage renders every variant
with `fit="aspect"` and styles only its own grid shell.
Late development-only source metadata waits for every open catalog Select or related-components
dropdown to finish closing before it rerenders the shell, so background discovery never dismisses
the popup a user is reading (HS2-S59CRP).

`TerminalTicketRail` is the dashboard's compact right-side companion. It composes the shared
content-sized project and view selectors, a rectangular full-row list/columns/notification switch (its columns view is the paged, snapping `TicketBoard` the phone uses, HS2-656Q43), the
same chip-based advanced search control used by the main workspace (placed last on its action row),
ticket list, quick-ticket launcher, and ticket inspector inside a Kerf NavStack navigation panel
(HS2-FY06N4).
The header/scroller boundary has a quiet one-pixel separator. Its launcher is the same blue pill
component used by list and column views, with the compact rail label `Ticket…` to prevent wrapping
and the launcher's own `size="compact"` (a 36px trigger) rather than a rail override. Its
notifications view asks `NotificationCenter` for `inset="flush"`. The view title is a Kerf `Select`
whose 36px, flush-start geometry and inset focus ring use the rail's own className hook until Kerf
ships a title presentation (`KF-PZ23ZP`). The rail stylesheet styles no other component
(HS2-8FS5BJ).
Its inspector route is one toolbar row: Kerf's back control, the ticket number, the ticket actions,
and the rail toggle; the catalog demo pushes a ticket on a plain row click and pops on Back.
The rail remains independently resizable and hideable beside the terminal grid and is represented
directly in the UX catalog. It already composes Kerf `Toolbar`, `ToolbarControlGroup`, `ToolbarText`,
`SegmentedControl`, and `Select` primitives. Its shared `WorkspaceControls` groups are the
trailing zone of a cataloged `Toolbar` (`responsive="stack"`, `responsiveAt="narrow"`): the rail
is always narrower than that breakpoint, so the zone stacks and wraps at group granularity, giving
the full-width view switcher its own row, sort plus selection actions plus the trailing-aligned
collapsible search the next, and an expanded search a full row of its own with the same enter
animation (the search field's `layout="row"`); the toolbar's own 8px inset and gap replace the
earlier app-owned grid (HS2-K9KWJJ, superseding the 4px connected top inset from HS2-4Y6SM9).

## 7. Overlays and shared interaction components

- `PopoverMenu`
- `ContextMenu`
- `ConfirmationDialog`
- `ErrorDialog`
- `FormDialog`
- `KeyboardShortcutHelp`
- `CommandPalette`
- `Tooltip`
- `ProgressIndicator`
- `InlineError`
- `EmptyState` — **built through `@kerfjs/ui`** with ticket-specific copy and states
- `LoadingSkeleton`
- `RelativeTime`
- `CountBadge`
- `IconButton`
- `MarkdownSurface`
- `ProviderCapabilityNotice`

On the web, these should use Web Awesome Core where it supplies the needed accessible
primitive. Hot Sheet components wrap those primitives with domain behavior and stable
semantic actions; Kerf owns state and composition.

`ContentTransition` is the shared two-screen navigation primitive. It retains stable
A/B DOM sides and supports paired forward push, backward pop, crossfade, and no-motion
replacement. Inactive content is inert and hidden after the transition. Slotted `label` and
`footer` regions span their dialog slot. The `sideLayout` variant sets how each side lays out
its children: `block` (default, normal flow) or `actions`, an end-aligned, wrapping action
row with the shared extra-small gap for dialog footers. Consumers choose the variant and
never style `.content-transition__side` themselves. An action that belongs at the row's
start, such as the ticket-source setup root Cancel or Remove/Disable, sets its own
`margin-inline-end: auto` (HS2-29Q3XG). Its UX demo exposes every transition style, both
visible sides, and both side layouts (block content, action-row footer).

`FlowBackButton` is the corresponding in-content back affordance for multi-screen dialogs:
a quiet brand-colored chevron and destination label at the start of the detail screen.
Back belongs there rather than beside Cancel/Submit in the footer. Ticket-source setup and
conversation save share this visual contract; save scope pushes forward and pops backward.

## 8. Setup, settings, and connection management

### 8.1 Initial client path

- `WelcomeScreen`
- `ServerConnectionForm`
- `LocalServerStatus`
- `ProjectDiscoveryList`
- `AddProjectFlow`
- `RecentProjects`

### 8.2 Later guided setup

- `FirstRunWizard`
- `ToolDetectionResults`
- `ToolSetupOffer`
- `MigrationOffer`
- `MigrationProgress`
- `RemotePairingFlow` and QR presentation/scanning

### 8.3 Settings

- `SettingsWindow`
- `SettingsNavigation`
- `SettingsSection`
- `EffectiveSettingField`
- `SettingScopePicker`
- `SecretReferenceField`
- `TicketProviderConnections`
- `ProviderIcon` — **demo built** (HS2-PK8THJ): the GitHub, GitLab, and Jira brand marks used by
  the ticket-source setup dialog, the provider setup form, and the ticket-source settings rows.
  `size="m"` (default) follows the surrounding font size (1em); `size="l"` is the fixed 24px
  identity mark on the Accounts card header. The icon owns its size and has no border or padding.
  The `provider-icon` UX demo shows every kind at both sizes.
- `ToolPluginSettings`
- `NotificationSettings`
- `TerminalSettings`
- `AppearanceSettings`

## 9. Later major surfaces

These remain in the component architecture but are not initial-client blockers:

- `TerminalDashboard` — interaction contract settled in §6.3; the global screen,
  responsive controls, shared interactive WebSocket/xterm viewport, and project drawer
  composition are built.
- `AnalyticsDashboard` — throughput, cycle time, category, usage, and cost charts
- `CustomViewBuilder` — query construction and saved-view editing
- `AnnouncerOverlay` — digest picture-in-picture, live narration, playback controls,
  provider/voice choice, and diff visuals
- `PrintPreview` and export/copy surfaces
- `CrossServerWorkspace`
- `FileViewerLauncher` and `DiffReviewLauncher`

## 10. `/ux-demo` catalog and mock support

`/ux-demo` is a development route in the real web client, not a separate throwaway
component implementation. Its Hono route is registered only for Vite `serve`, whose
host is fixed to loopback; it is absent from the production build. The initial shell
uses `@kerfjs/ui/catalog`: `Catalog` owns its responsive sidebar, titled detail canvas,
footer/status surface, related-component menu, theme control, and collapse control,
while `wireCatalog` synchronizes the selected entry with `?component=`. The nested Hot
Sheet inventory is flattened into path-labelled Kerf sections; phase, implementation,
and dependency metadata remain visible as tags and native related menus. The app retains
ownership of the selected entry and persisted collapsed/theme state, its development-only
Dev Review toggle, and an optional manually closed settings inspector
that keeps the demo visible during live adjustment. The settings action lives with the
other catalog-header tools; while the inspector is open, its Close settings action stays
viewport anchored. Stateful Web Awesome control properties stay synchronized when a demo
reset restores its canonical mock state. The remaining catalog review-tooling package is
tracked by HS2-89692E. It should grow to provide:

Kerf beta 18's native geometry overlay replaces the catalog's local alignment-outline
mode. Focused `component` entries always pass `geometryOverlay={true}` so transparent outer
bounds receive a dashed outline and positive computed margins receive orange bands;
`composition` entries explicitly pass `false`, because their outer placement belongs to
the embedding layout. This behavior is automatic rather than hidden behind a demo toolbar
toggle. `wireCatalogGeometryOverlay` keeps the layer synchronized across
controlled renders and resize, while explanatory content may opt out with
`data-catalog-geometry-overlay-skip`. Long desktop sidebars reveal both the initial deep
link and later controlled selection without moving keyboard focus; compact layouts retain
Kerf's default no-forced-scroll guard.

Hot Sheet publishes its package-qualified consumer metadata in
`clients/web/ai/component-catalog-extension.json`. Its generated entries record every
implemented app-owned component or composition's purpose, appropriate and inappropriate
uses, public CSS/token hooks, documentation, and explicit margin/border/padding owner.
`clients/web/scripts/sync-component-catalog-extension.mjs` derives that extension from
the catalog inventory, and `npm run catalog:check` prevents drift. AI and human consumers
must search this extension before the installed Kerf component catalog, retain the source
package identity, and use the ownership fields to avoid duplicate wrappers or insets.

Implemented entries' tags include a dependency-aware last-modified time: changes to a demo, recursively
imported component/style dependencies, or global catalog code make the demo current again.
Planned entries retain an explicit Planned tag. A development-only header toggle enables
or disables Dev Review without coupling that state to component selection.

- a searchable component index grouped by the sections above
- isolated examples plus composed screen scenarios
- viewport presets approximating desktop web, Tauri/macOS, and narrow inspector states
- light/dark and reduced-motion controls
- keyboard navigation and focus demonstrations
- deterministic scenario selection through the URL for review and browser tests
- visible event/action logs so mocked interactions communicate intent

The mock layer should implement the same client-facing service interfaces as the real
HTTP/WebSocket adapters:

- seeded projects, provider connections, tickets, notes, attachments, terminals,
  commands, activity, notifications, permissions, and repository status
- configurable latency and failures
- live event playback and reconnect behavior
- provider capability variants
- optimistic success, server rejection, conflict, and stale-data scenarios
- a reset action that returns every demo, rendered output, and live inspector control
  property to a deterministic baseline without closing the inspector; a subsequent
  edit must still propagate normally

Every stateful demo's browser contract walks the complete round trip: assert its initial
controls and output, change every exposed setting, reset or replace state, assert every
live control property and output, then edit once more. Tests exercise every visible
action. For Web Awesome elements, `value`, `checked`, focus, and relevant emitted events
are authoritative test surfaces; matching attributes alone do not prove UI state.

Initial composed demos:

1. Full list-mode shell matching the supplied wireframe.
2. Ticket inspector editing, reader escalation, notes, and attachments.
3. Ticket list states and keyboard/multi-selection behavior.
4. AI busy state and permission request flow.
5. Bottom terminal drawer with two sessions and viewport-size mismatch.
6. Multi-project tabs with local, remote, reconnecting, and attention states.
7. Empty, loading, error, offline, and unsupported-provider states.

## 11. Review questions

The first review should settle these before visual polish:

- Whether the sidebar, inspector, and terminal drawer are independently collapsible
  and how their restored sizes behave.
- Whether list selection opens the inspector immediately or follows a platform-specific
  single/double-click convention.
- What the sidebar percentage represents in the wireframe.
- Whether category/type is primarily icon-only in dense rows.
- Whether project tabs represent projects, server connections, checkouts, or a user-facing
  name over their combined identity.
- Which controls belong persistently in the top toolbar versus an overflow menu at
  narrower widths.
- Whether the terminal drawer is part of the initial feature-floor demo or the first
  follow-on desktop slice.

### Kerf beta.24 layout adoption

Settings and notification navigation, command rows, connected provider rows,
ticket-note cards, and terminal operations summaries use the package `List` for
vertical layout and explicit gaps. Their existing navigation landmarks, actions, and
pane/workspace scroll owners remain unchanged. `List` adds no list semantics;
semantic `ul`/`li` collections and ticket listboxes retain their existing structure.
The terminal rail lists tickets on its NavStack view's sunken scroll surface, and divider-free
toolbars use `dividerSides=""`. These compositions are exercised in their existing UX
demos and production flows.

The dedicated **List** demo exposes compact, standard-gap, and custom-gap scrollable
examples using the package component unchanged, including explicit edge dividers.

### Kerf beta.59 adoption

HS2-MYVVK3 moves `clients/web` and `spikes/kerf-webawesome` to Kerf 5.0.0-beta.59 (`kerfjs`,
`@kerfjs/ui`, `eslint-plugin-kerfjs`). Beta.59 fixes the duplicated `KUI-L201` report for a
`rendersAs` wrapper (`KF-KV08Y0`), so `TicketSearchField` and `WorkspaceControls` now declare
their Toolbar parent directly in `clients/web/ai/component-composition-extension.json`
(HS2-N0R5W0). Its composition catalog still omits the FloatingToolbar → ToolbarControlGroup
edge, so the version-pinned adapter in `clients/web/scripts/check-kerf-ui-doctor.mjs` now
covers beta.58 and beta.59 (HS2-10KEHN remains the upstream tracker). Beta.59 also adds an
opt-in managed `TokenSearchModel` (grammar, suggestions, evaluation) for `TokenSearchField`
and extends `PopupMenu` for nested and context actions; Hot Sheet kept its app-owned
`TicketSearchField` suggestions until beta.62 closed the model's API gaps (the
`TokenSearchModel` evaluation below, `HS2-HHRYP9` and `HS2-06Q4MG`; adoption in `HS2-5JXBQY`). HS2-CSRJ9Y moved every button-triggered command menu onto
`PopupMenu`: the workspace overflow menu (`WorkspaceOverflowControls`, `data-workspace-overflow`
root, items carrying `data-workspace-overflow-kind`/`-action`/`-state`), the command editor's
per-row Edit/Delete menu and its AI configuration menu, and the in-conversation model menu
(`data-conversation-model-menu`). The shared Provider/Model/Effort submenus now exist in two
forms from one module: `providerModelEffortEntries` (typed `PopupMenuEntry[]`, checked choices,
`data-value` attributes for the existing click handlers) for those menus, and the JSX
`ProviderModelEffortSubmenus` still used by the pointer-positioned menus. HS2-2EHD8R then moved
those pointer-positioned menus (drive options, ticket row, attachment, app tab, saved view,
terminal tile) onto `PopupMenu` context mode and retired the JSX submenus, so every command menu
is a `PopupMenu`. Each menu still renders from its app signal (the signal owns dismissal, and the
menu exists only while it is set); its app-owned wrapper keeps the `role="menu"` name tests and
the capture-phase dismissal use and records the pointer as `data-context-anchor-x`/`-y`; one
`effect` per signal (`revealContextPopupMenu` in `clients/web/src/context-menu-position.ts`) hands
that anchor to Kerf's `openPopupMenuAt` once the morph has placed the element, retrying through a
microtask and a frame. The Drive menu re-reveals after each provider/model/effort choice because
Web Awesome closes a menu on selection while the app keeps it open; its anchor is the drive row's
top-left corner (`placement="top-start"`). A command-editor row's right-click opens that row's
PopupMenu in place. Raw `wa-select` pickers moved to `Select` in `HS2-CWA0S6`. HS2-W0N1KP then
converted the UX demo stages' hand-written pickers (the catalog inspectors' scenario, presentation,
status, appearance, variant, size, priority, and category selects, plus the ToolbarControlGroup
demo's popup menu) to `Select` and `PopupMenu`; the demo keeps staged context menus open through
the same helper and mirrors Web Awesome's `wa-hide` into its demo menu state in a later task, since
its bubble-phase Escape handlers run after Web Awesome consumes the key and a selection's item click
must finish dispatching before the menu state clears.
`PopupMenu` submenus cannot hold a divider (`KF-7KR1BC`), so the Model submenu's separator
before "Other…" is absent in the entries form. HS2-CWA0S6 then replaced the raw `wa-select`
value pickers that Kerf `Select` covers: the Permissions settings' automatic-decision and delay
selects (`settings-workspace.tsx`) and the ticket-source setup dialog's preview-state picker
(`ticket-source-setup-dialog.tsx`, demo only). The terminal visibility dialog's "Item types"
picker kept raw `wa-select`/`wa-option` markup until `KF-F68TJS` shipped disabled choices and
bulk actions in beta.62; HS2-8ZC1YB then moved it onto a controlled multiple `Select` with
`disabledReason`, `selectAllLabel`, and `clearLabel`, deleting the app's morph-skip island and
its MutationObserver value sync. `KUI-L301` (discouraged Web Awesome elements) fell from 108 to
102 with the PopupMenu adoption, to 94 with the Select adoption, to 76 with the context-menu
adoption, to 9 with the demo-stage conversion (HS2-W0N1KP), to 2 with the picker adoption, and
to 0 when HS2-84751P rendered the visibility dialog's group context menu as a context-mode
`PopupMenu` (revealed through the shared `revealContextPopupMenu` helper at the pointer, with the
Rename… and danger-toned Delete items). It is a warning-level rule without an exact budget,
so the remaining count is tracked here rather than in the doctor script. The related warning-level
`KUI-L401` (a module rendering `Select` or `PopupMenu` should import its `register` entry) stays
accepted: the app registers those elements once in its entry points, because the register modules
touch `document` at import time and would break the node-side component unit tests. Beta.59 also sizes a compact PopupMenu trigger from its ToolbarControlGroup's `size="compact"` prop, so the terminal drawer's create menu uses that prop instead of an app `::part(base)` rule; the last budgeted `KUI-L011` finding is gone (`KUI-L011` 1→0) and every other budget is unchanged.

### Kerf beta.57–58 adoption

Beta.57 fixes delegated action matching when an earlier handler synchronously rerenders
and recycles the event target. Beta.58 adds `Toolbar` leading/center/trailing composition
and `ToolbarControlGroup.relocateOnCollapse`; these are available to future panel work
without changing Hot Sheet's existing toolbar layouts. The UI doctor now diagnoses
previously unreported component anatomy and styling ownership. Hot Sheet pins the first
beta.58 report by diagnostic ID, then reduces that baseline in HS2-GTX61Q
(HS2-SM3JK0). The Claude Code kerf-app drop-ins were updated to 1.25.0.

### Kerf beta.56 adoption

Beta.52–56 (HS2-QCSPDH) changed several things Hot Sheet depends on:

- **Custom elements own their `role` and `aria-*` (KF-KQWZ8M).** A re-render no longer removes a
  custom element's host `role` or `aria-*` just because the template omits it. A conditional
  attribute on a Web Awesome host (`wa-dialog`, `wa-dropdown-item`) therefore always renders an
  explicit value — `aria-hidden="false"`, `aria-modal="false"`, `aria-current="false"`, or an empty
  `aria-describedby` — rather than relying on the morph to remove it; otherwise a dialog opened after
  being hidden kept `aria-hidden="true"` and dropped out of the accessibility tree.
- **Catalog on Workbench.** The UX catalog is a Workbench of Panes, Toolbars, and Lists: the demo
  imports those primitives' CSS, gives its root a definite height (`.kui-app-root`), and passes its
  collapsed signal to `wireCatalog`, so the sidebar becomes a transient overlay on a small screen.
- **Toolbar control band (KF-KM1E5V).** Toolbar zones top-align against one control band; an
  app-owned control in a zone (the inspector's slug button) sizes itself to
  `--kui-toolbar-group-size` to stay centered in it.
- **Icon tile (KF-XZD841).** A direct icon in a `ToolbarControlGroup` is a 16px glyph centered in the
  control slot; heading glyphs use it instead of an app-owned 22px size.
- **Floating toolbar inset.** Kerf reads `--kui-floating-toolbar-inset` as an optional override, so
  app `calc()`s spell out its default; a ResizableRegion restore corner owns the inset for a restore
  toolbar composed there.
- **`AppTab` owns `data-pending`.** Project tabs publish their opening state as the app-owned
  `data-project-pending`.
- **Visual tokens.** Light-mode brand and danger fills darkened for WCAG AA contrast, and
  Web Awesome reflected defaults (for example a divider's `role="separator"`) render in Select
  options.

Beta.55 also shipped a restore-corner `:has()` rule that made style recalculation about 7× slower;
Hot Sheet stayed on beta.51 until beta.56 fixed it (KF-MEV7Q1).

### Kerf beta.51 adoption

Beta.51 (HS2-KMDJRH) changed four things Hot Sheet depends on:

- **Custom-element `open` is user-agent-owned (KF-900A8V).** A re-render no longer removes a live
  `open`, so controlled dialogs mark themselves with `data-controlled-open` and
  `src/controlled-open.ts` drives the live state after each render. Uncontrolled popups keep the
  new protection.
- **`SelectChoice.color` accepts only foreground colors (KF-CW8DVX).** Category and priority
  choices use `foregroundColorVar` over app-owned tokens; urgent and low priority gained
  `--hs-priority-urgent` / `--hs-priority-low` aliases that keep their Web Awesome loud hues.
- **Icon-only Select centering and caret (KF-Y3YZBE, KF-VAV2JT).** Kerf now centers the trigger in a
  single-control group, so the app's centering workaround was removed; the whole single-control
  pill is the hover surface.
- **ListHeader markup and action axis.** Labels carry `data-border`, counts render as a quiet
  `Badge`, and a header action is centered on the shared trailing toolbar-action axis (KF-NRB76K),
  so the sidebar's Add view action lines up under the sidebar toolbar control.
- **Explicit grouped search.** The token-search groups now declare `content="search"` and
  `focusRing="halo"`, and their `TokenSearchField` uses `presentation="toolbar-group"`; beta.51
  replaced the implicit `:has()` detection, without which the field drew a second border and lost
  its focus halo.
- **Pane edges are safe-area aware (KF-CZ3CBS).** The pane owns its header/content/footer inline
  padding, so the project sidebar's footer inset moved to an inner `.project-sidebar__footer-content`
  box instead of padding the pane footer itself.
- **Catalog.** `CatalogExampleStack` now caps examples at a 736px measure; Hot Sheet's catalog lifts
  it for its application-width compositions, and the catalog sidebar scrolls its pane content.

The toolbar row now top-aligns while the search field is expanded, so the peer controls stay at
exactly the same position as the field wraps (the HS2-W843B4 requirement, previously met only
within 4px).

### Kerf beta.49 configuration-first adoption

Beta.49 adds first-class `Text`, `Grid`, and `Spacer` components, baseline-aware
`Row`, side-selectable text/control insets, and `ListHeader.inline`. Hot Sheet treats
those as component configuration rather than invitations to reproduce their geometry
in product CSS. The initial adoption removes the terminal rename stylesheet, replaces
Code Review's custom equal-column grid and heading wrapper with `Grid` and canonical
`ToolbarText`, projects nested command-heading density through `ListHeader`, and uses
the inline header presentation for terminal-operation summaries. The note empty state
also moves from the deprecated `horizontalOnly` alias to `sides="rl"`.

Responsive column-count changes, asymmetric tracks, ticket-state visuals, and other
domain semantics remain application-owned. Remaining legacy shadow-part, private-token,
and hand-written layout migrations are tracked separately so each can retain behavioral
and visual coverage rather than becoming an unreviewed package-upgrade rewrite
(HS2-737H3X; follow-ups HS2-06GDW3, HS2-S3BXC0, HS2-HD1SCC).

Hot Sheet does not reach through Kerf's private descendant classes or assign its
uncataloged tokens. Product-specific list metadata uses application-owned classes,
command colors use application-owned root declarations, and toolbar selection uses
`ToolbarControlGroup`'s public `selectedChrome` and `selectedTone` configuration.
Each bounded master/detail surface declares its navigation and detail scroll owners
directly. Exact analyzer exceptions are reserved for documented Web Awesome shadow
parts, which remain an intentional external-component boundary rather than a Kerf
extension point (HS2-06GDW3).

The beta.49 forced-geometry review was also checked case by case. Supported recurring
geometry now comes from component configuration: terminal tabs use `size`,
`presentation`, and `labelMaxWidth`; inspector tabs use `presentation` and
`allocation`; the workspace sort uses `selectedPresentation`; comparison controls use
the compact toolbar-control size; and the project strip uses adjacent trailing-action
placement. Spinner and empty-state sizing is attached to product-owned classes rather
than Kerf descendants. The remaining analyzer review findings are intentional
application layout: responsive shell/toolbar placement, bounded master-detail panes,
domain-specific source rows, and container-dependent truncation. Kerf beta.49 has no
prop that expresses those host-layout constraints, so they remain visible as review
findings instead of being hidden by profile exceptions.

The follow-on layout pass moves the shared vertical rhythm and action alignment in the
manual-model, saved-view, bulk-ticket, project, trash, keyboard, and provider-setup
surfaces to Kerf `List`, `Row`, `Text`, `Grid`, and `Spacer` primitives. Forms and dialog
hosts keep their native semantics, keyboard shortcut collections remain semantic
`ul`/`li` lists, and Web Awesome footer slots remain on their required host elements.
Application CSS still owns responsive column collapse, project path input/action tracks,
and shortcut-row geometry because those policies are asymmetric or container-dependent
rather than reusable Kerf component configuration (HS2-S3BXC0).

### Kerf UI doctor baseline

**Policy (HS2-AD9WRF, maintainer, 2026-09-30).** Styling a Kerf UI component with application
CSS is a bug. Kerf components are configured through their props, variants, and public
`--kui-*` tokens; application stylesheets never take a `.kui-*` class, a Kerf-rendered part,
or an app class placed on a Kerf root as their subject. When Kerf lacks the prop, token, or
component the app needs, the fix belongs in Kerf (a `KF-*` ticket, adopted when it ships),
not in an override. Application-owned components keep CSS for their own markup, including
native HTML and raw Web Awesome elements, and surfaces compose Kerf components before
implementing custom ones. The `KUI-L019` and `KUI-L022` budgets below are the tracked
residual of that policy and only ever decrease. A direct maintainer request for a visual
change does not override the policy (HS2-JS9PSP): when meeting it would restyle a Kerf
component, the agent asks for feedback or proactively files the `KF-*` request instead of
writing the override.

The residual is gated on Kerf releases rather than on app work: every Kerf API request
from HS2-G5K1V0 (`KF-XNRXCK` Toolbar zone layout, `KF-9K8PTV` Select trigger width and
selected typography, `KF-8SD2EP` ListItem/ListHeader geometry and per-part color,
`KF-E47GAW` AppTab/TabBar names and strip geometry, `KF-96T4HM` StateBanner layout,
`KF-FT9R9M` ResizableRegion/FloatingToolbar placement, `KF-GC3RKN` TokenSearchField, Text,
ValueTable, ToolbarText, and app-root tokens) is completed in the Kerf store and awaits the
next `@kerfjs/ui` release after 5.0.0-beta.59, when `HS2-PKPGGZ` replaces the corresponding
rules. The remaining `wa-*` subjects wait on `KF-PDPAVF` (tag chip), except the cursor policy
rules, which are deliberate (the drive-options submenu rules went with HS2-2EHD8R). `KUI-L022` is
at zero: HS2-3J2PX3 retired the visibility Toolbar's class by moving its configuration onto its app
wrapper through Kerf beta.62's `--kui-toolbar-inset` token (`KF-3EZ92R`), HS2-WF3W6A retired the
drawer grid tab's class with Kerf's `pinned` AppTab (`KF-DTFQSC`), and HS2-T67Z3N retired the project
tab's class with Kerf's `dropTarget` and `nameOverflow` props (`KF-373HYM`): the drag handler marks
`data-drop-target` on a tab it hovers, and the strip sets the attention, drop-target, and
disconnected colors through the public `--kui-app-tab-*` tokens.

The web package checks in a workspace-scoped `.kerf-ui-profile.json` and
`.kerf-ui-doctor.json`. The profile contains exact `KUI-L011` exceptions only for the
22 stylesheets that customize documented Web Awesome shadow parts; the analyzer still
reports those 63 suppressed findings with their rationale. The doctor runs catalog,
TypeScript, isolated Kerf ESLint, and analyzer stages in full mode with a content cache.
Browser evaluation remains disabled unless a developer explicitly supplies a trusted
URL, for example `npm run ui:doctor:raw -- --browser-url http://127.0.0.1:4173`.

`npm run ui:doctor` is the repeatable local and CI gate. Since HS2-6PD4FS it keeps no per-id budget
table: it runs `kerf-ui-doctor --full` and fails on any active error, review finding, or warning.
`kerf-ui-doctor` itself exits non-zero only for errors, so this thin wrapper stays until Kerf ships
a failure threshold (`KF-6S5EKX`), after which `ui:doctor` becomes the plain doctor command. Known,
tracked gaps are not budgeted; they are documented `suppressions` in `.kerf-ui-doctor.json`, each
with a rationale naming the Kerf ticket that removes it (today the 13 `KUI-L401` wiring findings on
the two entry modules, `KF-KWMJMS` and `KF-XKMC7W`). The doctor report keeps counting them as
`suppressed`. The Kerf ESLint rules the gate cleared (`kerfjs/require-delegate-disposer`,
`kerfjs/prefer-attr-selector`) are also enforced by `npm run lint` with `--max-warnings 0`. Earlier
releases carried an exact per-id error/review budget that only ever decreased; HS2-9ME409 and
HS2-TF76Z2 drove it to zero.

#### Component CSS ownership guard (HS2-EWYDH7)

The doctor's ownership rules (`KUI-L019`–`KUI-L022`) treat a component as foreign only when it
comes from another package. Its analyzer decides `isForeign` by comparing `entry.package`, so
nothing in the Hot Sheet catalog is ever foreign to a Hot Sheet stylesheet, and narrowing
`publicClasses` cannot change that. `KF-5X1TWD` asks Kerf for per-component ownership.

Until it ships, `npm run css:ownership` runs as the last step of `npm run lint`. It is
`clients/web/scripts/check-css-ownership.mjs`, unit-tested in `check-css-ownership.test.mjs`, and
enforces this rule: **a component stylesheet styles only the class blocks its own component
renders, plus the native HTML and raw Web Awesome elements that component authors itself.**

Ownership comes from the TSX sources, not from file names:

- A stylesheet's owners are the modules that import it, directly or through CSS `@import`.
- Three shell stylesheets also own every style-less module in their scope: `src/style.css` owns
  `main.tsx` and `src/app/`, `src/ux-demo/style.css` owns the demo modules, and
  `dev-review.css` owns `src/dev-review/`.
- A class block's owners are the modules whose literals render it, including raw HTML
  `class="…"` attributes. When several modules render the same block, ownership goes to the
  stylesheet named after the block, and otherwise to the stylesheet that uses the block as a
  selector root.

The check reports four kinds of finding:

| Kind              | Example                                                                                                                                                                                                                                                                                                                                                                                    |
| ----------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `kerf`            | `.kui-*` classes or `[data-component]` anywhere in a selector.                                                                                                                                                                                                                                                                                                                             |
| `foreign-class`   | Another component's block anywhere in the selector, including inside `:has()`. This covers blocks whose name differs from their file (`.ticket-list-row*`) and blocks of components without a stylesheet.                                                                                                                                                                                  |
| `hook-descendant` | An element below a class the component places on another component's root, for example `.terminal-drawer__rail .terminal-tab i` on Kerf `AppTab`. It counts even when that element is content this component projected into the child's slots.                                                                                                                                             |
| `foreign-element` | A classless element subject that also reaches markup another component renders. The JSX tree below the nearest own classed element decides. A descendant combinator over a subtree containing another component (`.ai-conversation__activity svg` over `AIContentLabel`) is a finding. So is a child-combinator element the component never authors, such as the root of `LoadingSpinner`. |

The check allows a few things:

- A component's own local components (functions declared in the same module) count as its own
  markup.
- A `LucideIcon` the component renders itself counts as its own `svg`. `HS2-4AQJEX` tracks moving
  those sizing rules to the icon's `size` prop.
- `*`, `html`, `body`, sibling chains, and `@keyframes` steps are ignored.

Known residue is listed in `clients/web/css-ownership-allowlist.json`. Each entry has `file`,
`selector` (with whitespace normalized), an exact `count` when the selector occurs more than once,
a `ticket` (`HS2-*` or `KF-*`) and a `reason`.

The allowlist can only shrink. An uncovered finding fails lint, and so does an extra occurrence of
an allowlisted selector. An entry that now matches fewer findings than it claims is stale and also
fails lint until it is lowered or deleted.

At introduction the check found 122 findings in 115 entries, grouped by area under `HS2-DYAR0S`,
`HS2-PK1C1X`, `HS2-M2W2DP`, `HS2-YNW0B3`, `HS2-7ZGYJY`, `HS2-148B5C`, `HS2-4V4CV2`, `HS2-QM0C3T`,
`HS2-TV78E1`, and `HS2-0X36TX`. Never add an entry for new code; fix the selector, give the
element an own class, or configure the child through its props.

UX demo stage styles in `src/ux-demo/style.css` style only demo-authored markup (HS2-TV78E1).
Demo captions, case frames, and copy carry their own demo classes (`__caption`, `__case`,
`__empty-state`, `__copy`, `__detail`, `workspace-settings-preview__title`/`__description`)
instead of element descendant selectors such as `.list-item-demo h2` that also matched headings
and text inside the demoed `ListHeader`, `TicketList`, `TicketCodeReview`, or `Select`. Content a
demo projects into a component slot (the `ListItem` trailing counts) is styled through its own
class too. The guard now reports no `foreign-element` finding in the demo stylesheet; only the
fixed-aspect terminal card rules tracked by `HS2-0X36TX` remain.

The first fix moved `.app-empty` out of `style.css` into `AppEmptyState`'s own
`app-empty-state.css`. That also made the UX demo render the production presentation, which it
previously lacked. The cross-project stats placeholder now composes the new `AppMessageState`
instead of borrowing the class.

HS2-QM0C3T removed nine of its twelve list, navigation, tab, and popover entries (11 of 14
findings) without changing a rendered pixel or computed style. Each fix gives app markup its own class or uses a child prop:

- **Own classes.** `CommandNavigation`'s running indicator is `.command-navigation__running`.
  `ViewNavigation` renders `.view-navigation__list` and `.view-navigation__entry`. Both popover
  `<nav>`s are `.repository-status-popover__nav`. The workspace header's utility buttons are
  `.workspace-header__utility-action`, which keeps its disabled dimming until Kerf's group owns
  that state (`KF-FTADQT`).
- **Child props.** `RepositorySummary` and the popover's synchronization values size their icons
  through `LucideIcon`'s `size`. The popover tints them through an app wrapper,
  `.repository-status-popover__sync-icon`.
- **ListItem tokens.** The Errors view row takes its danger tone from ListItem tokens on its app
  wrapper (`data-view-tone="danger"`) instead of a class on the ListItem.
- **Own state.** `CorruptTicketRow`'s selected wiggle keys off its own `data-selected` instead of
  TicketRow's modifier. `HS2-QSR1TG` tracks the remaining borrowed `ticket-list-row` markup
  classes.

Three entries remain, waiting on Kerf releases:

| Entry                                             | Waits for                                    | Adoption ticket |
| ------------------------------------------------- | -------------------------------------------- | --------------- |
| `.active-claim-spinner > svg`                     | `LoadingSpinner` `size` prop (`KF-PA81HY`)   | `HS2-JVPPVV`    |
| `.project-tab__busy-spinner > svg`                | `LoadingSpinner` `size` prop (`KF-PA81HY`)   | `HS2-JVPPVV`    |
| `.ticket-status-menu > wa-select::part(combobox)` | Select `presentation="inline"` (`KF-V2Y51V`) | `HS2-WQ8T6B`    |

A pixel `font-size` on the spinner wrappers would size the 1em spinner without reaching into it,
but it breaks the typography-scale rule in `theme.test.ts`. `KF-V2Y51V` is done in Kerf but not
yet in a published `@kerfjs/ui`.

HS2-PK1C1X and HS2-M2W2DP cleared the content components' 22 entries. Each fix gives the element
an own class or configures the child through its props:

- `NoteCard`: `note-card__time` and `note-card__footer`.
- `MarkdownEditor`: `markdown-editor__source` on the source textarea, so the editor no longer
  reaches into `MarkdownPreview`.
- `TicketCodeReview`: an own `compare-prompt` and an icon wrapper that tints the icon. The icon is
  sized by LucideIcon `size`. Commit-summary rules use child combinators, so a commit body's
  Markdown bold and code keep `MarkdownPreview`'s own inherited tone.
- `ConfidenceCalibration`: `recent-title`, `recent-list`, and `recent-item`.
- The project statistics page: `project-statistics__title`.
- `AttachmentGallery`:
  - Its own buttons carry `attachment-gallery__control` for their disabled tone.
  - The video carries `attachment-gallery__video`.
  - Media-control icons use LucideIcon `size` instead of a gallery-wide `svg` rule that also
    reached Kerf's toolbars.
- `AIConversation`:
  - Its header and activity icons use LucideIcon `size`.
  - The activity list and items have own classes, so a summary's Markdown lists keep their own
    presentation.
  - The foreground passes `PermissionRequestPopup layer="flow"` instead of repositioning the popup.

HS2-4V4CV2 cleared the dialogs, settings, and forms group (32 entries). Each element those
components author now carries an own class, so the styled subject cannot reach a child component.
Examples are the command editor's `__field`, `__control`, `__hint`, and `__legend`, the provider
form's `__field*`, `__auth-copy`, and `__account-usage`, the not-working and HS1 dialogs'
`__footer`/`__button`/`__cancel`, and the bulk tag dialog's `__choice`. The closing ticket's search
icon uses LucideIcon `size={16}`.

Children are configured instead of restyled:

- The Accounts card header uses the new `ProviderIcon` `size="l"` variant, a fixed 24px mark. The
  icon owns `flex: none`.
- The connection details values wrap their text in an own `__value` span. That span fills the
  stacked row at phone width, rather than overriding the ValueTable `dd` alignment.
- The composer's `__footer wa-button svg` rule was dead and is deleted.

Before/after captures at 1280 and 390 are pixel-identical, with one deliberate exception. The
command editor's `.command-settings-editor__grid label` and `input` rules had leaked into
`LucideIconPicker`, forcing its search field to a grid layout with semibold text. The picker now
lays out as its own stylesheet declares, at the editor's 14px regular field text.

HS2-K9KWJJ then dropped the terminal rail's `.kui-token-search` width override (88), since the
rail's controls Toolbar now sizes the expanded search itself. HS2-402AXQ replaced the project
tab strip's Add-project `wa-button` (and a stale `wa-dropdown` rule) with an app-styled native
`.project-tab-bar__action` button (86): the strip sits beside Kerf's `TabBar`, whose trailing zone
accepts only dormant decoration, so no `Toolbar` owns a control group there. The rules whose
subject is still a raw Web Awesome element are deliberate: `cursor-semantics.css` applies the
cursor policy through documented Web Awesome parts (`wa-dropdown-item` and friends) as
CLAUDE.md requires; `tag-chip.css` kept `wa-tag`'s compact geometry, 16px remove button, and
disabled state until Kerf shipped its removable `Chip` (`KF-PDPAVF`, adopted by HS2-HJEHRW); and `drive-options-menu.css`
styled the raw submenu items until `HS2-2EHD8R` replaced that menu with `PopupMenu` context mode
(83). HS2-AT4AAA upgraded `@kerfjs/ui` to 5.0.0-beta.60, which ships the seven requested
API groups below, and adopted the first two: the workspace header Toolbar takes
`responsive="trailing-priority"` (an expanded search moves to a full second row under the
identity once the toolbar is narrower than Kerf's `narrow` breakpoint, so the app's stacked
grid rules are gone) and `ProjectTab` passes Kerf's `attention` prop, tinting the name through
the public `--kui-app-tab-attention-color` token (78). `HS2-PKPGGZ` adopts the rest. The
FloatingToolbar parent gap (`HS2-10KEHN`) persists in beta.60, so the doctor adapter accepts
that version too. HS2-PKPGGZ then adopted the rest of beta.60 and took `KUI-L019` from 78 to 10:
region surfaces and the focus-mode popup layer are `--kui-resizable-region-background` /
`--kui-resizable-region-popup-z` tokens, the drawer restore control is a `placement="inline"`
FloatingToolbar in Kerf's own corner, sidebar and rail inspector tabs use AppTab's `icon-only`
presentation (fill allocation centers them; the icon gap is `--kui-layout-item-gap`), the project
and drawer tab strips configure their inset, chrome, and growth through the `--kui-tab-bar-strip-*`
and `--kui-tab-bar-trailing-flex` tokens, Selects size and style their selected content through
`triggerWidth` and the `--kui-select-selected-*` tokens, list rows and headers use `density`,
`divider`, `multilineIconAlign`, `actionDisabled`, Kerf's `drag-target` state (set by the drag
handler), and the `--kui-list-item-*` / `--kui-list-header-*` geometry and color tokens (app copy
inside a label lives in an app span such as `.ticket-close-dialog__candidate`), the HS1 banners use
`copyLayout="stacked"` / `actionPlacement="below"` and `--kui-state-banner-copy-gap`, the
connection details table uses the `--kui-value-table-row-*` tokens, the UX demo imports Kerf's
document baseline for the app-root height chain, and the Text margin/line-height, toolbar zone,
token-search editor, and overlay max-height rules were dead against beta.60's defaults. The ten
remaining rules are the three Web Awesome ones above plus four residual gaps requested upstream:
the terminal rail's inspector header keeps four rules that balance the Toolbar's leading and trailing
tracks so the ticket number centers on the rail (`KF-R1TFTE`),
the terminal drawer lets an open create PopupMenu escape the region through a CSS `:has([open])`
rule because flipping `contentOverflow` would rerender the open menu (`KF-J55CGB`), the terminal
rail heading needs a ToolbarText step at `--kui-font-l` between `large` and the page-title clamp
(`KF-7JE0F2`), and the reader's tab names need a container-responsive icon-only presentation
(`KF-6EPNA1`).
HS2-S6JQX1 upgraded `kerfjs`, `@kerfjs/ui`, and `eslint-plugin-kerfjs` to 5.0.0-beta.62, which
shipped the four residual requests, and adopted them: the rail's inspector header Toolbar takes
`centerAlign="balanced"` (`KF-R1TFTE`), the reader's TabBar takes `iconOnlyAt="wide"` so Kerf
hides segmented tab names below 832px (`KF-6EPNA1`), the rail heading is a `ToolbarText`
`size="xlarge-fixed"` at `--kui-font-l` (`KF-7JE0F2`), and ResizableRegion/Workbench release
their clipping while a Web Awesome dropdown is open (`KF-J55CGB`), so the terminal drawer's
`:has([open])` rules are gone. Only the three Web Awesome rules remain (3). The FloatingToolbar
parent gap (`HS2-10KEHN`) persists in beta.62. Beta.62 also ships the removable `Chip`
(`HS2-2GYQ6C`), ListItem tones and the caret-free Select trigger (`HS2-1QQGH4`), the
TokenSearchModel additions (`HS2-06Q4MG`), and the scoped Workbench selectors that `HS2-P289N2`
waits on (`KF-JTVA2F`).
HS2-MM9XKW upgraded all three packages to 5.0.0-beta.64 (beta.63 was skipped: its Workbench
overlay `:has()` rules restyled the whole work area on every DOM change and failed the 138-ticket
paint budget, fixed upstream as `KF-FN91ET`). The doctor baseline is unchanged (3 errors); the
FloatingToolbar parent gap (`HS2-10KEHN`) still persists, so beta.63 and beta.64 join the adapter's
version list. TokenSearchField suggestions are now an anchored popover with a rounded (not pill)
radius and leading-aligned rows (`KF-EZRBXH`, `KF-94A4J3`, `KF-3XSD7K`, `KF-KMR9HJ`). The release
also ships the APIs several tickets wait on: the guarded post-commit caret (`KF-DNVMQE`,
`HS2-TNE7V7`) and removal/clear focus restoration (`KF-Q2G9QS`, `HS2-45F8WW`), Workbench
`keepOpenOn` (`KF-5D6T81`, `HS2-5APX20`), the Workbench panel `header` (`KF-ZBW7MS`, `HS2-QQW6CT`),
the PopupMenu submenu race fix (`KF-A388BJ`, `HS2-ZKMCVW`), phone submenus placed outside their
parent menu (`KF-5PZ768`, `HS2-282GTZ`), ListItem tone inheritance (`KF-XD6YH1`, `HS2-C3SPM6`), and
TokenSearchField's own deferred blur collapse through a pointer click (`KF-64W0RN`).
HS2-10KEHN removed the doctor's FloatingToolbar adapter: beta.67's composition catalog lists
FloatingToolbar as a ToolbarControlGroup parent (`KF-QMRNQC`), so the documented composition passes
`KUI-L201` directly and the gate reads the raw doctor report.
HS2-9ME409 upgraded to 5.0.0-beta.67 and drove every Kerf UI doctor error to zero (the budgets
are now 0 for all error and review ids). Beta.67 catalogs FloatingToolbar as a ToolbarControlGroup
parent (`KF-QMRNQC`) and detects rules that reach unclassed descendants inside a component
(`KF-1M836P`). The fixes, all without overriding Kerf: `wa-dropdown-item` keeps Web Awesome's own
pointer cursor; the HS1 dialog styles its own path elements and the HS1 banners rely on Kerf's
stacked copy layout and action-button padding; the ticket-close dialog styles its own project and
title spans; inspector tab and ticket-source trailing icons size through `LucideIcon size`; the
inspector field headers keep Kerf's label line height; and the UX catalog stage is an app-owned
labelled section rather than a width-capped CatalogExampleStack. HS2-G838PZ and HS2-NZT3MT then
cleared every `kerfjs/require-delegate-disposer` and `kerfjs/prefer-attr-selector` warning, and the
doctor gate now budgets warnings as well: every warning id is held at 0 except the 56 `KUI-L401`
wiring findings, which wait on Kerf accepting wiring done once at the app entry (`KF-VXWMM9`,
HS2-Y2QG3G).
HS2-TF76Z2 upgraded to 5.0.0-beta.68 and declared `src/main.tsx` and `src/ux-demo/main.tsx` as
`wiring.entries` in `.kerf-ui-profile.json`, so application-scoped wiring obligations are checked
once per entry instead of in every rendering file, and moved the single page-lifetime
`wireScrollDividers(appRoot)` into `src/main.tsx` (56 to 13 `KUI-L401`). The UX demo now retains
its search-field and workspace-overflow disposers (`KUI-L402`). The residual waits on Kerf
crediting helper calls in modules reachable from an entry and resolving app-owned helpers by
source file (`KF-KWMJMS`), and on Kerf recognizing that `wireCatalog` already installs
`wireScrollDividers` (the UX demo must not add a second instance: two instances on one root loop
forever, HS2-E6AG7W, `KF-XKMC7W`).
HS2-TAZJ0V wires Kerf's `wireScrollDividers()` once, page-lifetime, at the production app root
(`clients/web/src/app/runtime.tsx`). Kerf toolbars draw no divider by default; with the wiring each
Pane header gains its bottom divider (`data-scroll-divider` containing `b`) only while content is
scrolled beneath it, and each footer its top divider (`t`) only while content continues past it.
HS2-8R25B6 upgraded to 5.0.0-beta.66: `WorkbenchPanel` is now a union of a static panel and a
`navStack` navigation panel (`KF-WW33YJ`), so the shell's rails are typed `WorkbenchStaticPanel`. It
also ships `KF-SCS4RH` (Toolbar zone focus rings stay unclipped) and `KF-6P4NAV` (the form-field
TokenSearchField matches Web Awesome's control radius and required marker). The FloatingToolbar
parent gap persists, so beta.66 joins the adapter list.
HS2-DQBAC4 upgraded to 5.0.0-beta.65, which adds `TokenSearchField presentation="form-field"`
(`KF-9QHWR1`, used by the saved-view dialog in HS2-E40KC0). The doctor baseline is unchanged and
the FloatingToolbar parent gap persists, so beta.65 joins the adapter list. `KF-WW33YJ` (Workbench
panel navigation stacks, HS2-FY06N4) and `KF-SCS4RH` (Toolbar zone focus rings) are committed
upstream but not in beta.65.
HS2-G5K1V0 reduced `KUI-L019` (application rules whose subject is a Kerf component or
Web Awesome element) from 102 to 89 with fixes that need no new Kerf API: redundant per-menu
`wa-dropdown-item` cursor rules were removed because `cursor-semantics.css` already covers
them; Web Awesome geometry moved onto app-owned wrappers or containers (the trash-settings
field, the export-collision tag, the project-dialog recovery action, the command AI menu,
a stretching grid for the terminal-visibility filter, and grid footers for the narrow
project-close and repository-setup actions); and two selectors dropped a Kerf class or
attribute they did not need. Every remaining rule needs Kerf API that does not exist yet
and is requested upstream, grouped by component in the kerf store: `KF-XNRXCK` (Toolbar
zone layout and a trailing-priority responsive policy), `KF-9K8PTV` (Select trigger width
and selected-content typography), `KF-8SD2EP` (ListItem/ListHeader geometry, per-part
color, multi-line labels), `KF-E47GAW` (AppTab/TabBar icon-only names, truncation, strip
geometry), `KF-96T4HM` (StateBanner copy layout and action placement), `KF-FT9R9M`
(ResizableRegion/FloatingToolbar background, popup overflow, restore placement, inset),
and `KF-GC3RKN` (TokenSearchField, Text, ValueTable, ToolbarText, app-root tokens). The
Web Awesome element rules that remain (tab-bar action buttons, drive-options submenu
items, the tag chip) resolve by composing through Kerf components instead
(`HS2-CSRJ9Y` for menus; `HS2-402AXQ` for the rest). Adoption of the upstream APIs is
tracked in `HS2-PKPGGZ`.

HS2-57MAAH reduced `KUI-L022` (an app class placed on a Kerf component root and styled
by app CSS) from 61 to 33 without new Kerf API. Icon and spinner sizing moved off the
`LucideIcon`/`LoadingSpinner` roots onto app-owned wrapper spans: the wrapper owns the
box (`width`/`height` via `remify()`, `display`, color, and vertical alignment) and its
`> svg` fills it, so no Kerf class or 1em icon root is restyled and the typography test
keeps `font-size` on the Web Awesome scale. Layout-only classes (dialog bodies, settings
lists, toolbar placement, value tables, the notes empty inset, the status menu trigger)
moved onto app-owned wrapper elements; Web Awesome shadow parts are now addressed through
the wrapper's child element (`.ticket-status-menu > wa-select::part(combobox)`). Every
remaining `KUI-L022` finding restyled a Kerf root's own contract.

HS2-VABS08 then reduced `KUI-L022` from 33 to 3. The project tab strip, the drawer's tab
strip, the rail heading/content/view, the attachment gallery toolbar and footer, the HS1
banners, the provider connections card, the command rows, and the code-review header are
app-owned wrapper elements (`display: grid` shells that the Kerf component fills) which
configure the component only through cataloged public tokens (`--kui-toolbar-gap`,
`--kui-layout-*`, `--kui-state-banner-*`, `--kui-list-item-*`). The sidebar Panes
(`.project-sidebar`, `.settings-navigation`, `.terminal-operations-sidebar`) keep their class
as an unstyled hook and stay the shell regions' only children, because Kerf routes device
safe areas only into a sole Pane child; their card chrome lives in the UX demo stage
(`.sidebar-demo-card`). Props replaced the rest:
`Select` `presentation="toolbar-borderless"` and `focusRingOwner="group"` (workspace sort,
rail project and view, mobile project switcher), `ListItem` `divider="after"` (repository
files), and `data-state`-keyed public tokens (visibility rows). The gallery footer is an
app-owned balanced grid whose cells each host a gap-less Kerf Toolbar around one control group,
the zoom FloatingToolbar hangs off a zero-size safe-area anchor, and the provider setup form
used an app-owned responsive grid until HS2-7XX356 adopted beta.62's responsive Kerf `Grid`
(`minColumnWidth`, `KF-18Z9DC`) with a form maximum width that keeps two columns at most. The filled
command rows painted their fill on the wrapper and neutralized the row border tones through the
scoped semantic tokens until HS2-Z5YQWT moved them onto beta.62's public ListItem resting,
hover, and selected tone tokens (`KF-0PKY5K`). The three residual `KUI-L022` findings were
retired by HS2-3J2PX3, HS2-WF3W6A, and HS2-T67Z3N (see above). The workspace sort trigger showed Kerf's icon-and-caret pill (the icon-only
Select contract) until HS2-4ZA33S adopted beta.62's `caret={false}` (`KF-3DX5BX`): the trigger is
again a round 44px action on a `shape="pill"` group, and the expanded search field's floor went
back to 19rem.

HS2-P289N2 rebuilt `AppShell` on Kerf's `Workbench` (merged after HS2-S6JQX1, keeping `KUI-L019` at
3): the
`app-shell.css` rules on `.kui-resizable-region` (panel backgrounds, the focus-mode drawer
lift, restore-corner placement, the mobile overlay height cap, and the inspector's rail
selectors) are gone because the Workbench owns that chrome; the remaining shell rules target
Hot Sheet's own elements or the public `--kui-workbench-popup-z` token. HS2-RWGQWN then removed
`mobile-side-panels.css` (the inspector's own safe-area insets) by letting the Workbench pad
the rail around the inspector's Pane-based card, and its `KUI-L005` profile exception with it;
the shared `SidebarPane` wrapper's forwarded `className` is a reviewed `KUI-L008` exception.

HS2-HJEHRW rebuilt `TagChip` on Kerf's `Chip` (`@kerfjs/ui/chip`), taking `KUI-L019` from 3 to
1: the only remaining application rule on a non-app subject is `cursor-semantics.css`'s
`wa-dropdown-item` cursor policy. The wrapper `span.tag-chip[data-component="tag-chip"]` keeps
the tag identity and disabled flag for tests and delegation, the Chip owns every pixel of
chrome (compact size, quiet/solid/outline appearance, rounded/pill shape, tone, remove
button), and the three former `wa-remove` listeners are click delegates on the Chip's
`remove-tag-chip` action. The catalog records the component's border and padding as `child`
geometry.

HS2-M6B8AD resolved every review finding. `KUI-L006` off-scale spacing was rewritten to Kerf
steps (dev-review overlay, demo caret spacing), a named app token
(`--hs-selected-row-overlap`, the 1px border overlap between adjacent selected rows), or
explicit `remify()` sub-scale geometry (server busy bars). The `KUI-L004` nested-inset and
`KUI-L008` dynamic-class findings were reviewed site by site and documented as exact
per-file `exceptions` in `.kerf-ui-profile.json`: every nested inset is a deliberate card,
chip, badge, field, drop zone, callout, dialog body, or popover surface inside a padded
parent, and every dynamic class expression only composes the component's own app-owned base
class with a BEM state/variant modifier or forwards a consumer-supplied app class (each
rationale names the classes or expressions). New findings of either kind in a file without
an exception still fail the gate.

HS2-EZ1N7Z cleared the last budgeted `KUI-L201` findings by making the workspace header's
control groups real Toolbar zone children (the three documented FloatingToolbar edges remain
adapted, not counted). The workspace-grid rail still places those groups in its own grid
(`HS2-K9KWJJ` tracks composing it through a Toolbar).

The profile's `catalogs` entry points the doctor at Hot Sheet's own catalog extension and
composition extension (`clients/web/ai/component-composition-extension.json`), so an
app-owned wrapper that renders a cataloged Kerf root (`rendersAs`) is checked as that root
inside Toolbar zones and parent checks instead of counting as an unknown element. The
wrapper's own `parents` stay `any`: the rendered root already carries the listed-parent
requirement, and listing it on the wrapper too makes the rule report the same placement
twice (HS2-N5G6JS).

HS2-FEDDPX removed tests of Kerf's private List/Grid variables and uncataloged tokens,
gave count labels application-owned classes, switched the List demo to a standard gap,
and stopped writing the uncataloged expanded-size variable during live region resize.
The public region size variable still previews drag size; the component's rendered
expanded size is refreshed on the settled render. The remaining beta.58 ownership
findings are tracked by HS2-GTX61Q and its child tickets.
HS2-M78D5A adapts the exact missing FloatingToolbar → ToolbarControlGroup parent edge
in the beta.58 doctor report. It checks the installed beta.58 version, direct JSX parent,
and public Kerf imports,
so all other `KUI-L201` findings still count. Kerf's public FloatingToolbar signature
documents these children and its implementation renders `role="toolbar"`; no extra
Toolbar is added to the app. Remove the adapter when upstream catalogs this edge
(HS2-10KEHN).
HS2-90B8WH composes ticket-view selects through ToolbarControlGroup in Toolbar leading zones
and uses ToolbarText's placeholder for the loading inspector's center zone. Its one
remaining `KUI-L202` finding was TabBar trailing composition.
HS2-GX51F7 uses TabBar's adjacent trailing and far-edge end zones for terminal drawer
creation and hide actions, and gives the AppTab catalog example a TabBar parent. It also
removes a consumer rule targeting TabBar's trailing internals; all `KUI-L202` and
`KUI-L203` findings are now cleared.

HS2-NBMT1Q renders every ToolbarControlGroup catalog variant through a Toolbar leading
zone. The eight group variants and their interactions remain available; the doctor no
longer reports their parent composition.

HS2-8D3QSE puts the saved-view token search and repository comparison-side controls
inside Toolbar zones. Their form and review actions remain controlled by the app, while
the group geometry belongs to ToolbarControlGroup.

HS2-KB5YY6 composes the gallery's markup and zoom controls in a footer Toolbar and the
terminal visibility button/select in a compact Toolbar. The gallery retains its centered
markup and right-aligned zoom actions; visibility controls retain their group selection
and fit the dashboard header at desktop and phone widths.

HS2-2TN51D gives the phone terminal key bar app-owned compact groups. Its Fn control can
stay sticky in the same horizontal scroll container as the function keys, while the
ordinary row fits the phone viewport and all keys retain their dark treatment.

HS2-FRB545 adds phone terminal copy and paste. `TerminalKeyBar`'s Fn row gains a Clipboard group
(Copy, Paste) after the modifiers; the drawer focus mode shows a matching Copy/Paste pill beside the
text-size control, and the magnified terminal toolbar adds the same two buttons. `TerminalCopyDialog`
(a raw `wa-dialog` with an app-owned, read-only, terminal-colored native text view) and
`TerminalPasteDialog` (the denied/unavailable fallback with an editable native field) are cataloged
as `terminal-copy-dialog` and `terminal-paste-dialog`, with both paste reasons exposed in the demo.
