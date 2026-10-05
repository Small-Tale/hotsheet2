import '@kerfjs/ui/layout.css';

import { rem } from '@kerfjs/ui/css-values';
import { List } from '@kerfjs/ui/list';
import { ListHeader } from '@kerfjs/ui/list-header';
import { ListItem } from '@kerfjs/ui/list-item';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import {
  ArchiveRestore,
  Bot,
  CircleUserRound,
  Columns3,
  Database,
  Keyboard,
  ShieldCheck,
  SlidersHorizontal,
  TerminalSquare,
} from 'lucide';

import { SidebarPane, type SidebarPanelParts } from './sidebar-panel';

export type SettingsCategory =
  | 'sources'
  | 'ai'
  | 'commands'
  | 'lifecycle'
  | 'terminals'
  | 'permissions'
  | 'columns'
  | 'general'
  | 'accounts'
  | 'keyboard';

/** Project-scoped settings, followed by app-scoped (device-local) settings. */
const projectCategories = [
  { id: 'sources', label: 'Ticket sources', icon: Database, iconName: 'database' },
  { id: 'ai', label: 'AI tools', icon: Bot, iconName: 'bot' },
  { id: 'commands', label: 'Commands', icon: TerminalSquare, iconName: 'terminal-square' },
  { id: 'lifecycle', label: 'Lifecycle', icon: ArchiveRestore, iconName: 'archive-restore' },
  { id: 'terminals', label: 'Terminals', icon: TerminalSquare, iconName: 'terminal-square' },
  { id: 'permissions', label: 'Permissions', icon: ShieldCheck, iconName: 'shield-check' },
  { id: 'columns', label: 'Column view', icon: Columns3, iconName: 'columns-3' },
] as const;

const appCategories = [
  { id: 'general', label: 'General', icon: SlidersHorizontal, iconName: 'sliders-horizontal' },
  // Machine-wide provider sign-ins; ticket sources themselves belong to projects (HS2-SM9PM8).
  { id: 'accounts', label: 'Accounts', icon: CircleUserRound, iconName: 'circle-user-round' },
  { id: 'keyboard', label: 'Keyboard', icon: Keyboard, iconName: 'keyboard' },
] as const;

const allCategories = [...projectCategories, ...appCategories];

/**
 * Where a settings control stores its value: a server path as the web client requests it
 * (`{project}` marks the project id; the project bridge decides whether the server sees it
 * checkout-scoped), or a browser `localStorage` key.
 */
export type SettingsStorage =
  { control: string; server: string; browser?: never } | { control: string; browser: string; server?: never };

/**
 * Every settings category's controls and where each one stores its value (HS2-S1184P). A
 * project category may only store per project and an app category only machine- or
 * browser-wide; `settings-storage-scope.test.ts` resolves each entry through the project
 * bridge and fails when a setting is filed under the wrong group. Add a control's storage
 * here when you add the control.
 */
export const SETTINGS_STORAGE: Record<SettingsCategory, readonly SettingsStorage[]> = {
  sources: [
    { control: 'Linked sources list', server: '/providers' },
    { control: 'Own, edit, disable, and remove a source', server: '/provider-connections' },
    { control: 'Remove a source from this project', server: '/checkouts/{project}/sources' },
    { control: 'Default source', server: '/checkouts/{project}/default-source' },
  ],
  ai: [{ control: 'Default tool, model, and effort', server: '/ai-settings' }],
  commands: [
    { control: 'Command definitions', server: '/commands' },
    { control: 'Command groups', server: '/command-groups' },
  ],
  lifecycle: [{ control: 'Trash retention', server: '/checkouts/{project}/trash-settings' }],
  terminals: [{ control: 'Use global shell history', server: '/terminal-settings' }],
  permissions: [
    { control: 'Automatic decision and delay', browser: 'hotsheet.project.{project}.permission-automation' },
  ],
  columns: [{ control: 'Hide Verified column', browser: 'hotsheet.project.{project}.hide-verified-column' }],
  general: [{ control: 'Show loading activity', browser: 'hotsheet.show-loading-activity' }],
  accounts: [{ control: 'Sign-ins and sign out', server: '/accounts' }],
  keyboard: [{ control: 'Keyboard shortcut overrides', browser: 'hotsheet.keyboard-shortcuts' }],
};

/** Whether a settings category is app-scoped (device-local) rather than project-scoped. */
export function isAppSettingsCategory(category: SettingsCategory): boolean {
  return appCategories.some((item) => item.id === category);
}

export function settingsCategoryTitle(category: SettingsCategory): string {
  const item = allCategories.find((entry) => entry.id === category);
  if (!item) return 'Settings';
  return category === 'keyboard' ? 'Keyboard shortcuts' : item.label;
}

/** The settings navigator's panel parts for the Workbench's left rail (HS2-RWGQWN). */
export function settingsNavigationPanel({ selected }: { selected: SettingsCategory }): SidebarPanelParts {
  const renderGroup = (heading: string, items: readonly (typeof allCategories)[number][]) => (
    <section>
      <ListHeader label={heading} />
      <nav aria-label={heading}>
        <List gap={rem(0.125)}>
          {items.map((item) => (
            <ListItem
              action="select-settings-category"
              itemId={item.id}
              selected={selected === item.id}
              icon={<LucideIcon icon={item.icon} name={item.iconName} />}
              label={item.label}
            />
          ))}
        </List>
      </nav>
    </section>
  );
  return {
    label: 'Settings categories',
    toolbar: { label: 'Settings sidebar toolbar', dividerSides: '' },
    toggle: { action: 'toggle-project-sidebar', name: 'settings sidebar' },
    content: (
      <List gap="m" controlInsets="tb">
        {renderGroup('Project Settings', projectCategories)}
        {renderGroup('App Settings', appCategories)}
      </List>
    ),
    pane: { contentElement: 'nav', contentLabel: 'Settings categories' },
  };
}

export function SettingsNavigation({
  selected,
  collapseControl = false,
}: {
  selected: SettingsCategory;
  collapseControl?: boolean;
}) {
  return (
    <SidebarPane
      parts={settingsNavigationPanel({ selected })}
      className="settings-navigation"
      collapseControl={collapseControl}
    />
  );
}
