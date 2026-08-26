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

## 2. Application shell and navigation

### 2.1 `AppShell` — feature floor

Owns the top-level arrangement and responsive behavior of:

- `ProjectSidebar`
- `WorkspaceHeader`
- `ProjectTabBar`
- `Workspace`
- `TicketInspector`
- `BottomDrawer`
- global overlays and notifications

Supporting components:

- `ResizableRegion` — sidebar, inspector, and bottom-drawer splitters; collapsed and
  restored sizes are local UI state.
- `ConnectionStateBanner` — connecting, reconnecting, offline, incompatible-server,
  and authentication states.
- `GlobalDropTarget` — routes supported ticket, attachment, and cross-store drops.
- `FocusCoordinator` — predictable keyboard traversal and restoration after overlays.
- `WindowChrome` — native traffic lights/titlebar accommodation in Tauri/macOS; absent
  or adapted in an ordinary browser.

### 2.2 `ProjectSidebar` — feature floor

The left region in the wireframe, scoped to the selected project/store connection.

- `ProjectSummary`
  - `ProgressSparkline` / compact status histogram
  - completed and in-progress counts
  - `CoverageOrProgressIndicator` for the secondary percentage shown in the wireframe
- `RepositorySummary`
  - `BranchChip`
  - unpushed and uncommitted counts
  - opens the repository-status popover tracked by HS2-RPVFA4
- `ViewNavigation`
  - section heading and add-view action
  - `ViewNavigationItem` with icon, title, count, selection, and attention state
  - built-ins: Needs Review, All Tickets, Backlog, Archive
  - user-defined views when custom-view support lands
- `CommandNavigation`
  - `CommandButton`
  - `CommandGroup` with collapsible heading
  - running, stopping, last-run, success, and failure states
- `DriveControl`
  - primary launch/resume action
  - active tool/connection state and stop confirmation

### 2.3 `WorkspaceHeader` — feature floor

- `ProjectHeading`
- `ViewModeSwitcher` — list, columns, and later dashboard/analytics modes
- `SettingsButton`
- `SortControl`
- `SavedOrCommandMenu`
- `SearchButton` and `SearchField` expansion

### 2.4 `ProjectTabBar` — feature floor

- `ProjectTab` — selected, remote/local, busy, disconnected, attention, and close
  states
- `AddProjectButton`
- `ProjectPicker`
- `TabOverflowMenu`

Tabs represent server/project connections rather than embedded stores. The component
must tolerate two tabs that expose the same store through different checkouts or
servers.

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
- `QuickTicketComposer`
  - compact “New ticket…” entry
  - expands to the minimum useful creation fields
  - respects the selected ticket provider and its capabilities
- `TicketList`
  - virtualized, keyboard navigable, multi-select capable
  - incremental paging and live insertion/reordering
  - `TicketListSection` where grouping is active
  - `TicketRowDivider`
- `TicketRow` — **demo built**: a shared, horizontally responsive ticket-summary
  boundary for list and narrow column use. The comfortable list presentation is a
  flat, separator-led row; the same component becomes a lightly elevated card at
  narrow column widths. Its primary line treats qualified slug and two-line-clamped
  title as one normal inline formatting flow, ordered slug → priority → title so
  bounded priority remains visible before an arbitrarily long title (the slug is a
  stable-width inline block), while updated time is
  pinned right and baseline-aligned with the first line. A quieter, vertically
  centered secondary flow holds the persistent independently operable outline/filled
  Up Next star, status, short owner name, and all tags; it wraps without hiding or
  collapsing metadata at narrow widths. It also includes blue selection, keyboard
  selection, and a representative right-click context menu.
  The left rail is reserved for special-state attention in HS1 precedence order:
  needs review (purple), blocked (dark gray), then Up Next (yellow). Up Next also uses
  the familiar yellow Lucide star with an accessible add/remove name.
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

- `TicketBoard`
- `TicketColumn` with heading, count, loading, and empty states
- `TicketCard` is the narrow column presentation of the same responsive ticket-summary
  component contract and semantic actions as `TicketRow`, not an independent component
- keyboard and pointer movement between columns
- explicit mutation preview/error handling when a provider lacks the target field

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
- `TicketContextMenu`
- `CopyMoveTicketDialog`
- `UndoToast` / `UndoHistory`

## 4. Ticket inspector, reading, and editing

### 4.1 `TicketInspector` — feature floor

The trailing inspector shown in the wireframe.

- `InspectorHeader`
  - ticket identifier
  - title
  - up-next/star toggle
  - close/collapse action
- `InspectorTabBar`
  - `TicketInfoTab`
  - `TicketTimelineTab`
  - `TicketAttachmentsTab`
- `TicketMetadataEditor`
  - `CategoryPicker`
  - `PriorityPicker`
  - `StatusPicker`
  - `StatusBadge` — **built**: readable status text with status-specific tone and
    optional reinforcing Lucide icon; unit and bidirectional `/ux-demo` coverage
  - assignee/reviewer/claim fields when supported
  - capability-aware validation and unsupported-field explanation
- `TicketDetailsSection`
- `TicketTagsSection`
- `TicketAttachmentsSection`
- `TicketNotesSection`

### 4.2 Details and reader surfaces — feature floor

- `MarkdownPreview`
- `MarkdownEditor`
- `InlineEditableField`
- `ReaderButton`
- `TicketReader` — large, scrollable details-and-notes surface
- `ReaderEditMode` — carries an in-progress inline edit into the larger surface
- `UnsavedChangesGuard`

### 4.3 Tags — feature floor

- `TagList`
- `TagChip` — **built**: Web Awesome tag primitive, stable domain identity, compact
  filled, non-pill default presentation plus optional variants, disabled/removable behavior, unit tests, and interactive
  `/ux-demo` coverage. Tag padding uses a compact 2:1 horizontal-to-vertical ratio.

The component catalog records composition relationships. Its sidebar shows clickable
“Uses” and “Used by” links for the selected demo so composed components can be reviewed
in either direction.

Production web component CSS is colocated in `clients/web/src/components/` and imported
by its component module. The `/ux-demo` stylesheet owns only catalog shell, inspector,
and stage presentation, ensuring the demo exercises the same CSS the real app imports.
Actionable context-menu entries consistently pair their text with meaningful Lucide
icons; structural separators do not require icons.
- `TagPicker`
- `AddTagButton`
- tag removal, creation, autocomplete, unsupported-provider, and validation states

### 4.4 Attachments — feature floor

- `AttachmentList`
- `AttachmentRow`
- `AttachmentPicker`
- `AttachmentDropZone`
- `AttachmentProgress`
- `AttachmentActions` — open, copy reference/path, reveal, download, remove
- `AttachmentPreview` where the media type and provider permit it

### 4.5 Notes and activity — feature floor

- `NoteList`
- `NoteCard` with kind-specific presentation
- `RegularNote`
- `StatusNote`
- `FeedbackNeededNote`
- `FeedbackDraftEditor`
- `ActivityTimeline`
- `ActivityTimelineEntry`
- `NoteComposer`
- `NoteEditor`
- `NoteReaderButton`

Activity notes are durable ticket history. The separate rich AI activity stream can
feed a live timeline and, under HS2-3GRNZW, later propose distilled activity notes;
the two sources must remain visually and semantically distinguishable.

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

- `PermissionRequestDialog`
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
- `NotificationCenter`
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
