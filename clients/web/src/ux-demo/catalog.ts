import type { CatalogEntry, CatalogSection } from '@kerfjs/ui/catalog';

import componentCatalogExtension from '../../ai/component-catalog-extension.json';

export type DemoPhase = 'feature-floor' | 'desktop' | 'later';
export interface DemoDefinition {
  id: string;
  name: string;
  description: string;
  phase: DemoPhase;
  implemented?: boolean;
  uses?: string[];
}
export interface DemoCategory {
  id: string;
  name: string;
  children?: DemoCategory[];
  demos?: DemoDefinition[];
}

const demo = (
  id: string,
  name: string,
  description: string,
  phase: DemoPhase = 'feature-floor',
  implemented = false,
  uses?: string[],
): DemoDefinition => ({ id, name, description, phase, implemented, uses });

const catalogRecords = new Map(componentCatalogExtension.entries.map((entry) => [entry.id, entry] as const));

/**
 * A demo for an app-owned component recorded in `kerf.components.json`. Its name and description
 * come from the generated consumer catalog (`purpose`), so this file only owns ordering,
 * category, phase, and the `uses` graph.
 */
const cataloged = (
  id: string,
  phase: DemoPhase = 'feature-floor',
  implemented = false,
  uses?: string[],
): DemoDefinition => {
  const record = catalogRecords.get(id);
  if (!record) throw new Error(`UX demo "${id}" has no entry in ai/component-catalog-extension.json`);
  return demo(id, record.name, record.purpose, phase, implemented, uses);
};

export const demoCatalog: DemoCategory[] = [
  {
    id: 'shell',
    name: 'Application shell',
    demos: [
      cataloged('app-shell', 'feature-floor', true, [
        'project-sidebar',
        'workspace-header',
        'project-tabs',
        'connection-state-banner',
        'resizable-region',
        'toolbar',
        'toolbar-control-group',
        'floating-toolbar',
        'ticket-list',
        'ticket-inspector',
      ]),
      cataloged('app-loading-indicator', 'feature-floor', true, ['state-banner', 'loading-spinner']),
      cataloged('project-sidebar', 'feature-floor', true, [
        'toolbar',
        'project-summary',
        'repository-summary',
        'view-navigation',
        'command-navigation',
        'drive-control',
        'drive-options-menu',
        'list',
        'list-item',
        'list-header',
      ]),
      cataloged('project-summary', 'feature-floor', true),
      cataloged('repository-summary', 'feature-floor', true, ['list-item', 'row', 'text', 'badge']),
      cataloged('repository-status-popover', 'feature-floor', true, [
        'toolbar',
        'value-table',
        'list-item',
        'list-header',
        'ticket-code-review',
      ]),
      cataloged('change-evidence-dialog', 'feature-floor', true, ['toolbar', 'list-item', 'list-header']),
      cataloged('view-navigation', 'feature-floor', true, [
        'list',
        'list-header',
        'list-item',
        'list-action-row',
        'badge',
        'loading-spinner',
      ]),
      cataloged('command-navigation', 'feature-floor', true, ['list', 'list-header', 'list-item', 'loading-spinner']),
      cataloged('command-settings-editor', 'feature-floor', true, ['command-navigation', 'toolbar', 'list-header']),
      cataloged('drive-control', 'feature-floor', true),
      cataloged('drive-options-menu', 'feature-floor', true),
      cataloged('workspace-header', 'feature-floor', true, [
        'toolbar-text',
        'toolbar-control-group',
        'ticket-search-field',
        'ticket-list',
        'ticket-board',
        'notification-center',
      ]),
      cataloged('project-tab', 'feature-floor', true, ['app-tab']),
      cataloged('project-tabs', 'feature-floor', true, ['project-tab', 'select']),
      demo(
        'resizable-region',
        'ResizableRegion',
        'Accessible horizontal and vertical shell splitters.',
        'feature-floor',
        true,
      ),
      cataloged('connection-state-banner', 'feature-floor', true),
      cataloged('connection-details-dialog', 'feature-floor', true, ['toolbar', 'value-table']),
      cataloged('settings-navigation', 'feature-floor', true, ['list', 'list-item', 'toolbar']),
      cataloged('notification-navigation', 'feature-floor', true, ['list', 'list-item', 'badge', 'toolbar']),
    ],
  },
  {
    id: 'tickets',
    name: 'Ticket workspace',
    children: [
      {
        id: 'ticket-list',
        name: 'List',
        demos: [
          cataloged('quick-ticket-composer', 'feature-floor', true, ['ticket-category-select', 'ticket-list']),
          cataloged('ticket-list', 'feature-floor', true, ['ticket-row', 'ticket-page-more']),
          cataloged('ticket-page-more', 'feature-floor', true),
          cataloged('ticket-row', 'feature-floor', true, [
            'status-badge',
            'confidence-badge',
            'tag-chip',
            'loading-spinner',
          ]),
        ],
      },
      {
        id: 'ticket-board',
        name: 'Columns',
        demos: [
          cataloged('ticket-board', 'feature-floor', true, ['ticket-board-column']),
          cataloged('ticket-board-column', 'feature-floor', true, ['ticket-row', 'ticket-page-more']),
        ],
      },
    ],
  },
  {
    id: 'inspector',
    name: 'Ticket inspector',
    children: [
      {
        id: 'metadata',
        name: 'Metadata',
        demos: [
          cataloged('ticket-inspector', 'feature-floor', true, [
            'toolbar',
            'toolbar-text',
            'toolbar-control-group',
            'app-tab',
            'ticket-info-panel',
            'ticket-timeline',
            'ticket-code-review',
            'ticket-attachments',
            'note-card',
            'note-composer',
            'loading-spinner',
          ]),
          cataloged('ticket-inspector-skeleton', 'feature-floor', true),
          cataloged('ticket-info-panel', 'feature-floor', true, [
            'ticket-category-select',
            'ticket-priority-select',
            'ticket-status-menu',
            'markdown-editor',
            'tag-chip',
            'list-header',
            'note-card',
            'note-composer',
          ]),
          cataloged('ticket-timeline', 'feature-floor', true),
          cataloged('ticket-code-review', 'feature-floor', true),
          cataloged('ticket-attachments', 'feature-floor', true, ['attachment-gallery']),
          cataloged('ticket-category-select', 'feature-floor', true, ['select']),
          cataloged('ticket-priority-select', 'feature-floor', true, ['select']),
          cataloged('ticket-status-menu', 'feature-floor', true, ['status-badge']),
          demo('metadata-editor', 'TicketMetadataEditor', 'Capability-aware category, priority, and status fields.'),
          cataloged('status-badge', 'feature-floor', true),
          cataloged('confidence-badge', 'feature-floor', true),
          cataloged('confidence-calibration', 'feature-floor', true, ['confidence-badge']),
          cataloged('tag-chip', 'feature-floor', true),
          cataloged('choice-card', 'feature-floor', true),
          demo('tag-picker', 'TagPicker', 'Find, create, and attach ticket tags.'),
        ],
      },
      {
        id: 'content',
        name: 'Content',
        demos: [
          cataloged('ticket-reader', 'feature-floor', true, ['ticket-inspector']),
          cataloged('markdown-editor', 'feature-floor', true),
          cataloged('markdown-preview', 'feature-floor', true),
          cataloged('attachment-gallery', 'feature-floor', true),
          cataloged('ticket-close-dialog', 'feature-floor', true, ['select', 'list-item']),
          cataloged('not-working-dialog', 'feature-floor', true, ['pending-attachment-picker']),
          cataloged('bulk-ticket-dialog', 'feature-floor', true, ['surface-scaffold', 'list', 'text', 'row']),
          cataloged('saved-view-dialog', 'feature-floor', true, ['ticket-search-field']),
          cataloged('ticket-link-choice-dialog', 'feature-floor', true, [
            'surface-scaffold',
            'list',
            'text',
            'row',
            'list-item',
          ]),
          cataloged('project-dialog', 'feature-floor', true, ['list-item']),
          cataloged('project-close-dialog', 'feature-floor', true, [
            'state-banner',
            'list',
            'text',
            'row',
            'list-header',
            'list-item',
            'ai-conversation',
            'terminal-dashboard',
          ]),
          cataloged('conversation-export-dialog', 'feature-floor', true, ['content-transition', 'choice-card']),
          cataloged('command-run-dialog', 'feature-floor', true),
        ],
      },
      {
        id: 'notes',
        name: 'Notes and activity',
        demos: [
          cataloged('note-composer', 'feature-floor', true),
          cataloged('note-card', 'feature-floor', true),
          demo('feedback-draft', 'FeedbackDraftEditor', 'Local draft response to feedback.'),
          demo('activity-timeline', 'ActivityTimeline', 'Chronological durable activity notes.'),
        ],
      },
    ],
  },
  {
    id: 'ai',
    name: 'AI and attention',
    demos: [
      cataloged('ai-conversation', 'feature-floor', true, ['permission-request', 'drive-control']),
      cataloged('ai-content-label', 'feature-floor', true),
      cataloged('ai-feedback-dialog', 'feature-floor', true),
      cataloged('ai-tool-settings', 'feature-floor', true, ['select']),
      cataloged('manual-model-dialog', 'feature-floor', true),
      demo('busy-indicator', 'BusyIndicator', 'Tool and connection activity state.'),
      cataloged('halted-session-popup', 'feature-floor', true, ['list', 'row', 'text']),
      cataloged('permission-request', 'feature-floor', true),
      demo('command-button', 'CommandButton', 'Configured safe command action.', 'later'),
      cataloged('notification-center', 'feature-floor', true, ['permission-request']),
    ],
  },
  {
    id: 'terminal',
    name: 'Drawer and terminals',
    demos: [
      cataloged('terminal-drawer', 'feature-floor', true, [
        'app-tab',
        'list-item',
        'list-header',
        'ai-conversation',
        'terminal-key-bar',
      ]),
      cataloged('terminal-dashboard', 'feature-floor', true, [
        'fixed-aspect-terminal-card',
        'floating-toolbar',
        'toolbar-control-group',
        'list-item',
      ]),
      cataloged('terminal-operations-sidebar', 'feature-floor', true, [
        'project-summary',
        'list',
        'list-header',
        'toolbar',
      ]),
      cataloged('terminal-ticket-rail', 'feature-floor', true, [
        'ticket-list',
        'ticket-inspector',
        'workspace-header',
        'quick-ticket-composer',
        'notification-center',
      ]),
      cataloged('fixed-aspect-terminal-card', 'feature-floor', true, ['terminal-key-bar']),
      cataloged('terminal-key-bar', 'feature-floor', true, ['toolbar-control-group']),
      cataloged('terminal-visibility-dialog', 'feature-floor', true, ['list-item']),
      cataloged('terminal-copy-dialog', 'feature-floor', true),
      cataloged('terminal-paste-dialog', 'feature-floor', true),
      cataloged('terminal-edit-menu', 'feature-floor', true),
      cataloged('terminal-rename-dialog', 'feature-floor', true),
      demo('bottom-drawer', 'BottomDrawer', 'Resizable tabbed desktop utility region.', 'desktop'),
      demo('terminal-pane', 'TerminalPane', 'Live shared terminal and actions.', 'desktop'),
      demo(
        'terminal-size-notice',
        'TerminalSizeMismatchNotice',
        'PTY size ownership and resize affordance.',
        'desktop',
      ),
    ],
  },
  {
    id: 'shared',
    name: 'Shared interactions',
    demos: [
      cataloged('app-empty-state', 'feature-floor', true),
      cataloged('app-error', 'feature-floor', true, ['state-banner']),
      cataloged('content-transition', 'feature-floor', true),
      demo(
        'app-tab',
        'AppTab',
        'Shared project and terminal tab geometry, state, and close action.',
        'feature-floor',
        true,
      ),
      demo('select', 'Select', 'Compact icon-bearing single-value selection control.', 'feature-floor', true),
      demo('toolbar', 'Toolbar', 'Shared leading, center, and trailing toolbar geometry.', 'feature-floor', true, [
        'toolbar-text',
        'toolbar-control-group',
      ]),
      demo(
        'floating-toolbar',
        'FloatingToolbar',
        'Forced-dark controls floating over positioned content; child groups inherit the dark scheme without an inverse tone.',
        'feature-floor',
        true,
        ['toolbar-control-group'],
      ),
      demo(
        'list',
        'List',
        'Vertical row layout with no gap, standard gap, and bounded scrolling with custom gap and edge dividers.',
        'feature-floor',
        true,
      ),
      demo(
        'list-item',
        'ListItem',
        'Aligned icon, label, trailing value, and selection geometry for list rows.',
        'feature-floor',
        true,
      ),
      demo(
        'list-header',
        'ListHeader',
        'Aligned section heading and optional action for list groups.',
        'feature-floor',
        true,
      ),
      demo(
        'toolbar-control-group',
        'ToolbarControlGroup',
        'Consistent rounded grouping for native toolbar buttons and Web Awesome popup triggers.',
        'feature-floor',
        true,
      ),
      demo(
        'toolbar-text',
        'ToolbarText',
        'Vertically aligned large, default, and small toolbar identity text.',
        'feature-floor',
        true,
      ),
      cataloged('ticket-search-field', 'feature-floor', true, ['toolbar-control-group']),
      demo('value-table', 'ValueTable', 'Shared static key/value rows with inset separators.', 'feature-floor', true),
      cataloged('pending-attachment-picker', 'feature-floor', true),
      demo('confirmation-dialog', 'ConfirmationDialog', 'Reusable consequential confirmation.'),
      demo('empty-state', 'EmptyState', 'Actionable absence of content.'),
      demo('loading-skeleton', 'LoadingSkeleton', 'Stable loading presentation.'),
      demo('provider-capability', 'ProviderCapabilityNotice', 'Explains unavailable provider operations.'),
    ],
  },
  {
    id: 'setup',
    name: 'Setup and settings',
    demos: [
      cataloged('hs1-migration-dialog', 'feature-floor', true, ['value-table']),
      cataloged('hs1-migration-banner', 'feature-floor', true, ['shell-banner']),
      cataloged('codex-hooks-notice-banner', 'feature-floor', true, ['list', 'state-banner']),
      cataloged('shell-banner', 'feature-floor', true, ['state-banner']),
      cataloged('project-setup-warning-banner', 'feature-floor', true, ['shell-banner']),
      cataloged('notifications-paused-banner', 'feature-floor', true, ['shell-banner']),
      cataloged('provider-icon', 'feature-floor', true),
      cataloged('ticket-source-setup-dialog', 'feature-floor', true, [
        'content-transition',
        'list-item',
        'provider-icon',
      ]),
      cataloged('provider-setup-form', 'feature-floor', true, ['provider-icon']),
      cataloged('ticket-sources-settings', 'feature-floor', true, ['list', 'list-item', 'provider-icon']),
      cataloged('accounts-settings', 'feature-floor', true),
      cataloged('settings-workspace', 'feature-floor', true, ['ticket-sources-settings']),
      cataloged('keyboard-settings', 'feature-floor', true),
      cataloged('trash-settings', 'feature-floor', true),
      demo('welcome-screen', 'WelcomeScreen', 'First useful entry into a server connection.'),
      demo('add-project-flow', 'AddProjectFlow', 'Discover or connect a local or remote project.'),
      demo('settings-window', 'SettingsWindow', 'Effective-first scoped settings.', 'later'),
      demo('provider-connections', 'TicketProviderConnections', 'Configure authoritative ticket sources.', 'later'),
    ],
  },
  {
    id: 'later',
    name: 'Later major surfaces',
    demos: [
      demo('analytics-dashboard', 'AnalyticsDashboard', 'Ticket flow, usage, and cost visualizations.', 'later'),
      demo('custom-view-builder', 'CustomViewBuilder', 'Saved query and view construction.', 'later'),
      demo('announcer-overlay', 'AnnouncerOverlay', 'Live and digest narration experience.', 'later'),
    ],
  },
];

export function flattenCatalog(categories: DemoCategory[] = demoCatalog): DemoDefinition[] {
  return categories.flatMap((category) => [...(category.demos ?? []), ...flattenCatalog(category.children ?? [])]);
}

export function findDemo(id: string): DemoDefinition | undefined {
  return flattenCatalog().find((item) => item.id === id);
}

export function demosUsing(id: string): DemoDefinition[] {
  return flattenCatalog().filter((item) => item.uses?.includes(id));
}

const demoKinds = new Map(
  componentCatalogExtension.entries.map((entry) => [entry.id, entry.kind as 'component' | 'composition'] as const),
);

export function demoKind(id: string): 'component' | 'composition' {
  return demoKinds.get(id) ?? 'component';
}

const phaseLabel: Record<DemoPhase, string> = { 'feature-floor': 'Feature floor', desktop: 'Desktop', later: 'Later' };

function catalogEntry(item: DemoDefinition): CatalogEntry {
  const uses = (item.uses ?? []).map(findDemo).filter((demo): demo is DemoDefinition => Boolean(demo));
  const usedBy = demosUsing(item.id);
  return {
    id: item.id,
    name: item.name,
    description: item.description,
    tags: [phaseLabel[item.phase], ...(item.implemented ? [] : ['Planned'])],
    related: [
      ...usedBy.map((demo) => ({ id: demo.id, name: demo.name, group: 'Used by' })),
      ...uses.map((demo) => ({ id: demo.id, name: demo.name, group: 'Uses' })),
    ],
  };
}

/** Flatten nested Hot Sheet categories into Kerf's flat section contract while retaining the path. */
export function kerfCatalogSections(categories: readonly DemoCategory[] = demoCatalog): CatalogSection[] {
  const sections: CatalogSection[] = [];
  const visit = (category: DemoCategory, parents: readonly string[]) => {
    const path = [...parents, category.name];
    if (category.demos?.length)
      sections.push({
        category: path.join(' · '),
        entries: category.demos.map(catalogEntry),
      });
    for (const child of category.children ?? []) visit(child, path);
  };
  for (const category of categories) visit(category, []);
  return sections;
}
