export type DemoPhase = 'feature-floor' | 'desktop' | 'later';
export interface DemoDefinition { id: string; name: string; description: string; phase: DemoPhase; implemented?: boolean; uses?: string[] }
export interface DemoCategory { id: string; name: string; children?: DemoCategory[]; demos?: DemoDefinition[] }

const demo = (id: string, name: string, description: string, phase: DemoPhase = 'feature-floor', implemented = false, uses?: string[]): DemoDefinition =>
  ({ id, name, description, phase, implemented, uses });

export const demoCatalog: DemoCategory[] = [
  { id: 'shell', name: 'Application shell', demos: [
    demo('app-shell', 'AppShell', 'Top-level responsive application regions.'),
    demo('project-sidebar', 'ProjectSidebar', 'Project summary, repository state, views, commands, and drive controls.', 'feature-floor', false, ['project-summary', 'repository-summary', 'view-navigation', 'command-navigation', 'drive-control']),
    demo('project-summary', 'ProjectSummary', 'Compact progress trend and ticket totals.', 'feature-floor', true),
    demo('repository-summary', 'RepositorySummary', 'Branch and working-copy status action.', 'feature-floor', true),
    demo('view-navigation', 'ViewNavigation', 'Selectable built-in and custom ticket views.', 'feature-floor', true),
    demo('command-navigation', 'CommandNavigation', 'Grouped colored project command actions.', 'feature-floor', true),
    demo('drive-control', 'DriveControl', 'Primary AI drive start and stop action.', 'feature-floor', true),
    demo('workspace-header', 'WorkspaceHeader', 'Project heading, display modes, sorting, settings, and search.', 'feature-floor', true, ['toolbar-control-group', 'ticket-list', 'ticket-board']),
    demo('project-tabs', 'ProjectTabBar', 'Local and remote project connection tabs.'),
  ]},
  { id: 'tickets', name: 'Ticket workspace', children: [
    { id: 'ticket-list', name: 'List', demos: [
      demo('quick-ticket-composer', 'QuickTicketComposer', 'Compact ticket creation that expands in place.', 'feature-floor', true, ['ticket-category-select', 'ticket-list']),
      demo('ticket-list', 'TicketList', 'Responsive, keyboard-navigable ticket collection.', 'feature-floor', true, ['ticket-row']),
      demo('ticket-row', 'TicketRow', 'Dense ticket summary and selection target.', 'feature-floor', true, ['status-badge', 'tag-chip']),
    ]},
    { id: 'ticket-board', name: 'Columns', demos: [
      demo('ticket-board', 'TicketBoard', 'Status/category column workspace.', 'feature-floor', true, ['ticket-row']),
    ]},
    { id: 'search', name: 'Search and filtering', demos: [
      demo('global-search', 'GlobalSearchOverlay', 'FTS search, scope, and suggestions.'),
      demo('filter-chip', 'FilterChip', 'Active removable query constraint.'),
    ]},
  ]},
  { id: 'inspector', name: 'Ticket inspector', children: [
    { id: 'metadata', name: 'Metadata', demos: [
      demo('ticket-inspector', 'TicketInspector', 'Trailing ticket detail and editing surface.', 'feature-floor', true, ['ticket-info-panel', 'ticket-timeline', 'ticket-attachments']),
      demo('ticket-info-panel', 'TicketInfoPanel', 'Ticket metadata, details, and tags inspector section.', 'feature-floor', true, ['ticket-category-select', 'ticket-priority-select', 'ticket-status-menu', 'tag-chip']),
      demo('ticket-timeline', 'TicketTimeline', 'Chronological ticket activity inspector section.', 'feature-floor', true),
      demo('ticket-attachments', 'TicketAttachments', 'Ticket attachment inspector section.', 'feature-floor', true),
      demo('ticket-category-select', 'TicketCategorySelect', 'Category chooser with configured icons and colors.', 'feature-floor', true),
      demo('ticket-priority-select', 'TicketPrioritySelect', 'Priority chooser with semantic icons.', 'feature-floor', true),
      demo('ticket-status-menu', 'TicketStatusMenu', 'Badge-triggered status chooser with semantic icons.', 'feature-floor', true, ['status-badge']),
      demo('metadata-editor', 'TicketMetadataEditor', 'Capability-aware category, priority, and status fields.'),
      demo('status-badge', 'StatusBadge', 'Readable ticket state with reinforcing iconography.', 'feature-floor', true),
      demo('tag-chip', 'TagChip', 'Compact tag label with optional removal behavior.', 'feature-floor', true),
      demo('tag-picker', 'TagPicker', 'Find, create, and attach ticket tags.'),
    ]},
    { id: 'content', name: 'Content', demos: [
      demo('ticket-reader', 'TicketReader', 'Large details-and-notes reading surface.'),
      demo('markdown-editor', 'MarkdownEditor', 'Inline and expanded Markdown editing.'),
      demo('attachment-list', 'AttachmentList', 'Durable attachment identity and actions.'),
    ]},
    { id: 'notes', name: 'Notes and activity', demos: [
      demo('note-card', 'NoteCard', 'Kind-specific note presentation.'),
      demo('feedback-draft', 'FeedbackDraftEditor', 'Local draft response to feedback.'),
      demo('activity-timeline', 'ActivityTimeline', 'Chronological durable activity notes.'),
    ]},
  ]},
  { id: 'ai', name: 'AI and attention', demos: [
    demo('drive-launcher', 'DriveLauncher', 'Launch, observe, and stop an AI drive session.'),
    demo('busy-indicator', 'BusyIndicator', 'Tool and connection activity state.'),
    demo('permission-request', 'PermissionRequestDialog', 'Human approval with timeout and competing-client states.'),
    demo('command-button', 'CommandButton', 'Configured safe command action.', 'later'),
    demo('notification-center', 'NotificationCenter', 'Unified attention and notification history.', 'later'),
  ]},
  { id: 'terminal', name: 'Drawer and terminals', demos: [
    demo('bottom-drawer', 'BottomDrawer', 'Resizable tabbed desktop utility region.', 'desktop'),
    demo('terminal-pane', 'TerminalPane', 'Live shared terminal and actions.', 'desktop'),
    demo('terminal-size-notice', 'TerminalSizeMismatchNotice', 'PTY size ownership and resize affordance.', 'desktop'),
  ]},
  { id: 'shared', name: 'Shared interactions', demos: [
    demo('toolbar-control-group', 'ToolbarControlGroup', 'Consistent rounded grouping for toolbar controls.', 'feature-floor', true),
    demo('confirmation-dialog', 'ConfirmationDialog', 'Reusable consequential confirmation.'),
    demo('empty-state', 'EmptyState', 'Actionable absence of content.'),
    demo('loading-skeleton', 'LoadingSkeleton', 'Stable loading presentation.'),
    demo('provider-capability', 'ProviderCapabilityNotice', 'Explains unavailable provider operations.'),
  ]},
  { id: 'setup', name: 'Setup and settings', demos: [
    demo('welcome-screen', 'WelcomeScreen', 'First useful entry into a server connection.'),
    demo('add-project-flow', 'AddProjectFlow', 'Discover or connect a local or remote project.'),
    demo('settings-window', 'SettingsWindow', 'Effective-first scoped settings.', 'later'),
    demo('provider-connections', 'TicketProviderConnections', 'Configure authoritative ticket sources.', 'later'),
  ]},
  { id: 'later', name: 'Later major surfaces', demos: [
    demo('terminal-dashboard', 'TerminalDashboard', 'Saved grids and viewport controls.', 'later'),
    demo('analytics-dashboard', 'AnalyticsDashboard', 'Ticket flow, usage, and cost visualizations.', 'later'),
    demo('custom-view-builder', 'CustomViewBuilder', 'Saved query and view construction.', 'later'),
    demo('announcer-overlay', 'AnnouncerOverlay', 'Live and digest narration experience.', 'later'),
  ]},
];

export function flattenCatalog(categories: DemoCategory[] = demoCatalog): DemoDefinition[] {
  return categories.flatMap(category => [...(category.demos ?? []), ...flattenCatalog(category.children ?? [])]);
}

export function findDemo(id: string): DemoDefinition | undefined {
  return flattenCatalog().find(item => item.id === id);
}

export function demosUsing(id: string): DemoDefinition[] {
  return flattenCatalog().filter(item => item.uses?.includes(id));
}
