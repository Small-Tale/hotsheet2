import type { CatalogEntry, CatalogSection } from '@kerfjs/ui/catalog';

import componentCatalogExtension from '../../ai/component-catalog-extension.json';

export interface DemoDefinition {
  id: string;
  name: string;
  description: string;
  uses?: string[];
}
export interface DemoCategory {
  id: string;
  name: string;
  children?: DemoCategory[];
  demos?: DemoDefinition[];
}

const demo = (id: string, name: string, description: string, uses?: string[]): DemoDefinition => ({
  id,
  name,
  description,
  uses,
});

const catalogRecords = new Map(componentCatalogExtension.entries.map((entry) => [entry.id, entry] as const));

/**
 * A demo for an app-owned component recorded in `kerf.components.json`. Its name and description
 * come from the generated consumer catalog (`purpose`), so this file only owns ordering,
 * category, and the `uses` graph.
 */
const cataloged = (id: string, uses?: string[]): DemoDefinition => {
  const record = catalogRecords.get(id);
  if (!record) throw new Error(`UX demo "${id}" has no entry in ai/component-catalog-extension.json`);
  return demo(id, record.name, record.purpose, uses);
};

export const demoCatalog: DemoCategory[] = [
  {
    id: 'shell',
    name: 'Application shell',
    demos: [
      cataloged('app-shell', [
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
      cataloged('app-loading-indicator', ['state-banner', 'loading-spinner']),
      cataloged('project-sidebar', [
        'button',
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
      cataloged('project-summary'),
      cataloged('repository-summary', ['list-item', 'row', 'text', 'badge']),
      cataloged('repository-status-popover', [
        'toolbar',
        'value-table',
        'list-item',
        'list-header',
        'ticket-code-review',
      ]),
      cataloged('change-evidence-dialog', ['toolbar', 'list-item', 'list-header']),
      cataloged('view-navigation', ['list', 'list-header', 'list-item', 'list-action-row', 'badge', 'loading-spinner']),
      cataloged('command-navigation', ['list', 'list-header', 'list-item', 'loading-spinner']),
      cataloged('command-settings-editor', ['command-navigation', 'toolbar', 'list-header']),
      cataloged('drive-control'),
      cataloged('drive-options-menu'),
      cataloged('workspace-header', [
        'toolbar-text',
        'toolbar-control-group',
        'ticket-search-field',
        'ticket-list',
        'ticket-board',
        'notification-center',
      ]),
      cataloged('search-scope-toolbar', ['floating-toolbar', 'toolbar-control-group']),
      cataloged('project-tab', ['tab-bar-item']),
      cataloged('project-tabs', ['project-tab', 'select']),
      demo('resizable-region', 'ResizableRegion', 'Accessible horizontal and vertical shell splitters.'),
      cataloged('connection-state-banner'),
      cataloged('connection-details-dialog', ['toolbar', 'value-table']),
      cataloged('settings-navigation', ['list', 'list-item', 'toolbar']),
      cataloged('notification-navigation', ['list', 'list-item', 'badge', 'toolbar']),
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
          cataloged('quick-ticket-composer', ['ticket-category-select', 'ticket-list']),
          cataloged('ticket-list', ['ticket-row', 'ticket-page-more']),
          cataloged('ticket-page-more'),
          cataloged('ticket-row', ['status-badge', 'confidence-badge', 'tag-chip', 'loading-spinner']),
        ],
      },
      {
        id: 'ticket-board',
        name: 'Columns',
        demos: [
          cataloged('ticket-board', ['ticket-board-column']),
          cataloged('ticket-board-column', ['ticket-row', 'ticket-page-more']),
        ],
      },
      {
        id: 'worker-columns',
        name: 'Workers',
        demos: [cataloged('worker-columns', ['column-track', 'ticket-row', 'state-banner'])],
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
          cataloged('ticket-inspector', [
            'toolbar',
            'toolbar-text',
            'toolbar-control-group',
            'tab-bar-item',
            'ticket-info-panel',
            'ticket-timeline',
            'ticket-code-review',
            'ticket-attachments',
            'note-card',
            'note-composer',
            'loading-spinner',
          ]),
          cataloged('ticket-inspector-skeleton'),
          cataloged('corrupt-ticket-inspector', ['list-header', 'state-banner']),
          cataloged('ticket-info-panel', [
            'ticket-category-select',
            'ticket-priority-select',
            'ticket-status-menu',
            'markdown-editor',
            'tag-chip',
            'list-header',
            'note-card',
            'note-composer',
          ]),
          cataloged('ticket-timeline'),
          cataloged('ticket-code-review'),
          cataloged('ticket-attachments', ['attachment-gallery']),
          cataloged('ticket-category-select', ['select']),
          cataloged('ticket-priority-select', ['select']),
          cataloged('ticket-status-menu', ['status-badge']),
          cataloged('status-badge'),
          cataloged('confidence-badge'),
          cataloged('confidence-calibration', ['confidence-badge']),
          cataloged('repository-size', ['state-banner', 'value-table', 'badge']),
          cataloged('tag-chip'),
          cataloged('choice-card'),
        ],
      },
      {
        id: 'content',
        name: 'Content',
        demos: [
          cataloged('ticket-reader', ['ticket-inspector']),
          cataloged('markdown-editor'),
          cataloged('markdown-preview'),
          cataloged('attachment-gallery'),
          cataloged('ticket-close-dialog', ['select', 'list-item']),
          cataloged('not-working-dialog', ['pending-attachment-picker']),
          cataloged('bulk-ticket-dialog', ['surface-scaffold', 'list', 'text', 'row']),
          cataloged('saved-view-dialog', ['ticket-search-field']),
          cataloged('ticket-link-choice-dialog', ['surface-scaffold', 'list', 'text', 'row', 'list-item']),
          cataloged('project-dialog', ['list-item']),
          cataloged('project-close-dialog', [
            'state-banner',
            'list',
            'text',
            'row',
            'list-header',
            'list-item',
            'ai-conversation',
            'terminal-dashboard',
          ]),
          cataloged('conversation-export-dialog', ['content-transition', 'choice-card']),
          cataloged('command-run-dialog'),
        ],
      },
      {
        id: 'notes',
        name: 'Notes and activity',
        demos: [cataloged('note-composer'), cataloged('note-card')],
      },
    ],
  },
  {
    id: 'ai',
    name: 'AI and attention',
    demos: [
      cataloged('ai-conversation', ['permission-request', 'drive-control', 'list-header', 'row', 'text', 'badge']),
      cataloged('ai-content-label'),
      cataloged('ai-feedback-dialog'),
      cataloged('ai-tool-settings', ['select']),
      cataloged('guidance-settings', ['select', 'checkbox', 'text-field']),
      cataloged('manual-model-dialog'),
      cataloged('ai-launch-dialog'),
      cataloged('command-group-dialog'),
      cataloged('attachment-filename-editor'),
      cataloged('notification-card', ['badge']),
      cataloged('halted-session-popup', ['notification-card', 'list', 'row', 'text']),
      cataloged('permission-request', ['notification-card']),
      cataloged('notification-center', ['permission-request']),
    ],
  },
  {
    id: 'terminal',
    name: 'Drawer and terminals',
    demos: [
      cataloged('terminal-drawer', ['tab-bar-item', 'list-item', 'list-header', 'ai-conversation', 'terminal-key-bar']),
      cataloged('terminal-dashboard', [
        'fixed-aspect-terminal-card',
        'floating-toolbar',
        'toolbar-control-group',
        'list-item',
        'list',
        'text',
        'row',
      ]),
      cataloged('terminal-operations-sidebar', ['project-summary', 'list', 'list-header', 'toolbar']),
      cataloged('terminal-ticket-rail', [
        'ticket-list',
        'ticket-inspector',
        'workspace-header',
        'quick-ticket-composer',
        'notification-center',
      ]),
      cataloged('fixed-aspect-terminal-card', ['terminal-key-bar']),
      cataloged('terminal-key-bar', ['toolbar-control-group']),
      cataloged('terminal-visibility-dialog', ['list-item']),
      cataloged('terminal-copy-dialog'),
      cataloged('terminal-paste-dialog'),
      cataloged('terminal-edit-menu'),
      cataloged('terminal-rename-dialog'),
    ],
  },
  {
    id: 'shared',
    name: 'Shared interactions',
    demos: [
      cataloged('app-empty-state'),
      cataloged('app-error', ['state-banner']),
      cataloged('content-transition'),
      demo('tab-bar-item', 'TabBarItem', 'Shared project and terminal tab geometry, state, and close action.'),
      demo('select', 'Select', 'Compact icon-bearing single-value selection control.'),
      demo('toolbar', 'Toolbar', 'Shared leading, center, and trailing toolbar geometry.', [
        'toolbar-text',
        'toolbar-control-group',
      ]),
      demo(
        'floating-toolbar',
        'FloatingToolbar',
        'Forced-dark controls floating over positioned content; child groups inherit the dark scheme without an inverse tone.',
        ['toolbar-control-group'],
      ),
      demo(
        'list',
        'List',
        'Vertical row layout with no gap, standard gap, and bounded scrolling with custom gap and edge dividers.',
      ),
      demo('list-item', 'ListItem', 'Aligned icon, label, trailing value, and selection geometry for list rows.'),
      demo('list-header', 'ListHeader', 'Aligned section heading and optional action for list groups.'),
      demo(
        'toolbar-control-group',
        'ToolbarControlGroup',
        'Consistent rounded grouping for native toolbar buttons and Web Awesome popup triggers.',
      ),
      demo('toolbar-text', 'ToolbarText', 'Vertically aligned large, default, and small toolbar identity text.'),
      cataloged('ticket-search-field', ['toolbar-control-group']),
      demo('value-table', 'ValueTable', 'Shared static key/value rows with inset separators.'),
      cataloged('pending-attachment-picker'),
    ],
  },
  {
    id: 'setup',
    name: 'Setup and settings',
    demos: [
      cataloged('hs1-migration-dialog', ['value-table']),
      cataloged('hs1-migration-banner', ['shell-banner']),
      cataloged('codex-hooks-notice-banner', ['list', 'state-banner']),
      cataloged('shell-banner', ['state-banner']),
      cataloged('project-setup-warning-banner', ['shell-banner']),
      cataloged('server-build-notice-banner', ['shell-banner']),
      cataloged('source-stale-notice-banner', ['shell-banner']),
      cataloged('repository-size-banner', ['shell-banner']),
      cataloged('notifications-paused-banner', ['shell-banner']),
      cataloged('provider-icon'),
      cataloged('ticket-source-setup-dialog', ['content-transition', 'list-item', 'provider-icon']),
      cataloged('provider-setup-form', ['provider-icon']),
      cataloged('ticket-sources-settings', ['list', 'list-item', 'provider-icon']),
      cataloged('accounts-settings'),
      cataloged('settings-workspace', ['ticket-sources-settings']),
      cataloged('keyboard-settings'),
      cataloged('trash-settings'),
      cataloged('terminal-memory-settings'),
      cataloged('notification-settings', ['notification-channel-dialog', 'notification-rule-dialog']),
      cataloged('notification-channel-dialog'),
      cataloged('notification-rule-dialog'),
      cataloged('permission-rules-settings'),
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

function catalogEntry(item: DemoDefinition): CatalogEntry {
  const uses = (item.uses ?? []).map(findDemo).filter((demo): demo is DemoDefinition => Boolean(demo));
  const usedBy = demosUsing(item.id);
  return {
    id: item.id,
    name: item.name,
    description: item.description,
    // Like Kerf UI's own catalog, the only entry tag marks compositions (HS2-AEHES7).
    tags: demoKind(item.id) === 'composition' ? ['Composition'] : [],
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
