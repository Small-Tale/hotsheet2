import type { CatalogEntry, CatalogSection } from '@kerfjs/ui/catalog';

import componentCatalogExtension from '../../ai/component-catalog-extension.json';

export type DemoPhase = 'feature-floor';
export interface DemoDefinition {
  id: string;
  name: string;
  description: string;
  phase: DemoPhase;
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
  uses?: string[],
): DemoDefinition => ({ id, name, description, phase, uses });

const catalogRecords = new Map(componentCatalogExtension.entries.map((entry) => [entry.id, entry] as const));

/**
 * A demo for an app-owned component recorded in `kerf.components.json`. Its name and description
 * come from the generated consumer catalog (`purpose`), so this file only owns ordering,
 * category, phase, and the `uses` graph.
 */
const cataloged = (id: string, phase: DemoPhase = 'feature-floor', uses?: string[]): DemoDefinition => {
  const record = catalogRecords.get(id);
  if (!record) throw new Error(`UX demo "${id}" has no entry in ai/component-catalog-extension.json`);
  return demo(id, record.name, record.purpose, phase, uses);
};

export const demoCatalog: DemoCategory[] = [
  {
    id: 'shell',
    name: 'Application shell',
    demos: [
      cataloged('app-shell', 'feature-floor', [
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
      cataloged('app-loading-indicator', 'feature-floor', ['state-banner', 'loading-spinner']),
      cataloged('project-sidebar', 'feature-floor', [
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
      cataloged('project-summary', 'feature-floor'),
      cataloged('repository-summary', 'feature-floor', ['list-item', 'row', 'text', 'badge']),
      cataloged('repository-status-popover', 'feature-floor', [
        'toolbar',
        'value-table',
        'list-item',
        'list-header',
        'ticket-code-review',
      ]),
      cataloged('change-evidence-dialog', 'feature-floor', ['toolbar', 'list-item', 'list-header']),
      cataloged('view-navigation', 'feature-floor', [
        'list',
        'list-header',
        'list-item',
        'list-action-row',
        'badge',
        'loading-spinner',
      ]),
      cataloged('command-navigation', 'feature-floor', ['list', 'list-header', 'list-item', 'loading-spinner']),
      cataloged('command-settings-editor', 'feature-floor', ['command-navigation', 'toolbar', 'list-header']),
      cataloged('drive-control', 'feature-floor'),
      cataloged('drive-options-menu', 'feature-floor'),
      cataloged('workspace-header', 'feature-floor', [
        'toolbar-text',
        'toolbar-control-group',
        'ticket-search-field',
        'ticket-list',
        'ticket-board',
        'notification-center',
      ]),
      cataloged('project-tab', 'feature-floor', ['app-tab']),
      cataloged('project-tabs', 'feature-floor', ['project-tab', 'select']),
      demo(
        'resizable-region',
        'ResizableRegion',
        'Accessible horizontal and vertical shell splitters.',
        'feature-floor',
      ),
      cataloged('connection-state-banner', 'feature-floor'),
      cataloged('connection-details-dialog', 'feature-floor', ['toolbar', 'value-table']),
      cataloged('settings-navigation', 'feature-floor', ['list', 'list-item', 'toolbar']),
      cataloged('notification-navigation', 'feature-floor', ['list', 'list-item', 'badge', 'toolbar']),
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
          cataloged('quick-ticket-composer', 'feature-floor', ['ticket-category-select', 'ticket-list']),
          cataloged('ticket-list', 'feature-floor', ['ticket-row', 'ticket-page-more']),
          cataloged('ticket-page-more', 'feature-floor'),
          cataloged('ticket-row', 'feature-floor', ['status-badge', 'confidence-badge', 'tag-chip', 'loading-spinner']),
        ],
      },
      {
        id: 'ticket-board',
        name: 'Columns',
        demos: [
          cataloged('ticket-board', 'feature-floor', ['ticket-board-column']),
          cataloged('ticket-board-column', 'feature-floor', ['ticket-row', 'ticket-page-more']),
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
          cataloged('ticket-inspector', 'feature-floor', [
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
          cataloged('ticket-inspector-skeleton', 'feature-floor'),
          cataloged('ticket-info-panel', 'feature-floor', [
            'ticket-category-select',
            'ticket-priority-select',
            'ticket-status-menu',
            'markdown-editor',
            'tag-chip',
            'list-header',
            'note-card',
            'note-composer',
          ]),
          cataloged('ticket-timeline', 'feature-floor'),
          cataloged('ticket-code-review', 'feature-floor'),
          cataloged('ticket-attachments', 'feature-floor', ['attachment-gallery']),
          cataloged('ticket-category-select', 'feature-floor', ['select']),
          cataloged('ticket-priority-select', 'feature-floor', ['select']),
          cataloged('ticket-status-menu', 'feature-floor', ['status-badge']),
          cataloged('status-badge', 'feature-floor'),
          cataloged('confidence-badge', 'feature-floor'),
          cataloged('confidence-calibration', 'feature-floor', ['confidence-badge']),
          cataloged('tag-chip', 'feature-floor'),
          cataloged('choice-card', 'feature-floor'),
        ],
      },
      {
        id: 'content',
        name: 'Content',
        demos: [
          cataloged('ticket-reader', 'feature-floor', ['ticket-inspector']),
          cataloged('markdown-editor', 'feature-floor'),
          cataloged('markdown-preview', 'feature-floor'),
          cataloged('attachment-gallery', 'feature-floor'),
          cataloged('ticket-close-dialog', 'feature-floor', ['select', 'list-item']),
          cataloged('not-working-dialog', 'feature-floor', ['pending-attachment-picker']),
          cataloged('bulk-ticket-dialog', 'feature-floor', ['surface-scaffold', 'list', 'text', 'row']),
          cataloged('saved-view-dialog', 'feature-floor', ['ticket-search-field']),
          cataloged('ticket-link-choice-dialog', 'feature-floor', [
            'surface-scaffold',
            'list',
            'text',
            'row',
            'list-item',
          ]),
          cataloged('project-dialog', 'feature-floor', ['list-item']),
          cataloged('project-close-dialog', 'feature-floor', [
            'state-banner',
            'list',
            'text',
            'row',
            'list-header',
            'list-item',
            'ai-conversation',
            'terminal-dashboard',
          ]),
          cataloged('conversation-export-dialog', 'feature-floor', ['content-transition', 'choice-card']),
          cataloged('command-run-dialog', 'feature-floor'),
        ],
      },
      {
        id: 'notes',
        name: 'Notes and activity',
        demos: [cataloged('note-composer', 'feature-floor'), cataloged('note-card', 'feature-floor')],
      },
    ],
  },
  {
    id: 'ai',
    name: 'AI and attention',
    demos: [
      cataloged('ai-conversation', 'feature-floor', ['permission-request', 'drive-control']),
      cataloged('ai-content-label', 'feature-floor'),
      cataloged('ai-feedback-dialog', 'feature-floor'),
      cataloged('ai-tool-settings', 'feature-floor', ['select']),
      cataloged('manual-model-dialog', 'feature-floor'),
      cataloged('halted-session-popup', 'feature-floor', ['list', 'row', 'text']),
      cataloged('permission-request', 'feature-floor'),
      cataloged('notification-center', 'feature-floor', ['permission-request']),
    ],
  },
  {
    id: 'terminal',
    name: 'Drawer and terminals',
    demos: [
      cataloged('terminal-drawer', 'feature-floor', [
        'app-tab',
        'list-item',
        'list-header',
        'ai-conversation',
        'terminal-key-bar',
      ]),
      cataloged('terminal-dashboard', 'feature-floor', [
        'fixed-aspect-terminal-card',
        'floating-toolbar',
        'toolbar-control-group',
        'list-item',
      ]),
      cataloged('terminal-operations-sidebar', 'feature-floor', ['project-summary', 'list', 'list-header', 'toolbar']),
      cataloged('terminal-ticket-rail', 'feature-floor', [
        'ticket-list',
        'ticket-inspector',
        'workspace-header',
        'quick-ticket-composer',
        'notification-center',
      ]),
      cataloged('fixed-aspect-terminal-card', 'feature-floor', ['terminal-key-bar']),
      cataloged('terminal-key-bar', 'feature-floor', ['toolbar-control-group']),
      cataloged('terminal-visibility-dialog', 'feature-floor', ['list-item']),
      cataloged('terminal-copy-dialog', 'feature-floor'),
      cataloged('terminal-paste-dialog', 'feature-floor'),
      cataloged('terminal-edit-menu', 'feature-floor'),
      cataloged('terminal-rename-dialog', 'feature-floor'),
    ],
  },
  {
    id: 'shared',
    name: 'Shared interactions',
    demos: [
      cataloged('app-empty-state', 'feature-floor'),
      cataloged('app-error', 'feature-floor', ['state-banner']),
      cataloged('content-transition', 'feature-floor'),
      demo('app-tab', 'AppTab', 'Shared project and terminal tab geometry, state, and close action.', 'feature-floor'),
      demo('select', 'Select', 'Compact icon-bearing single-value selection control.', 'feature-floor'),
      demo('toolbar', 'Toolbar', 'Shared leading, center, and trailing toolbar geometry.', 'feature-floor', [
        'toolbar-text',
        'toolbar-control-group',
      ]),
      demo(
        'floating-toolbar',
        'FloatingToolbar',
        'Forced-dark controls floating over positioned content; child groups inherit the dark scheme without an inverse tone.',
        'feature-floor',
        ['toolbar-control-group'],
      ),
      demo(
        'list',
        'List',
        'Vertical row layout with no gap, standard gap, and bounded scrolling with custom gap and edge dividers.',
        'feature-floor',
      ),
      demo(
        'list-item',
        'ListItem',
        'Aligned icon, label, trailing value, and selection geometry for list rows.',
        'feature-floor',
      ),
      demo(
        'list-header',
        'ListHeader',
        'Aligned section heading and optional action for list groups.',
        'feature-floor',
      ),
      demo(
        'toolbar-control-group',
        'ToolbarControlGroup',
        'Consistent rounded grouping for native toolbar buttons and Web Awesome popup triggers.',
        'feature-floor',
      ),
      demo(
        'toolbar-text',
        'ToolbarText',
        'Vertically aligned large, default, and small toolbar identity text.',
        'feature-floor',
      ),
      cataloged('ticket-search-field', 'feature-floor', ['toolbar-control-group']),
      demo('value-table', 'ValueTable', 'Shared static key/value rows with inset separators.', 'feature-floor'),
      cataloged('pending-attachment-picker', 'feature-floor'),
    ],
  },
  {
    id: 'setup',
    name: 'Setup and settings',
    demos: [
      cataloged('hs1-migration-dialog', 'feature-floor', ['value-table']),
      cataloged('hs1-migration-banner', 'feature-floor', ['shell-banner']),
      cataloged('codex-hooks-notice-banner', 'feature-floor', ['list', 'state-banner']),
      cataloged('shell-banner', 'feature-floor', ['state-banner']),
      cataloged('project-setup-warning-banner', 'feature-floor', ['shell-banner']),
      cataloged('notifications-paused-banner', 'feature-floor', ['shell-banner']),
      cataloged('provider-icon', 'feature-floor'),
      cataloged('ticket-source-setup-dialog', 'feature-floor', ['content-transition', 'list-item', 'provider-icon']),
      cataloged('provider-setup-form', 'feature-floor', ['provider-icon']),
      cataloged('ticket-sources-settings', 'feature-floor', ['list', 'list-item', 'provider-icon']),
      cataloged('accounts-settings', 'feature-floor'),
      cataloged('settings-workspace', 'feature-floor', ['ticket-sources-settings']),
      cataloged('keyboard-settings', 'feature-floor'),
      cataloged('trash-settings', 'feature-floor'),
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

const phaseLabel: Record<DemoPhase, string> = { 'feature-floor': 'Feature floor' };

function catalogEntry(item: DemoDefinition): CatalogEntry {
  const uses = (item.uses ?? []).map(findDemo).filter((demo): demo is DemoDefinition => Boolean(demo));
  const usedBy = demosUsing(item.id);
  return {
    id: item.id,
    name: item.name,
    description: item.description,
    tags: [phaseLabel[item.phase]],
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
