# UX component catalog

> **Status: initial inventory; implementation started.** The development-only Hono
> `/ux-demo` catalog shell and production `TagChip`, `StatusBadge`, and initial
> `TicketRow` components are built in `clients/web` (HS2-61XG15/HS2-H0T0MV/HS2-RP0FKP). This
> catalog is derived from the
> [list-mode wireframe](design/exports/Main%20Interface%20Wireframe%20-%20List%20Mode.png),
> the structural SVG export beside it, and the client requirements in
> [06-clients.md](06-clients.md). The wireframe establishes information architecture,
> not final color, type, spacing, or density.

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

| Feature component | Web implementation | macOS SwiftUI implementation |
|---|---|---|
| Application shell and regions | Kerf composition, CSS grid/split panes | `NavigationSplitView`, split views, window scenes |
| State and API resources | Kerf signals, array signals, resources | observable models and async API services |
| Standard controls | Cherry-picked Web Awesome Core elements | Native SwiftUI controls |
| Lists and selection | Kerf virtualized list + semantic rows | `List`/lazy containers + native selection |
| Menus, dialogs, drawers | Web Awesome primitives where suitable | Native menus, sheets, popovers, inspectors |
| Terminal viewport | Imperative terminal widget behind a Kerf `ref`/scope | Native terminal surface wrapper |

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
header, ticket workspace, and inspector into a desktop shell. Sidebar and inspector
splitters are keyboard/pointer adjustable on wide displays; both secondary regions
yield to the ticket workspace on compact displays. Sidebar regions never resize below
250px. Both sidebars animate between visible and collapsed states; their restore
controls live at the matching leading/trailing edges of the center-column toolbar,
never in ProjectTabBar. Start-edge inspector resizing correctly mirrors end-edge sidebar resizing. Composed
WorkspaceHeader search retains its expand/autofocus/filter/empty-blur
contract. Project settings retain the sidebar but hide the inspector and disable
ticket-view actions. Global Terminal Dashboard and Cross-project Stats modes hide
both project-scoped regions, replace the project identity, and temporarily suppress
header controls pending their dedicated wireframes. It owns the top-level arrangement
The project-scoped list/column workspace also composes the real `QuickTicketComposer`
immediately above its ticket collection, matching the wireframe; settings and global
dashboard modes omit it.
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
  the restored size. Pointer drags update the region's CSS geometry at most once per
  animation frame and commit reactive state only on release, so large ticket
  collections are not rebuilt for every pointer event. Sidebar, inspector, and
  bottom-drawer sizes are local UI state.
- `ConnectionStateBanner` — **demo built**: connecting, reconnecting, offline,
  incompatible-server, and authentication variants with state-specific Lucide icons,
  live-region semantics, details, and relevant recovery actions.
- `GlobalDropTarget` — routes supported ticket, attachment, and cross-store drops.
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

- `ProjectSummary` — **demo built**: typed seven-day ticket-completion trend,
  completed-today count, and current in-progress count. The ambiguous day-over-day
  percentage from the wireframe is intentionally omitted. Zero-completion days retain
  a one-pixel neutral baseline mark so all seven day positions remain visible.
  - `ProgressSparkline` / compact status histogram
  - completed and in-progress counts
- `RepositorySummary` — **demo built**: one accessible branch/status action with
  unpushed and uncommitted counts. The uncommitted count remains text-only to avoid
  crowding the already icon-rich row.
  - `BranchChip`
  - unpushed and uncommitted counts
  - opens the repository-status popover tracked by HS2-RPVFA4
- `MenuItem` — **demo built**: the shared icon, label, trailing-value, and full-row
  selection grid used by repository, view, and command actions. This keeps icons,
  labels, and interaction boundaries aligned across menu-like sidebar surfaces.
- `MenuHeader` — **demo built**: shared section-label alignment with MenuItem icons,
  including optional trailing action and whole-header disclosure variants.
- `ViewNavigation` — **demo built**: icon-bearing views, counts, attention, add-view
  action, and controlled selection through `MenuItem`.
  - section heading and add-view action
  - `ViewNavigationItem` with icon, title, count, selection, and attention state
  - built-ins: Needs Review, Queue (active tickets), Backlog, Archive
  - user-defined views when custom-view support lands
- `CommandNavigation` — **demo built**: collapsible group of palette-colored,
  icon-bearing command actions with controlled running state. Colors are constrained
  to the exact shared HS1 custom-command palette, including contrast-aware neutral.
  - `CommandButton`
  - `CommandGroup` with collapsible heading
  - running, stopping, last-run, success, and failure states
- `DriveControl` — **demo built**: primary start/stop action with explicit tool and
  running semantics
  - primary launch/resume action
  - active tool/connection state and stop confirmation

### 2.3 `WorkspaceHeader` — feature floor

- `WorkspaceHeader` — **demo built**: responsive project identity, compact
  all-Lucide Tahoe-style toolbar groups, animated inline expanding live search, a functional
  sort menu whose selected option remains highlighted and toggles ascending/descending
  direction when reselected, and a connected list/column/settings workspace. Settings disables sort,
  favorite, overflow, and search actions; global shell modes omit project controls.
- `Toolbar` — **demo built**: shared 56px-high leading/optional-center/trailing layout
  with consistent horizontal padding across the project sidebar, center column, and
  ticket inspector. Trailing content is edge-aligned; when center is omitted, leading
  content owns the flexible space and trailing controls remain pinned right. Its
  bottom divider is an explicit option rather than consumer CSS.
- `ToolbarText` — **demo built**: vertically aligned large, default, and small toolbar
  identity text; project names use large and inspector ticket numbers use small.
- `PageHeader` — **demo built**: current view identity below ProjectTabBar, separate
  from the project-level toolbar above it.
- `ToolbarControlGroup` — **demo built**: shared equal-height rounded-border container for toolbar
  buttons, segmented choices, and popup triggers; child controls do not draw their
  own borders or divider lines. A single control highlights the whole group on
  hover; controls in multi-item groups receive individual 32px highlights inside
  the 40px shell. Slotted Lucide icons share explicit sizing, block layout, and
  vertical centering across native and Web Awesome buttons. Its borderless appearance
  keeps identical 40px geometry and hover highlights while omitting the idle border
  and background; sidebar visibility and inspector ticket-action groups use it.
- `ProjectHeading`
- `ViewModeSwitcher` — **built for list, columns, and project settings** with
  accessible pressed state. Settings replaces ticket content while active rather
  than opening a transient popover; later dashboard/analytics modes join the control.
- `SortControl` — popup menu with a persistent current-sort highlight and ascending or
  descending Lucide arrow; reselecting the current field reverses its direction. This
  intentionally remains an action menu rather than `Select`: a value-only select does
  not emit the same-value activation needed to reverse the current ordering.
- `SavedOrCommandMenu`
- `SearchButton` and `SearchField` expansion — the magnifier button is replaced by
  an animated, wider field carrying the same icon and automatic focus. An empty
  field collapses on blur; a non-empty query remains expanded. Focus is drawn by
  the outer control group so the ring is never clipped by the animated field.

### 2.4 `ProjectTabBar` — feature floor, demo built

- `ProjectTab` — **demo built**: macOS Tahoe-inspired pill presentation owned by the
  component itself, with selected, remote/local, busy, disconnected, attention,
  closable, and fixed states plus roving focus and arrow/Home/End keyboard navigation
  when composed in the bar. Fine-pointer devices reveal close affordances on hover or
  keyboard focus; touch-oriented devices retain the visible close control. Local tabs
  omit the redundant folder/branch icon, while remote tabs retain their cloud marker.
  The close affordance is a compact, highlight-free leading control with balanced
  trailing space
  so the tab identity remains visually centered; transient trailing indicators such as
  busy, offline, and attention occupy that reserved balance space instead of widening
  the tab or displacing its label. Busy uses a full-ring CSS spinner: its statically
  centered wrapper never transforms, and the ring alone rotates around its center.
  This avoids both transform-composition drift and the perceptual wobble of rotating
  an incomplete Lucide arc; browser coverage samples its center across animation frames.
- `ProjectTabContextMenu` — **built** with Lucide icons for Close Tab, Close Other
  Tabs, Close Tabs to the Right, and Close All Tabs.
- `AddProjectButton` — **demo built** with controlled insertion and selection.
- `ProjectPicker` — remains part of the later add-project flow.
- `TabOverflowMenu` — removed from the current design; the project strip itself is
  horizontally scrollable and does not duplicate projects in a secondary menu.
- `TerminalDashboardButton` and `CrossProjectStatsButton` — **shell navigation built**
  with controlled selected state. Their full dashboard surfaces remain tracked by
  HS2-2ZCN7K and HS2-38RJMK respectively.

Global dashboard modes precede project tabs; Add follows the project strip. Tabs
represent server/project connections rather than embedded stores. The component
must tolerate two tabs that expose the same store through different checkouts or
servers. The tab strip scrolls horizontally without truncating identities; the overflow
strip provides direct access to tabs outside the current viewport. The Add action remains
vertically centered with the pills.

In AppShell the hierarchy is Toolbar(WorkspaceHeader) → ProjectTabBar → connection banner →
PageHeader → workspace. TicketInspector is a root trailing region spanning the shell's
full height and uses the same animated slide/collapse contract as the project sidebar;
its panel-right control replaces a generic close glyph. Inspector tab
icons never shrink, and compact inspectors switch to icon-only labels.

## 3. Ticket workspace

### 3.1 `Workspace` — feature floor

Routes the selected project and view to one major content surface while retaining
selection where sensible.

- `ListWorkspace`
- `ColumnWorkspace`
- `SearchResultsWorkspace`
- later: `TerminalDashboard`, `AnalyticsDashboard`, and custom views
- `WorkspaceState` — loading skeleton, empty state, error/retry, offline snapshot,
  unsupported view, and no-project onboarding

### 3.2 `ListWorkspace` — feature floor and current wireframe focus

- `ViewHeading`
- `QuickTicketComposer` — **demo built**: compact launcher expands to title/category,
  provider destination, required-title validation, create, and cancel states; creation
  inserts a selected mock ticket into the shared collection. Category choices use
  the shared colored/iconic picker in both its selected and menu presentations;
  created mock tickets derive their category icon/color from that same choice model.
  Textual Cancel intentionally has no redundant icon.
  - compact “New ticket…” entry
  - expands to the minimum useful creation fields
  - respects the selected ticket provider and its capabilities
- `TicketList` — **demo built**: composes the production `TicketRow` at
  comfortable list width with platform-style replacement, Command/Ctrl toggle, Shift
  range, arrow-key range extension, and Select All semantics and no parallel row
  markup; it fills the width supplied by its host (which owns the standard workspace
  margins), and the list shell and its first/last rows share rounded outer corners
  - later data integration: virtualization for exceptionally large result sets
  - incremental paging and live insertion/reordering
  - `TicketListSection` where grouping is active
  - `TicketRowDivider`
- `TicketRow` — **demo built**: a shared, horizontally responsive ticket-summary
  boundary for list and narrow column use. The comfortable list presentation is a
  square-cornered, flat, separator-led row that reads as one continuous list; the same component becomes a lightly elevated, rounded card at
  narrow column widths. Its primary line treats qualified slug and title as one normal
  inline formatting flow, with an explicit two-line limit in lists and three-line
  limit in board columns. The flow is ordered slug → priority → title so
  bounded priority remains visible before an arbitrarily long title (the slug is a
  stable-width inline block). Updated time is the first item in that identity flow and
  floats right, allowing later lines of a long title to use the space beneath it. A quieter, vertically
  centered secondary flow holds the persistent independently operable outline/filled
  Up Next star, status, short owner name, and all tags; it wraps without hiding or
  collapsing metadata at narrow widths. It also includes blue selection, keyboard
  selection, and one shared right-click context menu. The real TicketList and TicketBoard
  compositions wire that same menu to reader, direct icon-rich category/priority/status
  submenus with bulk assignment, lifecycle-eligible Up Next, duplication, archive, and
  delete handlers; the demo only substitutes deterministic effects. A single completed
  selection prepends `Verified` and `Not Working…`; completed and verified selections
  never expose Up Next.
  The left rail is reserved for special-state attention in HS1 precedence order:
  needs review (purple), blocked (dark gray), then Up Next (yellow). Up Next also uses
  the familiar yellow Lucide star with an accessible add/remove name. Blocked tickets
  additionally show a compact `Blocked` pill immediately after their status.
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
  flat, elevation-free `TicketRow` at narrow width. The deterministic demo carries enough live tickets to
  overflow all columns; each ticket region scrolls independently while its heading and
  count remain fixed. A hosting workspace may add its own surrounding surface when
  appropriate.
- `TicketBoardColumn` — **demo built**: owns one heading, count derived from its ticket
  collection, fixed header, independently scrolling ticket region, visible scroll
  affordance, a standalone demo that preserves the 250px production minimum, and shared
  responsive `TicketRow` composition. Loading and empty states
  remain future variants.
- The real Queue board uses `Not Started`, `Started`, `Completed`, and `Verified`
  columns. A per-project setting can hide `Verified`, merging those tickets into
  `Completed`. Backlog and Archive views each use one eponymous column because the
  selected view already supplies their grouping.
- There is no separate `TicketCard`: narrow board columns activate `TicketRow`'s
  container-query card presentation while preserving identical markup and actions
- keyboard and pointer movement between columns
- explicit mutation preview/error handling when a provider lacks the target field

Comprehensive platform-aware keyboard shortcuts are planned in HS2-KTHGVE. They must
follow macOS conventions on Apple platforms and standard web/OS conventions elsewhere,
remain discoverable, avoid editable-field conflicts, and receive unit plus browser
coverage.

### 3.4 Search and filtering — feature floor

- `GlobalSearchOverlay`
- `SearchQueryInput`
- `SearchScopePicker`
- `SearchSuggestionList`
- `SearchResultRow`
- `ActiveFilterBar` and removable `FilterChip`
- `SavedViewAction`

Later custom-query work adds `QueryBuilder`, `FilterRule`, `FilterGroup`, and
`ViewEditor` without replacing the basic search components.

### 3.5 Selection and batch actions — later

- `SelectionBar`
- `BatchActionMenu`
- `TicketContextMenu` — **built**: shared list/board menu with Lucide icons, checked
  metadata submenus, stable field/value action contracts, and selection-aware bulk updates.
- `CopyMoveTicketDialog`
- `UndoToast` / `UndoHistory`

## 4. Ticket inspector, reading, and editing

### 4.1 `TicketInspector` — feature floor

The trailing inspector shown in the wireframe. `TicketInspector` is **demo built**
as a focused shell around separately demoed `TicketInfoPanel`, `TicketTimeline`, and
`TicketAttachments` components, plus the Up Next toggle, close/reopen, and controlled
tab routing. Its AppShell composition projects the same shared active-tab state rather
than substituting a hardcoded default. When one row has been selected but its full ticket
is still loading, AppShell keeps the visible inspector region mounted and shows the
placeholder in place; it never removes and re-adds the sidebar during that transition.

- `InspectorHeader`
  - ticket identifier
  - capability-aware inline title editing with debounced persistence
  - up-next/star toggle
  - close/collapse action
- `InspectorTabBar`
- `TicketInfoPanel` — **demo built**: metadata; safe Markdown details on a white
  basic-note-like surface; blocked reason before Details with its header outside the
  gray reason box; tags; notes with collection-derived counts; and provider/update
  provenance. An unblocked ticket exposes a small secondary `Block ticket` action;
  its controlled editor creates the reason and the adjacent status `Blocked` pill.
  - `TicketTimeline` — **demo built**: chronological activity shown as time-ago,
    required title, and optional subtitle along a continuous dot/line track; its
    displayed event total is derived from the rendered entry collection
  - `TicketAttachments` — **demo built**: attachment rows whose displayed total is
    derived from the rendered collection, plus native browse and drop entry points.
    The inspector and TicketRow are also attachment drop targets.
- `TicketMetadataEditor`
  - `Select` — **demo built**: compact, icon-bearing Web Awesome select foundation
    shared by ticket category and priority controls, including selected-value and
    popup-option icon/color projection with the same measured `0.5rem` icon/label
    gap used by custom-command `MenuItem`s;
    the light-DOM icon margin explicitly overrides Web Awesome's slotted default.
    Consumers may supply a custom selected-value renderer while retaining the shared
    option list, keyboard behavior, spacing, and typography
  - `TicketCategorySelect` — **demo built**: configured category icons and colors in
    both selected-value and popup-option presentations
  - `TicketPrioritySelect` — **demo built**: semantic priority icons in both
    selected-value and popup-option presentations
  - `TicketStatusMenu` — **demo built**: a shared `Select` whose custom selected-value
    renderer retains the compact `StatusBadge` presentation with lighter typography;
    every normally weighted popup option carries its semantic Lucide icon, and the
    selected control intentionally hides the redundant dropdown caret. Inspector and
    row-context status menus share one canonical order: Not started, Started,
    Completed, Verified, then a separator before Backlog and Archive. Backlog uses the
    clock metaphor; Archive uses the archive-box metaphor.
  - `StatusPicker`
  - `StatusBadge` — **built**: readable status text with status-specific tone,
    optional reinforcing Lucide icon, filled/plain appearances, and regular/compact
    sizing; every public variant is exposed with unit and bidirectional `/ux-demo` coverage
  - assignee/reviewer/claim fields when supported
  - capability-aware validation and unsupported-field explanation
- `TicketDetailsSection` — section header remains outside its visually distinct
  bordered Markdown surface, matching Notes hierarchy; double-click non-empty details,
  single-click the empty prompt, or use its keyboard action to begin editing
- `TicketTagsSection` — **built**: controlled chips with capability-aware removal,
  duplicate-safe creation, and native autocomplete suggestions shared by inspector
  and reader
- `TicketAttachmentsSection`
- `TicketNotesSection`

### 4.2 Details and reader surfaces — feature floor

- `MarkdownPreview` — **built**: HS1-parity `marked` rendering with GFM tables and
  task lists, line breaks, links, images, fenced/inline code, blockquotes, lists, and
  headings. Raw HTML is escaped and unsafe link/image protocols are rejected.
- `MarkdownEditor` — **demo built**: rendered preview by default, double-click/keyboard
  to edit non-empty content, single-click to add empty content, persistent controlled
  draft, vertically resizable embedded details, inline/expanded presentation, and 150 ms debounced autosave without routine
  Save/Cancel actions. Internal editor controls preserve editing; external blur flushes.
  The embedded appearance reuses the same behavior in inspector and reader without a
  redundant standalone toolbar or save-status footer; the real inspector persists edits
  through its checkout.
- `NotWorkingDialog` — **demo built**: an explicit completed-ticket failure report
  accepting a note and/or pending evidence. It retains input after failures, prevents
  accidental light dismissal, and reopens the ticket as Not Started + Up Next only
  after evidence upload succeeds.
- `PendingAttachmentPicker` — **demo built**: reusable browse/drop evidence staging
  with long-name ellipsis and per-file removal before submission.
- `InlineEditableField`
- `ReaderButton`
- `TicketReader` — **built in the demo and real web shell**: a large dialog
  presentation of the actual
  `TicketInspector`, preserving its metadata editing, tabs, attachments, timeline,
  Markdown details, notes, and controlled state rather than maintaining a reduced
  parallel reader implementation. The inspector exposes a Reader action, reader content
  uses its full available width, and the Edit action gates read-only details/notes.
- `ReaderEditMode` — **built**: carries an in-progress inline details edit and draft into
  the larger surface without losing focus or content
- `UnsavedChangesGuard`

### 4.3 Tags — feature floor

- `TagList`
- `TagChip` — **built**: Web Awesome tag primitive, stable domain identity, compact
  filled, non-pill default presentation plus optional variants, disabled/removable behavior, unit tests, and interactive
  `/ux-demo` coverage. Tag padding uses a compact 2:1 horizontal-to-vertical ratio.

The component catalog records composition relationships. A left-aligned “Related
components” menu in the main demo footer uses the shared `Select`. It lists `Used by`
first, then a separated `Uses` section when both exist; headings carry the relationship
meaning and each option uses the same evocative component icon as the catalog sidebar.

Production web component CSS is colocated in `clients/web/src/components/` and imported
by its component module. The `/ux-demo` stylesheet owns only catalog shell, inspector,
and stage presentation, ensuring the demo exercises the same CSS the real app imports.
Actionable context-menu entries consistently pair their text with meaningful Lucide
icons; structural separators do not require icons.
- `TicketTagEditor` — **built**: shared tag list, compact add control, autocomplete,
  normalization, duplicate prevention, removal, and unsupported-provider state

### 4.4 Attachments — feature floor

- `TicketAttachments` — the existing attachment-list surface, with browse/drop input;
  it will gain durable row identity and remaining action capabilities rather than
  being duplicated by a separate `AttachmentList` component
- `AttachmentRow`
- `AttachmentPicker`
- `AttachmentDropZone`
- `AttachmentProgress`
- `AttachmentActions` — open, copy reference/path, reveal, download, remove
- `AttachmentPreview` where the media type and provider permit it

### 4.5 Notes and activity — feature floor

- `NoteList`
- `NoteCard` — **demo built** with distinct regular, status, feedback-needed,
  feedback-draft, and activity presentations sharing stable author, timestamp, vertically resizable edit body,
  and note identity;
  double-click enters a controlled editor whose Save persists and Cancel restores.
  In reader mode, regular/status notes remain read-only until the top-level Edit action,
  while feedback-needed and feedback-draft notes always render their Respond/Submit
  editor style. Hover/focus reveals an explicit Edit action outside reader mode, while
  the inspector toolbar provides the single Reader entry point from every inspector tab.
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

Activity notes are durable ticket history. The separate rich AI activity stream can
feed a live timeline and, under HS2-3GRNZW, later propose distilled activity notes;
the two sources must remain visually and semantically distinguishable.

### 4.6 Deliberate HS1 detail-panel parity

The HS1 detail panel was reviewed before defining the HS2 inspector. HS2 currently
retains category, priority, status, Up Next, title and identity, rendered/click-to-edit
Markdown details, blocked reason, tags, attachments, notes, and provider/update
provenance. Timeline and attachments move to dedicated tabs so the narrow Info view
stays readable; the reader dialog exposes those same tabs at a comfortable width.

Remaining editing capabilities are explicit follow-up work: attachment actions.
Note composition/edit/delete is shipped with provider-capability gating and shared
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
  operation details, optional visible-time automation countdown, and capability-aware
  Ignore/Deny/Always Allow/Allow Once actions
- `PermissionRequestPopup` — non-modal fixed presentation of the shared request card
- `PermissionSummary`
- `PermissionDetailDisclosure`
- allow/deny/session-scope actions
- timeout, already-answered, disconnected, and competing-client states

### 5.3 Custom commands — later, contracts already available

- `CommandButton`
- `CommandGroup`
- `CommandRunIndicator`
- `CommandOutputViewer`
- `CommandHistory`
- `CommandCancellationDialog`
- `CommandSettingsEditor`

### 5.4 Notifications — presentation begins at feature floor

- `ToastRegion` and `Toast`
- `NotificationBell`
- `NotificationCenter` — pending requests followed by newest-first resolution history;
  externally resolved requests remain visible with a neutral outcome message
- `AttentionBadge`
- `NativeNotificationRouter`
- `NotificationPreferences`

Off-server/mobile push remains a later transport; clients consume the same normalized
notification model.

## 6. Bottom drawer and terminals

### 6.1 `BottomDrawer` — desktop feature

- `DrawerTabBar`
- `DrawerTab`
- `AddDrawerTabButton`
- `DrawerVisibilityButton`
- `DrawerResizeHandle`
- tab kinds: terminal first; activity, command output, or other tools may follow

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
- `TerminalGrid` and `MagnifiedTerminal` for the later dashboard

The browser mock should simulate output, focus ownership, resize arbitration, session
completion, and disconnection without spawning a PTY.

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
- `EmptyState`
- `LoadingSkeleton`
- `RelativeTime`
- `CountBadge`
- `IconButton`
- `MarkdownSurface`
- `ProviderCapabilityNotice`

On the web, these should use Web Awesome Core where it supplies the needed accessible
primitive. Hot Sheet components wrap those primitives with domain behavior and stable
semantic actions; Kerf owns state and composition.

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
- `ToolPluginSettings`
- `NotificationSettings`
- `TerminalSettings`
- `AppearanceSettings`

## 9. Later major surfaces

These remain in the component architecture but are not initial-client blockers:

- `TerminalDashboard` — drawer grids, saved layouts, viewport controls
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
provides the categorized master/detail catalog, URL-addressable selection, planned
component states, and an optional non-modal, manually closed settings inspector that
keeps the demo visible during live adjustment. At wide desktop widths the inspector
occupies a dedicated grid column and shrinks the detail surface; at narrower widths it
overlays the content to preserve usable demo space. One viewport-anchored toggle stays
in the same location and changes between “Settings” and “Close settings”; opening the
inspector must not introduce a second control or a moving pointer target. It should
also keep stateful Web Awesome control properties synchronized when a demo reset
restores its canonical mock state. It should grow to provide:

Catalog groups reset native list margins so their shared `MenuHeader` and `MenuItem`
rows begin on the same outer edge; hierarchy is already clear from the headers and
does not receive an additional list indent.
Implemented entries use component-specific Lucide icons instead of one generic glyph.
Their trailing value is a dependency-aware last-modified time: changes to a demo,
recursively imported component/style dependencies, or global catalog code make the
demo current again. Planned entries remain visually muted and omit that value. A
development-only sidebar toggle enables or disables Dev Review without editing the URL.

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
